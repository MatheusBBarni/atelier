//! End-to-end integration suite for sub-task DAG execution (task_08).
//!
//! These tests drive the real scheduler, approval gate, and Plan projection
//! through the deterministic `fake` runtime — which emits a diamond
//! `A → {B, C} → D` graph on the `execution graph` control phrase and scripts
//! per-node outcomes via `fail node <id>` / `fence node <id>` markers. Every
//! assertion is over observable behavior (the durable event log or the
//! projected chat), never internal scheduler state, and concurrency is proven by
//! event ordering rather than sleeps, so the suite is flake-free.

use std::path::Path;

use multiagent::app::chat::{ChatItemKind, ChatItemStatus};
use multiagent::app::{App, AppEvent, PlanApprovalAnswer};
use multiagent::config::{load_effective_config, ConfigLoadOptions, EffectiveConfig};
use multiagent::history::{read_events_from_path, HistoryEvent};
use tempfile::tempdir;

/// A fake-runtime config with the DAG capability enabled. `normal` selects the
/// whole-plan approval gate; otherwise the run is `yolo` (auto-accept).
fn dag_config(dir: &Path, normal: bool) -> EffectiveConfig {
    let approval = if normal {
        "approval_mode = \"normal\"\n\n"
    } else {
        ""
    };
    let contents = format!(
        "{approval}[features]\nexecution_graph = true\n\n\
         [limits]\nmax_parallel_agent_steps = 3\n\n\
         [runtimes.fake]\ntype = \"fake\"\n\n\
         [agents.orchestrator]\nruntime = \"fake\"\n\n\
         [agents.explorer]\nruntime = \"fake\"\n\n\
         [agents.fixer]\nruntime = \"fake\"\n\n\
         [agents.reviewer]\nruntime = \"fake\"\n"
    );
    let path = dir.join("atelier.toml");
    std::fs::write(&path, contents).unwrap();
    load_effective_config(ConfigLoadOptions {
        working_directory: dir.to_path_buf(),
        config_path: Some(path),
    })
    .unwrap()
}

fn read_events(working_dir: &Path, session_id: &str) -> Vec<HistoryEvent> {
    let path = working_dir
        .join(".atelier")
        .join("sessions")
        .join(session_id)
        .join("events.jsonl");
    read_events_from_path(&path).unwrap()
}

/// Position of a node lifecycle event (`kind` + payload `node_id`) in the log.
fn node_pos(events: &[HistoryEvent], kind: &str, node_id: &str) -> Option<usize> {
    events.iter().position(|event| {
        event.kind == kind && event.payload.get("node_id").and_then(|v| v.as_str()) == Some(node_id)
    })
}

fn has_node_event(events: &[HistoryEvent], kind: &str, node_id: &str) -> bool {
    node_pos(events, kind, node_id).is_some()
}

#[tokio::test]
async fn diamond_runs_b_and_c_concurrently_then_d() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), false)).await.unwrap();

    app.submit_prompt("execution graph create a feature")
        .await
        .unwrap();

    let session_id = app.state().session_id.clone();
    let events = read_events(dir.path(), &session_id);

    let run_b = node_pos(&events, "node_running", "b").expect("b ran");
    let run_c = node_pos(&events, "node_running", "c").expect("c ran");
    let done_b = node_pos(&events, "node_succeeded", "b").expect("b succeeded");
    let done_c = node_pos(&events, "node_succeeded", "c").expect("c succeeded");
    let run_d = node_pos(&events, "node_running", "d").expect("d ran");

    // Concurrency: B and C both START before either FINISHES — they overlap.
    assert!(
        run_b < done_b && run_b < done_c,
        "b started before either finished"
    );
    assert!(
        run_c < done_b && run_c < done_c,
        "c started before either finished"
    );
    // Dependency ordering: D starts only after both B and C have succeeded.
    assert!(
        run_d > done_b && run_d > done_c,
        "d waited for both b and c"
    );
    // The whole graph completed.
    assert!(events
        .iter()
        .any(|event| event.kind == "execution_graph_completed"));
}

#[tokio::test]
async fn fail_closed_skips_dependent_when_a_node_fails() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), false)).await.unwrap();

    app.submit_prompt("execution graph fail node b create a feature")
        .await
        .unwrap();

    let session_id = app.state().session_id.clone();
    let events = read_events(dir.path(), &session_id);

    assert!(has_node_event(&events, "node_failed", "b"), "b failed");
    assert!(
        has_node_event(&events, "node_succeeded", "c"),
        "c still succeeded"
    );
    // D is downstream of the failed B → fail-closed Skipped, never run.
    assert!(has_node_event(&events, "node_skipped", "d"), "d skipped");
    assert!(!has_node_event(&events, "node_running", "d"), "d never ran");

    // The completed report distinguishes failed vs skipped nodes.
    let completed = events
        .iter()
        .find(|event| event.kind == "execution_graph_completed")
        .expect("graph completed");
    let skipped: Vec<&str> = completed.payload["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let failed: Vec<&str> = completed.payload["failed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(skipped.contains(&"d"), "skipped: {skipped:?}");
    assert!(failed.contains(&"b"), "failed: {failed:?}");
}

#[tokio::test]
async fn yolo_runs_without_a_gate() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), false)).await.unwrap();

    app.submit_prompt("execution graph create a feature")
        .await
        .unwrap();

    assert!(
        app.state().pending_plan_approval.is_none(),
        "yolo never gates"
    );
    let events = read_events(dir.path(), &app.state().session_id);
    assert!(events.iter().any(|event| {
        event.kind == "execution_graph_approved" && event.payload["auto_accepted"] == true
    }));
}

#[tokio::test]
async fn normal_mode_blocks_until_accepted_then_completes() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), true)).await.unwrap();

    app.submit_prompt("execution graph create a feature")
        .await
        .unwrap();

    // Blocked: the plan is proposed and pending; no node has run.
    let pending = app
        .state()
        .pending_plan_approval
        .clone()
        .expect("plan pending in normal mode");
    {
        let events = read_events(dir.path(), &app.state().session_id);
        assert!(events
            .iter()
            .any(|event| event.kind == "execution_graph_proposed"));
        assert!(!events.iter().any(|event| event.kind == "node_running"));
    }

    app.handle_event(AppEvent::PlanApprovalResolved(
        pending.question_id,
        PlanApprovalAnswer::Accept,
    ))
    .await
    .unwrap();

    let events = read_events(dir.path(), &app.state().session_id);
    assert!(events
        .iter()
        .any(|event| event.kind == "execution_graph_approved"));
    assert!(
        has_node_event(&events, "node_succeeded", "d"),
        "d ran after accept"
    );
    assert!(events
        .iter()
        .any(|event| event.kind == "execution_graph_completed"));
    assert!(app.state().pending_plan_approval.is_none());
}

#[tokio::test]
async fn normal_mode_reject_with_reason_re_proposes() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), true)).await.unwrap();

    app.submit_prompt("execution graph create a feature")
        .await
        .unwrap();
    let pending = app
        .state()
        .pending_plan_approval
        .clone()
        .expect("plan pending");

    app.handle_event(AppEvent::PlanApprovalResolved(
        pending.question_id,
        PlanApprovalAnswer::Reject {
            reason: Some("too risky".to_string()),
        },
    ))
    .await
    .unwrap();

    let events = read_events(dir.path(), &app.state().session_id);
    let rejected = events
        .iter()
        .find(|event| event.kind == "execution_graph_rejected")
        .expect("rejected event");
    assert_eq!(rejected.payload["reason"], "too risky");
    // No node ran for the rejected plan; the orchestrator re-proposes, so a new
    // proposal is pending (return-to-orchestrator, not a hard failure).
    assert!(!events.iter().any(|event| event.kind == "node_running"));
    assert!(app.state().pending_plan_approval.is_some(), "re-proposed");
}

#[tokio::test]
async fn run_surfaces_one_evolving_plan_item() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), false)).await.unwrap();

    app.submit_prompt("execution graph create a feature")
        .await
        .unwrap();

    // Exactly one Plan chat item, ended Completed; no per-node AgentProgress flooding.
    let plan_items: Vec<_> = app
        .state()
        .chat_items
        .iter()
        .filter(|item| item.kind == ChatItemKind::Plan)
        .collect();
    assert_eq!(plan_items.len(), 1, "exactly one evolving Plan item");
    assert_eq!(plan_items[0].status, ChatItemStatus::Completed);
    assert!(
        !app.state()
            .chat_items
            .iter()
            .any(|item| item.kind == ChatItemKind::AgentProgress),
        "per-node live items are suppressed for graph nodes"
    );
}

#[tokio::test]
async fn out_of_scope_write_is_denied_by_the_fence() {
    let dir = tempdir().unwrap();
    let mut app = App::new(dag_config(dir.path(), false)).await.unwrap();

    // Node B is scripted to write outside its declared scope.
    app.submit_prompt("execution graph fence node b create a feature")
        .await
        .unwrap();

    let events = read_events(dir.path(), &app.state().session_id);
    // The runtime fence denied B's out-of-scope write.
    assert!(
        events.iter().any(|event| event.kind == "action_denied"),
        "the fence denied the out-of-scope write"
    );
    // B did not succeed, so its dependent D is fail-closed Skipped (the scheduler
    // never co-ran two intersecting-write nodes — sibling scopes are disjoint).
    assert!(
        !has_node_event(&events, "node_succeeded", "b"),
        "b did not succeed"
    );
    assert!(has_node_event(&events, "node_skipped", "d"), "d skipped");
    assert!(
        has_node_event(&events, "node_succeeded", "c"),
        "c still succeeded"
    );
}
