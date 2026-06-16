//! Sibling-conformance contract for the governance spine.
//!
//! This test proves the spine's central claim (ADR-001/002/003): the shared
//! [`GovernanceDecisionView`] is a *superset envelope* that can represent both
//! sibling packets' decisions without loss, so "shared contract" is verified,
//! not merely asserted. The siblings adopt this contract in Phase 2; V1 only
//! defines and proves it, and **must not** touch sibling code — so the sibling
//! shapes below are synthetic fixtures defined locally, never imported.
//!
//! # The conformance contract
//!
//! A consumer populates a [`GovernanceDecisionView`] like this:
//!
//! - `run_id` / `decision_id` — the consumer's run + a stable id for this
//!   decision (echoed back in the resolution event).
//! - `kind` — which surface raised it. V1 ships [`GovernanceKind::EarlyAbort`];
//!   the siblings add `ActionApproval` (approval-trust-list) and `PlanApproval`
//!   (subtask-dag-execution) variants when they migrate. The *envelope fields*
//!   below are already sufficient for both, so migration is additive.
//! - `title` — a short headline for the decision card.
//! - `intent` — the interpreted action/goal in plain language. For an action
//!   approval this is the action + its target ("force-push refs/heads/main");
//!   for a plan it is the plan's reason.
//! - `approach` — supporting bullets (e.g. the risk rationale, or the plan's
//!   high-level steps); optional.
//! - `agent` — the responsible agent, when one is.
//! - `write_scope` — the workspace targets the decision may touch. An action
//!   approval puts its single trust target here; a plan leaves it empty and
//!   carries per-step scope in `plan`.
//! - `risk_label` — plain-language risk (words, never color): the sibling's risk
//!   tier plus its one-line reason.
//! - `plan` — the optional structured [`GovernancePlanView`]. A single-agent
//!   echo carries one step and no edges; a DAG carries N steps + edges. Each
//!   [`GovernancePlanStep`] is `{ id, agent, label, write_scope }`; `edges` are
//!   `(from_id, to_id)` pairs.
//!
//! The two `map_*` functions below ARE that contract, realized for each sibling
//! shape; the tests assert the mapping is lossless on the fields that matter.

use std::path::PathBuf;

use multiagent::governance::{
    GovernanceDecisionView, GovernanceKind, GovernancePlanStep, GovernancePlanView,
};

// ── Synthetic `approval-trust-list` shapes (mirrored, not imported) ──────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RiskTier {
    Low,
    Medium,
    High,
}

impl RiskTier {
    fn word(self) -> &'static str {
        match self {
            RiskTier::Low => "Low",
            RiskTier::Medium => "Medium",
            RiskTier::High => "High",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TrustTarget {
    Command(String),
    WritePath(PathBuf),
}

impl TrustTarget {
    fn display(&self) -> String {
        match self {
            TrustTarget::Command(command) => command.clone(),
            TrustTarget::WritePath(path) => path.display().to_string(),
        }
    }
}

/// A `RiskNote`-backed approval, plus the action it gates (the fields the
/// approval-trust-list modal renders).
#[derive(Clone, Debug)]
struct ApprovalFixture {
    run_id: String,
    decision_id: String,
    agent: String,
    action: String,
    tier: RiskTier,
    catastrophic: bool,
    reason: String,
    target: Option<TrustTarget>,
}

// ── Synthetic `subtask-dag-execution` shapes (mirrored, not imported) ────────

#[derive(Clone, Debug)]
struct ExecutionNode {
    node_id: String,
    step_label: String,
    agent: String,
    write_files: Vec<String>,
}

#[derive(Clone, Debug)]
struct ExecutionEdge {
    from: String,
    to: String,
}

#[derive(Clone, Debug)]
struct ExecutionGraph {
    run_id: String,
    graph_id: String,
    reason: String,
    nodes: Vec<ExecutionNode>,
    edges: Vec<ExecutionEdge>,
}

// ── The conformance contract, realized ──────────────────────────────────────

/// Map an `approval-trust-list`-style action approval onto the shared view.
fn map_approval(approval: &ApprovalFixture) -> GovernanceDecisionView {
    let target = approval.target.as_ref().map(TrustTarget::display);
    let intent = match &target {
        Some(target) => format!("{} {}", approval.action, target),
        None => approval.action.clone(),
    };
    let mut approach = vec![approval.reason.clone()];
    if approval.catastrophic {
        approach.push("Catastrophic: always prompts and is never trustable.".to_string());
    }
    GovernanceDecisionView {
        run_id: approval.run_id.clone(),
        decision_id: approval.decision_id.clone(),
        // Phase 2 adds `GovernanceKind::ActionApproval`; the envelope is ready.
        kind: GovernanceKind::EarlyAbort,
        title: "Confirm a gated action".to_string(),
        intent,
        approach,
        agent: Some(approval.agent.clone()),
        write_scope: target.into_iter().collect(),
        risk_label: format!("{} - {}", approval.tier.word(), approval.reason),
        plan: None,
    }
}

/// Map a `subtask-dag-execution`-style `ExecutionGraph` onto the shared plan.
fn map_execution_graph(graph: &ExecutionGraph) -> GovernanceDecisionView {
    let steps = graph
        .nodes
        .iter()
        .map(|node| GovernancePlanStep {
            id: node.node_id.clone(),
            agent: node.agent.clone(),
            label: node.step_label.clone(),
            write_scope: node.write_files.clone(),
        })
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect();
    GovernanceDecisionView {
        run_id: graph.run_id.clone(),
        decision_id: graph.graph_id.clone(),
        // Phase 2 adds `GovernanceKind::PlanApproval`; the envelope is ready.
        kind: GovernanceKind::EarlyAbort,
        title: "Confirm the execution plan".to_string(),
        intent: graph.reason.clone(),
        approach: Vec::new(),
        agent: None,
        write_scope: Vec::new(),
        risk_label: "Runs a multi-step plan across your workspace".to_string(),
        plan: Some(GovernancePlanView { steps, edges }),
    }
}

#[test]
fn risk_tier_words_cover_all_tiers() {
    // The risk_label is built from the tier word, so every tier must render.
    assert_eq!(RiskTier::Low.word(), "Low");
    assert_eq!(RiskTier::Medium.word(), "Medium");
    assert_eq!(RiskTier::High.word(), "High");
}

#[test]
fn approval_shape_maps_into_the_view_without_losing_fields() {
    let approval = ApprovalFixture {
        run_id: "run-approval".to_string(),
        decision_id: "dec-approval".to_string(),
        agent: "fixer".to_string(),
        action: "force-push".to_string(),
        tier: RiskTier::High,
        catastrophic: true,
        reason: "Rewrites shared history on a protected branch".to_string(),
        target: Some(TrustTarget::Command(
            "git push --force origin refs/heads/main".to_string(),
        )),
    };

    let view = map_approval(&approval);

    // Intent carries the action + target.
    assert!(view.intent.contains("force-push"));
    assert!(view.intent.contains("refs/heads/main"));
    // Risk label carries the tier word and the reason.
    assert!(view.risk_label.contains("High"));
    assert!(view
        .risk_label
        .contains("Rewrites shared history on a protected branch"));
    // The trust target is preserved in the write-scope.
    assert_eq!(
        view.write_scope,
        vec!["git push --force origin refs/heads/main".to_string()]
    );
    assert_eq!(view.agent.as_deref(), Some("fixer"));
}

#[test]
fn approval_with_write_path_target_preserves_the_path() {
    let approval = ApprovalFixture {
        run_id: "run-2".to_string(),
        decision_id: "dec-2".to_string(),
        agent: "fixer".to_string(),
        action: "overwrite".to_string(),
        tier: RiskTier::Medium,
        catastrophic: false,
        reason: "Replaces a tracked source file".to_string(),
        target: Some(TrustTarget::WritePath(PathBuf::from("src/config/mod.rs"))),
    };

    let view = map_approval(&approval);

    assert!(view.intent.contains("src/config/mod.rs"));
    assert_eq!(view.write_scope, vec!["src/config/mod.rs".to_string()]);
    assert!(view.risk_label.starts_with("Medium"));
}

#[test]
fn execution_graph_maps_into_the_plan_preserving_steps_and_edges() {
    let graph = ExecutionGraph {
        run_id: "run-dag".to_string(),
        graph_id: "graph-1".to_string(),
        reason: "Implement, test, and review the feature".to_string(),
        nodes: vec![
            ExecutionNode {
                node_id: "implement".to_string(),
                step_label: "Implement the feature".to_string(),
                agent: "fixer".to_string(),
                write_files: vec!["src/feature.rs".to_string()],
            },
            ExecutionNode {
                node_id: "test".to_string(),
                step_label: "Add tests".to_string(),
                agent: "fixer".to_string(),
                write_files: vec!["tests/feature.rs".to_string()],
            },
            ExecutionNode {
                node_id: "review".to_string(),
                step_label: "Review the change".to_string(),
                agent: "reviewer".to_string(),
                write_files: vec![],
            },
        ],
        edges: vec![
            ExecutionEdge {
                from: "implement".to_string(),
                to: "test".to_string(),
            },
            ExecutionEdge {
                from: "test".to_string(),
                to: "review".to_string(),
            },
        ],
    };

    let view = map_execution_graph(&graph);
    let plan = view.plan.expect("plan present");

    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.edges.len(), 2);
    assert_eq!(
        plan.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["implement", "test", "review"]
    );
    assert_eq!(
        plan.edges,
        vec![
            ("implement".to_string(), "test".to_string()),
            ("test".to_string(), "review".to_string()),
        ]
    );
    // Per-node write scope survives the mapping.
    assert_eq!(
        plan.steps[0].write_scope,
        vec!["src/feature.rs".to_string()]
    );
    assert!(view.intent.contains("Implement, test, and review"));
}

#[test]
fn read_only_single_step_intent_maps_to_one_step_plan_with_no_edges() {
    // The early-abort echo case: a single read-only step, no edges.
    let graph = ExecutionGraph {
        run_id: "run-echo".to_string(),
        graph_id: "graph-echo".to_string(),
        reason: "Summarize the repository structure".to_string(),
        nodes: vec![ExecutionNode {
            node_id: "explore".to_string(),
            step_label: "Read and summarize".to_string(),
            agent: "explorer".to_string(),
            write_files: vec![],
        }],
        edges: vec![],
    };

    let plan = map_execution_graph(&graph).plan.expect("plan present");

    assert_eq!(plan.steps.len(), 1);
    assert!(plan.edges.is_empty());
    assert!(plan.steps[0].write_scope.is_empty());
}
