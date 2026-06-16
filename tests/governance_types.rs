//! Cross-module construction smoke test for the shared governance contract.
//!
//! This lives in the integration-test crate (a separate crate that depends on
//! `multiagent`) so it proves the governance types are reachable and
//! constructible from *outside* the defining module — i.e. the public exports
//! are real, not just `pub(crate)`.

use multiagent::governance::{
    GovernanceAnswer, GovernanceDecisionView, GovernanceKind, GovernancePlanStep,
    GovernancePlanView, GOVERNANCE_DECISION_REQUESTED, GOVERNANCE_DECISION_RESOLVED,
};

#[test]
fn governance_decision_view_is_constructible_from_outside_the_module() {
    let view = GovernanceDecisionView {
        run_id: "run-ext".to_string(),
        decision_id: "dec-ext".to_string(),
        kind: GovernanceKind::EarlyAbort,
        title: "Confirm intent before this run edits files".to_string(),
        intent: "Rename the widget".to_string(),
        approach: vec!["Update call sites".to_string()],
        agent: Some("implementer".to_string()),
        write_scope: vec!["src".to_string()],
        risk_label: "Edits source files".to_string(),
        plan: Some(GovernancePlanView {
            steps: vec![GovernancePlanStep {
                id: "s1".to_string(),
                agent: "implementer".to_string(),
                label: "Rename".to_string(),
                write_scope: vec!["src".to_string()],
            }],
            edges: vec![],
        }),
    };

    assert_eq!(view.kind, GovernanceKind::EarlyAbort);
    assert_eq!(view.plan.expect("plan present").steps.len(), 1);
}

#[test]
fn governance_answer_is_constructible_from_outside_the_module() {
    assert_eq!(GovernanceAnswer::Accept, GovernanceAnswer::Accept);
    let _ = GovernanceAnswer::Reject {
        redirect: Some("try a smaller change".to_string()),
    };
    let _ = GovernanceAnswer::Reject { redirect: None };
}

#[test]
fn event_kind_constants_are_exported() {
    assert_eq!(
        GOVERNANCE_DECISION_REQUESTED,
        "governance_decision_requested"
    );
    assert_eq!(GOVERNANCE_DECISION_RESOLVED, "governance_decision_resolved");
}
