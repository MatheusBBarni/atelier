//! The [`McpSupervisor`] actor and its cloneable [`McpHandle`] (ADR-005).
//!
//! The supervisor is the single owner of every long-lived MCP connection. It
//! spawns and supervises one [`McpClient`] per configured stdio server and keeps
//! all async/subprocess state out of `App`'s deterministic, event-sourced core.
//! Callers reach it only through [`McpHandle`], a cheap clone of an mpsc command
//! sender.
//!
//! **Concurrency:** the actor owns the connection map and lifecycle, but each
//! tool call / resource read is dispatched onto a spawned task (rmcp clients are
//! `Send + Sync` and self-correlate), so one slow call never blocks the actor or
//! other calls — there is no global call mutex (ADR-005).
//!
//! **Liveness:** every call is wrapped in a per-call timeout. On timeout the
//! connection is cancelled and evicted from the map; dropping the last `Arc`
//! tears down rmcp's `TokioChildProcess`, whose `Drop` kills the child process
//! (`kill_on_drop` backstop). A hung or dead server therefore fails its own
//! calls quickly without ever hanging the supervisor or other servers.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use crate::config::{McpServerConfig, McpTransport};
use crate::mcp::client::{McpClient, McpResource, McpTool, McpToolResult, RmcpClient};

/// Default per-call timeout. A hung server call fails rather than blocking the
/// run; the connection is then evicted and its child killed.
pub const DEFAULT_MCP_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Bounded command channel depth (ADR-005 backpressure).
const COMMAND_CHANNEL_DEPTH: usize = 64;

/// Immutable snapshot of every connected server's advertised tools (ADR-001/F5).
/// Recorded as an event and read by the prompt builder — never the live handle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolCatalog {
    pub servers: Vec<ToolCatalogServer>,
}

/// One server's slice of the [`ToolCatalog`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCatalogServer {
    pub server: String,
    pub tools: Vec<McpTool>,
}

/// Commands the supervisor accepts. `pub(crate)`: callers use [`McpHandle`].
pub(crate) enum McpCommand {
    CallTool {
        server: String,
        tool: String,
        args: Value,
        reply: oneshot::Sender<Result<McpToolResult>>,
    },
    ReadResource {
        server: String,
        uri: String,
        reply: oneshot::Sender<Result<McpResource>>,
    },
    SnapshotCatalog {
        reply: oneshot::Sender<ToolCatalog>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
    /// Internal: a dispatch task asks the actor to drop a connection it found
    /// dead (timed out). Removing the map's `Arc` lets the connection — and its
    /// child process — be torn down. Not exposed on [`McpHandle`].
    EvictConnection {
        server: String,
    },
}

/// A cheap, cloneable handle to the supervisor (ADR-005). Carried on
/// `ActionExecutionContext` (task_05) and held by `App` for ownership/shutdown.
#[derive(Clone, Debug)]
pub struct McpHandle {
    tx: mpsc::Sender<McpCommand>,
}

impl McpHandle {
    /// Invoke `tool` on `server`. Errors (descriptively) if the supervisor is
    /// gone, the server is unknown, or the call fails/times out — never hangs.
    pub async fn call_tool(&self, server: &str, tool: &str, args: Value) -> Result<McpToolResult> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(McpCommand::CallTool {
                server: server.to_string(),
                tool: tool.to_string(),
                args,
                reply,
            })
            .await
            .map_err(|_| anyhow!("MCP supervisor is unavailable"))?;
        rx.await
            .map_err(|_| anyhow!("MCP supervisor dropped the reply"))?
    }

    /// Read `uri` from `server`.
    pub async fn read_resource(&self, server: &str, uri: &str) -> Result<McpResource> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(McpCommand::ReadResource {
                server: server.to_string(),
                uri: uri.to_string(),
                reply,
            })
            .await
            .map_err(|_| anyhow!("MCP supervisor is unavailable"))?;
        rx.await
            .map_err(|_| anyhow!("MCP supervisor dropped the reply"))?
    }

    /// Take an immutable snapshot of every connected server's tools.
    pub async fn snapshot_catalog(&self) -> Result<ToolCatalog> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(McpCommand::SnapshotCatalog { reply })
            .await
            .map_err(|_| anyhow!("MCP supervisor is unavailable"))?;
        rx.await
            .map_err(|_| anyhow!("MCP supervisor dropped the reply"))
    }

    /// Drain and shut down every connection, killing child processes. Replies
    /// once teardown is complete.
    pub async fn shutdown(&self) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(McpCommand::Shutdown { reply })
            .await
            .map_err(|_| anyhow!("MCP supervisor is unavailable"))?;
        rx.await
            .map_err(|_| anyhow!("MCP supervisor dropped the reply"))
    }
}

/// Owns the connection lifecycle. Construct via [`McpSupervisor::spawn`], which
/// returns the [`McpHandle`]; the actor runs on its own task.
pub struct McpSupervisor;

impl McpSupervisor {
    /// Spawn the supervisor for the given stdio servers. Returns immediately with
    /// a handle; connections are established on the actor task (async connect,
    /// ADR-002). Servers that fail to connect are skipped (their calls then error
    /// with "unknown server"); event-recording the failure is a later task.
    pub fn spawn(servers: Vec<McpServerConfig>, per_call_timeout: Duration) -> McpHandle {
        let (tx, rx) = mpsc::channel(COMMAND_CHANNEL_DEPTH);
        let self_tx = tx.clone();
        tokio::spawn(async move {
            let connections = connect_all(servers).await;
            run_actor(connections, per_call_timeout, rx, self_tx).await;
        });
        McpHandle { tx }
    }

    /// Spawn the actor over pre-built (mock) connections — the unit-test seam, so
    /// supervisor logic is exercised without spawning subprocesses.
    #[cfg(test)]
    pub(crate) fn spawn_with_connections(
        connections: BTreeMap<String, Arc<dyn McpClient>>,
        per_call_timeout: Duration,
    ) -> McpHandle {
        let (tx, rx) = mpsc::channel(COMMAND_CHANNEL_DEPTH);
        let self_tx = tx.clone();
        tokio::spawn(async move {
            run_actor(connections, per_call_timeout, rx, self_tx).await;
        });
        McpHandle { tx }
    }
}

/// Connect to every stdio server (sequentially; each `connect` runs the
/// `initialize` handshake). V1 wires `Stdio` only (ADR-002).
async fn connect_all(servers: Vec<McpServerConfig>) -> BTreeMap<String, Arc<dyn McpClient>> {
    let mut connections: BTreeMap<String, Arc<dyn McpClient>> = BTreeMap::new();
    for server in servers {
        if server.transport != McpTransport::Stdio {
            continue;
        }
        let Some(command) = server.command.as_deref() else {
            continue;
        };
        let env: Vec<(String, String)> = server
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        match RmcpClient::connect_stdio(command, &server.args, &env).await {
            Ok(client) => {
                connections.insert(server.id.clone(), Arc::new(client));
            }
            Err(_error) => {
                // Server unavailable: skip it. Its calls will return a clear
                // "unknown server" error. Event-recording is a later task.
            }
        }
    }
    connections
}

/// The actor loop. Owns `connections`; dispatches calls onto spawned tasks so a
/// slow/dead server never blocks the loop or other servers.
async fn run_actor(
    mut connections: BTreeMap<String, Arc<dyn McpClient>>,
    per_call_timeout: Duration,
    mut rx: mpsc::Receiver<McpCommand>,
    self_tx: mpsc::Sender<McpCommand>,
) {
    while let Some(command) = rx.recv().await {
        match command {
            McpCommand::CallTool {
                server,
                tool,
                args,
                reply,
            } => match connections.get(&server) {
                Some(client) => {
                    let client = Arc::clone(client);
                    let evict_tx = self_tx.clone();
                    tokio::spawn(async move {
                        let result = dispatch_call(
                            client,
                            &server,
                            &tool,
                            args,
                            per_call_timeout,
                            &evict_tx,
                        )
                        .await;
                        let _ = reply.send(result);
                    });
                }
                None => {
                    let _ = reply.send(Err(unknown_server_error(&server, &connections)));
                }
            },
            McpCommand::ReadResource { server, uri, reply } => match connections.get(&server) {
                Some(client) => {
                    let client = Arc::clone(client);
                    let evict_tx = self_tx.clone();
                    tokio::spawn(async move {
                        let result =
                            dispatch_read(client, &server, &uri, per_call_timeout, &evict_tx).await;
                        let _ = reply.send(result);
                    });
                }
                None => {
                    let _ = reply.send(Err(unknown_server_error(&server, &connections)));
                }
            },
            McpCommand::SnapshotCatalog { reply } => {
                let connections = connections.clone();
                tokio::spawn(async move {
                    let catalog = snapshot_catalog(&connections, per_call_timeout).await;
                    let _ = reply.send(catalog);
                });
            }
            McpCommand::EvictConnection { server } => {
                // The connection timed out; drop the map's Arc so the dead
                // connection (and its child) can be torn down.
                connections.remove(&server);
            }
            McpCommand::Shutdown { reply } => {
                for client in connections.values() {
                    let _ = client.shutdown().await;
                }
                connections.clear();
                let _ = reply.send(());
                break;
            }
        }
    }
}

/// Run one tool call under the per-call timeout. On timeout, cancel the
/// connection and ask the actor to evict it (so its child is killed).
async fn dispatch_call(
    client: Arc<dyn McpClient>,
    server: &str,
    tool: &str,
    args: Value,
    per_call_timeout: Duration,
    evict_tx: &mpsc::Sender<McpCommand>,
) -> Result<McpToolResult> {
    match tokio::time::timeout(per_call_timeout, client.call_tool(tool, args)).await {
        Ok(result) => result,
        Err(_elapsed) => {
            let _ = client.shutdown().await;
            let _ = evict_tx
                .send(McpCommand::EvictConnection {
                    server: server.to_string(),
                })
                .await;
            Err(anyhow!(
                "MCP tool call `{tool}` on server `{server}` timed out after {per_call_timeout:?}"
            ))
        }
    }
}

/// Run one resource read under the per-call timeout (same eviction-on-timeout).
async fn dispatch_read(
    client: Arc<dyn McpClient>,
    server: &str,
    uri: &str,
    per_call_timeout: Duration,
    evict_tx: &mpsc::Sender<McpCommand>,
) -> Result<McpResource> {
    match tokio::time::timeout(per_call_timeout, client.read_resource(uri)).await {
        Ok(result) => result,
        Err(_elapsed) => {
            let _ = client.shutdown().await;
            let _ = evict_tx
                .send(McpCommand::EvictConnection {
                    server: server.to_string(),
                })
                .await;
            Err(anyhow!(
                "MCP resource read `{uri}` on server `{server}` timed out after {per_call_timeout:?}"
            ))
        }
    }
}

/// Gather every connection's tools concurrently into an immutable snapshot.
async fn snapshot_catalog(
    connections: &BTreeMap<String, Arc<dyn McpClient>>,
    per_call_timeout: Duration,
) -> ToolCatalog {
    let mut set: JoinSet<ToolCatalogServer> = JoinSet::new();
    for (name, client) in connections {
        let name = name.clone();
        let client = Arc::clone(client);
        set.spawn(async move {
            let tools = match tokio::time::timeout(per_call_timeout, client.list_tools()).await {
                Ok(Ok(tools)) => tools,
                // A failing/slow server contributes no tools rather than failing
                // the whole snapshot.
                _ => Vec::new(),
            };
            ToolCatalogServer {
                server: name,
                tools,
            }
        });
    }
    let mut servers = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(server) = joined {
            servers.push(server);
        }
    }
    servers.sort_by(|a, b| a.server.cmp(&b.server));
    ToolCatalog { servers }
}

fn unknown_server_error(
    server: &str,
    connections: &BTreeMap<String, Arc<dyn McpClient>>,
) -> anyhow::Error {
    let known: Vec<&str> = connections.keys().map(String::as_str).collect();
    anyhow!(
        "unknown MCP server `{server}` (connected servers: {})",
        if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    /// A configurable in-memory client for supervisor unit tests (no subprocess).
    struct StubClient {
        tools: Vec<McpTool>,
    }

    impl StubClient {
        fn with_tool(name: &str) -> Self {
            Self {
                tools: vec![McpTool {
                    name: name.to_string(),
                    description: None,
                    input_schema: json!({ "type": "object" }),
                    annotations: None,
                }],
            }
        }
    }

    #[async_trait]
    impl McpClient for StubClient {
        async fn list_tools(&self) -> Result<Vec<McpTool>> {
            Ok(self.tools.clone())
        }
        async fn call_tool(&self, tool: &str, args: Value) -> Result<McpToolResult> {
            Ok(McpToolResult {
                content: json!({ "tool": tool, "args": args }),
                is_error: false,
            })
        }
        async fn read_resource(&self, uri: &str) -> Result<McpResource> {
            Ok(McpResource {
                uri: uri.to_string(),
                contents: json!([]),
            })
        }
    }

    fn connections(pairs: Vec<(&str, Arc<dyn McpClient>)>) -> BTreeMap<String, Arc<dyn McpClient>> {
        pairs
            .into_iter()
            .map(|(id, client)| (id.to_string(), client))
            .collect()
    }

    #[tokio::test]
    async fn snapshot_catalog_includes_all_servers_tools() {
        let conns = connections(vec![
            (
                "alpha",
                Arc::new(StubClient::with_tool("a_tool")) as Arc<dyn McpClient>,
            ),
            (
                "beta",
                Arc::new(StubClient::with_tool("b_tool")) as Arc<dyn McpClient>,
            ),
        ]);
        let handle = McpSupervisor::spawn_with_connections(conns, DEFAULT_MCP_CALL_TIMEOUT);

        let catalog = handle.snapshot_catalog().await.expect("snapshot");
        let names: Vec<&str> = catalog.servers.iter().map(|s| s.server.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert_eq!(catalog.servers[0].tools[0].name, "a_tool");
        assert_eq!(catalog.servers[1].tools[0].name, "b_tool");
    }

    #[tokio::test]
    async fn call_tool_unknown_server_errors_without_hanging() {
        let conns = connections(vec![(
            "alpha",
            Arc::new(StubClient::with_tool("a_tool")) as Arc<dyn McpClient>,
        )]);
        let handle = McpSupervisor::spawn_with_connections(conns, DEFAULT_MCP_CALL_TIMEOUT);

        let error = handle
            .call_tool("missing", "whatever", json!({}))
            .await
            .expect_err("unknown server must error");
        let message = format!("{error:#}");
        assert!(
            message.contains("missing"),
            "error should name the unknown server: {message}"
        );
    }

    #[tokio::test]
    async fn call_tool_round_trips_through_stub() {
        let conns = connections(vec![(
            "alpha",
            Arc::new(StubClient::with_tool("a_tool")) as Arc<dyn McpClient>,
        )]);
        let handle = McpSupervisor::spawn_with_connections(conns, DEFAULT_MCP_CALL_TIMEOUT);

        let result = handle
            .call_tool("alpha", "a_tool", json!({ "x": 1 }))
            .await
            .expect("call");
        assert!(!result.is_error);
        assert_eq!(result.content["tool"], "a_tool");
    }

    #[tokio::test]
    async fn shutdown_reports_completion_and_stops_the_actor() {
        let conns = connections(vec![(
            "alpha",
            Arc::new(StubClient::with_tool("a_tool")) as Arc<dyn McpClient>,
        )]);
        let handle = McpSupervisor::spawn_with_connections(conns, DEFAULT_MCP_CALL_TIMEOUT);

        handle.shutdown().await.expect("shutdown completes");

        // After shutdown the actor has stopped: further commands fail fast rather
        // than hanging.
        let error = handle.snapshot_catalog().await;
        assert!(
            error.is_err(),
            "commands after shutdown should fail, not hang"
        );
    }
}
