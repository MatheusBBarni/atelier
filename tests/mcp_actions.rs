//! Integration test for the MCP action vertical (task_05): a full
//! validate → execute round-trip of a `CallMcpTool` action against the in-repo
//! fake stdio MCP server, exercised through the real action contract.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use multiagent::actions::{
    execute_action_request, validate_action_request_with_scope, ActionDecision,
    ActionExecutionContext, ActionKind, ActionRequest, ActionStatus, McpActionContext,
};
use multiagent::config::{
    load_effective_config, ApprovalMode, ConfigLoadOptions, McpServerConfig, McpTransport,
    WorkspacePolicy,
};
use multiagent::mcp::{McpSupervisor, McpTrustStore};
use serde_json::json;
use tempfile::tempdir;

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake-mcp-server");

/// Load a config that defines one MCP-capable agent (`caller`).
fn mcp_caller_agent() -> multiagent::config::AgentProfile {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("atelier.toml");
    std::fs::write(
        &config_path,
        r#"
[runtimes.fake]
type = "fake"

[agents.caller]
runtime = "fake"
capabilities = ["read", "mcp_tool"]
tools = ["call_mcp_tool", "read_mcp_resource", "list_mcp_resources"]
instructions = "MCP caller."
"#,
    )
    .unwrap();
    let config = load_effective_config(ConfigLoadOptions {
        working_directory: dir.path().to_path_buf(),
        config_path: Some(config_path),
    })
    .unwrap();
    config.agents.get("caller").unwrap().clone()
}

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
async fn call_mcp_tool_validates_and_executes_round_trip() {
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog snapshot");

    // Trust the server so the call validates as Allowed (untrusted would prompt).
    let trust_dir = tempdir().unwrap();
    let mut trust = McpTrustStore::load(trust_dir.path());
    trust.promote("fake").unwrap();

    let mut context = ActionExecutionContext::new(
        PathBuf::from("."),
        WorkspacePolicy::default(),
        ApprovalMode::Yolo,
    );
    context.mcp = Some(McpActionContext {
        handle: handle.clone(),
        trust,
        catalog: Arc::new(catalog),
    });

    let request = ActionRequest {
        schema_version: 1,
        action_id: "a1".to_string(),
        step_id: "s1".to_string(),
        kind: ActionKind::CallMcpTool,
        params: json!({ "server": "fake", "tool": "effect_tool", "args": { "echo": "round-trip" } }),
    };

    // Validate: trusted + allowlisted + unpinned ⇒ Allowed.
    assert!(matches!(
        validate_action_request_with_scope(&agent, &context, &request),
        ActionDecision::Allowed
    ));

    // Execute: the tool's echoed result comes back as the ActionResult content.
    let result = execute_action_request(&agent, &context, &request).await;
    assert_eq!(result.status, ActionStatus::Completed);
    let content = result.content.expect("tool result content");
    assert!(
        content.to_string().contains("round-trip"),
        "expected the echoed args in the result, got: {content}"
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn call_mcp_tool_on_untrusted_server_requires_approval_before_execute() {
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog snapshot");

    // No promote: the server is untrusted.
    let trust_dir = tempdir().unwrap();
    let trust = McpTrustStore::load(trust_dir.path());

    let mut context = ActionExecutionContext::new(
        PathBuf::from("."),
        WorkspacePolicy::default(),
        ApprovalMode::Yolo,
    );
    context.mcp = Some(McpActionContext {
        handle: handle.clone(),
        trust,
        catalog: Arc::new(catalog),
    });

    let request = ActionRequest {
        schema_version: 1,
        action_id: "a2".to_string(),
        step_id: "s2".to_string(),
        kind: ActionKind::CallMcpTool,
        params: json!({ "server": "fake", "tool": "effect_tool", "args": {} }),
    };

    assert!(matches!(
        validate_action_request_with_scope(&agent, &context, &request),
        ActionDecision::RequiresApproval(_)
    ));
    // Execution short-circuits to ApprovalRequired without invoking the tool.
    let result = execute_action_request(&agent, &context, &request).await;
    assert_eq!(result.status, ActionStatus::ApprovalRequired);

    handle.shutdown().await.ok();
}
