//! The [`McpClient`] seam and its real [`rmcp`]-backed stdio implementation.
//!
//! [`McpClient`] is the only surface the rest of atelier depends on: the
//! supervisor (task_03), action handlers (task_05), unit mocks, and the in-repo
//! fake server (ADR-008) all talk to this trait, never to `rmcp` directly. That
//! keeps the pre-1.0-shaped SDK churn localized to one adapter (ADR-004).

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use tokio::process::Command;

/// A tool advertised by an MCP server.
///
/// Deliberately a plain, SDK-agnostic value: `input_schema` and `annotations`
/// are raw JSON so nothing past this seam binds to `rmcp`'s typed models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

/// The result of a tool call, normalized for the harness action contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: Value,
    pub is_error: bool,
}

/// The contents of a resource read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub contents: Value,
}

/// The internal seam over an MCP connection.
///
/// `rmcp` types never appear in these signatures so the SDK stays swappable and
/// the trait is trivially mockable in unit tests.
#[async_trait]
pub trait McpClient: Send + Sync {
    /// List every tool the server advertises (pagination handled internally).
    async fn list_tools(&self) -> Result<Vec<McpTool>>;

    /// Invoke `tool` with the given JSON arguments (a JSON object or null).
    async fn call_tool(&self, tool: &str, args: Value) -> Result<McpToolResult>;

    /// Read a resource by URI.
    async fn read_resource(&self, uri: &str) -> Result<McpResource>;

    /// Best-effort shutdown of the underlying connection (ADR-004).
    ///
    /// Defaults to a no-op so mocks need not implement it; the real client
    /// cancels its running service. The supervisor (task_03) owns the full
    /// lifecycle.
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// The real [`McpClient`], adapting an `rmcp` client over a spawned stdio child
/// process. Mirrors the subprocess shape used by the Cursor runtime.
pub struct RmcpClient {
    service: RunningService<RoleClient, ()>,
}

impl RmcpClient {
    /// Spawn `command` (with `args` and `env`) as a stdio MCP server and run the
    /// `initialize` handshake. Returns a descriptive error — never a panic — if
    /// the process cannot be spawned or the handshake fails.
    pub async fn connect_stdio(
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<Self> {
        let args = args.to_vec();
        let env = env.to_vec();
        let transport = TokioChildProcess::new(Command::new(command).configure(|cmd| {
            cmd.args(&args);
            for (key, value) in &env {
                cmd.env(key, value);
            }
        }))
        .with_context(|| format!("failed to spawn MCP server process `{command}`"))?;
        let service = ()
            .serve(transport)
            .await
            .with_context(|| format!("MCP `initialize` handshake failed for `{command}`"))?;
        Ok(Self { service })
    }

    /// Wrap an already-connected client service. Used by in-process tests that
    /// drive the fake server over a duplex transport instead of a subprocess.
    #[cfg(test)]
    pub(crate) fn from_running_service(service: RunningService<RoleClient, ()>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl McpClient for RmcpClient {
    async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .context("failed to list MCP tools")?;
        tools.into_iter().map(map_tool).collect()
    }

    async fn call_tool(&self, tool: &str, args: Value) -> Result<McpToolResult> {
        let arguments = match args {
            Value::Null => None,
            Value::Object(map) => Some(map),
            other => {
                return Err(anyhow!(
                    "MCP tool `{tool}` arguments must be a JSON object or null, got {}",
                    value_kind(&other)
                ))
            }
        };
        let mut params = CallToolRequestParams::new(tool.to_string());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
        let result = self
            .service
            .call_tool(params)
            .await
            .with_context(|| format!("MCP tool call `{tool}` failed"))?;
        Ok(McpToolResult {
            content: serde_json::to_value(&result.content)
                .context("failed to encode MCP tool result content")?,
            is_error: result.is_error.unwrap_or(false),
        })
    }

    async fn read_resource(&self, uri: &str) -> Result<McpResource> {
        let result = self
            .service
            .read_resource(ReadResourceRequestParams::new(uri.to_string()))
            .await
            .with_context(|| format!("failed to read MCP resource `{uri}`"))?;
        Ok(McpResource {
            uri: uri.to_string(),
            contents: serde_json::to_value(&result.contents)
                .context("failed to encode MCP resource contents")?,
        })
    }

    async fn shutdown(&self) -> Result<()> {
        self.service.cancellation_token().cancel();
        Ok(())
    }
}

/// Convert an `rmcp` [`Tool`](rmcp::model::Tool) into our SDK-agnostic [`McpTool`].
fn map_tool(tool: rmcp::model::Tool) -> Result<McpTool> {
    let input_schema =
        serde_json::to_value(&*tool.input_schema).unwrap_or_else(|_| Value::Object(Map::new()));
    let annotations = match tool.annotations {
        Some(annotations) => Some(
            serde_json::to_value(annotations).context("failed to encode MCP tool annotations")?,
        ),
        None => None,
    };
    Ok(McpTool {
        name: tool.name.to_string(),
        description: tool.description.map(|description| description.to_string()),
        input_schema,
        annotations,
    })
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::fake_server::{FakeMcpServer, FIXTURE_RESOURCE_TEXT, FIXTURE_RESOURCE_URI};
    use serde_json::json;

    /// A trivial mock implementing the [`McpClient`] seam without any subprocess
    /// — proves the trait is mockable and returns the four fixture tools.
    struct MockMcpClient {
        tools: Vec<McpTool>,
    }

    impl MockMcpClient {
        fn with_fixture_tools() -> Self {
            let names = [
                "resource_read",
                "read_only_tool",
                "effect_tool",
                "mutating_tool",
            ];
            let tools = names
                .iter()
                .map(|name| McpTool {
                    name: (*name).to_string(),
                    description: Some(format!("fixture {name}")),
                    input_schema: json!({ "type": "object" }),
                    annotations: None,
                })
                .collect();
            Self { tools }
        }
    }

    #[async_trait]
    impl McpClient for MockMcpClient {
        async fn list_tools(&self) -> Result<Vec<McpTool>> {
            Ok(self.tools.clone())
        }
        async fn call_tool(&self, _tool: &str, _args: Value) -> Result<McpToolResult> {
            Ok(McpToolResult {
                content: json!([]),
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

    #[tokio::test]
    async fn mock_client_returns_four_fixture_tools() {
        let client = MockMcpClient::with_fixture_tools();
        let tools = client.list_tools().await.expect("list_tools");
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "resource_read",
                "read_only_tool",
                "effect_tool",
                "mutating_tool"
            ]
        );
    }

    #[tokio::test]
    async fn connect_stdio_errors_descriptively_on_missing_command() {
        let result =
            RmcpClient::connect_stdio("atelier-definitely-not-a-real-mcp-binary-xyz", &[], &[])
                .await;
        let Err(error) = result else {
            panic!("spawning a nonexistent command must fail");
        };
        let message = format!("{error:#}");
        assert!(
            message.contains("atelier-definitely-not-a-real-mcp-binary-xyz"),
            "error should name the failed command, got: {message}"
        );
    }

    /// Connect a real `rmcp` client to the in-repo fake server over an in-process
    /// duplex transport (no subprocess) and read the fixture resource through the
    /// [`McpClient`] seam.
    #[tokio::test]
    async fn read_resource_returns_fixture_content_over_duplex() {
        let (server_io, client_io) = tokio::io::duplex(8 * 1024);
        let server_task =
            tokio::spawn(async move { FakeMcpServer::default().serve(server_io).await });
        let service = ().serve(client_io).await.expect("client handshake over duplex");
        let _server = server_task
            .await
            .expect("server task join")
            .expect("server handshake over duplex");
        let client = RmcpClient::from_running_service(service);

        let resource = client
            .read_resource(FIXTURE_RESOURCE_URI)
            .await
            .expect("read fixture resource");
        assert_eq!(resource.uri, FIXTURE_RESOURCE_URI);
        let rendered = resource.contents.to_string();
        assert!(
            rendered.contains(FIXTURE_RESOURCE_TEXT),
            "resource contents should carry the fixture text, got: {rendered}"
        );
    }
}
