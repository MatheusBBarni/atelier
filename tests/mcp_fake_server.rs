//! Integration tests for the MCP client seam: spawn the in-repo fake stdio MCP
//! server (ADR-008) as a real subprocess and drive it through the real
//! [`RmcpClient`] over real stdio — exercising the `initialize` handshake and
//! JSON-RPC framing, not just a mock.

use multiagent::mcp::{McpClient, RmcpClient};
use serde_json::json;

/// Cargo builds the fixture binary for integration tests and exposes its path.
const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake-mcp-server");

async fn connect() -> RmcpClient {
    RmcpClient::connect_stdio(FAKE_SERVER, &[], &[])
        .await
        .expect("connect to the fake MCP server")
}

#[tokio::test]
async fn lists_exactly_the_four_fixture_tools() {
    let client = connect().await;
    let tools = client.list_tools().await.expect("list_tools");

    let mut names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
    names.sort();
    let mut expected = vec![
        "effect_tool",
        "mutating_tool",
        "read_only_tool",
        "resource_read",
    ];
    expected.sort();
    assert_eq!(
        names, expected,
        "fake server should expose exactly four tools"
    );

    let _ = client.shutdown().await;
}

#[tokio::test]
async fn effect_tool_round_trips_arguments_over_stdio() {
    let client = connect().await;
    let result = client
        .call_tool("effect_tool", json!({ "echo": "round-trip" }))
        .await
        .expect("call effect_tool");

    assert!(!result.is_error, "effect_tool should not report an error");
    let rendered = result.content.to_string();
    assert!(
        rendered.contains("round-trip"),
        "effect_tool should echo its arguments, got: {rendered}"
    );

    let _ = client.shutdown().await;
}

#[tokio::test]
async fn initialize_handshake_reaches_ready_state() {
    // `connect_stdio` only returns `Ok` once `serve` has completed the
    // `initialize` handshake; a subsequent call confirms a usable connection.
    let client = connect().await;
    let tools = client
        .list_tools()
        .await
        .expect("a ready connection serves list_tools");
    assert_eq!(tools.len(), 4);
    let _ = client.shutdown().await;
}

#[tokio::test]
async fn read_only_annotation_and_resource_read_round_trip() {
    let client = connect().await;

    let tools = client.list_tools().await.expect("list_tools");
    let read_only = tools
        .iter()
        .find(|tool| tool.name == "read_only_tool")
        .expect("read_only_tool present");
    let annotations = read_only
        .annotations
        .clone()
        .expect("read_only_tool carries annotations");
    assert_eq!(
        annotations.get("readOnlyHint").and_then(|v| v.as_bool()),
        Some(true),
        "read_only_tool should advertise readOnlyHint=true"
    );

    let resource = client
        .read_resource("mem://atelier/fixture/readme")
        .await
        .expect("read fixture resource");
    assert!(
        resource
            .contents
            .to_string()
            .contains("fixture resource contents"),
        "resource read should return the fixture text"
    );

    let _ = client.shutdown().await;
}
