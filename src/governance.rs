//! Shared governance-decision contract.
//!
//! This module holds the data-only types every governance surface renders
//! through: the [`GovernanceDecisionView`] envelope, its optional structured
//! [`GovernancePlanView`] payload, the [`GovernanceAnswer`] the user returns,
//! the [`GovernanceKind`] discriminator, and the two governance event-kind
//! constants.
//!
//! The early-abort gate is the V1 consumer; the sibling packets
//! (`approval-trust-list`, `subtask-dag-execution`) migrate their per-action
//! approval and DAG plan views onto this same envelope in a later phase
//! (ADR-003). Because of that, the view is deliberately a *superset* envelope:
//! a single-agent intent echo carries one [`GovernancePlanStep`] with no edges,
//! while a future DAG renders many steps + edges through the identical arm.
//!
//! Boundary: this module is **data + pure helpers only**. It must not depend on
//! `app`, `tui`, the orchestrator's run state, or any sibling-packet types.
//! Every type is serializable and round-trip stable because these views are
//! frozen into the durable event stream and replayed.

use serde::{Deserialize, Serialize};

/// Event kind written when a governance decision pauses a run for the user.
/// Payload is a serialized [`GovernanceDecisionView`].
pub const GOVERNANCE_DECISION_REQUESTED: &str = "governance_decision_requested";

/// Event kind written when a governance decision is resolved. Payload is
/// `{ decision_id, outcome: accept|reject, redirect? }`.
pub const GOVERNANCE_DECISION_RESOLVED: &str = "governance_decision_resolved";

/// Which governance surface raised the decision.
///
/// Only [`GovernanceKind::EarlyAbort`] exists in V1; `PlanApproval` and
/// `ActionApproval` are added when the sibling packets migrate onto this
/// envelope.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceKind {
    /// A first-turn single-agent run that intends to write — paused so the user
    /// can confirm the interpreted intent before anything is edited.
    EarlyAbort,
}

/// One node in the legibility plan: a single agent's slice of the work.
///
/// A single-agent echo carries exactly one of these (with no edges); a future
/// DAG carries many, joined by [`GovernancePlanView::edges`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernancePlanStep {
    /// Stable identifier referenced by [`GovernancePlanView::edges`].
    pub id: String,
    /// The agent profile that would run this step.
    pub agent: String,
    /// Plain-language description of what this step does.
    pub label: String,
    /// Workspace write-roots this step may touch.
    #[serde(default)]
    pub write_scope: Vec<String>,
}

/// The structured plan/intent payload carried by a [`GovernanceDecisionView`].
///
/// `steps` lists the work units; `edges` are `(from_id, to_id)` dependency
/// pairs. A single-agent intent echo uses one step and no edges; a DAG uses
/// many steps and the edges that connect them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernancePlanView {
    /// The plan's work units, in declaration order.
    pub steps: Vec<GovernancePlanStep>,
    /// Dependency edges as `(from_step_id, to_step_id)` pairs. Empty for a
    /// single-agent echo.
    #[serde(default)]
    pub edges: Vec<(String, String)>,
}

/// The shared decision envelope every governance surface renders through.
///
/// Consumers populate it; the early-abort gate is the V1 consumer. Fields use
/// plain language (the `risk_label` is words, never color) so the projection
/// and TUI can render it without interpreting domain-specific types.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernanceDecisionView {
    /// The run this decision belongs to.
    pub run_id: String,
    /// Stable identifier for this decision, echoed back in the resolution event.
    pub decision_id: String,
    /// Which governance surface raised the decision.
    pub kind: GovernanceKind,
    /// Headline, e.g. "Confirm intent before this run edits files".
    pub title: String,
    /// The interpreted goal (sourced from the orchestrator decision's reason).
    pub intent: String,
    /// Approach bullets (sourced from the orchestrator decision's plan).
    #[serde(default)]
    pub approach: Vec<String>,
    /// The agent that would run, when a single agent is responsible.
    #[serde(default)]
    pub agent: Option<String>,
    /// Workspace write-roots this run may touch.
    #[serde(default)]
    pub write_scope: Vec<String>,
    /// Plain-language risk label (words, not color).
    pub risk_label: String,
    /// Optional structured plan payload; a DAG fills this later.
    #[serde(default)]
    pub plan: Option<GovernancePlanView>,
}

/// The user's answer to a governance decision.
///
/// Serializes with an `outcome` tag (`accept` | `reject`) so it maps directly
/// onto the `governance_decision_resolved` event payload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GovernanceAnswer {
    /// Proceed with the run as interpreted.
    Accept,
    /// Abort the interpreted run; an optional `redirect` re-drives the
    /// orchestrator with corrective guidance.
    Reject {
        /// Corrective guidance appended to the re-drive; `None` aborts outright.
        #[serde(default)]
        redirect: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_view() -> GovernanceDecisionView {
        GovernanceDecisionView {
            run_id: "run-1".to_string(),
            decision_id: "dec-1".to_string(),
            kind: GovernanceKind::EarlyAbort,
            title: "Confirm intent before this run edits files".to_string(),
            intent: "Refactor the config loader into smaller modules".to_string(),
            approach: vec![
                "Split merge logic out of mod.rs".to_string(),
                "Add focused unit tests".to_string(),
            ],
            agent: Some("implementer".to_string()),
            write_scope: vec!["src/config".to_string()],
            risk_label: "Edits source files in src/config".to_string(),
            plan: Some(GovernancePlanView {
                steps: vec![GovernancePlanStep {
                    id: "step-1".to_string(),
                    agent: "implementer".to_string(),
                    label: "Split the config loader".to_string(),
                    write_scope: vec!["src/config".to_string()],
                }],
                edges: vec![],
            }),
        }
    }

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_string(value).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn governance_decision_view_round_trips() {
        let view = sample_view();
        assert_eq!(round_trip(&view), view);
    }

    #[test]
    fn governance_decision_view_round_trips_with_minimal_fields() {
        let view = GovernanceDecisionView {
            run_id: "run-2".to_string(),
            decision_id: "dec-2".to_string(),
            kind: GovernanceKind::EarlyAbort,
            title: "Confirm intent".to_string(),
            intent: "Do the thing".to_string(),
            approach: vec![],
            agent: None,
            write_scope: vec![],
            risk_label: "Low".to_string(),
            plan: None,
        };
        assert_eq!(round_trip(&view), view);
    }

    #[test]
    fn governance_kind_round_trips_as_snake_case() {
        let json = serde_json::to_string(&GovernanceKind::EarlyAbort).expect("serialize");
        assert_eq!(json, "\"early_abort\"");
        assert_eq!(
            round_trip(&GovernanceKind::EarlyAbort),
            GovernanceKind::EarlyAbort
        );
    }

    #[test]
    fn governance_answer_accept_round_trips() {
        assert_eq!(
            round_trip(&GovernanceAnswer::Accept),
            GovernanceAnswer::Accept
        );
    }

    #[test]
    fn governance_answer_reject_with_redirect_round_trips() {
        let answer = GovernanceAnswer::Reject {
            redirect: Some("focus on tests instead".to_string()),
        };
        assert_eq!(round_trip(&answer), answer);
    }

    #[test]
    fn governance_answer_reject_without_redirect_round_trips() {
        let answer = GovernanceAnswer::Reject { redirect: None };
        assert_eq!(round_trip(&answer), answer);
    }

    #[test]
    fn governance_answer_serializes_with_outcome_tag() {
        assert_eq!(
            serde_json::to_string(&GovernanceAnswer::Accept).expect("serialize"),
            "{\"outcome\":\"accept\"}"
        );
    }

    #[test]
    fn governance_plan_view_single_step_no_edges_round_trips() {
        let plan = GovernancePlanView {
            steps: vec![GovernancePlanStep {
                id: "only".to_string(),
                agent: "implementer".to_string(),
                label: "Do everything".to_string(),
                write_scope: vec!["src".to_string()],
            }],
            edges: vec![],
        };
        assert_eq!(round_trip(&plan), plan);
    }

    #[test]
    fn governance_plan_view_dag_shaped_round_trips() {
        let plan = GovernancePlanView {
            steps: vec![
                GovernancePlanStep {
                    id: "a".to_string(),
                    agent: "planner".to_string(),
                    label: "Plan".to_string(),
                    write_scope: vec![],
                },
                GovernancePlanStep {
                    id: "b".to_string(),
                    agent: "implementer".to_string(),
                    label: "Build".to_string(),
                    write_scope: vec!["src".to_string()],
                },
                GovernancePlanStep {
                    id: "c".to_string(),
                    agent: "reviewer".to_string(),
                    label: "Review".to_string(),
                    write_scope: vec![],
                },
            ],
            edges: vec![
                ("a".to_string(), "b".to_string()),
                ("b".to_string(), "c".to_string()),
            ],
        };
        let restored = round_trip(&plan);
        assert_eq!(restored, plan);
        assert_eq!(restored.steps.len(), 3);
        assert_eq!(restored.edges.len(), 2);
    }

    #[test]
    fn event_kind_constants_have_stable_values() {
        assert_eq!(
            GOVERNANCE_DECISION_REQUESTED,
            "governance_decision_requested"
        );
        assert_eq!(GOVERNANCE_DECISION_RESOLVED, "governance_decision_resolved");
    }
}
