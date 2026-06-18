use crate::config::{AgentProfile, Capability, EffectiveConfig};
use crate::mcp::ToolCatalog;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};

pub const JSON_START: &str = "<<<MULTIAGENT_JSON_START>>>";
pub const JSON_END: &str = "<<<MULTIAGENT_JSON_END>>>";
pub const COUNCIL_WORKFLOW_AGENT_ID: &str = "council";
const JSON_START_PREFIX: &str = "<<<MULTIAGENT_JSON_START";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Idle,
    Planning,
    Running,
    WaitingForUser,
    Interrupted,
    Completed,
    Failed,
    LimitReached,
}

impl RunState {
    /// Whether the run has reached a terminal state — finished, not in flight.
    /// True for exactly `Completed`, `Failed`, `Interrupted`, and `LimitReached`;
    /// false for `Idle`, `Planning`, `Running`, and `WaitingForUser`.
    ///
    /// The single expression of "is this run finished", consumed by session
    /// outcome derivation (task_03) and dangling-run detection on resume
    /// (task_10) (ADR-008). It does not change which states are terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RunState::Completed | RunState::Failed | RunState::Interrupted | RunState::LimitReached
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Queued,
    Starting,
    Running,
    WaitingForAction,
    WaitingForApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Continue,
    WaitingForUser,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrchestratorDecision {
    pub schema_version: u32,
    pub decision_id: String,
    pub run_id: String,
    pub status: DecisionStatus,
    pub plan: Vec<String>,
    #[serde(default)]
    pub next_agent: Option<String>,
    #[serde(default)]
    pub next_step: Option<DecisionNextStep>,
    pub reason: String,
    pub required_capabilities: Vec<Capability>,
    pub stop_condition: String,
    pub clarifying_question: Option<String>,
    #[serde(default)]
    pub clarifying_options: Vec<ClarificationOption>,
    #[serde(default)]
    pub recommended_option_id: Option<String>,
    /// When true, the clarifying_options may be answered by selecting several at
    /// once (multi-select). Defaults to false (single-select). Only meaningful
    /// for `waiting_for_user` decisions that carry clarifying_options.
    #[serde(default)]
    pub multi_select: bool,
    pub final_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClarificationOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DecisionNextStep {
    SingleAgent(SingleAgentStepPlan),
    ParallelGroup(ParallelGroupPlan),
    /// A dependency graph of nodes the scheduler admits lazily by predecessor
    /// success + write-disjointness (`kind="dag"`, decision `schema_version 3`,
    /// ADR-004). Only emitted when `features.execution_graph` is on.
    Dag(ExecutionGraph),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SingleAgentStepPlan {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelGroupPlan {
    pub group_id: String,
    pub reason: String,
    pub steps: Vec<ParallelChildStepPlan>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelChildStepPlan {
    pub step_label: String,
    pub agent: String,
    pub instruction: String,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    pub file_scope: ParallelFileScope,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParallelFileScope {
    pub write_files: Vec<String>,
    pub read_roots: Vec<String>,
}

/// A static dependency graph the orchestrator can propose as a single decision
/// (`kind="dag"`, ADR-004). Nodes mirror `ParallelChildStepPlan` plus a durable
/// `node_id`; edges declare ordering. The scheduler (task_04) lowers this onto
/// the reused parallel executor with concurrency-aware admission.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionGraph {
    pub graph_id: String,
    pub reason: String,
    pub nodes: Vec<ExecutionNode>,
    pub edges: Vec<ExecutionEdge>,
}

/// One graph node. `file_scope` is reused verbatim by the runtime write-fence;
/// command-capability is derived from `required_capabilities` (no new field).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionNode {
    pub node_id: String,
    pub step_label: String,
    pub agent: String,
    pub instruction: String,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    pub file_scope: ParallelFileScope,
}

/// A directed ordering edge: `to` becomes admittable only after `from` succeeds.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionEdge {
    pub from: String,
    pub to: String,
    pub kind: ExecutionEdgeKind,
}

/// Both edge kinds gate readiness identically; the distinction is intent
/// (a data handoff vs. a pure ordering constraint), surfaced for legibility.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEdgeKind {
    DataDependency,
    SemanticGate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentResultStatus {
    Completed,
    Blocked,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
    ApprovalDenied,
    NoChanges,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactReference {
    pub artifact_id: String,
    pub path: String,
    pub media_type: String,
    pub byte_length: usize,
    pub sha256: String,
    pub redaction_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentResult {
    pub schema_version: u32,
    pub agent: String,
    pub step_id: String,
    pub status: AgentResultStatus,
    pub summary: String,
    pub findings: Vec<String>,
    pub changed_files: Vec<String>,
    pub commands: Vec<String>,
    pub verification: Vec<String>,
    pub blocker: Option<String>,
    pub artifacts: Vec<ArtifactReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RunStepResult {
    Agent { result: AgentResult },
    ParallelGroup { result: ParallelGroupResult },
    Dag { result: ExecutionGraphResult },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelGroupResult {
    pub schema_version: u32,
    pub group_id: String,
    pub run_id: String,
    pub status: ParallelGroupStatus,
    pub summary: String,
    pub children: Vec<ParallelChildResultRef>,
    pub counts: BTreeMap<String, u32>,
    pub changed_files: Vec<String>,
    pub blocked_scopes: Vec<ParallelBlockedScope>,
    pub failed_scopes: Vec<ParallelFailedScope>,
    pub approval_denials: Vec<String>,
    pub started_at: String,
    pub completed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParallelGroupStatus {
    Completed,
    CompletedWithIssues,
    Failed,
    Cancelled,
    LimitReached,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelChildResultRef {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub status: AgentResultStatus,
    pub result_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelBlockedScope {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub blocker: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelFailedScope {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub diagnostic: String,
}

/// The terminal aggregate of an execution-graph (DAG) run (ADR-004/005),
/// modeled on [`ParallelGroupResult`] and serialized into the
/// `execution_graph_completed` event. The scheduler (task_04) populates it; the
/// projection (task_06) renders it. `skipped`/`failed` carry the `node_id`s the
/// fail-closed admission terminalized without (or after a failed) run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionGraphResult {
    pub schema_version: u32,
    pub graph_id: String,
    pub run_id: String,
    pub status: ExecutionGraphStatus,
    pub summary: String,
    pub node_results: Vec<NodeResultRef>,
    pub counts: BTreeMap<String, u32>,
    pub changed_files: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<String>,
    pub started_at: String,
    pub completed_at: String,
}

/// Terminal status of an execution graph, mirroring [`ParallelGroupStatus`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionGraphStatus {
    Completed,
    CompletedWithIssues,
    Failed,
    Cancelled,
    LimitReached,
}

/// One graph node's result reference, mirroring [`ParallelChildResultRef`] with
/// a durable `node_id` instead of a `step_id`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeResultRef {
    pub node_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub status: AgentResultStatus,
    pub result_index: usize,
}

impl AgentResult {
    pub fn completed(
        agent: impl Into<String>,
        step_id: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            agent: agent.into(),
            step_id: step_id.into(),
            status: AgentResultStatus::Completed,
            summary: summary.into(),
            findings: Vec::new(),
            changed_files: Vec::new(),
            commands: Vec::new(),
            verification: Vec::new(),
            blocker: None,
            artifacts: Vec::new(),
        }
    }
}

/// The outcome of a grading round (ADR-004). Harness-constructed from canonical
/// command exit codes — never deserialized from agent output, so a fabricated
/// `AgentResult.status`/`verification` cannot manufacture a `Pass`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GradeOutcome {
    Pass,
    Fail,
    Skip,
}

/// The decisive grading verdict. `command`/`exit_code` record the canonical
/// check that grounded it; `critique` carries the failure excerpt fed to the
/// producing agent on `Fail`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraderVerdict {
    pub outcome: GradeOutcome,
    pub command: Option<String>,
    pub exit_code: Option<i64>,
    pub critique: Option<String>,
}

/// One command the grade sub-step ran, sourced from the structured
/// `command_completed` record (never model-authored text). `exit_code` is
/// `None` when the command was denied or never produced a completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    pub command: String,
    pub exit_code: Option<i64>,
    /// Optional captured output excerpt, used to seed the `critique` on failure.
    pub excerpt: Option<String>,
}

/// Derive the grading verdict purely from the grade step's recorded command
/// results (ADR-004):
/// - `Pass` — at least one canonical verification command ran and every
///   canonical command exited `0`.
/// - `Fail` — at least one canonical command ran and any exited non-zero (or
///   produced no exit code, treated as a non-pass for that check).
/// - `Skip` — no canonical verification command ran at all.
///
/// Only commands accepted by [`crate::actions::is_canonical_verification_command`]
/// count; everything else (e.g. `echo`, `cargo run`) is ignored.
pub fn derive_grade_verdict(commands: &[CommandOutcome]) -> GraderVerdict {
    let canonical: Vec<&CommandOutcome> = commands
        .iter()
        .filter(|outcome| crate::actions::is_canonical_verification_command(&outcome.command))
        .collect();

    if canonical.is_empty() {
        return GraderVerdict {
            outcome: GradeOutcome::Skip,
            command: None,
            exit_code: None,
            critique: None,
        };
    }

    // First canonical command that did not cleanly exit 0 decides a Fail.
    if let Some(failed) = canonical
        .iter()
        .find(|outcome| outcome.exit_code != Some(0))
    {
        let critique = match &failed.excerpt {
            Some(excerpt) if !excerpt.trim().is_empty() => format!(
                "`{}` failed ({}):\n{}",
                failed.command,
                describe_exit(failed.exit_code),
                excerpt.trim()
            ),
            _ => format!(
                "`{}` failed ({})",
                failed.command,
                describe_exit(failed.exit_code)
            ),
        };
        return GraderVerdict {
            outcome: GradeOutcome::Fail,
            command: Some(failed.command.clone()),
            exit_code: failed.exit_code,
            critique: Some(critique),
        };
    }

    // Every canonical command exited 0 → Pass, attributed to the last one.
    let decided = canonical
        .last()
        .expect("non-empty canonical set has a last element");
    GraderVerdict {
        outcome: GradeOutcome::Pass,
        command: Some(decided.command.clone()),
        exit_code: decided.exit_code,
        critique: None,
    }
}

/// Render an exit code for a critique, naming the no-completion case explicitly.
fn describe_exit(exit_code: Option<i64>) -> String {
    match exit_code {
        Some(code) => format!("exit {code}"),
        None => "no exit code".to_string(),
    }
}

impl OrchestratorDecision {
    pub fn normalized_next_step(&self) -> Result<Option<DecisionNextStep>> {
        if self.next_agent.is_some() && self.next_step.is_some() {
            bail!("orchestrator decision contains both next_agent and next_step");
        }

        match self.schema_version {
            1 => {
                if let Some(next_step) = &self.next_step {
                    match next_step {
                        DecisionNextStep::SingleAgent(_) => Ok(Some(next_step.clone())),
                        DecisionNextStep::ParallelGroup(_) => {
                            bail!("schema_version 1 cannot select a parallel_group next_step")
                        }
                        DecisionNextStep::Dag(_) => {
                            bail!("schema_version 1 cannot select a dag next_step")
                        }
                    }
                } else {
                    Ok(self.next_agent.as_ref().map(|agent| {
                        DecisionNextStep::SingleAgent(SingleAgentStepPlan {
                            agent: agent.clone(),
                            instruction: None,
                            required_capabilities: self.required_capabilities.clone(),
                        })
                    }))
                }
            }
            2 => {
                if self.next_agent.is_some() {
                    bail!("schema_version 2 must use next_step instead of next_agent");
                }
                if matches!(self.next_step, Some(DecisionNextStep::Dag(_))) {
                    bail!("schema_version 2 cannot select a dag next_step");
                }
                Ok(self.next_step.clone())
            }
            3 => {
                if self.next_agent.is_some() {
                    bail!("schema_version 3 must use next_step instead of next_agent");
                }
                Ok(self.next_step.clone())
            }
            version => bail!("unsupported orchestrator decision schema_version {version}"),
        }
    }
}

pub fn agent_results(results: &[RunStepResult]) -> impl Iterator<Item = &AgentResult> {
    results.iter().flat_map(|result| match result {
        RunStepResult::Agent { result } => std::slice::from_ref(result),
        // Parallel groups and DAGs aggregate their own per-child/per-node
        // results; neither contributes a top-level AgentResult here.
        RunStepResult::ParallelGroup { .. } | RunStepResult::Dag { .. } => &[],
    })
}

pub fn last_agent_result(results: &[RunStepResult]) -> Option<&AgentResult> {
    agent_results(results).last()
}

pub fn parse_orchestrator_decision(text: &str) -> Result<OrchestratorDecision> {
    parse_contract(text)
}

pub fn parse_agent_result(text: &str) -> Result<AgentResult> {
    parse_contract(text)
}

pub fn wrap_json_contract<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_string_pretty(value)?;
    Ok(format!("{JSON_START}\n{json}\n{JSON_END}"))
}

pub fn parse_contract<T>(text: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let payload = extract_json_payload(text).ok_or_else(|| anyhow!("missing JSON contract"))?;
    serde_json::from_str(payload).context("failed to parse JSON contract")
}

pub fn extract_json_payload(text: &str) -> Option<&str> {
    if let Some(payload) = extract_delimited_json_payload(text) {
        return Some(payload);
    }

    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    if let Some(fence_start) = trimmed.find("```json") {
        let rest = &trimmed[fence_start + "```json".len()..];
        let fence_end = rest.find("```")?;
        return Some(rest[..fence_end].trim());
    }

    None
}

fn extract_delimited_json_payload(text: &str) -> Option<&str> {
    let start = text.find(JSON_START_PREFIX)?;
    let after_prefix = start + JSON_START_PREFIX.len();
    let after_marker = consume_angle_marker_suffix(text, after_prefix)?;
    let rest = &text[after_marker..];
    let object_start = after_marker + rest.find('{')?;
    let object_end = find_json_object_end(text, object_start)?;
    Some(text[object_start..object_end].trim())
}

fn consume_angle_marker_suffix(text: &str, index: usize) -> Option<usize> {
    let mut current = index;
    let mut count = 0;
    let bytes = text.as_bytes();
    while current < bytes.len() && bytes[current] == b'>' {
        current += 1;
        count += 1;
    }
    (count >= 2).then_some(current)
}

fn find_json_object_end(text: &str, start: usize) -> Option<usize> {
    if text[start..].chars().next()? != '{' {
        return None;
    }

    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

pub fn validate_orchestrator_decision(
    decision: &OrchestratorDecision,
    config: &EffectiveConfig,
) -> Result<()> {
    match decision.status {
        DecisionStatus::Continue => {
            if decision.stop_condition.trim().is_empty() {
                bail!("continue decision is missing stop_condition");
            }
            let next_step = decision
                .normalized_next_step()?
                .ok_or_else(|| anyhow!("continue decision is missing next_step"))?;
            validate_decision_next_step(&next_step, config)?;
        }
        DecisionStatus::WaitingForUser => {
            validate_terminal_decision_has_no_next_step(decision)?;
            if decision
                .clarifying_question
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                bail!("waiting_for_user decision is missing clarifying_question");
            }
            validate_clarification_options(decision)?;
        }
        DecisionStatus::Complete => {
            validate_terminal_decision_has_no_next_step(decision)?;
            if decision
                .final_summary
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                bail!("complete decision is missing final_summary");
            }
        }
        DecisionStatus::Failed => {
            validate_terminal_decision_has_no_next_step(decision)?;
            if decision.reason.trim().is_empty() {
                bail!("failed decision is missing reason");
            }
        }
    }

    Ok(())
}

fn validate_terminal_decision_has_no_next_step(decision: &OrchestratorDecision) -> Result<()> {
    if decision.next_agent.is_some() || decision.next_step.is_some() {
        bail!("non-continue decision must not contain a next step");
    }
    Ok(())
}

fn validate_clarification_options(decision: &OrchestratorDecision) -> Result<()> {
    let option_count = decision.clarifying_options.len();
    if !(2..=4).contains(&option_count) {
        bail!(
            "waiting_for_user decision must include 2-4 clarifying_options, found {option_count}"
        );
    }

    let mut option_ids = BTreeSet::new();
    for (index, option) in decision.clarifying_options.iter().enumerate() {
        let id = option.id.trim();
        if id.is_empty() {
            bail!("waiting_for_user clarifying_options[{index}] is missing id");
        }

        if option.label.trim().is_empty() {
            bail!("waiting_for_user clarifying_options[{index}] is missing label");
        }

        if !option_ids.insert(id.to_string()) {
            bail!("waiting_for_user clarifying_options contains duplicate id '{id}'");
        }
    }

    if let Some(recommended_option_id) = decision.recommended_option_id.as_deref() {
        let recommended_option_id = recommended_option_id.trim();
        if recommended_option_id.is_empty() {
            bail!("waiting_for_user recommended_option_id is blank");
        }
        if !option_ids.contains(recommended_option_id) {
            bail!(
                "waiting_for_user recommended_option_id '{recommended_option_id}' does not match any clarifying_options id"
            );
        }
    }

    Ok(())
}

fn validate_decision_next_step(
    next_step: &DecisionNextStep,
    config: &EffectiveConfig,
) -> Result<()> {
    match next_step {
        DecisionNextStep::SingleAgent(plan) => validate_single_agent_step_plan(plan, config),
        DecisionNextStep::ParallelGroup(group) => validate_parallel_group_plan(group, config),
        DecisionNextStep::Dag(graph) => validate_execution_graph(graph, config),
    }
}

fn validate_single_agent_step_plan(
    plan: &SingleAgentStepPlan,
    config: &EffectiveConfig,
) -> Result<()> {
    validate_agent_reference(&plan.agent, &plan.required_capabilities, config)
}

pub fn validate_parallel_group_plan(
    group: &ParallelGroupPlan,
    config: &EffectiveConfig,
) -> Result<()> {
    if !config.features.parallel_step_groups {
        bail!("parallel step groups are disabled by features.parallel_step_groups");
    }
    if config.limits.max_parallel_agent_steps == 0 {
        bail!("parallel step groups are disabled by limits.max_parallel_agent_steps = 0");
    }
    if group.group_id.trim().is_empty() {
        bail!("parallel group is missing group_id");
    }
    if group.steps.len() < 2 {
        bail!("parallel group must contain at least two child steps");
    }
    if group.steps.len() > config.limits.max_parallel_agent_steps as usize {
        bail!(
            "parallel group has {} child steps but max_parallel_agent_steps is {}",
            group.steps.len(),
            config.limits.max_parallel_agent_steps
        );
    }

    let mut owners = BTreeMap::<PathBuf, String>::new();
    for child in &group.steps {
        validate_parallel_child_step_plan(child, config)?;
        for write_file in &child.file_scope.write_files {
            let normalized =
                validate_parallel_scope_path(write_file, &config.workspace.extra_write_roots)?;
            if let Some(existing) = owners.insert(normalized, child.step_label.clone()) {
                bail!(
                    "parallel write file {write_file} is assigned to both {existing} and {}",
                    child.step_label
                );
            }
        }
    }

    Ok(())
}

/// Validate an `ExecutionGraph` decision (ADR-004). Reuses the per-node plan
/// checks (`validate_parallel_child_step_plan`) and scope-path normalization
/// (`validate_parallel_scope_path`), then adds the graph-level invariants:
/// unique node ids, edges referencing real nodes, acyclicity, and
/// **concurrency-aware** write-file disjointness — two nodes may share a write
/// file only when a dependency path orders them (sequential reuse is legal);
/// nodes that could run concurrently must write disjoint files.
pub fn validate_execution_graph(graph: &ExecutionGraph, config: &EffectiveConfig) -> Result<()> {
    if !config.features.execution_graph {
        bail!("execution graph is disabled by features.execution_graph");
    }
    if config.limits.max_parallel_agent_steps == 0 {
        bail!("execution graph is disabled by limits.max_parallel_agent_steps = 0");
    }
    if graph.graph_id.trim().is_empty() {
        bail!("execution graph is missing graph_id");
    }
    if graph.nodes.is_empty() {
        bail!("execution graph must contain at least one node");
    }

    // Node-id uniqueness + per-node plan validation (reused verbatim).
    let mut index_of = BTreeMap::<String, usize>::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        if node.node_id.trim().is_empty() {
            bail!("execution graph node is missing node_id");
        }
        if index_of.insert(node.node_id.clone(), idx).is_some() {
            bail!("execution graph has duplicate node_id {}", node.node_id);
        }
        let child = ParallelChildStepPlan {
            step_label: node.step_label.clone(),
            agent: node.agent.clone(),
            instruction: node.instruction.clone(),
            required_capabilities: node.required_capabilities.clone(),
            file_scope: node.file_scope.clone(),
        };
        validate_parallel_child_step_plan(&child, config)?;
    }

    // Edges must reference declared nodes; build the adjacency list (from -> to).
    let node_count = graph.nodes.len();
    let mut adjacency = vec![Vec::<usize>::new(); node_count];
    for edge in &graph.edges {
        let from = *index_of.get(&edge.from).ok_or_else(|| {
            anyhow!(
                "execution graph edge references unknown from node_id {}",
                edge.from
            )
        })?;
        let to = *index_of.get(&edge.to).ok_or_else(|| {
            anyhow!(
                "execution graph edge references unknown to node_id {}",
                edge.to
            )
        })?;
        adjacency[from].push(to);
    }

    // Acyclicity via Kahn's algorithm: if any node never reaches in-degree 0,
    // there is a cycle (a self-loop A->A is caught the same way).
    let mut indegree = vec![0usize; node_count];
    for targets in &adjacency {
        for &to in targets {
            indegree[to] += 1;
        }
    }
    let mut queue: VecDeque<usize> = (0..node_count).filter(|&i| indegree[i] == 0).collect();
    let mut settled = 0usize;
    while let Some(node) = queue.pop_front() {
        settled += 1;
        for &next in &adjacency[node] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                queue.push_back(next);
            }
        }
    }
    if settled != node_count {
        bail!("execution graph contains a cycle");
    }

    // Descendant reachability (the graph is acyclic here): descendants[i] is the
    // set of nodes reachable from i. Two nodes with no path either way could run
    // concurrently and must therefore write disjoint files.
    //
    // This is O(V^2) space (a full boolean reachability matrix). That is fine at
    // the practical graph sizes here — total node count is bounded by the
    // `max_agent_steps` step budget (validation rejects graphs that can't fit).
    // If that ceiling is ever raised by orders of magnitude, switch to on-demand
    // reachability (BFS per disjointness check) or a bitset transitive closure.
    let mut descendants = vec![vec![false; node_count]; node_count];
    for (start, reachable) in descendants.iter_mut().enumerate() {
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for &next in &adjacency[node] {
                if !reachable[next] {
                    reachable[next] = true;
                    stack.push(next);
                }
            }
        }
    }

    // Normalize each node's write set once, then enforce disjointness only
    // between nodes that may run concurrently (no path between them).
    let mut write_sets: Vec<BTreeSet<PathBuf>> = Vec::with_capacity(node_count);
    for node in &graph.nodes {
        let mut set = BTreeSet::new();
        for write_file in &node.file_scope.write_files {
            let normalized =
                validate_parallel_scope_path(write_file, &config.workspace.extra_write_roots)?;
            set.insert(normalized);
        }
        write_sets.push(set);
    }
    for i in 0..node_count {
        for j in (i + 1)..node_count {
            let may_run_concurrently = !descendants[i][j] && !descendants[j][i];
            if !may_run_concurrently {
                continue;
            }
            if let Some(shared) = write_sets[i].intersection(&write_sets[j]).next() {
                bail!(
                    "execution graph nodes {} and {} may run concurrently but both write {}",
                    graph.nodes[i].node_id,
                    graph.nodes[j].node_id,
                    shared.display()
                );
            }
        }
    }

    Ok(())
}

fn validate_parallel_child_step_plan(
    child: &ParallelChildStepPlan,
    config: &EffectiveConfig,
) -> Result<()> {
    if child.step_label.trim().is_empty() {
        bail!("parallel child step is missing step_label");
    }
    if child.instruction.trim().is_empty() {
        bail!(
            "parallel child step {} is missing instruction",
            child.step_label
        );
    }
    validate_agent_reference(&child.agent, &child.required_capabilities, config)?;
    validate_parallel_file_scope(&child.file_scope, config)?;
    let agent = config
        .agents
        .get(&child.agent)
        .ok_or_else(|| anyhow!("decision references unknown agent {}", child.agent))?;
    if agent.has_capability(&Capability::Edit) && child.file_scope.write_files.is_empty() {
        bail!(
            "parallel edit-capable child {} is missing write_files",
            child.step_label
        );
    }
    Ok(())
}

fn validate_parallel_file_scope(scope: &ParallelFileScope, config: &EffectiveConfig) -> Result<()> {
    if scope.write_files.is_empty() && scope.read_roots.is_empty() {
        bail!("parallel file scope must include at least one write_file or read_root");
    }
    let mut seen_write_files = BTreeMap::<PathBuf, ()>::new();
    for write_file in &scope.write_files {
        let normalized =
            validate_parallel_scope_path(write_file, &config.workspace.extra_write_roots)?;
        if seen_write_files.insert(normalized, ()).is_some() {
            bail!("parallel file scope contains duplicate write file {write_file}");
        }
        if parallel_scope_path_is_existing_directory(write_file, config)? {
            bail!("parallel write_files must be exact file paths, not directories: {write_file}");
        }
    }
    for read_root in &scope.read_roots {
        validate_parallel_scope_path(read_root, &config.workspace.extra_read_roots)?;
    }
    Ok(())
}

fn validate_parallel_scope_path(path: &str, extra_roots: &[PathBuf]) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("parallel file scope contains an empty path");
    }
    if trimmed.split('/').any(|component| component == ".") {
        bail!("current-directory components are not allowed in parallel scope: {path}");
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        if extra_roots.iter().any(|root| candidate.starts_with(root)) {
            return Ok(candidate.to_path_buf());
        }
        bail!("absolute paths are not allowed in parallel file scopes: {path}");
    }

    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                bail!("path traversal is not allowed in parallel scope: {path}")
            }
            Component::Prefix(_) | Component::RootDir => {
                bail!("rooted paths are not allowed in parallel scope: {path}")
            }
            Component::CurDir => {
                bail!("current-directory components are not allowed in parallel scope: {path}")
            }
            Component::Normal(_) => {}
        }
    }
    Ok(candidate.to_path_buf())
}

fn parallel_scope_path_is_existing_directory(path: &str, config: &EffectiveConfig) -> Result<bool> {
    let normalized = validate_parallel_scope_path(path, &config.workspace.extra_write_roots)?;
    let resolved = if normalized.is_absolute() {
        normalized
    } else {
        config.working_directory.join(normalized)
    };
    Ok(resolved.is_dir())
}

fn validate_agent_reference(
    agent_id: &str,
    required_capabilities: &[Capability],
    config: &EffectiveConfig,
) -> Result<()> {
    if agent_id == COUNCIL_WORKFLOW_AGENT_ID {
        if config
            .council
            .presets
            .get(&config.council.default_preset)
            .map(BTreeMap::is_empty)
            .unwrap_or(true)
        {
            bail!("council workflow has no configured councillors");
        }
        return Ok(());
    }
    let agent = config
        .agents
        .get(agent_id)
        .ok_or_else(|| anyhow!("decision references unknown agent {agent_id}"))?;
    if !agent.enabled {
        bail!("decision references disabled agent {agent_id}");
    }
    for capability in required_capabilities {
        if !agent.has_capability(capability) {
            bail!(
                "agent {agent_id} lacks required capability {:?}",
                capability
            );
        }
    }
    Ok(())
}

pub fn build_orchestrator_prompt(config: &EffectiveConfig) -> String {
    // No MCP advertisement without a recorded snapshot (replay-safe default).
    build_orchestrator_prompt_with_mcp(config, &ToolCatalog::default())
}

/// Build the orchestrator prompt, advertising MCP tools from a **recorded
/// snapshot** (ADR-001). `mcp_catalog` must come from the `mcp_catalog_snapshot`
/// event, never a live supervisor handle, so the prompt is deterministic and
/// replay-stable. The orchestrator module deliberately never imports the live
/// supervisor handle type (guard test
/// `orchestrator_prompt_never_reads_live_mcp_handle`).
pub fn build_orchestrator_prompt_with_mcp(
    config: &EffectiveConfig,
    mcp_catalog: &ToolCatalog,
) -> String {
    let base = config
        .agents
        .get("orchestrator")
        .map(|agent| agent.instructions.trim())
        .filter(|instructions| !instructions.is_empty())
        .unwrap_or("Own the run plan and route work through enabled specialized agents.");
    let mut lines = vec![
        base.to_string(),
        String::new(),
        "Enabled specialized agents:".to_string(),
    ];

    let mut enabled_agents = config
        .agents
        .values()
        .filter(|agent| agent.enabled && agent.id != "orchestrator")
        .collect::<Vec<_>>();
    enabled_agents.sort_by(|left, right| left.id.cmp(&right.id));

    if enabled_agents.is_empty() {
        lines.push("- none; ask the user to enable an agent before delegating.".to_string());
    } else {
        for agent in enabled_agents {
            lines.push(agent_routing_line(agent));
        }
    }

    lines.extend([
        String::new(),
        "Available harness workflows:".to_string(),
        format!(
            "- {COUNCIL_WORKFLOW_AGENT_ID}: serial council review using preset `{}`. Use only for architecture, security, data integrity, difficult review, high-risk decisions, or when the user explicitly asks for council review.",
            config.council.default_preset
        ),
    ]);

    // Advertise MCP tools from the recorded snapshot (ADR-001). Omitted entirely
    // when no servers/tools were snapshotted, so prompts for MCP-disabled or
    // server-less runs are unchanged.
    let mcp_lines: Vec<String> = mcp_catalog
        .servers
        .iter()
        .flat_map(|server| {
            server.tools.iter().map(move |tool| {
                let description = tool
                    .description
                    .as_deref()
                    .unwrap_or("(no description)")
                    .trim();
                format!("- {}/{}: {description}", server.server, tool.name)
            })
        })
        .collect();
    if !mcp_lines.is_empty() {
        lines.push(String::new());
        lines.push(
            "Available MCP tools (invoke via a CallMcpTool action through a specialist agent):"
                .to_string(),
        );
        lines.extend(mcp_lines);
    }

    lines.extend([
        String::new(),
        "Routing rules:".to_string(),
        "- Delegate only to enabled agents listed above.".to_string(),
        "- Route to council only for high-risk decisions or explicit user council requests."
            .to_string(),
        "- Do not route to disabled or unknown agents.".to_string(),
        "- Keep required_capabilities no broader than the next step needs.".to_string(),
        "- Ask a clarifying question when the next safe step is ambiguous, and include 2-4 concise recommended answers as clarifying_options.".to_string(),
        "- Give each clarifying_option a short label plus an optional one-sentence description that explains the trade-off of choosing it.".to_string(),
        "- Set recommended_option_id to the strongest option id, or leave it null when no option stands out.".to_string(),
        "- Set multi_select to true only when the user can sensibly pick several of the clarifying_options at once; otherwise leave it false (single choice).".to_string(),
        "- Do not add a custom, other, or free-text option; the app always provides its own custom text answer path.".to_string(),
        "- On the first turn, write `reason` as a one-line restatement of your interpreted goal for this run, and `plan` as a short list of concrete approach bullets; the user may see this as an intent confirmation before any file is changed.".to_string(),
        "- Mark the run complete only when the original user request is satisfied.".to_string(),
    ]);
    if config.features.parallel_step_groups && config.limits.max_parallel_agent_steps > 0 {
        lines.extend([
            "- You may return schema_version 2 with next_step.kind = \"parallel_group\" only when child file scopes are exact, disjoint, and safe to run concurrently.".to_string(),
            format!(
                "- Parallel groups may contain at most {} child steps.",
                config.limits.max_parallel_agent_steps
            ),
            "- Every parallel child needs a step_label, agent, instruction, required_capabilities, and file_scope with exact write_files plus read_roots.".to_string(),
            "- Do not put project-wide mutation commands, formatters, dependency installs, code generation, migrations, whole-suite tests, or VCS actions inside a parallel group.".to_string(),
        ]);
    } else {
        lines.push(
            "- Parallel step groups are disabled; return schema_version 1 sequential decisions with next_agent only."
                .to_string(),
        );
    }
    if config.features.execution_graph && config.limits.max_parallel_agent_steps > 0 {
        lines.extend([
            "- You may return schema_version 3 with next_step.kind = \"dag\" when the work forms a dependency graph: independent nodes run concurrently while dependent nodes wait on their predecessors.".to_string(),
            "- Every dag node needs a node_id, step_label, agent, instruction, required_capabilities, and file_scope with exact write_files plus read_roots.".to_string(),
            "- Declare an edge { from, to, kind } for each dependency; use kind \"data_dependency\" when the consumer reads the producer's output, or \"semantic_gate\" when ordering is required without a data handoff.".to_string(),
            "- Nodes that can run concurrently (no edge path between them) must have disjoint write_files; nodes on a dependency chain may reuse the same write file sequentially.".to_string(),
            format!(
                "- At most {} dag nodes run concurrently; the graph itself may contain more nodes than that.",
                config.limits.max_parallel_agent_steps
            ),
            "- Command-capable nodes run in isolation; keep project-wide mutations, formatters, dependency installs, code generation, migrations, whole-suite tests, and VCS actions in their own dag node.".to_string(),
        ]);
    }

    lines.join("\n")
}

fn agent_routing_line(agent: &AgentProfile) -> String {
    let capabilities = agent
        .capabilities
        .iter()
        .map(|capability| format!("{capability:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(",");
    let guidance = agent
        .orchestrator_description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
        .map(str::trim)
        .unwrap_or_else(|| default_delegation_guidance(agent));
    format!(
        "- {id} ({name}): capabilities=[{capabilities}], runtime={runtime}, model={model}. Use when: {guidance}",
        id = agent.id,
        name = agent.name,
        runtime = agent.runtime,
        model = agent.model,
    )
}

fn default_delegation_guidance(agent: &AgentProfile) -> &'static str {
    if agent.has_capability(&Capability::Review) {
        "review completed work, verification evidence, and regressions; do not edit files"
    } else if agent.has_capability(&Capability::Edit) {
        "apply scoped implementation changes and verify them through harness-owned actions"
    } else if agent.has_capability(&Capability::Challenge) {
        "challenge architecture, security, data integrity, and high-risk decisions before implementation"
    } else if agent.has_capability(&Capability::Answer) {
        "answer focused questions or research requests from available context"
    } else if agent.has_capability(&Capability::Read) {
        "gather repository context and summarize findings without changing files"
    } else {
        "perform only the explicitly configured capability-bounded work"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use tempfile::tempdir;

    fn outcome(command: &str, exit_code: Option<i64>) -> CommandOutcome {
        CommandOutcome {
            command: command.to_string(),
            exit_code,
            excerpt: None,
        }
    }

    #[test]
    fn derive_grade_verdict_passes_when_canonical_check_exits_zero() {
        let verdict = derive_grade_verdict(&[outcome("cargo test", Some(0))]);
        assert_eq!(verdict.outcome, GradeOutcome::Pass);
        assert_eq!(verdict.command.as_deref(), Some("cargo test"));
        assert_eq!(verdict.exit_code, Some(0));
        assert!(verdict.critique.is_none());
    }

    #[test]
    fn derive_grade_verdict_fails_with_critique_on_nonzero() {
        let verdict = derive_grade_verdict(&[CommandOutcome {
            command: "cargo test".to_string(),
            exit_code: Some(1),
            excerpt: Some("test foo ... FAILED".to_string()),
        }]);
        assert_eq!(verdict.outcome, GradeOutcome::Fail);
        assert_eq!(verdict.exit_code, Some(1));
        let critique = verdict.critique.expect("fail carries a critique");
        assert!(
            critique.contains("cargo test"),
            "critique names the command"
        );
        assert!(
            critique.contains("exit 1"),
            "critique carries the exit code"
        );
        assert!(critique.contains("FAILED"), "critique carries the excerpt");
    }

    #[test]
    fn derive_grade_verdict_skips_when_no_canonical_command_ran() {
        // Non-canonical command only → Skip (the echo is ignored).
        assert_eq!(
            derive_grade_verdict(&[outcome("echo hi", Some(0))]).outcome,
            GradeOutcome::Skip
        );
        // Empty set → Skip.
        assert_eq!(derive_grade_verdict(&[]).outcome, GradeOutcome::Skip);
    }

    #[test]
    fn derive_grade_verdict_fails_if_any_canonical_command_nonzero() {
        let verdict = derive_grade_verdict(&[
            outcome("cargo fmt --check", Some(0)),
            outcome("cargo test", Some(1)),
        ]);
        assert_eq!(verdict.outcome, GradeOutcome::Fail);
        assert_eq!(verdict.command.as_deref(), Some("cargo test"));
    }

    #[test]
    fn derive_grade_verdict_passes_when_all_canonical_commands_zero() {
        let verdict = derive_grade_verdict(&[
            outcome("cargo clippy --all-targets", Some(0)),
            outcome("cargo test", Some(0)),
        ]);
        assert_eq!(verdict.outcome, GradeOutcome::Pass);
    }

    #[test]
    fn derive_grade_verdict_treats_missing_exit_code_as_fail() {
        // A canonical command that produced no completion is not a clean pass.
        let verdict = derive_grade_verdict(&[outcome("cargo test", None)]);
        assert_eq!(verdict.outcome, GradeOutcome::Fail);
        assert!(verdict
            .critique
            .expect("critique present")
            .contains("no exit code"));
    }

    #[test]
    fn run_state_is_terminal_truth_table() {
        // Terminal: the run has finished.
        for state in [
            RunState::Completed,
            RunState::Failed,
            RunState::Interrupted,
            RunState::LimitReached,
        ] {
            assert!(state.is_terminal(), "{state:?} should be terminal");
        }
        // In-flight: the run is not finished.
        for state in [
            RunState::Idle,
            RunState::Planning,
            RunState::Running,
            RunState::WaitingForUser,
        ] {
            assert!(!state.is_terminal(), "{state:?} should not be terminal");
        }
    }

    fn parallel_enabled_config(max_parallel_agent_steps: u32) -> crate::config::EffectiveConfig {
        let dir = tempdir().unwrap();
        let mut config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        config.features.parallel_step_groups = true;
        config.limits.max_parallel_agent_steps = max_parallel_agent_steps;
        config
    }

    fn parallel_child(label: &str, write_file: &str) -> ParallelChildStepPlan {
        ParallelChildStepPlan {
            step_label: label.to_string(),
            agent: "fixer".to_string(),
            instruction: format!("Fix {write_file}."),
            required_capabilities: vec![Capability::Read, Capability::Edit],
            file_scope: ParallelFileScope {
                write_files: vec![write_file.to_string()],
                read_roots: vec!["src".to_string()],
            },
        }
    }

    fn validation_config() -> crate::config::EffectiveConfig {
        let dir = tempdir().unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap()
    }

    // ── MCP tool-catalog advertisement (task_07) ──

    fn mcp_catalog_fixture() -> ToolCatalog {
        use crate::mcp::{McpTool, ToolCatalogServer};
        let tool = |name: &str, desc: &str| McpTool {
            name: name.to_string(),
            description: Some(desc.to_string()),
            input_schema: serde_json::json!({ "type": "object" }),
            annotations: None,
        };
        ToolCatalog {
            servers: vec![
                ToolCatalogServer {
                    server: "filesystem".to_string(),
                    tools: vec![tool("read_file", "Read a file")],
                },
                ToolCatalogServer {
                    server: "search".to_string(),
                    tools: vec![tool("web_search", "Search the web")],
                },
            ],
        }
    }

    #[test]
    fn orchestrator_prompt_lists_snapshot_tools_with_servers() {
        let config = validation_config();
        let prompt = build_orchestrator_prompt_with_mcp(&config, &mcp_catalog_fixture());
        assert!(prompt.contains("Available MCP tools"));
        assert!(prompt.contains("- filesystem/read_file: Read a file"));
        assert!(prompt.contains("- search/web_search: Search the web"));
    }

    #[test]
    fn orchestrator_prompt_omits_mcp_section_for_empty_snapshot() {
        let config = validation_config();
        let prompt = build_orchestrator_prompt_with_mcp(&config, &ToolCatalog::default());
        assert!(!prompt.contains("Available MCP tools"));
        // The no-arg builder also omits it (empty default catalog).
        assert!(!build_orchestrator_prompt(&config).contains("Available MCP tools"));
    }

    #[test]
    fn orchestrator_prompt_is_byte_identical_across_replay() {
        let config = validation_config();
        let catalog = mcp_catalog_fixture();
        // Simulate record → replay: serialize the catalog into the event payload
        // and deserialize it back, then rebuild the prompt from the reconstructed
        // snapshot. A pure builder over a serde-stable snapshot is replay-safe.
        let payload = serde_json::to_value(&catalog).unwrap();
        let replayed: ToolCatalog = serde_json::from_value(payload).unwrap();
        let live = build_orchestrator_prompt_with_mcp(&config, &catalog);
        let from_replay = build_orchestrator_prompt_with_mcp(&config, &replayed);
        assert_eq!(live, from_replay);
    }

    #[test]
    fn orchestrator_prompt_never_reads_live_mcp_handle() {
        // Guard (mirrors `colors_live_only_in_theme_module`): the orchestrator
        // module advertises tools from a recorded snapshot only, so it must never
        // name the live supervisor handle type — that would let live, non-replayable
        // state into the prompt. `concat!` keeps the needle out of this test's own
        // source.
        let needle = concat!("Mcp", "Handle");
        let source = include_str!("mod.rs");
        assert!(
            !source.contains(needle),
            "src/orchestrator/mod.rs must not reference the live MCP handle type; \
             advertise tools from the recorded ToolCatalog snapshot instead"
        );
    }

    fn clarification_option(id: &str, label: &str) -> ClarificationOption {
        ClarificationOption {
            id: id.to_string(),
            label: label.to_string(),
            description: None,
        }
    }

    fn waiting_decision(
        clarifying_options: Vec<ClarificationOption>,
        recommended_option_id: Option<&str>,
    ) -> OrchestratorDecision {
        OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::WaitingForUser,
            plan: vec!["Clarify the target.".to_string()],
            next_agent: None,
            next_step: None,
            reason: "The requested target is ambiguous.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "User provides clarification.".to_string(),
            clarifying_question: Some("Which target should this run prioritize?".to_string()),
            clarifying_options,
            recommended_option_id: recommended_option_id.map(str::to_string),
            multi_select: false,
            final_summary: None,
        }
    }

    fn valid_clarification_options(count: usize) -> Vec<ClarificationOption> {
        (0..count)
            .map(|index| {
                clarification_option(
                    &format!("option_{}", index + 1),
                    &format!("Option {}", index + 1),
                )
            })
            .collect()
    }

    fn continue_decision_without_options() -> OrchestratorDecision {
        OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Read context.".to_string()],
            next_agent: Some("explorer".to_string()),
            next_step: None,
            reason: "Need repository context.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Explorer returns context.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        }
    }

    fn complete_decision_without_options() -> OrchestratorDecision {
        OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Complete,
            plan: Vec::new(),
            next_agent: None,
            next_step: None,
            reason: "The run is complete.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Run complete.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: Some("Completed the run.".to_string()),
        }
    }

    #[test]
    fn parses_delimited_orchestrator_decision() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Read context".to_string()],
            next_agent: Some("explorer".to_string()),
            next_step: None,
            reason: "Need repository context before editing.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Context gathered.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };
        let wrapped = wrap_json_contract(&decision).unwrap();
        assert_eq!(parse_orchestrator_decision(&wrapped).unwrap(), decision);
    }

    #[test]
    fn parses_delimiters_with_missing_angle_bracket() {
        let raw = r#"<<<MULTIAGENT_JSON_START>>{
  "schema_version": 1,
  "decision_id": "01KT7K2ZEMTHX0ZZBN19SW03C7-dec-01",
  "run_id": "01KT7K2ZEMTHX0ZZBN19SW03C7",
  "status": "continue",
  "plan": [
    "Read repository metadata/files before drafting the README."
  ],
  "next_agent": "explorer",
  "reason": "The orchestrator lacks read capability; delegate to a read-capable agent.",
  "required_capabilities": ["read"],
  "stop_condition": "Explorer returns sufficient project context.",
  "clarifying_question": null,
  "final_summary": null
}
<<<MULTIAGENT_JSON_END>>"#;

        let decision = parse_orchestrator_decision(raw).unwrap();

        assert_eq!(decision.next_agent.as_deref(), Some("explorer"));
        assert_eq!(decision.required_capabilities, vec![Capability::Read]);
    }

    #[test]
    fn parses_decision_omitting_structured_clarification_fields() {
        let raw = r#"{
  "schema_version": 1,
  "decision_id": "decision",
  "run_id": "run",
  "status": "continue",
  "plan": ["Read context."],
  "next_agent": "explorer",
  "next_step": null,
  "reason": "Need repository context.",
  "required_capabilities": ["read"],
  "stop_condition": "Explorer returns context.",
  "clarifying_question": null,
  "final_summary": null
}"#;

        let decision = parse_orchestrator_decision(raw).unwrap();

        assert!(decision.clarifying_options.is_empty());
        assert_eq!(decision.recommended_option_id, None);
    }

    #[test]
    fn parses_waiting_decision_with_structured_clarification_options() {
        let raw = r#"{
  "schema_version": 1,
  "decision_id": "decision",
  "run_id": "run",
  "status": "waiting_for_user",
  "plan": ["Clarify the target."],
  "next_agent": null,
  "next_step": null,
  "reason": "The requested target is ambiguous.",
  "required_capabilities": [],
  "stop_condition": "User provides clarification.",
  "clarifying_question": "Which target should this run prioritize?",
  "clarifying_options": [
    {
      "id": "tests",
      "label": "Prioritize tests",
      "description": "Focus on regression coverage first."
    },
    {
      "id": "docs",
      "label": "Prioritize docs"
    }
  ],
  "recommended_option_id": "tests",
  "final_summary": null
}"#;

        let decision = parse_orchestrator_decision(raw).unwrap();

        assert_eq!(decision.status, DecisionStatus::WaitingForUser);
        assert_eq!(decision.clarifying_options.len(), 2);
        assert_eq!(decision.clarifying_options[0].id, "tests");
        assert_eq!(decision.clarifying_options[0].label, "Prioritize tests");
        assert_eq!(
            decision.clarifying_options[0].description.as_deref(),
            Some("Focus on regression coverage first.")
        );
        assert_eq!(decision.clarifying_options[1].id, "docs");
        assert_eq!(decision.clarifying_options[1].description, None);
        assert_eq!(decision.recommended_option_id.as_deref(), Some("tests"));
    }

    #[test]
    fn serializes_decision_with_structured_clarification_fields() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::WaitingForUser,
            plan: vec!["Clarify the target.".to_string()],
            next_agent: None,
            next_step: None,
            reason: "The requested target is ambiguous.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "User provides clarification.".to_string(),
            clarifying_question: Some("Which target should this run prioritize?".to_string()),
            clarifying_options: vec![
                ClarificationOption {
                    id: "tests".to_string(),
                    label: "Prioritize tests".to_string(),
                    description: Some("Focus on regression coverage first.".to_string()),
                },
                ClarificationOption {
                    id: "docs".to_string(),
                    label: "Prioritize docs".to_string(),
                    description: None,
                },
            ],
            recommended_option_id: Some("tests".to_string()),
            multi_select: false,
            final_summary: None,
        };

        let value = serde_json::to_value(&decision).unwrap();

        assert!(value.get("clarifying_options").is_some());
        assert_eq!(
            value
                .get("clarifying_options")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            value
                .get("recommended_option_id")
                .and_then(|value| value.as_str()),
            Some("tests")
        );
    }

    #[test]
    fn validates_waiting_decision_with_two_clarification_options() {
        let config = validation_config();
        let decision = waiting_decision(valid_clarification_options(2), Some("option_1"));

        validate_orchestrator_decision(&decision, &config).unwrap();
    }

    #[test]
    fn validates_waiting_decision_with_four_clarification_options() {
        let config = validation_config();
        let decision = waiting_decision(valid_clarification_options(4), Some("option_4"));

        validate_orchestrator_decision(&decision, &config).unwrap();
    }

    #[test]
    fn rejects_waiting_decision_with_invalid_clarification_option_count() {
        let config = validation_config();

        for option_count in [0, 1, 5] {
            let decision = waiting_decision(valid_clarification_options(option_count), None);
            let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

            assert!(
                error
                    .to_string()
                    .contains(&format!("2-4 clarifying_options, found {option_count}")),
                "unexpected error for {option_count} options: {error}"
            );
        }
    }

    #[test]
    fn rejects_waiting_decision_with_blank_option_id() {
        let config = validation_config();
        let decision = waiting_decision(
            vec![
                clarification_option(" ", "Use the CLI path"),
                clarification_option("web", "Use the web path"),
            ],
            None,
        );

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("clarifying_options[0] is missing id"));
    }

    #[test]
    fn rejects_waiting_decision_with_blank_option_label() {
        let config = validation_config();
        let decision = waiting_decision(
            vec![
                clarification_option("cli", " "),
                clarification_option("web", "Use the web path"),
            ],
            None,
        );

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("clarifying_options[0] is missing label"));
    }

    #[test]
    fn rejects_waiting_decision_with_duplicate_option_ids() {
        let config = validation_config();
        let decision = waiting_decision(
            vec![
                clarification_option("cli", "Use the CLI path"),
                clarification_option("cli", "Use the terminal path"),
            ],
            None,
        );

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("clarifying_options contains duplicate id 'cli'"));
    }

    #[test]
    fn rejects_waiting_decision_with_invalid_recommended_option_id() {
        let config = validation_config();
        let decision = waiting_decision(
            vec![
                clarification_option("cli", "Use the CLI path"),
                clarification_option("web", "Use the web path"),
            ],
            Some("docs"),
        );

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("recommended_option_id 'docs' does not match any clarifying_options id"));
    }

    #[test]
    fn non_waiting_decisions_do_not_require_clarification_options() {
        let config = validation_config();

        validate_orchestrator_decision(&continue_decision_without_options(), &config).unwrap();
        validate_orchestrator_decision(&complete_decision_without_options(), &config).unwrap();
    }

    #[test]
    fn waiting_decision_still_rejects_next_agent() {
        let config = validation_config();
        let mut decision = waiting_decision(valid_clarification_options(2), Some("option_1"));
        decision.next_agent = Some("explorer".to_string());

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("non-continue decision must not contain a next step"));
    }

    #[test]
    fn delimited_extraction_respects_braces_inside_strings() {
        let result = AgentResult {
            schema_version: 1,
            agent: "explorer".to_string(),
            step_id: "03".to_string(),
            status: AgentResultStatus::Completed,
            summary: "Found literals like } and { in project docs.".to_string(),
            findings: Vec::new(),
            changed_files: Vec::new(),
            commands: Vec::new(),
            verification: Vec::new(),
            blocker: None,
            artifacts: Vec::new(),
        };
        let raw = format!(
            "prefix\n<<<MULTIAGENT_JSON_START>>\n{}\n<<<MULTIAGENT_JSON_END>>>>\ntrailing",
            serde_json::to_string_pretty(&result).unwrap()
        );

        assert_eq!(parse_agent_result(&raw).unwrap(), result);
    }

    #[test]
    fn validates_required_capability_against_selected_agent() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: vec![],
            next_agent: Some("explorer".to_string()),
            next_step: None,
            reason: "Need edit.".to_string(),
            required_capabilities: vec![Capability::Edit],
            stop_condition: "Done.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };
        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();
        assert!(error.to_string().contains("lacks required capability"));
    }

    #[test]
    fn normalizes_v1_next_agent_to_single_agent_step() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some("explorer".to_string()),
            next_step: None,
            reason: "Need context.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Explorer returns context.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        let next_step = decision.normalized_next_step().unwrap().unwrap();

        assert!(matches!(
            next_step,
            DecisionNextStep::SingleAgent(SingleAgentStepPlan { agent, .. }) if agent == "explorer"
        ));
    }

    #[test]
    fn rejects_v2_decision_with_both_next_agent_and_next_step() {
        let decision = OrchestratorDecision {
            schema_version: 2,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some("explorer".to_string()),
            next_step: Some(DecisionNextStep::SingleAgent(SingleAgentStepPlan {
                agent: "explorer".to_string(),
                instruction: None,
                required_capabilities: vec![Capability::Read],
            })),
            reason: "Need context.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Explorer returns context.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        let error = decision.normalized_next_step().unwrap_err();

        assert!(error.to_string().contains("both next_agent and next_step"));
    }

    #[test]
    fn rejects_parallel_group_when_feature_disabled() {
        let dir = tempdir().unwrap();
        let mut config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        // Force the feature off so this stays hermetic regardless of the
        // developer's home config (config_path: None reads it), mirroring
        // parallel_enabled_config which forces it on.
        config.features.parallel_step_groups = false;
        let decision = OrchestratorDecision {
            schema_version: 2,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: None,
            next_step: Some(DecisionNextStep::ParallelGroup(ParallelGroupPlan {
                group_id: "group".to_string(),
                reason: "Disjoint scopes.".to_string(),
                steps: vec![
                    ParallelChildStepPlan {
                        step_label: "fix a".to_string(),
                        agent: "fixer".to_string(),
                        instruction: "Fix a.".to_string(),
                        required_capabilities: vec![Capability::Read, Capability::Edit],
                        file_scope: ParallelFileScope {
                            write_files: vec!["src/a.rs".to_string()],
                            read_roots: vec!["src".to_string()],
                        },
                    },
                    ParallelChildStepPlan {
                        step_label: "fix b".to_string(),
                        agent: "fixer".to_string(),
                        instruction: "Fix b.".to_string(),
                        required_capabilities: vec![Capability::Read, Capability::Edit],
                        file_scope: ParallelFileScope {
                            write_files: vec!["src/b.rs".to_string()],
                            read_roots: vec!["src".to_string()],
                        },
                    },
                ],
            })),
            reason: "Use parallel work.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Group joins.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("parallel step groups are disabled"));
    }

    #[test]
    fn rejects_parallel_group_above_configured_limit() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Too many parallel children.".to_string(),
            steps: vec![
                parallel_child("fix a", "src/a.rs"),
                parallel_child("fix b", "src/b.rs"),
                parallel_child("fix c", "src/c.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("max_parallel_agent_steps"));
    }

    #[test]
    fn rejects_parallel_group_with_overlapping_write_files() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Duplicate write ownership.".to_string(),
            steps: vec![
                parallel_child("fix first", "src/a.rs"),
                parallel_child("fix second", "src/a.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("assigned to both"));
    }

    #[test]
    fn rejects_parallel_group_with_empty_child_scope() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Missing scope.".to_string(),
            steps: vec![
                parallel_child("fix first", "src/a.rs"),
                ParallelChildStepPlan {
                    step_label: "review nothing".to_string(),
                    agent: "reviewer".to_string(),
                    instruction: "Review nothing.".to_string(),
                    required_capabilities: vec![Capability::Read, Capability::Review],
                    file_scope: ParallelFileScope {
                        write_files: Vec::new(),
                        read_roots: Vec::new(),
                    },
                },
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("at least one write_file or read_root"));
    }

    #[test]
    fn rejects_parallel_group_with_directory_write_file() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let mut config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        config.features.parallel_step_groups = true;
        config.limits.max_parallel_agent_steps = 2;
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Directory write scope.".to_string(),
            steps: vec![
                parallel_child("fix directory", "src"),
                parallel_child("fix file", "src/a.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("not directories"));
    }

    #[test]
    fn rejects_parallel_group_with_current_dir_scope_component() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Ambiguous lexical scope.".to_string(),
            steps: vec![
                parallel_child("fix dotted", "src/./a.rs"),
                parallel_child("fix file", "src/b.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("current-directory components"));
    }

    // ── execution graph (DAG) decision schema + validation (task_02) ──

    fn dag_enabled_config(max_parallel_agent_steps: u32) -> crate::config::EffectiveConfig {
        let dir = tempdir().unwrap();
        let mut config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        config.features.execution_graph = true;
        config.limits.max_parallel_agent_steps = max_parallel_agent_steps;
        config
    }

    fn dag_node(node_id: &str, write_file: &str) -> ExecutionNode {
        ExecutionNode {
            node_id: node_id.to_string(),
            step_label: format!("step {node_id}"),
            agent: "fixer".to_string(),
            instruction: format!("Fix {write_file}."),
            required_capabilities: vec![Capability::Read, Capability::Edit],
            file_scope: ParallelFileScope {
                write_files: vec![write_file.to_string()],
                read_roots: vec!["src".to_string()],
            },
        }
    }

    fn dag_edge(from: &str, to: &str) -> ExecutionEdge {
        ExecutionEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind: ExecutionEdgeKind::DataDependency,
        }
    }

    fn dag_continue_decision(schema_version: u32) -> OrchestratorDecision {
        OrchestratorDecision {
            schema_version,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Run the graph.".to_string()],
            next_agent: None,
            next_step: Some(DecisionNextStep::Dag(ExecutionGraph {
                graph_id: "graph".to_string(),
                reason: "Two independent then joined.".to_string(),
                nodes: vec![dag_node("a", "src/a.rs"), dag_node("b", "src/b.rs")],
                edges: vec![dag_edge("a", "b")],
            })),
            reason: "The work forms a dependency graph.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Graph completes.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        }
    }

    #[test]
    fn dag_decision_round_trips_through_serde() {
        let next_step = DecisionNextStep::Dag(ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Diamond.".to_string(),
            nodes: vec![dag_node("a", "src/a.rs"), dag_node("b", "src/b.rs")],
            edges: vec![dag_edge("a", "b")],
        });
        let value = serde_json::to_value(&next_step).unwrap();
        assert_eq!(value["kind"], "dag");
        let parsed: DecisionNextStep = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, next_step);
    }

    #[test]
    fn existing_next_step_variants_still_deserialize_unchanged() {
        let single: DecisionNextStep =
            serde_json::from_str(r#"{"kind":"single_agent","agent":"explorer"}"#).unwrap();
        assert!(matches!(single, DecisionNextStep::SingleAgent(_)));
        let parallel: DecisionNextStep = serde_json::from_str(
            r#"{"kind":"parallel_group","group_id":"g","reason":"r","steps":[]}"#,
        )
        .unwrap();
        assert!(matches!(parallel, DecisionNextStep::ParallelGroup(_)));
    }

    #[test]
    fn dag_normalizes_at_schema_version_3() {
        let decision = dag_continue_decision(3);
        let next_step = decision.normalized_next_step().unwrap().unwrap();
        assert!(matches!(next_step, DecisionNextStep::Dag(_)));
    }

    #[test]
    fn dag_under_schema_version_2_is_rejected() {
        let decision = dag_continue_decision(2);
        let error = decision.normalized_next_step().unwrap_err();
        assert!(error
            .to_string()
            .contains("schema_version 2 cannot select a dag next_step"));
    }

    #[test]
    fn dag_under_schema_version_1_is_rejected() {
        let decision = dag_continue_decision(1);
        let error = decision.normalized_next_step().unwrap_err();
        assert!(error
            .to_string()
            .contains("schema_version 1 cannot select a dag next_step"));
    }

    #[test]
    fn schema_version_2_error_string_is_unchanged_for_next_agent() {
        // The existing v2 message must stay byte-identical (replay/test contract).
        let mut decision = dag_continue_decision(2);
        decision.next_step = None;
        decision.next_agent = Some("explorer".to_string());
        let error = decision.normalized_next_step().unwrap_err();
        assert!(error
            .to_string()
            .contains("schema_version 2 must use next_step instead of next_agent"));
    }

    #[test]
    fn validate_execution_graph_accepts_valid_diamond() {
        // a -> {b, c} -> d; b and c are concurrent but write disjoint files. The
        // graph (4 nodes) is larger than the concurrency ceiling (2), which is
        // legal — the ceiling bounds simultaneity, not total node count.
        let config = dag_enabled_config(2);
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Diamond.".to_string(),
            nodes: vec![
                dag_node("a", "src/a.rs"),
                dag_node("b", "src/b.rs"),
                dag_node("c", "src/c.rs"),
                dag_node("d", "src/d.rs"),
            ],
            edges: vec![
                dag_edge("a", "b"),
                dag_edge("a", "c"),
                dag_edge("b", "d"),
                dag_edge("c", "d"),
            ],
        };
        validate_execution_graph(&graph, &config).unwrap();
    }

    #[test]
    fn validate_execution_graph_rejects_cycle() {
        let config = dag_enabled_config(2);
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Cyclic.".to_string(),
            nodes: vec![dag_node("a", "src/a.rs"), dag_node("b", "src/b.rs")],
            edges: vec![dag_edge("a", "b"), dag_edge("b", "a")],
        };
        let error = validate_execution_graph(&graph, &config).unwrap_err();
        assert!(error.to_string().contains("contains a cycle"));
    }

    #[test]
    fn validate_execution_graph_rejects_unknown_edge_endpoint() {
        let config = dag_enabled_config(2);
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Dangling edge.".to_string(),
            nodes: vec![dag_node("a", "src/a.rs")],
            edges: vec![dag_edge("a", "ghost")],
        };
        let error = validate_execution_graph(&graph, &config).unwrap_err();
        assert!(error.to_string().contains("unknown to node_id ghost"));
    }

    #[test]
    fn validate_execution_graph_rejects_duplicate_node_id() {
        let config = dag_enabled_config(2);
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Duplicate ids.".to_string(),
            nodes: vec![dag_node("a", "src/a.rs"), dag_node("a", "src/b.rs")],
            edges: Vec::new(),
        };
        let error = validate_execution_graph(&graph, &config).unwrap_err();
        assert!(error.to_string().contains("duplicate node_id a"));
    }

    #[test]
    fn validate_execution_graph_rejects_concurrent_write_overlap() {
        // a and b have no path between them, so they could run concurrently and
        // must not write the same file.
        let config = dag_enabled_config(2);
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Concurrent overlap.".to_string(),
            nodes: vec![
                dag_node("a", "src/shared.rs"),
                dag_node("b", "src/shared.rs"),
            ],
            edges: Vec::new(),
        };
        let error = validate_execution_graph(&graph, &config).unwrap_err();
        assert!(error
            .to_string()
            .contains("may run concurrently but both write"));
    }

    #[test]
    fn validate_execution_graph_accepts_sequential_shared_write_file() {
        // a -> b orders the two nodes, so reusing the same write file is legal.
        let config = dag_enabled_config(2);
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Sequential reuse.".to_string(),
            nodes: vec![
                dag_node("a", "src/shared.rs"),
                dag_node("b", "src/shared.rs"),
            ],
            edges: vec![dag_edge("a", "b")],
        };
        validate_execution_graph(&graph, &config).unwrap();
    }

    #[test]
    fn validate_execution_graph_rejects_edit_node_missing_write_files() {
        let config = dag_enabled_config(2);
        let mut node = dag_node("a", "src/a.rs");
        node.file_scope.write_files = Vec::new();
        node.file_scope.read_roots = vec!["src".to_string()];
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Edit without writes.".to_string(),
            nodes: vec![node],
            edges: Vec::new(),
        };
        let error = validate_execution_graph(&graph, &config).unwrap_err();
        assert!(error.to_string().contains("missing write_files"));
    }

    #[test]
    fn validate_execution_graph_rejects_when_feature_disabled() {
        let mut config = dag_enabled_config(2);
        config.features.execution_graph = false;
        let graph = ExecutionGraph {
            graph_id: "graph".to_string(),
            reason: "Disabled.".to_string(),
            nodes: vec![dag_node("a", "src/a.rs")],
            edges: Vec::new(),
        };
        let error = validate_execution_graph(&graph, &config).unwrap_err();
        assert!(error
            .to_string()
            .contains("disabled by features.execution_graph"));
    }

    #[test]
    fn build_orchestrator_prompt_emits_dag_guidance_only_when_enabled() {
        let mut config = validation_config();
        config.limits.max_parallel_agent_steps = 2;

        config.features.execution_graph = false;
        assert!(!build_orchestrator_prompt(&config).contains("kind = \"dag\""));

        config.features.execution_graph = true;
        assert!(build_orchestrator_prompt(&config).contains("kind = \"dag\""));

        // A zero concurrency ceiling disables the DAG even when the flag is on.
        config.limits.max_parallel_agent_steps = 0;
        assert!(!build_orchestrator_prompt(&config).contains("kind = \"dag\""));
    }

    #[test]
    fn full_dag_decision_parses_and_validates_end_to_end() {
        let config = dag_enabled_config(2);
        let decision = dag_continue_decision(3);
        let wrapped = wrap_json_contract(&decision).unwrap();
        let parsed = parse_orchestrator_decision(&wrapped).unwrap();
        assert_eq!(parsed, decision);
        validate_orchestrator_decision(&parsed, &config).unwrap();
    }

    #[test]
    fn execution_graph_result_round_trips_via_run_step_result() {
        let mut counts = std::collections::BTreeMap::new();
        counts.insert("completed".to_string(), 1u32);
        counts.insert("skipped".to_string(), 1u32);
        counts.insert("failed".to_string(), 1u32);
        let result = RunStepResult::Dag {
            result: ExecutionGraphResult {
                schema_version: 1,
                graph_id: "graph".to_string(),
                run_id: "run".to_string(),
                status: ExecutionGraphStatus::CompletedWithIssues,
                summary: "one succeeded, one failed, one skipped".to_string(),
                node_results: vec![NodeResultRef {
                    node_id: "a".to_string(),
                    step_label: "step a".to_string(),
                    agent: "fixer".to_string(),
                    file_scope: ParallelFileScope {
                        write_files: vec!["src/a.rs".to_string()],
                        read_roots: vec!["src".to_string()],
                    },
                    status: AgentResultStatus::Completed,
                    result_index: 0,
                }],
                counts,
                changed_files: vec!["src/a.rs".to_string()],
                skipped: vec!["c".to_string()],
                failed: vec!["b".to_string()],
                started_at: "2026-06-17T00:00:00.000Z".to_string(),
                completed_at: "2026-06-17T00:01:00.000Z".to_string(),
            },
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["kind"], "dag");
        let parsed: RunStepResult = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, result);
    }

    #[test]
    fn parses_agent_result() {
        let result = AgentResult::completed("explorer", "03", "Found files.");
        let wrapped = wrap_json_contract(&result).unwrap();
        assert_eq!(parse_agent_result(&wrapped).unwrap(), result);
    }

    #[test]
    fn generated_orchestrator_prompt_lists_enabled_custom_agents() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"
enabled = false

[agents.docs]
runtime = "fake"
model = "docs-model"
capabilities = ["read", "answer"]
instructions = "Answer documentation questions."
orchestrator_description = "Use for official documentation and API lookup."
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("- docs (Docs):"));
        assert!(prompt.contains("Use for official documentation and API lookup."));
        assert!(!prompt.contains("- explorer (Explorer):"));
    }

    #[test]
    fn generated_orchestrator_prompt_lists_council_workflow_with_routing_limit() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("- council: serial council review"));
        assert!(prompt.contains("only for high-risk decisions"));
    }

    #[test]
    fn validates_council_as_workflow_target() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some(COUNCIL_WORKFLOW_AGENT_ID.to_string()),
            next_step: None,
            reason: "High-risk architecture decision needs council review.".to_string(),
            required_capabilities: vec![Capability::Read, Capability::Challenge],
            stop_condition: "Council returns a recommendation.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        validate_orchestrator_decision(&decision, &config).unwrap();
    }

    #[test]
    fn generated_orchestrator_prompt_includes_structured_clarification_guidance() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("Ask a clarifying question when the next safe step is ambiguous"));
        assert!(prompt.contains("2-4 concise recommended answers as clarifying_options"));
        assert!(prompt.contains("Set recommended_option_id to the strongest option id"));
        assert!(prompt.contains("the app always provides its own custom text answer path"));
        assert!(!prompt.contains("question tool"));
        assert!(!prompt.contains("ask_user"));
    }

    #[test]
    fn generated_orchestrator_prompt_includes_turn_one_intent_nudge() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("On the first turn, write `reason` as a one-line restatement"));
        assert!(prompt.contains("`plan` as a short list of concrete approach bullets"));
        // The nudge is additive — the other routing rules are untouched.
        assert!(prompt
            .contains("Mark the run complete only when the original user request is satisfied."));
    }

    #[test]
    fn generated_orchestrator_prompt_excludes_disabled_librarian_by_default() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(!prompt.contains("- librarian (Librarian):"));
    }

    #[test]
    fn generated_orchestrator_prompt_excludes_disabled_designer_by_default() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(!prompt.contains("- designer (Designer):"));
    }

    #[test]
    fn generated_orchestrator_prompt_includes_designer_guidance_when_enabled() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.designer]
runtime = "fake"
enabled = true
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("- designer (Designer):"));
        assert!(prompt.contains("do not use for backend-only"));
    }
}
