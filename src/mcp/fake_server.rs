//! An in-repo fake stdio MCP server for deterministic tests (ADR-008).
//!
//! It implements the same wire protocol a real stdio server would, exposing a
//! fixed set of fixtures so tests exercise the real `initialize` handshake and
//! JSON-RPC framing without depending on a published server or the network:
//!
//! - one read-only **resource** ([`FIXTURE_RESOURCE_URI`]);
//! - a `resource_read` tool returning that resource's text;
//! - a `read_only_tool` carrying a `readOnlyHint` annotation;
//! - an `effect_tool` that echoes its arguments (round-trip coverage);
//! - a `mutating_tool` whose description changes on every `tools/list` call, so
//!   later tasks can exercise the F6 diff-on-change re-prompt.
//!
//! The handler is written by hand (rather than via the `#[tool]` macro) so the
//! annotations and the mutating description are fully under test control.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rmcp::model::{
    AnnotateAble, CallToolRequestParams, CallToolResult, Content, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, RawResource, ReadResourceRequestParams,
    ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Map, Value};

/// URI of the single read-only fixture resource.
pub const FIXTURE_RESOURCE_URI: &str = "mem://atelier/fixture/readme";
/// Text returned when reading [`FIXTURE_RESOURCE_URI`].
pub const FIXTURE_RESOURCE_TEXT: &str = "atelier fake MCP fixture resource contents";

/// The fixtures' tool names, in advertised order.
pub const FIXTURE_TOOL_NAMES: [&str; 4] = [
    "resource_read",
    "read_only_tool",
    "effect_tool",
    "mutating_tool",
];

/// A deterministic in-process MCP server used only by tests.
#[derive(Clone, Default)]
pub struct FakeMcpServer {
    /// Counts `tools/list` calls so `mutating_tool`'s description changes.
    list_calls: Arc<AtomicU64>,
}

impl FakeMcpServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pure fixture lookup: the text served for `uri`, or `None`.
    ///
    /// Lets tests assert the fixture content without spawning a connection.
    pub fn fixture_resource_text(uri: &str) -> Option<&'static str> {
        if uri == FIXTURE_RESOURCE_URI {
            Some(FIXTURE_RESOURCE_TEXT)
        } else {
            None
        }
    }
}

/// Build a minimal JSON-Schema `object` with the given `properties`.
fn object_schema(properties: Value) -> Arc<Map<String, Value>> {
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), properties);
    Arc::new(schema)
}

impl ServerHandler for FakeMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let revision = self.list_calls.fetch_add(1, Ordering::SeqCst) + 1;
        let tools = vec![
            Tool::new(
                "resource_read",
                "Returns the fixture resource contents",
                object_schema(json!({})),
            ),
            Tool::new(
                "read_only_tool",
                "A read-only fixture tool",
                object_schema(json!({})),
            )
            .annotate(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "effect_tool",
                "Echoes its arguments back to the caller",
                object_schema(json!({ "echo": { "type": "string" } })),
            ),
            Tool::new(
                "mutating_tool",
                format!("Mutating fixture tool (revision {revision})"),
                object_schema(json!({})),
            ),
        ];
        let mut tools = tools;
        // High-tool-count mode for the emission spike (task_11): pad with N
        // synthetic tools so the prompt advertises a large catalog.
        if let Ok(extra) = std::env::var("FAKE_MCP_TOOL_COUNT") {
            if let Ok(count) = extra.parse::<usize>() {
                for index in 0..count {
                    tools.push(Tool::new(
                        format!("spike_tool_{index}"),
                        format!("Synthetic spike tool #{index}"),
                        object_schema(json!({ "n": { "type": "integer" } })),
                    ));
                }
            }
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let arguments = request.arguments.clone().unwrap_or_default();
        match request.name.as_ref() {
            "resource_read" => Ok(CallToolResult::success(vec![Content::text(
                FIXTURE_RESOURCE_TEXT,
            )])),
            "read_only_tool" => Ok(CallToolResult::success(vec![Content::text(
                "read-only fixture tool result",
            )])),
            "effect_tool" => {
                // Opt-in side behaviors used by the supervisor's liveness tests
                // (task_03). Plain calls just echo their arguments, so the
                // task_01 contract is unchanged.
                if let Some(ms) = arguments.get("sleep_ms").and_then(Value::as_u64) {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                }
                if let Some(ms) = arguments.get("exit_after_ms").and_then(Value::as_u64) {
                    // Schedule this server process to exit shortly, simulating a
                    // server that dies mid-session. The current call still
                    // returns normally.
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                        std::process::exit(0);
                    });
                }
                let echoed = Value::Object(arguments).to_string();
                Ok(CallToolResult::success(vec![Content::text(echoed)]))
            }
            "mutating_tool" => Ok(CallToolResult::success(vec![Content::text(
                "mutating fixture tool result",
            )])),
            other => Err(McpError::invalid_params(
                format!("unknown tool: {other}"),
                None,
            )),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(vec![RawResource::new(
            FIXTURE_RESOURCE_URI,
            "fixture-readme",
        )
        .no_annotation()]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        match FakeMcpServer::fixture_resource_text(&request.uri) {
            Some(text) => Ok(ReadResourceResult::new(vec![ResourceContents::text(
                text,
                request.uri.clone(),
            )])),
            None => Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({ "uri": request.uri })),
            )),
        }
    }
}

/// Serve the fake server over real stdio until the peer disconnects.
///
/// This is the entry point used by the `fake-mcp-server` binary that
/// integration tests spawn as a subprocess.
pub async fn run_stdio() -> anyhow::Result<()> {
    // Opt-in startup noise on stderr (task_10): proves a healthy server that
    // logs to stderr is NOT a doctor false-failure — only the stdout handshake
    // matters.
    if std::env::var("FAKE_MCP_STDERR_NOISE").is_ok() {
        eprintln!("fake-mcp-server: starting up — this stderr line is harmless");
    }
    let service = FakeMcpServer::default()
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_resource_text_matches_only_the_fixture_uri() {
        assert_eq!(
            FakeMcpServer::fixture_resource_text(FIXTURE_RESOURCE_URI),
            Some(FIXTURE_RESOURCE_TEXT)
        );
        assert_eq!(FakeMcpServer::fixture_resource_text("mem://unknown"), None);
    }

    #[test]
    fn object_schema_is_a_typed_object() {
        let schema = object_schema(json!({ "echo": { "type": "string" } }));
        assert_eq!(schema.get("type"), Some(&Value::String("object".into())));
        assert!(schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some());
    }
}
