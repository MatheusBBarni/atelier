//! Integration tests for the [`McpSupervisor`] (task_03): drive the real
//! supervisor actor over the in-repo fake stdio MCP server, exercising
//! round-trip dispatch, dead-server isolation, per-call timeout + kill, and
//! concurrent (non-head-of-line-blocking) calls.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use multiagent::config::{McpServerConfig, McpTransport};
use multiagent::mcp::McpSupervisor;
use serde_json::json;

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake-mcp-server");

fn fake_server_config(id: &str) -> McpServerConfig {
    McpServerConfig {
        id: id.to_string(),
        transport: McpTransport::Stdio,
        command: Some(FAKE_SERVER.to_string()),
        args: Vec::new(),
        env: BTreeMap::new(),
        url: None,
    }
}

#[tokio::test]
async fn supervisor_round_trips_call_tool_through_handle() {
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));

    let result = handle
        .call_tool("fake", "effect_tool", json!({ "echo": "supervised" }))
        .await
        .expect("call_tool through the supervisor");
    assert!(!result.is_error);
    assert!(
        result.content.to_string().contains("supervised"),
        "effect_tool should echo via the supervisor: {}",
        result.content
    );

    // The snapshot reflects the connected server's four tools.
    let catalog = handle.snapshot_catalog().await.expect("snapshot");
    assert_eq!(catalog.servers.len(), 1);
    assert_eq!(catalog.servers[0].server, "fake");
    assert_eq!(catalog.servers[0].tools.len(), 4);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn dead_server_fails_its_next_call_without_affecting_others() {
    let handle = McpSupervisor::spawn(
        vec![fake_server_config("dying"), fake_server_config("healthy")],
        Duration::from_secs(3),
    );

    // The dying server replies, then exits shortly after.
    let first = handle
        .call_tool("dying", "effect_tool", json!({ "exit_after_ms": 50 }))
        .await;
    assert!(first.is_ok(), "first call returns before the server exits");

    // Give the server process time to exit.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // The next call to the dead server fails fast — it does not hang.
    let second = handle
        .call_tool("dying", "effect_tool", json!({ "echo": "x" }))
        .await;
    assert!(second.is_err(), "a call to a dead server must fail");

    // The other server is unaffected.
    let healthy = handle
        .call_tool("healthy", "effect_tool", json!({ "echo": "ok" }))
        .await
        .expect("healthy server still works");
    assert!(healthy.content.to_string().contains("ok"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn slow_call_times_out_and_evicts_the_connection() {
    // Per-call timeout is 300ms; the tool sleeps 60s.
    let handle = McpSupervisor::spawn(vec![fake_server_config("slow")], Duration::from_millis(300));

    let error = handle
        .call_tool("slow", "effect_tool", json!({ "sleep_ms": 60_000 }))
        .await
        .expect_err("a call exceeding the per-call timeout must error");
    assert!(
        format!("{error:#}").contains("timed out"),
        "expected a timeout error, got: {error:#}"
    );

    // The timed-out connection is cancelled and evicted (its child killed); a
    // subsequent call to the same server fails rather than reusing a dead conn.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let second = handle
        .call_tool("slow", "effect_tool", json!({ "echo": "x" }))
        .await;
    assert!(
        second.is_err(),
        "the evicted server must no longer be callable"
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn concurrent_calls_to_same_server_do_not_block() {
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(10));
    let h1 = handle.clone();
    let h2 = handle.clone();

    // Warm up so the one-time connect (subprocess spawn + `initialize`
    // handshake) is not counted against the concurrency measurement.
    handle.snapshot_catalog().await.expect("warmup connect");

    let start = Instant::now();
    let (a, b) = tokio::join!(
        h1.call_tool("fake", "effect_tool", json!({ "sleep_ms": 500 })),
        h2.call_tool("fake", "effect_tool", json!({ "sleep_ms": 500 })),
    );
    let elapsed = start.elapsed();

    assert!(
        a.is_ok() && b.is_ok(),
        "both concurrent calls should succeed"
    );
    // Serial execution would take ~1000ms; concurrent ~500ms. A generous ceiling
    // proves the supervisor (and server) did not serialize them.
    assert!(
        elapsed < Duration::from_millis(900),
        "two overlapping 500ms calls should not serialize; took {elapsed:?}"
    );

    handle.shutdown().await.ok();
}
