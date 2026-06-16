//! Integration test for the governance-decision chat projection.
//!
//! Exercises the public event→transcript path: a recorded
//! `governance_decision_requested` event must project into the chat transcript
//! as a single `GovernanceDecision` item carrying the intent text. This lives
//! in the integration-test crate so it only touches `multiagent`'s public API.

use multiagent::app::chat::{ChatItemKind, ChatItemStatus, ChatProjection};
use multiagent::governance::{
    GovernanceDecisionView, GovernanceKind, GOVERNANCE_DECISION_REQUESTED,
};
use multiagent::history::HistoryEvent;

#[test]
fn governance_decision_requested_event_projects_into_transcript() {
    let view = GovernanceDecisionView {
        run_id: "run-int".to_string(),
        decision_id: "dec-int".to_string(),
        kind: GovernanceKind::EarlyAbort,
        title: "Confirm intent before this run edits files".to_string(),
        intent: "Add a retry budget to the runtime executor".to_string(),
        approach: vec!["Thread a budget through execute_runtime_step".to_string()],
        agent: Some("implementer".to_string()),
        write_scope: vec!["src/runtime".to_string()],
        risk_label: "Edits source files in src/runtime".to_string(),
        plan: None,
    };

    let payload = serde_json::to_value(&view).expect("serialize view to payload");
    let event = HistoryEvent::new(
        "session-int",
        Some("run-int".to_string()),
        None,
        GOVERNANCE_DECISION_REQUESTED,
        payload,
    );

    let projection = ChatProjection::rebuild(std::slice::from_ref(&event));
    let items = projection.items();

    let governance_items: Vec<_> = items
        .iter()
        .filter(|item| item.kind == ChatItemKind::GovernanceDecision)
        .collect();
    assert_eq!(
        governance_items.len(),
        1,
        "exactly one governance decision item expected"
    );

    let item = governance_items[0];
    assert_eq!(item.status, ChatItemStatus::WaitingForUser);
    assert_eq!(item.title, "Confirm intent before this run edits files");
    assert!(
        item.body
            .iter()
            .any(|line| line.text == "Add a retry budget to the runtime executor"),
        "decision card must carry the intent text"
    );
    assert!(
        item.body
            .iter()
            .any(|line| line.text == "Risk: Edits source files in src/runtime"),
        "risk must be an explicit text label"
    );
}
