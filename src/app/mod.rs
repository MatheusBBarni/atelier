pub mod chat;
pub mod git;

use self::chat::{ChatItemView, ChatProjection};
use self::git::{fetch_git_context, GitContext};
use crate::actions::{
    execute_action_request, is_vcs_mutation, vcs_action_explicitly_requested, ActionDecision,
    ActionExecutionContext, ActionKind, ActionRequest, ActionResult, ActionStatus, GateOutcome,
    RiskNote, RiskTier, TrustTarget,
};
use crate::config::{
    AgentProfile, AgentPromptMetadata, ApprovalMode, Capability, CouncilExecutionMode,
    CouncilMemberProfile, EffectiveConfig, Limit,
};
use crate::governance::{
    GovernanceAnswer, GovernanceDecisionView, GovernanceKind, GOVERNANCE_DECISION_REQUESTED,
    GOVERNANCE_DECISION_RESOLVED,
};
use crate::history::{
    dangling_run_from_events, derive_goal, derive_outcome, hash_events_digest,
    last_recorded_head_sha, list_session_summaries, HistoryEvent, HistoryStore,
    RunInterruptedPayload, SessionResumedPayload, RESUME_DRIFT_ACK_KIND, RUN_INTERRUPTED_KIND,
    SESSION_RESUMED_KIND,
};
use crate::hooks::{
    normalize, public_name_for_kind, try_dispatch, ActorCtx, DroppedHookCounter, HookDispatch,
    HookLifecycleRecord, MatchedHandler, PayloadDetail,
};
use crate::ids::new_id;
use crate::orchestrator::{
    agent_results, validate_orchestrator_decision, AgentResult, AgentResultStatus,
    ClarificationOption, DecisionNextStep, DecisionStatus, ParallelBlockedScope,
    ParallelChildResultRef, ParallelFailedScope, ParallelFileScope, ParallelGroupPlan,
    ParallelGroupResult, ParallelGroupStatus, RunState, RunStepResult, COUNCIL_WORKFLOW_AGENT_ID,
};
use crate::runtime::{
    check_all_runtime_availability, execute_runtime_step, execute_runtime_step_streaming,
    ParallelRuntimeContext, ParallelSiblingContext, RuntimeAvailability, RuntimeAvailabilityStatus,
    RuntimeEvent, RuntimeEventSink, RuntimeHistoryEvent, RuntimeOutput, RuntimeRecentAction,
    RuntimeRecentContext, RuntimeRecentFile, RuntimeRequest, RuntimeStreamDelta,
    RUNTIME_EVENT_CHANNEL_CAPACITY,
};
use crate::skills::{
    self, CompiledPrompt, LoadedSkillMetadata, SkillLoadError, SkillPromptContext,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::env;
use std::future::{pending, Future};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

const LARGE_ACTION_CONTENT_BYTES: usize = 8 * 1024;
const SEARCH_TEXT_HISTORY_PREVIEW_MATCHES: usize = 8;
const RUNTIME_HISTORY_EVENT_LIMIT: usize = 100;
const RUNTIME_HISTORY_PAYLOAD_DEPTH: usize = 3;
const RUNTIME_HISTORY_PAYLOAD_FIELDS: usize = 20;
const RUNTIME_HISTORY_PAYLOAD_ITEMS: usize = 20;
const RUNTIME_HISTORY_STRING_CHARS: usize = 512;
const RUNTIME_RECENT_CONTEXT_LIMIT: usize = 20;
const LIVE_STREAM_CONTENT_LIMIT: usize = 16 * 1024;
const STREAM_COALESCE_BYTES: usize = 2 * 1024;
const STREAM_COALESCE_INTERVAL: Duration = Duration::from_millis(250);
const WORKFLOW_COMMAND: &str = "/workflow";
const WORKFLOW_USAGE: &str = "usage: /workflow <prompt>";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub model: String,
    pub effort: String,
    pub thinking: bool,
    pub capabilities: Vec<String>,
    pub availability: Option<RuntimeAvailability>,
    pub status: String,
}

/// Activity classification for a roster row (ADR-001/ADR-002). Computed in the
/// app layer by the roster builder and pre-baked into [`RosterRow`] so the
/// renderer never reads a clock. `snake_case` on the wire mirrors
/// [`LiveStepStatus`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    Active,
    NeedsInput,
    Stalled,
    Idle,
}

/// Live-activity-first view-model for one agent in the roster (ADR-003). Joins
/// identity ([`AgentView`]) with liveness ([`LiveStepView`]) and carries
/// pre-formatted, clock-free values for the pure renderer. Built by the roster
/// builder and stored on [`AppState::roster_rows`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterRow {
    /// Stable identity key (`AgentView.id`).
    pub agent_id: String,
    pub name: String,
    /// Canonical-order index into the theme accent palette (ADR-005); decoupled
    /// from render-time position so the `NeedsInput` pin cannot recolor an agent.
    pub accent_index: usize,
    pub activity: ActivityState,
    /// `"runtime/model"`.
    pub runtime_model: String,
    pub effort: String,
    pub thinking: bool,
    /// Step label, active rows only.
    pub current_step: Option<String>,
    /// Coarse pre-formatted elapsed (e.g. `"1m 20s"`), active rows only.
    pub elapsed: Option<String>,
    /// Existing terminal status labels, preserved.
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppState {
    pub session_id: String,
    pub run_state: RunState,
    pub active_run_id: Option<String>,
    pub session_goal: Option<String>,
    pub config_status: ConfigStatusView,
    pub live_step: Option<LiveStepView>,
    pub live_steps: Vec<LiveStepView>,
    pub pending_approval: Option<PendingApprovalView>,
    /// True only while the *first* approval a user ever hits is pending, so the
    /// render path can show a one-line first-approval explainer at most once
    /// per user (ADR-004). Gated by a persisted latch in `HistoryStore`.
    #[serde(default)]
    pub show_first_approval_explainer: bool,
    pub pending_clarification: Option<PendingClarificationView>,
    pub pending_governance_decision: Option<PendingGovernanceDecisionView>,
    /// A proposed execution graph awaiting whole-plan approval (normal mode,
    /// ADR-005). Distinct from `pending_approval` (per-action) so the two never
    /// collide; `None` outside the DAG plan-approval gate.
    #[serde(default)]
    pub pending_plan_approval: Option<PendingPlanApprovalView>,
    pub agents: Vec<AgentView>,
    /// Live-activity-first roster view-model, rebuilt on every publish (ADR-003).
    /// Empty until the roster builder (task 03) populates it.
    #[serde(default)]
    pub roster_rows: Vec<RosterRow>,
    pub chat_items: Vec<ChatItemView>,
    pub queued_follow_ups: Vec<QueuedFollowUpView>,
    pub events: Vec<String>,
    pub input: String,
    /// Repo + branch of the working directory, refreshed by the git poller
    /// (ADR-006). `None` outside a git repo or while git is unavailable.
    pub git_context: Option<GitContext>,
    /// True when the newest *prior* session (not this one) ended non-terminally,
    /// computed once at startup from `list_session_summaries` (task_13). Drives the
    /// dynamic post-crash hint in the welcome facts; the welcome only renders on an
    /// empty chat, so it self-clears once the user resumes or starts working.
    #[serde(default)]
    pub recoverable_session: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveStepView {
    pub run_id: String,
    pub group_id: Option<String>,
    pub step_id: String,
    pub step_label: Option<String>,
    pub file_scope: Option<ParallelFileScope>,
    pub agent: String,
    pub status: LiveStepStatus,
    pub streams: Vec<LiveStreamView>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveStepStatus {
    Starting,
    Running,
    Streaming,
    WaitingForAction,
    WaitingForApproval,
    Cancelling,
    Interrupted,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedFollowUpView {
    pub id: String,
    pub prompt: String,
    pub created_at: String,
    pub status: QueuedFollowUpStatus,
    pub pause_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueuedFollowUpStatus {
    Pending,
    Paused,
    Replaying,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveStreamView {
    pub stream: String,
    pub content: String,
    pub sequence_end: u32,
    pub final_delta: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigStatusView {
    pub summary: String,
    pub sources: Vec<String>,
    pub preset: Option<String>,
    pub warnings: Vec<String>,
    /// The resolved approval posture (ADR-005), surfaced in `/config` so the
    /// gating mode is discoverable in-app. `#[serde(default)]` keeps older
    /// `config_viewed` payloads deserializing.
    #[serde(default)]
    pub approval_mode: ApprovalMode,
    /// Whether the sub-task DAG capability is enabled (`features.execution_graph`).
    #[serde(default)]
    pub execution_graph_enabled: bool,
    /// The reused concurrency ceiling (`limits.max_parallel_agent_steps`); `0`
    /// disables both flat groups and the DAG.
    #[serde(default)]
    pub max_parallel_agent_steps: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PendingApprovalView {
    pub run_id: String,
    pub group_id: Option<String>,
    pub step_id: String,
    pub action_id: String,
    pub agent: String,
    pub summary: String,
    pub diagnostic: Option<String>,
    /// Risk fields for the rich modal (ADR-003), populated from the action's
    /// `RiskNote` at approval-creation time. `#[serde(default)]` keeps older
    /// snapshots/records deserializing.
    #[serde(default)]
    pub tier: Option<RiskTier>,
    #[serde(default)]
    pub catastrophic: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub resolved_command: Option<String>,
    #[serde(default)]
    pub diff: Option<String>,
    #[serde(default)]
    pub affected_paths: Vec<String>,
    #[serde(default)]
    pub boundary_crossed: Option<String>,
    #[serde(default)]
    pub reversible: Option<bool>,
    /// The exact target the "approve-and-trust" option would grant; `None` hides
    /// that option (e.g. for catastrophic actions).
    #[serde(default)]
    pub trust_target: Option<TrustTarget>,
    /// Drift context for the first state-mutating action after a drifted resume
    /// (ADR-004/007): the workspace moved or HEAD changed since this session
    /// paused, so the user must acknowledge before this change. `None` on the
    /// clean common path. Surfaced in the approval modal.
    #[serde(default)]
    pub drift_notice: Option<String>,
    /// MCP trust-legibility fields (task_09), populated for a `CallMcpTool`
    /// awaiting approval: the origin server id, the tool name, the tool's FULL
    /// untruncated description, and the server's current trust tier. `mcp_server`
    /// being `Some` also offers the approve-and-trust action (promote the server).
    #[serde(default)]
    pub mcp_server: Option<String>,
    #[serde(default)]
    pub mcp_tool: Option<String>,
    #[serde(default)]
    pub mcp_description: Option<String>,
    #[serde(default)]
    pub mcp_trusted: bool,
}

impl PendingApprovalView {
    /// Whether the approve-and-trust action is offered: either a session trust
    /// target (commands/writes) or an MCP origin server to promote (task_09).
    /// Catastrophic actions are never trustable and are handled before this.
    pub fn offers_trust(&self) -> bool {
        self.trust_target.is_some() || self.mcp_server.is_some()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingClarificationView {
    pub run_id: String,
    pub question_id: String,
    pub question: String,
    pub options: Vec<ClarificationOption>,
    pub recommended_option_id: Option<String>,
    /// When true the user may select several options at once; the composer
    /// renders checkboxes instead of a single-choice list.
    #[serde(default)]
    pub multi_select: bool,
}

/// The user-facing snapshot of a proposed execution graph awaiting whole-plan
/// approval in `normal` mode (ADR-005). A **distinct** slot from
/// `pending_approval` (per-action) so the two never collide; resolved through
/// the shared accept/reject approval channel (accept is the recommended
/// default). The full graph rendering lives in the Plan chat item (task_06);
/// this view carries just the identity + a one-line summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingPlanApprovalView {
    pub run_id: String,
    pub graph_id: String,
    /// Stable id derived from `graph_id` so an accept/reject re-keys identically.
    pub question_id: String,
    pub summary: String,
}

/// Deterministic clarification/approval question id for an execution graph's
/// whole-plan approval gate, derived from its `graph_id` (ADR-005).
pub(crate) fn plan_question_id(graph_id: &str) -> String {
    format!("plan-approval:{graph_id}")
}

/// The user-facing snapshot of a pending governance decision, mirroring
/// `PendingClarificationView`. The full decision payload lives in `view`; the
/// `decision_id` is what the resolver matches an answer against (ADR-003).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingGovernanceDecisionView {
    pub run_id: String,
    pub decision_id: String,
    pub view: GovernanceDecisionView,
}

#[derive(Clone, Debug)]
pub struct InterruptHandle {
    sender: watch::Sender<u64>,
}

impl InterruptHandle {
    pub fn request_interrupt(&self) {
        let next = sender_next_interrupt_value(&self.sender);
        let _ = self.sender.send(next);
    }
}

/// The user's answer at the approval modal (ADR-004). Replaces the former bare
/// `bool`: "approve" and "approve-and-trust" must never share a keystroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalResolution {
    Deny,
    ApproveOnce,
    ApproveAndTrust,
}

/// The user's answer at the whole-plan approval gate (ADR-005): accept runs the
/// proposed graph; reject carries an optional free-text reason that returns the
/// plan to the orchestrator to re-propose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanApprovalAnswer {
    Accept,
    Reject { reason: Option<String> },
}

impl ApprovalResolution {
    /// Whether the action should run (either approve variant).
    pub fn approves(self) -> bool {
        matches!(
            self,
            ApprovalResolution::ApproveOnce | ApprovalResolution::ApproveAndTrust
        )
    }

    /// Whether the action's target should be added to the session trust list.
    pub fn grants_trust(self) -> bool {
        matches!(self, ApprovalResolution::ApproveAndTrust)
    }
}

#[derive(Clone, Debug)]
pub struct ApprovalHandle {
    sender: watch::Sender<ApprovalSignal>,
}

impl ApprovalHandle {
    pub fn resolve(&self, resolution: ApprovalResolution) {
        let current = *self.sender.borrow();
        let _ = self.sender.send(ApprovalSignal {
            sequence: current.sequence.wrapping_add(1),
            resolution,
        });
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClarificationAnswer {
    pub question_id: String,
    pub answer: String,
    pub selected_option_id: Option<String>,
    pub selected_option_label: Option<String>,
    pub answer_source: String,
}

/// Provenance of a submitted prompt for the recall-adoption KPI (ADR-003): a
/// composition seeded from a recalled history entry is `Recalled`; anything
/// freshly typed is `Fresh`. The payload tag is written in task_06.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptSource {
    Fresh,
    Recalled,
}

impl PromptSource {
    /// Wire string written to the `prompt_submitted` payload `source` field.
    pub fn as_str(self) -> &'static str {
        match self {
            PromptSource::Fresh => "fresh",
            PromptSource::Recalled => "recalled",
        }
    }

    /// Read a payload `source` value back into a `PromptSource`. A missing or
    /// unrecognized value (events written before this field existed) defaults to
    /// `Fresh`, keeping the change backward-compatible (ADR-003).
    pub fn from_payload_value(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("recalled") => PromptSource::Recalled,
            _ => PromptSource::Fresh,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    PromptSubmitted(String, PromptSource),
    ApprovalAnswered(ApprovalResolution),
    ClarificationAnswered(ClarificationAnswer),
    /// Resolve the pending governance decision `decision_id` with `answer`
    /// (governance spine, ADR-003). The `decision_id` lets the resolver reject a
    /// stale answer that does not match the pending decision.
    GovernanceDecisionResolved(String, GovernanceAnswer),
    /// Resolve the pending whole-plan approval `question_id` with `answer`
    /// (DAG, ADR-005). The `question_id` (derived from the graph_id) lets the
    /// resolver reject a stale answer that does not match the pending plan.
    PlanApprovalResolved(String, PlanApprovalAnswer),
    FollowUpCancelled(String),
    FollowUpResumeRequested(String),
    /// Resume the session with this id (session browser, ADR-002): the worker
    /// reads it off-thread, `adopt_session`s it, and appends the resume
    /// lifecycle events. Rejected while a run is active (one-active-run guard).
    ResumeSession(String),
    InputCharacter(char),
    InputBackspace,
    RunInterruptRequested,
}

/// Internal per-step lifecycle timing used to drive elapsed display and stall
/// detection (Task 03). Held only on `App`; never serialized into `AppState`,
/// `LiveStepView`, or the durable history record (ADR-004) so wall-clock
/// `Instant`s stay out of the event stream.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StepTiming {
    started_at: Instant,
    last_activity: Instant,
}

/// A `Running`/`Streaming` step whose last activity is at least this old is
/// classified `Stalled` rather than `Active` (ADR-004). Fixed in V1;
/// configurability is a documented Non-Goal.
const STALL_THRESHOLD: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct App {
    config: EffectiveConfig,
    history: HistoryStore,
    availability: BTreeMap<String, RuntimeAvailability>,
    state: AppState,
    /// `step_id -> StepTiming`. Stamped on step registration, bumped on stream
    /// arrival and active status transitions, cleared on step end. Private and
    /// non-serialized (ADR-004); keyed by `step_id` so concurrent steps in a
    /// parallel group track timing independently of one another.
    step_timings: BTreeMap<String, StepTiming>,
    chat_projection: ChatProjection,
    pending_approval: Option<PendingApproval>,
    pending_clarification: Option<PendingClarification>,
    pending_governance_decision: Option<PendingGovernanceDecision>,
    pending_plan_approval: Option<PendingPlanApproval>,
    active_step: Option<ActiveStep>,
    active_steps: Vec<ActiveStep>,
    follow_up_queue: VecDeque<QueuedFollowUp>,
    debug_enabled: bool,
    session_ended: bool,
    state_sender: Option<watch::Sender<AppState>>,
    interrupt_sender: watch::Sender<u64>,
    interrupt_receiver: watch::Receiver<u64>,
    approval_sender: watch::Sender<ApprovalSignal>,
    approval_receiver: watch::Receiver<ApprovalSignal>,
    /// Session-scoped trust list (ADR-004): in-memory only, never persisted, and
    /// gone when the process exits. A `Vec` preserves grant order so the `/trust`
    /// listing and 1-based `revoke_index` line up.
    trust_store: TrustStore,
    /// Sender to the off-thread hook dispatcher (ADR-003). `None` until
    /// `attach_hook_sender` wires it (only when handlers are configured), so the
    /// event tap is zero-cost when no hooks are present and recording behaves
    /// exactly as before.
    hook_sender: Option<mpsc::Sender<HookDispatch>>,
    /// Shared best-effort drop counter, incremented by the tap when the dispatch
    /// channel is full. Surfaced by `--doctor` (task_08).
    dropped_hooks: DroppedHookCounter,
    /// Per-session approval-mode override for a resumed session (ADR-004):
    /// `Some(Normal)` after resume forces an approval before any write/command,
    /// independent of the global `config.approval_mode` (which may be `Yolo`).
    /// `None` for a fresh session. Re-elevation to `Yolo` is a fresh explicit act;
    /// prior approvals are never replayed (the trust store is cleared on adopt).
    resume_approval_mode: Option<ApprovalMode>,
    /// Armed when a resumed session detected workspace drift (cwd moved or HEAD
    /// changed, ADR-004/007). Forces a positive acknowledgment on the FIRST
    /// state-mutating action and is cleared once acknowledged. `None` on the clean
    /// path. Reset by `adopt_session`, set by `resume_session`.
    pending_drift_ack: Option<git::WorkspaceDrift>,
    /// Handle to the MCP supervisor actor (ADR-005), owned here so its long-lived
    /// connections live and die with the session. `Some` only when
    /// `features.mcp_enabled` (task_03); cloned onto `ActionExecutionContext`
    /// (task_05) and the catalog snapshot path (task_07). Dropped on
    /// `end_session`, which closes the command channel so the actor tears down
    /// every connection (`kill_on_drop` backstop).
    mcp_handle: Option<crate::mcp::McpHandle>,
    /// Durable, workspace-scoped MCP trust + tool-description pins (ADR-006,
    /// task_04). Loaded once at startup for synchronous reads during validation
    /// (task_05); promote/revoke write the file and record an event.
    mcp_trust: crate::mcp::McpTrustStore,
    /// Cached immutable MCP tool-catalog snapshot (ADR-001, task_07). Refreshed
    /// from the supervisor at each top-level run entry and recorded as an
    /// `mcp_catalog_snapshot` event; the orchestrator prompt and the action
    /// context read this snapshot, never a live handle, so prompts are
    /// replay-deterministic. Empty until the first snapshot.
    mcp_catalog: crate::mcp::ToolCatalog,
}

/// In-memory, session-scoped allow-list of exact trust targets (ADR-004). Granted
/// via `ApproveAndTrust` (task_05), managed by `/trust` (task_08), and snapshotted
/// into each `ActionExecutionContext` so the enforcement matrix can auto-approve a
/// repeat of an already-trusted target.
#[derive(Clone, Debug, Default)]
pub struct TrustStore {
    entries: Vec<TrustTarget>,
}

impl TrustStore {
    pub fn contains(&self, target: &TrustTarget) -> bool {
        self.entries.iter().any(|entry| entry == target)
    }

    /// Insert a target if absent. Returns `true` when it was newly granted.
    pub fn grant(&mut self, target: TrustTarget) -> bool {
        if self.contains(&target) {
            false
        } else {
            self.entries.push(target);
            true
        }
    }

    /// Remove the entry at a 1-based index from the most recent listing order.
    /// Returns the removed target, or `None` for an out-of-range index (the store
    /// is left unchanged).
    pub fn revoke_index(&mut self, one_based: usize) -> Option<TrustTarget> {
        if one_based == 0 || one_based > self.entries.len() {
            return None;
        }
        Some(self.entries.remove(one_based - 1))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The trusted targets in grant order (the order `/trust` lists and
    /// `revoke_index` addresses).
    pub fn list(&self) -> &[TrustTarget] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A cheap, shareable snapshot for one action's `ActionExecutionContext`.
    pub fn snapshot(&self) -> Arc<HashSet<TrustTarget>> {
        Arc::new(self.entries.iter().cloned().collect())
    }
}

#[derive(Clone, Debug)]
struct PendingApproval {
    run_id: String,
    step_id: String,
    action_request: ActionRequest,
    agent_profile: AgentProfile,
    context: ActionExecutionContext,
    reason: Option<String>,
    run: RunDriveContext,
    step: PausedStep,
    step_started_at: Instant,
    request: RuntimeRequest,
}

#[derive(Clone, Debug)]
struct PendingParallelApproval {
    run_id: String,
    group_id: String,
    step_id: String,
    action_request: ActionRequest,
    agent_profile: AgentProfile,
    context: ActionExecutionContext,
    reason: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingClarification {
    run: RunDriveContext,
}

/// The captured run context behind a pending governance decision, mirroring
/// `PendingClarification`. Held on `App` (not `AppState`) so the resume can ride
/// the clarification transport (`RunDriveContext` capture + `drive_and_replay`).
#[derive(Clone, Debug)]
struct PendingGovernanceDecision {
    run: RunDriveContext,
}

/// Captured state for a DAG whole-plan approval paused in `normal` mode
/// (ADR-005): the run, the proposing decision, and the validated graph, all
/// owned so accept can run the graph and reject can re-drive the orchestrator —
/// the same pause/resume transport as [`PendingGovernanceDecision`].
#[derive(Clone, Debug)]
struct PendingPlanApproval {
    run: RunDriveContext,
    decision: crate::orchestrator::OrchestratorDecision,
    graph: crate::orchestrator::ExecutionGraph,
}

#[derive(Clone, Debug)]
struct ActiveStep {
    run_id: String,
    group_id: Option<String>,
    step_id: String,
    step_label: Option<String>,
    file_scope: Option<ParallelFileScope>,
    agent: String,
}

#[derive(Clone, Debug)]
struct QueuedFollowUp {
    id: String,
    prompt: String,
    created_at: String,
    status: QueuedFollowUpStatus,
    pause_reason: Option<String>,
}

impl QueuedFollowUp {
    fn new(prompt: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            prompt: prompt.into(),
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            status: QueuedFollowUpStatus::Pending,
            pause_reason: None,
        }
    }

    fn to_view(&self) -> QueuedFollowUpView {
        QueuedFollowUpView {
            id: self.id.clone(),
            prompt: self.prompt.clone(),
            created_at: self.created_at.clone(),
            status: self.status.clone(),
            pause_reason: self.pause_reason.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct RunDriveContext {
    run_id: String,
    parent_run_id: Option<String>,
    submitted_prompt: String,
    prompt: String,
    skill_context: Option<SkillPromptContext>,
    subtask: Option<SubtaskContext>,
    workflow: Option<WorkflowRunContext>,
    previous_results: Vec<RunStepResult>,
    step_count: u32,
    started_at: Instant,
    parse_repair_attempts: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowRunContext {
    original_command: String,
    user_prompt: String,
    target_ledger: BTreeMap<String, Vec<WorkflowTarget>>,
    verification: Vec<String>,
    skipped_checks: Vec<String>,
    residual_risks: Vec<String>,
    #[serde(skip)]
    completion_recorded: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowTarget {
    path: String,
    source_group_id: String,
    source_step_id: Option<String>,
    source_step_label: String,
    status: WorkflowTargetStatus,
    reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WorkflowTargetStatus {
    Planned,
    Completed,
    Skipped,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCompletionStatus {
    Completed,
    CompletedWithIssues,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowTargetCounts {
    planned: usize,
    completed: usize,
    skipped: usize,
    blocked: usize,
    failed: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowUnfinishedTarget {
    path: String,
    source_group_id: String,
    source_step_id: Option<String>,
    source_step_label: String,
    status: WorkflowTargetStatus,
    reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowCompletionPayload {
    run_id: String,
    status: WorkflowCompletionStatus,
    target_counts: WorkflowTargetCounts,
    unfinished_targets: Vec<WorkflowUnfinishedTarget>,
    verification: Vec<String>,
    skipped_checks: Vec<String>,
    residual_risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkflowCommand {
    original_command: String,
    prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkflowStart {
    command: WorkflowCommand,
    preflight: WorkflowPreflight,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct WorkflowPreflight {
    parallel_step_groups: bool,
    max_parallel_agent_steps: u32,
}

#[derive(Clone, Copy, Debug)]
struct RuntimePrompt<'a> {
    text: &'a str,
    skill_context: Option<&'a SkillPromptContext>,
}

impl<'a> RuntimePrompt<'a> {
    fn new(text: &'a str, skill_context: Option<&'a SkillPromptContext>) -> Self {
        Self {
            text,
            skill_context,
        }
    }
}

impl RunDriveContext {
    fn new(
        run_id: impl Into<String>,
        parent_run_id: Option<String>,
        submitted_prompt: impl Into<String>,
        prompt: impl Into<String>,
        skill_context: Option<SkillPromptContext>,
        subtask: Option<SubtaskContext>,
        workflow: Option<WorkflowRunContext>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            parent_run_id,
            submitted_prompt: submitted_prompt.into(),
            prompt: prompt.into(),
            skill_context,
            subtask,
            workflow,
            previous_results: Vec::new(),
            step_count: 0,
            started_at: Instant::now(),
            parse_repair_attempts: 0,
        }
    }

    fn loaded_skill_metadata(&self) -> Vec<LoadedSkillMetadata> {
        self.skill_context
            .as_ref()
            .map(SkillPromptContext::metadata)
            .unwrap_or_default()
    }
}

impl WorkflowRunContext {
    fn new(command: &WorkflowCommand) -> Self {
        Self {
            original_command: command.original_command.clone(),
            user_prompt: command.prompt.clone(),
            target_ledger: BTreeMap::new(),
            verification: Vec::new(),
            skipped_checks: Vec::new(),
            residual_risks: Vec::new(),
            completion_recorded: false,
        }
    }

    fn record_planned_targets(
        &mut self,
        group_id: &str,
        step_id: Option<&str>,
        step_label: &str,
        file_scope: &ParallelFileScope,
        working_directory: &Path,
        extra_write_roots: &[PathBuf],
    ) -> Result<usize> {
        let mut recorded = 0usize;
        for write_file in &file_scope.write_files {
            let path =
                normalize_workflow_target_key(write_file, working_directory, extra_write_roots)?;
            let target = WorkflowTarget {
                path: path.clone(),
                source_group_id: group_id.to_string(),
                source_step_id: step_id.map(str::to_string),
                source_step_label: step_label.to_string(),
                status: WorkflowTargetStatus::Planned,
                reason: None,
            };
            self.target_ledger.entry(path).or_default().push(target);
            recorded += 1;
        }
        Ok(recorded)
    }

    fn record_child_result(
        &mut self,
        group_id: &str,
        step_id: &str,
        file_scope: &ParallelFileScope,
        result: &AgentResult,
        working_directory: &Path,
        extra_write_roots: &[PathBuf],
    ) -> Result<()> {
        self.record_verification(result);
        let (status, reason) = workflow_target_status_from_agent_result(result);
        for write_file in &file_scope.write_files {
            let path =
                normalize_workflow_target_key(write_file, working_directory, extra_write_roots)?;
            if let Some(targets) = self.target_ledger.get_mut(&path) {
                for target in targets.iter_mut().filter(|target| {
                    target.source_group_id == group_id
                        && target.source_step_id.as_deref() == Some(step_id)
                }) {
                    target.status = status.clone();
                    target.reason = reason.clone();
                }
            }
        }
        Ok(())
    }

    fn record_verification(&mut self, result: &AgentResult) {
        for item in result.commands.iter().chain(result.verification.iter()) {
            if !item.trim().is_empty() && !self.verification.contains(item) {
                self.verification.push(item.clone());
            }
        }
    }

    fn completion_payload(&self, run_id: &str, interrupted: bool) -> WorkflowCompletionPayload {
        let target_counts = self.target_counts();
        let unfinished_targets = self.unfinished_targets();
        let status =
            derive_workflow_completion_status(&target_counts, &unfinished_targets, interrupted);
        WorkflowCompletionPayload {
            run_id: run_id.to_string(),
            status,
            target_counts,
            unfinished_targets,
            verification: self.verification.clone(),
            skipped_checks: self.skipped_checks.clone(),
            residual_risks: self.residual_risks.clone(),
        }
    }

    fn target_counts(&self) -> WorkflowTargetCounts {
        let mut counts = WorkflowTargetCounts {
            planned: 0,
            completed: 0,
            skipped: 0,
            blocked: 0,
            failed: 0,
        };
        for target in self.targets() {
            match target.status {
                WorkflowTargetStatus::Planned => counts.planned += 1,
                WorkflowTargetStatus::Completed => counts.completed += 1,
                WorkflowTargetStatus::Skipped => counts.skipped += 1,
                WorkflowTargetStatus::Blocked => counts.blocked += 1,
                WorkflowTargetStatus::Failed => counts.failed += 1,
            }
        }
        counts
    }

    fn unfinished_targets(&self) -> Vec<WorkflowUnfinishedTarget> {
        self.targets()
            .filter(|target| target.status != WorkflowTargetStatus::Completed)
            .map(|target| WorkflowUnfinishedTarget {
                path: target.path.clone(),
                source_group_id: target.source_group_id.clone(),
                source_step_id: target.source_step_id.clone(),
                source_step_label: target.source_step_label.clone(),
                status: target.status.clone(),
                reason: target.reason.clone().unwrap_or_else(|| {
                    if matches!(target.status, WorkflowTargetStatus::Planned) {
                        "planned target did not receive terminal workflow evidence".to_string()
                    } else {
                        "workflow target did not include a reason".to_string()
                    }
                }),
            })
            .collect()
    }

    fn targets(&self) -> impl Iterator<Item = &WorkflowTarget> {
        self.target_ledger
            .values()
            .flat_map(|targets| targets.iter())
    }
}

#[derive(Clone, Debug)]
struct SubtaskContext {
    agent_id: String,
    request: String,
    submitted_request: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CouncilMemberReport {
    member_id: String,
    status: AgentResultStatus,
    summary: String,
    diagnostic: Option<String>,
    artifact: Option<crate::orchestrator::ArtifactReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CouncilDecisionEnvelope {
    schema_version: u32,
    confidence: String,
    dissent: Vec<String>,
    risks: Vec<String>,
    recommended_action: String,
    stop_condition: String,
}

#[derive(Clone, Debug)]
struct StepResume {
    step: PausedStep,
    step_started_at: Instant,
    request: RuntimeRequest,
}

#[derive(Clone, Debug)]
enum PausedStep {
    Orchestrator { step_id: String },
    Agent { step_id: String, agent_id: String },
}

impl PausedStep {
    fn step_id(&self) -> &str {
        match self {
            PausedStep::Orchestrator { step_id } | PausedStep::Agent { step_id, .. } => step_id,
        }
    }
}

#[derive(Debug)]
enum StepOutcome {
    Output(Box<RuntimeOutput>),
    Paused,
    LimitReached,
    Interrupted,
}

#[derive(Debug)]
enum OrchestratorStepOutcome {
    Decision(Box<crate::orchestrator::OrchestratorDecision>),
    Paused,
    Retry,
    Stop,
}

#[derive(Debug)]
enum AgentStepOutcome {
    Completed,
    Paused,
    Stop,
}

#[derive(Debug)]
enum ParallelRuntimeMessage {
    RuntimeEvent {
        step_id: String,
        event: RuntimeEvent,
    },
    Output {
        step_id: String,
        output: Box<Result<RuntimeOutput>>,
    },
}

#[derive(Clone, Debug)]
struct ParallelRuntimeResumeHandle {
    cancellation: CancellationToken,
    sender: mpsc::Sender<ParallelRuntimeMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ApprovalSignal {
    sequence: u64,
    resolution: ApprovalResolution,
}

/// Per-node lifecycle discriminator for the DAG scheduler (ADR-004). The flat
/// parallel path spawns every child immediately, so its children are `Running`
/// from the start and never read this field; the DAG path uses it to tell a
/// *planned-but-not-yet-admitted* node (`Pending`) apart from a *spawned*
/// node (`Running`) — a distinction `terminal_result == None` alone cannot make.
/// Terminality remains authoritative via `terminal_result.is_some()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeRunState {
    Pending,
    Running,
    Terminal,
}

#[derive(Clone, Debug)]
struct ParallelChildRuntimeState {
    step_id: String,
    step_label: String,
    agent_id: String,
    file_scope: ParallelFileScope,
    request: RuntimeRequest,
    step_started_at: Instant,
    next_runtime_sequence: u32,
    action_count: u32,
    cancellation: CancellationToken,
    terminal_result: Option<AgentResult>,
    result_recorded: bool,
    /// Admission state for the DAG scheduler; always `Running` on the flat
    /// parallel path (which spawns all children up front).
    node_run_state: NodeRunState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunRecord {
    schema_version: u32,
    run_id: String,
    parent_run_id: Option<String>,
    session_id: String,
    submitted_prompt: String,
    prompt: String,
    loaded_skills: Vec<LoadedSkillMetadata>,
    session_goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workflow: Option<WorkflowRunContext>,
    subtask: Option<SubtaskRecord>,
    state: RunState,
    results: Vec<RunStepResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SubtaskRecord {
    agent_id: String,
    submitted_request: String,
    request: String,
}

/// A fully replayed session ready to replace the running one in a single atomic
/// swap (ADR-003/006). The heavy disk read (enumerating + reading `events.jsonl`)
/// can run off-thread — see [`LoadedSession::from_history_and_events`], which
/// folds an already-read `Vec<HistoryEvent>` on the worker; [`adopt_session`]
/// then performs the cheap swap. Every session-scoped value the swap needs is
/// captured here so adoption never re-reads disk or derives state piecemeal.
///
/// [`adopt_session`]: App::adopt_session
pub struct LoadedSession {
    /// The adopted durable store, bound via [`HistoryStore::open`] (schema
    /// validated, so an incompatible log is unconstructible — ADR-003).
    history: HistoryStore,
    /// The chat transcript folded from the adopted log (`ChatProjection::rebuild`).
    projection: ChatProjection,
    /// The adopted session id (mirrors `history.session_id()`).
    session_id: String,
    /// The session goal folded from the log, or `None`.
    session_goal: Option<String>,
    /// `Some((run_id, prior_state))` when the last run never reached a terminal
    /// state — a dangling run that adoption reconciles with a terminal
    /// `run_interrupted` event (ADR-002). Already filtered to non-terminal by
    /// [`dangling_run_from_events`], so a `Some` here always needs reconciling.
    dangling_run: Option<(String, RunState)>,
    /// The session's end-state before resume, recorded into `session_resumed`
    /// (ADR-002): the dangling run's in-flight state, else the folded terminal
    /// outcome. Captured by the resume flow before `adopt_session` consumes this.
    prior_end_state: RunState,
    /// A digest of the pre-resume log tail for tamper-evidence (ADR-007), recorded
    /// into `session_resumed`. `None` for an empty log.
    prior_tail_hash: Option<String>,
    /// The session's last-recorded short HEAD (`last_recorded_head_sha`) — the
    /// drift baseline (ADR-007). The resume flow compares it against the live HEAD
    /// to detect a branch/rebase change. `None` for a non-git session.
    stored_head_sha: Option<String>,
}

impl LoadedSession {
    /// Open, read, and fold a stored session into an adoptable value. The schema
    /// validation in [`HistoryStore::open`] makes an incompatible log
    /// unconstructible (ADR-003), so adoption downstream never sees a stale or
    /// raw event. The read happens on the calling thread here; the resume flow
    /// (task_11) reads off-thread and calls [`from_history_and_events`] directly.
    ///
    /// [`from_history_and_events`]: LoadedSession::from_history_and_events
    pub fn load(root: &Path, session_id: &str) -> Result<Self> {
        let history = HistoryStore::open(root, session_id)?;
        let events = history.read_events()?;
        Ok(Self::from_history_and_events(history, &events))
    }

    /// Fold an already-read event log (from an opened store) into an adoptable
    /// value. Pure + `Send`-friendly so the heavy read can be lifted off-thread
    /// while this cheap fold runs on the worker (ADR-003/006).
    pub fn from_history_and_events(history: HistoryStore, events: &[HistoryEvent]) -> Self {
        let session_id = history.session_id().to_string();
        let dangling_run = dangling_run_from_events(events);
        // Prior end-state: the dangling run's in-flight state if any, else the
        // folded terminal outcome (ADR-002).
        let prior_end_state = dangling_run
            .as_ref()
            .map(|(_, state)| state.clone())
            .unwrap_or_else(|| derive_outcome(events));
        Self {
            projection: ChatProjection::rebuild(events),
            session_goal: derive_goal(events),
            prior_tail_hash: hash_events_digest(events),
            stored_head_sha: last_recorded_head_sha(events),
            prior_end_state,
            dangling_run,
            session_id,
            history,
        }
    }
}

impl App {
    pub async fn new(config: EffectiveConfig) -> Result<Self> {
        Self::new_with_debug(config, false).await
    }

    pub async fn new_with_debug(config: EffectiveConfig, debug_enabled: bool) -> Result<Self> {
        let history = HistoryStore::create(&config.working_directory)?;
        // Post-crash hint (task_13): does the newest *prior* session (not this
        // just-created one) show as non-terminal? Computed once at startup.
        let recoverable_session = newest_prior_session_recoverable(
            &config.working_directory.join(".atelier"),
            history.session_id(),
        );
        let availability = check_all_runtime_availability(&config).await;
        // Own the MCP supervisor when enabled (ADR-005). Spawning returns a handle
        // immediately; connections are established asynchronously on the actor
        // task, so this never blocks or fails session startup.
        let mcp_handle = if config.features.mcp_enabled {
            Some(crate::mcp::McpSupervisor::spawn(
                config.mcp_servers.values().cloned().collect(),
                crate::mcp::DEFAULT_MCP_CALL_TIMEOUT,
            ))
        } else {
            None
        };
        // Trust is loaded unconditionally (cheap, no secrets): validation needs a
        // synchronous read path whenever an MCP action is evaluated (task_05). The
        // `.atelier` root already exists — `HistoryStore::create` made it above.
        let mcp_trust = crate::mcp::McpTrustStore::load(&config.working_directory.join(".atelier"));
        let (interrupt_sender, interrupt_receiver) = watch::channel(0);
        let (approval_sender, approval_receiver) = watch::channel(ApprovalSignal {
            sequence: 0,
            resolution: ApprovalResolution::Deny,
        });
        let state = AppState {
            session_id: history.session_id().to_string(),
            run_state: RunState::Idle,
            active_run_id: None,
            session_goal: None,
            config_status: build_config_status(&config, &availability),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: build_agent_views(&config, &availability),
            roster_rows: Vec::new(),
            // Branded welcome item present from the first frame (ADR-005);
            // `sync_chat_items` keeps it prepended across projection updates.
            chat_items: vec![ChatItemView::welcome()],
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: String::new(),
            git_context: None,
            recoverable_session,
        };
        let mut app = Self {
            config,
            history,
            availability,
            state,
            step_timings: BTreeMap::new(),
            chat_projection: ChatProjection::new(),
            pending_approval: None,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            active_step: None,
            active_steps: Vec::new(),
            follow_up_queue: VecDeque::new(),
            debug_enabled: debug_enabled || debug_enabled_from_env(),
            session_ended: false,
            state_sender: None,
            interrupt_sender,
            interrupt_receiver,
            approval_sender,
            approval_receiver,
            trust_store: TrustStore::default(),
            hook_sender: None,
            dropped_hooks: DroppedHookCounter::new(),
            resume_approval_mode: None,
            pending_drift_ack: None,
            mcp_handle,
            mcp_trust,
            mcp_catalog: crate::mcp::ToolCatalog::default(),
        };
        app.record_event(
            None,
            None,
            "session_started",
            json!({}),
            "Harness session started.",
        )?;
        Ok(app)
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    pub fn attach_state_sender(&mut self, sender: watch::Sender<AppState>) {
        self.state_sender = Some(sender);
        self.publish_state();
    }

    /// Wire the event tap to the off-thread hook dispatcher (ADR-003). Called by
    /// `run_tui` only when handlers are configured; the shared `dropped` counter
    /// is the one the tap increments and `--doctor` reports.
    pub fn attach_hook_sender(
        &mut self,
        sender: mpsc::Sender<HookDispatch>,
        dropped: DroppedHookCounter,
    ) {
        self.hook_sender = Some(sender);
        self.dropped_hooks = dropped;
    }

    /// Count of hook dispatches dropped on a full channel (best-effort delivery).
    pub fn dropped_hook_count(&self) -> u64 {
        self.dropped_hooks.count()
    }

    pub fn interrupt_handle(&self) -> InterruptHandle {
        InterruptHandle {
            sender: self.interrupt_sender.clone(),
        }
    }

    pub fn approval_handle(&self) -> ApprovalHandle {
        ApprovalHandle {
            sender: self.approval_sender.clone(),
        }
    }

    pub fn record_diagnostic(&mut self, message: impl Into<String>) -> Result<()> {
        let message = message.into();
        self.record_event(
            self.state.active_run_id.clone(),
            None,
            "diagnostic",
            json!({ "message": message.clone() }),
            format!("Error: {}", concise_diagnostic(&message)),
        )
    }

    /// The approval mode action decisions run under: a resumed session's cautious
    /// override (ADR-004) when present, else the global config mode. Consulted
    /// everywhere an `ActionExecutionContext` is built so a resumed `Yolo` session
    /// still prompts before any write/command.
    fn effective_approval_mode(&self) -> ApprovalMode {
        self.resume_approval_mode
            .clone()
            .unwrap_or_else(|| self.config.approval_mode.clone())
    }

    /// The drift context to fold into the first mutating action's approval prompt
    /// (ADR-004/007), or `None` when no drift gate is armed. Carried into the
    /// `ActionExecutionContext` so the enforcement matrix forces the prompt and
    /// the modal shows what moved.
    fn drift_ack_context(&self) -> Option<String> {
        self.pending_drift_ack.as_ref().map(drift_ack_message)
    }

    pub async fn handle_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::PromptSubmitted(prompt, source) => {
                self.state.input.clear();
                // Refresh git before the run so the footer/welcome reflect the
                // branch the prompt actually runs against (ADR-006).
                self.refresh_git_context().await;
                self.publish_state();
                self.submit_prompt_with_source(prompt, source).await
            }
            AppEvent::ApprovalAnswered(resolution) => {
                self.state.input.clear();
                self.publish_state();
                self.resolve_pending_approval(resolution).await?;
                Ok(())
            }
            AppEvent::ClarificationAnswered(answer) => {
                self.state.input.clear();
                self.publish_state();
                self.resolve_pending_clarification(answer).await
            }
            AppEvent::GovernanceDecisionResolved(decision_id, answer) => {
                self.state.input.clear();
                self.publish_state();
                self.resolve_pending_governance_decision(&decision_id, answer)
                    .await
            }
            AppEvent::PlanApprovalResolved(question_id, answer) => {
                self.state.input.clear();
                self.publish_state();
                self.resolve_pending_plan_approval(&question_id, answer)
                    .await
            }
            AppEvent::FollowUpCancelled(id) => {
                self.state.input.clear();
                self.publish_state();
                self.cancel_follow_up(&id)
            }
            AppEvent::FollowUpResumeRequested(id) => {
                self.state.input.clear();
                self.publish_state();
                self.resume_follow_up(&id)?;
                // Resume only replays when the app is already in a clean idle state;
                // it never pauses, so it cannot undo the resume it just performed.
                if self.can_replay_now() {
                    self.react_to_run_end_for_queue().await?;
                }
                Ok(())
            }
            AppEvent::ResumeSession(session_id) => {
                self.state.input.clear();
                self.publish_state();
                self.resume_session(session_id).await
            }
            AppEvent::InputCharacter(ch) => {
                self.state.input.push(ch);
                self.publish_state();
                Ok(())
            }
            AppEvent::InputBackspace => {
                self.state.input.pop();
                self.publish_state();
                Ok(())
            }
            AppEvent::RunInterruptRequested => self.interrupt(),
        }
    }

    pub async fn refresh_runtime_availability(&mut self) {
        self.availability = check_all_runtime_availability(&self.config).await;
        self.state.agents = build_agent_views(&self.config, &self.availability);
        self.state.config_status = build_config_status(&self.config, &self.availability);
        self.publish_state();
    }

    pub fn end_session(&mut self) -> Result<()> {
        if self.session_ended {
            return Ok(());
        }
        self.record_event(
            None,
            None,
            "session_ended",
            json!({
                "run_state": self.state.run_state.clone(),
                "active_run_id": self.state.active_run_id.clone()
            }),
            "Harness session ended.",
        )?;
        // Drop the supervisor handle: closing the command channel makes the actor
        // exit and drop every connection, whose `TokioChildProcess::drop` kills
        // the child (ADR-005 kill_on_drop backstop). Sync-safe — no await needed.
        self.mcp_handle = None;
        self.session_ended = true;
        Ok(())
    }

    /// A clone of the MCP supervisor handle, if MCP is enabled. Consumed by the
    /// action-execution path (task_05) and the catalog-snapshot path (task_07).
    pub fn mcp_handle(&self) -> Option<crate::mcp::McpHandle> {
        self.mcp_handle.clone()
    }

    /// Synchronous read access to the MCP trust store, for validation (task_05).
    pub fn mcp_trust(&self) -> &crate::mcp::McpTrustStore {
        &self.mcp_trust
    }

    /// The MCP wiring for one action's `ActionExecutionContext` (task_05): the
    /// supervisor handle plus a snapshot of the trust store and the cached tool
    /// catalog (recorded at run entry, task_07). `None` when MCP is disabled. The
    /// catalog gives the validation pin-diff current tool definitions to compare.
    fn mcp_action_context(&self) -> Option<crate::actions::McpActionContext> {
        self.mcp_handle
            .clone()
            .map(|handle| crate::actions::McpActionContext {
                handle,
                trust: self.mcp_trust.clone(),
                catalog: std::sync::Arc::new(self.mcp_catalog.clone()),
            })
    }

    /// Refresh and record the MCP tool-catalog snapshot at run entry (task_07).
    /// A no-op when MCP is disabled. The recorded `mcp_catalog_snapshot` event is
    /// the replay source for the orchestrator prompt's MCP-tools section.
    async fn record_mcp_catalog_snapshot(&mut self, run_id: &str) -> Result<()> {
        let Some(handle) = self.mcp_handle.clone() else {
            return Ok(());
        };
        let catalog = handle.snapshot_catalog().await.unwrap_or_default();
        self.record_event(
            Some(run_id.to_string()),
            None,
            "mcp_catalog_snapshot",
            serde_json::to_value(&catalog).unwrap_or_else(|_| json!({ "servers": [] })),
            format!(
                "Recorded MCP tool catalog snapshot ({} server(s)).",
                catalog.servers.len()
            ),
        )?;
        self.mcp_catalog = catalog;
        Ok(())
    }

    /// Promote an MCP server to `Trusted`: persist the file and, only on a real
    /// change, record an `mcp_server_trusted` event (the audit trail, ADR-006).
    pub fn promote_mcp_server(&mut self, server: &str) -> Result<()> {
        if self.mcp_trust.promote(server)? {
            self.record_event(
                None,
                None,
                "mcp_server_trusted",
                json!({ "server": server }),
                format!("Trusted MCP server {server}."),
            )?;
        }
        Ok(())
    }

    /// Revoke an MCP server's trust: persist the file and, only on a real change,
    /// record an `mcp_server_revoked` event.
    pub fn revoke_mcp_server(&mut self, server: &str) -> Result<()> {
        if self.mcp_trust.revoke(server)? {
            self.record_event(
                None,
                None,
                "mcp_server_revoked",
                json!({ "server": server }),
                format!("Revoked trust for MCP server {server}."),
            )?;
        }
        Ok(())
    }

    /// The single atomic point that swaps the running session for `loaded`
    /// (ADR-003/006): reassign **every** session-scoped field, reconcile a
    /// dangling run, and emit exactly one state broadcast. No other path may
    /// rebind these fields piecemeal — the exhaustiveness test
    /// `adopt_session_resets_every_session_scoped_field` guards that, and the
    /// `AppState` subset is rebuilt via a full struct literal below so a new
    /// `AppState` field forces a compile error here rather than silently
    /// stranding prior-session state.
    ///
    /// The worker thread owns `App`, so this swap is atomic from the UI's
    /// perspective regardless of the single broadcast (ADR-006).
    ///
    /// Kept private per ADR-006 (the single, audited swap point); the production
    /// caller is [`resume_session`]. `prior_end_state` / `prior_tail_hash` are
    /// the resume flow's to read off `loaded` *before* this consumes it, so they
    /// are ignored here.
    ///
    /// [`resume_session`]: App::resume_session
    fn adopt_session(&mut self, loaded: LoadedSession) -> Result<()> {
        let LoadedSession {
            history,
            projection,
            session_id,
            session_goal,
            dangling_run,
            prior_end_state: _,
            prior_tail_hash: _,
            stored_head_sha: _,
        } = loaded;

        // Swap the durable store + folded transcript first, so the reconciling
        // `run_interrupted` event below appends into the ADOPTED log/projection.
        self.history = history;
        self.chat_projection = projection;

        // Reset every transient session-scoped field on `App` to its fresh value
        // so no prior-session modal / queue / live step / trust grant leaks
        // across the swap (mirrors the `interrupt()` teardown).
        self.step_timings.clear();
        self.pending_approval = None;
        self.pending_clarification = None;
        self.pending_governance_decision = None;
        self.pending_plan_approval = None;
        self.active_step = None;
        self.active_steps.clear();
        self.follow_up_queue.clear();
        self.trust_store.clear();
        self.session_ended = false;
        // Resume safety state (ADR-004) is reset here and (re)armed by
        // `resume_session` after this swap, so an adoption that is not a resume
        // (or a re-resume) never inherits a stale cautious-mode/drift gate.
        self.resume_approval_mode = None;
        self.pending_drift_ack = None;

        // Rebuild the session subset of `AppState` via a FULL struct literal (no
        // `..` spread): a future `AppState` field then forces a compile error
        // here, making the session-scoped set exhaustive at compile time
        // (ADR-006). Process-scoped fields (config/availability-derived status,
        // live git context) carry across unchanged; live agent activity resets
        // to a fresh idle roster. `chat_items` is filled by `sync_chat_items`
        // below. Resume always opens `Idle` with no active run (ADR-002).
        self.state = AppState {
            session_id,
            run_state: RunState::Idle,
            active_run_id: None,
            session_goal,
            config_status: self.state.config_status.clone(),
            git_context: self.state.git_context.clone(),
            agents: build_agent_views(&self.config, &self.availability),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: String::new(),
            // The post-crash hint is a startup-only welcome cue; a session adopted
            // mid-run never shows the welcome, so this is always false here.
            recoverable_session: false,
        };

        // Reconcile a dangling (non-terminal) run: append a terminal
        // `run_interrupted` event so a future replay knows the run ended
        // (ADR-002). `dangling_run` is pre-filtered to non-terminal, so its
        // presence is the "detect dangling" signal. The no-publish append keeps
        // the swap a single broadcast; `run_state` stays `Idle` either way.
        if let Some((run_id, prior_state)) = dangling_run {
            let payload = serde_json::to_value(RunInterruptedPayload {
                run_id: run_id.clone(),
                prior_state,
            })?;
            self.append_event_with_group(
                Some(run_id),
                None,
                None,
                RUN_INTERRUPTED_KIND,
                payload,
                "Prior run interrupted on resume.",
            )?;
        }

        // Make `chat_items` reflect the adopted projection (+ the reconciliation
        // event, if any), then broadcast exactly once.
        self.sync_chat_items();
        self.publish_state();
        Ok(())
    }

    /// Resume the session `session_id` from the browser (ADR-002): read it
    /// off-thread, adopt it on the worker, and append the resume lifecycle
    /// events, landing `Idle` with the full prior transcript re-rendered so the
    /// user can continue in the SAME durable log. No automatic re-execution of
    /// the interrupted step (V1). A failed read degrades to a diagnostic rather
    /// than crashing the worker.
    async fn resume_session(&mut self, session_id: String) -> Result<()> {
        // One-active-run guard (req): never swap the session out from under a
        // live run. The browser is openable mid-run, so this can happen.
        if self.state.active_run_id.is_some() {
            self.record_diagnostic("Cannot resume a session while a run is active.")?;
            return Ok(());
        }

        // Refresh git first so the resume boundary records the live HEAD/dirty
        // and the drift baseline reflects where resume actually runs (ADR-007).
        self.refresh_git_context().await;

        // The heavy disk read (enumerate + read `events.jsonl`) runs off-thread;
        // the cheap fold + swap run here on the worker (ADR-006). `LoadedSession`
        // is `Send`, so it crosses the `spawn_blocking` boundary.
        let root = self.config.working_directory.join(".atelier");
        let load_id = session_id.clone();
        let loaded = match tokio::task::spawn_blocking(move || LoadedSession::load(&root, &load_id))
            .await
        {
            Ok(Ok(loaded)) => loaded,
            Ok(Err(err)) => {
                self.record_diagnostic(format!("Failed to load session {session_id}: {err}"))?;
                return Ok(());
            }
            Err(join_err) => {
                self.record_diagnostic(format!("Failed to load session {session_id}: {join_err}"))?;
                return Ok(());
            }
        };

        // Capture the pre-resume context BEFORE `adopt_session` consumes `loaded`.
        let prior_end_state = loaded.prior_end_state.clone();
        let prior_tail_hash = loaded.prior_tail_hash.clone();

        // Drift baseline (ADR-007): the session's stored working dir + last HEAD
        // vs the live workspace (git just refreshed above). A moved cwd or a
        // changed HEAD is drift; a merely dirty tree is not (handled by
        // `detect_drift`).
        let stored_cwd = loaded.history.read_metadata()?.working_directory;
        let stored_head = loaded.stored_head_sha.clone();
        let live_head = self
            .state
            .git_context
            .as_ref()
            .and_then(|context| context.head_sha.clone());
        let drift = git::detect_drift(
            &stored_cwd,
            stored_head.as_deref(),
            &self.config.working_directory,
            live_head.as_deref(),
        );

        // Atomic swap: reconciles a dangling run (`run_interrupted`) and lands
        // Idle, re-rendering the full prior transcript (ADR-002/006).
        self.adopt_session(loaded)?;

        // Re-consent to the cautious default and arm the first-mutation drift
        // interlock (ADR-004). Set AFTER adopt (which resets both) and BEFORE the
        // resume boundary, so `session_resumed.approval_mode` records the cautious
        // override rather than the global config mode.
        self.resume_approval_mode = Some(ApprovalMode::Normal);
        if drift.any() {
            self.pending_drift_ack = Some(drift);
        }

        // Append the tamper-evident resume boundary AFTER the swap, so it lands
        // in the adopted log as the most recent event (ADR-002/007/008).
        self.record_session_resumed(prior_end_state, prior_tail_hash)?;
        Ok(())
    }

    /// Append the `session_resumed` boundary event capturing the resume context
    /// (ADR-002/007/008). The approval mode recorded here is the current
    /// effective mode; task_12 defaults resume to the cautious (`Normal`) mode.
    fn record_session_resumed(
        &mut self,
        prior_end_state: RunState,
        prior_tail_hash: Option<String>,
    ) -> Result<()> {
        let (head_sha, dirty) = self
            .state
            .git_context
            .as_ref()
            .map(|context| (context.head_sha.clone(), context.dirty))
            .unwrap_or((None, false));
        let approval_mode = serde_json::to_value(self.effective_approval_mode())
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "normal".to_string());
        let payload = SessionResumedPayload {
            resumed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            cwd: self.config.working_directory.clone(),
            head_sha,
            dirty,
            prior_end_state,
            approval_mode,
            prior_tail_hash,
        };
        self.record_event(
            None,
            None,
            SESSION_RESUMED_KIND,
            serde_json::to_value(payload)?,
            "Session resumed.",
        )
    }

    /// On approving the first state-mutating action after a drifted resume,
    /// record the acknowledgment and disarm the gate so later mutations run under
    /// the normal cautious approval without re-prompting for drift (ADR-004). A
    /// no-op when no gate is armed or the action is read-only (which never holds
    /// the gate). Called from both the serial and parallel approve paths.
    fn acknowledge_resume_drift(&mut self, action_request: &ActionRequest) -> Result<()> {
        if !crate::actions::is_mutating_kind(&action_request.kind) {
            return Ok(());
        }
        let Some(drift) = self.pending_drift_ack.take() else {
            return Ok(());
        };
        self.record_event(
            self.state.active_run_id.clone(),
            Some(action_request.step_id.clone()),
            RESUME_DRIFT_ACK_KIND,
            json!({
                "cwd_moved": drift.cwd_moved,
                "head_changed": drift.head_changed,
                "action_id": action_request.action_id.clone(),
            }),
            "Resume drift acknowledged before the first change.",
        )
    }

    pub async fn submit_prompt(&mut self, prompt: impl Into<String>) -> Result<()> {
        // Non-recall entry points (queued-follow-up replays, programmatic and
        // test submits) are Fresh; the TUI recall path tags provenance by
        // calling `submit_prompt_with_source` directly (ADR-003).
        self.submit_prompt_with_source(prompt, PromptSource::Fresh)
            .await
    }

    pub async fn submit_prompt_with_source(
        &mut self,
        prompt: impl Into<String>,
        source: PromptSource,
    ) -> Result<()> {
        let prompt = prompt.into();
        if prompt.trim().is_empty() {
            return Ok(());
        }
        if self.handle_goal_command(&prompt)? {
            return Ok(());
        }
        if self.handle_config_command(&prompt)? {
            return Ok(());
        }
        if self.handle_trust_command(&prompt)? {
            return Ok(());
        }
        if self.handle_subtask_command(&prompt).await? {
            return Ok(());
        }
        if self.handle_queue_command(&prompt).await? {
            return Ok(());
        }
        if self.handle_provider_status_command(&prompt).await? {
            return Ok(());
        }
        if matches!(self.state.run_state, RunState::WaitingForUser) {
            if self.state.pending_approval.is_some() {
                bail!("a run is waiting for action approval");
            }
            if self.pending_clarification.is_some() {
                bail!("a run is waiting for clarification; use the structured clarification answer event");
            }
        }
        let workflow_start = self.handle_workflow_command(&prompt)?;
        if workflow_start.is_none() {
            reject_unknown_slash_command(&prompt)?;
        }
        if matches!(
            self.state.run_state,
            RunState::Planning | RunState::Running | RunState::WaitingForUser
        ) {
            bail!("a run is already active");
        }
        let compiled_prompt = compile_app_prompt(
            &self.config.working_directory,
            workflow_start
                .as_ref()
                .map_or(prompt.as_str(), |start| start.command.prompt.as_str()),
        )?;
        // Show the prompt as the user typed it so `/skill:` references stay
        // visible in chat (mirrors the workflow branch below, which displays
        // `original_command`). `run_prompt` stays stripped for the runtime.
        let mut visible_prompt = compiled_prompt.submitted_prompt.clone();
        let mut run_prompt = compiled_prompt.user_prompt.clone();
        let mut submitted_prompt = compiled_prompt.submitted_prompt.clone();
        if let Some(start) = workflow_start.as_ref() {
            visible_prompt = start.command.original_command.clone();
            submitted_prompt = start.command.original_command.clone();
            run_prompt = workflow_runtime_prompt(
                &start.command,
                &compiled_prompt.user_prompt,
                &start.preflight,
            );
        }

        self.reset_enabled_agent_statuses();
        let run_id = new_id();
        self.state.active_run_id = Some(run_id.clone());
        self.state.run_state = RunState::Planning;
        // Record the user's prompt before `run_started` so the prompt renders
        // above the run's "started" summary in chat. The user submitted, then
        // the run begins; the run summary later moves to the end on completion.
        self.record_event(
            Some(run_id.clone()),
            None,
            "prompt_submitted",
            json!({
                "prompt": visible_prompt.clone(),
                "submitted_prompt": submitted_prompt.clone(),
                "source": source.as_str(),
            }),
            user_event_display(&visible_prompt),
        )?;
        self.record_event(
            Some(run_id.clone()),
            None,
            "run_started",
            json!({
                "run_id": run_id,
                // Resume drift baseline (ADR-007): the short HEAD as of the last
                // git poll, recoverable later by folding the log.
                "head_sha": self
                    .state
                    .git_context
                    .as_ref()
                    .and_then(|context| context.head_sha.clone()),
            }),
            "Run started.",
        )?;
        if let Some(start) = workflow_start.as_ref() {
            self.record_event(
                Some(run_id.clone()),
                None,
                "workflow_started",
                workflow_started_payload(&run_id, start),
                "Workflow started.",
            )?;
        }
        self.record_skills_loaded(run_id.as_str(), compiled_prompt.skill_context.as_ref())?;
        // Snapshot the MCP tool catalog at run entry (ADR-001, task_07): records
        // the `mcp_catalog_snapshot` event and refreshes the cached snapshot the
        // orchestrator prompt advertises from. No-op when MCP is disabled.
        self.record_mcp_catalog_snapshot(run_id.as_str()).await?;

        let run = RunDriveContext::new(
            run_id,
            None,
            submitted_prompt,
            run_prompt,
            compiled_prompt.skill_context,
            None,
            workflow_start
                .as_ref()
                .map(|start| WorkflowRunContext::new(&start.command)),
        );
        self.drive_and_replay(run, None).await
    }

    fn handle_workflow_command(&self, prompt: &str) -> Result<Option<WorkflowStart>> {
        let command = parse_workflow_command(prompt)?;
        command
            .map(|command| {
                let preflight = preflight_workflow_prerequisites(&self.config)?;
                Ok(WorkflowStart { command, preflight })
            })
            .transpose()
    }

    fn handle_goal_command(&mut self, prompt: &str) -> Result<bool> {
        let trimmed = prompt.trim();
        if trimmed == "/goal" {
            let display = self
                .state
                .session_goal
                .as_deref()
                .map(|goal| format!("Goal: {}", single_line_event_text(goal)))
                .unwrap_or_else(|| "No active goal.".to_string());
            self.record_event(
                None,
                None,
                "session_goal_viewed",
                json!({ "goal": self.state.session_goal.clone() }),
                display,
            )?;
            return Ok(true);
        }

        if trimmed == "/goal clear" {
            let previous_goal = self.state.session_goal.take();
            self.record_event(
                None,
                None,
                "session_goal_cleared",
                json!({ "previous_goal": previous_goal }),
                "Goal cleared.",
            )?;
            return Ok(true);
        }

        if let Some(goal) = trimmed.strip_prefix("/goal ") {
            let goal = goal.trim();
            if goal.is_empty() {
                bail!("usage: /goal <text>");
            }
            self.state.session_goal = Some(goal.to_string());
            self.record_event(
                None,
                None,
                "session_goal_set",
                json!({ "goal": goal }),
                "Goal set.",
            )?;
            return Ok(true);
        }

        if trimmed.starts_with("/goal") {
            bail!("usage: /goal, /goal <text>, or /goal clear");
        }

        Ok(false)
    }

    fn handle_config_command(&mut self, prompt: &str) -> Result<bool> {
        let trimmed = prompt.trim();
        if trimmed != "/config" {
            if trimmed.starts_with("/config") {
                bail!("usage: /config");
            }
            return Ok(false);
        }

        self.record_event(
            None,
            None,
            "config_viewed",
            serde_json::to_value(&self.state.config_status)?,
            config_status_display(&self.state.config_status),
        )?;
        Ok(true)
    }

    /// `/trust` session-trust management (ADR-004): list entries, revoke one by
    /// 1-based index from the last listing, or clear all. Malformed input reports
    /// a usage error and never mutates the store. Follows the
    /// `handle_config_command`/`handle_goal_command` pattern.
    fn handle_trust_command(&mut self, prompt: &str) -> Result<bool> {
        let trimmed = prompt.trim();
        if trimmed != "/trust" && !trimmed.starts_with("/trust ") {
            if trimmed.starts_with("/trust") {
                bail!("usage: /trust | /trust revoke <n> | /trust clear");
            }
            return Ok(false);
        }

        let args = trimmed.strip_prefix("/trust").unwrap_or("").trim();
        if args.is_empty() {
            return self.trust_list();
        }
        let mut parts = args.split_whitespace();
        match parts.next() {
            Some("revoke") => {
                let token = parts.next();
                if parts.next().is_some() {
                    bail!("usage: /trust revoke <n>");
                }
                let Some(token) = token else {
                    bail!("usage: /trust revoke <n>");
                };
                let Ok(index) = token.parse::<usize>() else {
                    bail!("usage: /trust revoke <n> (n must be a positive number)");
                };
                self.trust_revoke(index)
            }
            Some("clear") => {
                if parts.next().is_some() {
                    bail!("usage: /trust clear");
                }
                self.trust_clear()
            }
            _ => bail!("usage: /trust | /trust revoke <n> | /trust clear"),
        }
    }

    fn trust_list(&mut self) -> Result<bool> {
        let entries = self.trust_store.list();
        let display = if entries.is_empty() {
            "No trusted actions this session.".to_string()
        } else {
            let mut lines = vec![format!(
                "{} trusted action(s), this session only:",
                entries.len()
            )];
            for (index, target) in entries.iter().enumerate() {
                lines.push(format!("  {}. {}", index + 1, trust_target_label(target)));
            }
            lines.join("\n")
        };
        let payload = json!({
            "count": entries.len(),
            "entries": entries.iter().map(trust_target_payload).collect::<Vec<_>>(),
            "message": display.clone(),
        });
        self.record_event(None, None, "trust_listed", payload, display)?;
        Ok(true)
    }

    fn trust_revoke(&mut self, one_based: usize) -> Result<bool> {
        let Some(target) = self.trust_store.revoke_index(one_based) else {
            bail!(
                "no trusted entry at index {one_based}; run /trust to see the current numbered list"
            );
        };
        self.record_event(
            None,
            None,
            "trust_revoked",
            json!({ "target": trust_target_payload(&target) }),
            format!("Trust revoked: {}", trust_target_label(&target)),
        )?;
        Ok(true)
    }

    fn trust_clear(&mut self) -> Result<bool> {
        let count = self.trust_store.list().len();
        self.trust_store.clear();
        self.record_event(
            None,
            None,
            "trust_cleared",
            json!({ "count": count }),
            format!(
                "Cleared {count} trusted entr{}",
                if count == 1 { "y" } else { "ies" }
            ),
        )?;
        Ok(true)
    }

    async fn handle_provider_status_command(&mut self, prompt: &str) -> Result<bool> {
        let trimmed = prompt.trim();
        if trimmed != "/provider:status" {
            // Only the exact (optionally whitespace-trimmed) command is handled
            // here. `/provider:statusFOO` is a malformed provider-status call;
            // unrelated commands like `/provider:unknown` fall through to the
            // existing unknown-command guard untouched.
            if trimmed.starts_with("/provider:status") {
                bail!("usage: /provider:status");
            }
            return Ok(false);
        }

        let report = self.provider_status_report().await;
        self.record_event(
            None,
            None,
            "provider_status_viewed",
            json!({ "report": report.clone() }),
            report,
        )?;
        Ok(true)
    }

    /// Collect provider runway status via the runtime status service and render
    /// it with the compact share-safe formatter. Provider discovery, probing,
    /// timeouts, normalization, and redaction all stay behind `runtime::status`;
    /// the app layer only triggers the bounded service and delivers the result.
    async fn provider_status_report(&self) -> String {
        let service = crate::runtime::status::ProviderStatusService::from_config(&self.config);
        let statuses = service.collect_status().await;
        crate::runtime::status::render_provider_status(&statuses)
    }

    async fn handle_queue_command(&mut self, prompt: &str) -> Result<bool> {
        let Some(message) = parse_queue_command(prompt)? else {
            return Ok(false);
        };
        let item = QueuedFollowUp::new(message);
        let view = item.to_view();
        self.follow_up_queue.push_back(item);
        self.sync_queued_follow_ups();
        self.record_event(
            self.state.active_run_id.clone(),
            None,
            "follow_up_queued",
            json!({
                "id": view.id,
                "prompt": view.prompt,
                "created_at": view.created_at,
                "status": view.status,
            }),
            format!("Queued follow-up: {}", single_line_event_text(&view.prompt)),
        )?;
        // The worker drives a run to completion before it can service the next
        // command, so a `/queue` submitted *during* a run is only processed once
        // that run has already ended — after its run-end drain has passed. When
        // no run is in flight, run the run-end handling now so the new item is
        // drained on a clean completion or paused (with the right reason) after
        // a non-clean ending, instead of sitting Pending with nothing to trigger
        // it. A run that is `WaitingForUser` still owns `active_run_id`, so it is
        // excluded here — its queued items stay Pending and replay once the user
        // resolves the wait, rather than being wrongly paused.
        if self.state.active_run_id.is_none() {
            self.react_to_run_end_for_queue().await?;
        }
        Ok(true)
    }

    /// Drive a run to its terminal/waiting state, then replay or pause queued
    /// follow-ups based on how the run ended.
    async fn drive_and_replay(
        &mut self,
        run: RunDriveContext,
        resume: Option<StepResume>,
    ) -> Result<()> {
        self.drive_run(run, resume).await?;
        self.react_to_run_end_for_queue().await
    }

    /// Replay is only safe immediately after a clean completed Run with no
    /// outstanding user input and no active Run.
    fn can_replay_now(&self) -> bool {
        matches!(self.state.run_state, RunState::Completed)
            && self.state.active_run_id.is_none()
            && self.pending_approval.is_none()
            && self.pending_clarification.is_none()
            && self.pending_governance_decision.is_none()
            && self.pending_plan_approval.is_none()
    }

    /// React to the Run that just ended: drain queued follow-ups FIFO after a
    /// clean completion (chaining across each replayed Run's completion), or
    /// pause the oldest pending item after a non-clean ending. Replays at most
    /// one item per completed Run and preserves the one-active-Run invariant.
    async fn react_to_run_end_for_queue(&mut self) -> Result<()> {
        loop {
            if self.can_replay_now() {
                let Some(prompt) = self.pop_oldest_pending_for_replay()? else {
                    break;
                };
                let run = self.build_follow_up_run(prompt)?;
                self.drive_run(run, None).await?;
            } else if matches!(
                self.state.run_state,
                RunState::Failed
                    | RunState::Interrupted
                    | RunState::LimitReached
                    | RunState::WaitingForUser
            ) {
                self.pause_oldest_pending_for_queue()?;
                break;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn first_pending_index(&self) -> Option<usize> {
        self.follow_up_queue
            .iter()
            .position(|item| item.status == QueuedFollowUpStatus::Pending)
    }

    /// Remove the oldest pending follow-up and record that it began replaying.
    /// The item leaves the queue because it becomes a normal Run.
    fn pop_oldest_pending_for_replay(&mut self) -> Result<Option<String>> {
        let Some(index) = self.first_pending_index() else {
            return Ok(None);
        };
        let item = self
            .follow_up_queue
            .remove(index)
            .expect("first_pending_index returns a valid index");
        self.sync_queued_follow_ups();
        self.record_event(
            self.state.active_run_id.clone(),
            None,
            "follow_up_replay_started",
            json!({
                "id": item.id,
                "prompt": item.prompt,
            }),
            format!(
                "Replaying queued follow-up: {}",
                single_line_event_text(&item.prompt)
            ),
        )?;
        Ok(Some(item.prompt))
    }

    /// Pause the oldest pending follow-up with a reason describing the unsafe
    /// Run ending. No-op when nothing is pending.
    fn pause_oldest_pending_for_queue(&mut self) -> Result<()> {
        let reason = self.queue_pause_reason();
        let Some(index) = self.first_pending_index() else {
            return Ok(());
        };
        let item = &mut self.follow_up_queue[index];
        item.status = QueuedFollowUpStatus::Paused;
        item.pause_reason = Some(reason.to_string());
        let view = item.to_view();
        self.sync_queued_follow_ups();
        self.record_event(
            self.state.active_run_id.clone(),
            None,
            "follow_up_replay_paused",
            json!({
                "id": view.id,
                "prompt": view.prompt,
                "status": view.status,
                "pause_reason": reason,
            }),
            format!(
                "Paused queued follow-up ({reason}): {}",
                single_line_event_text(&view.prompt)
            ),
        )?;
        Ok(())
    }

    fn queue_pause_reason(&self) -> &'static str {
        match self.state.run_state {
            RunState::Failed => "previous run failed",
            RunState::Interrupted => "previous run interrupted",
            RunState::LimitReached => "previous run reached a limit",
            RunState::WaitingForUser => {
                if self.pending_approval.is_some() {
                    "run is waiting for action approval"
                } else if self.pending_clarification.is_some() {
                    "run is waiting for clarification"
                } else if self.pending_governance_decision.is_some() {
                    "run is waiting for a governance decision"
                } else if self.pending_plan_approval.is_some() {
                    "run is waiting for plan approval"
                } else {
                    "run is waiting for user"
                }
            }
            _ => "queue paused",
        }
    }

    /// Build a normal Run for a replayed follow-up prompt, mirroring the
    /// run-creation path used by `submit_prompt` for ordinary prompts.
    fn build_follow_up_run(&mut self, prompt: String) -> Result<RunDriveContext> {
        let compiled_prompt = compile_app_prompt(&self.config.working_directory, &prompt)?;
        // Keep `/skill:` references visible in the replayed prompt (matches
        // `submit_prompt`); `run_prompt` stays stripped for the runtime.
        let visible_prompt = compiled_prompt.submitted_prompt.clone();
        let run_prompt = compiled_prompt.user_prompt.clone();
        let submitted_prompt = compiled_prompt.submitted_prompt.clone();

        self.reset_enabled_agent_statuses();
        let run_id = new_id();
        self.state.active_run_id = Some(run_id.clone());
        self.state.run_state = RunState::Planning;
        // Prompt before `run_started` so the prompt renders above the run's
        // "started" summary in chat (matches `submit_prompt`).
        self.record_event(
            Some(run_id.clone()),
            None,
            "prompt_submitted",
            json!({
                "prompt": visible_prompt.clone(),
                "submitted_prompt": submitted_prompt.clone(),
                // A queued follow-up is a user-typed prompt that was deferred, so its
                // provenance is `Fresh` — mirroring `submit_prompt` so replayed events
                // carry the same schema and stay distinguishable from legacy
                // pre-`PromptSource` events (which read back as `Fresh` by default).
                "source": PromptSource::Fresh.as_str(),
            }),
            user_event_display(&visible_prompt),
        )?;
        self.record_event(
            Some(run_id.clone()),
            None,
            "run_started",
            json!({
                "run_id": run_id,
                // Resume drift baseline (ADR-007): the short HEAD as of the last
                // git poll, recoverable later by folding the log.
                "head_sha": self
                    .state
                    .git_context
                    .as_ref()
                    .and_then(|context| context.head_sha.clone()),
            }),
            "Run started.",
        )?;
        self.record_skills_loaded(run_id.as_str(), compiled_prompt.skill_context.as_ref())?;
        Ok(RunDriveContext::new(
            run_id,
            None,
            submitted_prompt,
            run_prompt,
            compiled_prompt.skill_context,
            None,
            None,
        ))
    }

    /// Cancel a queued follow-up before it begins replaying.
    fn cancel_follow_up(&mut self, id: &str) -> Result<()> {
        let Some(item) = self.follow_up_queue.iter_mut().find(|item| item.id == id) else {
            bail!("no queued follow-up with id {id}");
        };
        if item.status == QueuedFollowUpStatus::Cancelled {
            return Ok(());
        }
        item.status = QueuedFollowUpStatus::Cancelled;
        item.pause_reason = None;
        let view = item.to_view();
        self.sync_queued_follow_ups();
        self.record_event(
            self.state.active_run_id.clone(),
            None,
            "follow_up_cancelled",
            json!({
                "id": view.id,
                "prompt": view.prompt,
                "status": view.status,
            }),
            format!(
                "Cancelled queued follow-up: {}",
                single_line_event_text(&view.prompt)
            ),
        )?;
        Ok(())
    }

    /// Resume a paused follow-up, making it eligible for replay again.
    fn resume_follow_up(&mut self, id: &str) -> Result<()> {
        let Some(item) = self.follow_up_queue.iter_mut().find(|item| item.id == id) else {
            bail!("no queued follow-up with id {id}");
        };
        if item.status != QueuedFollowUpStatus::Paused {
            bail!("queued follow-up {id} is not paused");
        }
        item.status = QueuedFollowUpStatus::Pending;
        item.pause_reason = None;
        let view = item.to_view();
        self.sync_queued_follow_ups();
        self.record_event(
            self.state.active_run_id.clone(),
            None,
            "follow_up_replay_resumed",
            json!({
                "id": view.id,
                "prompt": view.prompt,
                "status": view.status,
            }),
            format!(
                "Resumed queued follow-up: {}",
                single_line_event_text(&view.prompt)
            ),
        )?;
        Ok(())
    }

    async fn handle_subtask_command(&mut self, prompt: &str) -> Result<bool> {
        let trimmed = prompt.trim();
        if !trimmed.starts_with("/subtask") {
            return Ok(false);
        }
        let Some(rest) = trimmed.strip_prefix("/subtask ") else {
            bail!("usage: /subtask <agent> <task>");
        };
        let (agent_id, task) = parse_subtask_command(rest)?;
        if matches!(
            self.state.run_state,
            RunState::Planning | RunState::Running | RunState::WaitingForUser
        ) {
            bail!("a run is already active");
        }
        self.start_subtask(agent_id, task).await?;
        Ok(true)
    }

    async fn start_subtask(&mut self, agent_id: &str, task: &str) -> Result<()> {
        if agent_id == "orchestrator" {
            bail!("subtasks must target a specialized agent, not orchestrator");
        }
        let agent = self.agent(agent_id)?.clone();
        if !agent.enabled {
            bail!("subtask references disabled agent {agent_id}");
        }
        let compiled_task = compile_app_prompt(&self.config.working_directory, task)?;

        self.reset_enabled_agent_statuses();
        let run_id = new_id();
        let parent_run_id = self.state.active_run_id.clone();
        self.state.active_run_id = Some(run_id.clone());
        self.state.run_state = RunState::Running;
        let submitted_prompt = subtask_prompt(&compiled_task.submitted_prompt);
        let prompt = subtask_prompt(&compiled_task.user_prompt);
        self.record_event(
            Some(run_id.clone()),
            None,
            "subtask_started",
            json!({
                "run_id": run_id,
                "parent_run_id": parent_run_id,
                "agent": agent_id,
                "request": compiled_task.user_prompt.clone(),
                "submitted_request": compiled_task.submitted_prompt.clone(),
            }),
            format!("Subtask started: {agent_id}."),
        )?;
        self.record_skills_loaded(run_id.as_str(), compiled_task.skill_context.as_ref())?;
        let run = RunDriveContext::new(
            run_id,
            parent_run_id,
            submitted_prompt,
            prompt,
            compiled_task.skill_context,
            Some(SubtaskContext {
                agent_id: agent.id.clone(),
                request: compiled_task.user_prompt,
                submitted_request: compiled_task.submitted_prompt,
            }),
            None,
        );
        self.drive_and_replay(run, None).await
    }

    pub async fn resolve_pending_approval(
        &mut self,
        resolution: ApprovalResolution,
    ) -> Result<Option<ActionResult>> {
        let Some(mut pending) = self.pending_approval.take() else {
            return Ok(None);
        };
        self.state.pending_approval = None;
        let approved = resolution.approves();
        // Keep the explainer flag in sync with approval presence (none pending → false).
        self.state.show_first_approval_explainer = false;
        self.record_event(
            Some(pending.run_id.clone()),
            Some(pending.step_id.clone()),
            "approval_resolved",
            json!({
                "action_id": pending.action_request.action_id.clone(),
                "approved": approved,
                "resolution": approval_resolution_label(resolution),
            }),
            if approved {
                "Action approval granted."
            } else {
                "Action approval denied."
            },
        )?;

        // Approve-and-trust grants the exact target into the session trust list,
        // but only for non-catastrophic actions (catastrophic exposes no target,
        // ADR-004). Recorded as `trust_granted` so trust stays auditable.
        if resolution.grants_trust() {
            if let Some(target) =
                crate::actions::assess_risk(&pending.action_request, &pending.context).target
            {
                if self.trust_store.grant(target.clone()) {
                    self.record_event(
                        Some(pending.run_id.clone()),
                        Some(pending.step_id.clone()),
                        "trust_granted",
                        json!({ "target": trust_target_payload(&target) }),
                        format!("Trusted for this session: {}", trust_target_label(&target)),
                    )?;
                }
            }
            // For an MCP tool call, approve-and-trust promotes the ORIGIN SERVER
            // in the durable trust store (ADR-006/007) — not the session list —
            // so future calls from it auto-allow until revoked. `promote_mcp_server`
            // persists the file and records `mcp_server_trusted`.
            if pending.action_request.kind == ActionKind::CallMcpTool {
                if let Some(server) = pending
                    .action_request
                    .params
                    .get("server")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                {
                    self.promote_mcp_server(&server)?;
                }
            }
        }

        if self.wall_clock_limit_reached(&pending.run) {
            self.stop_for_wall_clock_limit(&pending.run)?;
            self.write_run_record(&pending.run)?;
            self.state.active_run_id = None;
            self.pause_oldest_pending_for_queue()?;
            return Ok(None);
        }
        if self.step_time_limit_reached(pending.step_started_at) {
            self.stop_for_step_time_limit(&pending.run, &pending.step_id, pending.step_started_at)?;
            self.write_run_record(&pending.run)?;
            self.state.active_run_id = None;
            self.pause_oldest_pending_for_queue()?;
            return Ok(None);
        }

        let result = if approved {
            // The user approved this exact action; mark the context pre-approved so
            // the re-run runs it even when catastrophic or floor=Enforce (which
            // would otherwise re-prompt).
            pending.context.pre_approved = true;
            // Approving the first mutating action after a drifted resume is the
            // drift acknowledgment (ADR-004): record it and disarm the gate.
            self.acknowledge_resume_drift(&pending.action_request)?;
            self.record_command_started_if_executable(
                &pending.run.run_id,
                &pending.step_id,
                &pending.agent_profile,
                &pending.context,
                &pending.action_request,
            )?;
            let action = execute_action_request(
                &pending.agent_profile,
                &pending.context,
                &pending.action_request,
            );
            let Some(result) = await_with_step_limit(
                action,
                &self.config.limits.max_step_minutes,
                pending.step_started_at,
            )
            .await
            else {
                self.stop_for_step_time_limit(
                    &pending.run,
                    &pending.step_id,
                    pending.step_started_at,
                )?;
                self.write_run_record(&pending.run)?;
                self.state.active_run_id = None;
                self.pause_oldest_pending_for_queue()?;
                return Ok(None);
            };
            result
        } else {
            ActionResult::approval_denied(
                &pending.action_request,
                pending
                    .reason
                    .unwrap_or_else(|| "user denied action approval".to_string()),
            )
        };
        self.record_action_specific_events(
            &pending.run_id,
            &pending.step_id,
            &pending.action_request,
            &result,
        )?;
        self.record_action_completed(
            &pending.run_id,
            &pending.step_id,
            &pending.agent_profile.runtime,
            &pending.action_request,
            &result,
        )?;
        pending.request.action_results.push(result.clone());
        let resume = StepResume {
            step: pending.step,
            step_started_at: pending.step_started_at,
            request: pending.request,
        };
        self.drive_and_replay(pending.run, Some(resume)).await?;
        Ok(Some(result))
    }

    pub async fn resolve_pending_clarification(
        &mut self,
        answer: ClarificationAnswer,
    ) -> Result<()> {
        let Some(clarification_view) = &self.state.pending_clarification else {
            bail!("no clarification is pending");
        };

        if answer.question_id != clarification_view.question_id {
            return Err(anyhow!(
                "answer question id does not match pending clarification (expected: {}, got: {})",
                clarification_view.question_id,
                answer.question_id
            ));
        }

        if answer.answer.trim().is_empty() {
            return Err(anyhow!("clarification answer cannot be empty"));
        }

        let Some(pending) = self.pending_clarification.take() else {
            bail!("no clarification is pending");
        };

        self.record_event(
            Some(pending.run.run_id.clone()),
            None,
            "clarification_answered",
            json!({
                "question_id": answer.question_id.clone(),
                "answer": answer.answer.clone(),
                "answer_source": answer.answer_source.clone(),
                "selected_option_id": answer.selected_option_id.clone(),
                "selected_option_label": answer.selected_option_label.clone(),
            }),
            user_event_display(&answer.answer),
        )?;

        let mut run = pending.run;
        run.prompt = format!("{}\n\nUser clarification: {}", run.prompt, answer.answer);

        self.state.pending_clarification = None;
        self.state.run_state = RunState::Planning;
        self.publish_state();

        self.drive_and_replay(run, None).await?;
        Ok(())
    }

    /// Resolve the pending governance decision, mirroring
    /// `resolve_pending_clarification` over the same pause/resume transport.
    ///
    /// `decision_id` is the id the user is answering; it must match the pending
    /// decision (the shared `GovernanceAnswer` carries no id, so the caller
    /// threads it through — ADR-003). `Accept` resumes the captured run via
    /// `drive_and_replay`; `Reject { redirect: Some }` appends the redirect to
    /// the prompt and re-drives; `Reject { redirect: None }` aborts the run.
    pub async fn resolve_pending_governance_decision(
        &mut self,
        decision_id: &str,
        answer: GovernanceAnswer,
    ) -> Result<()> {
        let Some(view) = self.state.pending_governance_decision.as_ref() else {
            bail!("no governance decision is pending");
        };

        if view.decision_id != decision_id {
            return Err(anyhow!(
                "decision id does not match pending governance decision (expected: {}, got: {})",
                view.decision_id,
                decision_id
            ));
        }

        let Some(pending) = self.pending_governance_decision.take() else {
            bail!("no governance decision is pending");
        };
        let run_id = pending.run.run_id.clone();

        match answer {
            GovernanceAnswer::Accept => {
                self.record_event(
                    Some(run_id.clone()),
                    None,
                    GOVERNANCE_DECISION_RESOLVED,
                    json!({ "decision_id": decision_id, "outcome": "accept" }),
                    "Governance decision accepted; run continues.",
                )?;

                self.state.pending_governance_decision = None;
                self.state.run_state = RunState::Planning;
                self.publish_state();

                self.drive_and_replay(pending.run, None).await?;
            }
            GovernanceAnswer::Reject {
                redirect: Some(redirect),
            } => {
                self.record_event(
                    Some(run_id.clone()),
                    None,
                    GOVERNANCE_DECISION_RESOLVED,
                    json!({
                        "decision_id": decision_id,
                        "outcome": "reject",
                        "redirect": redirect.clone(),
                    }),
                    user_event_display(&redirect),
                )?;

                let mut run = pending.run;
                let redirect = redirect.trim();
                if !redirect.is_empty() {
                    run.prompt = format!("{}\n\nUser redirect: {}", run.prompt, redirect);
                }

                self.state.pending_governance_decision = None;
                self.state.run_state = RunState::Planning;
                self.publish_state();

                self.drive_and_replay(run, None).await?;
            }
            GovernanceAnswer::Reject { redirect: None } => {
                self.record_event(
                    Some(run_id.clone()),
                    None,
                    GOVERNANCE_DECISION_RESOLVED,
                    json!({ "decision_id": decision_id, "outcome": "reject" }),
                    "Governance decision rejected; run aborted.",
                )?;
                self.record_event(
                    Some(run_id.clone()),
                    None,
                    "run_interrupted",
                    json!({ "reason": "governance_decision_rejected" }),
                    "Run aborted before any action.",
                )?;

                // Finalize state to Interrupted BEFORE persisting and pausing the queue:
                // write_run_record snapshots self.state.run_state, and the queue-pause
                // reason is derived from it — both must see the interrupted state.
                self.state.pending_governance_decision = None;
                self.state.active_run_id = None;
                self.state.live_step = None;
                self.state.live_steps.clear();
                self.state.run_state = RunState::Interrupted;
                self.write_run_record(&pending.run)?;
                self.sync_chat_items();
                self.publish_state();
                // Follow-ups queued during the governance pause must not stay stuck,
                // mirroring the interrupt() path.
                self.pause_oldest_pending_for_queue()?;
            }
        }

        Ok(())
    }

    pub fn interrupt(&mut self) -> Result<()> {
        if self.state.active_run_id.is_some() {
            self.state.run_state = RunState::Interrupted;
            let run_id = self.state.active_run_id.clone();
            let run_record = self
                .pending_approval
                .as_ref()
                .map(|pending| pending.run.clone())
                .or_else(|| {
                    self.pending_clarification
                        .as_ref()
                        .map(|pending| pending.run.clone())
                })
                .or_else(|| {
                    self.pending_governance_decision
                        .as_ref()
                        .map(|pending| pending.run.clone())
                })
                .or_else(|| {
                    self.pending_plan_approval
                        .as_ref()
                        .map(|pending| pending.run.clone())
                });
            self.record_step_cancelled_if_active()?;
            self.record_event(
                run_id,
                None,
                "run_interrupted",
                json!({}),
                "Run interrupted.",
            )?;
            if let Some(run) = run_record {
                self.write_run_record(&run)?;
            }
            self.state.active_run_id = None;
            self.state.live_step = None;
            self.state.live_steps.clear();
            self.state.pending_approval = None;
            // Keep the explainer flag in sync with approval presence (none pending → false).
            self.state.show_first_approval_explainer = false;
            self.state.pending_clarification = None;
            self.state.pending_governance_decision = None;
            self.state.pending_plan_approval = None;
            self.pending_approval = None;
            self.pending_clarification = None;
            self.pending_governance_decision = None;
            self.pending_plan_approval = None;
            self.active_step = None;
            self.active_steps.clear();
            self.sync_chat_items();
            self.publish_state();
            self.pause_oldest_pending_for_queue()?;
        }
        Ok(())
    }

    async fn drive_run(
        &mut self,
        mut run: RunDriveContext,
        resume: Option<StepResume>,
    ) -> Result<()> {
        let drive_result = self.drive_run_inner(&mut run, resume).await;
        if drive_result.is_ok() {
            self.finalize_run_record(&mut run)?;
        }
        drive_result
    }

    /// Persist the run record and release `active_run_id` at run end, unless the
    /// run paused for the user (then the record is written but the run stays
    /// active). Shared by `drive_run` and the plan-approval accept path so a
    /// directly-driven graph that ends on a limit/interrupt isn't left with a
    /// stale record or a dangling active run.
    fn finalize_run_record(&mut self, run: &mut RunDriveContext) -> Result<()> {
        if !matches!(self.state.run_state, RunState::WaitingForUser) {
            self.record_workflow_completed(
                run,
                matches!(self.state.run_state, RunState::Interrupted),
            )?;
        }
        self.write_run_record(run)?;
        if !matches!(self.state.run_state, RunState::WaitingForUser) {
            self.state.active_run_id = None;
            self.publish_state();
        }
        Ok(())
    }

    async fn drive_run_inner(
        &mut self,
        run: &mut RunDriveContext,
        resume: Option<StepResume>,
    ) -> Result<()> {
        if let Some(resume) = resume {
            match resume.step {
                PausedStep::Orchestrator { step_id } => {
                    match self
                        .run_orchestrator_step(
                            run,
                            Some((step_id, resume.request, resume.step_started_at)),
                        )
                        .await?
                    {
                        OrchestratorStepOutcome::Decision(decision) => {
                            if !self.handle_orchestrator_decision(run, *decision).await? {
                                return Ok(());
                            }
                        }
                        OrchestratorStepOutcome::Retry => {}
                        OrchestratorStepOutcome::Paused | OrchestratorStepOutcome::Stop => {
                            return Ok(());
                        }
                    }
                }
                PausedStep::Agent { step_id, agent_id } => {
                    match self
                        .run_agent_step(
                            run,
                            &agent_id,
                            Some((step_id, resume.request, resume.step_started_at)),
                        )
                        .await?
                    {
                        AgentStepOutcome::Completed => {}
                        AgentStepOutcome::Paused | AgentStepOutcome::Stop => return Ok(()),
                    }
                    if run.subtask.is_some() {
                        self.finish_subtask_run(run)?;
                        return Ok(());
                    }
                }
            }
        }

        if let Some(subtask) = run.subtask.clone() {
            match self.run_agent_step(run, &subtask.agent_id, None).await? {
                AgentStepOutcome::Completed => {
                    self.finish_subtask_run(run)?;
                }
                AgentStepOutcome::Paused | AgentStepOutcome::Stop => {}
            }
            return Ok(());
        }

        loop {
            match self.run_orchestrator_step(run, None).await? {
                OrchestratorStepOutcome::Decision(decision) => {
                    // Governance early-abort (ADR-004): on the first orchestrator
                    // turn of a non-trivial single-agent write run, pause for an
                    // intent check before `handle_orchestrator_decision` runs any
                    // agent/action. Gated entirely behind the feature flag.
                    if self.config.features.governance_early_abort
                        && early_abort_triggers(run, decision.as_ref())
                    {
                        self.pause_for_early_abort(run, decision.as_ref())?;
                        break;
                    }
                    if !self.handle_orchestrator_decision(run, *decision).await? {
                        break;
                    }
                }
                OrchestratorStepOutcome::Retry => continue,
                OrchestratorStepOutcome::Paused | OrchestratorStepOutcome::Stop => break,
            }
        }

        Ok(())
    }

    /// Build the governance decision view and pause the run into
    /// `pending_governance_decision`, recording the `governance_decision_requested`
    /// event. Mirrors the clarification pause; no agent or action has run yet, so
    /// nothing is written before the user resolves the decision.
    fn pause_for_early_abort(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
    ) -> Result<()> {
        let agent = match decision.normalized_next_step() {
            Ok(Some(DecisionNextStep::SingleAgent(plan))) => Some(plan.agent),
            _ => None,
        };
        let decision_id = new_id();
        let view = GovernanceDecisionView {
            run_id: run.run_id.clone(),
            decision_id: decision_id.clone(),
            kind: GovernanceKind::EarlyAbort,
            title: "Confirm intent before this run edits files".to_string(),
            intent: decision.reason.clone(),
            approach: decision.plan.clone(),
            agent,
            write_scope: self.governance_write_scope(),
            risk_label: early_abort_risk_label(decision),
            plan: None,
        };

        self.state.run_state = RunState::WaitingForUser;
        self.pending_governance_decision = Some(PendingGovernanceDecision { run: run.clone() });
        self.state.pending_governance_decision = Some(PendingGovernanceDecisionView {
            run_id: run.run_id.clone(),
            decision_id: decision_id.clone(),
            view: view.clone(),
        });
        self.set_agent_status("orchestrator", "waiting_for_user");
        // `record_event` applies the event to the chat projection and publishes
        // the (already-set) paused state, so the decision card renders.
        self.record_event(
            Some(run.run_id.clone()),
            None,
            GOVERNANCE_DECISION_REQUESTED,
            serde_json::to_value(&view)?,
            "Confirm intent before this run edits files.",
        )?;
        Ok(())
    }

    /// The workspace write-roots this run may touch, rendered for display: the
    /// working directory plus any configured extra write roots (ADR-004 — no new
    /// per-step `file_scope`).
    fn governance_write_scope(&self) -> Vec<String> {
        let mut roots = vec![self.config.working_directory.display().to_string()];
        for root in &self.config.workspace.extra_write_roots {
            roots.push(root.display().to_string());
        }
        roots
    }

    async fn handle_orchestrator_decision(
        &mut self,
        run: &mut RunDriveContext,
        decision: crate::orchestrator::OrchestratorDecision,
    ) -> Result<bool> {
        match decision.status {
            DecisionStatus::Continue => {
                let next_step = decision
                    .normalized_next_step()
                    .context("failed to normalize orchestrator next step")?
                    .context("validated continue decision missing next_step")?;
                match next_step {
                    DecisionNextStep::SingleAgent(plan) => {
                        self.handle_single_agent_decision(run, &decision, &plan.agent)
                            .await
                    }
                    DecisionNextStep::ParallelGroup(group) => {
                        self.handle_parallel_group_decision(run, &decision, group)
                            .await
                    }
                    DecisionNextStep::Dag(graph) => {
                        // Propose the plan, then gate (normal) or auto-accept
                        // (yolo) before the scheduler admits any node.
                        self.handle_execution_graph_decision(run, &decision, graph)
                            .await
                    }
                }
            }
            DecisionStatus::WaitingForUser => {
                self.state.run_state = RunState::WaitingForUser;
                self.pending_clarification = Some(PendingClarification { run: run.clone() });
                let question = decision
                    .clarifying_question
                    .clone()
                    .context("validated waiting_for_user decision missing clarifying_question")?;
                let view = PendingClarificationView {
                    run_id: run.run_id.clone(),
                    question_id: new_id(),
                    question,
                    options: decision.clarifying_options.clone(),
                    recommended_option_id: decision.recommended_option_id.clone(),
                    multi_select: decision.multi_select,
                };
                self.state.pending_clarification = Some(view.clone());
                self.set_agent_status("orchestrator", "waiting_for_user");
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "clarification_requested",
                    json!({
                        "question_id": view.question_id,
                        "question": view.question,
                        "options": view.options,
                        "recommended_option_id": view.recommended_option_id,
                        "multi_select": view.multi_select,
                    }),
                    "Orchestrator asked a clarifying question.",
                )?;
                Ok(false)
            }
            DecisionStatus::Complete => {
                self.state.run_state = RunState::Completed;
                self.record_workflow_completed(run, false)?;
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "run_completed",
                    json!({ "summary": decision.final_summary }),
                    "Run completed.",
                )?;
                Ok(false)
            }
            DecisionStatus::Failed => {
                self.state.run_state = RunState::Failed;
                self.record_workflow_completed(run, false)?;
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "run_failed",
                    json!({ "reason": decision.reason }),
                    "Run failed by orchestrator decision.",
                )?;
                Ok(false)
            }
        }
    }

    async fn handle_single_agent_decision(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
        next_agent_id: &str,
    ) -> Result<bool> {
        if next_agent_id == COUNCIL_WORKFLOW_AGENT_ID {
            if !council_route_allowed(&run.prompt, self.state.session_goal.as_deref()) {
                self.state.run_state = RunState::Failed;
                self.record_event(
                    Some(run.run_id.clone()),
                    Some(decision.decision_id.clone()),
                    "orchestrator_decision_invalid",
                    json!({
                        "reason": "council workflow can only be routed for high-risk or user-requested cases",
                        "decision": decision
                    }),
                    "Orchestrator routed council outside its allowed scope.",
                )?;
                return Ok(false);
            }
            return self.run_council_workflow(run, decision).await;
        }
        if self.review_fix_cycle_limit_reached(run, next_agent_id) {
            self.stop_for_review_fix_cycle_limit(run)?;
            return Ok(false);
        }
        match self.run_agent_step(run, next_agent_id, None).await? {
            AgentStepOutcome::Completed => Ok(true),
            AgentStepOutcome::Paused | AgentStepOutcome::Stop => Ok(false),
        }
    }

    async fn handle_parallel_group_decision(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
        group: ParallelGroupPlan,
    ) -> Result<bool> {
        if self.review_fix_cycle_limit_reached(run, "fixer")
            && group.steps.iter().any(|step| step.agent == "fixer")
        {
            self.stop_for_review_fix_cycle_limit(run)?;
            return Ok(false);
        }
        match self.run_parallel_group(run, decision, group).await? {
            AgentStepOutcome::Completed => Ok(true),
            AgentStepOutcome::Paused | AgentStepOutcome::Stop => Ok(false),
        }
    }

    async fn run_parallel_group(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
        group: ParallelGroupPlan,
    ) -> Result<AgentStepOutcome> {
        self.state.run_state = RunState::Running;
        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            return Ok(AgentStepOutcome::Stop);
        }
        if self.parallel_group_exceeds_agent_step_limit(run, &group) {
            self.stop_for_parallel_group_agent_step_limit(run, &group)?;
            return Ok(AgentStepOutcome::Stop);
        }

        let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let cancellation = CancellationToken::new();
        let interrupt_start = *self.interrupt_receiver.borrow();
        let approval_start = self.approval_receiver.borrow().sequence;
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group.group_id.clone()),
            None,
            "parallel_group_started",
            json!({
                "group_id": group.group_id,
                "decision_id": decision.decision_id,
                "reason": group.reason,
                "children": group.steps.len()
            }),
            "Parallel group started.",
        )?;

        let child_specs = self.prepare_parallel_children(run, &group)?;
        if let Some(workflow) = run.workflow.as_mut() {
            for child in &child_specs {
                workflow.record_planned_targets(
                    &group.group_id,
                    Some(&child.step_id),
                    &child.step_label,
                    &child.file_scope,
                    &self.config.working_directory,
                    &self.config.workspace.extra_write_roots,
                )?;
            }
        }
        let (sender, mut receiver) =
            mpsc::channel(RUNTIME_EVENT_CHANNEL_CAPACITY * child_specs.len().max(1));
        let resume_handle = ParallelRuntimeResumeHandle {
            cancellation: cancellation.clone(),
            sender: sender.clone(),
        };
        let mut children = BTreeMap::new();
        let mut coalescers = BTreeMap::new();
        for mut child in child_specs {
            child.cancellation = cancellation.child_token();
            self.record_event_with_group(
                Some(run.run_id.clone()),
                Some(group.group_id.clone()),
                Some(child.step_id.clone()),
                "parallel_child_started",
                json!({
                    "group_id": group.group_id,
                    "step_id": child.step_id,
                    "agent": child.agent_id,
                    "step_label": child.step_label,
                    "file_scope": child.file_scope
                }),
                format!("Parallel child started: {}.", child.step_label),
            )?;
            self.record_event_with_group(
                Some(run.run_id.clone()),
                Some(group.group_id.clone()),
                Some(child.step_id.clone()),
                "agent_step_started",
                json!({
                    "agent": child.agent_id,
                    "group_id": group.group_id,
                    "step_label": child.step_label,
                    "file_scope": child.file_scope
                }),
                format!("{} parallel step started.", child.agent_id),
            )?;
            self.set_active_step_with_metadata(
                &run.run_id,
                Some(group.group_id.clone()),
                &child.step_id,
                Some(child.step_label.clone()),
                Some(child.file_scope.clone()),
                &child.agent_id,
            );
            self.set_agent_status(&child.agent_id, "running_parallel");
            coalescers.insert(
                child.step_id.clone(),
                RuntimeStreamCoalescer::new(child.agent_id.clone()),
            );
            spawn_parallel_runtime_task(
                self.config.clone(),
                child.step_id.clone(),
                child.request.clone(),
                child.next_runtime_sequence,
                child.cancellation.clone(),
                sender.clone(),
            );
            child.next_runtime_sequence = child.next_runtime_sequence.saturating_add(1);
            children.insert(child.step_id.clone(), child);
        }
        let interrupt_requested =
            wait_for_interrupt(self.interrupt_receiver.clone(), interrupt_start);
        tokio::pin!(interrupt_requested);
        let mut approval_answered = Box::pin(wait_for_approval(
            self.approval_receiver.clone(),
            approval_start,
        ));
        let mut approval_queue: VecDeque<PendingParallelApproval> = VecDeque::new();
        let mut limit_tick = tokio::time::interval(Duration::from_millis(100));

        let mut interrupted = false;
        while children
            .values()
            .any(|child| child.terminal_result.is_none())
        {
            let message = tokio::select! {
                message = receiver.recv() => message,
                _ = &mut interrupt_requested => {
                    cancellation.cancel();
                    interrupted = true;
                    self.state.run_state = RunState::Interrupted;
                    self.state.pending_approval = None;
                    // Keep the explainer flag in sync with approval presence (none pending → false).
                    self.state.show_first_approval_explainer = false;
                    approval_queue.clear();
                    self.record_step_cancelled_if_active()?;
                    self.record_event(
                        Some(run.run_id.clone()),
                        None,
                        "run_interrupted",
                        json!({}),
                        "Run interrupted.",
                    )?;
                    for child in children.values_mut() {
                        if child.terminal_result.is_none() {
                            child.terminal_result = Some(cancelled_agent_result(
                                &child.agent_id,
                                &child.step_id,
                                "Parallel group cancelled by run interrupt.",
                            ));
                        }
                    }
                    None
                }
                approval = &mut approval_answered => {
                    approval_answered = Box::pin(wait_for_approval(
                        self.approval_receiver.clone(),
                        approval.sequence,
                    ));
                    if let Some(pending) = approval_queue.pop_front() {
                        self.publish_parallel_approval_head(&approval_queue)?;
                        if let Some(child) = children.get_mut(&pending.step_id) {
                            if child.terminal_result.is_none() {
                                self.resolve_parallel_approval(
                                    run,
                                    child,
                                    pending,
                                    approval.resolution,
                                    resume_handle.clone(),
                                )
                                .await?;
                            }
                        }
                        self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
                        self.publish_parallel_approval_head(&approval_queue)?;
                    }
                    continue;
                }
                _ = limit_tick.tick() => {
                    if self.apply_parallel_limit_checks(run, &mut children, &cancellation)? {
                        self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
                        self.publish_parallel_approval_head(&approval_queue)?;
                    }
                    continue;
                }
            };
            let Some(message) = message else {
                break;
            };
            match message {
                ParallelRuntimeMessage::RuntimeEvent { step_id, event } => {
                    if let Some(child) = children.get(&step_id) {
                        if child.terminal_result.is_some() {
                            continue;
                        }
                        let coalescer = coalescers
                            .get_mut(&step_id)
                            .context("missing parallel stream coalescer")?;
                        self.record_parallel_runtime_event(
                            run,
                            &group.group_id,
                            &step_id,
                            coalescer,
                            event,
                        )?;
                        self.set_agent_status(&child.agent_id, "running_parallel");
                    }
                }
                ParallelRuntimeMessage::Output { step_id, output } => {
                    let Some(child) = children.get_mut(&step_id) else {
                        continue;
                    };
                    if child.terminal_result.is_some() {
                        continue;
                    }
                    let output = *output;
                    if let Some(coalescer) = coalescers.get_mut(&step_id) {
                        self.flush_runtime_stream_coalescer_with_group(
                            run,
                            Some(&group.group_id),
                            &step_id,
                            coalescer,
                            true,
                        )?;
                    }
                    match output {
                        Ok(RuntimeOutput::AgentResult { result }) => {
                            self.record_parallel_child_result(run, &group.group_id, child, result)?;
                        }
                        Ok(RuntimeOutput::ActionRequest { request }) => {
                            if self
                                .handle_parallel_child_action(
                                    run,
                                    &group.group_id,
                                    child,
                                    request,
                                    resume_handle.clone(),
                                    &mut approval_queue,
                                )
                                .await?
                            {
                                continue;
                            }
                            self.publish_parallel_approval_head(&approval_queue)?;
                        }
                        Ok(RuntimeOutput::ParseError {
                            agent,
                            raw_output,
                            diagnostic,
                        }) => {
                            let result = self.persist_parse_error_with_group(
                                &run.run_id,
                                &group.group_id,
                                &step_id,
                                &agent,
                                raw_output,
                                diagnostic,
                            )?;
                            self.record_parallel_child_result(run, &group.group_id, child, result)?;
                        }
                        Ok(RuntimeOutput::OrchestratorDecision { .. }) => {
                            let result = failed_agent_result(
                                &child.agent_id,
                                &step_id,
                                "Parallel child returned an orchestrator decision.",
                            );
                            self.record_parallel_child_result(run, &group.group_id, child, result)?;
                        }
                        Err(error) => {
                            let result = failed_agent_result(
                                &child.agent_id,
                                &step_id,
                                &format!("Parallel child runtime failed: {error:#}"),
                            );
                            self.record_parallel_child_result(run, &group.group_id, child, result)?;
                        }
                    }
                }
            }
        }

        let unrecorded = children
            .iter()
            .filter_map(|(step_id, child)| {
                child
                    .terminal_result
                    .as_ref()
                    .filter(|_| !child.result_recorded)
                    .map(|_| step_id.clone())
            })
            .collect::<Vec<_>>();
        for step_id in unrecorded {
            let child = children
                .get_mut(&step_id)
                .context("missing unrecorded parallel child")?;
            let result = child
                .terminal_result
                .clone()
                .context("unrecorded parallel child missing terminal result")?;
            self.record_parallel_child_result(run, &group.group_id, child, result)?;
        }

        let child_results = children
            .values()
            .filter_map(|child| child.terminal_result.clone().map(|result| (child, result)))
            .collect::<Vec<_>>();
        for (_child, result) in &child_results {
            run.previous_results.push(RunStepResult::Agent {
                result: result.clone(),
            });
        }
        let group_result = synthesize_parallel_group_result(
            &run.run_id,
            &group.group_id,
            &started_at,
            &child_results,
        );
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group.group_id.clone()),
            None,
            "parallel_group_joined",
            serde_json::to_value(&group_result)?,
            "Parallel group joined.",
        )?;
        run.previous_results.push(RunStepResult::ParallelGroup {
            result: group_result,
        });
        for child in children.values() {
            self.clear_active_step(&child.step_id);
        }
        self.state.pending_approval = None;
        // Keep the explainer flag in sync with approval presence (none pending → false).
        self.state.show_first_approval_explainer = false;
        self.sync_chat_items();
        self.publish_state();
        if interrupted {
            self.record_workflow_completed(run, true)?;
            return Ok(AgentStepOutcome::Stop);
        }
        Ok(AgentStepOutcome::Completed)
    }

    /// Propose an execution graph and gate it before scheduling (ADR-005). In
    /// `normal` mode the whole plan pauses on a binary accept/reject resolved
    /// through the clarification answer channel (a distinct `pending_plan_approval`
    /// slot, never the per-action approval slot); `yolo` auto-accepts and runs
    /// immediately. A node is admitted only after acceptance, so fail-closed
    /// behavior (task_04) holds in both modes.
    async fn handle_execution_graph_decision(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
        graph: crate::orchestrator::ExecutionGraph,
    ) -> Result<bool> {
        // Honor the review/fix cycle cap for fixer-bearing graphs, mirroring the
        // single-agent and parallel-group paths so a DAG-routed fixer can't
        // bypass the loop-protection invariant.
        if self.review_fix_cycle_limit_reached(run, "fixer")
            && graph.nodes.iter().any(|node| node.agent == "fixer")
        {
            self.stop_for_review_fix_cycle_limit(run)?;
            return Ok(false);
        }
        self.emit_execution_graph_proposed(run, &graph, &decision.decision_id)?;
        if self.config.approval_mode == ApprovalMode::Normal {
            // Pause: capture the run + graph + decision so accept can run the
            // graph and reject can re-drive the orchestrator (same transport as
            // the governance gate). No node is admitted until the user accepts.
            let question_id = plan_question_id(&graph.graph_id);
            self.state.run_state = RunState::WaitingForUser;
            self.pending_plan_approval = Some(PendingPlanApproval {
                run: run.clone(),
                decision: decision.clone(),
                graph: graph.clone(),
            });
            self.state.pending_plan_approval = Some(PendingPlanApprovalView {
                run_id: run.run_id.clone(),
                graph_id: graph.graph_id.clone(),
                question_id,
                summary: format!(
                    "Review the proposed plan: {} node(s), {} edge(s).",
                    graph.nodes.len(),
                    graph.edges.len()
                ),
            });
            self.set_agent_status("orchestrator", "waiting_for_user");
            self.publish_state();
            Ok(false)
        } else {
            self.emit_execution_graph_approved(run, &graph, &decision.decision_id, true)?;
            match self.run_execution_graph(run, decision, graph).await? {
                AgentStepOutcome::Completed => Ok(true),
                AgentStepOutcome::Paused | AgentStepOutcome::Stop => Ok(false),
            }
        }
    }

    /// Resolve a pending whole-plan approval (ADR-005). `Accept` records the
    /// approval, runs the graph, and continues the orchestrator loop;
    /// `Reject { reason }` records the rejection with its optional free-text
    /// reason and returns the plan to the orchestrator to re-propose (not a hard
    /// failure). Mirrors `resolve_pending_governance_decision`'s transport.
    pub async fn resolve_pending_plan_approval(
        &mut self,
        question_id: &str,
        answer: PlanApprovalAnswer,
    ) -> Result<()> {
        let expected = match self.state.pending_plan_approval.as_ref() {
            Some(view) => view.question_id.clone(),
            None => bail!("no plan approval is pending"),
        };
        if expected != question_id {
            return Err(anyhow!(
                "answer question id does not match pending plan approval (expected: {expected}, got: {question_id})"
            ));
        }
        let Some(pending) = self.pending_plan_approval.take() else {
            bail!("no plan approval is pending");
        };
        self.state.pending_plan_approval = None;
        let mut run = pending.run;
        let decision = pending.decision;
        let graph = pending.graph;

        match answer {
            PlanApprovalAnswer::Accept => {
                self.emit_execution_graph_approved(&run, &graph, &decision.decision_id, false)?;
                self.state.run_state = RunState::Running;
                self.publish_state();
                match self.run_execution_graph(&mut run, &decision, graph).await? {
                    AgentStepOutcome::Completed => self.drive_and_replay(run, None).await?,
                    AgentStepOutcome::Paused | AgentStepOutcome::Stop => {
                        // The graph ended the run (limit/interrupt) without
                        // continuing the orchestrator loop; finalize like
                        // drive_run so the record and active_run_id aren't stale.
                        self.finalize_run_record(&mut run)?;
                        self.react_to_run_end_for_queue().await?;
                    }
                }
            }
            PlanApprovalAnswer::Reject { reason } => {
                let reason_text = reason.as_deref().map(str::trim).filter(|r| !r.is_empty());
                self.emit_execution_graph_rejected(
                    &run,
                    &graph,
                    &decision.decision_id,
                    reason_text,
                )?;
                if let Some(reason_text) = reason_text {
                    run.prompt = format!(
                        "{}\n\nThe user rejected the proposed plan: {reason_text}",
                        run.prompt
                    );
                }
                self.state.run_state = RunState::Planning;
                self.publish_state();
                self.drive_and_replay(run, None).await?;
            }
        }
        Ok(())
    }

    fn emit_execution_graph_proposed(
        &mut self,
        run: &RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
        decision_id: &str,
    ) -> Result<()> {
        let snapshot = execution_graph_snapshot(graph, &BTreeMap::new(), &BTreeSet::new());
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            None,
            crate::history::EXECUTION_GRAPH_PROPOSED_KIND,
            json!({
                "graph_id": graph.graph_id,
                "decision_id": decision_id,
                "reason": graph.reason,
                "graph": snapshot,
            }),
            "Execution graph proposed.",
        )
    }

    fn emit_execution_graph_approved(
        &mut self,
        run: &RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
        decision_id: &str,
        auto_accepted: bool,
    ) -> Result<()> {
        let snapshot = execution_graph_snapshot(graph, &BTreeMap::new(), &BTreeSet::new());
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            None,
            crate::history::EXECUTION_GRAPH_APPROVED_KIND,
            json!({
                "graph_id": graph.graph_id,
                "decision_id": decision_id,
                "auto_accepted": auto_accepted,
                "graph": snapshot,
            }),
            "Execution graph approved.",
        )
    }

    fn emit_execution_graph_rejected(
        &mut self,
        run: &RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
        decision_id: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let snapshot = execution_graph_snapshot(graph, &BTreeMap::new(), &BTreeSet::new());
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            None,
            crate::history::EXECUTION_GRAPH_REJECTED_KIND,
            json!({
                "graph_id": graph.graph_id,
                "decision_id": decision_id,
                "reason": reason,
                "graph": snapshot,
            }),
            "Execution graph rejected.",
        )
    }

    /// Lower a validated [`ExecutionGraph`](crate::orchestrator::ExecutionGraph)
    /// onto the reused parallel executor with **lazy ready-set admission**
    /// (ADR-004). Instead of spawning every node up front, the join loop re-runs
    /// [`admit_ready_nodes`](Self::admit_ready_nodes) at the top of every
    /// iteration: it spawns newly-ready nodes, fail-closed-skips nodes downstream
    /// of a failure, and — because skip-propagation cascades to a fixpoint and a
    /// blocked node always has a running node ahead of it — always drains to
    /// all-terminal without deadlocking.
    async fn run_execution_graph(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
        graph: crate::orchestrator::ExecutionGraph,
    ) -> Result<AgentStepOutcome> {
        self.state.run_state = RunState::Running;
        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            return Ok(AgentStepOutcome::Stop);
        }
        if self.execution_graph_exceeds_agent_step_limit(run, &graph) {
            self.stop_for_execution_graph_agent_step_limit(run, &graph)?;
            return Ok(AgentStepOutcome::Stop);
        }

        let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let cancellation = CancellationToken::new();
        let interrupt_start = *self.interrupt_receiver.borrow();
        let approval_start = self.approval_receiver.borrow().sequence;

        // Static graph metadata: predecessors and command-ness per node.
        let mut predecessors: BTreeMap<String, Vec<String>> = graph
            .nodes
            .iter()
            .map(|node| (node.node_id.clone(), Vec::new()))
            .collect();
        for edge in &graph.edges {
            if let Some(preds) = predecessors.get_mut(&edge.to) {
                preds.push(edge.from.clone());
            }
        }
        let command_nodes: BTreeSet<String> = graph
            .nodes
            .iter()
            .filter(|node| node_runs_commands(node))
            .map(|node| node.node_id.clone())
            .collect();

        let child_specs = self.prepare_execution_graph_nodes(run, &graph)?;
        if let Some(workflow) = run.workflow.as_mut() {
            for child in &child_specs {
                workflow.record_planned_targets(
                    &graph.graph_id,
                    Some(&child.step_id),
                    &child.step_label,
                    &child.file_scope,
                    &self.config.working_directory,
                    &self.config.workspace.extra_write_roots,
                )?;
            }
        }

        let (sender, mut receiver) =
            mpsc::channel(RUNTIME_EVENT_CHANNEL_CAPACITY * child_specs.len().max(1));
        let resume_handle = ParallelRuntimeResumeHandle {
            cancellation: cancellation.clone(),
            sender: sender.clone(),
        };
        let mut children = BTreeMap::new();
        let mut coalescers = BTreeMap::new();
        for mut child in child_specs {
            child.cancellation = cancellation.child_token();
            coalescers.insert(
                child.step_id.clone(),
                RuntimeStreamCoalescer::new(child.agent_id.clone()),
            );
            children.insert(child.step_id.clone(), child);
        }
        let mut skipped_nodes: BTreeSet<String> = BTreeSet::new();

        // Each node_pending event carries the full graph snapshot, so the Plan
        // projection (task_06) can render the whole graph from the first event
        // even before task_05's proposed/approved events exist.
        for node in &graph.nodes {
            let node_id = node.node_id.clone();
            self.emit_node_lifecycle_event(
                run,
                &graph,
                &children,
                &skipped_nodes,
                &node_id,
                crate::history::NODE_PENDING_KIND,
            )?;
        }

        let interrupt_requested =
            wait_for_interrupt(self.interrupt_receiver.clone(), interrupt_start);
        tokio::pin!(interrupt_requested);
        let mut approval_answered = Box::pin(wait_for_approval(
            self.approval_receiver.clone(),
            approval_start,
        ));
        let mut approval_queue: VecDeque<PendingParallelApproval> = VecDeque::new();
        let mut limit_tick = tokio::time::interval(Duration::from_millis(100));

        let mut interrupted = false;
        loop {
            // Re-run admission at the top of every iteration: this is where the
            // no-deadlock invariant is enforced (spawn ready nodes, fail-closed
            // skip doomed ones, all cascaded to a fixpoint).
            self.admit_ready_nodes(
                &graph,
                &mut children,
                &predecessors,
                &command_nodes,
                run,
                &sender,
                &mut coalescers,
                &mut skipped_nodes,
            )?;
            self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
            self.publish_parallel_approval_head(&approval_queue)?;
            if children
                .values()
                .all(|child| child.terminal_result.is_some())
            {
                break;
            }

            let message = tokio::select! {
                message = receiver.recv() => message,
                _ = &mut interrupt_requested => {
                    cancellation.cancel();
                    interrupted = true;
                    self.state.run_state = RunState::Interrupted;
                    self.state.pending_approval = None;
                    self.state.show_first_approval_explainer = false;
                    approval_queue.clear();
                    self.record_step_cancelled_if_active()?;
                    self.record_event(
                        Some(run.run_id.clone()),
                        None,
                        "run_interrupted",
                        json!({}),
                        "Run interrupted.",
                    )?;
                    let to_cancel: Vec<(String, String)> = children
                        .values()
                        .filter(|child| child.terminal_result.is_none())
                        .map(|child| (child.step_id.clone(), child.agent_id.clone()))
                        .collect();
                    for (node_id, agent_id) in to_cancel {
                        let result = cancelled_agent_result(
                            &agent_id,
                            &node_id,
                            "Execution graph cancelled by run interrupt.",
                        );
                        self.finalize_node(
                            run,
                            &graph,
                            &mut children,
                            &skipped_nodes,
                            &node_id,
                            result,
                            crate::history::NODE_CANCELLED_KIND,
                        )?;
                    }
                    None
                }
                approval = &mut approval_answered => {
                    approval_answered = Box::pin(wait_for_approval(
                        self.approval_receiver.clone(),
                        approval.sequence,
                    ));
                    if let Some(pending) = approval_queue.pop_front() {
                        self.publish_parallel_approval_head(&approval_queue)?;
                        if let Some(child) = children.get_mut(&pending.step_id) {
                            if child.terminal_result.is_none() {
                                self.resolve_parallel_approval(
                                    run,
                                    child,
                                    pending,
                                    approval.resolution,
                                    resume_handle.clone(),
                                )
                                .await?;
                            }
                        }
                        self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
                        self.publish_parallel_approval_head(&approval_queue)?;
                    }
                    continue;
                }
                _ = limit_tick.tick() => {
                    if self.apply_parallel_limit_checks(run, &mut children, &cancellation)? {
                        self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
                        self.publish_parallel_approval_head(&approval_queue)?;
                    }
                    continue;
                }
            };
            let Some(message) = message else {
                break;
            };
            match message {
                ParallelRuntimeMessage::RuntimeEvent { step_id, event } => {
                    if let Some(child) = children.get(&step_id) {
                        if child.terminal_result.is_some() {
                            continue;
                        }
                        let coalescer = coalescers
                            .get_mut(&step_id)
                            .context("missing execution graph stream coalescer")?;
                        self.record_parallel_runtime_event(
                            run,
                            &graph.graph_id,
                            &step_id,
                            coalescer,
                            event,
                        )?;
                        self.set_agent_status(&child.agent_id, "running_parallel");
                    }
                }
                ParallelRuntimeMessage::Output { step_id, output } => {
                    if children
                        .get(&step_id)
                        .map(|child| child.terminal_result.is_some())
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    if let Some(coalescer) = coalescers.get_mut(&step_id) {
                        self.flush_runtime_stream_coalescer_with_group(
                            run,
                            Some(&graph.graph_id),
                            &step_id,
                            coalescer,
                            true,
                        )?;
                    }
                    match *output {
                        Ok(RuntimeOutput::AgentResult { result }) => {
                            let kind = execution_graph_node_kind(&result.status);
                            self.finalize_node(
                                run,
                                &graph,
                                &mut children,
                                &skipped_nodes,
                                &step_id,
                                result,
                                kind,
                            )?;
                        }
                        Ok(RuntimeOutput::ActionRequest { request }) => {
                            let Some(child) = children.get_mut(&step_id) else {
                                continue;
                            };
                            if self
                                .handle_parallel_child_action(
                                    run,
                                    &graph.graph_id,
                                    child,
                                    request,
                                    resume_handle.clone(),
                                    &mut approval_queue,
                                )
                                .await?
                            {
                                continue;
                            }
                            self.publish_parallel_approval_head(&approval_queue)?;
                        }
                        Ok(RuntimeOutput::ParseError {
                            agent,
                            raw_output,
                            diagnostic,
                        }) => {
                            let result = self.persist_parse_error_with_group(
                                &run.run_id,
                                &graph.graph_id,
                                &step_id,
                                &agent,
                                raw_output,
                                diagnostic,
                            )?;
                            self.finalize_node(
                                run,
                                &graph,
                                &mut children,
                                &skipped_nodes,
                                &step_id,
                                result,
                                crate::history::NODE_FAILED_KIND,
                            )?;
                        }
                        Ok(RuntimeOutput::OrchestratorDecision { .. }) => {
                            let agent_id = children
                                .get(&step_id)
                                .map(|child| child.agent_id.clone())
                                .unwrap_or_default();
                            let result = failed_agent_result(
                                &agent_id,
                                &step_id,
                                "Execution graph node returned an orchestrator decision.",
                            );
                            self.finalize_node(
                                run,
                                &graph,
                                &mut children,
                                &skipped_nodes,
                                &step_id,
                                result,
                                crate::history::NODE_FAILED_KIND,
                            )?;
                        }
                        Err(error) => {
                            let agent_id = children
                                .get(&step_id)
                                .map(|child| child.agent_id.clone())
                                .unwrap_or_default();
                            let result = failed_agent_result(
                                &agent_id,
                                &step_id,
                                &format!("Execution graph node runtime failed: {error:#}"),
                            );
                            self.finalize_node(
                                run,
                                &graph,
                                &mut children,
                                &skipped_nodes,
                                &step_id,
                                result,
                                crate::history::NODE_FAILED_KIND,
                            )?;
                        }
                    }
                }
            }
        }

        // Backfill any node terminalized by a reused helper (the limit-check /
        // approval sweep sets terminal_result directly without going through
        // finalize_node), so its agent_result event, node lifecycle event, and
        // workflow-ledger entry are still recorded. Mirrors the flat parallel
        // path's unrecorded sweep.
        let unrecorded: Vec<(String, AgentResult)> = children
            .iter()
            .filter(|(_, child)| child.terminal_result.is_some() && !child.result_recorded)
            .map(|(node_id, child)| {
                (
                    node_id.clone(),
                    child.terminal_result.clone().expect("filtered to Some"),
                )
            })
            .collect();
        for (node_id, result) in unrecorded {
            let kind = execution_graph_node_kind(&result.status);
            self.finalize_node(
                run,
                &graph,
                &mut children,
                &skipped_nodes,
                &node_id,
                result,
                kind,
            )?;
        }

        let node_results = children
            .values()
            .filter_map(|child| child.terminal_result.clone().map(|result| (child, result)))
            .collect::<Vec<_>>();
        for (_child, result) in &node_results {
            run.previous_results.push(RunStepResult::Agent {
                result: result.clone(),
            });
        }
        let graph_result = synthesize_execution_graph_result(
            &run.run_id,
            &graph.graph_id,
            &started_at,
            &node_results,
            &skipped_nodes,
        );
        let mut completed_payload = serde_json::to_value(&graph_result)?;
        if let serde_json::Value::Object(map) = &mut completed_payload {
            map.insert(
                "decision_id".to_string(),
                serde_json::Value::String(decision.decision_id.clone()),
            );
            map.insert(
                "graph".to_string(),
                execution_graph_snapshot(&graph, &children, &skipped_nodes),
            );
        }
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            None,
            crate::history::EXECUTION_GRAPH_COMPLETED_KIND,
            completed_payload,
            "Execution graph completed.",
        )?;
        run.previous_results.push(RunStepResult::Dag {
            result: graph_result,
        });
        for child in children.values() {
            self.clear_active_step(&child.step_id);
        }
        self.state.pending_approval = None;
        self.state.show_first_approval_explainer = false;
        self.sync_chat_items();
        self.publish_state();
        if interrupted {
            self.record_workflow_completed(run, true)?;
            return Ok(AgentStepOutcome::Stop);
        }
        Ok(AgentStepOutcome::Completed)
    }

    /// Apply one ready-set admission pass (ADR-004): sync terminal nodes, compute
    /// the pure [`compute_admission`] decision, fail-closed-skip the doomed
    /// nodes, and spawn the admitted ones on the reused parallel runtime.
    #[allow(clippy::too_many_arguments)]
    fn admit_ready_nodes(
        &mut self,
        graph: &crate::orchestrator::ExecutionGraph,
        children: &mut BTreeMap<String, ParallelChildRuntimeState>,
        predecessors: &BTreeMap<String, Vec<String>>,
        command_nodes: &BTreeSet<String>,
        run: &mut RunDriveContext,
        sender: &mpsc::Sender<ParallelRuntimeMessage>,
        coalescers: &mut BTreeMap<String, RuntimeStreamCoalescer>,
        skipped_nodes: &mut BTreeSet<String>,
    ) -> Result<()> {
        // Any node carrying a terminal_result is terminal regardless of how it
        // got there (a reused limit/approval/interrupt path may set the result
        // without the discriminator); reconcile before snapshotting.
        for child in children.values_mut() {
            if child.terminal_result.is_some() {
                child.node_run_state = NodeRunState::Terminal;
            }
        }

        let snapshots: Vec<DagNodeSnapshot> = graph
            .nodes
            .iter()
            .map(|node| {
                let child = children.get(&node.node_id);
                let succeeded = child
                    .and_then(|child| child.terminal_result.as_ref())
                    .map(|result| {
                        matches!(
                            result.status,
                            AgentResultStatus::Completed | AgentResultStatus::NoChanges
                        )
                    })
                    .unwrap_or(false);
                DagNodeSnapshot {
                    node_id: node.node_id.clone(),
                    run_state: child
                        .map(|child| child.node_run_state)
                        .unwrap_or(NodeRunState::Pending),
                    succeeded,
                    is_command: command_nodes.contains(&node.node_id),
                    write_files: node.file_scope.write_files.iter().cloned().collect(),
                    predecessors: predecessors.get(&node.node_id).cloned().unwrap_or_default(),
                }
            })
            .collect();
        // The DAG reuses `max_parallel_agent_steps` as its concurrency ceiling;
        // admission never spawns more than that many nodes at once.
        let max_concurrent = self.config.limits.max_parallel_agent_steps as usize;
        let decision = compute_admission(&snapshots, max_concurrent);

        for node_id in decision.skip {
            let agent_id = children
                .get(&node_id)
                .map(|child| child.agent_id.clone())
                .unwrap_or_default();
            if let Some(child) = children.get_mut(&node_id) {
                child.cancellation.cancel();
            }
            skipped_nodes.insert(node_id.clone());
            let result = skipped_agent_result(
                &agent_id,
                &node_id,
                "Skipped: an upstream node did not succeed.",
            );
            self.finalize_node(
                run,
                graph,
                children,
                skipped_nodes,
                &node_id,
                result,
                crate::history::NODE_SKIPPED_KIND,
            )?;
        }

        for node_id in decision.admit {
            let spawn = {
                let child = children
                    .get_mut(&node_id)
                    .context("admitted execution graph node missing")?;
                child.node_run_state = NodeRunState::Running;
                child.step_started_at = Instant::now();
                let sequence = child.next_runtime_sequence;
                child.next_runtime_sequence = child.next_runtime_sequence.saturating_add(1);
                (
                    child.step_id.clone(),
                    child.agent_id.clone(),
                    child.step_label.clone(),
                    child.file_scope.clone(),
                    child.request.clone(),
                    child.cancellation.clone(),
                    sequence,
                )
            };
            let (step_id, agent_id, step_label, file_scope, request, child_cancellation, sequence) =
                spawn;
            self.set_active_step_with_metadata(
                &run.run_id,
                Some(graph.graph_id.clone()),
                &step_id,
                Some(step_label.clone()),
                Some(file_scope.clone()),
                &agent_id,
            );
            self.set_agent_status(&agent_id, "running_parallel");
            coalescers
                .entry(step_id.clone())
                .or_insert_with(|| RuntimeStreamCoalescer::new(agent_id.clone()));
            spawn_parallel_runtime_task(
                self.config.clone(),
                step_id.clone(),
                request,
                sequence,
                child_cancellation,
                sender.clone(),
            );
            self.emit_node_lifecycle_event(
                run,
                graph,
                children,
                skipped_nodes,
                &node_id,
                crate::history::NODE_RUNNING_KIND,
            )?;
        }
        Ok(())
    }

    /// Terminalize one node: persist its `agent_result`, record the terminal
    /// result + run-state, fold it into the workflow ledger, and emit the
    /// matching node lifecycle event (succeeded/failed/skipped/cancelled) with a
    /// full graph snapshot.
    #[allow(clippy::too_many_arguments)]
    fn finalize_node(
        &mut self,
        run: &mut RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
        children: &mut BTreeMap<String, ParallelChildRuntimeState>,
        skipped_nodes: &BTreeSet<String>,
        node_id: &str,
        result: AgentResult,
        lifecycle_kind: &str,
    ) -> Result<()> {
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            Some(node_id.to_string()),
            "agent_result",
            serde_json::to_value(&result)?,
            format!("{}: {}", result.agent, result.summary),
        )?;
        let file_scope = {
            let child = children
                .get_mut(node_id)
                .context("finalized execution graph node missing")?;
            child.terminal_result = Some(result.clone());
            child.result_recorded = true;
            child.node_run_state = NodeRunState::Terminal;
            child.file_scope.clone()
        };
        self.set_live_step_status(
            node_id,
            if matches!(
                result.status,
                AgentResultStatus::Completed | AgentResultStatus::NoChanges
            ) {
                LiveStepStatus::Completed
            } else {
                LiveStepStatus::Failed
            },
        );
        if let Some(workflow) = run.workflow.as_mut() {
            workflow.record_child_result(
                &graph.graph_id,
                node_id,
                &file_scope,
                &result,
                &self.config.working_directory,
                &self.config.workspace.extra_write_roots,
            )?;
        }
        self.emit_node_lifecycle_event(
            run,
            graph,
            children,
            skipped_nodes,
            node_id,
            lifecycle_kind,
        )?;
        self.clear_active_step(node_id);
        Ok(())
    }

    /// Emit one node lifecycle event keyed by `graph_id` + `node_id`, carrying a
    /// full graph snapshot so the Plan projection can re-render from it.
    fn emit_node_lifecycle_event(
        &mut self,
        run: &RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
        children: &BTreeMap<String, ParallelChildRuntimeState>,
        skipped_nodes: &BTreeSet<String>,
        node_id: &str,
        kind: &str,
    ) -> Result<()> {
        let status = children
            .get(node_id)
            .map(|child| node_status_label(child, skipped_nodes))
            .unwrap_or("pending");
        let payload = json!({
            "graph_id": graph.graph_id,
            "node_id": node_id,
            "status": status,
            "graph": execution_graph_snapshot(graph, children, skipped_nodes),
        });
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            Some(node_id.to_string()),
            kind,
            payload,
            format!("Execution graph node {node_id}: {status}."),
        )
    }

    fn prepare_execution_graph_nodes(
        &mut self,
        run: &mut RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
    ) -> Result<Vec<ParallelChildRuntimeState>> {
        let sibling_contexts = graph
            .nodes
            .iter()
            .map(|node| ParallelSiblingContext {
                step_id: node.node_id.clone(),
                step_label: node.step_label.clone(),
                agent: node.agent.clone(),
                file_scope: node.file_scope.clone(),
            })
            .collect::<Vec<_>>();
        let mut children = Vec::with_capacity(graph.nodes.len());
        // The whole-graph agent-step fit is pre-checked in
        // execution_graph_exceeds_agent_step_limit, so every node is prepared.
        for node in &graph.nodes {
            let agent = self.agent(&node.agent)?.clone();
            run.step_count += 1;
            let step = crate::orchestrator::ParallelChildStepPlan {
                step_label: node.step_label.clone(),
                agent: node.agent.clone(),
                instruction: node.instruction.clone(),
                required_capabilities: node.required_capabilities.clone(),
                file_scope: node.file_scope.clone(),
            };
            let prompt = parallel_child_prompt(&run.prompt, &step);
            let mut request = self.runtime_request(
                &run.run_id,
                &node.node_id,
                RuntimePrompt::new(&prompt, run.skill_context.as_ref()),
                agent,
                run.previous_results.clone(),
                "agent_result",
            )?;
            request.parallel_context = Some(ParallelRuntimeContext {
                group_id: graph.graph_id.clone(),
                step_label: node.step_label.clone(),
                file_scope: node.file_scope.clone(),
                parallel_siblings: sibling_contexts
                    .iter()
                    .filter(|sibling| sibling.step_id != node.node_id)
                    .cloned()
                    .collect(),
                scope_policy_summary: parallel_scope_policy_summary(&node.file_scope),
            });
            children.push(ParallelChildRuntimeState {
                step_id: node.node_id.clone(),
                step_label: node.step_label.clone(),
                agent_id: node.agent.clone(),
                file_scope: node.file_scope.clone(),
                request,
                step_started_at: Instant::now(),
                next_runtime_sequence: 1,
                action_count: 0,
                cancellation: CancellationToken::new(),
                terminal_result: None,
                result_recorded: false,
                node_run_state: NodeRunState::Pending,
            });
        }
        Ok(children)
    }

    fn execution_graph_exceeds_agent_step_limit(
        &self,
        run: &RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
    ) -> bool {
        match self.config.limits.max_agent_steps {
            Limit::Value(limit) => run.step_count.saturating_add(graph.nodes.len() as u32) > limit,
            Limit::Unlimited => false,
        }
    }

    fn stop_for_execution_graph_agent_step_limit(
        &mut self,
        run: &RunDriveContext,
        graph: &crate::orchestrator::ExecutionGraph,
    ) -> Result<()> {
        self.state.run_state = RunState::LimitReached;
        self.record_event_with_graph(
            Some(run.run_id.clone()),
            Some(graph.graph_id.clone()),
            None,
            "run_limit_reached",
            json!({
                "limit": "max_agent_steps",
                "value": run.step_count,
                "requested_graph_nodes": graph.nodes.len(),
                "graph_id": graph.graph_id
            }),
            "Run limit reached before the execution graph could start.",
        )
    }

    fn parallel_group_exceeds_agent_step_limit(
        &self,
        run: &RunDriveContext,
        group: &ParallelGroupPlan,
    ) -> bool {
        match self.config.limits.max_agent_steps {
            Limit::Value(limit) => run.step_count.saturating_add(group.steps.len() as u32) > limit,
            Limit::Unlimited => false,
        }
    }

    fn stop_for_parallel_group_agent_step_limit(
        &mut self,
        run: &RunDriveContext,
        group: &ParallelGroupPlan,
    ) -> Result<()> {
        self.state.run_state = RunState::LimitReached;
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group.group_id.clone()),
            None,
            "run_limit_reached",
            json!({
                "limit": "max_agent_steps",
                "value": run.step_count,
                "requested_parallel_children": group.steps.len(),
                "group_id": group.group_id
            }),
            "Run limit reached before the parallel group could start.",
        )
    }

    fn prepare_parallel_children(
        &mut self,
        run: &mut RunDriveContext,
        group: &ParallelGroupPlan,
    ) -> Result<Vec<ParallelChildRuntimeState>> {
        let child_ids = group
            .steps
            .iter()
            .map(|step| (new_id(), step))
            .collect::<Vec<_>>();
        let sibling_contexts = child_ids
            .iter()
            .map(|(step_id, step)| ParallelSiblingContext {
                step_id: step_id.clone(),
                step_label: step.step_label.clone(),
                agent: step.agent.clone(),
                file_scope: step.file_scope.clone(),
            })
            .collect::<Vec<_>>();
        let mut children = Vec::with_capacity(child_ids.len());
        for (step_id, step) in child_ids {
            if limit_reached(&self.config.limits.max_agent_steps, run.step_count) {
                self.state.run_state = RunState::LimitReached;
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "run_limit_reached",
                    json!({ "limit": "max_agent_steps", "value": run.step_count }),
                    "Run limit reached before a parallel child step.",
                )?;
                break;
            }
            let agent = self.agent(&step.agent)?.clone();
            run.step_count += 1;
            let prompt = parallel_child_prompt(&run.prompt, step);
            let mut request = self.runtime_request(
                &run.run_id,
                &step_id,
                RuntimePrompt::new(&prompt, run.skill_context.as_ref()),
                agent,
                run.previous_results.clone(),
                "agent_result",
            )?;
            request.parallel_context = Some(ParallelRuntimeContext {
                group_id: group.group_id.clone(),
                step_label: step.step_label.clone(),
                file_scope: step.file_scope.clone(),
                parallel_siblings: sibling_contexts
                    .iter()
                    .filter(|sibling| sibling.step_id != step_id)
                    .cloned()
                    .collect(),
                scope_policy_summary: parallel_scope_policy_summary(&step.file_scope),
            });
            children.push(ParallelChildRuntimeState {
                step_id,
                step_label: step.step_label.clone(),
                agent_id: step.agent.clone(),
                file_scope: step.file_scope.clone(),
                request,
                step_started_at: Instant::now(),
                next_runtime_sequence: 1,
                action_count: 0,
                cancellation: CancellationToken::new(),
                terminal_result: None,
                result_recorded: false,
                // The flat parallel path spawns every child immediately.
                node_run_state: NodeRunState::Running,
            });
        }
        Ok(children)
    }

    async fn handle_parallel_child_action(
        &mut self,
        run: &RunDriveContext,
        group_id: &str,
        child: &mut ParallelChildRuntimeState,
        action_request: ActionRequest,
        resume_handle: ParallelRuntimeResumeHandle,
        approval_queue: &mut VecDeque<PendingParallelApproval>,
    ) -> Result<bool> {
        if limit_reached(&self.config.limits.max_step_actions, child.action_count) {
            self.stop_for_step_action_limit(run, &child.step_id, child.action_count as usize)?;
            child.terminal_result = Some(limit_reached_agent_result(
                &child.agent_id,
                &child.step_id,
                "Parallel child reached max_step_actions.",
            ));
            return Ok(false);
        }
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group_id.to_string()),
            Some(child.step_id.clone()),
            "action_requested",
            serde_json::to_value(&action_request)?,
            action_requested_display(&action_request),
        )?;
        self.set_live_step_status(&child.step_id, LiveStepStatus::WaitingForAction);
        self.set_agent_status(&child.agent_id, "waiting_action");
        let context = ActionExecutionContext {
            working_directory: self.config.working_directory.clone(),
            workspace: self.config.workspace.clone(),
            approval_mode: self.effective_approval_mode(),
            command_timeout: command_timeout(&self.config.limits.max_command_minutes),
            user_prompt: Some(run.prompt.clone()),
            action_scope: crate::actions::ActionScope::ParallelFileScope(child.file_scope.clone()),
            floor: self.config.approval.floor,
            trusted_targets: self.trust_store.snapshot(),
            pre_approved: false,
            drift_ack: self.drift_ack_context(),
            mcp: self.mcp_action_context(),
        };
        self.record_command_started_if_executable_with_group(
            &run.run_id,
            Some(group_id),
            &child.step_id,
            &child.request.agent_profile,
            &context,
            &action_request,
        )?;
        let action =
            execute_action_request(&child.request.agent_profile, &context, &action_request);
        let Some(result) = await_with_step_limit(
            action,
            &self.config.limits.max_step_minutes,
            child.step_started_at,
        )
        .await
        else {
            self.stop_for_step_time_limit(run, &child.step_id, child.step_started_at)?;
            child.cancellation.cancel();
            child.terminal_result = Some(limit_reached_agent_result(
                &child.agent_id,
                &child.step_id,
                "Parallel child reached max_step_minutes.",
            ));
            return Ok(false);
        };
        if matches!(result.status, ActionStatus::ApprovalRequired) {
            self.set_live_step_status(&child.step_id, LiveStepStatus::WaitingForApproval);
            self.set_agent_status(&child.agent_id, "waiting_approval");
            self.record_event_with_group(
                Some(run.run_id.clone()),
                Some(group_id.to_string()),
                Some(child.step_id.clone()),
                "approval_requested",
                serde_json::to_value(&result)?,
                "Action approval required.",
            )?;
            approval_queue.push_back(PendingParallelApproval {
                run_id: run.run_id.clone(),
                group_id: group_id.to_string(),
                step_id: child.step_id.clone(),
                action_request,
                agent_profile: child.request.agent_profile.clone(),
                context,
                reason: result.diagnostic.clone(),
            });
            return Ok(false);
        }
        if matches!(result.status, ActionStatus::Denied) {
            self.record_event_with_group(
                Some(run.run_id.clone()),
                Some(group_id.to_string()),
                Some(child.step_id.clone()),
                "action_denied",
                serde_json::to_value(&result)?,
                action_denied_display(&action_request, &result),
            )?;
        }
        self.record_action_specific_events_with_group(
            &run.run_id,
            Some(group_id),
            &child.step_id,
            &action_request,
            &result,
        )?;
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group_id.to_string()),
            Some(child.step_id.clone()),
            "action_completed",
            serde_json::to_value(&result)?,
            action_completed_display(&action_request, &result),
        )?;
        child.action_count += 1;
        child.request.action_results.push(result);
        self.set_live_step_status(&child.step_id, LiveStepStatus::Running);
        self.set_agent_status(&child.agent_id, "running_parallel");
        spawn_parallel_runtime_task(
            self.config.clone(),
            child.step_id.clone(),
            child.request.clone(),
            child.next_runtime_sequence,
            child.cancellation.clone(),
            resume_handle.sender,
        );
        child.next_runtime_sequence = child.next_runtime_sequence.saturating_add(1);
        Ok(true)
    }

    async fn resolve_parallel_approval(
        &mut self,
        run: &RunDriveContext,
        child: &mut ParallelChildRuntimeState,
        mut pending: PendingParallelApproval,
        resolution: ApprovalResolution,
        resume_handle: ParallelRuntimeResumeHandle,
    ) -> Result<()> {
        let approved = resolution.approves();
        self.record_event_with_group(
            Some(pending.run_id.clone()),
            Some(pending.group_id.clone()),
            Some(pending.step_id.clone()),
            "approval_resolved",
            json!({
                "action_id": pending.action_request.action_id.clone(),
                "approved": approved,
                "resolution": approval_resolution_label(resolution),
            }),
            if approved {
                "Action approval granted."
            } else {
                "Action approval denied."
            },
        )?;

        // Approve-and-trust grants the non-catastrophic target (ADR-004).
        if resolution.grants_trust() {
            if let Some(target) =
                crate::actions::assess_risk(&pending.action_request, &pending.context).target
            {
                if self.trust_store.grant(target.clone()) {
                    self.record_event_with_group(
                        Some(pending.run_id.clone()),
                        Some(pending.group_id.clone()),
                        Some(pending.step_id.clone()),
                        "trust_granted",
                        json!({ "target": trust_target_payload(&target) }),
                        format!("Trusted for this session: {}", trust_target_label(&target)),
                    )?;
                }
            }
        }

        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            resume_handle.cancellation.cancel();
            child.terminal_result = Some(limit_reached_agent_result(
                &child.agent_id,
                &child.step_id,
                "Parallel group reached max_wall_clock_minutes.",
            ));
            return Ok(());
        }
        if self.step_time_limit_reached(child.step_started_at) {
            self.stop_for_step_time_limit(run, &child.step_id, child.step_started_at)?;
            child.cancellation.cancel();
            child.terminal_result = Some(limit_reached_agent_result(
                &child.agent_id,
                &child.step_id,
                "Parallel child reached max_step_minutes.",
            ));
            return Ok(());
        }

        let result = if approved {
            // Pre-approve the re-run so catastrophic / floor=Enforce actions execute
            // instead of re-prompting (mirrors the serial path).
            pending.context.pre_approved = true;
            // First-mutation drift acknowledgment, mirroring the serial path (ADR-004).
            self.acknowledge_resume_drift(&pending.action_request)?;
            self.record_command_started_if_executable_with_group(
                &pending.run_id,
                Some(&pending.group_id),
                &pending.step_id,
                &pending.agent_profile,
                &pending.context,
                &pending.action_request,
            )?;
            let action = execute_action_request(
                &pending.agent_profile,
                &pending.context,
                &pending.action_request,
            );
            let Some(result) = await_with_step_limit(
                action,
                &self.config.limits.max_step_minutes,
                child.step_started_at,
            )
            .await
            else {
                self.stop_for_step_time_limit(run, &child.step_id, child.step_started_at)?;
                child.cancellation.cancel();
                child.terminal_result = Some(limit_reached_agent_result(
                    &child.agent_id,
                    &child.step_id,
                    "Parallel child reached max_step_minutes.",
                ));
                return Ok(());
            };
            result
        } else {
            ActionResult::approval_denied(
                &pending.action_request,
                pending
                    .reason
                    .unwrap_or_else(|| "user denied action approval".to_string()),
            )
        };

        self.record_action_specific_events_with_group(
            &pending.run_id,
            Some(&pending.group_id),
            &pending.step_id,
            &pending.action_request,
            &result,
        )?;
        self.record_action_completed_with_group(
            &pending.run_id,
            Some(&pending.group_id),
            &pending.step_id,
            &pending.agent_profile.runtime,
            &pending.action_request,
            &result,
        )?;
        child.action_count += 1;
        child.request.action_results.push(result);
        self.set_live_step_status(&child.step_id, LiveStepStatus::Running);
        self.set_agent_status(&child.agent_id, "running_parallel");
        spawn_parallel_runtime_task(
            self.config.clone(),
            child.step_id.clone(),
            child.request.clone(),
            child.next_runtime_sequence,
            child.cancellation.clone(),
            resume_handle.sender,
        );
        child.next_runtime_sequence = child.next_runtime_sequence.saturating_add(1);
        Ok(())
    }

    /// Apply the ADR-004 first-approval explainer latch: show the one-line
    /// explainer the first time any approval becomes pending for this user, then
    /// persist the latch so every later approval suppresses it. A latch write
    /// failure propagates rather than silently re-showing. Shared by the serial and
    /// parallel approval paths so the two cannot diverge.
    fn apply_first_approval_explainer_latch(&mut self) -> Result<()> {
        if !self.history.first_approval_explainer_shown() {
            self.history.mark_first_approval_explainer_shown()?;
            self.state.show_first_approval_explainer = true;
        } else {
            self.state.show_first_approval_explainer = false;
        }
        Ok(())
    }

    fn publish_parallel_approval_head(
        &mut self,
        queue: &VecDeque<PendingParallelApproval>,
    ) -> Result<()> {
        let prev_step = self
            .state
            .pending_approval
            .as_ref()
            .map(|pending| pending.step_id.clone());
        self.state.pending_approval = queue.front().map(|pending| {
            build_pending_approval_view(
                pending.run_id.clone(),
                Some(pending.group_id.clone()),
                pending.step_id.clone(),
                pending.agent_profile.id.clone(),
                action_requested_display(&pending.action_request).to_string(),
                pending.reason.clone(),
                &pending.action_request,
                &pending.context,
            )
        });
        // First-approval explainer (ADR-004), mirroring the serial path: consult the
        // persisted latch only when the *surfaced* approval changes (a new head, not a
        // re-publish of the same one), so a user whose first approval is a parallel
        // child still sees the one-line explainer exactly once. Re-publishing the same
        // head (e.g. when another child enqueues) leaves the flag untouched.
        let new_step = self
            .state
            .pending_approval
            .as_ref()
            .map(|pending| pending.step_id.clone());
        if new_step != prev_step {
            if self.state.pending_approval.is_some() {
                self.apply_first_approval_explainer_latch()?;
            } else {
                self.state.show_first_approval_explainer = false;
            }
        }
        self.sync_chat_items();
        self.publish_state();
        Ok(())
    }

    fn drop_terminal_parallel_approvals(
        &mut self,
        children: &BTreeMap<String, ParallelChildRuntimeState>,
        queue: &mut VecDeque<PendingParallelApproval>,
    ) {
        queue.retain(|pending| {
            children
                .get(&pending.step_id)
                .is_some_and(|child| child.terminal_result.is_none())
        });
    }

    fn apply_parallel_limit_checks(
        &mut self,
        run: &RunDriveContext,
        children: &mut BTreeMap<String, ParallelChildRuntimeState>,
        cancellation: &CancellationToken,
    ) -> Result<bool> {
        let mut changed = false;
        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            cancellation.cancel();
            for child in children.values_mut() {
                if child.terminal_result.is_none() {
                    child.terminal_result = Some(limit_reached_agent_result(
                        &child.agent_id,
                        &child.step_id,
                        "Parallel group reached max_wall_clock_minutes.",
                    ));
                    changed = true;
                }
            }
            return Ok(changed);
        }

        let timed_out = children
            .values()
            .filter(|child| {
                // Only spawned (Running) nodes can hit the per-step time limit; a
                // DAG node still Pending (planned, not admitted) has a stale
                // step_started_at and must not be terminalized for "timing out"
                // before it ever runs.
                child.terminal_result.is_none()
                    && child.node_run_state == NodeRunState::Running
                    && self.step_time_limit_reached(child.step_started_at)
            })
            .map(|child| {
                (
                    child.step_id.clone(),
                    child.agent_id.clone(),
                    child.step_started_at,
                    child.cancellation.clone(),
                )
            })
            .collect::<Vec<_>>();
        for (_, _, _, child_cancellation) in &timed_out {
            child_cancellation.cancel();
        }
        for (step_id, agent_id, started_at, _) in timed_out {
            self.stop_for_step_time_limit(run, &step_id, started_at)?;
            if let Some(child) = children.get_mut(&step_id) {
                child.terminal_result = Some(limit_reached_agent_result(
                    &agent_id,
                    &step_id,
                    "Parallel child reached max_step_minutes.",
                ));
                changed = true;
            }
        }
        Ok(changed)
    }

    fn record_parallel_child_result(
        &mut self,
        run: &mut RunDriveContext,
        group_id: &str,
        child: &mut ParallelChildRuntimeState,
        result: AgentResult,
    ) -> Result<()> {
        let status = result.status.clone();
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group_id.to_string()),
            Some(result.step_id.clone()),
            "agent_result",
            serde_json::to_value(&result)?,
            format!("{}: {}", result.agent, result.summary),
        )?;
        let kind = if matches!(
            status,
            AgentResultStatus::Completed | AgentResultStatus::NoChanges
        ) {
            "parallel_child_completed"
        } else if matches!(
            status,
            AgentResultStatus::Blocked | AgentResultStatus::ApprovalDenied
        ) {
            "parallel_child_blocked"
        } else {
            "parallel_child_failed"
        };
        self.record_event_with_group(
            Some(run.run_id.clone()),
            Some(group_id.to_string()),
            Some(child.step_id.clone()),
            kind,
            json!({
                "group_id": group_id,
                "step_id": child.step_id,
                "agent": child.agent_id,
                "step_label": child.step_label,
                "file_scope": child.file_scope,
                "status": status
            }),
            format!("Parallel child finished: {}.", child.step_label),
        )?;
        self.set_live_step_status(
            &child.step_id,
            if matches!(
                result.status,
                AgentResultStatus::Completed | AgentResultStatus::NoChanges
            ) {
                LiveStepStatus::Completed
            } else {
                LiveStepStatus::Failed
            },
        );
        if let Some(workflow) = run.workflow.as_mut() {
            workflow.record_child_result(
                group_id,
                &child.step_id,
                &child.file_scope,
                &result,
                &self.config.working_directory,
                &self.config.workspace.extra_write_roots,
            )?;
        }
        child.terminal_result = Some(result);
        child.result_recorded = true;
        Ok(())
    }

    async fn run_orchestrator_step(
        &mut self,
        run: &mut RunDriveContext,
        resume: Option<(String, RuntimeRequest, Instant)>,
    ) -> Result<OrchestratorStepOutcome> {
        self.state.run_state = RunState::Planning;
        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            return Ok(OrchestratorStepOutcome::Stop);
        }
        let (step_id, request, step_started_at) = if let Some(resume) = resume {
            resume
        } else {
            if limit_reached(&self.config.limits.max_agent_steps, run.step_count) {
                self.state.run_state = RunState::LimitReached;
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "run_limit_reached",
                    json!({ "limit": "max_agent_steps", "value": run.step_count }),
                    "Run limit reached.",
                )?;
                return Ok(OrchestratorStepOutcome::Stop);
            }

            let orchestrator = self.agent("orchestrator")?.clone();
            let step_id = new_id();
            let step_started_at = Instant::now();
            run.step_count += 1;
            self.record_event(
                Some(run.run_id.clone()),
                Some(step_id.clone()),
                "agent_step_started",
                json!({ "agent": "orchestrator" }),
                "Orchestrator step started.",
            )?;
            let request = self.runtime_request(
                &run.run_id,
                &step_id,
                RuntimePrompt::new(&run.prompt, run.skill_context.as_ref()),
                orchestrator,
                run.previous_results.clone(),
                "orchestrator_decision",
            )?;
            (step_id, request, step_started_at)
        };

        let step = PausedStep::Orchestrator {
            step_id: step_id.clone(),
        };
        self.set_active_step(&run.run_id, &step_id, "orchestrator");
        match self
            .execute_runtime_step_with_actions(request, run, step, step_started_at)
            .await
        {
            Ok(StepOutcome::Output(output)) => match *output {
                RuntimeOutput::OrchestratorDecision { decision } => {
                    if let Err(error) = validate_orchestrator_decision(&decision, &self.config) {
                        self.record_parallel_group_rejected_if_present(
                            &run.run_id,
                            &decision,
                            &error.to_string(),
                        )?;
                        self.state.run_state = RunState::Failed;
                        self.record_event(
                            Some(run.run_id.clone()),
                            Some(step_id.clone()),
                            "orchestrator_decision_invalid",
                            json!({ "reason": error.to_string(), "decision": decision }),
                            "Orchestrator decision was invalid.",
                        )?;
                        self.clear_active_step(&step_id);
                        return Ok(OrchestratorStepOutcome::Stop);
                    }

                    self.record_event(
                        Some(run.run_id.clone()),
                        Some(decision.decision_id.clone()),
                        "orchestrator_decision",
                        serde_json::to_value(&decision)?,
                        format!("Orchestrator: {}", decision.reason),
                    )?;
                    self.clear_active_step(&step_id);
                    Ok(OrchestratorStepOutcome::Decision(Box::new(decision)))
                }
                RuntimeOutput::ParseError {
                    agent,
                    raw_output,
                    diagnostic,
                } => {
                    let result = self.persist_parse_error(
                        &run.run_id,
                        &step_id,
                        &agent,
                        raw_output,
                        diagnostic,
                    )?;
                    run.previous_results.push(RunStepResult::Agent { result });
                    if self.schedule_parse_repair(run, &step_id, &agent)? {
                        self.clear_active_step(&step_id);
                        return Ok(OrchestratorStepOutcome::Retry);
                    }
                    self.state.run_state = RunState::Failed;
                    self.record_event(
                        Some(run.run_id.clone()),
                        None,
                        "run_failed",
                        json!({ "reason": "orchestrator parse_error" }),
                        "Run failed after an orchestrator parse error.",
                    )?;
                    self.clear_active_step(&step_id);
                    Ok(OrchestratorStepOutcome::Stop)
                }
                RuntimeOutput::AgentResult { .. } => {
                    Err(anyhow!("orchestrator runtime returned an agent result"))
                }
                RuntimeOutput::ActionRequest { .. } => Err(anyhow!(
                    "runtime action loop returned an unhandled action request"
                )),
            },
            Ok(StepOutcome::Paused) => Ok(OrchestratorStepOutcome::Paused),
            Ok(StepOutcome::LimitReached) => {
                self.clear_active_step(&step_id);
                Ok(OrchestratorStepOutcome::Stop)
            }
            Ok(StepOutcome::Interrupted) => Ok(OrchestratorStepOutcome::Stop),
            Err(error) => {
                self.state.run_state = RunState::Failed;
                self.record_event(
                    Some(run.run_id.clone()),
                    Some(step_id.clone()),
                    "run_failed",
                    json!({ "reason": error.to_string() }),
                    format!(
                        "Orchestrator failed: {}",
                        concise_diagnostic(&error.to_string())
                    ),
                )?;
                self.clear_active_step(&step_id);
                Ok(OrchestratorStepOutcome::Stop)
            }
        }
    }

    async fn run_agent_step(
        &mut self,
        run: &mut RunDriveContext,
        next_agent_id: &str,
        resume: Option<(String, RuntimeRequest, Instant)>,
    ) -> Result<AgentStepOutcome> {
        self.state.run_state = RunState::Running;
        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            return Ok(AgentStepOutcome::Stop);
        }
        let (step_id, request, step_started_at) = if let Some(resume) = resume {
            resume
        } else {
            if limit_reached(&self.config.limits.max_agent_steps, run.step_count) {
                self.state.run_state = RunState::LimitReached;
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "run_limit_reached",
                    json!({ "limit": "max_agent_steps", "value": run.step_count }),
                    "Run limit reached before the next specialized agent.",
                )?;
                return Ok(AgentStepOutcome::Stop);
            }

            let agent = self.agent(next_agent_id)?.clone();
            let step_id = new_id();
            let step_started_at = Instant::now();
            run.step_count += 1;
            self.record_event(
                Some(run.run_id.clone()),
                Some(step_id.clone()),
                "agent_step_started",
                json!({ "agent": next_agent_id }),
                format!("{next_agent_id} step started."),
            )?;
            let request = self.runtime_request(
                &run.run_id,
                &step_id,
                RuntimePrompt::new(&run.prompt, run.skill_context.as_ref()),
                agent,
                run.previous_results.clone(),
                "agent_result",
            )?;
            (step_id, request, step_started_at)
        };

        let step = PausedStep::Agent {
            step_id: step_id.clone(),
            agent_id: next_agent_id.to_string(),
        };
        self.set_active_step(&run.run_id, &step_id, next_agent_id);
        match self
            .execute_runtime_step_with_actions(request, run, step, step_started_at)
            .await
        {
            Ok(StepOutcome::Output(output)) => match *output {
                RuntimeOutput::AgentResult { result } => {
                    self.record_event(
                        Some(run.run_id.clone()),
                        Some(result.step_id.clone()),
                        "agent_result",
                        serde_json::to_value(&result)?,
                        format!("{}: {}", result.agent, result.summary),
                    )?;
                    run.previous_results.push(RunStepResult::Agent { result });
                    self.clear_active_step(&step_id);
                    Ok(AgentStepOutcome::Completed)
                }
                RuntimeOutput::ParseError {
                    agent,
                    raw_output,
                    diagnostic,
                } => {
                    let result = self.persist_parse_error(
                        &run.run_id,
                        &step_id,
                        &agent,
                        raw_output,
                        diagnostic,
                    )?;
                    run.previous_results.push(RunStepResult::Agent { result });
                    if self.schedule_parse_repair(run, &step_id, &agent)? {
                        self.clear_active_step(&step_id);
                        return Ok(AgentStepOutcome::Completed);
                    }
                    self.state.run_state = RunState::Failed;
                    self.record_event(
                        Some(run.run_id.clone()),
                        None,
                        "run_failed",
                        json!({ "reason": "agent parse_error" }),
                        "Run failed after an agent parse error.",
                    )?;
                    self.clear_active_step(&step_id);
                    Ok(AgentStepOutcome::Stop)
                }
                RuntimeOutput::OrchestratorDecision { .. } => Err(anyhow!(
                    "specialized runtime returned an orchestrator decision"
                )),
                RuntimeOutput::ActionRequest { .. } => Err(anyhow!(
                    "runtime action loop returned an unhandled action request"
                )),
            },
            Ok(StepOutcome::Paused) => Ok(AgentStepOutcome::Paused),
            Ok(StepOutcome::LimitReached) => {
                self.clear_active_step(&step_id);
                Ok(AgentStepOutcome::Stop)
            }
            Ok(StepOutcome::Interrupted) => Ok(AgentStepOutcome::Stop),
            Err(error) => {
                self.state.run_state = RunState::Failed;
                self.record_event(
                    Some(run.run_id.clone()),
                    Some(step_id.clone()),
                    "run_failed",
                    json!({ "reason": error.to_string() }),
                    format!(
                        "{next_agent_id} failed: {}",
                        concise_diagnostic(&error.to_string())
                    ),
                )?;
                self.clear_active_step(&step_id);
                Ok(AgentStepOutcome::Stop)
            }
        }
    }

    async fn run_council_workflow(
        &mut self,
        run: &mut RunDriveContext,
        decision: &crate::orchestrator::OrchestratorDecision,
    ) -> Result<bool> {
        self.state.run_state = RunState::Running;
        if self.wall_clock_limit_reached(run) {
            self.stop_for_wall_clock_limit(run)?;
            return Ok(false);
        }
        if !matches!(
            self.config.council.execution_mode,
            CouncilExecutionMode::Serial
        ) {
            self.state.run_state = RunState::Failed;
            self.record_event(
                Some(run.run_id.clone()),
                Some(decision.decision_id.clone()),
                "run_failed",
                json!({ "reason": "unsupported council execution_mode" }),
                "Council execution mode is unsupported.",
            )?;
            return Ok(false);
        }

        let preset_name = self.config.council.default_preset.clone();
        let members = self
            .config
            .council
            .presets
            .get(&preset_name)
            .cloned()
            .ok_or_else(|| anyhow!("council preset {preset_name} is not configured"))?;
        let timeout = Duration::from_secs(self.config.council.timeout_seconds);
        self.record_event(
            Some(run.run_id.clone()),
            Some(decision.decision_id.clone()),
            "council_started",
            json!({
                "preset": preset_name,
                "execution_mode": self.config.council.execution_mode.clone(),
                "timeout_seconds": self.config.council.timeout_seconds,
                "councillors": members.keys().cloned().collect::<Vec<_>>()
            }),
            "Council workflow started.",
        )?;

        let mut reports = Vec::new();
        for (member_id, member) in members {
            if self.wall_clock_limit_reached(run) {
                self.stop_for_wall_clock_limit(run)?;
                return Ok(false);
            }
            if limit_reached(&self.config.limits.max_agent_steps, run.step_count) {
                self.state.run_state = RunState::LimitReached;
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "run_limit_reached",
                    json!({ "limit": "max_agent_steps", "value": run.step_count }),
                    "Run limit reached before the next councillor.",
                )?;
                return Ok(false);
            }

            let step_id = new_id();
            run.step_count += 1;
            self.record_event(
                Some(run.run_id.clone()),
                Some(step_id.clone()),
                "agent_step_started",
                json!({ "agent": format!("{COUNCIL_WORKFLOW_AGENT_ID}.{member_id}") }),
                format!("Council councillor {member_id} step started."),
            )?;

            let agent = council_member_agent(&member_id, &member);
            let prompt = council_member_prompt(&run.prompt, decision);
            let request = self.runtime_request(
                &run.run_id,
                &step_id,
                RuntimePrompt::new(&prompt, run.skill_context.as_ref()),
                agent,
                run.previous_results.clone(),
                "agent_result",
            )?;
            self.set_active_step(
                &run.run_id,
                &step_id,
                &format!("{COUNCIL_WORKFLOW_AGENT_ID}.{member_id}"),
            );

            let step_result =
                tokio::time::timeout(timeout, execute_runtime_step(&self.config, request)).await;
            let report = match step_result {
                Ok(Ok(result)) => {
                    self.record_runtime_stream_deltas(
                        run,
                        &step_id,
                        &format!("{COUNCIL_WORKFLOW_AGENT_ID}.{member_id}"),
                        &result.stream_deltas,
                    )?;
                    self.council_report_from_runtime_output(
                        run,
                        &step_id,
                        &member_id,
                        result.output,
                    )?
                }
                Ok(Err(error)) => CouncilMemberReport {
                    member_id: member_id.clone(),
                    status: AgentResultStatus::Failed,
                    summary: "Councillor runtime failed.".to_string(),
                    diagnostic: Some(concise_diagnostic(&error.to_string())),
                    artifact: None,
                },
                Err(_) => CouncilMemberReport {
                    member_id: member_id.clone(),
                    status: AgentResultStatus::Failed,
                    summary: "Councillor timed out.".to_string(),
                    diagnostic: Some(format!(
                        "councillor exceeded council timeout of {} seconds",
                        timeout.as_secs()
                    )),
                    artifact: None,
                },
            };
            self.record_event(
                Some(run.run_id.clone()),
                Some(step_id.clone()),
                "councillor_result",
                serde_json::to_value(&report)?,
                format!(
                    "Council {member_id}: {}",
                    concise_diagnostic(&report.summary)
                ),
            )?;
            self.clear_active_step(&step_id);
            reports.push(report);
        }

        let synthesis = synthesize_council_decision(decision, &reports);
        let result = council_agent_result(new_id(), &synthesis, &reports);
        self.record_event(
            Some(run.run_id.clone()),
            Some(result.step_id.clone()),
            "council_synthesized",
            json!({
                "envelope": synthesis,
                "reports": reports
            }),
            "Council synthesized councillor results.",
        )?;
        self.record_event(
            Some(run.run_id.clone()),
            Some(result.step_id.clone()),
            "agent_result",
            serde_json::to_value(&result)?,
            format!("council: {}", result.summary),
        )?;
        self.record_event(
            Some(run.run_id.clone()),
            Some(result.step_id.clone()),
            "council_completed",
            json!({ "result": result.clone() }),
            "Council workflow completed.",
        )?;
        run.previous_results.push(RunStepResult::Agent { result });
        Ok(true)
    }

    fn council_report_from_runtime_output(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        member_id: &str,
        output: RuntimeOutput,
    ) -> Result<CouncilMemberReport> {
        match output {
            RuntimeOutput::AgentResult { result } => {
                self.record_event(
                    Some(run.run_id.clone()),
                    Some(result.step_id.clone()),
                    "councillor_agent_result",
                    serde_json::to_value(&result)?,
                    format!("{member_id}: {}", result.summary),
                )?;
                self.council_report_from_agent_result(run, step_id, member_id, &result)
            }
            RuntimeOutput::ParseError {
                raw_output,
                diagnostic,
                ..
            } => {
                let artifact = self.history.write_artifact(
                    "txt",
                    "text/plain",
                    raw_output.as_bytes(),
                    "contains_user_content",
                )?;
                self.record_event(
                    Some(run.run_id.clone()),
                    Some(step_id.to_string()),
                    "artifact_written",
                    serde_json::to_value(&artifact)?,
                    "Malformed councillor output stored as an artifact.",
                )?;
                Ok(CouncilMemberReport {
                    member_id: member_id.to_string(),
                    status: AgentResultStatus::ParseError,
                    summary: "Councillor output did not match the required structured contract."
                        .to_string(),
                    diagnostic: Some(diagnostic),
                    artifact: Some(artifact),
                })
            }
            RuntimeOutput::ActionRequest { .. } => Ok(CouncilMemberReport {
                member_id: member_id.to_string(),
                status: AgentResultStatus::Failed,
                summary: "Councillor requested an action, but council runs cannot execute actions."
                    .to_string(),
                diagnostic: Some(
                    "council workflows collect opinions only; use fixer/reviewer for actions"
                        .to_string(),
                ),
                artifact: None,
            }),
            RuntimeOutput::OrchestratorDecision { .. } => Ok(CouncilMemberReport {
                member_id: member_id.to_string(),
                status: AgentResultStatus::Failed,
                summary: "Councillor returned an orchestrator decision instead of an agent result."
                    .to_string(),
                diagnostic: Some("councillors must return agent_result envelopes".to_string()),
                artifact: None,
            }),
        }
    }

    fn council_report_from_agent_result(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        member_id: &str,
        result: &AgentResult,
    ) -> Result<CouncilMemberReport> {
        let artifact = self.write_large_councillor_artifact(run, step_id, member_id, result)?;
        Ok(CouncilMemberReport {
            member_id: member_id.to_string(),
            status: result.status.clone(),
            summary: concise_diagnostic(&result.summary),
            diagnostic: result
                .blocker
                .clone()
                .map(|blocker| concise_diagnostic(&blocker)),
            artifact,
        })
    }

    fn write_large_councillor_artifact(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        member_id: &str,
        result: &AgentResult,
    ) -> Result<Option<crate::orchestrator::ArtifactReference>> {
        let contents = serde_json::to_vec_pretty(result)?;
        if contents.len() <= LARGE_ACTION_CONTENT_BYTES {
            return Ok(None);
        }
        let artifact = self.history.write_artifact(
            "json",
            "application/json",
            &contents,
            "contains_user_content",
        )?;
        self.record_event(
            Some(run.run_id.clone()),
            Some(step_id.to_string()),
            "artifact_written",
            json!({ "member_id": member_id, "artifact": artifact }),
            "Large councillor output stored as an artifact.",
        )?;
        Ok(Some(artifact))
    }

    fn write_run_record(&mut self, run: &RunDriveContext) -> Result<()> {
        let record = RunRecord {
            schema_version: 1,
            run_id: run.run_id.clone(),
            parent_run_id: run.parent_run_id.clone(),
            session_id: self.history.session_id().to_string(),
            submitted_prompt: run.submitted_prompt.clone(),
            prompt: run.prompt.clone(),
            loaded_skills: run.loaded_skill_metadata(),
            session_goal: self.state.session_goal.clone(),
            workflow: run.workflow.clone(),
            subtask: run.subtask.as_ref().map(|subtask| SubtaskRecord {
                agent_id: subtask.agent_id.clone(),
                submitted_request: subtask.submitted_request.clone(),
                request: subtask.request.clone(),
            }),
            state: self.state.run_state.clone(),
            results: run.previous_results.clone(),
        };
        self.history.write_run_record(&run.run_id, &record)?;
        Ok(())
    }

    fn set_active_step(&mut self, run_id: &str, step_id: &str, agent: &str) {
        self.set_active_step_with_metadata(run_id, None, step_id, None, None, agent);
    }

    fn set_active_step_with_metadata(
        &mut self,
        run_id: &str,
        group_id: Option<String>,
        step_id: &str,
        step_label: Option<String>,
        file_scope: Option<ParallelFileScope>,
        agent: &str,
    ) {
        let active = ActiveStep {
            run_id: run_id.to_string(),
            group_id: group_id.clone(),
            step_id: step_id.to_string(),
            step_label: step_label.clone(),
            file_scope: file_scope.clone(),
            agent: agent.to_string(),
        };
        self.active_step = Some(active.clone());
        self.active_steps
            .retain(|step| step.step_id != step_id || step.run_id != run_id);
        self.active_steps.push(active);
        let view = LiveStepView {
            run_id: run_id.to_string(),
            group_id,
            step_id: step_id.to_string(),
            step_label,
            file_scope,
            agent: agent.to_string(),
            status: LiveStepStatus::Starting,
            streams: Vec::new(),
        };
        self.state
            .live_steps
            .retain(|live_step| live_step.step_id != step_id || live_step.run_id != run_id);
        self.state.live_steps.push(view.clone());
        self.state.live_step = Some(view);
        // Stamp lifecycle timing for this step (ADR-004). Both timestamps start
        // equal; keyed by `step_id` so parallel-group peers stay independent.
        let now = Instant::now();
        self.step_timings.insert(
            step_id.to_string(),
            StepTiming {
                started_at: now,
                last_activity: now,
            },
        );
        self.sync_chat_items();
        self.set_agent_status(agent, "running");
    }

    fn clear_active_step(&mut self, step_id: &str) {
        let agent = self
            .active_steps
            .iter()
            .find(|step| step.step_id == step_id)
            .map(|step| step.agent.clone())
            .or_else(|| {
                self.active_step
                    .as_ref()
                    .filter(|step| step.step_id == step_id)
                    .map(|step| step.agent.clone())
            });
        if let Some(agent) = agent {
            self.state
                .live_steps
                .retain(|live_step| live_step.step_id != step_id);
            self.state.live_step = self.state.live_steps.first().cloned();
            self.sync_chat_items();
            self.active_steps.retain(|step| step.step_id != step_id);
            if self
                .active_steps
                .iter()
                .any(|step| step.agent == agent && step.group_id.is_some())
            {
                self.set_agent_status(&agent, "running_parallel");
            } else if self.active_steps.iter().any(|step| step.agent == agent) {
                self.set_agent_status(&agent, "running");
            } else {
                self.set_agent_status(&agent, "idle");
            }
            if self
                .active_step
                .as_ref()
                .is_some_and(|step| step.step_id == step_id)
            {
                self.active_step = self.active_steps.first().cloned();
            }
        }
        // Drop the timing entry after the step is cleared from `active_steps`
        // and `live_steps` to prevent unbounded growth (ADR-004). Done
        // unconditionally so a missed agent lookup above cannot leak an entry.
        self.step_timings.remove(step_id);
    }

    fn set_agent_status(&mut self, agent_id: &str, status: &str) {
        if let Some(agent) = self
            .state
            .agents
            .iter_mut()
            .find(|agent| agent.id == agent_id && agent.status != "disabled")
        {
            agent.status = status.to_string();
            self.publish_state();
        }
    }

    fn reset_enabled_agent_statuses(&mut self) {
        for agent in &mut self.state.agents {
            if agent.status != "disabled" {
                agent.status = "idle".to_string();
            }
        }
        self.publish_state();
    }

    fn record_step_cancelled_if_active(&mut self) -> Result<()> {
        let steps = if self.active_steps.is_empty() {
            self.active_step.iter().cloned().collect::<Vec<_>>()
        } else {
            self.active_steps.clone()
        };
        if steps.is_empty() {
            return Ok(());
        };
        for step in steps {
            self.set_live_step_status(&step.step_id, LiveStepStatus::Interrupted);
            self.set_agent_status(&step.agent, "interrupted");
            let payload = json!({
                "agent": step.agent,
                "step_label": step.step_label,
                "file_scope": step.file_scope
            });
            self.record_event_with_group(
                Some(step.run_id.clone()),
                step.group_id.clone(),
                Some(step.step_id.clone()),
                "step_cancel_requested",
                payload.clone(),
                "Step cancellation requested.",
            )?;
            self.record_event_with_group(
                Some(step.run_id),
                step.group_id,
                Some(step.step_id),
                "step_cancelled",
                payload,
                "Step cancelled.",
            )?;
        }
        Ok(())
    }

    fn record_step_cancel_failed_if_active(&mut self, diagnostic: &str) -> Result<()> {
        let Some(step) = self.active_step.clone() else {
            return Ok(());
        };
        self.set_live_step_status(&step.step_id, LiveStepStatus::Failed);
        self.set_agent_status(&step.agent, "failed");
        let payload = json!({
            "agent": step.agent,
            "diagnostic": diagnostic
        });
        self.record_event(
            Some(step.run_id.clone()),
            Some(step.step_id.clone()),
            "step_cancel_requested",
            json!({ "agent": step.agent }),
            "Step cancellation requested.",
        )?;
        self.record_event(
            Some(step.run_id),
            Some(step.step_id),
            "step_cancel_failed",
            payload,
            "Step cancellation failed.",
        )
    }

    fn wall_clock_limit_reached(&self, run: &RunDriveContext) -> bool {
        wall_clock_limit_reached(&self.config.limits.max_wall_clock_minutes, run.started_at)
    }

    fn stop_for_wall_clock_limit(&mut self, run: &RunDriveContext) -> Result<()> {
        self.state.run_state = RunState::LimitReached;
        self.record_event(
            Some(run.run_id.clone()),
            None,
            "run_limit_reached",
            json!({
                "limit": "max_wall_clock_minutes",
                "elapsed_seconds": run.started_at.elapsed().as_secs()
            }),
            "Run wall-clock limit reached.",
        )
    }

    fn step_time_limit_reached(&self, step_started_at: Instant) -> bool {
        time_limit_reached(&self.config.limits.max_step_minutes, step_started_at)
    }

    fn stop_for_step_time_limit(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        step_started_at: Instant,
    ) -> Result<()> {
        self.state.run_state = RunState::LimitReached;
        self.record_event(
            Some(run.run_id.clone()),
            Some(step_id.to_string()),
            "step_limit_reached",
            json!({
                "limit": "max_step_minutes",
                "elapsed_seconds": step_started_at.elapsed().as_secs()
            }),
            "Step time limit reached.",
        )
    }

    fn stop_for_step_action_limit(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        action_count: usize,
    ) -> Result<()> {
        self.state.run_state = RunState::LimitReached;
        self.record_event(
            Some(run.run_id.clone()),
            Some(step_id.to_string()),
            "step_limit_reached",
            json!({
                "limit": "max_step_actions",
                "value": action_count
            }),
            "Step action limit reached.",
        )
    }

    fn review_fix_cycle_limit_reached(&self, run: &RunDriveContext, next_agent_id: &str) -> bool {
        next_agent_id == "fixer"
            && limit_reached(
                &self.config.limits.max_review_fix_cycles,
                review_fix_cycle_count(&run.previous_results),
            )
    }

    fn stop_for_review_fix_cycle_limit(&mut self, run: &RunDriveContext) -> Result<()> {
        self.state.run_state = RunState::LimitReached;
        self.record_event(
            Some(run.run_id.clone()),
            None,
            "run_limit_reached",
            json!({
                "limit": "max_review_fix_cycles",
                "value": review_fix_cycle_count(&run.previous_results)
            }),
            "Run review/fix cycle limit reached.",
        )
    }

    fn finish_subtask_run(&mut self, run: &RunDriveContext) -> Result<()> {
        let subtask = run
            .subtask
            .as_ref()
            .context("subtask run is missing subtask context")?;
        let result = agent_results(&run.previous_results)
            .last()
            .ok_or_else(|| anyhow!("subtask finished without an agent result"))?;
        let completed = matches!(
            result.status,
            AgentResultStatus::Completed | AgentResultStatus::NoChanges
        );
        self.state.run_state = if completed {
            RunState::Completed
        } else {
            RunState::Failed
        };
        self.record_event(
            Some(run.run_id.clone()),
            Some(result.step_id.clone()),
            "subtask_completed",
            json!({
                "run_id": run.run_id,
                "parent_run_id": run.parent_run_id,
                "agent": subtask.agent_id,
                "request": subtask.request,
                "result": result,
                "scope_guard": "subtask_result_must_not_broaden_request",
            }),
            format!(
                "Subtask completed: {}: {}",
                subtask.agent_id, result.summary
            ),
        )
    }

    fn schedule_parse_repair(
        &mut self,
        run: &mut RunDriveContext,
        step_id: &str,
        agent: &str,
    ) -> Result<bool> {
        if run.subtask.is_some() {
            return Ok(false);
        }
        if run.parse_repair_attempts >= 1
            || limit_reached(&self.config.limits.max_agent_steps, run.step_count)
        {
            return Ok(false);
        }

        run.parse_repair_attempts += 1;
        self.record_event(
            Some(run.run_id.clone()),
            Some(step_id.to_string()),
            "diagnostic",
            json!({
                "reason": "parse_error",
                "agent": agent,
                "repair_attempt": run.parse_repair_attempts
            }),
            "Runtime parse error queued for Orchestrator repair.",
        )?;
        Ok(true)
    }

    fn runtime_request(
        &self,
        run_id: &str,
        step_id: &str,
        prompt: RuntimePrompt<'_>,
        agent_profile: AgentProfile,
        previous_results: Vec<RunStepResult>,
        output_schema: &str,
    ) -> Result<RuntimeRequest> {
        let mut agent_profile = agent_profile;
        if agent_profile.id == "orchestrator" {
            // Advertise MCP tools from the recorded snapshot, never a live handle
            // (ADR-001, task_07), so the prompt is replay-deterministic.
            agent_profile.instructions = crate::orchestrator::build_orchestrator_prompt_with_mcp(
                &self.config,
                &self.mcp_catalog,
            );
        }
        let prompt = skills::render_runtime_prompt(prompt.skill_context, prompt.text);
        Ok(RuntimeRequest {
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            prompt,
            session_goal: self.state.session_goal.clone(),
            working_directory: self.config.working_directory.clone(),
            capability_constraints: agent_profile.capabilities.clone(),
            agent_profile,
            session_events: self.runtime_history_events()?,
            recent_context: self.runtime_recent_context()?,
            previous_results,
            action_results: Vec::new(),
            output_schema: output_schema.to_string(),
            parallel_context: None,
            limits: self.config.limits.clone(),
        })
    }

    fn runtime_history_events(&self) -> Result<Vec<RuntimeHistoryEvent>> {
        let events = self.history.read_events()?;
        let start = events.len().saturating_sub(RUNTIME_HISTORY_EVENT_LIMIT);
        Ok(events[start..].iter().map(runtime_history_event).collect())
    }

    fn runtime_recent_context(&self) -> Result<RuntimeRecentContext> {
        let events = self.history.read_events()?;
        Ok(runtime_recent_context(&events))
    }

    async fn drive_runtime_step_streaming(
        &mut self,
        request: RuntimeRequest,
        run: &RunDriveContext,
        step_id: &str,
        step_started_at: Instant,
        first_sequence: u32,
    ) -> Result<(Option<RuntimeOutput>, u32)> {
        if time_limit_reached(&self.config.limits.max_step_minutes, step_started_at) {
            self.stop_for_step_time_limit(run, step_id, step_started_at)?;
            return Ok((None, first_sequence));
        }

        let agent_id = request.agent_profile.id.clone();
        self.set_live_step_status(step_id, LiveStepStatus::Running);
        self.set_agent_status(&agent_id, "running");

        let (events, mut receiver) =
            RuntimeEventSink::channel_from(RUNTIME_EVENT_CHANNEL_CAPACITY, first_sequence);
        let cancellation = CancellationToken::new();
        let config = self.config.clone();
        let runtime_step =
            execute_runtime_step_streaming(&config, request, events, cancellation.clone());
        tokio::pin!(runtime_step);
        let interrupt_start = *self.interrupt_receiver.borrow();
        let interrupt_requested =
            wait_for_interrupt(self.interrupt_receiver.clone(), interrupt_start);
        tokio::pin!(interrupt_requested);

        let step_limit = self.config.limits.max_step_minutes.clone();
        let limit_reached = async move {
            match remaining_limit_duration(&step_limit, step_started_at) {
                Some(remaining) => tokio::time::sleep(remaining).await,
                None => pending::<()>().await,
            }
        };
        tokio::pin!(limit_reached);

        let mut coalescer = RuntimeStreamCoalescer::new(agent_id.clone());
        let mut next_sequence = first_sequence;
        let mut flush_interval = tokio::time::interval(STREAM_COALESCE_INTERVAL);
        flush_interval.tick().await;

        loop {
            tokio::select! {
                output = &mut runtime_step => {
                    while let Ok(event) = receiver.try_recv() {
                        next_sequence = next_sequence.max(event.sequence().saturating_add(1));
                        self.record_runtime_event(run, step_id, &mut coalescer, event)?;
                    }
                    self.set_live_step_status(
                        step_id,
                        if output.is_ok() {
                            LiveStepStatus::Completed
                        } else {
                            LiveStepStatus::Failed
                        },
                    );
                    self.flush_runtime_stream_coalescer(run, step_id, &mut coalescer, true)?;
                    return output.map(|output| (Some(output), next_sequence));
                }
                event = receiver.recv() => {
                    if let Some(event) = event {
                        next_sequence = next_sequence.max(event.sequence().saturating_add(1));
                        self.record_runtime_event(run, step_id, &mut coalescer, event)?;
                    }
                }
                _ = flush_interval.tick() => {
                    self.flush_runtime_stream_coalescer(run, step_id, &mut coalescer, false)?;
                }
                _ = &mut limit_reached => {
                    self.set_live_step_status(step_id, LiveStepStatus::Cancelling);
                    self.set_agent_status(&agent_id, "cancelling");
                    cancellation.cancel();
                    self.flush_runtime_stream_coalescer(run, step_id, &mut coalescer, true)?;
                    self.stop_for_step_time_limit(run, step_id, step_started_at)?;
                    return Ok((None, next_sequence));
                }
                _ = &mut interrupt_requested => {
                    self.set_live_step_status(step_id, LiveStepStatus::Cancelling);
                    self.set_agent_status(&agent_id, "cancelling");
                    cancellation.cancel();
                    let stopped = tokio::time::timeout(
                        Duration::from_secs(2),
                        &mut runtime_step,
                    )
                    .await;
                    while let Ok(event) = receiver.try_recv() {
                        next_sequence = next_sequence.max(event.sequence().saturating_add(1));
                        self.record_runtime_event(run, step_id, &mut coalescer, event)?;
                    }
                    self.set_live_step_status(step_id, LiveStepStatus::Interrupted);
                    self.flush_runtime_stream_coalescer(run, step_id, &mut coalescer, true)?;
                    self.finish_streaming_interrupt(run, step_id, stopped)?;
                    return Ok((None, next_sequence));
                }
            }
        }
    }

    async fn execute_runtime_step_with_actions(
        &mut self,
        mut request: RuntimeRequest,
        run: &RunDriveContext,
        step: PausedStep,
        step_started_at: Instant,
    ) -> Result<StepOutcome> {
        let run_id = run.run_id.as_str();
        let step_id = step.step_id().to_string();
        let mut next_runtime_sequence = 1;
        loop {
            let (output, next_sequence) = match self
                .drive_runtime_step_streaming(
                    request.clone(),
                    run,
                    &step_id,
                    step_started_at,
                    next_runtime_sequence,
                )
                .await?
            {
                (Some(output), next_sequence) => (output, next_sequence),
                (None, _) if matches!(self.state.run_state, RunState::Interrupted) => {
                    return Ok(StepOutcome::Interrupted);
                }
                (None, _) => return Ok(StepOutcome::LimitReached),
            };
            next_runtime_sequence = next_sequence;

            match output {
                RuntimeOutput::ActionRequest {
                    request: action_request,
                } => {
                    if limit_reached(
                        &self.config.limits.max_step_actions,
                        request.action_results.len() as u32,
                    ) {
                        self.stop_for_step_action_limit(
                            run,
                            &step_id,
                            request.action_results.len(),
                        )?;
                        return Ok(StepOutcome::LimitReached);
                    }
                    self.record_event(
                        Some(run_id.to_string()),
                        Some(step_id.clone()),
                        "action_requested",
                        serde_json::to_value(&action_request)?,
                        action_requested_display(&action_request),
                    )?;
                    self.set_live_step_status(&step_id, LiveStepStatus::WaitingForAction);
                    self.set_agent_status(&request.agent_profile.id, "waiting_action");
                    let context = ActionExecutionContext {
                        working_directory: self.config.working_directory.clone(),
                        workspace: self.config.workspace.clone(),
                        approval_mode: self.effective_approval_mode(),
                        command_timeout: command_timeout(&self.config.limits.max_command_minutes),
                        user_prompt: Some(run.prompt.clone()),
                        action_scope: crate::actions::ActionScope::Unrestricted,
                        floor: self.config.approval.floor,
                        trusted_targets: self.trust_store.snapshot(),
                        pre_approved: false,
                        drift_ack: self.drift_ack_context(),
                        mcp: self.mcp_action_context(),
                    };
                    self.record_command_started_if_executable(
                        run_id,
                        &step_id,
                        &request.agent_profile,
                        &context,
                        &action_request,
                    )?;
                    let action =
                        execute_action_request(&request.agent_profile, &context, &action_request);
                    let result = match await_with_step_limit(
                        action,
                        &self.config.limits.max_step_minutes,
                        step_started_at,
                    )
                    .await
                    {
                        Some(result) => result,
                        None => {
                            self.stop_for_step_time_limit(run, &step_id, step_started_at)?;
                            return Ok(StepOutcome::LimitReached);
                        }
                    };
                    self.record_action_specific_events(run_id, &step_id, &action_request, &result)?;
                    if matches!(result.status, ActionStatus::ApprovalRequired) {
                        self.set_live_step_status(&step_id, LiveStepStatus::WaitingForApproval);
                        self.set_agent_status(&request.agent_profile.id, "waiting_approval");
                        let view = build_pending_approval_view(
                            run_id.to_string(),
                            None,
                            step_id.clone(),
                            request.agent_profile.id.clone(),
                            result.summary.clone(),
                            result.diagnostic.clone(),
                            &action_request,
                            &context,
                        );
                        self.pending_approval = Some(PendingApproval {
                            run_id: run_id.to_string(),
                            step_id: step_id.clone(),
                            action_request: action_request.clone(),
                            agent_profile: request.agent_profile.clone(),
                            context,
                            reason: result.diagnostic.clone(),
                            run: run.clone(),
                            step: step.clone(),
                            step_started_at,
                            request: request.clone(),
                        });
                        self.state.pending_approval = Some(view);
                        // Show the one-line first-approval explainer at most once per
                        // user (ADR-004), consulting the persisted latch exactly when an
                        // approval becomes pending.
                        self.apply_first_approval_explainer_latch()?;
                        self.state.run_state = RunState::WaitingForUser;
                        self.record_event(
                            Some(run_id.to_string()),
                            Some(step_id.clone()),
                            "approval_requested",
                            serde_json::to_value(&result)?,
                            "Action approval required.",
                        )?;
                        return Ok(StepOutcome::Paused);
                    }
                    self.record_action_completed(
                        run_id,
                        &step_id,
                        &request.agent_profile.runtime,
                        &action_request,
                        &result,
                    )?;
                    request.action_results.push(result);
                }
                output => return Ok(StepOutcome::Output(Box::new(output))),
            }
        }
    }

    fn agent(&self, id: &str) -> Result<&AgentProfile> {
        self.config
            .agents
            .get(id)
            .ok_or_else(|| anyhow!("missing required agent {id}"))
    }

    fn persist_parse_error(
        &mut self,
        run_id: &str,
        step_id: &str,
        agent: &str,
        raw_output: String,
        diagnostic: String,
    ) -> Result<AgentResult> {
        let artifact = self.history.write_artifact(
            "txt",
            "text/plain",
            raw_output.as_bytes(),
            "contains_user_content",
        )?;
        self.record_event(
            Some(run_id.to_string()),
            Some(step_id.to_string()),
            "artifact_written",
            serde_json::to_value(&artifact)?,
            "Malformed runtime output stored as an artifact.",
        )?;
        let result = AgentResult {
            schema_version: 1,
            agent: agent.to_string(),
            step_id: step_id.to_string(),
            status: AgentResultStatus::ParseError,
            summary: "Runtime output did not match the required structured contract.".to_string(),
            findings: Vec::new(),
            changed_files: Vec::new(),
            commands: Vec::new(),
            verification: Vec::new(),
            blocker: Some(diagnostic),
            artifacts: vec![artifact],
        };
        self.record_event(
            Some(run_id.to_string()),
            Some(step_id.to_string()),
            "agent_result",
            serde_json::to_value(&result)?,
            "Parse error represented as an agent result.",
        )?;
        Ok(result)
    }

    fn persist_parse_error_with_group(
        &mut self,
        run_id: &str,
        group_id: &str,
        step_id: &str,
        agent: &str,
        raw_output: String,
        diagnostic: String,
    ) -> Result<AgentResult> {
        let artifact = self.history.write_artifact(
            "txt",
            "text/plain",
            raw_output.as_bytes(),
            "contains_user_content",
        )?;
        self.record_event_with_group(
            Some(run_id.to_string()),
            Some(group_id.to_string()),
            Some(step_id.to_string()),
            "artifact_written",
            serde_json::to_value(&artifact)?,
            "Malformed runtime output stored as an artifact.",
        )?;
        let result = AgentResult {
            schema_version: 1,
            agent: agent.to_string(),
            step_id: step_id.to_string(),
            status: AgentResultStatus::ParseError,
            summary: "Runtime output did not match the required structured contract.".to_string(),
            findings: Vec::new(),
            changed_files: Vec::new(),
            commands: Vec::new(),
            verification: Vec::new(),
            blocker: Some(diagnostic),
            artifacts: vec![artifact],
        };
        self.record_event_with_group(
            Some(run_id.to_string()),
            Some(group_id.to_string()),
            Some(step_id.to_string()),
            "agent_result",
            serde_json::to_value(&result)?,
            "Parse error represented as an agent result.",
        )?;
        Ok(result)
    }

    fn record_event(
        &mut self,
        run_id: Option<String>,
        step_id: Option<String>,
        kind: &str,
        payload: serde_json::Value,
        display: impl Into<String>,
    ) -> Result<()> {
        self.record_event_with_group(run_id, None, step_id, kind, payload, display)
    }

    fn record_skills_loaded(
        &mut self,
        run_id: &str,
        skill_context: Option<&SkillPromptContext>,
    ) -> Result<()> {
        let Some(skill_context) = skill_context.filter(|context| !context.loaded.is_empty()) else {
            return Ok(());
        };
        let metadata = skill_context.metadata();
        self.record_event(
            Some(run_id.to_string()),
            None,
            "skills_loaded",
            skills_loaded_payload_from_metadata(&metadata),
            loaded_skills_display(&metadata),
        )
    }

    fn record_workflow_completed(
        &mut self,
        run: &mut RunDriveContext,
        interrupted: bool,
    ) -> Result<()> {
        let Some(workflow) = run.workflow.as_mut() else {
            return Ok(());
        };
        if workflow.completion_recorded {
            return Ok(());
        }
        let payload = workflow.completion_payload(&run.run_id, interrupted);
        workflow.completion_recorded = true;
        self.record_event(
            Some(run.run_id.clone()),
            None,
            "workflow_completed",
            serde_json::to_value(payload)?,
            "Workflow completed.",
        )
    }

    fn record_event_with_group(
        &mut self,
        run_id: Option<String>,
        group_id: Option<String>,
        step_id: Option<String>,
        kind: &str,
        payload: serde_json::Value,
        display: impl Into<String>,
    ) -> Result<()> {
        self.append_event_with_group(run_id, group_id, step_id, kind, payload, display)?;
        self.publish_state();
        Ok(())
    }

    /// Durably append an event and fold it into the projection/chat **without**
    /// broadcasting. [`record_event_with_group`] is this plus a `publish_state`;
    /// `adopt_session` (ADR-006) appends its reconciling `run_interrupted` event
    /// through this path so the swap emits exactly one `watch` broadcast rather
    /// than one mid-swap and one at the end.
    fn append_event_with_group(
        &mut self,
        run_id: Option<String>,
        group_id: Option<String>,
        step_id: Option<String>,
        kind: &str,
        payload: serde_json::Value,
        display: impl Into<String>,
    ) -> Result<()> {
        let event = HistoryEvent::new_with_group(
            self.history.session_id().to_string(),
            run_id,
            group_id,
            step_id,
            kind,
            payload,
        );
        self.history.append_event(&event)?;
        // Non-blocking hook tap (ADR-003): immediately after the durable append,
        // map the event to the public vocabulary, resolve the actor, normalize,
        // and `try_send` to the dispatcher. Gated on a wired sender, so it is
        // zero-cost when no hooks are configured and never `.await`s here.
        self.dispatch_hooks_for_event(&event);
        if self.debug_enabled {
            self.history.append_debug_event(&event)?;
        }
        self.chat_projection.apply_history_event(&event);
        self.sync_chat_items();
        self.state.events.push(display.into());
        Ok(())
    }

    /// Durably record a graph-keyed event and broadcast (ADR-005). The DAG
    /// sibling of [`record_event_with_group`]: the scheduler (task_04) emits the
    /// `execution_graph_*` / `node_*` lifecycle through here so graph events
    /// carry `graph_id` and are never mis-keyed through the flat-group path.
    fn record_event_with_graph(
        &mut self,
        run_id: Option<String>,
        graph_id: Option<String>,
        step_id: Option<String>,
        kind: &str,
        payload: serde_json::Value,
        display: impl Into<String>,
    ) -> Result<()> {
        self.append_event_with_graph(run_id, graph_id, step_id, kind, payload, display)?;
        self.publish_state();
        Ok(())
    }

    /// Durably append a graph-keyed event and fold it into the projection
    /// **without** broadcasting — the DAG sibling of [`append_event_with_group`].
    fn append_event_with_graph(
        &mut self,
        run_id: Option<String>,
        graph_id: Option<String>,
        step_id: Option<String>,
        kind: &str,
        payload: serde_json::Value,
        display: impl Into<String>,
    ) -> Result<()> {
        let event = HistoryEvent::new_with_graph(
            self.history.session_id().to_string(),
            run_id,
            graph_id,
            step_id,
            kind,
            payload,
        );
        self.history.append_event(&event)?;
        self.dispatch_hooks_for_event(&event);
        if self.debug_enabled {
            self.history.append_debug_event(&event)?;
        }
        self.chat_projection.apply_history_event(&event);
        self.sync_chat_items();
        self.state.events.push(display.into());
        Ok(())
    }

    /// The event tap (ADR-003): resolve actor + normalize + match handlers +
    /// `try_send` to the off-thread dispatcher. Non-blocking and pure-read of
    /// `&self`; returns immediately when no sender is wired or the event is
    /// outside the public vocabulary. `hook_started`/`hook_completed` map to
    /// `None` here, so recording them never re-triggers a hook (recursion guard).
    fn dispatch_hooks_for_event(&self, event: &HistoryEvent) {
        let Some(sender) = self.hook_sender.as_ref() else {
            return; // No hooks wired ⇒ zero cost.
        };
        let Some(public_event) = public_name_for_kind(&event.kind) else {
            return; // Outside the public vocabulary (incl. hook_* events).
        };
        let matched: Vec<MatchedHandler> = self
            .config
            .hooks
            .handlers
            .iter()
            .enumerate()
            .filter(|(_, handler)| handler.matches(public_event))
            .map(|(index, handler)| MatchedHandler {
                index,
                handler: handler.clone(),
            })
            .collect();
        if matched.is_empty() {
            return;
        }
        let actor = self.resolve_actor_ctx(event);
        // Normalize with the full body so the dispatcher can project per-handler
        // detail (metadata strips it); redaction happens in the dispatcher.
        let Some(payload) = normalize(event, actor, PayloadDetail::Full) else {
            return;
        };
        try_dispatch(
            sender,
            HookDispatch {
                payload,
                handlers: matched,
            },
            &self.dropped_hooks,
        );
    }

    /// Resolve the uniform cross-runtime actor for an event (ADR-003): the agent
    /// of the step the event belongs to (matched by `step_id`, falling back to
    /// the current `active_step`) plus that agent profile's runtime. Both `None`
    /// for an orchestrator-level event emitted before any agent step is active.
    fn resolve_actor_ctx(&self, event: &HistoryEvent) -> ActorCtx {
        let agent = event
            .step_id
            .as_ref()
            .and_then(|step_id| {
                self.active_steps
                    .iter()
                    .chain(self.active_step.iter())
                    .find(|step| &step.step_id == step_id)
                    .map(|step| step.agent.clone())
            })
            .or_else(|| self.active_step.as_ref().map(|step| step.agent.clone()));
        let Some(agent) = agent else {
            return ActorCtx::default();
        };
        let runtime = self
            .agent(&agent)
            .ok()
            .map(|profile| profile.runtime.clone());
        ActorCtx {
            agent: Some(agent),
            runtime,
        }
    }

    /// Record a `hook_started`/`hook_completed` lifecycle event sent back from the
    /// dispatcher (ADR-003). This is the only `&mut App` site for hook events;
    /// the dispatcher never calls `record_event` directly. The event kind is
    /// outside the public vocabulary, so the tap above ignores it (no re-trigger).
    pub(crate) fn record_hook_lifecycle(&mut self, record: HookLifecycleRecord) -> Result<()> {
        let display = hook_lifecycle_display(&record);
        let payload = serde_json::to_value(&record.payload)?;
        self.record_event_with_group(
            record.run_id,
            None,
            record.step_id,
            record.kind.event_kind(),
            payload,
            display,
        )
    }

    fn record_parallel_group_rejected_if_present(
        &mut self,
        run_id: &str,
        decision: &crate::orchestrator::OrchestratorDecision,
        reason: &str,
    ) -> Result<()> {
        let Some(DecisionNextStep::ParallelGroup(group)) = decision.next_step.as_ref() else {
            return Ok(());
        };
        self.record_event_with_group(
            Some(run_id.to_string()),
            Some(group.group_id.clone()),
            None,
            "parallel_group_rejected",
            json!({
                "group_id": group.group_id,
                "decision_id": decision.decision_id,
                "reason": reason,
                "children": group.steps.len()
            }),
            "Parallel group rejected.",
        )
    }

    fn sync_chat_items(&mut self) {
        self.chat_projection
            .apply_live_steps(&self.state.live_steps);
        self.chat_projection.apply_pending_approval(
            self.state.pending_approval.as_ref(),
            self.state.show_first_approval_explainer,
        );
        // The welcome item is prepended (not part of the projection) so it stays
        // first and stable across re-syncs (ADR-005).
        let mut items = Vec::with_capacity(self.chat_projection.items().len() + 1);
        items.push(ChatItemView::welcome());
        items.extend(self.chat_projection.items().iter().cloned());
        self.state.chat_items = items;
    }

    fn sync_queued_follow_ups(&mut self) {
        self.state.queued_follow_ups = self
            .follow_up_queue
            .iter()
            .map(QueuedFollowUp::to_view)
            .collect();
    }

    fn publish_state(&mut self) {
        // Rebuild the roster view-model centrally so rows never drift from
        // `agents`/`live_steps` and the renderer stays a pure, clock-free
        // function of `AppState` (ADR-003).
        self.rebuild_roster_rows();
        self.send_state();
    }

    /// Send the current `AppState` snapshot over the watch channel **without**
    /// rebuilding the roster. Used by change-gated publishers (e.g.
    /// `refresh_roster_tick`) that have already prepared `roster_rows`.
    fn send_state(&self) {
        if let Some(sender) = &self.state_sender {
            let _ = sender.send(self.state.clone());
        }
    }

    /// Bounded 1 Hz roster refresh (ADR-004). Rebuilds the roster view-model
    /// with a fresh clock so coarse elapsed advances and a quiet step surfaces
    /// as `Stalled` even when no stream events arrive, then publishes **only**
    /// when the rebuilt rows differ — the change gate mirrors `set_git_context`.
    /// A no-op unless a run is actively working. Returns whether it published.
    pub(crate) fn refresh_roster_tick(&mut self) -> bool {
        // Gate to active runs (keep in sync with `tui::work_indicator_active`):
        // an idle roster never changes, so ticking it would only churn the watch.
        if !matches!(self.state.run_state, RunState::Planning | RunState::Running) {
            return false;
        }
        let rows = build_roster_rows(
            &self.state.agents,
            &self.state.live_steps,
            &self.step_timings,
            Instant::now(),
        );
        // Rows pre-format elapsed into coarse buckets and carry activity state,
        // so an unchanged vec means neither the bucket nor the activity moved
        // (ADR-004 change gate) — suppress the publish.
        if self.state.roster_rows == rows {
            return false;
        }
        self.state.roster_rows = rows;
        self.send_state();
        true
    }

    /// Rebuild `AppState.roster_rows` from the current agents, live steps, and
    /// step timing using a fresh wall-clock (ADR-003). Called inside
    /// `publish_state`; the injected `now` keeps the builder itself testable.
    fn rebuild_roster_rows(&mut self) {
        let rows = build_roster_rows(
            &self.state.agents,
            &self.state.live_steps,
            &self.step_timings,
            Instant::now(),
        );
        self.state.roster_rows = rows;
    }

    /// Apply a freshly fetched git context, publishing a state update only when
    /// the value changed (ADR-006 change gate). Returns whether it published.
    fn set_git_context(&mut self, context: Option<GitContext>) -> bool {
        if self.state.git_context == context {
            return false;
        }
        self.state.git_context = context;
        self.publish_state();
        true
    }

    /// Fetch the working directory's git context and apply it through the change
    /// gate. Driven by the startup refresh, the 5s poll, and prompt submission.
    pub(crate) async fn refresh_git_context(&mut self) {
        let context = fetch_git_context(&self.config.working_directory).await;
        self.set_git_context(context);
    }

    fn record_action_completed(
        &mut self,
        run_id: &str,
        step_id: &str,
        runtime: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        self.record_action_completed_with_group(run_id, None, step_id, runtime, request, result)
    }

    fn record_action_completed_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        runtime: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        let durable_result =
            self.action_result_for_history_with_group(run_id, group_id, step_id, request, result)?;
        if matches!(durable_result.status, ActionStatus::Denied) {
            self.record_event_with_group(
                Some(run_id.to_string()),
                group_id.map(str::to_string),
                Some(step_id.to_string()),
                "action_denied",
                serde_json::to_value(&durable_result)?,
                action_denied_display(request, result),
            )?;
        }
        self.record_event_with_group(
            Some(run_id.to_string()),
            group_id.map(str::to_string),
            Some(step_id.to_string()),
            "action_completed",
            serde_json::to_value(&durable_result)?,
            action_completed_display(request, result),
        )?;
        // Observability event for MCP tool calls (task_10): {server, tool,
        // runtime, status, trusted}. Feeds the `--doctor` parity matrix and
        // trusted-completion metric; never rendered as chat (no-op projection
        // arm). Local-only, no telemetry.
        if request.kind == ActionKind::CallMcpTool {
            let server = request
                .params
                .get("server")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let tool = request
                .params
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let trusted = self.mcp_trust.is_trusted(server);
            let status_label = match durable_result.status {
                ActionStatus::Completed => "completed",
                ActionStatus::Denied => "denied",
                ActionStatus::ApprovalRequired => "approval_required",
                ActionStatus::Failed => "failed",
            };
            self.record_event_with_group(
                Some(run_id.to_string()),
                group_id.map(str::to_string),
                Some(step_id.to_string()),
                "mcp_tool_result",
                json!({
                    "server": server,
                    "tool": tool,
                    "runtime": runtime,
                    "status": status_label,
                    "trusted": trusted,
                }),
                format!("MCP tool {server}/{tool} {status_label}."),
            )?;
        }
        Ok(())
    }

    fn record_command_started_if_executable(
        &mut self,
        run_id: &str,
        step_id: &str,
        agent_profile: &AgentProfile,
        context: &ActionExecutionContext,
        request: &ActionRequest,
    ) -> Result<()> {
        self.record_command_started_if_executable_with_group(
            run_id,
            None,
            step_id,
            agent_profile,
            context,
            request,
        )
    }

    fn record_command_started_if_executable_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        agent_profile: &AgentProfile,
        context: &ActionExecutionContext,
        request: &ActionRequest,
    ) -> Result<()> {
        if !matches!(request.kind, ActionKind::RunCommand)
            || !action_executable_without_approval(agent_profile, context, request)
        {
            return Ok(());
        }

        let command = request
            .params
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        self.record_event_with_group(
            Some(run_id.to_string()),
            group_id.map(str::to_string),
            Some(step_id.to_string()),
            "command_started",
            json!({
                "action_id": request.action_id.clone(),
                "command": command
            }),
            format!("Command started: {command}"),
        )
    }

    fn record_runtime_stream_deltas(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        agent_id: &str,
        deltas: &[RuntimeStreamDelta],
    ) -> Result<()> {
        let mut coalescer = RuntimeStreamCoalescer::new(agent_id.to_string());
        for delta in deltas {
            self.push_live_stream_delta(step_id, delta);
            if delta.transient {
                continue;
            }
            coalescer.push_delta(delta);
            if coalescer.should_flush() {
                self.flush_runtime_stream_coalescer(run, step_id, &mut coalescer, false)?;
            }
        }
        self.flush_runtime_stream_coalescer(run, step_id, &mut coalescer, true)?;
        Ok(())
    }

    fn record_runtime_event(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        coalescer: &mut RuntimeStreamCoalescer,
        event: RuntimeEvent,
    ) -> Result<()> {
        self.push_live_runtime_event(step_id, &event);
        if event.is_transient() {
            return Ok(());
        }
        coalescer.push_event(event);
        if coalescer.should_flush() {
            self.flush_runtime_stream_coalescer(run, step_id, coalescer, false)?;
        }
        Ok(())
    }

    fn record_parallel_runtime_event(
        &mut self,
        run: &RunDriveContext,
        group_id: &str,
        step_id: &str,
        coalescer: &mut RuntimeStreamCoalescer,
        event: RuntimeEvent,
    ) -> Result<()> {
        self.push_live_runtime_event(step_id, &event);
        if event.is_transient() {
            return Ok(());
        }
        coalescer.push_event(event);
        if coalescer.should_flush() {
            self.flush_runtime_stream_coalescer_with_group(
                run,
                Some(group_id),
                step_id,
                coalescer,
                false,
            )?;
        }
        Ok(())
    }

    fn finish_streaming_interrupt(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        runtime_stop: std::result::Result<Result<RuntimeOutput>, tokio::time::error::Elapsed>,
    ) -> Result<()> {
        self.state.run_state = RunState::Interrupted;
        match runtime_stop {
            Ok(_) => self.record_step_cancelled_if_active()?,
            Err(error) => self.record_step_cancel_failed_if_active(&error.to_string())?,
        }
        self.record_event(
            Some(run.run_id.clone()),
            None,
            "run_interrupted",
            json!({}),
            "Run interrupted.",
        )?;
        self.state.active_run_id = None;
        self.state.pending_approval = None;
        // Keep the explainer flag in sync with approval presence (none pending → false).
        self.state.show_first_approval_explainer = false;
        self.state.pending_clarification = None;
        self.pending_approval = None;
        self.pending_clarification = None;
        if self
            .active_step
            .as_ref()
            .is_some_and(|step| step.step_id == step_id)
        {
            self.active_steps.retain(|step| step.step_id != step_id);
            self.active_step = self.active_steps.first().cloned();
        }
        self.sync_chat_items();
        self.publish_state();
        Ok(())
    }

    fn flush_runtime_stream_coalescer(
        &mut self,
        run: &RunDriveContext,
        step_id: &str,
        coalescer: &mut RuntimeStreamCoalescer,
        final_delta: bool,
    ) -> Result<()> {
        self.flush_runtime_stream_coalescer_with_group(run, None, step_id, coalescer, final_delta)
    }

    fn flush_runtime_stream_coalescer_with_group(
        &mut self,
        run: &RunDriveContext,
        group_id: Option<&str>,
        step_id: &str,
        coalescer: &mut RuntimeStreamCoalescer,
        final_delta: bool,
    ) -> Result<()> {
        let records = coalescer.flush(final_delta);
        for record in records {
            let stream = record.stream.clone();
            let payload = self.runtime_stream_record_payload(&record)?;
            self.record_event_with_group(
                Some(run.run_id.clone()),
                group_id.map(str::to_string),
                Some(step_id.to_string()),
                "runtime_stream_delta",
                payload,
                format!("Runtime stream: {stream}"),
            )?;
        }
        Ok(())
    }

    fn push_live_runtime_event(&mut self, step_id: &str, event: &RuntimeEvent) {
        self.push_live_stream_content(
            step_id,
            event.stream_name(),
            event.content(),
            event.sequence(),
            false,
        );
    }

    fn push_live_stream_delta(&mut self, step_id: &str, delta: &RuntimeStreamDelta) {
        self.push_live_stream_content(
            step_id,
            delta.stream.clone(),
            delta.content.clone(),
            delta.sequence,
            delta.final_delta,
        );
    }

    fn push_live_stream_content(
        &mut self,
        step_id: &str,
        stream: String,
        content: String,
        sequence: u32,
        final_delta: bool,
    ) {
        // Bump activity at the single stream chokepoint (ADR-004) before any
        // other processing, so stall detection sees the freshest signal even if
        // the step has no matching live view.
        if let Some(timing) = self.step_timings.get_mut(step_id) {
            timing.last_activity = Instant::now();
        }
        let Some(live_step) = self
            .state
            .live_steps
            .iter_mut()
            .find(|live_step| live_step.step_id == step_id)
        else {
            return;
        };
        live_step.status = LiveStepStatus::Streaming;
        if let Some(existing) = live_step
            .streams
            .iter_mut()
            .find(|existing| existing.stream == stream)
        {
            existing.content.push_str(&content);
            trim_live_stream_content(&mut existing.content);
            existing.sequence_end = sequence;
            existing.final_delta = final_delta;
        } else {
            let mut content = content;
            trim_live_stream_content(&mut content);
            live_step.streams.push(LiveStreamView {
                stream,
                content,
                sequence_end: sequence,
                final_delta,
            });
        }
        let agent = live_step.agent.clone();
        self.state.live_step = self.state.live_steps.first().cloned();
        self.sync_chat_items();
        self.set_agent_status(&agent, "running");
    }

    fn set_live_step_status(&mut self, step_id: &str, status: LiveStepStatus) {
        let Some(live_step) = self
            .state
            .live_steps
            .iter_mut()
            .find(|live_step| live_step.step_id == step_id)
        else {
            return;
        };
        let final_delta = matches!(
            status,
            LiveStepStatus::Completed | LiveStepStatus::Interrupted | LiveStepStatus::Failed
        );
        // Bump activity on transitions into the active states where stall
        // detection matters (ADR-004); terminal/waiting transitions do not.
        if matches!(status, LiveStepStatus::Running | LiveStepStatus::Streaming) {
            if let Some(timing) = self.step_timings.get_mut(step_id) {
                timing.last_activity = Instant::now();
            }
        }
        live_step.status = status;
        if final_delta {
            for stream in &mut live_step.streams {
                stream.final_delta = true;
            }
        }
        if final_delta {
            // A finished step no longer needs timing: drop it so the agent
            // classifies as `Idle` and the map doesn't leak entries (ADR-004).
            self.step_timings.remove(step_id);
        }
        self.state.live_step = self.state.live_steps.first().cloned();
        self.sync_chat_items();
        self.publish_state();
    }

    fn runtime_stream_record_payload(
        &mut self,
        record: &RuntimeStreamRecord,
    ) -> Result<serde_json::Value> {
        let mut payload = json!({
            "agent": record.agent,
            "sequence_start": record.sequence_start,
            "sequence_end": record.sequence_end,
            "stream": record.stream,
            "final_delta": record.final_delta,
            "coalesced": true,
            "content": record.content
        });
        let bytes = record.content.as_bytes();
        if bytes.len() <= LARGE_ACTION_CONTENT_BYTES {
            return Ok(payload);
        }

        let artifact =
            self.history
                .write_artifact("txt", "text/plain", bytes, "contains_user_content")?;
        payload["content"] = serde_json::Value::Null;
        payload["artifact"] = serde_json::to_value(artifact)?;
        Ok(payload)
    }

    fn record_action_specific_events(
        &mut self,
        run_id: &str,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        self.record_action_specific_events_with_group(run_id, None, step_id, request, result)
    }

    fn record_action_specific_events_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        self.record_gate_outcome_event_with_group(run_id, group_id, step_id, request, result)?;
        match request.kind {
            ActionKind::RunCommand => {
                self.record_command_completed_with_group(run_id, group_id, step_id, request, result)
            }
            ActionKind::ApplyPatch | ActionKind::WriteFile => {
                self.record_file_edit_applied_with_group(run_id, group_id, step_id, request, result)
            }
            _ => Ok(()),
        }
    }

    /// Audit the floor/trust gate outcome stamped by the enforcement point
    /// (ADR-002/003): a trust auto-approval emits `approval_auto_resolved`; a
    /// gray-area warn-and-run under Yolo+Warn emits `floor_warned`.
    fn record_gate_outcome_event_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        match result.gate_outcome {
            GateOutcome::AutoApprovedByTrust => {
                let target = result.risk.as_ref().and_then(|risk| risk.target.as_ref());
                self.record_event_with_group(
                    Some(run_id.to_string()),
                    group_id.map(str::to_string),
                    Some(step_id.to_string()),
                    "approval_auto_resolved",
                    json!({
                        "action_id": request.action_id.clone(),
                        "target": target.map(trust_target_payload),
                    }),
                    "Auto-approved by session trust.",
                )
            }
            GateOutcome::WarnedAllowed => {
                let (tier, reason) = result
                    .risk
                    .as_ref()
                    .map(|risk| (risk_tier_label(risk.tier), risk.reason.clone()))
                    .unwrap_or(("medium", "gray-area action".to_string()));
                self.record_event_with_group(
                    Some(run_id.to_string()),
                    group_id.map(str::to_string),
                    Some(step_id.to_string()),
                    "floor_warned",
                    json!({
                        "action_id": request.action_id.clone(),
                        "tier": tier,
                        "reason": reason,
                    }),
                    format!("Floor warning ({tier}): {reason}"),
                )
            }
            GateOutcome::Normal | GateOutcome::ApprovalRequired => Ok(()),
        }
    }

    fn record_command_completed_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        if matches!(
            result.status,
            ActionStatus::ApprovalRequired | ActionStatus::Denied
        ) {
            return Ok(());
        }

        let command = result
            .content
            .as_ref()
            .and_then(|content| content.get("command"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                request
                    .params
                    .get("command")
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or("");
        let exit_code = result
            .content
            .as_ref()
            .and_then(|content| content.get("exit_code"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let display = command_completed_display(command, result, &exit_code);
        self.record_event_with_group(
            Some(run_id.to_string()),
            group_id.map(str::to_string),
            Some(step_id.to_string()),
            "command_completed",
            json!({
                "action_id": request.action_id.clone(),
                "command": command,
                "status": result.status.clone(),
                "exit_code": exit_code,
                "diagnostic": result.diagnostic.clone()
            }),
            display,
        )
    }

    fn record_file_edit_applied_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        if !matches!(result.status, ActionStatus::Completed) {
            return Ok(());
        }

        let (payload, display) = match request.kind {
            ActionKind::WriteFile => {
                let path = result
                    .content
                    .as_ref()
                    .and_then(|content| content.get("path"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let bytes = result
                    .content
                    .as_ref()
                    .and_then(|content| content.get("bytes"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let payload = json!({
                    "action_id": request.action_id.clone(),
                    "operation": "write_file",
                    "path": path,
                    "bytes": bytes
                });
                let display = file_created_display(&payload);
                (payload, display)
            }
            ActionKind::ApplyPatch => {
                let changed_files = result
                    .content
                    .as_ref()
                    .and_then(|content| content.get("changed_files"))
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let payload = json!({
                    "action_id": request.action_id.clone(),
                    "operation": "apply_patch",
                    "changed_files": changed_files
                });
                let display = files_edited_display(&payload);
                (payload, display)
            }
            _ => return Ok(()),
        };

        self.record_event_with_group(
            Some(run_id.to_string()),
            group_id.map(str::to_string),
            Some(step_id.to_string()),
            "file_edit_applied",
            payload,
            display,
        )
    }

    fn action_result_for_history_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<ActionResult> {
        let mut durable_result = result.clone();
        let Some(content) = &result.content else {
            return Ok(durable_result);
        };
        let bytes = serde_json::to_vec_pretty(content)?;
        if bytes.len() <= LARGE_ACTION_CONTENT_BYTES {
            return Ok(durable_result);
        }

        let artifact = self.history.write_artifact(
            "json",
            "application/json",
            &bytes,
            "contains_user_content",
        )?;
        self.record_event_with_group(
            Some(run_id.to_string()),
            group_id.map(str::to_string),
            Some(step_id.to_string()),
            "artifact_written",
            serde_json::to_value(&artifact)?,
            format!(
                "Large output for {} stored as an artifact.",
                action_target_display(request)
            ),
        )?;
        durable_result.content = action_content_preview_for_history(request, content);
        durable_result.artifact = Some(serde_json::to_value(artifact)?);
        Ok(durable_result)
    }
}

fn action_content_preview_for_history(request: &ActionRequest, content: &Value) -> Option<Value> {
    match request.kind {
        ActionKind::SearchText => search_text_content_preview(content),
        _ => None,
    }
}

fn search_text_content_preview(content: &Value) -> Option<Value> {
    let matches = content.get("matches").and_then(Value::as_array)?;
    let preview = matches
        .iter()
        .take(SEARCH_TEXT_HISTORY_PREVIEW_MATCHES)
        .cloned()
        .collect::<Vec<_>>();
    Some(json!({
        "query": content.get("query").cloned().unwrap_or(Value::Null),
        "path": content.get("path").cloned().unwrap_or(Value::Null),
        "matches": preview,
        "total_matches": matches.len(),
        "truncated": matches.len() > SEARCH_TEXT_HISTORY_PREVIEW_MATCHES
    }))
}

#[derive(Clone, Debug)]
struct RuntimeStreamRecord {
    agent: String,
    sequence_start: u32,
    sequence_end: u32,
    stream: String,
    content: String,
    final_delta: bool,
}

#[derive(Clone, Debug)]
struct RuntimeStreamCoalescer {
    agent: String,
    buffers: BTreeMap<String, StreamBuffer>,
    last_flush_at: Instant,
}

#[derive(Clone, Debug)]
struct StreamBuffer {
    sequence_start: u32,
    sequence_end: u32,
    content: String,
}

impl RuntimeStreamCoalescer {
    fn new(agent: String) -> Self {
        Self {
            agent,
            buffers: BTreeMap::new(),
            last_flush_at: Instant::now(),
        }
    }

    fn push_event(&mut self, event: RuntimeEvent) {
        self.push(event.stream_name(), event.sequence(), event.content());
    }

    fn push_delta(&mut self, delta: &RuntimeStreamDelta) {
        self.push(delta.stream.clone(), delta.sequence, delta.content.clone());
    }

    fn push(&mut self, stream: String, sequence: u32, content: String) {
        self.buffers
            .entry(stream)
            .and_modify(|buffer| {
                buffer.sequence_end = sequence;
                buffer.content.push_str(&content);
            })
            .or_insert_with(|| StreamBuffer {
                sequence_start: sequence,
                sequence_end: sequence,
                content,
            });
    }

    fn should_flush(&self) -> bool {
        self.buffers
            .values()
            .any(|buffer| buffer.content.len() >= STREAM_COALESCE_BYTES)
            || (!self.buffers.is_empty()
                && self.last_flush_at.elapsed() >= STREAM_COALESCE_INTERVAL)
    }

    fn flush(&mut self, final_delta: bool) -> Vec<RuntimeStreamRecord> {
        if self.buffers.is_empty() {
            return Vec::new();
        }
        self.last_flush_at = Instant::now();
        std::mem::take(&mut self.buffers)
            .into_iter()
            .map(|(stream, buffer)| RuntimeStreamRecord {
                agent: self.agent.clone(),
                sequence_start: buffer.sequence_start,
                sequence_end: buffer.sequence_end,
                stream,
                content: buffer.content,
                final_delta,
            })
            .collect()
    }
}

fn trim_live_stream_content(content: &mut String) {
    if content.len() <= LIVE_STREAM_CONTENT_LIMIT {
        return;
    }
    let keep_from = content.len().saturating_sub(LIVE_STREAM_CONTENT_LIMIT);
    let keep_from = content
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= keep_from)
        .unwrap_or(content.len());
    content.drain(..keep_from);
}

fn runtime_history_event(event: &HistoryEvent) -> RuntimeHistoryEvent {
    let (payload, payload_truncated) = compact_history_payload(&event.kind, &event.payload);
    RuntimeHistoryEvent {
        schema_version: event.schema_version,
        event_id: event.event_id.clone(),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        group_id: event.group_id.clone(),
        step_id: event.step_id.clone(),
        timestamp: event.timestamp.clone(),
        kind: event.kind.clone(),
        payload,
        payload_truncated: event.payload_truncated || payload_truncated,
    }
}

fn runtime_recent_context(events: &[HistoryEvent]) -> RuntimeRecentContext {
    let mut files = Vec::new();
    let mut actions = Vec::new();
    for event in events.iter().rev() {
        if files.len() < RUNTIME_RECENT_CONTEXT_LIMIT {
            files.extend(recent_files_from_event(event));
            files.truncate(RUNTIME_RECENT_CONTEXT_LIMIT);
        }
        if actions.len() < RUNTIME_RECENT_CONTEXT_LIMIT {
            if let Some(action) = recent_action_from_event(event) {
                actions.push(action);
            }
        }
        if files.len() >= RUNTIME_RECENT_CONTEXT_LIMIT
            && actions.len() >= RUNTIME_RECENT_CONTEXT_LIMIT
        {
            break;
        }
    }
    files.reverse();
    actions.reverse();
    RuntimeRecentContext { files, actions }
}

fn recent_files_from_event(event: &HistoryEvent) -> Vec<RuntimeRecentFile> {
    if event.kind != "file_edit_applied" {
        return Vec::new();
    }
    let operation = event
        .payload
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("file_event")
        .to_string();
    if let Some(path) = event.payload.get("path").and_then(Value::as_str) {
        return vec![RuntimeRecentFile {
            path: path.to_string(),
            operation,
            event_id: event.event_id.clone(),
        }];
    }
    event
        .payload
        .get("changed_files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|path| RuntimeRecentFile {
            path: path.to_string(),
            operation: operation.clone(),
            event_id: event.event_id.clone(),
        })
        .collect()
}

fn recent_action_from_event(event: &HistoryEvent) -> Option<RuntimeRecentAction> {
    match event.kind.as_str() {
        "action_requested" | "action_completed" | "action_denied" => Some(RuntimeRecentAction {
            event_kind: event.kind.clone(),
            action_id: event
                .payload
                .get("action_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            action_kind: event
                .payload
                .get("kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            status: event
                .payload
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
            summary: event
                .payload
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string),
            event_id: event.event_id.clone(),
        }),
        _ => None,
    }
}

fn compact_history_payload(kind: &str, payload: &Value) -> (Value, bool) {
    if kind == "runtime_stream_delta" {
        return compact_runtime_stream_delta_payload(payload);
    }
    compact_json_value(payload, RUNTIME_HISTORY_PAYLOAD_DEPTH)
}

fn compact_runtime_stream_delta_payload(payload: &Value) -> (Value, bool) {
    let Some(object) = payload.as_object() else {
        return compact_json_value(payload, RUNTIME_HISTORY_PAYLOAD_DEPTH);
    };

    let mut compacted = Map::new();
    for key in [
        "agent",
        "sequence",
        "sequence_start",
        "sequence_end",
        "stream",
        "final_delta",
        "coalesced",
        "artifact",
    ] {
        if let Some(value) = object.get(key) {
            compacted.insert(key.to_string(), value.clone());
        }
    }

    let mut truncated = false;
    if let Some(content) = object.get("content") {
        let (content, content_truncated) = compact_json_value(content, 1);
        compacted.insert("content".to_string(), content);
        truncated |= content_truncated;
    }
    if object.len() > compacted.len() {
        truncated = true;
    }

    (Value::Object(compacted), truncated)
}

fn compact_json_value(value: &Value, depth: usize) -> (Value, bool) {
    match value {
        Value::String(value) => compact_string_value(value),
        Value::Array(values) => compact_json_array(values, depth),
        Value::Object(object) => compact_json_object(object, depth),
        value => (value.clone(), false),
    }
}

fn compact_json_array(values: &[Value], depth: usize) -> (Value, bool) {
    if depth == 0 {
        return (
            json!({
                "type": "array",
                "item_count": values.len()
            }),
            !values.is_empty(),
        );
    }

    let mut truncated = values.len() > RUNTIME_HISTORY_PAYLOAD_ITEMS;
    let mut compacted = values
        .iter()
        .take(RUNTIME_HISTORY_PAYLOAD_ITEMS)
        .map(|value| {
            let (value, value_truncated) = compact_json_value(value, depth - 1);
            truncated |= value_truncated;
            value
        })
        .collect::<Vec<_>>();
    if values.len() > RUNTIME_HISTORY_PAYLOAD_ITEMS {
        compacted.push(json!({
            "type": "truncated_items",
            "omitted": values.len() - RUNTIME_HISTORY_PAYLOAD_ITEMS
        }));
    }

    (Value::Array(compacted), truncated)
}

fn compact_json_object(object: &Map<String, Value>, depth: usize) -> (Value, bool) {
    if depth == 0 {
        return (
            json!({
                "type": "object",
                "field_count": object.len(),
                "keys": object.keys().take(RUNTIME_HISTORY_PAYLOAD_FIELDS).collect::<Vec<_>>()
            }),
            !object.is_empty(),
        );
    }

    let mut truncated = object.len() > RUNTIME_HISTORY_PAYLOAD_FIELDS;
    let mut compacted = Map::new();
    for (key, value) in object.iter().take(RUNTIME_HISTORY_PAYLOAD_FIELDS) {
        let (value, value_truncated) = compact_json_value(value, depth - 1);
        compacted.insert(key.clone(), value);
        truncated |= value_truncated;
    }
    if object.len() > RUNTIME_HISTORY_PAYLOAD_FIELDS {
        compacted.insert(
            "_truncated_fields".to_string(),
            json!(object.len() - RUNTIME_HISTORY_PAYLOAD_FIELDS),
        );
    }

    (Value::Object(compacted), truncated)
}

fn compact_string_value(value: &str) -> (Value, bool) {
    let chars = value.chars().count();
    if chars <= RUNTIME_HISTORY_STRING_CHARS {
        return (Value::String(value.to_string()), false);
    }

    (
        json!({
            "type": "string",
            "chars": chars,
            "preview": value.chars().take(RUNTIME_HISTORY_STRING_CHARS).collect::<String>()
        }),
        true,
    )
}

fn build_agent_views(
    config: &EffectiveConfig,
    availability: &BTreeMap<String, RuntimeAvailability>,
) -> Vec<AgentView> {
    let mut views = config
        .agents
        .values()
        .map(|agent| AgentView {
            id: agent.id.clone(),
            name: agent.name.clone(),
            runtime: agent.runtime.clone(),
            model: agent.model.clone(),
            effort: format!("{:?}", agent.effort).to_ascii_lowercase(),
            thinking: agent.thinking,
            capabilities: agent
                .capabilities
                .iter()
                .map(|capability| format!("{capability:?}").to_ascii_lowercase())
                .collect(),
            availability: availability.get(&agent.runtime).cloned(),
            status: if agent.enabled {
                "idle".to_string()
            } else {
                "disabled".to_string()
            },
        })
        .collect::<Vec<_>>();

    views.sort_by(|left, right| {
        (agent_roster_rank(&left.id), left.id.as_str())
            .cmp(&(agent_roster_rank(&right.id), right.id.as_str()))
    });
    views
}

/// Format a coarse, whole-second elapsed duration for an active roster row
/// (ADR-002/ADR-004). Sub-minute renders as `"8s"`, minutes as `"1m 20s"`
/// (the seconds remainder is dropped when zero, e.g. `"1m"`), and hours as
/// `"1h 5m"`. The returned string is pre-formatted into [`RosterRow::elapsed`]
/// so the renderer never touches a clock.
fn format_coarse_elapsed(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    if total < 60 {
        return format!("{total}s");
    }
    if total < 3600 {
        let minutes = total / 60;
        let seconds = total % 60;
        if seconds == 0 {
            return format!("{minutes}m");
        }
        return format!("{minutes}m {seconds}s");
    }
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    if minutes == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {minutes}m")
    }
}

/// Pure roster builder (ADR-003): joins canonical agents with their live steps,
/// classifies each row's [`ActivityState`], assigns a canonical-order
/// `accent_index` immune to the pin-sort (ADR-005), pre-formats elapsed on
/// active rows, and floats `NeedsInput` rows to the top via a stable pin-sort.
///
/// `agents` must already be in canonical order (orchestrator-first then
/// alphabetical, as [`build_agent_views`] yields); `accent_index` is taken from
/// that order *before* the pin so reordering never recolors an agent. `now` is
/// injected for determinism — production passes `Instant::now()`, tests a fixed
/// `Instant`.
pub(crate) fn build_roster_rows(
    agents: &[AgentView],
    live_steps: &[LiveStepView],
    timing: &BTreeMap<String, StepTiming>,
    now: Instant,
) -> Vec<RosterRow> {
    let mut rows: Vec<(usize, RosterRow)> = agents
        .iter()
        .enumerate()
        .map(|(accent_index, agent)| {
            // Join: an agent's live step is the one whose `agent` matches its id.
            let live_step = live_steps.iter().find(|step| step.agent == agent.id);

            let (activity, current_step, elapsed) = match live_step {
                Some(step) => classify_step(step, timing, now),
                None => (ActivityState::Idle, None, None),
            };

            let row = RosterRow {
                agent_id: agent.id.clone(),
                name: agent.name.clone(),
                accent_index,
                activity,
                runtime_model: format!("{}/{}", agent.runtime, agent.model),
                effort: agent.effort.clone(),
                thinking: agent.thinking,
                current_step,
                elapsed,
                // Terminal/idle rows keep their existing status label; only the
                // activity-driven states carry their own meaning via `activity`.
                status: agent.status.clone(),
            };
            (accent_index, row)
        })
        .collect();

    // The single permitted reorder: stable pin-sort floating `NeedsInput` rows
    // to the top (pin_rank 0) while every other row keeps its canonical order
    // (pin_rank 1). `accent_index` was assigned above, so the pin never moves a
    // color. The secondary key preserves canonical order within each rank.
    rows.sort_by_key(|(canonical_order, row)| {
        let pin_rank = if row.activity == ActivityState::NeedsInput {
            0
        } else {
            1
        };
        (pin_rank, *canonical_order)
    });

    rows.into_iter().map(|(_, row)| row).collect()
}

/// Classify a joined live step into an [`ActivityState`] plus the pre-formatted
/// `current_step`/`elapsed` shown on active rows. Terminal statuses collapse to
/// `Idle` with no step/elapsed; their label survives on `RosterRow::status`.
fn classify_step(
    step: &LiveStepView,
    timing: &BTreeMap<String, StepTiming>,
    now: Instant,
) -> (ActivityState, Option<String>, Option<String>) {
    match step.status {
        LiveStepStatus::WaitingForApproval | LiveStepStatus::WaitingForAction => {
            (ActivityState::NeedsInput, None, None)
        }
        LiveStepStatus::Starting
        | LiveStepStatus::Running
        | LiveStepStatus::Streaming
        | LiveStepStatus::Cancelling => {
            let entry = timing.get(&step.step_id);
            let stalled = entry
                .is_some_and(|t| now.saturating_duration_since(t.last_activity) >= STALL_THRESHOLD);
            let activity = if stalled {
                ActivityState::Stalled
            } else {
                ActivityState::Active
            };
            let elapsed =
                entry.map(|t| format_coarse_elapsed(now.saturating_duration_since(t.started_at)));
            let current_step = Some(step_display_label(step));
            (activity, current_step, elapsed)
        }
        // Terminal statuses: Idle, label preserved from `agent.status`.
        LiveStepStatus::Completed | LiveStepStatus::Failed | LiveStepStatus::Interrupted => {
            (ActivityState::Idle, None, None)
        }
    }
}

/// Resolve the step label shown on an active row, falling back to the step id
/// when the label is absent.
fn step_display_label(step: &LiveStepView) -> String {
    step.step_label
        .clone()
        .unwrap_or_else(|| step.step_id.clone())
}

fn build_config_status(
    config: &EffectiveConfig,
    availability: &BTreeMap<String, RuntimeAvailability>,
) -> ConfigStatusView {
    let sources = if config.config_sources.is_empty() {
        vec!["built-in defaults".to_string()]
    } else {
        config
            .config_sources
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    };
    let mut warnings = config_warning_messages(config);
    warnings.extend(runtime_warning_messages(availability));
    let preset = config.active_preset.clone();
    let preset_label = preset.as_deref().unwrap_or("none");
    let summary = format!(
        "Config: sources={} preset={} warnings={}",
        sources.len(),
        preset_label,
        warnings.len()
    );
    ConfigStatusView {
        summary,
        sources,
        preset,
        warnings,
        approval_mode: config.approval_mode.clone(),
        execution_graph_enabled: config.features.execution_graph,
        max_parallel_agent_steps: config.limits.max_parallel_agent_steps,
    }
}

fn config_warning_messages(config: &EffectiveConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    let agents_without_fallbacks = config
        .agents
        .values()
        .filter(|agent| agent.enabled && agent.model_fallbacks.is_empty())
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    if !agents_without_fallbacks.is_empty() {
        warnings.push(format!(
            "enabled agents without model_fallbacks: {}",
            agents_without_fallbacks.join(", ")
        ));
    }
    // Keybinding trust-boundary + soft-fail notes (ADR-004): surfaced at startup
    // and via /config alongside the model-fallback warning.
    warnings.extend(config.keybinding_warnings.iter().cloned());
    // Enabled-but-disabled-by-ceiling: the DAG flag is on but the reused
    // concurrency ceiling is 0, so no graph can ever run (ADR-005).
    if config.features.execution_graph && config.limits.max_parallel_agent_steps == 0 {
        warnings.push(
            "execution_graph is enabled but limits.max_parallel_agent_steps = 0 disables it"
                .to_string(),
        );
    }
    warnings
}

fn runtime_warning_messages(availability: &BTreeMap<String, RuntimeAvailability>) -> Vec<String> {
    availability
        .values()
        .filter_map(|runtime| match runtime.status {
            RuntimeAvailabilityStatus::Available => None,
            RuntimeAvailabilityStatus::Unavailable => Some(format!(
                "runtime {} unavailable: {}",
                runtime.runtime_id, runtime.message
            )),
            RuntimeAvailabilityStatus::Unknown => Some(format!(
                "runtime {} status unknown: {}",
                runtime.runtime_id, runtime.message
            )),
        })
        .collect()
}

fn agent_roster_rank(agent_id: &str) -> u8 {
    match agent_id {
        "orchestrator" => 0,
        _ => 1,
    }
}

fn limit_reached(limit: &Limit, count: u32) -> bool {
    limit.is_reached_by(count)
}

/// True when the governance early-abort should pause this decision (ADR-004),
/// independent of the feature flag (the caller checks the flag). It fires only on
/// the **first orchestrator turn** of a non-trivial single-agent write run:
///
/// - `status == Continue`,
/// - first orchestrator turn — `step_count == 1` (the orchestrator step bumps the
///   count to 1 before returning its decision) with no agent results yet,
/// - not a subtask,
/// - the next step is a `SingleAgent`,
/// - and the decision's `required_capabilities` carry a write-class tool.
///
/// Because `step_count` keeps climbing on each orchestrator turn, this is false
/// on the Accept / reject-redirect re-drive, so the gate never re-fires.
fn early_abort_triggers(
    run: &RunDriveContext,
    decision: &crate::orchestrator::OrchestratorDecision,
) -> bool {
    decision.status == DecisionStatus::Continue
        && run.step_count == 1
        && run.previous_results.is_empty()
        && run.subtask.is_none()
        && matches!(
            decision.normalized_next_step(),
            Ok(Some(DecisionNextStep::SingleAgent(_)))
        )
        && decision_requires_write(decision)
}

/// The complexity signal: the decision intends to write if its required
/// capabilities include a write-class tool. No `is_write()` helper exists on
/// `Capability`, so match `Edit`/`Command` directly.
fn decision_requires_write(decision: &crate::orchestrator::OrchestratorDecision) -> bool {
    decision
        .required_capabilities
        .iter()
        .any(|capability| matches!(capability, Capability::Edit | Capability::Command))
}

/// Plain-language risk label (words, never color) describing what the run may do.
fn early_abort_risk_label(decision: &crate::orchestrator::OrchestratorDecision) -> String {
    let edits = decision.required_capabilities.contains(&Capability::Edit);
    let commands = decision
        .required_capabilities
        .contains(&Capability::Command);
    match (edits, commands) {
        (true, true) => "This run may edit files and run commands in your workspace".to_string(),
        (true, false) => "This run intends to edit files in your workspace".to_string(),
        (false, true) => "This run may run commands in your workspace".to_string(),
        (false, false) => "This run may modify your workspace".to_string(),
    }
}

fn command_timeout(limit: &Limit) -> Option<Duration> {
    match limit {
        Limit::Value(minutes) => Some(Duration::from_secs(u64::from(*minutes) * 60)),
        Limit::Unlimited => None,
    }
}

fn council_member_agent(member_id: &str, member: &CouncilMemberProfile) -> AgentProfile {
    AgentProfile {
        id: format!("{COUNCIL_WORKFLOW_AGENT_ID}.{member_id}"),
        name: format!("Council {member_id}"),
        runtime: member.runtime.clone(),
        model: member.model.clone(),
        model_fallbacks: member.model_fallbacks.clone(),
        effort: member.effort.clone(),
        thinking: member.thinking,
        capabilities: vec![Capability::Read, Capability::Challenge, Capability::Review],
        tools: Some(Vec::new()),
        instructions: format!(
            "{}\n\nReturn only an agent_result JSON envelope. Do not request actions; council workflows collect opinions and recommendations only.",
            member.prompt.trim()
        ),
        orchestrator_description: None,
        prompt_metadata: AgentPromptMetadata::default(),
        enabled: true,
    }
}

fn council_member_prompt(
    user_prompt: &str,
    decision: &crate::orchestrator::OrchestratorDecision,
) -> String {
    format!(
        "Council review request:\n\nOriginal user prompt:\n{user_prompt}\n\nOrchestrator reason:\n{reason}\n\nCouncil stop condition:\n{stop_condition}\n\nReturn focused risks, dissent, and a recommended next action from your council role.",
        reason = decision.reason,
        stop_condition = decision.stop_condition
    )
}

fn council_route_allowed(prompt: &str, session_goal: Option<&str>) -> bool {
    let text = match session_goal {
        Some(goal) => format!("{prompt} {goal}").to_ascii_lowercase(),
        None => prompt.to_ascii_lowercase(),
    };
    [
        "council",
        "high-risk",
        "high risk",
        "architecture",
        "security",
        "data integrity",
        "difficult review",
        "privacy",
        "compliance",
        "migration",
        "rollback",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn synthesize_council_decision(
    decision: &crate::orchestrator::OrchestratorDecision,
    reports: &[CouncilMemberReport],
) -> CouncilDecisionEnvelope {
    let successful = reports
        .iter()
        .filter(|report| {
            matches!(
                report.status,
                AgentResultStatus::Completed | AgentResultStatus::NoChanges
            )
        })
        .collect::<Vec<_>>();
    let failed = reports
        .iter()
        .filter(|report| {
            !matches!(
                report.status,
                AgentResultStatus::Completed | AgentResultStatus::NoChanges
            )
        })
        .collect::<Vec<_>>();

    let confidence = if successful.is_empty() {
        "blocked"
    } else if failed.is_empty() {
        "high"
    } else {
        "partial"
    }
    .to_string();
    let recommended_action = successful
        .first()
        .map(|report| report.summary.clone())
        .unwrap_or_else(|| {
            "Do not proceed until council runtime failures are resolved.".to_string()
        });
    let risks = successful
        .iter()
        .map(|report| format!("{}: {}", report.member_id, report.summary))
        .chain(failed.iter().filter_map(|report| {
            report
                .diagnostic
                .as_ref()
                .map(|diagnostic| format!("{} failed: {diagnostic}", report.member_id))
        }))
        .collect();
    let dissent = failed
        .iter()
        .map(|report| {
            format!(
                "{} did not complete: {}",
                report.member_id,
                report
                    .diagnostic
                    .as_deref()
                    .unwrap_or(report.summary.as_str())
            )
        })
        .collect();

    CouncilDecisionEnvelope {
        schema_version: 1,
        confidence,
        dissent,
        risks,
        recommended_action,
        stop_condition: decision.stop_condition.clone(),
    }
}

fn council_agent_result(
    step_id: String,
    synthesis: &CouncilDecisionEnvelope,
    reports: &[CouncilMemberReport],
) -> AgentResult {
    let successful_count = reports
        .iter()
        .filter(|report| {
            matches!(
                report.status,
                AgentResultStatus::Completed | AgentResultStatus::NoChanges
            )
        })
        .count();
    let status = if successful_count == 0 {
        AgentResultStatus::Failed
    } else {
        AgentResultStatus::Completed
    };
    let mut findings = vec![
        format!("confidence: {}", synthesis.confidence),
        format!("recommended_action: {}", synthesis.recommended_action),
        format!("stop_condition: {}", synthesis.stop_condition),
    ];
    findings.extend(synthesis.risks.iter().map(|risk| format!("risk: {risk}")));
    findings.extend(
        synthesis
            .dissent
            .iter()
            .map(|dissent| format!("dissent: {dissent}")),
    );
    let artifacts = reports
        .iter()
        .filter_map(|report| report.artifact.clone())
        .collect::<Vec<_>>();
    let blocker = (successful_count == 0).then(|| {
        reports
            .iter()
            .map(|report| {
                format!(
                    "{}: {}",
                    report.member_id,
                    report
                        .diagnostic
                        .as_deref()
                        .unwrap_or(report.summary.as_str())
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    });

    AgentResult {
        schema_version: 1,
        agent: COUNCIL_WORKFLOW_AGENT_ID.to_string(),
        step_id,
        status,
        summary: format!(
            "Council confidence {} from {}/{} councillor(s). Recommended action: {}",
            synthesis.confidence,
            successful_count,
            reports.len(),
            synthesis.recommended_action
        ),
        findings,
        changed_files: Vec::new(),
        commands: Vec::new(),
        verification: Vec::new(),
        blocker,
        artifacts,
    }
}

fn action_executable_without_approval(
    agent_profile: &AgentProfile,
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> bool {
    // The allowed-ish decisions all run without a pending approval (ADR-003):
    // plain `Allowed`, trust auto-approval, and gray-area warn-and-run.
    if !matches!(
        crate::actions::validate_action_request_with_scope(agent_profile, context, request),
        ActionDecision::Allowed
            | ActionDecision::AllowedByTrust(_)
            | ActionDecision::AllowedWithWarning(_)
    ) {
        return false;
    }

    if let ActionKind::RunCommand = request.kind {
        let Some(command) = request
            .params
            .get("command")
            .and_then(serde_json::Value::as_str)
        else {
            return false;
        };
        if is_vcs_mutation(command)
            && !vcs_action_explicitly_requested(&context.user_prompt, command)
        {
            return false;
        }
    }

    true
}

fn wall_clock_limit_reached(limit: &Limit, started_at: Instant) -> bool {
    time_limit_reached(limit, started_at)
}

fn time_limit_reached(limit: &Limit, started_at: Instant) -> bool {
    matches!(remaining_limit_duration(limit, started_at), Some(remaining) if remaining.is_zero())
}

fn remaining_limit_duration(limit: &Limit, started_at: Instant) -> Option<Duration> {
    match limit {
        Limit::Value(minutes) => {
            Some(Duration::from_secs(u64::from(*minutes) * 60).saturating_sub(started_at.elapsed()))
        }
        Limit::Unlimited => None,
    }
}

async fn await_with_step_limit<F, T>(future: F, limit: &Limit, started_at: Instant) -> Option<T>
where
    F: Future<Output = T>,
{
    match remaining_limit_duration(limit, started_at) {
        Some(remaining) if remaining.is_zero() => None,
        Some(remaining) => tokio::time::timeout(remaining, future).await.ok(),
        None => Some(future.await),
    }
}

fn sender_next_interrupt_value(sender: &watch::Sender<u64>) -> u64 {
    (*sender.borrow()).wrapping_add(1)
}

async fn wait_for_interrupt(mut receiver: watch::Receiver<u64>, initial_value: u64) {
    loop {
        if *receiver.borrow_and_update() != initial_value {
            return;
        }
        if receiver.changed().await.is_err() {
            pending::<()>().await;
        }
    }
}

async fn wait_for_approval(
    mut receiver: watch::Receiver<ApprovalSignal>,
    initial_sequence: u64,
) -> ApprovalSignal {
    loop {
        let signal = *receiver.borrow_and_update();
        if signal.sequence != initial_sequence {
            return signal;
        }
        if receiver.changed().await.is_err() {
            pending::<()>().await;
        }
    }
}

/// Wire label for the `tier` field on risk-bearing events / views.
pub(crate) fn risk_tier_label(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::Low => "low",
        RiskTier::Medium => "medium",
        RiskTier::High => "high",
    }
}

/// Wire label for the `approval_resolved.resolution` field.
fn approval_resolution_label(resolution: ApprovalResolution) -> &'static str {
    match resolution {
        ApprovalResolution::Deny => "deny",
        ApprovalResolution::ApproveOnce => "approve_once",
        ApprovalResolution::ApproveAndTrust => "approve_and_trust",
    }
}

/// Human-readable description of a trust target for chat/event display.
fn trust_target_label(target: &TrustTarget) -> String {
    match target {
        TrustTarget::Command(command) => format!("command `{command}`"),
        TrustTarget::WritePath(path) => format!("writes to {}", path.display()),
    }
}

/// Stable structured payload for trust targets in `trust_granted` /
/// `approval_auto_resolved` events (projected in task_06).
fn trust_target_payload(target: &TrustTarget) -> serde_json::Value {
    match target {
        TrustTarget::Command(command) => json!({ "kind": "command", "value": command }),
        TrustTarget::WritePath(path) => {
            json!({ "kind": "write_path", "value": path.display().to_string() })
        }
    }
}

/// Cap a preview string (command/diff) before it enters `AppState`, so a huge
/// diff can't bloat the snapshot channel (TechSpec "Known Risks").
fn capped_preview(text: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 4000;
    if text.chars().count() <= MAX_PREVIEW_CHARS {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX_PREVIEW_CHARS).collect();
    format!("{truncated}\n… (truncated)")
}

/// Whether the newest session OTHER than `current_id` ended non-terminally — the
/// signal for the dynamic post-crash welcome hint (task_13). Read from
/// `list_session_summaries` (newest-first), skipping the just-created current
/// session so a fresh launch never flags itself. `false` when there is no prior
/// session or it ended cleanly (no false "crash" on a normal quit).
fn newest_prior_session_recoverable(root: &Path, current_id: &str) -> bool {
    list_session_summaries(root)
        .into_iter()
        .find(|summary| summary.session_id != current_id)
        .map(|summary| !summary.outcome.is_terminal())
        .unwrap_or(false)
}

/// Human-readable drift context for the first-mutation approval prompt and the
/// `ActionExecutionContext` gate (ADR-004/007). Names the concrete drift modes
/// (moved cwd, changed HEAD) without surfacing the dirty flag (never a trigger).
fn drift_ack_message(drift: &git::WorkspaceDrift) -> String {
    let mut reasons = Vec::new();
    if drift.cwd_moved {
        reasons.push("the working directory moved");
    }
    if drift.head_changed {
        reasons.push("the git HEAD changed");
    }
    let what = if reasons.is_empty() {
        "the workspace changed".to_string()
    } else {
        reasons.join(" and ")
    };
    format!(
        "Workspace drift since this session paused: {what}. Verify you are on the \
         intended tree before this first change."
    )
}

/// Build the rich `PendingApprovalView` for the modal (ADR-003). Risk fields are
/// derived from `assess_risk` (the same pure assessment the gate used), with the
/// command/diff previews capped before they enter `AppState`.
#[allow(clippy::too_many_arguments)]
fn build_pending_approval_view(
    run_id: String,
    group_id: Option<String>,
    step_id: String,
    agent: String,
    summary: String,
    diagnostic: Option<String>,
    request: &ActionRequest,
    context: &ActionExecutionContext,
) -> PendingApprovalView {
    let risk: RiskNote = crate::actions::assess_risk(request, context);
    let resolved_command = request
        .params
        .get("command")
        .and_then(serde_json::Value::as_str)
        .map(|command| capped_preview(&crate::actions::normalize_command(command)));
    let diff = request
        .params
        .get("diff")
        .and_then(serde_json::Value::as_str)
        .map(capped_preview);
    let affected_paths = match &risk.target {
        Some(TrustTarget::WritePath(path)) => vec![path.display().to_string()],
        _ => Vec::new(),
    };
    // Fold the drift context into the prompt only for the mutating action the
    // interlock actually gates (ADR-004); a read-only action under the same armed
    // gate shows no drift notice.
    let drift_notice = context
        .drift_ack
        .clone()
        .filter(|_| crate::actions::is_mutating_kind(&request.kind));
    // MCP trust-legibility (task_09): for a tool call, surface the origin server,
    // the tool name, its full untruncated description (from the catalog snapshot),
    // and whether the server is already trusted.
    let (mcp_server, mcp_tool, mcp_description, mcp_trusted) =
        if request.kind == ActionKind::CallMcpTool {
            let server = request
                .params
                .get("server")
                .and_then(serde_json::Value::as_str);
            let tool = request
                .params
                .get("tool")
                .and_then(serde_json::Value::as_str);
            let description = match (context.mcp.as_ref(), server, tool) {
                (Some(mcp), Some(server), Some(tool)) => mcp
                    .catalog
                    .tool(server, tool)
                    .and_then(|tool| tool.description.clone()),
                _ => None,
            };
            let trusted = match (context.mcp.as_ref(), server) {
                (Some(mcp), Some(server)) => mcp.trust.is_trusted(server),
                _ => false,
            };
            (
                server.map(str::to_string),
                tool.map(str::to_string),
                description,
                trusted,
            )
        } else {
            (None, None, None, false)
        };
    PendingApprovalView {
        run_id,
        group_id,
        step_id,
        action_id: request.action_id.clone(),
        agent,
        summary,
        diagnostic,
        tier: Some(risk.tier),
        catastrophic: risk.catastrophic,
        reason: Some(risk.reason),
        resolved_command,
        diff,
        affected_paths,
        boundary_crossed: None,
        reversible: None,
        trust_target: risk.target,
        drift_notice,
        mcp_server,
        mcp_tool,
        mcp_description,
        mcp_trusted,
    }
}

fn spawn_parallel_runtime_task(
    config: EffectiveConfig,
    step_id: String,
    request: RuntimeRequest,
    first_sequence: u32,
    cancellation: CancellationToken,
    sender: mpsc::Sender<ParallelRuntimeMessage>,
) {
    tokio::spawn(async move {
        let (events, mut receiver) =
            RuntimeEventSink::channel_from(RUNTIME_EVENT_CHANNEL_CAPACITY, first_sequence);
        let runtime_step =
            execute_runtime_step_streaming(&config, request, events, cancellation.clone());
        tokio::pin!(runtime_step);
        loop {
            tokio::select! {
                output = &mut runtime_step => {
                    while let Ok(event) = receiver.try_recv() {
                        let _ = sender
                            .send(ParallelRuntimeMessage::RuntimeEvent {
                                step_id: step_id.clone(),
                                event,
                            })
                            .await;
                    }
                    let _ = sender
                        .send(ParallelRuntimeMessage::Output {
                            step_id: step_id.clone(),
                            output: Box::new(output),
                        })
                        .await;
                    break;
                }
                event = receiver.recv() => {
                    let Some(event) = event else {
                        continue;
                    };
                    if sender
                        .send(ParallelRuntimeMessage::RuntimeEvent {
                            step_id: step_id.clone(),
                            event,
                        })
                        .await
                        .is_err()
                    {
                        cancellation.cancel();
                        break;
                    }
                }
            }
        }
    });
}

fn parallel_child_prompt(
    user_prompt: &str,
    step: &crate::orchestrator::ParallelChildStepPlan,
) -> String {
    format!(
        "Original user prompt:\n{user_prompt}\n\nParallel child step:\n{}\n\nScoped instruction:\n{}\n\nReturn only this child step's agent_result. Do not work outside the assigned Parallel File Scope.",
        step.step_label, step.instruction
    )
}

fn parallel_scope_policy_summary(scope: &ParallelFileScope) -> String {
    format!(
        "May write only exact files [{}]. May read roots [{}]. Out-of-scope actions are denied by Harness policy.",
        scope.write_files.join(", "),
        scope.read_roots.join(", ")
    )
}

fn synthesize_parallel_group_result(
    run_id: &str,
    group_id: &str,
    started_at: &str,
    child_results: &[(&ParallelChildRuntimeState, AgentResult)],
) -> ParallelGroupResult {
    let mut counts = BTreeMap::new();
    let mut changed_files = Vec::new();
    let mut blocked_scopes = Vec::new();
    let mut failed_scopes = Vec::new();
    let mut approval_denials = Vec::new();
    let mut children = Vec::new();
    let mut successful = 0usize;

    for (index, (child, result)) in child_results.iter().enumerate() {
        let status_label = agent_status_key(&result.status).to_string();
        *counts.entry(status_label).or_insert(0) += 1;
        if matches!(
            result.status,
            AgentResultStatus::Completed | AgentResultStatus::NoChanges
        ) {
            successful += 1;
        }
        changed_files.extend(result.changed_files.iter().cloned());
        if matches!(result.status, AgentResultStatus::Blocked) {
            blocked_scopes.push(ParallelBlockedScope {
                step_id: child.step_id.clone(),
                step_label: child.step_label.clone(),
                agent: child.agent_id.clone(),
                file_scope: child.file_scope.clone(),
                blocker: result
                    .blocker
                    .clone()
                    .unwrap_or_else(|| result.summary.clone()),
            });
        }
        if matches!(result.status, AgentResultStatus::ApprovalDenied) {
            approval_denials.push(child.step_id.clone());
        }
        if matches!(
            result.status,
            AgentResultStatus::Failed | AgentResultStatus::ParseError
        ) {
            failed_scopes.push(ParallelFailedScope {
                step_id: child.step_id.clone(),
                step_label: child.step_label.clone(),
                agent: child.agent_id.clone(),
                file_scope: child.file_scope.clone(),
                diagnostic: result
                    .blocker
                    .clone()
                    .unwrap_or_else(|| result.summary.clone()),
            });
        }
        children.push(ParallelChildResultRef {
            step_id: child.step_id.clone(),
            step_label: child.step_label.clone(),
            agent: child.agent_id.clone(),
            file_scope: child.file_scope.clone(),
            status: result.status.clone(),
            result_index: index,
        });
    }
    changed_files.sort();
    changed_files.dedup();

    let status = if child_results.iter().all(|(_, result)| {
        matches!(
            result.status,
            AgentResultStatus::Cancelled | AgentResultStatus::LimitReached
        )
    }) {
        if child_results
            .iter()
            .any(|(_, result)| matches!(result.status, AgentResultStatus::LimitReached))
        {
            ParallelGroupStatus::LimitReached
        } else {
            ParallelGroupStatus::Cancelled
        }
    } else if successful == child_results.len() {
        ParallelGroupStatus::Completed
    } else if successful > 0 {
        ParallelGroupStatus::CompletedWithIssues
    } else {
        ParallelGroupStatus::Failed
    };
    let summary = format!(
        "Parallel group {group_id} joined with {successful}/{} successful child step(s).",
        child_results.len()
    );

    ParallelGroupResult {
        schema_version: 1,
        group_id: group_id.to_string(),
        run_id: run_id.to_string(),
        status,
        summary,
        children,
        counts,
        changed_files,
        blocked_scopes,
        failed_scopes,
        approval_denials,
        started_at: started_at.to_string(),
        completed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

/// One node's view for the pure DAG admission decision (ADR-004): current
/// lifecycle state, whether it succeeded (meaningful only when terminal),
/// command-ness, its write set, and its predecessor node_ids.
#[derive(Clone, Debug)]
struct DagNodeSnapshot {
    node_id: String,
    run_state: NodeRunState,
    succeeded: bool,
    is_command: bool,
    write_files: BTreeSet<String>,
    predecessors: Vec<String>,
}

/// The admission decision over a graph snapshot: which currently-Pending nodes
/// to spawn now (`admit`) and which to terminalize as Skipped now (`skip`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AdmissionDecision {
    admit: Vec<String>,
    skip: Vec<String>,
}

/// Pure ready-set admission over node states (ADR-004) — no I/O, no spawning;
/// the scheduler applies the returned decision. This function carries the
/// feature's hardest invariant (fail-closed forward progress):
///   * **skip-propagation cascades to a fixpoint**, so every node downstream of
///     a failed/blocked/cancelled/skipped predecessor is terminalized in a
///     single call (the join loop can then drain to all-terminal — no deadlock);
///   * a node is **admitted** only when every predecessor is terminal AND
///     succeeded, its `write_files` are disjoint from the union of running
///     nodes' writes, and — if it runs commands — nothing else is running
///     (command nodes run in isolation).
///
/// `Pending` (planned-not-admitted) nodes are never counted as "running", so
/// their write scopes do not pre-occupy the running set.
fn compute_admission(nodes: &[DagNodeSnapshot], max_concurrent: usize) -> AdmissionDecision {
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.node_id.as_str(), i))
        .collect();

    // Fail-closed skip-propagation to a fixpoint: a Pending node is doomed when
    // any predecessor is terminal-and-not-succeeded or itself already doomed.
    let mut doomed: BTreeSet<usize> = BTreeSet::new();
    loop {
        let mut changed = false;
        for (i, node) in nodes.iter().enumerate() {
            if node.run_state != NodeRunState::Pending || doomed.contains(&i) {
                continue;
            }
            let blocked = node.predecessors.iter().any(|pred_id| {
                index.get(pred_id.as_str()).is_some_and(|&pi| {
                    doomed.contains(&pi)
                        || (nodes[pi].run_state == NodeRunState::Terminal && !nodes[pi].succeeded)
                })
            });
            if blocked {
                doomed.insert(i);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Admission over the surviving Pending nodes (deterministic by slice order).
    let running_count = nodes
        .iter()
        .filter(|n| n.run_state == NodeRunState::Running)
        .count();
    let initially_running = running_count > 0;
    let mut command_active = nodes
        .iter()
        .any(|n| n.run_state == NodeRunState::Running && n.is_command);
    let mut admitted_any = false;
    let mut running_writes: BTreeSet<String> = nodes
        .iter()
        .filter(|n| n.run_state == NodeRunState::Running)
        .flat_map(|n| n.write_files.iter().cloned())
        .collect();
    let mut admit = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if node.run_state != NodeRunState::Pending || doomed.contains(&i) {
            continue;
        }
        // A command node, once running or admitted this pass, monopolizes the
        // graph: nothing else may be admitted alongside it.
        if command_active {
            break;
        }
        // Honor the reused concurrency ceiling: never let running + newly-admitted
        // nodes exceed `max_parallel_agent_steps` (ADR-004 — the DAG reuses the
        // same bound the flat parallel path enforces).
        if running_count + admit.len() >= max_concurrent {
            break;
        }
        let ready = node.predecessors.iter().all(|pred_id| {
            index.get(pred_id.as_str()).is_none_or(|&pi| {
                nodes[pi].run_state == NodeRunState::Terminal && nodes[pi].succeeded
            })
        });
        if !ready {
            continue;
        }
        if node.is_command {
            // Command nodes run alone: nothing running, nothing admitted yet.
            if !initially_running && !admitted_any {
                admit.push(node.node_id.clone());
                admitted_any = true;
                command_active = true;
            }
        } else if node.write_files.is_disjoint(&running_writes) {
            admit.push(node.node_id.clone());
            running_writes.extend(node.write_files.iter().cloned());
            admitted_any = true;
        }
    }

    let skip = doomed.iter().map(|&i| nodes[i].node_id.clone()).collect();
    AdmissionDecision { admit, skip }
}

/// Whether a graph node runs commands, derived from its declared capabilities
/// (ADR-004) — command nodes are scheduled in isolation since their reads and
/// side effects can't be fenced.
fn node_runs_commands(node: &crate::orchestrator::ExecutionNode) -> bool {
    node.required_capabilities.contains(&Capability::Command)
}

/// Map a completed node's result status to its lifecycle event kind.
fn execution_graph_node_kind(status: &AgentResultStatus) -> &'static str {
    match status {
        AgentResultStatus::Completed | AgentResultStatus::NoChanges => {
            crate::history::NODE_SUCCEEDED_KIND
        }
        _ => crate::history::NODE_FAILED_KIND,
    }
}

/// The Plan-view status label for a node's current runtime state. A node in the
/// `skipped_nodes` set renders `skipped`; otherwise terminality and the result
/// status (or the admission state) decide the label.
fn node_status_label(
    child: &ParallelChildRuntimeState,
    skipped_nodes: &BTreeSet<String>,
) -> &'static str {
    if skipped_nodes.contains(&child.step_id) {
        return "skipped";
    }
    match &child.terminal_result {
        Some(result) => match result.status {
            AgentResultStatus::Completed | AgentResultStatus::NoChanges => "succeeded",
            AgentResultStatus::Cancelled => "cancelled",
            _ => "failed",
        },
        None => match child.node_run_state {
            NodeRunState::Running => "running",
            NodeRunState::Pending | NodeRunState::Terminal => "pending",
        },
    }
}

/// A full graph snapshot (every node's current status + the typed edges) carried
/// in each graph/node event payload (ADR-005) so the single evolving Plan item
/// can re-render the whole graph from any one event.
fn execution_graph_snapshot(
    graph: &crate::orchestrator::ExecutionGraph,
    children: &BTreeMap<String, ParallelChildRuntimeState>,
    skipped_nodes: &BTreeSet<String>,
) -> serde_json::Value {
    let nodes: Vec<serde_json::Value> = graph
        .nodes
        .iter()
        .map(|node| {
            let status = children
                .get(&node.node_id)
                .map(|child| node_status_label(child, skipped_nodes))
                .unwrap_or("pending");
            json!({
                "node_id": node.node_id,
                "step_label": node.step_label,
                "agent": node.agent,
                "status": status,
                "file_scope": node.file_scope,
            })
        })
        .collect();
    let edges: Vec<serde_json::Value> = graph
        .edges
        .iter()
        .map(|edge| json!({ "from": edge.from, "to": edge.to, "kind": edge.kind }))
        .collect();
    json!({
        "graph_id": graph.graph_id,
        "reason": graph.reason,
        "nodes": nodes,
        "edges": edges,
    })
}

/// The terminal AgentResult for a node skipped by fail-closed propagation. Uses
/// `Cancelled` status (it never ran); the scheduler tracks skipped node_ids
/// separately so the result and events can distinguish skip from interrupt.
fn skipped_agent_result(agent: &str, node_id: &str, diagnostic: &str) -> AgentResult {
    AgentResult {
        status: AgentResultStatus::Cancelled,
        ..failed_agent_result(agent, node_id, diagnostic)
    }
}

/// Build the terminal [`ExecutionGraphResult`] from each node's recorded result
/// plus the set of node_ids that were skipped (ADR-004/005). Status mirrors
/// [`synthesize_parallel_group_result`].
fn synthesize_execution_graph_result(
    run_id: &str,
    graph_id: &str,
    started_at: &str,
    node_results: &[(&ParallelChildRuntimeState, AgentResult)],
    skipped_nodes: &BTreeSet<String>,
) -> crate::orchestrator::ExecutionGraphResult {
    use crate::orchestrator::{ExecutionGraphStatus, NodeResultRef};
    let mut counts = BTreeMap::new();
    let mut changed_files = Vec::new();
    let mut results = Vec::new();
    let mut failed = Vec::new();
    let mut successful = 0usize;

    for (index, (child, result)) in node_results.iter().enumerate() {
        let is_skipped = skipped_nodes.contains(&child.step_id);
        let label = if is_skipped {
            "skipped".to_string()
        } else {
            agent_status_key(&result.status).to_string()
        };
        *counts.entry(label).or_insert(0) += 1;
        if !is_skipped
            && matches!(
                result.status,
                AgentResultStatus::Completed | AgentResultStatus::NoChanges
            )
        {
            successful += 1;
        }
        if !is_skipped
            && matches!(
                result.status,
                AgentResultStatus::Failed
                    | AgentResultStatus::ParseError
                    | AgentResultStatus::Blocked
                    | AgentResultStatus::ApprovalDenied
            )
        {
            failed.push(child.step_id.clone());
        }
        changed_files.extend(result.changed_files.iter().cloned());
        results.push(NodeResultRef {
            node_id: child.step_id.clone(),
            step_label: child.step_label.clone(),
            agent: child.agent_id.clone(),
            file_scope: child.file_scope.clone(),
            status: result.status.clone(),
            result_index: index,
        });
    }
    changed_files.sort();
    changed_files.dedup();

    let total = node_results.len();
    let status = if total > 0
        && node_results.iter().all(|(_, result)| {
            matches!(
                result.status,
                AgentResultStatus::Cancelled | AgentResultStatus::LimitReached
            )
        }) {
        if node_results
            .iter()
            .any(|(_, result)| matches!(result.status, AgentResultStatus::LimitReached))
        {
            ExecutionGraphStatus::LimitReached
        } else {
            ExecutionGraphStatus::Cancelled
        }
    } else if total > 0 && successful == total {
        ExecutionGraphStatus::Completed
    } else if successful > 0 {
        ExecutionGraphStatus::CompletedWithIssues
    } else {
        ExecutionGraphStatus::Failed
    };
    let summary = format!(
        "Execution graph {graph_id} joined with {successful}/{total} successful node(s), {} skipped.",
        skipped_nodes.len()
    );

    crate::orchestrator::ExecutionGraphResult {
        schema_version: 1,
        graph_id: graph_id.to_string(),
        run_id: run_id.to_string(),
        status,
        summary,
        node_results: results,
        counts,
        changed_files,
        skipped: skipped_nodes.iter().cloned().collect(),
        failed,
        started_at: started_at.to_string(),
        completed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

fn failed_agent_result(agent: &str, step_id: &str, diagnostic: &str) -> AgentResult {
    AgentResult {
        schema_version: 1,
        agent: agent.to_string(),
        step_id: step_id.to_string(),
        status: AgentResultStatus::Failed,
        summary: diagnostic.to_string(),
        findings: Vec::new(),
        changed_files: Vec::new(),
        commands: Vec::new(),
        verification: Vec::new(),
        blocker: Some(diagnostic.to_string()),
        artifacts: Vec::new(),
    }
}

fn limit_reached_agent_result(agent: &str, step_id: &str, diagnostic: &str) -> AgentResult {
    AgentResult {
        status: AgentResultStatus::LimitReached,
        ..failed_agent_result(agent, step_id, diagnostic)
    }
}

fn cancelled_agent_result(agent: &str, step_id: &str, diagnostic: &str) -> AgentResult {
    AgentResult {
        status: AgentResultStatus::Cancelled,
        ..failed_agent_result(agent, step_id, diagnostic)
    }
}

fn agent_status_key(status: &AgentResultStatus) -> &'static str {
    match status {
        AgentResultStatus::Completed => "completed",
        AgentResultStatus::Blocked => "blocked",
        AgentResultStatus::Failed => "failed",
        AgentResultStatus::Cancelled => "cancelled",
        AgentResultStatus::ParseError => "parse_error",
        AgentResultStatus::LimitReached => "limit_reached",
        AgentResultStatus::ApprovalDenied => "approval_denied",
        AgentResultStatus::NoChanges => "no_changes",
    }
}

fn review_fix_cycle_count(results: &[RunStepResult]) -> u32 {
    let mut saw_review_since_last_fix = false;
    let mut cycles = 0;
    for result in agent_results(results) {
        match result.agent.as_str() {
            "reviewer" => saw_review_since_last_fix = true,
            "fixer" if saw_review_since_last_fix => {
                cycles += 1;
                saw_review_since_last_fix = false;
            }
            _ => {}
        }
    }
    cycles
}

fn debug_enabled_from_env() -> bool {
    env::var("MULTIAGENT_LOG")
        .map(|value| value.eq_ignore_ascii_case("debug"))
        .unwrap_or(false)
}

fn user_event_display(message: &str) -> String {
    format!("You: {}", single_line_event_text(message))
}

fn compile_app_prompt(working_directory: &Path, prompt: &str) -> Result<CompiledPrompt> {
    skills::compile_prompt(working_directory, prompt)
        .map_err(|error| anyhow!(skill_load_diagnostic(&error)))
}

fn skill_load_diagnostic(error: &SkillLoadError) -> String {
    format!("failed to load skill reference: {error}")
}

fn skills_loaded_payload_from_metadata(metadata: &[LoadedSkillMetadata]) -> Value {
    json!({ "skills": metadata })
}

fn loaded_skills_display(metadata: &[LoadedSkillMetadata]) -> String {
    let skills = metadata
        .iter()
        .map(|skill| format!("{} ({})", skill.display_name, skill.source_origin))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Skills loaded: {skills}.")
}

/// Transcript line for a `hook_started`/`hook_completed` lifecycle event.
fn hook_lifecycle_display(record: &HookLifecycleRecord) -> String {
    let payload = &record.payload;
    match record.kind {
        crate::hooks::HookLifecycleKind::Started => {
            format!("Hook started: {} on {}.", payload.action, payload.event)
        }
        crate::hooks::HookLifecycleKind::Completed => {
            let status = payload.status.as_deref().unwrap_or("done");
            format!(
                "Hook completed: {} on {} ({status}).",
                payload.action, payload.event
            )
        }
    }
}

fn config_status_display(status: &ConfigStatusView) -> String {
    let sources = status.sources.join(", ");
    let preset = status.preset.as_deref().unwrap_or("none");
    let approval = approval_mode_label(&status.approval_mode);
    let dag = if status.execution_graph_enabled {
        format!(
            "enabled (max_parallel_agent_steps={})",
            status.max_parallel_agent_steps
        )
    } else {
        "disabled".to_string()
    };
    let warnings = if status.warnings.is_empty() {
        "none".to_string()
    } else {
        status.warnings.join("; ")
    };
    format!(
        "Config: sources: {sources}; preset: {preset}; approval: {approval}; dag: {dag}; warnings: {warnings}."
    )
}

/// Wire label for an [`ApprovalMode`] in the `/config` display.
fn approval_mode_label(mode: &ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::Yolo => "yolo",
        ApprovalMode::Normal => "normal",
    }
}

fn parse_workflow_command(input: &str) -> Result<Option<WorkflowCommand>> {
    let trimmed = input.trim();
    if trimmed == WORKFLOW_COMMAND {
        bail!(WORKFLOW_USAGE);
    }
    let Some(rest) = trimmed.strip_prefix(WORKFLOW_COMMAND) else {
        return Ok(None);
    };
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return Ok(None);
    }
    let prompt = rest.trim();
    if prompt.is_empty() {
        bail!(WORKFLOW_USAGE);
    }
    Ok(Some(WorkflowCommand {
        original_command: trimmed.to_string(),
        prompt: prompt.to_string(),
    }))
}

fn preflight_workflow_prerequisites(config: &EffectiveConfig) -> Result<WorkflowPreflight> {
    if !config.features.parallel_step_groups {
        bail!(
            "workflow mode requires Parallel Step Groups; parallel step groups are disabled by features.parallel_step_groups"
        );
    }
    if config.limits.max_parallel_agent_steps == 0 {
        bail!(
            "workflow mode requires Parallel Step Groups; parallel step groups are disabled by limits.max_parallel_agent_steps = 0"
        );
    }
    Ok(WorkflowPreflight {
        parallel_step_groups: config.features.parallel_step_groups,
        max_parallel_agent_steps: config.limits.max_parallel_agent_steps,
    })
}

fn normalize_workflow_target_key(
    path: &str,
    working_directory: &Path,
    extra_write_roots: &[PathBuf],
) -> Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("workflow target path is empty");
    }
    if trimmed.split('/').any(|component| component == ".") {
        bail!("current-directory components are not allowed in workflow target path: {path}");
    }

    let candidate = Path::new(trimmed);
    let relative = if candidate.is_absolute() {
        // The path is already validated against the workspace and the configured
        // extra write roots. Strip whichever base actually matches: an extra
        // write root can live outside the repo, where stripping the working
        // directory would fail and abort the whole workflow.
        let base = std::iter::once(working_directory)
            .chain(extra_write_roots.iter().map(PathBuf::as_path))
            .find(|base| candidate.starts_with(base));
        match base {
            Some(base) => candidate
                .strip_prefix(base)
                .expect("candidate starts_with the matched base"),
            None => {
                bail!("absolute paths are not allowed in workflow target paths: {path}")
            }
        }
    } else {
        candidate
    };

    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::ParentDir => {
                bail!("path traversal is not allowed in workflow target path: {path}")
            }
            Component::Prefix(_) | Component::RootDir => {
                bail!("rooted paths are not allowed in workflow target paths: {path}")
            }
            Component::CurDir => {
                bail!(
                    "current-directory components are not allowed in workflow target path: {path}"
                )
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
        }
    }
    if parts.is_empty() {
        bail!("workflow target path is empty");
    }
    Ok(parts.join("/"))
}

fn workflow_started_payload(run_id: &str, start: &WorkflowStart) -> Value {
    json!({
        "run_id": run_id,
        "original_command": start.command.original_command.as_str(),
        "user_prompt": start.command.prompt.as_str(),
        "mode": "workflow",
        "preflight": &start.preflight,
    })
}

fn workflow_target_status_from_agent_result(
    result: &AgentResult,
) -> (WorkflowTargetStatus, Option<String>) {
    match result.status {
        AgentResultStatus::Completed | AgentResultStatus::NoChanges => {
            (WorkflowTargetStatus::Completed, None)
        }
        AgentResultStatus::Blocked | AgentResultStatus::ApprovalDenied => (
            WorkflowTargetStatus::Blocked,
            Some(workflow_result_diagnostic(result)),
        ),
        AgentResultStatus::Failed
        | AgentResultStatus::ParseError
        | AgentResultStatus::LimitReached
        | AgentResultStatus::Cancelled => (
            WorkflowTargetStatus::Failed,
            Some(workflow_result_diagnostic(result)),
        ),
    }
}

fn workflow_result_diagnostic(result: &AgentResult) -> String {
    result
        .blocker
        .clone()
        .filter(|diagnostic| !diagnostic.trim().is_empty())
        .unwrap_or_else(|| result.summary.clone())
}

fn derive_workflow_completion_status(
    counts: &WorkflowTargetCounts,
    unfinished_targets: &[WorkflowUnfinishedTarget],
    interrupted: bool,
) -> WorkflowCompletionStatus {
    // A workflow that finished without interruption and left no planned target
    // unaccounted for is a success. Having zero declared file-edit targets is
    // normal for orchestrator-driven runs that write via single-agent actions
    // (or that legitimately make no edits) — it must NOT be reported as failed.
    if interrupted || counts.planned > 0 {
        return WorkflowCompletionStatus::Failed;
    }
    if unfinished_targets.is_empty() {
        WorkflowCompletionStatus::Completed
    } else {
        WorkflowCompletionStatus::CompletedWithIssues
    }
}

fn workflow_runtime_prompt(
    command: &WorkflowCommand,
    user_prompt: &str,
    preflight: &WorkflowPreflight,
) -> String {
    format!(
        r#"Workflow mode instructions:
- Treat this as one normal app Run in workflow mode.
- Decompose the user's broad request into a concrete Run Plan before mutation-capable work.
- Execute safe specialized-agent steps, using Parallel Step Groups only when file scopes are exact, disjoint, and policy-compliant.
- Validate completed work with appropriate checks, or explain skipped checks with reasons.
- Account for planned file-edit targets explicitly: completed, skipped, blocked, or failed.
- Final synthesis must include plan evidence, child outcomes, changed files, verification evidence, skipped checks, and residual risks.

Workflow preflight:
- parallel_step_groups: {parallel_step_groups}
- max_parallel_agent_steps: {max_parallel_agent_steps}

Original command:
{original_command}

Extracted user prompt:
{user_prompt}"#,
        parallel_step_groups = preflight.parallel_step_groups,
        max_parallel_agent_steps = preflight.max_parallel_agent_steps,
        original_command = command.original_command,
        user_prompt = user_prompt,
    )
}

fn parse_subtask_command(input: &str) -> Result<(&str, &str)> {
    let trimmed = input.trim();
    let Some((agent, task)) = trimmed.split_once(char::is_whitespace) else {
        bail!("usage: /subtask <agent> <task>");
    };
    let task = task.trim();
    if agent.trim().is_empty() || task.is_empty() {
        bail!("usage: /subtask <agent> <task>");
    }
    Ok((agent.trim(), task))
}

fn parse_queue_command(prompt: &str) -> Result<Option<String>> {
    let trimmed = prompt.trim();
    if !trimmed.starts_with('/') {
        return Ok(None);
    }
    let (command, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((command, rest)) => (command, rest.trim()),
        None => (trimmed, ""),
    };
    if command != "/queue" && command != "/q" {
        return Ok(None);
    }
    if rest.is_empty() {
        bail!("usage: /queue <message> or /q <message>");
    }
    Ok(Some(rest.to_string()))
}

fn reject_unknown_slash_command(prompt: &str) -> Result<()> {
    let trimmed = prompt.trim();
    if !trimmed.starts_with('/') {
        return Ok(());
    }
    if is_named_prompt_prefix(trimmed, "/agent:") || trimmed.starts_with("/skill:") {
        return Ok(());
    }
    let command = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let available = crate::slash_commands::available_commands_summary();
    bail!("unknown command {command}. Available commands: {available}")
}

fn is_named_prompt_prefix(prompt: &str, prefix: &str) -> bool {
    let Some(rest) = prompt.strip_prefix(prefix) else {
        return false;
    };
    rest.split_whitespace()
        .next()
        .is_some_and(|name| !name.is_empty())
}

fn subtask_prompt(task: &str) -> String {
    format!(
        "Subtask request:\n{task}\n\nScope guard:\n- Work only on the subtask request above.\n- Do not broaden scope beyond this subtask.\n- If the request requires broader work, return a blocked agent_result explaining the needed parent-scope decision.\n- Return a concise child summary and verification evidence for only this subtask."
    )
}

fn action_requested_display(request: &ActionRequest) -> String {
    format!("Action requested: {}", action_target_display(request))
}

fn action_completed_display(request: &ActionRequest, result: &ActionResult) -> String {
    let target = action_target_display(request);
    let status = action_status_label(&result.status);
    let detail = action_result_detail(result);
    let artifact_suffix = if result.artifact.is_some() {
        " Output stored as artifact."
    } else {
        ""
    };
    if detail.is_empty() {
        format!("Action completed: {target} -> {status}.{artifact_suffix}")
    } else {
        format!("Action completed: {target} -> {status} ({detail}).{artifact_suffix}")
    }
}

fn action_denied_display(request: &ActionRequest, result: &ActionResult) -> String {
    let detail = action_result_detail(result);
    if detail.is_empty() {
        format!("Action denied: {}.", action_target_display(request))
    } else {
        format!(
            "Action denied: {} ({detail}).",
            action_target_display(request)
        )
    }
}

fn action_target_display(request: &ActionRequest) -> String {
    match request.kind {
        ActionKind::ReadFile => format!("read {}", required_path_display(&request.params)),
        ActionKind::ListFiles => {
            format!("list files in {}", optional_path_display(&request.params))
        }
        ActionKind::SearchText => format!(
            "search {} for {:?}",
            optional_path_display(&request.params),
            request
                .params
                .get("query")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing query>")
        ),
        ActionKind::RunCommand => format!(
            "run command {}",
            concise_diagnostic(
                request
                    .params
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<missing command>")
            )
        ),
        ActionKind::ApplyPatch => "apply patch".to_string(),
        ActionKind::WriteFile => format!("write {}", required_path_display(&request.params)),
        ActionKind::RecordNote => "record note".to_string(),
        ActionKind::CallMcpTool => format!(
            "call MCP tool {}/{}",
            request
                .params
                .get("server")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing server>"),
            request
                .params
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing tool>")
        ),
        ActionKind::ReadMcpResource => format!(
            "read MCP resource {}",
            request
                .params
                .get("uri")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing uri>")
        ),
        ActionKind::ListMcpResources => format!(
            "list MCP resources on {}",
            request
                .params
                .get("server")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing server>")
        ),
    }
}

fn optional_path_display(params: &Value) -> &str {
    params.get("path").and_then(Value::as_str).unwrap_or(".")
}

fn required_path_display(params: &Value) -> &str {
    params
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("<missing path>")
}

fn action_result_detail(result: &ActionResult) -> String {
    if matches!(
        result.status,
        ActionStatus::Failed | ActionStatus::Denied | ActionStatus::ApprovalRequired
    ) {
        if let Some(diagnostic) = &result.diagnostic {
            return concise_diagnostic(diagnostic);
        }
    }
    concise_diagnostic(&result.summary)
}

fn action_status_label(status: &ActionStatus) -> &'static str {
    match status {
        ActionStatus::Completed => "Completed",
        ActionStatus::Denied => "Denied",
        ActionStatus::ApprovalRequired => "Approval required",
        ActionStatus::Failed => "Failed",
    }
}

fn command_completed_display(command: &str, result: &ActionResult, exit_code: &Value) -> String {
    let status = action_status_label(&result.status);
    let exit = match exit_code.as_i64() {
        Some(code) => format!("exit {code}"),
        None => "signal".to_string(),
    };
    let detail = action_result_detail(result);
    format!(
        "Command completed: {} -> {status} ({exit}; {detail}).",
        concise_diagnostic(command)
    )
}

fn file_created_display(payload: &Value) -> String {
    let path = payload
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("<unknown file>");
    match payload.get("bytes").and_then(Value::as_u64) {
        Some(bytes) => format!("File created: {path} ({bytes} bytes)."),
        None => format!("File created: {path}."),
    }
}

fn files_edited_display(payload: &Value) -> String {
    let files = payload
        .get("changed_files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if files.is_empty() {
        return "Files edited.".to_string();
    }
    format!("Files edited: {}.", summarize_items(&files, 4))
}

fn summarize_items(items: &[String], limit: usize) -> String {
    let visible = items
        .iter()
        .take(limit)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() <= limit {
        visible
    } else {
        format!("{visible}, +{} more", items.len() - limit)
    }
}

fn concise_diagnostic(message: &str) -> String {
    let message = single_line_event_text(message);
    const MAX_DIAGNOSTIC_CHARS: usize = 220;
    if message.chars().count() <= MAX_DIAGNOSTIC_CHARS {
        return message;
    }
    format!(
        "{}...",
        message
            .chars()
            .take(MAX_DIAGNOSTIC_CHARS.saturating_sub(3))
            .collect::<String>()
    )
}

fn single_line_event_text(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::chat::{ChatItemKind, ChatItemStatus, ChatLifecycleKey, ChatSeverity};
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use crate::skills::{LoadedSkill, SkillLoadErrorKind};
    use serde_json::Value;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn fake_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"

[council.presets.default.architect]
runtime = "fake"

[council.presets.default.security]
runtime = "fake"

[council.presets.default.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    #[test]
    fn config_warning_messages_includes_keybinding_warnings() {
        let dir = tempdir().unwrap();
        let mut config = fake_config(dir.path());
        // No keybinding warnings ⇒ none surfaced for that source.
        assert!(!config_warning_messages(&config)
            .iter()
            .any(|w| w.contains("[keybindings]")));

        // A recorded keybinding warning is surfaced at startup / via /config.
        config
            .keybinding_warnings
            .push("ignored [keybindings] in ./atelier.toml".to_string());
        let warnings = config_warning_messages(&config);
        assert!(
            warnings.iter().any(|w| w.contains("ignored [keybindings]")),
            "expected keybinding warning, got: {warnings:?}"
        );
    }

    // ── /config DAG visibility (task_07) ──

    #[test]
    fn config_display_shows_dag_enabled_with_ceiling() {
        let dir = tempdir().unwrap();
        let mut config = fake_config(dir.path());
        config.features.execution_graph = true;
        config.limits.max_parallel_agent_steps = 3;
        let status = build_config_status(&config, &BTreeMap::new());
        assert!(status.execution_graph_enabled);
        assert_eq!(status.max_parallel_agent_steps, 3);
        let display = config_status_display(&status);
        assert!(
            display.contains("dag: enabled (max_parallel_agent_steps=3)"),
            "{display}"
        );
    }

    #[test]
    fn config_display_shows_dag_disabled_by_default() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let status = build_config_status(&config, &BTreeMap::new());
        assert!(!status.execution_graph_enabled);
        assert!(config_status_display(&status).contains("dag: disabled"));
    }

    #[test]
    fn config_warns_when_dag_enabled_but_ceiling_zero() {
        let dir = tempdir().unwrap();
        let mut config = fake_config(dir.path());
        config.features.execution_graph = true;
        config.limits.max_parallel_agent_steps = 0;
        let status = build_config_status(&config, &BTreeMap::new());
        assert!(
            status
                .warnings
                .iter()
                .any(|w| w.contains("max_parallel_agent_steps = 0 disables it")),
            "{:?}",
            status.warnings
        );
        assert!(config_status_display(&status).contains("max_parallel_agent_steps = 0 disables it"));
    }

    #[test]
    fn config_display_shows_approval_mode() {
        let dir = tempdir().unwrap();
        let mut config = fake_config(dir.path());
        config.approval_mode = ApprovalMode::Normal;
        let normal = config_status_display(&build_config_status(&config, &BTreeMap::new()));
        assert!(normal.contains("approval: normal"), "{normal}");
        config.approval_mode = ApprovalMode::Yolo;
        let yolo = config_status_display(&build_config_status(&config, &BTreeMap::new()));
        assert!(yolo.contains("approval: yolo"), "{yolo}");
    }

    #[test]
    fn config_status_view_serializes_with_dag_fields() {
        let dir = tempdir().unwrap();
        let mut config = fake_config(dir.path());
        config.features.execution_graph = true;
        config.limits.max_parallel_agent_steps = 2;
        let value = serde_json::to_value(build_config_status(&config, &BTreeMap::new())).unwrap();
        assert_eq!(value["execution_graph_enabled"], json!(true));
        assert_eq!(value["max_parallel_agent_steps"], json!(2));
        assert_eq!(value["approval_mode"], json!("yolo"));
    }

    #[tokio::test]
    async fn config_command_records_dag_fields() {
        let dir = tempdir().unwrap();
        let mut config = fake_config(dir.path());
        config.features.execution_graph = true;
        config.limits.max_parallel_agent_steps = 2;
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/config").await.unwrap();

        let events = app.history.read_events().unwrap();
        let viewed = events
            .iter()
            .find(|event| event.kind == "config_viewed")
            .expect("config_viewed recorded");
        assert_eq!(viewed.payload["execution_graph_enabled"], json!(true));
        assert_eq!(viewed.payload["max_parallel_agent_steps"], json!(2));
        let display = app.state.events.join("\n");
        assert!(
            display.contains("dag: enabled (max_parallel_agent_steps=2)"),
            "{display}"
        );
        assert!(display.contains("approval: yolo"), "{display}");
    }

    fn write_project_skill(
        project_root: &Path,
        directory_name: &str,
        frontmatter_name: Option<&str>,
        body: &str,
    ) {
        let skill_dir = project_root.join(".agents/skills").join(directory_name);
        fs::create_dir_all(&skill_dir).unwrap();
        let contents = if let Some(name) = frontmatter_name {
            format!("---\nname: {name}\ndescription: test skill\n---\n{body}\n")
        } else {
            format!("{body}\n")
        };
        fs::write(skill_dir.join("SKILL.md"), contents).unwrap();
    }

    fn test_loaded_skill(display_name: &str, content: &str) -> LoadedSkill {
        LoadedSkill {
            metadata: LoadedSkillMetadata {
                requested_names: vec![display_name.to_string()],
                display_name: display_name.to_string(),
                canonical_id: format!(".agents/skills/{display_name}/SKILL.md"),
                source_origin: ".agents/skills".to_string(),
                source_path: format!(".agents/skills/{display_name}/SKILL.md"),
                load_reason: "explicit".to_string(),
            },
            content: content.to_string(),
        }
    }

    fn count_occurrences(haystack: &str, needle: &str) -> usize {
        haystack.match_indices(needle).count()
    }

    fn chat_item_text(item: &ChatItemView) -> String {
        let mut text = item.title.clone();
        if let Some(summary) = item.summary.as_deref() {
            text.push('\n');
            text.push_str(summary);
        }
        for line in &item.body {
            text.push('\n');
            text.push_str(&line.text);
        }
        text
    }

    // ── Session adoption (task_10) ──

    /// The `.atelier` data root for a workspace temp dir (where sessions live).
    fn atelier_root(dir: &Path) -> std::path::PathBuf {
        dir.join(".atelier")
    }

    /// Build a separate, fully-recorded session under the same `.atelier` root as
    /// `dir` and return its id. The `App` is dropped before returning (there is no
    /// `Drop` that writes events), leaving only the durable log for
    /// `LoadedSession::load` to replay — the realistic "adopt a session this
    /// process did not just write" path.
    async fn seed_session(dir: &Path, record: impl FnOnce(&mut App) -> Result<()>) -> String {
        let mut app = App::new_with_debug(fake_config(dir), false).await.unwrap();
        let id = app.history.session_id().to_string();
        record(&mut app).unwrap();
        id
    }

    fn sentinel_active_step(step_id: &str) -> ActiveStep {
        ActiveStep {
            run_id: "stale-run".into(),
            group_id: None,
            step_id: step_id.into(),
            step_label: None,
            file_scope: None,
            agent: "explorer".into(),
        }
    }

    #[tokio::test]
    async fn adopt_session_swaps_identity_goal_and_transcript() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                None,
                None,
                "session_goal_set",
                json!({ "goal": "Ship the browser" }),
                "Goal set.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "browse sessions", "submitted_prompt": "browse sessions", "source": "fresh" }),
                "browse sessions",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "all done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new_with_debug(fake_config(dir.path()), false)
            .await
            .unwrap();
        let original = app.state().session_id.clone();
        assert_ne!(original, session_b, "the two sessions must be distinct");

        let (sender, receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let loaded = LoadedSession::load(&atelier_root(dir.path()), &session_b).unwrap();
        app.adopt_session(loaded).unwrap();

        // Identity + goal + run state reflect the ADOPTED session, not app's own.
        assert_eq!(app.state().session_id, session_b);
        assert_eq!(app.history.session_id(), session_b);
        assert_eq!(
            app.state().session_goal.as_deref(),
            Some("Ship the browser")
        );
        assert_eq!(app.state().run_state, RunState::Idle);
        assert_eq!(app.state().active_run_id, None);

        // The transcript is the adopted log's fold; the welcome item stays first.
        assert!(
            app.state().chat_items.len() >= 2,
            "expected welcome + adopted items"
        );
        let transcript: String = app
            .state()
            .chat_items
            .iter()
            .map(chat_item_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            transcript.contains("browse sessions"),
            "adopted transcript missing the prompt: {transcript}"
        );

        // The swap broadcast carried the adopted state over the watch channel.
        assert_eq!(receiver.borrow().session_id, session_b);

        // A terminal last run needs no reconciliation event.
        let interrupted = app
            .history
            .read_events()
            .unwrap()
            .iter()
            .filter(|event| event.kind == RUN_INTERRUPTED_KIND)
            .count();
        assert_eq!(interrupted, 0);
    }

    #[tokio::test]
    async fn adopt_session_reconciles_a_dangling_run_to_idle() {
        let dir = tempdir().unwrap();
        // A run that started but never reached a terminal event (crash mid-run).
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "do work", "submitted_prompt": "do work", "source": "fresh" }),
                "do work",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )
        })
        .await;

        let mut app = App::new_with_debug(fake_config(dir.path()), false)
            .await
            .unwrap();
        let loaded = LoadedSession::load(&atelier_root(dir.path()), &session_b).unwrap();
        app.adopt_session(loaded).unwrap();

        // Resume always opens Idle (ADR-002), even reconciling a dangling run.
        assert_eq!(app.state().run_state, RunState::Idle);
        assert_eq!(app.state().active_run_id, None);

        // Exactly one terminal run_interrupted was appended to the ADOPTED log so
        // a future replay knows the run ended.
        let events = app.history.read_events().unwrap();
        let reconciliations: Vec<_> = events
            .iter()
            .filter(|event| event.kind == RUN_INTERRUPTED_KIND)
            .collect();
        assert_eq!(
            reconciliations.len(),
            1,
            "expected exactly one reconciliation event"
        );
        let payload: RunInterruptedPayload =
            serde_json::from_value(reconciliations[0].payload.clone()).unwrap();
        assert_eq!(payload.run_id, "run-b");
        assert_eq!(payload.prior_state, RunState::Running);
    }

    #[tokio::test]
    async fn adopt_session_does_not_reconcile_a_terminal_run() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "ok" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new_with_debug(fake_config(dir.path()), false)
            .await
            .unwrap();
        let loaded = LoadedSession::load(&atelier_root(dir.path()), &session_b).unwrap();
        app.adopt_session(loaded).unwrap();

        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == RUN_INTERRUPTED_KIND)
                .count(),
            0,
            "a cleanly-completed run must not be reconciled"
        );
        assert_eq!(app.state().run_state, RunState::Idle);
    }

    /// The exhaustiveness guard (ADR-006): mutate every session-scoped field to a
    /// sentinel, adopt a different session, and assert each was reset/replaced so
    /// no prior-session state leaks across the swap. The `AppState` subset is
    /// ALSO guaranteed at compile time by the full struct literal in
    /// `adopt_session` (a new field there is a compile error); these runtime
    /// assertions additionally cover the `App`-level fields the literal can't.
    #[tokio::test]
    async fn adopt_session_resets_every_session_scoped_field() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "adopted", "submitted_prompt": "adopted", "source": "fresh" }),
                "adopted",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "ok" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new_with_debug(fake_config(dir.path()), false)
            .await
            .unwrap();

        // ── Sentinel every App-level session-scoped field ──────────────────
        app.step_timings.insert(
            "step-x".into(),
            StepTiming {
                started_at: Instant::now(),
                last_activity: Instant::now(),
            },
        );
        app.pending_clarification = Some(PendingClarification {
            run: RunDriveContext::new("stale", None, "q", "q", None, None, None),
        });
        app.pending_governance_decision = Some(PendingGovernanceDecision {
            run: RunDriveContext::new("stale", None, "g", "g", None, None, None),
        });
        app.active_step = Some(sentinel_active_step("s1"));
        app.active_steps.push(sentinel_active_step("s2"));
        app.follow_up_queue
            .push_back(QueuedFollowUp::new("queued work"));
        app.trust_store
            .grant(TrustTarget::Command("rm -rf /".into()));
        app.session_ended = true;
        app.resume_approval_mode = Some(ApprovalMode::Normal);
        app.pending_drift_ack = Some(git::WorkspaceDrift {
            cwd_moved: true,
            head_changed: true,
        });
        // `pending_approval` (App) is omitted: a PendingApproval needs a full
        // PausedStep + RuntimeRequest + ActionExecutionContext. Its reset is the
        // explicit `self.pending_approval = None` in adopt_session, asserted below.

        // ── Sentinel the cheap AppState fields (the literal guards them all at
        //    compile time; these prove the runtime reset for the obvious ones) ──
        app.state.run_state = RunState::Running;
        app.state.active_run_id = Some("stale-run".into());
        app.state.session_goal = Some("stale goal".into());
        app.state.input = "half-typed".into();
        app.state.show_first_approval_explainer = true;
        app.state.pending_approval = Some(PendingApprovalView::default());
        app.state.pending_clarification = Some(PendingClarificationView {
            run_id: "stale".into(),
            question_id: "q".into(),
            question: "?".into(),
            options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
        });
        app.state.events.push("stale event".into());

        // ── Adopt a different session ──────────────────────────────────────
        let loaded = LoadedSession::load(&atelier_root(dir.path()), &session_b).unwrap();
        app.adopt_session(loaded).unwrap();

        // ── App-level fields reset ─────────────────────────────────────────
        assert!(app.step_timings.is_empty(), "step_timings not cleared");
        assert!(
            app.pending_approval.is_none(),
            "pending_approval (App) not cleared"
        );
        assert!(
            app.pending_clarification.is_none(),
            "pending_clarification (App) not cleared"
        );
        assert!(
            app.pending_governance_decision.is_none(),
            "pending_governance_decision (App) not cleared"
        );
        assert!(app.active_step.is_none(), "active_step not cleared");
        assert!(app.active_steps.is_empty(), "active_steps not cleared");
        assert!(
            app.follow_up_queue.is_empty(),
            "follow_up_queue not cleared"
        );
        assert!(app.trust_store.is_empty(), "trust_store not cleared");
        assert!(!app.session_ended, "session_ended not reset");
        assert!(
            app.resume_approval_mode.is_none(),
            "resume_approval_mode not reset"
        );
        assert!(
            app.pending_drift_ack.is_none(),
            "pending_drift_ack not reset"
        );

        // ── AppState session subset reset / replaced ───────────────────────
        assert_eq!(app.state().session_id, session_b);
        assert_eq!(app.state().run_state, RunState::Idle);
        assert_eq!(app.state().active_run_id, None);
        assert_eq!(app.state().session_goal, None, "stale goal leaked");
        assert!(app.state().input.is_empty(), "input not cleared");
        assert!(!app.state().show_first_approval_explainer);
        assert!(app.state().pending_approval.is_none());
        assert!(app.state().pending_clarification.is_none());
        assert!(app.state().pending_governance_decision.is_none());
        assert!(app.state().live_step.is_none());
        assert!(app.state().live_steps.is_empty());
        assert!(app.state().queued_follow_ups.is_empty());
        assert!(
            !app.state()
                .events
                .iter()
                .any(|event| event == "stale event"),
            "stale events leaked across the swap"
        );
    }

    // ── Resume flow (task_11) ──

    fn session_resumed_payload(app: &App) -> SessionResumedPayload {
        let events = app.history.read_events().unwrap();
        let event = events
            .iter()
            .find(|event| event.kind == SESSION_RESUMED_KIND)
            .expect("a session_resumed event was appended");
        serde_json::from_value(event.payload.clone()).unwrap()
    }

    #[tokio::test]
    async fn resume_appends_session_resumed_with_context() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/resume");
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b.clone()))
            .await
            .unwrap();

        let resumed = session_resumed_payload(&app);
        assert!(!resumed.resumed_at.is_empty(), "resumed_at recorded");
        // The live working directory (canonicalized by config load on macOS, where
        // tempdirs live under a `/private` symlink).
        assert_eq!(
            resumed.cwd, app.config.working_directory,
            "cwd is the live working directory"
        );
        assert!(
            resumed.head_sha.is_some(),
            "a git workspace records a HEAD baseline"
        );
        assert!(
            resumed.prior_tail_hash.is_some(),
            "a non-empty prior log records a tamper-evidence digest (ADR-007)"
        );
        // Terminal prior run ⇒ prior_end_state is the folded outcome.
        assert_eq!(resumed.prior_end_state, RunState::Completed);
        assert_eq!(app.state().run_state, RunState::Idle);
    }

    #[tokio::test]
    async fn resume_dangling_appends_run_interrupted_then_session_resumed() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "work", "submitted_prompt": "work", "source": "fresh" }),
                "work",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )
            // dangling: no terminal event
        })
        .await;

        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        // Both lifecycle events present, run_interrupted BEFORE session_resumed.
        let interrupted_at = events
            .iter()
            .position(|event| event.kind == RUN_INTERRUPTED_KIND)
            .expect("run_interrupted appended for the dangling run");
        let resumed_at = events
            .iter()
            .position(|event| event.kind == SESSION_RESUMED_KIND)
            .expect("session_resumed appended");
        assert!(
            interrupted_at < resumed_at,
            "the dangling run is closed before the resume boundary"
        );
        assert_eq!(app.state().run_state, RunState::Idle);
        // prior_end_state reflects the in-flight (non-terminal) run.
        assert_eq!(
            session_resumed_payload(&app).prior_end_state,
            RunState::Running
        );
    }

    #[tokio::test]
    async fn resume_terminal_session_appends_only_session_resumed() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "ok" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == RUN_INTERRUPTED_KIND)
                .count(),
            0,
            "a cleanly-terminal session is not reconciled"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == SESSION_RESUMED_KIND)
                .count(),
            1,
            "exactly one resume boundary"
        );
    }

    #[tokio::test]
    async fn resume_renders_full_prior_transcript() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "remembered prompt", "submitted_prompt": "remembered prompt", "source": "fresh" }),
                "remembered prompt",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "finished the work" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        // The live transcript re-renders the prior thread plus a resume divider.
        let transcript: String = app
            .state()
            .chat_items
            .iter()
            .map(chat_item_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            transcript.contains("remembered prompt"),
            "prior prompt re-rendered: {transcript}"
        );
        assert!(
            transcript.to_lowercase().contains("resum"),
            "a resume divider is rendered: {transcript}"
        );
    }

    #[tokio::test]
    async fn resume_is_rejected_while_a_run_is_active() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )
        })
        .await;

        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        let own_session = app.state().session_id.clone();
        // Simulate an in-flight run (the one-active-run guard).
        app.state.active_run_id = Some("live-run".into());

        app.handle_event(AppEvent::ResumeSession(session_b.clone()))
            .await
            .unwrap();

        // The guard refused: the session was NOT swapped and no resume boundary
        // was written.
        assert_eq!(app.state().session_id, own_session, "session not swapped");
        assert_ne!(app.history.session_id(), session_b);
        assert_eq!(
            app.history
                .read_events()
                .unwrap()
                .iter()
                .filter(|event| event.kind == SESSION_RESUMED_KIND)
                .count(),
            0,
            "no resume boundary written while a run is active"
        );
    }

    #[tokio::test]
    async fn resume_then_new_prompt_appends_to_the_same_log_and_drives_a_run() {
        let dir = tempdir().unwrap();
        // A session left dangling by a crash mid-run.
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "original work", "submitted_prompt": "original work", "source": "fresh" }),
                "original work",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )
        })
        .await;

        let mut app = App::new(fake_config(dir.path())).await.unwrap();

        // Resume → both lifecycle events land in the SAME events.jsonl.
        app.handle_event(AppEvent::ResumeSession(session_b.clone()))
            .await
            .unwrap();
        assert_eq!(app.history.session_id(), session_b, "adopted B's store");
        let after_resume = app.history.read_events().unwrap();
        assert!(after_resume.iter().any(|e| e.kind == RUN_INTERRUPTED_KIND));
        assert!(after_resume.iter().any(|e| e.kind == SESSION_RESUMED_KIND));
        let resume_event_count = after_resume.len();

        // A new prompt drives a fresh fake run that appends to the SAME log.
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
        assert_eq!(app.state().run_state, RunState::Completed);

        let final_events = app.history.read_events().unwrap();
        assert!(
            final_events.len() > resume_event_count,
            "the new run appended to the same log"
        );
        // The new run is distinct from the reconciled dangling one, completed in
        // B's log.
        let new_run = final_events
            .iter()
            .find(|event| {
                event.kind == "prompt_submitted" && event.payload["prompt"] == "create a feature"
            })
            .and_then(|event| event.run_id.clone())
            .expect("the new prompt started a run");
        assert_ne!(new_run, "run-b", "a fresh run id, not the reconciled one");
        assert!(
            final_events.iter().any(|event| {
                event.kind == "run_completed" && event.run_id.as_deref() == Some(new_run.as_str())
            }),
            "the new run completed in the adopted log"
        );
    }

    // ── Resume safety: cautious approval + drift interlock (task_12) ──

    fn short_head(dir: &Path) -> String {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(dir)
            .output()
            .expect("git available");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn write_action(action_id: &str) -> ActionRequest {
        ActionRequest {
            schema_version: 1,
            action_id: action_id.to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::WriteFile,
            params: json!({ "path": "out.txt", "content": "x" }),
        }
    }

    fn read_action(action_id: &str) -> ActionRequest {
        ActionRequest {
            schema_version: 1,
            action_id: action_id.to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ReadFile,
            params: json!({ "path": "in.txt" }),
        }
    }

    /// A `yolo` config so the resumed-session cautious override is observable
    /// (the global default would not require approval on its own).
    fn yolo_fake_config(dir: &Path) -> EffectiveConfig {
        let config = fake_config(dir);
        assert_eq!(
            config.approval_mode,
            ApprovalMode::Yolo,
            "fake_config must default to Yolo for these tests"
        );
        config
    }

    #[tokio::test]
    async fn resumed_session_forces_normal_approval_mode_over_yolo_config() {
        let dir = tempdir().unwrap();
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(yolo_fake_config(dir.path())).await.unwrap();
        // Before resume: the global Yolo config governs.
        assert_eq!(app.effective_approval_mode(), ApprovalMode::Yolo);

        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        // After resume: cautious Normal, regardless of the Yolo config, and the
        // resume boundary records it.
        assert_eq!(app.effective_approval_mode(), ApprovalMode::Normal);
        assert_eq!(session_resumed_payload(&app).approval_mode, "normal");
    }

    #[tokio::test]
    async fn resume_with_head_drift_arms_the_interlock() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/resume");
        // The session last ran at a HEAD that differs from the live one (branch
        // switch / rebase since it paused).
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b", "head_sha": "0000000" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(yolo_fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        let drift = app
            .pending_drift_ack
            .as_ref()
            .expect("HEAD drift arms the gate");
        assert!(drift.head_changed);
        assert!(!drift.cwd_moved, "same directory");
    }

    #[tokio::test]
    async fn resume_without_drift_has_no_interlock() {
        let dir = tempdir().unwrap();
        // Non-git workspace: no HEAD on either side, same cwd ⇒ no drift.
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(yolo_fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        assert!(
            app.pending_drift_ack.is_none(),
            "no drift ⇒ only the cautious approval applies, no extra gate"
        );
        // Still cautious, though.
        assert_eq!(app.effective_approval_mode(), ApprovalMode::Normal);
    }

    #[tokio::test]
    async fn resume_dirty_tree_but_same_head_is_not_drift() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/resume");
        let head = short_head(dir.path());
        // The session last ran at the CURRENT HEAD; the tree is dirty (the
        // untracked `.atelier/` dir at least), which must NOT count as drift.
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b", "head_sha": head }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(yolo_fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();

        assert!(
            app.pending_drift_ack.is_none(),
            "a dirty tree at the same HEAD is not drift (ADR-004)"
        );
    }

    #[tokio::test]
    async fn acknowledging_first_mutation_records_audit_and_clears_gate() {
        let dir = tempdir().unwrap();
        let mut app = App::new(yolo_fake_config(dir.path())).await.unwrap();
        app.pending_drift_ack = Some(git::WorkspaceDrift {
            cwd_moved: false,
            head_changed: true,
        });

        // A read-only action never satisfies the gate (the gate stays armed).
        app.acknowledge_resume_drift(&read_action("r1")).unwrap();
        assert!(
            app.pending_drift_ack.is_some(),
            "read-only action must not consume the drift gate"
        );

        // The first mutating action acknowledges: records the audit event + clears.
        app.acknowledge_resume_drift(&write_action("w1")).unwrap();
        assert!(app.pending_drift_ack.is_none(), "gate cleared after ack");
        let events = app.history.read_events().unwrap();
        let ack = events
            .iter()
            .find(|event| event.kind == RESUME_DRIFT_ACK_KIND)
            .expect("drift acknowledgment recorded in the audit log");
        assert_eq!(ack.payload["head_changed"], json!(true));
        assert_eq!(ack.payload["action_id"], json!("w1"));

        // A SECOND mutating action is not re-gated (gate already cleared) and adds
        // no further ack event.
        app.acknowledge_resume_drift(&write_action("w2")).unwrap();
        assert_eq!(
            app.history
                .read_events()
                .unwrap()
                .iter()
                .filter(|event| event.kind == RESUME_DRIFT_ACK_KIND)
                .count(),
            1,
            "the interlock fires once, not on every later mutation"
        );
    }

    #[test]
    fn drift_notice_is_folded_into_mutating_approval_prompts_only() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut context = ActionExecutionContext::new(
            config.working_directory.clone(),
            config.workspace.clone(),
            ApprovalMode::Normal,
        );
        context.drift_ack = Some("HEAD changed since this session paused".to_string());

        // A mutating action's prompt carries the drift context.
        let write_view = build_pending_approval_view(
            "run".to_string(),
            None,
            "step".to_string(),
            "fixer".to_string(),
            "writes a file".to_string(),
            None,
            &write_action("w1"),
            &context,
        );
        assert!(write_view.drift_notice.is_some());

        // A read-only action under the same armed gate shows no drift notice.
        let read_view = build_pending_approval_view(
            "run".to_string(),
            None,
            "step".to_string(),
            "explorer".to_string(),
            "reads a file".to_string(),
            None,
            &read_action("r1"),
            &context,
        );
        assert!(read_view.drift_notice.is_none());
    }

    #[tokio::test]
    async fn resume_with_drift_requires_acknowledgment_before_first_write() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/resume");
        // Session last ran at a different HEAD ⇒ drift on resume.
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b", "head_sha": "0000000" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let mut app = App::new(yolo_fake_config(dir.path())).await.unwrap();
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();
        assert!(app.pending_drift_ack.is_some(), "resume armed the gate");

        // A new prompt that drives the fixer to a write pauses for approval, and the
        // prompt carries the drift context — even though the global config is Yolo.
        app.submit_prompt("approval action create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        let pending = app
            .state
            .pending_approval
            .as_ref()
            .expect("the first mutation paused for approval");
        assert!(
            pending.drift_notice.is_some(),
            "the first write surfaces the drift context"
        );

        // Acknowledging (approving) records the audit event, clears the gate, and
        // lets the run proceed.
        app.handle_event(AppEvent::ApprovalAnswered(ApprovalResolution::ApproveOnce))
            .await
            .unwrap();
        assert!(app.pending_drift_ack.is_none(), "ack cleared the gate");
        assert!(
            app.history
                .read_events()
                .unwrap()
                .iter()
                .any(|event| event.kind == RESUME_DRIFT_ACK_KIND),
            "the acknowledgment is auditable in the log"
        );
    }

    // ── Resume-rate instrumentation + post-crash hint (task_13) ──

    #[tokio::test]
    async fn post_crash_hint_shown_when_newest_prior_session_is_non_terminal() {
        let dir = tempdir().unwrap();
        // A prior session left mid-flight (dangling ⇒ non-terminal outcome).
        let prior = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )
        })
        .await;

        let app = App::new(fake_config(dir.path())).await.unwrap();
        assert!(
            app.state().recoverable_session,
            "a fresh launch nudges recovery of the interrupted prior session"
        );
        // The outcome comes from list_session_summaries, not an ad-hoc scan.
        let summaries = list_session_summaries(&dir.path().join(".atelier"));
        let prior_outcome = summaries
            .iter()
            .find(|summary| summary.session_id == prior)
            .map(|summary| summary.outcome.clone())
            .unwrap();
        assert!(!prior_outcome.is_terminal());
    }

    #[tokio::test]
    async fn post_crash_hint_suppressed_when_newest_prior_session_completed() {
        let dir = tempdir().unwrap();
        // A prior session that ended cleanly: a normal quit must not look like a crash.
        seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_completed",
                json!({ "summary": "done" }),
                "Run completed.",
            )
        })
        .await;

        let app = App::new(fake_config(dir.path())).await.unwrap();
        assert!(
            !app.state().recoverable_session,
            "a cleanly-completed prior session shows no post-crash hint"
        );
    }

    #[tokio::test]
    async fn first_ever_session_shows_no_post_crash_hint() {
        let dir = tempdir().unwrap();
        // No prior session at all ⇒ nothing to recover.
        let app = App::new(fake_config(dir.path())).await.unwrap();
        assert!(!app.state().recoverable_session);
    }

    #[tokio::test]
    async fn resume_metrics_and_hint_reflect_a_crash_then_recovery() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        // A session left mid-flight (dangling crash).
        let session_b = seed_session(dir.path(), |app| {
            app.record_event(
                Some("run-b".into()),
                None,
                "prompt_submitted",
                json!({ "prompt": "work", "submitted_prompt": "work", "source": "fresh" }),
                "work",
            )?;
            app.record_event(
                Some("run-b".into()),
                None,
                "run_started",
                json!({ "run_id": "run-b" }),
                "Run started.",
            )
        })
        .await;

        // A fresh launch shows the post-crash hint…
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        assert!(app.state().recoverable_session, "hint shown until resumed");

        // …then the user resumes and completes it.
        app.handle_event(AppEvent::ResumeSession(session_b))
            .await
            .unwrap();
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
        assert_eq!(app.state().run_state, RunState::Completed);

        // The derived metrics reflect exactly one recovered + completed resume.
        let metrics = crate::history::resume_metrics(&root);
        assert_eq!(metrics.recovered, 1);
        assert_eq!(metrics.resumed_completed, 1);
        assert_eq!(metrics.crash_recovery_rate(), Some(1.0));
        assert_eq!(metrics.resumed_completion_rate(), Some(1.0));
    }

    fn runtime_skill_context(display_name: &str, content: &str) -> SkillPromptContext {
        SkillPromptContext {
            loaded: vec![test_loaded_skill(display_name, content)],
        }
    }

    #[test]
    fn run_drive_context_stores_prompt_provenance_and_skill_context_only_in_memory() {
        let skill_context = SkillPromptContext {
            loaded: vec![test_loaded_skill(
                "reviewer",
                "SENTINEL_FULL_SKILL_BODY_PRIVATE",
            )],
        };

        let run = RunDriveContext::new(
            "run",
            None,
            "/skill:reviewer inspect README",
            "inspect README",
            Some(skill_context),
            None,
            None,
        );

        assert_eq!(run.submitted_prompt, "/skill:reviewer inspect README");
        assert_eq!(run.prompt, "inspect README");
        assert!(!run.prompt.contains("/skill:reviewer"));
        assert!(!run.prompt.contains("SENTINEL_FULL_SKILL_BODY_PRIVATE"));
        assert!(!run.prompt.contains("<Skill:"));
        assert_eq!(
            run.skill_context
                .as_ref()
                .unwrap()
                .loaded
                .first()
                .unwrap()
                .content,
            "SENTINEL_FULL_SKILL_BODY_PRIVATE"
        );
    }

    #[test]
    fn skills_loaded_payload_serializes_loaded_skill_metadata_only() {
        let skill = test_loaded_skill("reviewer", "SENTINEL_FULL_SKILL_BODY_PRIVATE");

        let payload = skills_loaded_payload_from_metadata(&[skill.metadata]);

        let loaded = payload["skills"].as_array().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0]["requested_names"], json!(["reviewer"]));
        assert_eq!(loaded[0]["display_name"], "reviewer");
        assert_eq!(
            loaded[0]["canonical_id"],
            ".agents/skills/reviewer/SKILL.md"
        );
        assert_eq!(loaded[0]["source_origin"], ".agents/skills");
        assert_eq!(loaded[0]["source_path"], ".agents/skills/reviewer/SKILL.md");
        assert_eq!(loaded[0]["load_reason"], "explicit");
        assert!(!payload
            .to_string()
            .contains("SENTINEL_FULL_SKILL_BODY_PRIVATE"));
        assert!(loaded[0].get("content").is_none());
    }

    #[tokio::test]
    async fn runtime_request_renders_skill_context_once_for_runtime_prompt_shapes() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let app = App::new(config).await.unwrap();
        let skill_context =
            runtime_skill_context("reviewer", "SENTINEL_RUNTIME_SKILL_BODY_PRIVATE");
        let parallel_step = crate::orchestrator::ParallelChildStepPlan {
            step_label: "inspect docs".to_string(),
            agent: "explorer".to_string(),
            instruction: "Inspect README only.".to_string(),
            required_capabilities: vec![Capability::Read],
            file_scope: ParallelFileScope {
                write_files: Vec::new(),
                read_roots: vec![".".to_string()],
            },
        };
        let decision = crate::orchestrator::OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some(COUNCIL_WORKFLOW_AGENT_ID.to_string()),
            next_step: None,
            reason: "High-risk architecture review requested.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Council returns a recommendation.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };
        let council_member = app
            .config
            .council
            .presets
            .get("default")
            .unwrap()
            .get("architect")
            .unwrap();

        let cases = vec![
            (
                "orchestrator",
                "inspect README".to_string(),
                app.agent("orchestrator").unwrap().clone(),
                "orchestrator_decision",
                "inspect README",
            ),
            (
                "specialized",
                "inspect README".to_string(),
                app.agent("explorer").unwrap().clone(),
                "agent_result",
                "inspect README",
            ),
            (
                "parallel",
                parallel_child_prompt("inspect README", &parallel_step),
                app.agent("explorer").unwrap().clone(),
                "agent_result",
                "Parallel child step:",
            ),
            (
                "council",
                council_member_prompt("inspect README", &decision),
                council_member_agent("architect", council_member),
                "agent_result",
                "Council review request:",
            ),
            (
                "subtask",
                subtask_prompt("inspect README"),
                app.agent("explorer").unwrap().clone(),
                "agent_result",
                "Scope guard:",
            ),
        ];

        for (label, prompt, agent, output_schema, expected_prompt_text) in cases {
            let request = app
                .runtime_request(
                    "run",
                    &format!("step-{label}"),
                    RuntimePrompt::new(&prompt, Some(&skill_context)),
                    agent,
                    Vec::new(),
                    output_schema,
                )
                .unwrap();

            assert_eq!(
                count_occurrences(&request.prompt, "<Skill: reviewer"),
                1,
                "{label} request should render one skill section"
            );
            assert_eq!(
                count_occurrences(&request.prompt, "SENTINEL_RUNTIME_SKILL_BODY_PRIVATE"),
                1,
                "{label} request should render the skill body once"
            );
            assert!(request.prompt.contains("<User Prompt>"));
            assert!(request.prompt.contains(expected_prompt_text));
            let envelope = crate::runtime::prompt_envelope_json(&request).unwrap();
            assert_eq!(
                count_occurrences(&envelope, "<Skill: reviewer"),
                1,
                "{label} envelope should contain one rendered skill section"
            );
            assert_eq!(
                count_occurrences(&envelope, "SENTINEL_RUNTIME_SKILL_BODY_PRIVATE"),
                1,
                "{label} envelope should contain the skill body once"
            );
        }
    }

    #[test]
    fn derived_prompt_helpers_keep_skill_sections_out_of_helper_strings() {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "reviewer",
            Some("reviewer"),
            "SENTINEL_DERIVED_SKILL_BODY_PRIVATE",
        );
        let compiled = compile_app_prompt(dir.path(), "/skill:reviewer inspect README").unwrap();
        let parallel_step = crate::orchestrator::ParallelChildStepPlan {
            step_label: "inspect docs".to_string(),
            agent: "explorer".to_string(),
            instruction: "Inspect README only.".to_string(),
            required_capabilities: vec![Capability::Read],
            file_scope: ParallelFileScope {
                write_files: Vec::new(),
                read_roots: vec![".".to_string()],
            },
        };
        let decision = crate::orchestrator::OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some(COUNCIL_WORKFLOW_AGENT_ID.to_string()),
            next_step: None,
            reason: "High-risk architecture review requested.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Council returns a recommendation.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        let prompts = [
            parallel_child_prompt(&compiled.user_prompt, &parallel_step),
            council_member_prompt(&compiled.user_prompt, &decision),
            subtask_prompt(&compiled.user_prompt),
        ];

        for prompt in prompts {
            assert!(prompt.contains("inspect README"));
            assert!(!prompt.contains("/skill:reviewer"));
            assert!(!prompt.contains("<Skill:"));
            assert!(!prompt.contains("SENTINEL_DERIVED_SKILL_BODY_PRIVATE"));
        }
    }

    #[tokio::test]
    async fn prompt_submitted_keeps_skill_reference_in_visible_prompt() {
        // A `/skill:` prompt must render in chat as the user typed it — the
        // reference stays visible. Only the runtime prompt is stripped.
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "reviewer",
            Some("reviewer"),
            "SENTINEL_VISIBLE_SKILL_BODY_PRIVATE",
        );
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("/skill:reviewer inspect README")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let submitted = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .expect("prompt_submitted recorded");
        // Display keeps the reference; the full skill body never leaks into it.
        assert_eq!(
            submitted.payload["prompt"], "/skill:reviewer inspect README",
            "displayed prompt must keep the /skill: reference"
        );
        assert_eq!(
            submitted.payload["submitted_prompt"],
            "/skill:reviewer inspect README"
        );
        let displayed = submitted.payload["prompt"].as_str().unwrap();
        assert!(!displayed.contains("SENTINEL_VISIBLE_SKILL_BODY_PRIVATE"));
    }

    #[tokio::test]
    async fn action_authorization_uses_normalized_prompt_not_rendered_skill_body() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let app = App::new(config).await.unwrap();
        let agent = app.agent("fixer").unwrap().clone();
        let skill_context = runtime_skill_context(
            "reviewer",
            "This workflow body mentions commit, but it is not user intent.",
        );
        let request = app
            .runtime_request(
                "run",
                "step",
                RuntimePrompt::new("inspect README", Some(&skill_context)),
                agent.clone(),
                Vec::new(),
                "agent_result",
            )
            .unwrap();
        let action_request = ActionRequest {
            schema_version: 1,
            action_id: "commit".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::RunCommand,
            params: json!({ "command": "git commit -m test" }),
        };
        let normalized_context = ActionExecutionContext {
            working_directory: app.config.working_directory.clone(),
            workspace: app.config.workspace.clone(),
            approval_mode: app.config.approval_mode.clone(),
            command_timeout: None,
            user_prompt: Some("inspect README".to_string()),
            action_scope: crate::actions::ActionScope::Unrestricted,
            floor: app.config.approval.floor,
            trusted_targets: std::sync::Arc::new(std::collections::HashSet::new()),
            pre_approved: false,
            drift_ack: None,
            mcp: None,
        };
        let rendered_context = ActionExecutionContext {
            user_prompt: Some(request.prompt.clone()),
            ..normalized_context.clone()
        };

        assert!(request.prompt.contains("commit"));
        assert!(!action_executable_without_approval(
            &agent,
            &normalized_context,
            &action_request
        ));
        assert!(action_executable_without_approval(
            &agent,
            &rendered_context,
            &action_request
        ));
    }

    #[test]
    fn skill_load_error_diagnostic_includes_resolver_suggestions() {
        let error = SkillLoadError {
            requested_name: "revier".to_string(),
            kind: SkillLoadErrorKind::Unknown,
            suggestions: vec!["reviewer".to_string()],
        };

        let diagnostic = skill_load_diagnostic(&error);

        assert!(diagnostic.contains("failed to load skill reference"));
        assert!(diagnostic.contains("unknown skill 'revier'"));
        assert!(diagnostic.contains("Did you mean reviewer?"));
    }

    #[test]
    fn workflow_command_parser_extracts_prompt() {
        let command =
            parse_workflow_command("/workflow parallel scoped write action create a feature")
                .unwrap()
                .unwrap();

        assert_eq!(
            command.prompt,
            "parallel scoped write action create a feature"
        );
        assert_eq!(
            command.original_command,
            "/workflow parallel scoped write action create a feature"
        );
    }

    #[test]
    fn workflow_command_parser_rejects_empty_prompt() {
        for input in ["/workflow", "/workflow     "] {
            let error = parse_workflow_command(input).unwrap_err();

            assert!(error.to_string().contains(WORKFLOW_USAGE));
        }
    }

    #[test]
    fn workflow_command_parser_does_not_match_longer_slash_command() {
        assert!(parse_workflow_command("/workflowfoo create a feature")
            .unwrap()
            .is_none());
    }

    #[test]
    fn workflow_prompt_envelope_includes_user_prompt_and_evidence_requirements() {
        let command = WorkflowCommand {
            original_command: "/workflow parallel create a feature".to_string(),
            prompt: "parallel create a feature".to_string(),
        };
        let preflight = WorkflowPreflight {
            parallel_step_groups: true,
            max_parallel_agent_steps: 2,
        };

        let envelope = workflow_runtime_prompt(&command, &command.prompt, &preflight);

        assert!(envelope.contains("Workflow mode instructions"));
        assert!(envelope.contains("Decompose the user's broad request"));
        assert!(envelope.contains("Execute safe specialized-agent steps"));
        assert!(envelope.contains("Validate completed work"));
        assert!(envelope.contains("planned file-edit targets"));
        assert!(envelope.contains("verification evidence"));
        assert!(envelope.contains("Extracted user prompt:\nparallel create a feature"));
    }

    #[test]
    fn workflow_started_payload_includes_mode_and_preflight_details() {
        let start = WorkflowStart {
            command: WorkflowCommand {
                original_command: "/workflow migrate auth module".to_string(),
                prompt: "migrate auth module".to_string(),
            },
            preflight: WorkflowPreflight {
                parallel_step_groups: true,
                max_parallel_agent_steps: 2,
            },
        };

        let payload = workflow_started_payload("run-123", &start);

        assert_eq!(payload["run_id"], "run-123");
        assert_eq!(payload["original_command"], "/workflow migrate auth module");
        assert_eq!(payload["user_prompt"], "migrate auth module");
        assert_eq!(payload["mode"], "workflow");
        assert_eq!(payload["preflight"]["parallel_step_groups"], true);
        assert_eq!(payload["preflight"]["max_parallel_agent_steps"], 2);
    }

    fn test_workflow_context() -> WorkflowRunContext {
        WorkflowRunContext::new(&WorkflowCommand {
            original_command: "/workflow parallel create a feature".to_string(),
            prompt: "parallel create a feature".to_string(),
        })
    }

    fn workflow_test_scope() -> ParallelFileScope {
        ParallelFileScope {
            write_files: vec!["parallel-output/fixer-a.txt".to_string()],
            read_roots: vec!["src/runtime".to_string()],
        }
    }

    fn workflow_test_result(status: AgentResultStatus, summary: &str) -> AgentResult {
        let mut result = AgentResult::completed("fixer", "step-1", summary);
        result.status = status;
        result.summary = summary.to_string();
        result
    }

    fn workflow_test_result_with_blocker(
        status: AgentResultStatus,
        summary: &str,
        blocker: &str,
    ) -> AgentResult {
        let mut result = workflow_test_result(status, summary);
        result.blocker = Some(blocker.to_string());
        result
    }

    fn workflow_with_planned_test_target(
        config: &EffectiveConfig,
        scope: &ParallelFileScope,
    ) -> WorkflowRunContext {
        let mut workflow = test_workflow_context();
        workflow
            .record_planned_targets(
                "group-1",
                Some("step-1"),
                "fix scoped file",
                scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
    }

    fn first_workflow_target<'a>(
        workflow: &'a WorkflowRunContext,
        path: &str,
    ) -> &'a WorkflowTarget {
        workflow.target_ledger.get(path).unwrap().first().unwrap()
    }

    #[test]
    fn workflow_ledger_records_child_write_file_as_planned_target() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut workflow = test_workflow_context();
        let scope = ParallelFileScope {
            write_files: vec!["parallel-output/fixer-a.txt".to_string()],
            read_roots: vec!["src/runtime".to_string()],
        };

        let recorded = workflow
            .record_planned_targets(
                "group-1",
                Some("step-1"),
                "fix first scoped file",
                &scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        assert_eq!(recorded, 1);
        let targets = workflow
            .target_ledger
            .get("parallel-output/fixer-a.txt")
            .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "parallel-output/fixer-a.txt");
        assert_eq!(targets[0].source_group_id, "group-1");
        assert_eq!(targets[0].source_step_id.as_deref(), Some("step-1"));
        assert_eq!(targets[0].source_step_label, "fix first scoped file");
        assert_eq!(targets[0].status, WorkflowTargetStatus::Planned);
        assert_eq!(targets[0].reason, None);
    }

    #[test]
    fn workflow_ledger_ignores_read_only_child_without_write_files() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut workflow = test_workflow_context();
        let scope = ParallelFileScope {
            write_files: Vec::new(),
            read_roots: vec!["src/app".to_string()],
        };

        let recorded = workflow
            .record_planned_targets(
                "group-1",
                Some("step-review"),
                "review app scope",
                &scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        assert_eq!(recorded, 0);
        assert!(workflow.target_ledger.is_empty());
    }

    #[test]
    fn workflow_ledger_records_multiple_write_files_with_same_source_metadata() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut workflow = test_workflow_context();
        let scope = ParallelFileScope {
            write_files: vec![
                "parallel-output/fixer-a.txt".to_string(),
                "parallel-output/fixer-b.txt".to_string(),
            ],
            read_roots: vec!["src".to_string()],
        };

        let recorded = workflow
            .record_planned_targets(
                "group-1",
                Some("step-1"),
                "fix scoped files",
                &scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        assert_eq!(recorded, 2);
        for path in ["parallel-output/fixer-a.txt", "parallel-output/fixer-b.txt"] {
            let target = workflow.target_ledger.get(path).unwrap().first().unwrap();
            assert_eq!(target.source_group_id, "group-1");
            assert_eq!(target.source_step_id.as_deref(), Some("step-1"));
            assert_eq!(target.source_step_label, "fix scoped files");
            assert_eq!(target.status, WorkflowTargetStatus::Planned);
        }
    }

    #[test]
    fn workflow_target_keys_normalize_workspace_relative_paths() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());

        let key = normalize_workflow_target_key(
            " parallel-output//nested/fixer-a.txt ",
            &config.working_directory,
            &config.workspace.extra_write_roots,
        )
        .unwrap();

        assert_eq!(key, "parallel-output/nested/fixer-a.txt");
    }

    #[test]
    fn workflow_ledger_retains_duplicate_path_source_evidence_across_groups() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut workflow = test_workflow_context();
        let scope = ParallelFileScope {
            write_files: vec!["parallel-output/fixer-a.txt".to_string()],
            read_roots: vec!["src".to_string()],
        };

        workflow
            .record_planned_targets(
                "group-1",
                Some("step-1"),
                "first pass",
                &scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_planned_targets(
                "group-2",
                Some("step-2"),
                "second pass",
                &scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let targets = workflow
            .target_ledger
            .get("parallel-output/fixer-a.txt")
            .unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].source_group_id, "group-1");
        assert_eq!(targets[0].source_step_label, "first pass");
        assert_eq!(targets[1].source_group_id, "group-2");
        assert_eq!(targets[1].source_step_label, "second pass");
    }

    #[test]
    fn workflow_child_completed_marks_planned_targets_completed() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let scope = workflow_test_scope();
        let mut workflow = workflow_with_planned_test_target(&config, &scope);
        let result = workflow_test_result(AgentResultStatus::Completed, "done");

        workflow
            .record_child_result(
                "group-1",
                "step-1",
                &scope,
                &result,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let target = first_workflow_target(&workflow, "parallel-output/fixer-a.txt");
        assert_eq!(target.status, WorkflowTargetStatus::Completed);
        assert_eq!(target.reason, None);
    }

    #[test]
    fn workflow_child_no_changes_marks_planned_targets_completed() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let scope = workflow_test_scope();
        let mut workflow = workflow_with_planned_test_target(&config, &scope);
        let result = workflow_test_result(AgentResultStatus::NoChanges, "validated no changes");

        workflow
            .record_child_result(
                "group-1",
                "step-1",
                &scope,
                &result,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let target = first_workflow_target(&workflow, "parallel-output/fixer-a.txt");
        assert_eq!(target.status, WorkflowTargetStatus::Completed);
        assert_eq!(target.reason, None);
    }

    #[test]
    fn workflow_child_blocked_marks_planned_targets_blocked_with_reason() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let scope = workflow_test_scope();
        let mut workflow = workflow_with_planned_test_target(&config, &scope);
        let result = workflow_test_result_with_blocker(
            AgentResultStatus::Blocked,
            "blocked summary",
            "scope needs clarification",
        );

        workflow
            .record_child_result(
                "group-1",
                "step-1",
                &scope,
                &result,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let target = first_workflow_target(&workflow, "parallel-output/fixer-a.txt");
        assert_eq!(target.status, WorkflowTargetStatus::Blocked);
        assert_eq!(target.reason.as_deref(), Some("scope needs clarification"));
    }

    #[test]
    fn workflow_child_approval_denied_marks_planned_targets_blocked_with_reason() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let scope = workflow_test_scope();
        let mut workflow = workflow_with_planned_test_target(&config, &scope);
        let result = workflow_test_result_with_blocker(
            AgentResultStatus::ApprovalDenied,
            "approval stopped",
            "user denied action approval",
        );

        workflow
            .record_child_result(
                "group-1",
                "step-1",
                &scope,
                &result,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let target = first_workflow_target(&workflow, "parallel-output/fixer-a.txt");
        assert_eq!(target.status, WorkflowTargetStatus::Blocked);
        assert_eq!(
            target.reason.as_deref(),
            Some("user denied action approval")
        );
    }

    #[test]
    fn workflow_child_failed_statuses_mark_planned_targets_failed() {
        for status in [
            AgentResultStatus::Failed,
            AgentResultStatus::ParseError,
            AgentResultStatus::LimitReached,
            AgentResultStatus::Cancelled,
        ] {
            let dir = tempdir().unwrap();
            let config = fake_parallel_config(dir.path());
            let scope = workflow_test_scope();
            let mut workflow = workflow_with_planned_test_target(&config, &scope);
            let result =
                workflow_test_result_with_blocker(status, "failed summary", "terminal diagnostic");

            workflow
                .record_child_result(
                    "group-1",
                    "step-1",
                    &scope,
                    &result,
                    &config.working_directory,
                    &config.workspace.extra_write_roots,
                )
                .unwrap();

            let target = first_workflow_target(&workflow, "parallel-output/fixer-a.txt");
            assert_eq!(target.status, WorkflowTargetStatus::Failed);
            assert_eq!(target.reason.as_deref(), Some("terminal diagnostic"));
        }
    }

    #[test]
    fn workflow_completion_status_derives_completed_with_issues_for_terminal_unfinished_targets() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut workflow = test_workflow_context();
        let completed_scope = ParallelFileScope {
            write_files: vec!["parallel-output/fixer-a.txt".to_string()],
            read_roots: vec!["src/runtime".to_string()],
        };
        let blocked_scope = ParallelFileScope {
            write_files: vec!["parallel-output/fixer-b.txt".to_string()],
            read_roots: vec!["src/app".to_string()],
        };
        workflow
            .record_planned_targets(
                "group-1",
                Some("step-1"),
                "fix first scoped file",
                &completed_scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_planned_targets(
                "group-1",
                Some("step-2"),
                "fix second scoped file",
                &blocked_scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_child_result(
                "group-1",
                "step-1",
                &completed_scope,
                &workflow_test_result(AgentResultStatus::Completed, "done"),
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_child_result(
                "group-1",
                "step-2",
                &blocked_scope,
                &workflow_test_result_with_blocker(
                    AgentResultStatus::Blocked,
                    "blocked",
                    "blocked by dependency",
                ),
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let payload = workflow.completion_payload("run-1", false);

        assert_eq!(
            payload.status,
            WorkflowCompletionStatus::CompletedWithIssues
        );
        assert_eq!(payload.target_counts.completed, 1);
        assert_eq!(payload.target_counts.blocked, 1);
        assert_eq!(payload.unfinished_targets.len(), 1);
        assert_eq!(
            payload.unfinished_targets[0].path,
            "parallel-output/fixer-b.txt"
        );
    }

    #[test]
    fn workflow_completion_status_derives_failed_for_unaccounted_planned_targets() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let scope = workflow_test_scope();
        let workflow = workflow_with_planned_test_target(&config, &scope);

        let payload = workflow.completion_payload("run-1", false);

        assert_eq!(payload.status, WorkflowCompletionStatus::Failed);
        assert_eq!(payload.target_counts.planned, 1);
        assert_eq!(payload.unfinished_targets.len(), 1);
        assert_eq!(
            payload.unfinished_targets[0].reason,
            "planned target did not receive terminal workflow evidence"
        );
    }

    #[test]
    fn workflow_completion_status_completes_when_no_targets_were_declared() {
        // Orchestrator-driven runs that write via single-agent actions (or make
        // no edits) declare no parallel file-scope targets. A clean, finished
        // run with zero targets is a success, not a failure.
        let counts = WorkflowTargetCounts {
            planned: 0,
            completed: 0,
            skipped: 0,
            blocked: 0,
            failed: 0,
        };

        let status = derive_workflow_completion_status(&counts, &[], false);

        assert_eq!(status, WorkflowCompletionStatus::Completed);
    }

    #[test]
    fn workflow_target_key_normalizes_external_write_root_without_erroring() {
        // Regression: an absolute write_file under an extra write root OUTSIDE
        // the workspace previously aborted the whole workflow because the key
        // was stripped against the working directory (not a prefix). It must
        // normalize against the matching root instead.
        let working_directory = Path::new("/workspace/repo");
        let extra_write_roots = vec![PathBuf::from("/var/out")];

        let key = normalize_workflow_target_key(
            "/var/out/reports/summary.md",
            working_directory,
            &extra_write_roots,
        )
        .unwrap();

        assert_eq!(key, "reports/summary.md");
    }

    #[test]
    fn workflow_target_key_rejects_absolute_path_under_no_allowed_root() {
        let working_directory = Path::new("/workspace/repo");

        let error = normalize_workflow_target_key(
            "/etc/passwd",
            working_directory,
            &[PathBuf::from("/var/out")],
        )
        .unwrap_err();

        assert!(error.to_string().contains("absolute paths are not allowed"));
    }

    #[test]
    fn workflow_completion_payload_includes_unfinished_blocked_and_failed_reasons() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut workflow = test_workflow_context();
        let blocked_scope = ParallelFileScope {
            write_files: vec!["parallel-output/blocked.txt".to_string()],
            read_roots: vec!["src/runtime".to_string()],
        };
        let failed_scope = ParallelFileScope {
            write_files: vec!["parallel-output/failed.txt".to_string()],
            read_roots: vec!["src/app".to_string()],
        };
        workflow
            .record_planned_targets(
                "group-1",
                Some("step-blocked"),
                "blocked scoped file",
                &blocked_scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_planned_targets(
                "group-1",
                Some("step-failed"),
                "failed scoped file",
                &failed_scope,
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_child_result(
                "group-1",
                "step-blocked",
                &blocked_scope,
                &workflow_test_result_with_blocker(
                    AgentResultStatus::Blocked,
                    "blocked",
                    "waiting on user decision",
                ),
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();
        workflow
            .record_child_result(
                "group-1",
                "step-failed",
                &failed_scope,
                &workflow_test_result_with_blocker(
                    AgentResultStatus::Failed,
                    "failed",
                    "test command failed",
                ),
                &config.working_directory,
                &config.workspace.extra_write_roots,
            )
            .unwrap();

        let payload = workflow.completion_payload("run-1", false);
        let reasons = payload
            .unfinished_targets
            .iter()
            .map(|target| (target.path.as_str(), target.reason.as_str()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            reasons.get("parallel-output/blocked.txt").copied(),
            Some("waiting on user decision")
        );
        assert_eq!(
            reasons.get("parallel-output/failed.txt").copied(),
            Some("test command failed")
        );
    }

    fn fake_parallel_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[features]
parallel_step_groups = true

[limits]
max_parallel_agent_steps = 2

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn fake_dag_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[features]
execution_graph = true

[limits]
max_parallel_agent_steps = 2

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    // ── DAG ready-set admission (task_04) ──

    fn dag_snapshot(
        node_id: &str,
        run_state: NodeRunState,
        succeeded: bool,
        is_command: bool,
        writes: &[&str],
        preds: &[&str],
    ) -> DagNodeSnapshot {
        DagNodeSnapshot {
            node_id: node_id.to_string(),
            run_state,
            succeeded,
            is_command,
            write_files: writes.iter().map(|w| w.to_string()).collect(),
            predecessors: preds.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn admission_promotes_disjoint_dependents_in_one_pass() {
        // A done-succeeded; B and C depend on A with disjoint writes → both ready.
        let nodes = vec![
            dag_snapshot("a", NodeRunState::Terminal, true, false, &["a.rs"], &[]),
            dag_snapshot("b", NodeRunState::Pending, false, false, &["b.rs"], &["a"]),
            dag_snapshot("c", NodeRunState::Pending, false, false, &["c.rs"], &["a"]),
        ];
        let decision = compute_admission(&nodes, 8);
        assert_eq!(decision.admit, vec!["b".to_string(), "c".to_string()]);
        assert!(decision.skip.is_empty());
    }

    #[test]
    fn admission_write_overlap_holds_second_node() {
        // B and C share a write file; only one is admitted, the other waits.
        let nodes = vec![
            dag_snapshot("a", NodeRunState::Terminal, true, false, &["a.rs"], &[]),
            dag_snapshot(
                "b",
                NodeRunState::Pending,
                false,
                false,
                &["shared.rs"],
                &["a"],
            ),
            dag_snapshot(
                "c",
                NodeRunState::Pending,
                false,
                false,
                &["shared.rs"],
                &["a"],
            ),
        ];
        let decision = compute_admission(&nodes, 8);
        assert_eq!(decision.admit, vec!["b".to_string()]);
        assert!(decision.skip.is_empty());
    }

    #[test]
    fn admission_command_node_waits_while_others_run() {
        // A command-capable ready node cannot be admitted while anything runs.
        let nodes = vec![
            dag_snapshot("x", NodeRunState::Running, false, false, &["x.rs"], &[]),
            dag_snapshot("y", NodeRunState::Pending, false, true, &[], &[]),
        ];
        let decision = compute_admission(&nodes, 8);
        assert!(decision.admit.is_empty());
    }

    #[test]
    fn admission_command_node_runs_in_isolation() {
        // Nothing running; a command node and a non-command node are both ready.
        // The command node monopolizes: exactly one node is admitted.
        let nodes = vec![
            dag_snapshot("y", NodeRunState::Pending, false, true, &[], &[]),
            dag_snapshot("z", NodeRunState::Pending, false, false, &["z.rs"], &[]),
        ];
        let decision = compute_admission(&nodes, 8);
        assert_eq!(decision.admit, vec!["y".to_string()]);
    }

    #[test]
    fn admission_fail_closed_skips_dependent_of_failed_node() {
        // A failed → its dependent D is skipped, never admitted.
        let nodes = vec![
            dag_snapshot("a", NodeRunState::Terminal, false, false, &["a.rs"], &[]),
            dag_snapshot("d", NodeRunState::Pending, false, false, &["d.rs"], &["a"]),
        ];
        let decision = compute_admission(&nodes, 8);
        assert!(decision.admit.is_empty());
        assert_eq!(decision.skip, vec!["d".to_string()]);
    }

    #[test]
    fn admission_forward_progress_cascades_skip_through_tail() {
        // A failed; B→C→D are all downstream. One admission call skips the whole
        // tail so the join loop drains to all-terminal (no deadlock).
        let nodes = vec![
            dag_snapshot("a", NodeRunState::Terminal, false, false, &["a.rs"], &[]),
            dag_snapshot("b", NodeRunState::Pending, false, false, &["b.rs"], &["a"]),
            dag_snapshot("c", NodeRunState::Pending, false, false, &["c.rs"], &["b"]),
            dag_snapshot("d", NodeRunState::Pending, false, false, &["d.rs"], &["c"]),
        ];
        let decision = compute_admission(&nodes, 8);
        assert!(decision.admit.is_empty());
        assert_eq!(
            decision.skip,
            vec!["b".to_string(), "c".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn admission_pending_writes_do_not_pre_occupy_running_set() {
        // Two ready nodes share a write file, nothing running. If a Pending node's
        // writes were wrongly counted as "running", neither would admit
        // (deadlock); the correct behavior admits the first and holds the second.
        let nodes = vec![
            dag_snapshot(
                "a",
                NodeRunState::Pending,
                false,
                false,
                &["shared.rs"],
                &[],
            ),
            dag_snapshot(
                "b",
                NodeRunState::Pending,
                false,
                false,
                &["shared.rs"],
                &[],
            ),
        ];
        let decision = compute_admission(&nodes, 8);
        assert_eq!(decision.admit, vec!["a".to_string()]);
    }

    #[test]
    fn admission_respects_the_max_concurrent_ceiling() {
        // Four independent, write-disjoint, ready nodes but a ceiling of 2: only
        // two are admitted in one pass (the reused max_parallel_agent_steps bound).
        let nodes = vec![
            dag_snapshot("a", NodeRunState::Pending, false, false, &["a.rs"], &[]),
            dag_snapshot("b", NodeRunState::Pending, false, false, &["b.rs"], &[]),
            dag_snapshot("c", NodeRunState::Pending, false, false, &["c.rs"], &[]),
            dag_snapshot("d", NodeRunState::Pending, false, false, &["d.rs"], &[]),
        ];
        let decision = compute_admission(&nodes, 2);
        assert_eq!(decision.admit, vec!["a".to_string(), "b".to_string()]);

        // With one node already running, only one more slot remains.
        let nodes = vec![
            dag_snapshot("a", NodeRunState::Running, false, false, &["a.rs"], &[]),
            dag_snapshot("b", NodeRunState::Pending, false, false, &["b.rs"], &[]),
            dag_snapshot("c", NodeRunState::Pending, false, false, &["c.rs"], &[]),
        ];
        let decision = compute_admission(&nodes, 2);
        assert_eq!(decision.admit, vec!["b".to_string()]);
    }

    #[tokio::test]
    async fn dag_decision_routes_through_scheduler_and_completes() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_dag_config(dir.path())).await.unwrap();
        let mut run = RunDriveContext::new(
            "run-dag",
            None,
            "graph work",
            "graph work",
            None,
            None,
            None,
        );
        let node = |id: &str, label: &str| crate::orchestrator::ExecutionNode {
            node_id: id.to_string(),
            step_label: label.to_string(),
            agent: "explorer".to_string(),
            instruction: format!("inspect {label}"),
            required_capabilities: vec![Capability::Read],
            file_scope: ParallelFileScope {
                write_files: Vec::new(),
                read_roots: vec!["src".to_string()],
            },
        };
        let graph = crate::orchestrator::ExecutionGraph {
            graph_id: "graph-1".to_string(),
            reason: "two read-only nodes in sequence".to_string(),
            nodes: vec![node("a", "alpha"), node("b", "beta")],
            edges: vec![crate::orchestrator::ExecutionEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                kind: crate::orchestrator::ExecutionEdgeKind::DataDependency,
            }],
        };
        let decision = crate::orchestrator::OrchestratorDecision {
            schema_version: 3,
            decision_id: "decision-dag".to_string(),
            run_id: "run-dag".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Run the graph.".to_string()],
            next_agent: None,
            next_step: Some(DecisionNextStep::Dag(graph)),
            reason: "The work forms a dependency graph.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Graph completes.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        let routed = app
            .handle_orchestrator_decision(&mut run, decision)
            .await
            .unwrap();

        assert!(routed, "a completed graph routes as a successful step");
        // Routed to the scheduler (not the single-agent or parallel-group path).
        assert!(run
            .previous_results
            .iter()
            .any(|result| matches!(result, RunStepResult::Dag { .. })));
        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "node_succeeded"
                    && event.graph_id.as_deref() == Some("graph-1"))
                .count(),
            2
        );
        assert!(events.iter().any(|event| {
            event.kind == "execution_graph_completed"
                && event.graph_id.as_deref() == Some("graph-1")
        }));
        // The DAG path must not leak flat parallel-group vocabulary.
        assert!(!events
            .iter()
            .any(|event| event.kind == "parallel_group_started"));
    }

    // ── whole-plan approval gate (task_05) ──

    fn fake_dag_normal_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[features]
execution_graph = true

[limits]
max_parallel_agent_steps = 2

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn simple_dag_graph(graph_id: &str) -> crate::orchestrator::ExecutionGraph {
        let node = |id: &str, label: &str| crate::orchestrator::ExecutionNode {
            node_id: id.to_string(),
            step_label: label.to_string(),
            agent: "explorer".to_string(),
            instruction: format!("inspect {label}"),
            required_capabilities: vec![Capability::Read],
            file_scope: ParallelFileScope {
                write_files: Vec::new(),
                read_roots: vec!["src".to_string()],
            },
        };
        crate::orchestrator::ExecutionGraph {
            graph_id: graph_id.to_string(),
            reason: "two read-only nodes in sequence".to_string(),
            nodes: vec![node("a", "alpha"), node("b", "beta")],
            edges: vec![crate::orchestrator::ExecutionEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                kind: crate::orchestrator::ExecutionEdgeKind::DataDependency,
            }],
        }
    }

    fn dag_continue_decision_for(
        graph: crate::orchestrator::ExecutionGraph,
    ) -> crate::orchestrator::OrchestratorDecision {
        crate::orchestrator::OrchestratorDecision {
            schema_version: 3,
            decision_id: "decision-dag".to_string(),
            run_id: "run-dag".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Run the graph.".to_string()],
            next_agent: None,
            next_step: Some(DecisionNextStep::Dag(graph)),
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
    fn plan_question_id_is_deterministic_from_graph_id() {
        assert_eq!(plan_question_id("graph-1"), plan_question_id("graph-1"));
        assert_ne!(plan_question_id("graph-1"), plan_question_id("graph-2"));
        assert!(plan_question_id("graph-1").contains("graph-1"));
    }

    #[tokio::test]
    async fn dag_normal_mode_pauses_for_plan_approval_without_spawning() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_dag_normal_config(dir.path())).await.unwrap();
        let mut run = RunDriveContext::new(
            "run-dag",
            None,
            "graph work",
            "graph work",
            None,
            None,
            None,
        );

        let cont = app
            .handle_orchestrator_decision(
                &mut run,
                dag_continue_decision_for(simple_dag_graph("graph-1")),
            )
            .await
            .unwrap();

        assert!(!cont, "normal mode pauses until the plan is accepted");
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        let plan = app
            .state
            .pending_plan_approval
            .as_ref()
            .expect("plan gate pending");
        assert_eq!(plan.graph_id, "graph-1");
        assert_eq!(plan.question_id, plan_question_id("graph-1"));
        // The plan gate uses a distinct slot; the per-action slot is untouched.
        assert!(app.state.pending_approval.is_none());
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "execution_graph_proposed" && event.graph_id.as_deref() == Some("graph-1")
        }));
        // No node spawned, no approval yet.
        assert!(!events.iter().any(|event| event.kind == "node_running"));
        assert!(!events
            .iter()
            .any(|event| event.kind == "execution_graph_approved"));
    }

    #[tokio::test]
    async fn dag_yolo_mode_auto_accepts_and_runs() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_dag_config(dir.path())).await.unwrap();
        let mut run = RunDriveContext::new(
            "run-dag",
            None,
            "graph work",
            "graph work",
            None,
            None,
            None,
        );

        let cont = app
            .handle_orchestrator_decision(
                &mut run,
                dag_continue_decision_for(simple_dag_graph("graph-1")),
            )
            .await
            .unwrap();

        assert!(cont);
        assert!(
            app.state.pending_plan_approval.is_none(),
            "yolo never gates"
        );
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "execution_graph_approved"
                && event.payload["auto_accepted"] == json!(true)
        }));
        assert!(events
            .iter()
            .any(|event| event.kind == "execution_graph_completed"));
        assert!(run
            .previous_results
            .iter()
            .any(|result| matches!(result, RunStepResult::Dag { .. })));
    }

    #[tokio::test]
    async fn dag_plan_reject_emits_rejected_with_reason_and_spawns_nothing() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_dag_normal_config(dir.path())).await.unwrap();
        let mut run = RunDriveContext::new(
            "run-dag",
            None,
            "what is the status?",
            "what is the status?",
            None,
            None,
            None,
        );
        app.handle_orchestrator_decision(
            &mut run,
            dag_continue_decision_for(simple_dag_graph("graph-1")),
        )
        .await
        .unwrap();
        assert!(app.state.pending_plan_approval.is_some());

        app.resolve_pending_plan_approval(
            &plan_question_id("graph-1"),
            PlanApprovalAnswer::Reject {
                reason: Some("too risky".to_string()),
            },
        )
        .await
        .unwrap();

        let events = app.history.read_events().unwrap();
        let rejected = events
            .iter()
            .find(|event| event.kind == "execution_graph_rejected")
            .expect("rejected event recorded");
        assert_eq!(rejected.payload["reason"], json!("too risky"));
        // Rejection returns the plan to the orchestrator; no graph node ran.
        assert!(!events.iter().any(|event| event.kind == "node_running"));
        assert!(app.state.pending_plan_approval.is_none());
    }

    #[tokio::test]
    async fn dag_plan_accept_runs_the_graph() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_dag_normal_config(dir.path())).await.unwrap();
        let mut run = RunDriveContext::new(
            "run-dag",
            None,
            "graph work",
            "graph work",
            None,
            None,
            None,
        );
        app.handle_orchestrator_decision(
            &mut run,
            dag_continue_decision_for(simple_dag_graph("graph-1")),
        )
        .await
        .unwrap();
        assert!(app.state.pending_plan_approval.is_some());

        app.resolve_pending_plan_approval(&plan_question_id("graph-1"), PlanApprovalAnswer::Accept)
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "execution_graph_approved"
                && event.payload["auto_accepted"] == json!(false)
        }));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "node_succeeded"
                    && event.graph_id.as_deref() == Some("graph-1"))
                .count(),
            2
        );
        assert!(events
            .iter()
            .any(|event| event.kind == "execution_graph_completed"));
        assert!(app.state.pending_plan_approval.is_none());
    }

    fn assert_no_workflow_run_start_events(events: &[HistoryEvent]) {
        assert!(events.iter().all(|event| {
            event.kind != "run_started"
                && event.kind != "workflow_started"
                && event.kind != "prompt_submitted"
        }));
    }

    fn workflow_completed_event(events: &[HistoryEvent]) -> &HistoryEvent {
        events
            .iter()
            .find(|event| event.kind == "workflow_completed")
            .unwrap()
    }

    fn fake_parallel_normal_approval_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[features]
parallel_step_groups = true

[limits]
max_parallel_agent_steps = 2

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn fake_parallel_low_agent_step_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[features]
parallel_step_groups = true

[limits]
max_agent_steps = 2
max_parallel_agent_steps = 2

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn fake_parallel_reviewer_parse_error_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[features]
parallel_step_groups = true

[limits]
max_parallel_agent_steps = 2

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
instructions = "fake parse error"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn fake_config_with_council_prompts(
        dir: &std::path::Path,
        architect_prompt: &str,
        security_prompt: &str,
        reviewer_prompt: &str,
    ) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            format!(
                r#"
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"

[council]
default_preset = "default"
timeout_seconds = 5
execution_mode = "serial"

[council.presets.default.architect]
runtime = "fake"
model = "default"
prompt = "{architect_prompt}"

[council.presets.default.security]
runtime = "fake"
model = "default"
prompt = "{security_prompt}"

[council.presets.default.reviewer]
runtime = "fake"
model = "default"
prompt = "{reviewer_prompt}"
"#
            ),
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn agent_status(app: &App, agent_id: &str) -> String {
        app.state
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .map(|agent| agent.status.clone())
            .unwrap()
    }

    fn agent_status_from_state(state: &AppState, agent_id: &str) -> String {
        state
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .map(|agent| agent.status.clone())
            .unwrap()
    }

    #[tokio::test]
    async fn roster_places_orchestrator_first() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let app = App::new(config).await.unwrap();

        assert_eq!(
            app.state.agents.first().map(|agent| agent.id.as_str()),
            Some("orchestrator")
        );
    }

    #[tokio::test]
    async fn fake_runtime_completes_code_change_loop() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("create a feature").await.unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.state.events.join("\n");
        assert!(events.contains("You: create a feature"));
        assert!(events.contains("explorer:"));
        assert!(events.contains("fixer:"));
        assert!(events.contains("reviewer:"));
        assert!(app
            .state
            .agents
            .iter()
            .filter(|agent| agent.status != "disabled")
            .all(|agent| agent.status == "idle"));
        let history_events = app.history.read_events().unwrap();
        assert!(history_events
            .iter()
            .any(|event| event.kind == "runtime_stream_delta"));
        assert!(dir.path().join(".atelier/runs").exists());
    }

    // ── lifecycle hooks: event tap + dispatcher wiring (task_05) ──

    /// A fake-runtime config whose agents all use `runtime_id` (type `fake`),
    /// with `hooks_toml` appended as a user-scope `[hooks]` section (honored, not
    /// dropped, because it is loaded as the explicit `--config` layer).
    fn fake_config_with_hooks(dir: &Path, runtime_id: &str, hooks_toml: &str) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        let toml = format!(
            "[runtimes.{rid}]\ntype = \"fake\"\n\
             [agents.orchestrator]\nruntime = \"{rid}\"\n\
             [agents.explorer]\nruntime = \"{rid}\"\n\
             [agents.fixer]\nruntime = \"{rid}\"\n\
             [agents.reviewer]\nruntime = \"{rid}\"\n\
             [agents.oracle]\nruntime = \"{rid}\"\n\
             [agents.consul]\nruntime = \"{rid}\"\n\
             [council.presets.default.architect]\nruntime = \"{rid}\"\n\
             [council.presets.default.security]\nruntime = \"{rid}\"\n\
             [council.presets.default.reviewer]\nruntime = \"{rid}\"\n\
             {hooks}",
            rid = runtime_id,
            hooks = hooks_toml,
        );
        fs::write(&config_path, toml).unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn command_hook_handler(public_event: &str, command: &str) -> crate::hooks::HookHandler {
        crate::hooks::HookHandler {
            on: vec![public_event.to_string()],
            action: crate::hooks::HookAction::Command(command.to_string()),
            payload: PayloadDetail::Metadata,
        }
    }

    struct NoopNotifier;
    impl crate::hooks::Notifier for NoopNotifier {
        fn notify(&self, _title: &str, _body: &str) -> Result<()> {
            Ok(())
        }
    }

    /// Wire `app`'s tap to a freshly-spawned dispatcher and return the lifecycle
    /// back-channel receiver (the records `run_tui` would forward to the worker).
    fn wire_hook_dispatcher(
        app: &mut App,
        notifier: Arc<dyn crate::hooks::Notifier>,
    ) -> mpsc::Receiver<HookLifecycleRecord> {
        let (hook_tx, hook_rx) = crate::hooks::hook_channel();
        let (life_tx, life_rx) = mpsc::channel(256);
        app.attach_hook_sender(hook_tx, DroppedHookCounter::new());
        tokio::spawn(crate::hooks::run_hook_dispatcher(
            hook_rx,
            life_tx,
            notifier,
            crate::hooks::DEFAULT_HOOK_TIMEOUT,
        ));
        life_rx
    }

    #[tokio::test]
    async fn tap_does_no_dispatch_when_no_hooks_configured() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        // A sender is wired, but no handlers are configured.
        let (tx, mut rx) = crate::hooks::hook_channel();
        app.attach_hook_sender(tx, DroppedHookCounter::new());
        app.record_event(
            Some("run-1".into()),
            None,
            "run_completed",
            json!({}),
            "done",
        )
        .unwrap();
        assert!(rx.try_recv().is_err(), "no item should be dispatched");
        assert_eq!(app.dropped_hook_count(), 0);
    }

    #[tokio::test]
    async fn tap_resolves_actor_runtime_from_active_step_agent() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.config
            .hooks
            .handlers
            .push(command_hook_handler("step_started", "true"));
        let (tx, mut rx) = crate::hooks::hook_channel();
        app.attach_hook_sender(tx, DroppedHookCounter::new());
        app.active_step = Some(ActiveStep {
            run_id: "run-1".into(),
            group_id: None,
            step_id: "step-1".into(),
            step_label: None,
            file_scope: None,
            agent: "explorer".into(),
        });
        app.record_event_with_group(
            Some("run-1".into()),
            None,
            Some("step-1".into()),
            "agent_step_started",
            json!({ "agent": "explorer" }),
            "started",
        )
        .unwrap();
        let item = rx.try_recv().expect("a dispatch item");
        assert_eq!(item.payload.event, "step_started");
        assert_eq!(item.payload.actor.agent.as_deref(), Some("explorer"));
        // fake_config's explorer agent uses runtime "fake".
        assert_eq!(item.payload.actor.runtime.as_deref(), Some("fake"));
    }

    #[tokio::test]
    async fn tap_increments_dropped_counter_on_full_channel_without_blocking() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.config
            .hooks
            .handlers
            .push(command_hook_handler("run_completed", "true"));
        // Capacity 1, never drained: the first fits, the second is dropped.
        let (tx, _rx) = mpsc::channel(1);
        let dropped = DroppedHookCounter::new();
        app.attach_hook_sender(tx, dropped.clone());
        app.record_event(Some("run-1".into()), None, "run_completed", json!({}), "a")
            .unwrap();
        app.record_event(Some("run-1".into()), None, "run_completed", json!({}), "b")
            .unwrap();
        assert_eq!(dropped.count(), 1);
    }

    #[tokio::test]
    async fn tap_ignores_kinds_outside_public_vocabulary() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.config
            .hooks
            .handlers
            .push(command_hook_handler("run_completed", "true"));
        let (tx, mut rx) = crate::hooks::hook_channel();
        app.attach_hook_sender(tx, DroppedHookCounter::new());
        // `runtime_stream_delta` is not in the public vocabulary → no dispatch.
        app.record_event(
            Some("run-1".into()),
            None,
            "runtime_stream_delta",
            json!({ "agent": "explorer", "content": "x" }),
            "delta",
        )
        .unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn end_to_end_command_hook_writes_normalized_payload() {
        let dir = tempdir().unwrap();
        let sentinel = dir.path().join("hook.json");
        let hooks = format!(
            "[[hooks.handler]]\non = \"run_completed\"\ncommand = \"cat > '{}'\"\n",
            sentinel.display()
        );
        let config = fake_config_with_hooks(dir.path(), "fake", &hooks);
        let mut app = App::new(config).await.unwrap();
        let mut life_rx = wire_hook_dispatcher(&mut app, Arc::new(NoopNotifier));

        app.submit_prompt("create a feature").await.unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);

        // hook_started arrives, then hook_completed once the command has run.
        let _started = tokio::time::timeout(Duration::from_secs(5), life_rx.recv())
            .await
            .expect("hook_started within timeout")
            .expect("hook_started present");
        let _completed = tokio::time::timeout(Duration::from_secs(5), life_rx.recv())
            .await
            .expect("hook_completed within timeout")
            .expect("hook_completed present");

        let written = std::fs::read_to_string(&sentinel).expect("hook wrote the sentinel");
        assert!(
            written.contains("\"event\":\"run_completed\""),
            "payload: {written}"
        );
        assert!(
            written.contains("\"outcome\":\"success\""),
            "payload: {written}"
        );
    }

    #[tokio::test]
    async fn cross_runtime_run_completed_payload_shape_is_uniform() {
        async fn captured_run_completed(dir: &Path, runtime_id: &str) -> crate::hooks::HookPayload {
            let config = fake_config_with_hooks(
                dir,
                runtime_id,
                "[[hooks.handler]]\non = \"run_completed\"\nnotify = true\n",
            );
            let mut app = App::new(config).await.unwrap();
            let (tx, mut rx) = crate::hooks::hook_channel();
            app.attach_hook_sender(tx, DroppedHookCounter::new());
            app.submit_prompt("create a feature").await.unwrap();
            let mut found = None;
            while let Ok(item) = rx.try_recv() {
                if item.payload.event == "run_completed" {
                    found = Some(item.payload);
                }
            }
            found.expect("run_completed dispatched")
        }

        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let a = captured_run_completed(dir_a.path(), "fake").await;
        // A distinct runtime id (also of type `fake`): the actor.runtime value
        // differs, but the payload SHAPE must be identical — the PRD's
        // cross-runtime conformance proceed-criterion.
        let b = captured_run_completed(dir_b.path(), "alt").await;

        let key_set = |payload: &crate::hooks::HookPayload| {
            let value = serde_json::to_value(payload).unwrap();
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(
            key_set(&a),
            key_set(&b),
            "run_completed payload shape differs across runtimes"
        );
        assert_eq!(a.event, b.event);
        assert_eq!(a.outcome, b.outcome);
        assert_eq!(a.schema_version, b.schema_version);
        // The actor field is present and structurally uniform in both.
        assert!(serde_json::to_value(&a).unwrap().get("actor").is_some());
        assert!(serde_json::to_value(&b).unwrap().get("actor").is_some());
    }

    #[tokio::test]
    async fn hook_lifecycle_events_are_recorded_into_history() {
        let dir = tempdir().unwrap();
        let sentinel = dir.path().join("hook.json");
        let hooks = format!(
            "[[hooks.handler]]\non = \"run_completed\"\ncommand = \"cat > '{}'\"\n",
            sentinel.display()
        );
        let config = fake_config_with_hooks(dir.path(), "fake", &hooks);
        let mut app = App::new(config).await.unwrap();
        let mut life_rx = wire_hook_dispatcher(&mut app, Arc::new(NoopNotifier));

        app.submit_prompt("create a feature").await.unwrap();
        let started = tokio::time::timeout(Duration::from_secs(5), life_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let completed = tokio::time::timeout(Duration::from_secs(5), life_rx.recv())
            .await
            .unwrap()
            .unwrap();
        // Record them as the worker would (the only &mut App site).
        app.record_hook_lifecycle(started).unwrap();
        app.record_hook_lifecycle(completed).unwrap();

        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "hook_started"));
        let completed_event = events
            .iter()
            .find(|event| event.kind == "hook_completed")
            .expect("hook_completed recorded");
        assert_eq!(
            completed_event
                .payload
                .get("exit_code")
                .and_then(Value::as_i64),
            Some(0)
        );
        // Recording hook_* events must NOT re-trigger hooks: exactly one
        // hook_completed exists (no recursive cascade).
        assert_eq!(
            events.iter().filter(|e| e.kind == "hook_completed").count(),
            1
        );
    }

    #[tokio::test]
    async fn prompt_submitted_is_recorded_before_run_started_for_chat_order() {
        // The user's prompt must render above the run's "started" summary, so
        // prompt_submitted is recorded before run_started in history.
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("create a feature").await.unwrap();

        let events = app.history.read_events().unwrap();
        let prompt_idx = events
            .iter()
            .position(|event| event.kind == "prompt_submitted")
            .expect("prompt_submitted recorded");
        let run_started_idx = events
            .iter()
            .position(|event| event.kind == "run_started")
            .expect("run_started recorded");
        assert!(
            prompt_idx < run_started_idx,
            "prompt_submitted ({prompt_idx}) must precede run_started ({run_started_idx})"
        );
    }

    #[test]
    fn prompt_source_is_copy_and_comparable() {
        let fresh = PromptSource::Fresh;
        // Copy: `fresh` is still usable after the bind below (no move).
        let copied = fresh;
        assert_eq!(copied, fresh);
        assert_eq!(PromptSource::Recalled, PromptSource::Recalled);
        assert_ne!(PromptSource::Fresh, PromptSource::Recalled);
    }

    #[test]
    fn prompt_submitted_event_carries_and_exposes_source() {
        let event = AppEvent::PromptSubmitted("x".to_string(), PromptSource::Recalled);
        match event {
            AppEvent::PromptSubmitted(prompt, source) => {
                assert_eq!(prompt, "x");
                assert_eq!(source, PromptSource::Recalled);
            }
            other => panic!("expected PromptSubmitted, got {other:?}"),
        }
    }

    #[test]
    fn prompt_source_payload_roundtrip_and_missing_defaults_fresh() {
        assert_eq!(PromptSource::Fresh.as_str(), "fresh");
        assert_eq!(PromptSource::Recalled.as_str(), "recalled");
        assert_eq!(
            PromptSource::from_payload_value(Some(&json!("recalled"))),
            PromptSource::Recalled
        );
        assert_eq!(
            PromptSource::from_payload_value(Some(&json!("fresh"))),
            PromptSource::Fresh
        );
        // A missing or unrecognized value (older events) reads as Fresh.
        assert_eq!(PromptSource::from_payload_value(None), PromptSource::Fresh);
        assert_eq!(
            PromptSource::from_payload_value(Some(&json!("bogus"))),
            PromptSource::Fresh
        );
    }

    #[tokio::test]
    async fn recalled_submission_persists_source_in_payload() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt_with_source("create a feature", PromptSource::Recalled)
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let event = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .expect("prompt_submitted recorded");
        assert_eq!(event.payload["source"], "recalled");
        assert_eq!(
            PromptSource::from_payload_value(event.payload.get("source")),
            PromptSource::Recalled
        );
    }

    #[tokio::test]
    async fn fresh_submission_persists_fresh_source_in_payload() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        // The default submit path (and all non-recall callers) is Fresh.
        app.submit_prompt("create a feature").await.unwrap();

        let events = app.history.read_events().unwrap();
        let event = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .expect("prompt_submitted recorded");
        assert_eq!(event.payload["source"], "fresh");
    }

    #[tokio::test]
    async fn project_prompt_history_recalls_fake_runtime_submissions_newest_first() {
        // End-to-end: two prompts submitted through a real fake run land in the
        // event log; the projection surfaces them newest-first (ADR-001).
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("alpha").await.unwrap();
        app.submit_prompt("beta").await.unwrap();

        let history = crate::history::project_prompt_history(&dir.path().join(".atelier"), 10);
        assert_eq!(history, vec!["beta", "alpha"]);
    }

    #[tokio::test]
    async fn fake_runtime_executes_parallel_group_and_synthesizes_result() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "parallel_group_started" && event.group_id.is_some()));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "parallel_child_started")
                .count(),
            2
        );
        let joined = events
            .iter()
            .find(|event| event.kind == "parallel_group_joined")
            .unwrap();
        assert_eq!(joined.payload["status"], "completed");
        assert_eq!(joined.payload["children"].as_array().unwrap().len(), 2);
        assert!(
            events
                .iter()
                .filter(|event| event.kind == "agent_result" && event.group_id.is_some())
                .count()
                >= 2
        );
    }

    #[tokio::test]
    async fn disabled_parallel_feature_records_group_rejection_without_children() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Failed);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "parallel_group_rejected"
                && event.group_id.is_some()
                && event.payload["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("parallel step groups are disabled"))
        }));
        assert!(events
            .iter()
            .any(|event| event.kind == "orchestrator_decision_invalid"));
        assert!(!events
            .iter()
            .any(|event| event.kind == "parallel_child_started"));
    }

    #[tokio::test]
    async fn parallel_group_stops_before_start_when_agent_step_limit_cannot_fit_children() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_low_agent_step_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::LimitReached);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "run_limit_reached"
                && event.group_id.is_some()
                && event.payload["limit"] == "max_agent_steps"
                && event.payload["requested_parallel_children"] == 2
        }));
        assert!(!events
            .iter()
            .any(|event| event.kind == "parallel_group_started"));
        assert!(!events
            .iter()
            .any(|event| event.kind == "parallel_child_started"));
    }

    #[tokio::test]
    async fn fake_runtime_publishes_two_parallel_live_steps() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("parallel create a feature")
                .await
                .unwrap();
            app
        });

        let mut saw_two_live_steps = false;
        let deadline = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let state = receiver.borrow_and_update().clone();
                    if state.live_steps.len() == 2
                        && state
                            .live_steps
                            .iter()
                            .all(|step| step.group_id.is_some() && step.step_label.is_some())
                    {
                        saw_two_live_steps = true;
                        break;
                    }
                }
            }
        }

        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();

        assert!(saw_two_live_steps);
        assert_eq!(app.state.run_state, RunState::Completed);
    }

    #[tokio::test]
    async fn parallel_same_agent_profile_live_steps_are_distinguishable() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("parallel same agent create a feature")
                .await
                .unwrap();
            app
        });

        let mut labels = Vec::new();
        let deadline = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let state = receiver.borrow_and_update().clone();
                    let fixer_steps = state
                        .live_steps
                        .iter()
                        .filter(|step| step.agent == "fixer")
                        .collect::<Vec<_>>();
                    if fixer_steps.len() == 2
                        && fixer_steps.iter().all(|step| {
                            step.group_id.is_some()
                                && step.step_label.is_some()
                                && step.file_scope.is_some()
                        })
                    {
                        labels = fixer_steps
                            .iter()
                            .filter_map(|step| step.step_label.clone())
                            .collect();
                        assert_eq!(agent_status_from_state(&state, "fixer"), "running_parallel");
                        break;
                    }
                }
            }
        }

        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();

        labels.sort();
        assert_eq!(
            labels,
            vec![
                "fix first scoped file".to_string(),
                "fix second scoped file".to_string()
            ]
        );
        assert_eq!(app.state.run_state, RunState::Completed);
    }

    #[tokio::test]
    async fn parallel_two_fixers_write_disjoint_scoped_files() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel scoped write action create a feature")
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("parallel-output/fixer-a.txt")).unwrap(),
            "created by fake runtime\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("parallel-output/fixer-b.txt")).unwrap(),
            "created by fake runtime\n"
        );
        let events = app.history.read_events().unwrap();
        assert!(!events.iter().any(|event| {
            event.kind == "workflow_started" || event.kind == "workflow_completed"
        }));
        let joined = events
            .iter()
            .find(|event| event.kind == "parallel_group_joined")
            .unwrap();
        let changed_files = joined.payload["changed_files"].as_array().unwrap();
        assert!(changed_files
            .iter()
            .any(|path| path.as_str() == Some("parallel-output/fixer-a.txt")));
        assert!(changed_files
            .iter()
            .any(|path| path.as_str() == Some("parallel-output/fixer-b.txt")));
        assert_eq!(joined.payload["status"], "completed");
    }

    #[tokio::test]
    async fn parallel_approval_queue_allows_unrelated_child_to_finish() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_normal_approval_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let approval_handle = app.approval_handle();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("parallel approval action create a feature")
                .await
                .unwrap();
            app
        });

        let deadline = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(deadline);
        let mut saw_pending_approval = false;
        let mut saw_reviewer_complete_while_pending = false;
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let state = receiver.borrow_and_update().clone();
                    if state.pending_approval.as_ref().is_some_and(|approval| {
                        approval.agent == "fixer"
                    }) {
                        saw_pending_approval = true;
                    }
                    if state.pending_approval.is_some()
                        && state.live_steps.iter().any(|step| {
                            step.agent == "reviewer"
                                && matches!(step.status, LiveStepStatus::Completed)
                        })
                    {
                        saw_reviewer_complete_while_pending = true;
                        break;
                    }
                }
            }
        }

        assert!(saw_pending_approval);
        assert!(saw_reviewer_complete_while_pending);
        approval_handle.resolve(ApprovalResolution::Deny);

        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.pending_approval.is_none());
        let events = app.history.read_events().unwrap();
        let approval_requested = events
            .iter()
            .position(|event| event.kind == "approval_requested" && event.group_id.is_some())
            .unwrap();
        let reviewer_finished = events
            .iter()
            .position(|event| {
                event.kind == "parallel_child_completed"
                    && event
                        .payload
                        .get("agent")
                        .and_then(serde_json::Value::as_str)
                        == Some("reviewer")
            })
            .unwrap();
        let approval_resolved = events
            .iter()
            .position(|event| event.kind == "approval_resolved" && event.group_id.is_some())
            .unwrap();
        assert!(approval_requested < reviewer_finished);
        assert!(reviewer_finished < approval_resolved);
        assert!(events.iter().any(|event| {
            event.kind == "agent_result"
                && event.group_id.is_some()
                && event.payload["agent"] == "fixer"
                && event.payload["status"] == "approval_denied"
        }));
        let joined = events
            .iter()
            .find(|event| event.kind == "parallel_group_joined")
            .unwrap();
        assert_eq!(joined.payload["status"], "completed_with_issues");
        assert_eq!(
            joined.payload["approval_denials"].as_array().unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn parallel_child_parse_error_joins_as_terminal_result() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_reviewer_parse_error_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "agent_result"
                && event.group_id.is_some()
                && event.payload["agent"] == "reviewer"
                && event.payload["status"] == "parse_error"
        }));
        assert!(events.iter().any(|event| {
            event.kind == "parallel_child_failed"
                && event.group_id.is_some()
                && event.payload["agent"] == "reviewer"
                && event.payload["status"] == "parse_error"
        }));
        let joined = events
            .iter()
            .find(|event| event.kind == "parallel_group_joined")
            .unwrap();
        assert_eq!(joined.payload["status"], "completed_with_issues");
        assert!(!app
            .state
            .events
            .iter()
            .any(|event| event.contains("Runtime parse error queued for Orchestrator repair.")));
    }

    #[tokio::test]
    async fn parallel_child_out_of_scope_write_is_denied_before_mutation() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel write action create a feature")
            .await
            .unwrap();

        assert!(!dir.path().join("multiagent-action-output.txt").exists());
        let events = app.history.read_events().unwrap();
        let denied = events
            .iter()
            .find(|event| event.kind == "action_denied" && event.group_id.is_some())
            .unwrap();
        assert!(denied
            .payload
            .get("diagnostic")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .contains("exact write_files"));
        let blocked = events
            .iter()
            .find(|event| {
                event.kind == "agent_result"
                    && event.group_id.is_some()
                    && event.payload["status"] == "blocked"
            })
            .unwrap();
        assert_eq!(blocked.payload["agent"], "fixer");
    }

    #[tokio::test]
    async fn interrupt_cancels_active_parallel_children_and_joins_group() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let interrupt_handle = app.interrupt_handle();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("parallel interrupt create a feature")
                .await
                .unwrap();
            app
        });

        let deadline = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => panic!("parallel live steps did not start before deadline"),
                changed = receiver.changed() => {
                    changed.unwrap();
                    let state = receiver.borrow_and_update().clone();
                    if state.live_steps.len() == 2
                        && state.live_steps.iter().all(|step| {
                            !matches!(
                                step.status,
                                LiveStepStatus::Completed
                                    | LiveStepStatus::Failed
                                    | LiveStepStatus::Interrupted
                            )
                        })
                    {
                        break;
                    }
                }
            }
        }

        interrupt_handle.request_interrupt();
        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Interrupted);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "step_cancelled"));
        let joined = events
            .iter()
            .find(|event| event.kind == "parallel_group_joined")
            .unwrap();
        assert_eq!(joined.payload["status"], "cancelled");
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event.kind == "agent_result"
                        && event.group_id.is_some()
                        && event.payload["status"] == "cancelled"
                })
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn live_step_state_tracks_recent_runtime_streams_until_step_clears() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run", "step", "fixer");
        app.push_live_stream_delta("step", &RuntimeStreamDelta::new(1, "stdout", "first"));
        app.push_live_stream_delta(
            "step",
            &RuntimeStreamDelta::final_delta(2, "stdout", "done"),
        );

        let live_step = app.state.live_step.as_ref().unwrap();
        assert_eq!(live_step.agent, "fixer");
        assert_eq!(live_step.status, LiveStepStatus::Streaming);
        assert_eq!(live_step.streams.len(), 1);
        assert_eq!(live_step.streams[0].content, "firstdone");
        assert_eq!(live_step.streams[0].sequence_end, 2);
        assert!(live_step.streams[0].final_delta);

        app.clear_active_step("step");

        assert!(app.state.live_step.is_none());
    }

    #[tokio::test]
    async fn fake_runtime_publishes_running_state_before_first_stream_delta() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let interrupt_handle = app.interrupt_handle();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("/subtask explorer slow stream inspect README")
                .await
                .unwrap();
            app
        });

        let mut saw_running_before_stream = false;
        let deadline = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let state = receiver.borrow_and_update().clone();
                    if state.live_step.as_ref().is_some_and(|live_step| {
                        live_step.agent == "explorer"
                            && live_step.status == LiveStepStatus::Running
                            && live_step.streams.is_empty()
                    }) {
                        saw_running_before_stream = true;
                        break;
                    }
                }
            }
        }

        interrupt_handle.request_interrupt();
        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();

        assert!(saw_running_before_stream);
        assert_eq!(app.state.run_state, RunState::Interrupted);
    }

    #[tokio::test]
    async fn fake_runtime_publishes_live_state_before_final_completion() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("/subtask explorer inspect README only")
                .await
                .unwrap();
            app
        });

        let mut saw_live_stream_before_completion = false;
        let deadline = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let state = receiver.borrow_and_update().clone();
                    if state.run_state != RunState::Completed
                        && state.live_step.as_ref().is_some_and(|live_step| {
                            live_step.status == LiveStepStatus::Streaming
                                && live_step.streams.iter().any(|stream| !stream.content.is_empty())
                        })
                    {
                        saw_live_stream_before_completion = true;
                        break;
                    }
                }
            }
        }

        let app = run.await.unwrap();

        assert!(saw_live_stream_before_completion);
        assert_eq!(app.state.run_state, RunState::Completed);
    }

    #[tokio::test]
    async fn runtime_stream_history_uses_coalesced_sequence_ranges() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/subtask explorer inspect README only")
            .await
            .unwrap();

        let stream_events = app
            .history
            .read_events()
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "runtime_stream_delta")
            .collect::<Vec<_>>();

        assert_eq!(stream_events.len(), 2);
        assert!(stream_events.iter().all(|event| {
            event.payload["agent"] == "explorer"
                && event.payload["coalesced"] == true
                && event.payload.get("sequence").is_none()
                && event.payload.get("sequence_start").is_some()
                && event.payload.get("sequence_end").is_some()
        }));
    }

    #[tokio::test]
    async fn transient_runtime_stream_deltas_are_live_only() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let mut run = RunDriveContext::new("run", None, "prompt", "prompt", None, None, None);
        run.step_count = 1;

        app.set_active_step("run", "step", "explorer");
        app.record_runtime_stream_deltas(
            &run,
            "step",
            "explorer",
            &[RuntimeStreamDelta::transient(1, "message", "partial text")],
        )
        .unwrap();

        let live_step = app.state.live_step.as_ref().unwrap();
        assert!(live_step
            .streams
            .iter()
            .any(|stream| stream.stream == "message" && stream.content == "partial text"));
        assert!(app
            .history
            .read_events()
            .unwrap()
            .iter()
            .all(|event| event.kind != "runtime_stream_delta"));
    }

    #[tokio::test]
    async fn runtime_stream_sequences_continue_after_harness_actions() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "action context\n").unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("use action to create a feature")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let action_step_id = events
            .iter()
            .find(|event| event.kind == "action_requested")
            .and_then(|event| event.step_id.clone())
            .unwrap();
        let sequence_starts = events
            .iter()
            .filter(|event| {
                event.kind == "runtime_stream_delta"
                    && event.step_id.as_deref() == Some(action_step_id.as_str())
            })
            .filter_map(|event| {
                event
                    .payload
                    .get("sequence_start")
                    .and_then(serde_json::Value::as_u64)
            })
            .collect::<Vec<_>>();

        assert!(sequence_starts.contains(&1));
        assert!(sequence_starts.iter().any(|sequence| *sequence > 3));
    }

    #[tokio::test]
    async fn interrupt_signal_cancels_active_streaming_runtime() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let interrupt_handle = app.interrupt_handle();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("/subtask explorer slow stream inspect README")
                .await
                .unwrap();
            app
        });

        let deadline = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => panic!("runtime did not start streaming before test deadline"),
                changed = receiver.changed() => {
                    changed.unwrap();
                    let state = receiver.borrow_and_update().clone();
                    if state.live_step.as_ref().is_some_and(|live_step| {
                        live_step.status == LiveStepStatus::Streaming
                            && live_step.agent == "explorer"
                    }) {
                        break;
                    }
                }
            }
        }

        interrupt_handle.request_interrupt();
        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Interrupted);
        assert!(app.active_step.is_none());
        assert_eq!(agent_status(&app, "explorer"), "interrupted");
        assert_eq!(
            app.state.live_step.as_ref().map(|live| &live.status),
            Some(&LiveStepStatus::Interrupted)
        );
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "step_cancel_requested"));
        assert!(events.iter().any(|event| event.kind == "step_cancelled"));
        assert!(events.iter().any(|event| event.kind == "run_interrupted"));
    }

    #[tokio::test]
    async fn failed_runtime_cancellation_records_visible_failure() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.state.active_run_id = Some("run".to_string());
        app.set_active_step("run", "step", "explorer");
        let mut run =
            RunDriveContext::new("run", None, "slow stream", "slow stream", None, None, None);
        run.step_count = 1;
        let elapsed = tokio::time::timeout(Duration::ZERO, pending::<Result<RuntimeOutput>>())
            .await
            .unwrap_err();

        app.finish_streaming_interrupt(&run, "step", Err(elapsed))
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Interrupted);
        assert_eq!(agent_status(&app, "explorer"), "failed");
        assert_eq!(
            app.state.live_step.as_ref().map(|live| &live.status),
            Some(&LiveStepStatus::Failed)
        );
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "step_cancel_requested"));
        assert!(events
            .iter()
            .any(|event| event.kind == "step_cancel_failed"));
        assert!(events.iter().any(|event| event.kind == "run_interrupted"));
    }

    #[tokio::test]
    async fn large_runtime_stream_records_are_spilled_to_history_artifacts() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let record = RuntimeStreamRecord {
            agent: "explorer".to_string(),
            sequence_start: 1,
            sequence_end: 2,
            stream: "stdout".to_string(),
            content: "x".repeat(LARGE_ACTION_CONTENT_BYTES + 1),
            final_delta: true,
        };

        let payload = app.runtime_stream_record_payload(&record).unwrap();

        assert!(payload["content"].is_null());
        assert_eq!(payload["coalesced"], true);
        assert_eq!(payload["sequence_start"], 1);
        assert_eq!(payload["sequence_end"], 2);
        assert!(payload.get("artifact").is_some());
    }

    #[tokio::test]
    async fn council_workflow_runs_serial_councillors_and_returns_result() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("ask council for a high-risk architecture decision")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "council_started"));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "councillor_result")
                .count(),
            3
        );
        let council_result = events
            .iter()
            .find(|event| {
                event.kind == "agent_result"
                    && event
                        .payload
                        .get("agent")
                        .and_then(serde_json::Value::as_str)
                        == Some(COUNCIL_WORKFLOW_AGENT_ID)
            })
            .unwrap();
        assert_eq!(council_result.payload["status"], "completed");
    }

    #[tokio::test]
    async fn council_partial_failure_synthesizes_partial_confidence() {
        let dir = tempdir().unwrap();
        let config = fake_config_with_council_prompts(
            dir.path(),
            "fake parse error",
            "security review",
            "reviewer review",
        );
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("ask council for a high-risk migration review")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let synthesized = events
            .iter()
            .find(|event| event.kind == "council_synthesized")
            .unwrap();
        assert_eq!(synthesized.payload["envelope"]["confidence"], "partial");
        assert!(synthesized.payload["envelope"]["dissent"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap().contains("architect")));
    }

    #[tokio::test]
    async fn council_all_fail_returns_failed_result_with_diagnostics() {
        let dir = tempdir().unwrap();
        let config = fake_config_with_council_prompts(
            dir.path(),
            "fake parse error",
            "fake parse error",
            "fake parse error",
        );
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("ask council for a high-risk security review")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let council_result = events
            .iter()
            .find(|event| {
                event.kind == "agent_result"
                    && event
                        .payload
                        .get("agent")
                        .and_then(serde_json::Value::as_str)
                        == Some(COUNCIL_WORKFLOW_AGENT_ID)
            })
            .unwrap();
        assert_eq!(council_result.payload["status"], "failed");
        assert!(council_result.payload["blocker"]
            .as_str()
            .unwrap()
            .contains("fake runtime emitted malformed control output"));
    }

    #[test]
    fn council_route_guard_allows_user_prompt_council_requests() {
        assert!(council_route_allowed("please ask council", None));
    }

    #[test]
    fn council_route_guard_allows_session_goal_risk_context() {
        assert!(council_route_allowed(
            "continue with the next step",
            Some("high-risk architecture migration")
        ));
    }

    #[test]
    fn council_route_guard_rejects_low_risk_prompt_without_user_controlled_risk_context() {
        assert!(!council_route_allowed(
            "fix a typo",
            Some("keep implementation scoped")
        ));
    }

    #[tokio::test]
    async fn council_route_guard_rejects_model_authored_risk_terms() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let mut run =
            RunDriveContext::new("run", None, "fix a typo", "fix a typo", None, None, None);
        let decision = crate::orchestrator::OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some(COUNCIL_WORKFLOW_AGENT_ID.to_string()),
            next_step: None,
            reason: "High-risk security council review is useful.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Council returns a recommendation.".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        };

        let continue_run = app
            .handle_orchestrator_decision(&mut run, decision)
            .await
            .unwrap();

        assert!(!continue_run);
        assert_eq!(app.state.run_state, RunState::Failed);
        assert!(app
            .history
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind == "orchestrator_decision_invalid"));
    }

    #[tokio::test]
    async fn runtime_request_includes_recent_session_history() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.record_event(
            Some("prior-run".to_string()),
            Some("prior-step".to_string()),
            "agent_result",
            json!({
                "summary": "prior finding",
                "details": "x".repeat(RUNTIME_HISTORY_STRING_CHARS + 1)
            }),
            "Prior result recorded.",
        )
        .unwrap();

        let orchestrator = app.agent("orchestrator").unwrap().clone();
        let request = app
            .runtime_request(
                "current-run",
                "current-step",
                RuntimePrompt::new("create another feature", None),
                orchestrator,
                Vec::new(),
                "orchestrator_decision",
            )
            .unwrap();

        let prior = request
            .session_events
            .iter()
            .find(|event| event.kind == "agent_result")
            .unwrap();
        assert_eq!(prior.run_id.as_deref(), Some("prior-run"));
        assert_eq!(prior.step_id.as_deref(), Some("prior-step"));
        assert!(prior.payload_truncated);
        assert_eq!(prior.payload["summary"], "prior finding");
        assert_eq!(prior.payload["details"]["type"], "string");
        assert_eq!(
            prior.payload["details"]["preview"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            RUNTIME_HISTORY_STRING_CHARS
        );
    }

    #[tokio::test]
    async fn runtime_request_includes_compact_recent_file_and_action_context() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("write action create a feature")
            .await
            .unwrap();
        let orchestrator = app.agent("orchestrator").unwrap().clone();
        let request = app
            .runtime_request(
                "next-run",
                "next-step",
                RuntimePrompt::new("continue", None),
                orchestrator,
                Vec::new(),
                "orchestrator_decision",
            )
            .unwrap();

        assert!(request
            .recent_context
            .files
            .iter()
            .any(|file| file.path == "multiagent-action-output.txt"
                && file.operation == "write_file"));
        assert!(request
            .recent_context
            .actions
            .iter()
            .any(|action| action.event_kind == "action_completed"
                && action.status.as_deref() == Some("completed")));
        let serialized = serde_json::to_string(&request.recent_context).unwrap();
        assert!(!serialized.contains("created by fake runtime"));
    }

    // ── task_05 git context ──

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git available");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_git_repo(dir: &std::path::Path, branch: &str) {
        run_git(dir, &["init"]);
        run_git(dir, &["config", "user.email", "test@example.com"]);
        run_git(dir, &["config", "user.name", "Test"]);
        std::fs::write(dir.join("seed.txt"), "seed").unwrap();
        run_git(dir, &["add", "seed.txt"]);
        run_git(dir, &["commit", "-m", "init"]);
        run_git(dir, &["checkout", "-b", branch]);
    }

    #[tokio::test]
    async fn run_records_head_baseline_and_detects_external_commit_drift() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/resume");
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.refresh_git_context().await;
        let stored_head = app
            .state
            .git_context
            .as_ref()
            .and_then(|context| context.head_sha.clone());
        assert!(stored_head.is_some(), "a git repo yields a head_sha");

        // Recording a run captures the HEAD baseline into the log (ADR-007).
        app.submit_prompt("create a feature").await.unwrap();
        let events = app.history.read_events().unwrap();
        assert_eq!(
            crate::history::last_recorded_head_sha(&events),
            stored_head,
            "run_started records the head baseline, recoverable by folding"
        );

        // An external commit moves HEAD.
        std::fs::write(dir.path().join("new.txt"), "x").unwrap();
        run_git(dir.path(), &["add", "new.txt"]);
        run_git(dir.path(), &["commit", "-m", "external"]);
        let live_head = fetch_git_context(dir.path())
            .await
            .and_then(|context| context.head_sha);
        assert_ne!(stored_head, live_head, "external commit moved HEAD");

        // Drift: HEAD changed, same cwd.
        let drift = git::detect_drift(
            dir.path(),
            stored_head.as_deref(),
            dir.path(),
            live_head.as_deref(),
        );
        assert!(drift.head_changed);
        assert!(!drift.cwd_moved);
    }

    #[tokio::test]
    async fn set_git_context_publishes_only_on_change() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        let (sender, mut receiver) = watch::channel(app.state.clone());
        app.attach_state_sender(sender);
        receiver.borrow_and_update(); // consume the publish from attach

        let context = GitContext {
            repo_name: "atelier".to_string(),
            branch: "main".to_string(),
            head_sha: None,
            dirty: false,
        };
        assert!(app.set_git_context(Some(context.clone())));
        assert!(receiver.has_changed().unwrap(), "change published");
        receiver.borrow_and_update();

        assert!(!app.set_git_context(Some(context)));
        assert!(
            !receiver.has_changed().unwrap(),
            "identical context must not publish"
        );
    }

    #[tokio::test]
    async fn refresh_git_context_populates_from_repo() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/start");
        let mut app = App::new(fake_config(dir.path())).await.unwrap();

        assert!(app.state.git_context.is_none());
        app.refresh_git_context().await;

        let context = app.state.git_context.as_ref().expect("Some after refresh");
        assert_eq!(context.branch, "feat/start");
        assert_eq!(
            context.repo_name,
            dir.path().file_name().unwrap().to_string_lossy()
        );
    }

    #[tokio::test]
    async fn non_git_working_directory_refreshes_to_none_without_error() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.refresh_git_context().await;
        assert!(app.state.git_context.is_none());
    }

    #[tokio::test]
    async fn prompt_submission_refreshes_git_context_to_current_branch() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path(), "feat/one");
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.refresh_git_context().await;
        assert_eq!(app.state.git_context.as_ref().unwrap().branch, "feat/one");

        // Switch branch outside the app, then submit a prompt.
        run_git(dir.path(), &["checkout", "-b", "feat/two"]);
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.git_context.as_ref().unwrap().branch, "feat/two");
    }

    #[tokio::test]
    async fn app_event_handler_updates_input_and_submits_prompt() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.handle_event(AppEvent::InputCharacter('x'))
            .await
            .unwrap();
        app.handle_event(AppEvent::InputCharacter('y'))
            .await
            .unwrap();
        app.handle_event(AppEvent::InputBackspace).await.unwrap();
        assert_eq!(app.state.input, "x");

        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert!(app.state.input.is_empty());
        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app
            .history
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind == "prompt_submitted"));
    }

    #[tokio::test]
    async fn goal_command_updates_state_and_session_history() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/goal keep implementation scoped")
            .await
            .unwrap();
        app.submit_prompt("/goal").await.unwrap();
        app.submit_prompt("/goal clear").await.unwrap();

        assert!(app.state.session_goal.is_none());
        let display_events = app.state.events.join("\n");
        assert!(display_events.contains("Goal set."));
        assert!(display_events.contains("Goal: keep implementation scoped"));
        assert!(display_events.contains("Goal cleared."));
        let history_events = app.history.read_events().unwrap();
        assert!(history_events
            .iter()
            .any(|event| event.kind == "session_goal_set"
                && event.payload["goal"] == "keep implementation scoped"));
        assert!(history_events
            .iter()
            .any(|event| event.kind == "session_goal_cleared"));
    }

    #[tokio::test]
    async fn config_command_reports_sources_preset_and_warnings_without_prompt_bodies() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("agents")).unwrap();
        fs::write(dir.path().join("agents/explorer.md"), "secret prompt body").unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
preset = "research"

[runtimes.fake]
type = "fake"

[presets.research.agents.explorer]
runtime = "fake"
instructions_file = "agents/explorer.md"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/config").await.unwrap();

        let display_events = app.state.events.join("\n");
        assert!(display_events.contains("Config: sources:"));
        assert!(display_events.contains("preset: research"));
        assert!(display_events.contains("enabled agents without model_fallbacks"));
        assert!(!display_events.contains("secret prompt body"));
        let history_events = app.history.read_events().unwrap();
        let config_viewed = history_events
            .iter()
            .find(|event| event.kind == "config_viewed")
            .unwrap();
        assert_eq!(
            config_viewed
                .payload
                .get("preset")
                .and_then(serde_json::Value::as_str),
            Some("research")
        );
        assert!(!config_viewed
            .payload
            .to_string()
            .contains("secret prompt body"));
    }

    #[tokio::test]
    async fn unknown_slash_command_is_not_submitted_as_agent_prompt() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app.submit_prompt("/doctor").await.unwrap_err();

        assert!(error.to_string().contains("unknown command /doctor"));
        assert!(error.to_string().contains("/workflow <prompt>"));
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert!(!events
            .iter()
            .any(|event| event.kind == "run_started" || event.kind == "prompt_submitted"));
    }

    fn fake_with_zai_config(dir: &std::path::Path) -> EffectiveConfig {
        // Most agents use the deterministic fake runtime; one agent uses a Z.ai
        // runtime whose api_key_env points at a guaranteed-unset, uniquely
        // named variable. That resolves to a deterministic Unauthenticated
        // status in any environment (the var is never set), exercising the
        // partial-failure / share-safe path of `/provider:status`.
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[runtimes.zai]
type = "zai"
api_key_env = "MULTIAGENT_TEST_PROVIDER_STATUS_NO_KEY"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "zai"

[agents.consul]
runtime = "fake"

[council.presets.default.architect]
runtime = "fake"

[council.presets.default.security]
runtime = "fake"

[council.presets.default.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn provider_status_command_renders_one_row_for_each_used_provider() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/provider:status").await.unwrap();

        let display = app.state.events.join("\n");
        assert!(display.contains("Provider status"), "{display}");
        assert!(display.contains("Fake"), "{display}");
        assert!(display.contains("ready"), "{display}");
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());

        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "provider_status_viewed"));
        // No agent run was started — this is a read-only command.
        assert!(!events
            .iter()
            .any(|event| event.kind == "run_started" || event.kind == "prompt_submitted"));
    }

    #[tokio::test]
    async fn provider_status_command_tolerates_surrounding_whitespace() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("   /provider:status   ").await.unwrap();

        assert!(app.state.events.join("\n").contains("Provider status"));
        assert_eq!(app.state.run_state, RunState::Idle);
    }

    #[tokio::test]
    async fn provider_status_handler_only_claims_the_exact_command() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        // The exact command is handled.
        assert!(app
            .handle_provider_status_command("/provider:status")
            .await
            .unwrap());
        // An ordinary prompt is not intercepted.
        assert!(!app
            .handle_provider_status_command("just a normal prompt")
            .await
            .unwrap());
        // A different namespaced command is not intercepted (falls through to
        // the existing unknown-command guard).
        assert!(!app
            .handle_provider_status_command("/provider:unknown")
            .await
            .unwrap());
        // A malformed provider-status call with trailing args is rejected.
        assert!(app
            .handle_provider_status_command("/provider:status extra")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn provider_unknown_command_keeps_existing_unknown_behavior() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app.submit_prompt("/provider:unknown").await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unknown command /provider:unknown"),
            "{error}"
        );
        assert_eq!(app.state.run_state, RunState::Idle);
    }

    #[tokio::test]
    async fn provider_status_command_renders_failing_provider_share_safely() {
        let dir = tempdir().unwrap();
        let config = fake_with_zai_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/provider:status").await.unwrap();

        let display = app.state.events.join("\n");
        // Healthy and failing providers both appear, independently.
        assert!(display.contains("Fake"), "{display}");
        assert!(display.contains("ready"), "{display}");
        assert!(display.contains("Z.ai"), "{display}");
        assert!(display.contains("unauthenticated"), "{display}");
        assert_eq!(app.state.run_state, RunState::Idle);

        // The full recorded payload is share-safe: no env var values or
        // credential material, only the share-safe config message.
        let events = app.history.read_events().unwrap();
        let payload = events
            .iter()
            .find(|event| event.kind == "provider_status_viewed")
            .unwrap()
            .payload
            .to_string();
        assert!(!payload.contains("sk-"), "{payload}");
        assert!(
            !payload.contains('@'),
            "leaked an email/account id: {payload}"
        );
    }

    #[test]
    fn unknown_command_guidance_is_catalog_derived() {
        // Guidance must come from the shared catalog so it stays aligned with
        // the dropdown and help overlay — including `/reload:skills`, which the
        // old hardcoded list omitted.
        let error = reject_unknown_slash_command("/doctor").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("unknown command /doctor"), "{message}");
        for spec in crate::slash_commands::catalog() {
            assert!(
                message.contains(spec.label),
                "guidance missing {}: {message}",
                spec.label
            );
        }
        assert!(message.contains("/reload:skills"), "{message}");
        assert!(message.contains("/workflow <prompt>"), "{message}");
    }

    #[test]
    fn prompt_prefixes_are_allowed_through_unknown_command_guard() {
        // Named `/agent:` and `/skill:` prompts are prefixes, not commands, so
        // the unknown-command guard must let them pass through to submission.
        assert!(reject_unknown_slash_command("/agent:fixer inspect README").is_ok());
        assert!(reject_unknown_slash_command("/skill:reviewer inspect README").is_ok());
    }

    #[test]
    fn parse_queue_command_accepts_explicit_commands_and_rejects_empty() {
        assert_eq!(
            parse_queue_command("/queue update docs").unwrap(),
            Some("update docs".to_string())
        );
        assert_eq!(
            parse_queue_command("/q update docs").unwrap(),
            Some("update docs".to_string())
        );
        // Text after the alias is preserved verbatim, including internal spacing.
        assert_eq!(
            parse_queue_command("/q update  the   docs").unwrap(),
            Some("update  the   docs".to_string())
        );
        // Empty queue commands are usage errors, not silent no-ops.
        assert!(parse_queue_command("/queue").is_err());
        assert!(parse_queue_command("/q").is_err());
        assert!(parse_queue_command("/queue    ").is_err());
        // Non-queue input is never treated as a queue command.
        assert_eq!(parse_queue_command("q").unwrap(), None);
        assert_eq!(parse_queue_command("queue later").unwrap(), None);
        assert_eq!(parse_queue_command("/queueextra now").unwrap(), None);
        assert_eq!(parse_queue_command("/goal something").unwrap(), None);
    }

    #[tokio::test]
    async fn queue_command_creates_pending_follow_up_without_starting_run() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/queue update docs").await.unwrap();

        assert_eq!(app.state.queued_follow_ups.len(), 1);
        let item = &app.state.queued_follow_ups[0];
        assert_eq!(item.prompt, "update docs");
        assert_eq!(item.status, QueuedFollowUpStatus::Pending);
        assert!(item.pause_reason.is_none());
        assert!(!item.id.is_empty());
        assert!(!item.created_at.is_empty());

        // A queue command must not start a Run.
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert!(!events
            .iter()
            .any(|event| event.kind == "run_started" || event.kind == "prompt_submitted"));
        assert!(events.iter().any(|event| event.kind == "follow_up_queued"
            && event.payload["prompt"] == "update docs"
            && event.payload["status"] == "pending"));
    }

    #[tokio::test]
    async fn q_alias_queues_follow_up_and_preserves_prompt_text() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/q update the README docs")
            .await
            .unwrap();

        assert_eq!(app.state.queued_follow_ups.len(), 1);
        assert_eq!(
            app.state.queued_follow_ups[0].prompt,
            "update the README docs"
        );
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Pending
        );
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
    }

    #[tokio::test]
    async fn empty_queue_command_returns_usage_and_leaves_queue_empty() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app.submit_prompt("/queue").await.unwrap_err();

        assert!(error.to_string().contains("usage"));
        assert!(error.to_string().contains("/queue"));
        assert!(app.state.queued_follow_ups.is_empty());
        assert_eq!(app.state.run_state, RunState::Idle);
        let events = app.history.read_events().unwrap();
        assert!(!events.iter().any(|event| event.kind == "follow_up_queued"));
    }

    #[tokio::test]
    async fn empty_q_alias_returns_usage_and_leaves_queue_empty() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app.submit_prompt("/q").await.unwrap_err();

        assert!(error.to_string().contains("usage"));
        assert!(app.state.queued_follow_ups.is_empty());
        assert_eq!(app.state.run_state, RunState::Idle);
    }

    #[tokio::test]
    async fn plain_q_is_not_queued_and_follows_normal_prompt_path() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("q").await.unwrap();

        // Plain "q" is ordinary input, not the /q alias.
        assert!(app.state.queued_follow_ups.is_empty());
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "run_started"));
        assert!(!events.iter().any(|event| event.kind == "follow_up_queued"));
    }

    async fn queue_via_event(app: &mut App, message: &str) {
        app.handle_event(AppEvent::PromptSubmitted(
            format!("/queue {message}"),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
    }

    fn replay_started_prompts(app: &App) -> Vec<String> {
        app.history
            .read_events()
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "follow_up_replay_started")
            .map(|event| event.payload["prompt"].as_str().unwrap().to_string())
            .collect()
    }

    fn approval_mode_config(dir: &Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    fn single_agent_step_limit_config(dir: &Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[limits]
max_agent_steps = 1

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn two_queued_prompts_replay_oldest_first_across_two_completed_runs() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "first follow up").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "/q second follow up".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
        assert_eq!(app.state.queued_follow_ups.len(), 2);

        // A clean completed Run drains both queued items oldest-first.
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.queued_follow_ups.is_empty());
        assert_eq!(
            replay_started_prompts(&app),
            vec![
                "first follow up".to_string(),
                "second follow up".to_string()
            ]
        );
        let events = app.history.read_events().unwrap();
        let submitted: Vec<&str> = events
            .iter()
            .filter(|event| event.kind == "prompt_submitted")
            .filter_map(|event| event.payload["prompt"].as_str())
            .collect();
        assert!(submitted.contains(&"first follow up"));
        assert!(submitted.contains(&"second follow up"));
    }

    #[tokio::test]
    async fn clean_completed_run_replays_only_one_queued_item() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        // The first queued item will pause its own replay on a clarification;
        // the second must not start while the first is still waiting.
        queue_via_event(&mut app, "needs clarification create a feature").await;
        queue_via_event(&mut app, "second follow up").await;

        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_clarification.is_some());
        // Exactly one item was replayed from the single completed Run.
        assert_eq!(
            replay_started_prompts(&app),
            vec!["needs clarification create a feature".to_string()]
        );
        // The second item remains queued and is paused, not replayed.
        assert_eq!(app.state.queued_follow_ups.len(), 1);
        assert_eq!(app.state.queued_follow_ups[0].prompt, "second follow up");
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Paused
        );
    }

    #[tokio::test]
    async fn cancelling_pending_queued_item_prevents_replay() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "cancel me").await;
        let id = app.state.queued_follow_ups[0].id.clone();
        app.handle_event(AppEvent::FollowUpCancelled(id))
            .await
            .unwrap();
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Cancelled
        );

        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "follow_up_cancelled"));
        assert!(replay_started_prompts(&app).is_empty());
        assert!(app
            .state
            .queued_follow_ups
            .iter()
            .all(|item| item.status == QueuedFollowUpStatus::Cancelled));
    }

    #[tokio::test]
    async fn resuming_paused_queued_item_makes_it_eligible_for_replay() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "resume target").await;

        // A clarification-waiting Run pauses the queue.
        app.handle_event(AppEvent::PromptSubmitted(
            "needs clarification create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Paused
        );
        let id = app.state.queued_follow_ups[0].id.clone();

        // Resolving the clarification must NOT auto-replay the paused item.
        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();
        app.handle_event(AppEvent::ClarificationAnswered(ClarificationAnswer {
            question_id,
            answer: "use the CLI path".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        }))
        .await
        .unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Paused
        );
        assert!(replay_started_prompts(&app).is_empty());

        // Resuming makes the item eligible and it replays against the completed state.
        app.handle_event(AppEvent::FollowUpResumeRequested(id))
            .await
            .unwrap();

        assert!(app.state.queued_follow_ups.is_empty());
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "follow_up_replay_resumed"));
        assert_eq!(
            replay_started_prompts(&app),
            vec!["resume target".to_string()]
        );
        assert!(events
            .iter()
            .any(|event| event.kind == "prompt_submitted"
                && event.payload["prompt"] == "resume target"));
    }

    #[tokio::test]
    async fn replay_records_replay_started_and_normal_prompt_submitted() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "replay me").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        let events = app.history.read_events().unwrap();
        let replay_started = events
            .iter()
            .find(|event| event.kind == "follow_up_replay_started")
            .unwrap();
        assert_eq!(replay_started.payload["prompt"], "replay me");
        let replay_submitted = events
            .iter()
            .find(|event| {
                event.kind == "prompt_submitted" && event.payload["prompt"] == "replay me"
            })
            .unwrap();
        assert!(replay_submitted.run_id.is_some());
        // The replayed follow-up carries explicit `Fresh` provenance, matching the
        // main submit path so the event conforms to the same schema (F3).
        assert_eq!(replay_submitted.payload["source"], "fresh");
        let original_submitted = events
            .iter()
            .find(|event| {
                event.kind == "prompt_submitted" && event.payload["prompt"] == "create a feature"
            })
            .unwrap();
        // The replayed item runs as its own distinct Run.
        assert_ne!(replay_submitted.run_id, original_submitted.run_id);
        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.queued_follow_ups.is_empty());
    }

    #[tokio::test]
    async fn queued_prompt_after_successful_run_starts_as_next_run() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "deferred work").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        // Exactly two Runs started: the original prompt and the one replayed follow-up.
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "run_started")
                .count(),
            2
        );
        let replay_submitted = events
            .iter()
            .find(|event| {
                event.kind == "prompt_submitted" && event.payload["prompt"] == "deferred work"
            })
            .unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "run_completed" && event.run_id == replay_submitted.run_id));
        assert!(app.state.queued_follow_ups.is_empty());
    }

    #[tokio::test]
    async fn queue_command_processed_after_run_finishes_starts_immediately() {
        // In the TUI the worker drives a run to completion before it can service
        // the next command, so a `/queue` typed during a run is only processed
        // after the run already ended. The queued item must still start rather
        // than sit forever with no run-end to trigger its drain.
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);

        // The run has already finished when the queued follow-up arrives.
        queue_via_event(&mut app, "deferred work").await;

        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(
            app.state.queued_follow_ups.is_empty(),
            "queued follow-up should have started instead of staying queued"
        );
        assert!(replay_started_prompts(&app).contains(&"deferred work".to_string()));
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "prompt_submitted" && event.payload["prompt"] == "deferred work"
        }));
    }

    #[tokio::test]
    async fn clarification_waiting_pauses_queue_with_reason() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "queued doc update").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "needs clarification create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_clarification.is_some());
        let item = &app.state.queued_follow_ups[0];
        assert_eq!(item.status, QueuedFollowUpStatus::Paused);
        assert!(item
            .pause_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("clarification")));
        assert!(replay_started_prompts(&app).is_empty());
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "follow_up_replay_paused"));
    }

    #[tokio::test]
    async fn approval_waiting_pauses_queue_while_pending() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "post-approval work").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "approval action create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_approval.is_some());
        let item = &app.state.queued_follow_ups[0];
        assert_eq!(item.status, QueuedFollowUpStatus::Paused);
        assert!(item
            .pause_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("approval")));
        assert!(replay_started_prompts(&app).is_empty());
    }

    #[tokio::test]
    async fn first_approval_shows_explainer_and_latches_then_suppresses() {
        let dir = tempdir().unwrap();

        // Fresh install, first approval: the explainer flag is set and the
        // projected approval body carries the one-line explainer.
        let mut first = App::new(approval_mode_config(dir.path())).await.unwrap();
        first
            .handle_event(AppEvent::PromptSubmitted(
                "approval action create a feature".to_string(),
                PromptSource::Fresh,
            ))
            .await
            .unwrap();
        assert!(first.state.pending_approval.is_some());
        assert!(first.state.show_first_approval_explainer);
        assert!(first.state.chat_items.iter().any(|item| item
            .body
            .iter()
            .any(|line| line.text == crate::app::chat::FIRST_APPROVAL_EXPLAINER)));
        // The latch is persisted at the workspace root.
        assert!(first.history.first_approval_explainer_shown());

        // Resolving the approval clears the flag so it never lingers true while no
        // approval is pending — the flag stays in sync with approval state (F1).
        first
            .resolve_pending_approval(ApprovalResolution::Deny)
            .await
            .unwrap();
        assert!(first.state.pending_approval.is_none());
        assert!(!first.state.show_first_approval_explainer);

        // A later session (latch persisted on disk) suppresses the explainer.
        let mut later = App::new(approval_mode_config(dir.path())).await.unwrap();
        later
            .handle_event(AppEvent::PromptSubmitted(
                "approval action create a feature".to_string(),
                PromptSource::Fresh,
            ))
            .await
            .unwrap();
        assert!(later.state.pending_approval.is_some());
        assert!(!later.state.show_first_approval_explainer);
        assert!(later.state.chat_items.iter().all(|item| item
            .body
            .iter()
            .all(|line| line.text != crate::app::chat::FIRST_APPROVAL_EXPLAINER)));
    }

    #[tokio::test]
    async fn parse_error_run_does_not_replay_queued_items() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "post-failure work").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "always parse error create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::Failed);
        assert!(replay_started_prompts(&app).is_empty());
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Paused
        );
    }

    #[tokio::test]
    async fn limit_reached_run_does_not_replay_queued_items() {
        let dir = tempdir().unwrap();
        let config = single_agent_step_limit_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        queue_via_event(&mut app, "post-limit work").await;
        app.handle_event(AppEvent::PromptSubmitted(
            "create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::LimitReached);
        assert!(replay_started_prompts(&app).is_empty());
        assert_eq!(
            app.state.queued_follow_ups[0].status,
            QueuedFollowUpStatus::Paused
        );
    }

    #[tokio::test]
    async fn delayed_queue_after_failed_run_is_paused_with_reason() {
        // The run has already ended in Failed by the time the worker processes
        // the `/queue` (it was blocked driving the run). The new item must be
        // paused with the failure reason rather than left Pending forever.
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.handle_event(AppEvent::PromptSubmitted(
            "always parse error create a feature".to_string(),
            PromptSource::Fresh,
        ))
        .await
        .unwrap();
        assert_eq!(app.state.run_state, RunState::Failed);

        queue_via_event(&mut app, "post-failure work").await;

        assert!(replay_started_prompts(&app).is_empty());
        assert_eq!(app.state.queued_follow_ups.len(), 1);
        let item = &app.state.queued_follow_ups[0];
        assert_eq!(item.status, QueuedFollowUpStatus::Paused);
        assert_eq!(item.pause_reason.as_deref(), Some("previous run failed"));
    }

    #[tokio::test]
    async fn workflow_command_disabled_parallel_feature_fails_before_run_creation() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app
            .submit_prompt("/workflow create a feature")
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("workflow mode requires Parallel Step Groups"));
        assert!(message.contains("features.parallel_step_groups"));
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert_no_workflow_run_start_events(&events);
        let runs_dir = dir.path().join(".atelier/runs");
        assert_eq!(fs::read_dir(runs_dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn workflow_command_zero_parallel_limit_fails_before_run_creation() {
        let dir = tempdir().unwrap();
        let mut config = fake_parallel_config(dir.path());
        config.limits.max_parallel_agent_steps = 0;
        let mut app = App::new(config).await.unwrap();

        let error = app
            .submit_prompt("/workflow create a feature")
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("workflow mode requires Parallel Step Groups"));
        assert!(message.contains("limits.max_parallel_agent_steps = 0"));
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert_no_workflow_run_start_events(&events);
        let runs_dir = dir.path().join(".atelier/runs");
        assert_eq!(fs::read_dir(runs_dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn workflow_command_start_records_workflow_event_and_preserves_visible_command() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/workflow parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let run_started_index = events
            .iter()
            .position(|event| event.kind == "run_started")
            .unwrap();
        let workflow_started_index = events
            .iter()
            .position(|event| event.kind == "workflow_started")
            .unwrap();
        let prompt_submitted_index = events
            .iter()
            .position(|event| event.kind == "prompt_submitted")
            .unwrap();
        // The prompt is recorded first so it renders above the run summaries in
        // chat; then run_started, then workflow_started.
        assert!(prompt_submitted_index < run_started_index);
        assert!(run_started_index < workflow_started_index);

        let run_id = events[run_started_index].run_id.as_ref().unwrap();
        let workflow_started = &events[workflow_started_index];
        assert_eq!(workflow_started.run_id.as_ref(), Some(run_id));
        assert_eq!(workflow_started.payload["run_id"], run_id.as_str());
        assert_eq!(
            workflow_started.payload["original_command"],
            "/workflow parallel create a feature"
        );
        assert_eq!(
            workflow_started.payload["user_prompt"],
            "parallel create a feature"
        );
        assert_eq!(workflow_started.payload["mode"], "workflow");
        assert_eq!(
            workflow_started.payload["preflight"]["parallel_step_groups"],
            true
        );
        assert_eq!(
            workflow_started.payload["preflight"]["max_parallel_agent_steps"],
            2
        );

        let prompt = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        assert_eq!(
            prompt
                .payload
                .get("prompt")
                .and_then(serde_json::Value::as_str),
            Some("/workflow parallel create a feature")
        );
        assert_eq!(
            prompt
                .payload
                .get("submitted_prompt")
                .and_then(serde_json::Value::as_str),
            Some("/workflow parallel create a feature")
        );
        assert!(!prompt
            .payload
            .to_string()
            .contains("Workflow mode instructions"));
        assert!(events
            .iter()
            .any(|event| event.kind == "parallel_group_started"));
        assert!(events
            .iter()
            .any(|event| event.kind == "orchestrator_decision"));

        let run_record =
            fs::read_to_string(dir.path().join(format!(".atelier/runs/{run_id}.json"))).unwrap();
        assert!(
            run_record.contains("\"submitted_prompt\": \"/workflow parallel create a feature\"")
        );
        assert!(run_record.contains("Workflow mode instructions"));
        assert!(run_record.contains("Extracted user prompt:\\nparallel create a feature"));
        assert!(run_record.contains("planned file-edit targets"));
        assert!(run_record.contains("verification evidence"));
    }

    #[tokio::test]
    async fn workflow_parallel_prompt_persists_planned_targets_from_fake_group() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/workflow parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let run_id = events
            .iter()
            .find(|event| event.kind == "run_started")
            .and_then(|event| event.payload["run_id"].as_str())
            .unwrap();
        let record_path = dir.path().join(format!(".atelier/runs/{run_id}.json"));
        let record: Value =
            serde_json::from_str(&fs::read_to_string(record_path).unwrap()).unwrap();
        let ledger = record["workflow"]["target_ledger"].as_object().unwrap();
        let targets = ledger["src/runtime/fake.rs"].as_array().unwrap();

        assert_eq!(ledger.len(), 1);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0]["path"], "src/runtime/fake.rs");
        assert_eq!(targets[0]["source_step_label"], "fix runtime scope");
        assert_eq!(targets[0]["status"], "completed");
        assert_eq!(targets[0]["reason"], Value::Null);
    }

    #[tokio::test]
    async fn workflow_parallel_scoped_write_action_records_completed_workflow_evidence() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/workflow parallel scoped write action create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "workflow_started"));
        let completed = workflow_completed_event(&events);
        assert_eq!(completed.payload["status"], "completed");
        assert_eq!(completed.payload["target_counts"]["completed"], 2);
        assert_eq!(completed.payload["target_counts"]["planned"], 0);
        assert_eq!(completed.payload["unfinished_targets"], json!([]));
        assert!(completed.payload["verification"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("fake verification passed")));
        assert_eq!(
            fs::read_to_string(dir.path().join("parallel-output/fixer-a.txt")).unwrap(),
            "created by fake runtime\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("parallel-output/fixer-b.txt")).unwrap(),
            "created by fake runtime\n"
        );
    }

    #[tokio::test]
    async fn workflow_parallel_approval_denial_records_completed_with_issues_evidence() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_normal_approval_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let approval_handle = app.approval_handle();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("/workflow parallel approval action create a feature")
                .await
                .unwrap();
            app
        });

        let deadline = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => panic!("workflow approval did not become pending before deadline"),
                changed = receiver.changed() => {
                    changed.unwrap();
                    let state = receiver.borrow_and_update().clone();
                    if state.pending_approval.as_ref().is_some_and(|approval| {
                        approval.agent == "fixer"
                    }) {
                        break;
                    }
                }
            }
        }
        approval_handle.resolve(ApprovalResolution::Deny);

        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let completed = workflow_completed_event(&events);
        assert_eq!(completed.payload["status"], "completed_with_issues");
        assert_eq!(completed.payload["target_counts"]["blocked"], 1);
        let unfinished = completed.payload["unfinished_targets"].as_array().unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0]["path"], "src/runtime/fake.rs");
        assert_eq!(unfinished[0]["status"], "blocked");
        assert!(unfinished[0]["reason"]
            .as_str()
            .unwrap()
            .contains("approval"));
        let workflow_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.title == "Workflow completed with issues")
            .unwrap();
        assert_eq!(workflow_item.kind, ChatItemKind::RunSummary);
        assert_eq!(workflow_item.severity, ChatSeverity::Warning);
        let workflow_text = workflow_item
            .body
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(workflow_text.contains("targets:"));
        assert!(workflow_text.contains("unfinished target: src/runtime/fake.rs (blocked)"));
    }

    #[tokio::test]
    async fn workflow_scoped_write_child_parse_error_records_failed_target_evidence() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt(
            "/workflow parallel scoped write action child parse error create a feature",
        )
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert_eq!(
            fs::read_to_string(dir.path().join("parallel-output/fixer-a.txt")).unwrap(),
            "created by fake runtime\n"
        );
        assert!(!dir.path().join("parallel-output/fixer-b.txt").exists());

        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "parallel_child_failed"
                && event.group_id.is_some()
                && event.payload["status"] == "parse_error"
                && event.payload["file_scope"]["write_files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path.as_str() == Some("parallel-output/fixer-b.txt"))
        }));

        let completed = workflow_completed_event(&events);
        assert_eq!(completed.payload["status"], "completed_with_issues");
        assert_eq!(completed.payload["target_counts"]["completed"], 1);
        assert_eq!(completed.payload["target_counts"]["failed"], 1);
        assert_eq!(completed.payload["target_counts"]["blocked"], 0);
        assert_eq!(completed.payload["target_counts"]["planned"], 0);
        let unfinished = completed.payload["unfinished_targets"].as_array().unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0]["path"], "parallel-output/fixer-b.txt");
        assert_eq!(unfinished[0]["status"], "failed");
        assert!(unfinished[0]["reason"]
            .as_str()
            .unwrap()
            .contains("fake runtime emitted malformed control output"));
    }

    #[tokio::test]
    async fn workflow_completed_event_precedes_generic_run_completed_event() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/workflow parallel scoped write action create a feature")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let workflow_completed_index = events
            .iter()
            .position(|event| event.kind == "workflow_completed")
            .unwrap();
        let run_completed_index = events
            .iter()
            .position(|event| event.kind == "run_completed")
            .unwrap();
        assert!(workflow_completed_index < run_completed_index);
    }

    #[tokio::test]
    async fn interrupted_workflow_records_failed_workflow_evidence() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let interrupt_handle = app.interrupt_handle();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        let run = tokio::spawn(async move {
            app.submit_prompt("/workflow parallel interrupt create a feature")
                .await
                .unwrap();
            app
        });

        let deadline = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => panic!("workflow parallel live steps did not start before deadline"),
                changed = receiver.changed() => {
                    changed.unwrap();
                    let state = receiver.borrow_and_update().clone();
                    if state.live_steps.len() == 2 {
                        break;
                    }
                }
            }
        }

        interrupt_handle.request_interrupt();
        let app = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(app.state.run_state, RunState::Interrupted);
        let events = app.history.read_events().unwrap();
        let completed = workflow_completed_event(&events);
        assert_eq!(completed.payload["status"], "failed");
        assert_eq!(completed.payload["target_counts"]["failed"], 1);
        let unfinished = completed.payload["unfinished_targets"].as_array().unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0]["path"], "src/runtime/fake.rs");
        assert_eq!(unfinished[0]["status"], "failed");
        assert!(unfinished[0]["reason"]
            .as_str()
            .unwrap()
            .contains("cancelled"));
    }

    #[tokio::test]
    async fn workflow_parallel_prompt_excludes_read_only_reviewer_from_planned_targets() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/workflow parallel create a feature")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let run_id = events
            .iter()
            .find(|event| event.kind == "run_started")
            .and_then(|event| event.payload["run_id"].as_str())
            .unwrap();
        let record_path = dir.path().join(format!(".atelier/runs/{run_id}.json"));
        let record: Value =
            serde_json::from_str(&fs::read_to_string(record_path).unwrap()).unwrap();
        let ledger = record["workflow"]["target_ledger"].as_object().unwrap();

        assert!(!ledger.values().any(|targets| {
            targets.as_array().unwrap().iter().any(|target| {
                target["source_step_label"] == "review app scope" || target["path"] == "src/app"
            })
        }));
    }

    #[tokio::test]
    async fn normal_parallel_prompt_records_no_workflow_started_event() {
        let dir = tempdir().unwrap();
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("parallel create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(!events.iter().any(|event| event.kind == "workflow_started"));
        let run_id = events
            .iter()
            .find(|event| event.kind == "run_started")
            .and_then(|event| event.payload["run_id"].as_str())
            .unwrap()
            .to_string();
        let generic_run_item = app
            .state
            .chat_items
            .iter()
            .find(|item| {
                item.lifecycle_key.as_ref()
                    == Some(&ChatLifecycleKey::Run {
                        run_id: run_id.clone(),
                    })
            })
            .unwrap();
        assert_eq!(generic_run_item.title, "Run completed");
        assert_eq!(generic_run_item.severity, ChatSeverity::Success);
        assert!(!app.state.chat_items.iter().any(|item| matches!(
            item.lifecycle_key.as_ref(),
            Some(ChatLifecycleKey::Workflow { .. })
        )));
        let prompt = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        assert_eq!(
            prompt
                .payload
                .get("prompt")
                .and_then(serde_json::Value::as_str),
            Some("parallel create a feature")
        );
        assert!(events
            .iter()
            .any(|event| event.kind == "parallel_group_started"));
    }

    #[tokio::test]
    async fn agent_prompt_prefix_is_allowed_as_agent_prompt() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/agent:fixer inspect README")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let history_events = app.history.read_events().unwrap();
        let prompt = history_events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        assert_eq!(
            prompt
                .payload
                .get("prompt")
                .and_then(serde_json::Value::as_str),
            Some("/agent:fixer inspect README")
        );
    }

    #[tokio::test]
    async fn skill_prompt_prefix_loads_skill_before_runtime_work() {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "reviewer",
            Some("reviewer"),
            "Review workflow guidance.",
        );
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/skill:reviewer inspect README")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let run_started_index = events
            .iter()
            .position(|event| event.kind == "run_started")
            .unwrap();
        let prompt_submitted_index = events
            .iter()
            .position(|event| event.kind == "prompt_submitted")
            .unwrap();
        let skills_loaded_index = events
            .iter()
            .position(|event| event.kind == "skills_loaded")
            .unwrap();
        let runtime_index = events
            .iter()
            .position(|event| event.kind == "agent_step_started")
            .unwrap();
        // The prompt is recorded first (so it renders above the run summary in
        // chat), then run_started, then skills load before any runtime work.
        assert!(prompt_submitted_index < run_started_index);
        assert!(run_started_index < skills_loaded_index);
        assert!(skills_loaded_index < runtime_index);

        let prompt = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        // The displayed prompt keeps the `/skill:` reference so chat shows what
        // the user typed; only the runtime prompt is stripped.
        assert_eq!(
            prompt
                .payload
                .get("prompt")
                .and_then(serde_json::Value::as_str),
            Some("/skill:reviewer inspect README")
        );
        assert_eq!(
            prompt
                .payload
                .get("submitted_prompt")
                .and_then(serde_json::Value::as_str),
            Some("/skill:reviewer inspect README")
        );
        let skills_loaded = events
            .iter()
            .find(|event| event.kind == "skills_loaded")
            .unwrap();
        assert_eq!(skills_loaded.payload["skills"].as_array().unwrap().len(), 1);
        assert_eq!(
            skills_loaded.payload["skills"][0]["display_name"],
            Value::String("reviewer".to_string())
        );
        assert_eq!(
            skills_loaded.payload["skills"][0]["source_origin"],
            Value::String(".agents/skills".to_string())
        );
    }

    #[tokio::test]
    async fn unknown_skill_reference_fails_before_run_creation() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app
            .submit_prompt("/skill:missing inspect README")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unknown skill 'missing'"));
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert!(events.iter().all(|event| {
            event.kind != "run_started"
                && event.kind != "prompt_submitted"
                && event.kind != "skills_loaded"
        }));
        let runs_dir = dir.path().join(".atelier/runs");
        assert_eq!(fs::read_dir(runs_dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn empty_skill_reference_reports_skill_load_diagnostic() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app
            .submit_prompt("/skill: inspect README")
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("empty /skill: reference"));
        assert!(!message.contains("unknown command"));
        let events = app.history.read_events().unwrap();
        assert!(events.iter().all(|event| {
            event.kind != "run_started"
                && event.kind != "prompt_submitted"
                && event.kind != "skills_loaded"
        }));
    }

    #[tokio::test]
    async fn mid_prompt_skill_reference_loads_and_normalizes_prompt() {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "reviewer",
            Some("reviewer"),
            "Review workflow guidance.",
        );
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("please use /skill:reviewer here")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let prompt = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        // Display keeps the mid-prompt reference as typed; the runtime prompt
        // (normalized to "please use here") is stripped separately.
        assert_eq!(prompt.payload["prompt"], "please use /skill:reviewer here");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "skills_loaded")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn duplicate_skill_references_emit_one_loaded_skill_entry() {
        let dir = tempdir().unwrap();
        write_project_skill(dir.path(), "a", Some("a"), "Skill A guidance.");
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/skill:a do x /skill:a").await.unwrap();

        let events = app.history.read_events().unwrap();
        let skills_loaded = events
            .iter()
            .find(|event| event.kind == "skills_loaded")
            .unwrap();
        let skills = skills_loaded.payload["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["requested_names"], json!(["a"]));
        // Display shows the prompt as typed (both references kept); only the
        // runtime prompt is normalized to "do x".
        assert_eq!(
            events
                .iter()
                .find(|event| event.kind == "prompt_submitted")
                .unwrap()
                .payload["prompt"],
            "/skill:a do x /skill:a"
        );
    }

    #[tokio::test]
    async fn skill_bodies_are_not_persisted_to_history_debug_log_or_run_record() {
        let dir = tempdir().unwrap();
        const SENTINEL: &str = "SENTINEL_FULL_SKILL_BODY_PRIVATE";
        write_project_skill(dir.path(), "reviewer", Some("reviewer"), SENTINEL);
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.debug_enabled = true;

        app.submit_prompt("/skill:reviewer inspect README")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let run_id = events
            .iter()
            .find(|event| event.kind == "run_started")
            .and_then(|event| event.run_id.clone())
            .unwrap();
        let prompt_submitted = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        assert!(!prompt_submitted.payload.to_string().contains(SENTINEL));

        let events_jsonl =
            fs::read_to_string(app.history.session_dir().join("events.jsonl")).unwrap();
        let debug_log = fs::read_to_string(dir.path().join(".atelier/debug.log")).unwrap();
        let run_record =
            fs::read_to_string(dir.path().join(format!(".atelier/runs/{run_id}.json"))).unwrap();
        let chat_projection = serde_json::to_string(&app.state.chat_items).unwrap();
        let skill_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.title == "Skills loaded")
            .unwrap();
        let skill_item_text = chat_item_text(skill_item);
        assert!(!events_jsonl.contains(SENTINEL));
        assert!(!debug_log.contains(SENTINEL));
        assert!(!run_record.contains(SENTINEL));
        assert!(!chat_projection.contains(SENTINEL));
        assert!(events_jsonl.contains("\"kind\":\"skills_loaded\""));
        assert!(events_jsonl.contains(".agents/skills/reviewer/SKILL.md"));
        assert!(debug_log.contains("\"kind\":\"skills_loaded\""));
        assert!(debug_log.contains(".agents/skills/reviewer/SKILL.md"));
        assert!(run_record.contains("\"submitted_prompt\""));
        assert!(run_record.contains("\"loaded_skills\""));
        assert_eq!(skill_item.kind, ChatItemKind::SkillContext);
        assert_eq!(skill_item.status, ChatItemStatus::Completed);
        assert_eq!(skill_item.severity, ChatSeverity::Info);
        assert!(skill_item_text.contains("reviewer"));
        assert!(skill_item_text.contains(".agents/skills"));
        assert!(skill_item_text.contains(".agents/skills/reviewer/SKILL.md"));
        assert!(!skill_item_text.contains(SENTINEL));
    }

    #[tokio::test]
    async fn fake_runtime_ignores_skill_body_trigger_words_for_fixture_routing() {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "trigger",
            Some("trigger"),
            "SENTINEL_TRIGGER_SKILL_BODY_PRIVATE parallel use action approval action command action write action retryable provider error needs clarification high-risk architecture typo",
        );
        let config = fake_parallel_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/skill:trigger inspect README")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "skills_loaded"));
        assert!(!events
            .iter()
            .any(|event| event.kind == "parallel_group_started"));
        assert!(!events.iter().any(|event| event.kind == "action_requested"));
        assert!(!events.iter().any(|event| event.kind == "blocker_reported"));
        assert!(!events
            .iter()
            .any(|event| event.kind == "clarification_requested"));
        assert!(!events.iter().any(|event| event.kind == "council_started"));
    }

    #[tokio::test]
    async fn subtask_command_runs_one_bounded_child_agent_step() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/subtask explorer inspect README only")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.active_run_id.is_none());
        let display_events = app.state.events.join("\n");
        assert!(display_events.contains("Subtask started: explorer."));
        assert!(display_events.contains("Subtask completed: explorer:"));
        assert!(!display_events.contains("Orchestrator step started."));
        let history_events = app.history.read_events().unwrap();
        assert!(history_events
            .iter()
            .any(|event| event.kind == "subtask_started"));
        let completed = history_events
            .iter()
            .find(|event| event.kind == "subtask_completed")
            .unwrap();
        assert_eq!(
            completed
                .payload
                .get("scope_guard")
                .and_then(serde_json::Value::as_str),
            Some("subtask_result_must_not_broaden_request")
        );
    }

    #[tokio::test]
    async fn subtask_run_record_persists_scope_guarded_prompt_and_metadata() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/subtask explorer inspect src only")
            .await
            .unwrap();

        let subtask_started = app
            .history
            .read_events()
            .unwrap()
            .into_iter()
            .find(|event| event.kind == "subtask_started")
            .unwrap();
        let run_id = subtask_started
            .payload
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let record_path = dir
            .path()
            .join(".atelier")
            .join("runs")
            .join(format!("{run_id}.json"));
        let record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(record_path).unwrap()).unwrap();

        assert!(record
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .contains("Do not broaden scope beyond this subtask."));
        assert_eq!(
            record
                .get("subtask")
                .and_then(|subtask| subtask.get("agent_id"))
                .and_then(serde_json::Value::as_str),
            Some("explorer")
        );
        assert_eq!(
            record
                .get("subtask")
                .and_then(|subtask| subtask.get("request"))
                .and_then(serde_json::Value::as_str),
            Some("inspect src only")
        );
        assert_eq!(
            record
                .get("subtask")
                .and_then(|subtask| subtask.get("submitted_request"))
                .and_then(serde_json::Value::as_str),
            Some("inspect src only")
        );
    }

    #[tokio::test]
    async fn subtask_skill_reference_loads_before_subtask_runtime_work() {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "reviewer",
            Some("reviewer"),
            "Review workflow guidance.",
        );
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/subtask explorer /skill:reviewer inspect README")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let subtask_started_index = events
            .iter()
            .position(|event| event.kind == "subtask_started")
            .unwrap();
        let skills_loaded_index = events
            .iter()
            .position(|event| event.kind == "skills_loaded")
            .unwrap();
        let runtime_index = events
            .iter()
            .position(|event| event.kind == "agent_step_started")
            .unwrap();
        assert!(subtask_started_index < skills_loaded_index);
        assert!(skills_loaded_index < runtime_index);

        let subtask_started = events
            .iter()
            .find(|event| event.kind == "subtask_started")
            .unwrap();
        assert_eq!(subtask_started.payload["request"], "inspect README");
        assert_eq!(
            subtask_started.payload["submitted_request"],
            "/skill:reviewer inspect README"
        );
        let run_id = subtask_started.run_id.as_ref().unwrap();
        let run_record =
            fs::read_to_string(dir.path().join(format!(".atelier/runs/{run_id}.json"))).unwrap();
        assert!(run_record.contains("\"request\": \"inspect README\""));
        assert!(run_record.contains("\"submitted_request\": \"/skill:reviewer inspect README\""));
        assert!(!run_record.contains("Review workflow guidance."));
    }

    #[tokio::test]
    async fn failed_subtask_skill_reference_creates_no_subtask_run() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app
            .submit_prompt("/subtask explorer /skill:missing inspect README")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unknown skill 'missing'"));
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .all(|event| event.kind != "subtask_started" && event.kind != "skills_loaded"));
        assert_eq!(
            fs::read_dir(dir.path().join(".atelier/runs"))
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn subtask_rejects_disabled_orchestrator_targets() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let error = app
            .submit_prompt("/subtask orchestrator plan everything")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("specialized agent"));
    }

    #[tokio::test]
    async fn runtime_request_includes_active_session_goal() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/goal prefer root-cause fixes")
            .await
            .unwrap();
        let orchestrator = app.agent("orchestrator").unwrap().clone();
        let request = app
            .runtime_request(
                "run",
                "step",
                RuntimePrompt::new("create a feature", None),
                orchestrator,
                Vec::new(),
                "orchestrator_decision",
            )
            .unwrap();

        assert_eq!(
            request.session_goal.as_deref(),
            Some("prefer root-cause fixes")
        );
    }

    #[tokio::test]
    async fn run_record_persists_active_session_goal() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/goal preserve action policy")
            .await
            .unwrap();
        app.submit_prompt("create a feature").await.unwrap();

        let run_started = app
            .history
            .read_events()
            .unwrap()
            .into_iter()
            .find(|event| event.kind == "run_started")
            .unwrap();
        let run_id = run_started
            .payload
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let record_path = dir
            .path()
            .join(".atelier")
            .join("runs")
            .join(format!("{run_id}.json"));
        let record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(record_path).unwrap()).unwrap();

        assert_eq!(
            record
                .get("session_goal")
                .and_then(serde_json::Value::as_str),
            Some("preserve action policy")
        );
    }

    #[tokio::test]
    async fn debug_mode_writes_debug_log() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let _app = App::new_with_debug(config, true).await.unwrap();
        let debug_log = dir.path().join(".atelier/debug.log");
        let contents = fs::read_to_string(debug_log).unwrap();
        assert!(contents.contains("\"kind\":\"session_started\""));
    }

    #[tokio::test]
    async fn end_session_records_one_session_ended_event() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.end_session().unwrap();
        app.end_session().unwrap();

        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "session_ended")
                .count(),
            1
        );
        let session_ended = events
            .iter()
            .find(|event| event.kind == "session_ended")
            .unwrap();
        assert_eq!(
            session_ended
                .payload
                .get("run_state")
                .and_then(serde_json::Value::as_str),
            Some("idle")
        );
        assert!(app
            .state
            .events
            .contains(&"Harness session ended.".to_string()));
    }

    #[tokio::test]
    async fn promote_mcp_server_persists_file_and_records_event() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.promote_mcp_server("fs").unwrap();

        // The trust file was written under the workspace `.atelier/` root.
        let trust_path = dir.path().join(".atelier").join("mcp-trust.json");
        assert!(trust_path.exists(), "promote must write mcp-trust.json");

        // An audit event landed in history.
        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "mcp_server_trusted")
                .count(),
            1
        );

        // Cross-session remember: a fresh store loaded from the file reflects it.
        let reloaded = crate::mcp::McpTrustStore::load(&dir.path().join(".atelier"));
        assert!(reloaded.is_trusted("fs"));

        // Revoke records its own event and flips the persisted tier back.
        app.revoke_mcp_server("fs").unwrap();
        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "mcp_server_revoked")
                .count(),
            1
        );
        let reloaded = crate::mcp::McpTrustStore::load(&dir.path().join(".atelier"));
        assert!(!reloaded.is_trusted("fs"));
    }

    #[tokio::test]
    async fn record_mcp_catalog_snapshot_emits_event_only_when_enabled() {
        // Enabled: a (server-less) supervisor produces an empty snapshot that is
        // still recorded as the replay source for the orchestrator prompt.
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.mcp_handle = Some(crate::mcp::McpSupervisor::spawn(
            Vec::new(),
            crate::mcp::DEFAULT_MCP_CALL_TIMEOUT,
        ));
        app.record_mcp_catalog_snapshot("run1").await.unwrap();
        let events = app.history.read_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "mcp_catalog_snapshot")
                .count(),
            1
        );

        // Disabled (no handle): recording is a no-op.
        let dir2 = tempdir().unwrap();
        let mut disabled = App::new(fake_config(dir2.path())).await.unwrap();
        disabled.record_mcp_catalog_snapshot("run1").await.unwrap();
        assert!(!disabled
            .history
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind == "mcp_catalog_snapshot"));
    }

    #[tokio::test]
    async fn fake_runtime_respects_agent_step_limit() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[limits]
max_agent_steps = 1

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("create a feature").await.unwrap();
        assert_eq!(app.state.run_state, RunState::LimitReached);
    }

    #[test]
    fn wall_clock_limit_uses_elapsed_minutes() {
        let expired = Instant::now() - Duration::from_secs(61);
        assert!(wall_clock_limit_reached(&Limit::Value(1), expired));
        assert!(!wall_clock_limit_reached(&Limit::Unlimited, expired));
    }

    #[test]
    fn step_time_limit_uses_elapsed_minutes() {
        let expired = Instant::now() - Duration::from_secs(61);
        assert!(time_limit_reached(&Limit::Value(1), expired));
        assert_eq!(
            remaining_limit_duration(&Limit::Value(1), expired),
            Some(Duration::ZERO)
        );
    }

    #[tokio::test]
    async fn run_driver_stops_when_wall_clock_limit_elapsed() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[limits]
max_wall_clock_minutes = 1

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        let run_id = "expired-run".to_string();
        app.state.active_run_id = Some(run_id.clone());
        app.state.run_state = RunState::Planning;
        let mut run = RunDriveContext::new(
            run_id,
            None,
            "create a feature",
            "create a feature",
            None,
            None,
            None,
        );
        run.started_at = Instant::now() - Duration::from_secs(61);

        app.drive_run(run, None).await.unwrap();

        assert_eq!(app.state.run_state, RunState::LimitReached);
        assert!(app.state.active_run_id.is_none());
        let events = app.state.events.join("\n");
        assert!(events.contains("Run wall-clock limit reached."));
        assert!(!events.contains("Orchestrator step started."));
    }

    #[tokio::test]
    async fn step_time_limit_stops_resumed_step() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[limits]
max_step_minutes = 1

[agents.orchestrator]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        let run_id = "step-timeout-run".to_string();
        let step_id = "expired-step".to_string();
        let orchestrator = app.agent("orchestrator").unwrap().clone();
        let request = app
            .runtime_request(
                &run_id,
                &step_id,
                RuntimePrompt::new("create a feature", None),
                orchestrator,
                Vec::new(),
                "orchestrator_decision",
            )
            .unwrap();
        app.state.active_run_id = Some(run_id.clone());
        app.state.run_state = RunState::Planning;
        let mut run = RunDriveContext::new(
            run_id,
            None,
            "create a feature",
            "create a feature",
            None,
            None,
            None,
        );
        run.step_count = 1;
        let resume = StepResume {
            step: PausedStep::Orchestrator { step_id },
            step_started_at: Instant::now() - Duration::from_secs(61),
            request,
        };

        app.drive_run(run, Some(resume)).await.unwrap();

        assert_eq!(app.state.run_state, RunState::LimitReached);
        assert!(app.state.active_run_id.is_none());
        let events = app.state.events.join("\n");
        assert!(events.contains("Step time limit reached."));
        assert!(!events.contains("Orchestrator:"));
    }

    #[test]
    fn review_fix_cycle_count_skips_initial_fixer_pass() {
        let results = vec![
            RunStepResult::Agent {
                result: AgentResult::completed("explorer", "s1", "explored"),
            },
            RunStepResult::Agent {
                result: AgentResult::completed("fixer", "s2", "initial fix"),
            },
            RunStepResult::Agent {
                result: AgentResult::completed("reviewer", "s3", "reviewed"),
            },
            RunStepResult::Agent {
                result: AgentResult::completed("fixer", "s4", "review fix"),
            },
            RunStepResult::Agent {
                result: AgentResult::completed("reviewer", "s5", "reviewed again"),
            },
        ];
        assert_eq!(review_fix_cycle_count(&results), 1);
    }

    #[test]
    fn action_display_includes_targets_and_failures() {
        let request = ActionRequest {
            schema_version: 1,
            action_id: "action".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::SearchText,
            params: json!({ "path": "src", "query": "TODO" }),
        };
        let result = ActionResult {
            schema_version: 1,
            action_id: "action".to_string(),
            status: ActionStatus::Failed,
            summary: "Action failed.".to_string(),
            content: None,
            artifact: None,
            diagnostic: Some("failed to search src: permission denied".to_string()),
            risk: None,
            gate_outcome: crate::actions::GateOutcome::Normal,
        };

        assert_eq!(
            action_requested_display(&request),
            "Action requested: search src for \"TODO\""
        );
        assert_eq!(
            action_completed_display(&request, &result),
            "Action completed: search src for \"TODO\" -> Failed (failed to search src: permission denied)."
        );

        assert_eq!(
            file_created_display(&json!({ "path": "README.md", "bytes": 128 })),
            "File created: README.md (128 bytes)."
        );
        assert_eq!(
            files_edited_display(&json!({ "changed_files": ["src/lib.rs", "src/main.rs"] })),
            "Files edited: src/lib.rs, src/main.rs."
        );
    }

    #[test]
    fn subtask_parser_requires_agent_and_task() {
        let (agent, task) = parse_subtask_command("explorer inspect docs").unwrap();
        assert_eq!(agent, "explorer");
        assert_eq!(task, "inspect docs");

        let error = parse_subtask_command("explorer").unwrap_err();
        assert!(error.to_string().contains("usage"));
    }

    #[tokio::test]
    async fn review_fix_cycle_limit_stops_before_extra_fixer_pass() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[limits]
max_review_fix_cycles = 1

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("review cycle create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::LimitReached);
        let events = app.state.events.join("\n");
        assert!(events.contains("Run review/fix cycle limit reached."));
        assert_eq!(events.matches("fixer:").count(), 2);
    }

    #[tokio::test]
    async fn agent_parse_error_gets_one_orchestrator_repair_attempt() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("agent parse error create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.state.events.join("\n");
        assert!(events.contains("Parse error represented as an agent result."));
        assert!(events.contains("Runtime parse error queued for Orchestrator repair."));
        assert!(events.contains("Run completed."));
        assert!(!events.contains("Run failed after an agent parse error."));
    }

    #[tokio::test]
    async fn repeated_parse_errors_fail_after_one_repair_attempt() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("always parse error create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Failed);
        let events = app.state.events.join("\n");
        assert_eq!(
            events
                .matches("Runtime parse error queued for Orchestrator repair.")
                .count(),
            1
        );
        assert!(events.contains("Run failed after an orchestrator parse error."));
    }

    #[tokio::test]
    async fn fake_runtime_action_request_is_executed_and_recorded() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "action context\n").unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("use action to create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.state.events.join("\n");
        assert!(events.contains("Action requested: read README.md"));
        assert!(events.contains("Action completed: read README.md -> Completed"));
        assert!(events.contains("Read 15 bytes from README.md"));
    }

    #[tokio::test]
    async fn tool_policy_denials_are_recorded_as_durable_events() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"
tools = ["list_files"]

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("use action to create a feature")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        let denied = events
            .iter()
            .find(|event| event.kind == "action_denied")
            .unwrap();
        assert!(denied
            .payload
            .get("diagnostic")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .contains("not allowed to use tool"));
    }

    #[tokio::test]
    async fn command_actions_record_command_history_events() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("command action create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let command_started = events
            .iter()
            .find(|event| event.kind == "command_started")
            .unwrap();
        assert_eq!(
            command_started
                .payload
                .get("command")
                .and_then(serde_json::Value::as_str),
            Some("pwd")
        );
        let command_completed = events
            .iter()
            .find(|event| event.kind == "command_completed")
            .unwrap();
        assert_eq!(
            command_completed
                .payload
                .get("command")
                .and_then(serde_json::Value::as_str),
            Some("pwd")
        );
        assert_eq!(
            command_completed
                .payload
                .get("status")
                .and_then(serde_json::Value::as_str),
            Some("completed")
        );
        let display_events = app.state.events.join("\n");
        assert!(display_events.contains("Action requested: run command pwd"));
        assert!(display_events.contains("Command completed: pwd -> Completed"));
        let command_items = app
            .state
            .chat_items
            .iter()
            .filter(|item| {
                item.kind == ChatItemKind::CommandResult && item.source.action_id.is_some()
            })
            .collect::<Vec<_>>();
        assert_eq!(command_items.len(), 1);
        assert_eq!(command_items[0].severity, ChatSeverity::Success);
        assert!(command_items[0]
            .body
            .iter()
            .any(|line| line.text.contains("$ pwd")));
    }

    #[tokio::test]
    async fn write_actions_record_file_edit_history_events() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("write action create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert_eq!(
            fs::read_to_string(dir.path().join("multiagent-action-output.txt")).unwrap(),
            "created by fake runtime\n"
        );
        let events = app.history.read_events().unwrap();
        let file_edit = events
            .iter()
            .find(|event| event.kind == "file_edit_applied")
            .unwrap();
        assert_eq!(
            file_edit
                .payload
                .get("operation")
                .and_then(serde_json::Value::as_str),
            Some("write_file")
        );
        assert_eq!(
            file_edit
                .payload
                .get("path")
                .and_then(serde_json::Value::as_str),
            Some("multiagent-action-output.txt")
        );
        let display_events = app.state.events.join("\n");
        assert!(display_events.contains("Action requested: write multiagent-action-output.txt"));
        assert!(display_events.contains("File created: multiagent-action-output.txt"));
        let file_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.kind == ChatItemKind::FileEdit && item.source.action_id.is_some())
            .unwrap();
        assert_eq!(file_item.severity, ChatSeverity::Success);
        assert!(file_item.title.contains("multiagent-action-output.txt"));
    }

    #[tokio::test]
    async fn step_action_limit_allows_final_response_after_last_allowed_action() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "action context\n").unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[limits]
max_step_actions = 1

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("use action to create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::Completed);
    }

    #[tokio::test]
    async fn normal_approval_mode_pauses_run_with_pending_approval() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("approval action create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.active_run_id.is_some());
        let pending = app.state.pending_approval.as_ref().unwrap();
        assert_eq!(pending.agent, "fixer");
        assert!(app.state.pending_clarification.is_none());
        assert!(app.pending_clarification.is_none());
        assert_eq!(agent_status(&app, "fixer"), "waiting_approval");
        // Diagnostic keeps the "requires action approval" phrasing and now appends
        // the plain-language risk reason that drove the gate (ADR-003).
        assert!(pending
            .diagnostic
            .as_ref()
            .unwrap()
            .contains("requires action approval"));
        let events = app.state.events.join("\n");
        assert!(events.contains("Action approval required."));
        assert!(!events.contains("Action completed: ApprovalRequired"));
        let approval_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.kind == ChatItemKind::Approval)
            .unwrap();
        assert_eq!(approval_item.status, ChatItemStatus::WaitingApproval);
        assert_eq!(approval_item.severity, ChatSeverity::Warning);
    }

    // ---- Session TrustStore (task_04) ---------------------------------

    #[test]
    fn trust_store_grant_contains_and_dedupes() {
        let mut store = TrustStore::default();
        let target = TrustTarget::Command("echo trusted-run".to_string());
        assert!(store.grant(target.clone()));
        assert!(store.contains(&target));
        // A second grant of the same target is a no-op.
        assert!(!store.grant(target.clone()));
        assert_eq!(store.list().len(), 1);
        // A different target is not trusted.
        assert!(!store.contains(&TrustTarget::Command("ls".to_string())));
    }

    #[test]
    fn trust_store_revoke_index_is_one_based_and_order_preserving() {
        let mut store = TrustStore::default();
        let first = TrustTarget::Command("cargo build".to_string());
        let second = TrustTarget::WritePath(PathBuf::from("/tmp/x.rs"));
        store.grant(first.clone());
        store.grant(second.clone());

        // Out-of-range indices return None and leave the store unchanged.
        assert_eq!(store.revoke_index(0), None);
        assert_eq!(store.revoke_index(3), None);
        assert_eq!(store.list().len(), 2);

        // 1-based removal of the first-listed entry.
        assert_eq!(store.revoke_index(1), Some(first));
        assert_eq!(store.list(), &[second]);
    }

    #[test]
    fn trust_store_clear_and_snapshot_reflect_entries() {
        let mut store = TrustStore::default();
        let target = TrustTarget::Command("cargo build".to_string());
        store.grant(target.clone());
        let snapshot = store.snapshot();
        assert!(snapshot.contains(&target));

        store.clear();
        assert!(store.is_empty());
        // The earlier snapshot is independent of later mutations.
        assert!(snapshot.contains(&target));
        assert!(store.snapshot().is_empty());
    }

    fn yolo_enforce_fake_config(dir: &Path) -> EffectiveConfig {
        let config_path = dir.join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "yolo"

[approval]
floor = "enforce"

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn untrusted_gray_area_command_raises_pending_approval() {
        // Control case: with nothing trusted, the gray-area command pauses.
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        let pending = app.state.pending_approval.as_ref().unwrap();
        assert_eq!(pending.agent, "fixer");
    }

    #[tokio::test]
    async fn trusted_gray_area_command_auto_runs_without_pending_approval() {
        // Pre-grant the exact command the fixer will emit; the enforcement matrix
        // returns AllowedByTrust, so the run completes with no modal.
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.trust_store
            .grant(TrustTarget::Command("echo trusted-run".to_string()));

        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        assert!(app.state.pending_approval.is_none());
        assert_ne!(app.state.run_state, RunState::WaitingForUser);
    }

    #[tokio::test]
    async fn enforce_floor_from_config_makes_gray_area_prompt_under_yolo() {
        // Proves config.approval.floor flows into the per-action context: under
        // Yolo a gray-area command would normally warn-and-run, but floor=enforce
        // turns it into a pending approval.
        let dir = tempdir().unwrap();
        let config = yolo_enforce_fake_config(dir.path());
        assert_eq!(config.approval.floor, crate::config::FloorPolicy::Enforce);
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_approval.is_some());
    }

    // ---- Approval resolution, trust grant & audit events (task_05) ----

    #[test]
    fn capped_preview_truncates_long_input() {
        assert_eq!(capped_preview("short"), "short");
        let long = "x".repeat(10_000);
        let capped = capped_preview(&long);
        assert!(capped.chars().count() < long.chars().count());
        assert!(capped.contains("truncated"));
    }

    #[tokio::test]
    async fn pending_view_is_populated_with_risk_fields() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        let pending = app.state.pending_approval.as_ref().unwrap();
        assert_eq!(pending.tier, Some(RiskTier::Medium));
        assert!(!pending.catastrophic);
        assert!(pending.reason.as_ref().is_some_and(|r| !r.is_empty()));
        assert_eq!(
            pending.resolved_command.as_deref(),
            Some("echo trusted-run")
        );
        assert_eq!(
            pending.trust_target,
            Some(TrustTarget::Command("echo trusted-run".to_string()))
        );
    }

    #[tokio::test]
    async fn approve_and_trust_grants_target_and_records_event() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();
        assert!(app.state.pending_approval.is_some());

        app.resolve_pending_approval(ApprovalResolution::ApproveAndTrust)
            .await
            .unwrap();

        assert!(app
            .trust_store
            .contains(&TrustTarget::Command("echo trusted-run".to_string())));
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "trust_granted"));
    }

    #[tokio::test]
    async fn approve_and_trust_is_noop_for_catastrophic_action() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("secret read action create a feature")
            .await
            .unwrap();

        let pending = app.state.pending_approval.as_ref().unwrap();
        assert!(pending.catastrophic);
        assert_eq!(pending.trust_target, None);

        app.resolve_pending_approval(ApprovalResolution::ApproveAndTrust)
            .await
            .unwrap();
        // Catastrophic exposes no target, so nothing is granted.
        assert!(app.trust_store.is_empty());
    }

    #[tokio::test]
    async fn approve_once_runs_without_granting_trust() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        app.resolve_pending_approval(ApprovalResolution::ApproveOnce)
            .await
            .unwrap();
        assert!(app.trust_store.is_empty());
    }

    #[tokio::test]
    async fn deny_produces_reason_and_resumes_the_run() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        let result = app
            .resolve_pending_approval(ApprovalResolution::Deny)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.status, ActionStatus::Denied);
        assert!(result.diagnostic.is_some_and(|reason| !reason.is_empty()));
        // Deny-and-continue: the run is no longer waiting on the user.
        assert!(app.state.pending_approval.is_none());
        assert_ne!(app.state.run_state, RunState::WaitingForUser);
    }

    #[tokio::test]
    async fn trusted_auto_run_records_approval_auto_resolved_event() {
        let dir = tempdir().unwrap();
        let config = approval_mode_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.trust_store
            .grant(TrustTarget::Command("echo trusted-run".to_string()));

        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        assert!(app.state.pending_approval.is_none());
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "approval_auto_resolved"));
    }

    #[tokio::test]
    async fn gray_area_under_yolo_warn_records_floor_warned_event() {
        let dir = tempdir().unwrap();
        // fake_config is Yolo with the default Warn floor.
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();

        // The gray-area command ran (no modal) and a warning was audited.
        assert!(app.state.pending_approval.is_none());
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "floor_warned"));
    }

    // ---- /trust list & revoke command (task_08) -----------------------

    #[tokio::test]
    async fn trust_list_empty_reports_no_trusted_actions() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        assert!(app.handle_trust_command("/trust").unwrap());
        let events = app.history.read_events().unwrap();
        let listed = events
            .iter()
            .find(|event| event.kind == "trust_listed")
            .unwrap();
        assert_eq!(listed.payload["count"], 0);
        assert!(listed.payload["message"]
            .as_str()
            .unwrap()
            .contains("No trusted actions"));
    }

    #[tokio::test]
    async fn trust_list_numbers_entries_with_session_scope() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.trust_store
            .grant(TrustTarget::Command("cargo build".to_string()));
        app.trust_store
            .grant(TrustTarget::WritePath(PathBuf::from("src/lib.rs")));

        assert!(app.handle_trust_command("/trust").unwrap());
        let events = app.history.read_events().unwrap();
        let message = events
            .iter()
            .find(|event| event.kind == "trust_listed")
            .unwrap()
            .payload["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(message.contains("this session only"));
        assert!(message.contains("1."));
        assert!(message.contains("2."));
        assert!(message.contains("cargo build"));
    }

    #[tokio::test]
    async fn trust_revoke_removes_first_entry_and_emits_event() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.trust_store
            .grant(TrustTarget::Command("cargo build".to_string()));
        app.trust_store
            .grant(TrustTarget::Command("npm install x".to_string()));

        assert!(app.handle_trust_command("/trust revoke 1").unwrap());
        assert!(!app
            .trust_store
            .contains(&TrustTarget::Command("cargo build".to_string())));
        assert!(app
            .trust_store
            .contains(&TrustTarget::Command("npm install x".to_string())));
        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "trust_revoked"));
    }

    #[tokio::test]
    async fn trust_malformed_input_is_rejected_without_mutation() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.trust_store
            .grant(TrustTarget::Command("cargo build".to_string()));

        assert!(app.handle_trust_command("/trust revoke abc").is_err());
        assert!(app.handle_trust_command("/trust revoke 9").is_err());
        // The store is untouched by malformed input.
        assert_eq!(app.trust_store.list().len(), 1);
    }

    #[tokio::test]
    async fn trust_clear_empties_store_and_emits_count() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.trust_store
            .grant(TrustTarget::Command("cargo build".to_string()));
        app.trust_store
            .grant(TrustTarget::Command("npm install x".to_string()));

        assert!(app.handle_trust_command("/trust clear").unwrap());
        assert!(app.trust_store.is_empty());
        let events = app.history.read_events().unwrap();
        let cleared = events
            .iter()
            .find(|event| event.kind == "trust_cleared")
            .unwrap();
        assert_eq!(cleared.payload["count"], 2);
    }

    #[tokio::test]
    async fn trust_revoke_re_arms_the_approval_prompt() {
        let dir = tempdir().unwrap();
        let mut app = App::new(approval_mode_config(dir.path())).await.unwrap();
        // Pre-trust so the first occurrence auto-runs without a modal.
        app.trust_store
            .grant(TrustTarget::Command("echo trusted-run".to_string()));
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();
        assert!(app.state.pending_approval.is_none());

        // Revoke the trust, then the same action prompts again (re-armed).
        assert!(app.handle_trust_command("/trust revoke 1").unwrap());
        assert!(app.trust_store.is_empty());
        app.submit_prompt("trusted run action create a feature")
            .await
            .unwrap();
        assert!(app.state.pending_approval.is_some());
    }

    // ---- First-run onboarding & Approvals help (task_09) --------------

    #[test]
    fn first_approval_explainer_copy_is_tier_aware() {
        let copy = crate::app::chat::FIRST_APPROVAL_EXPLAINER;
        assert!(copy.contains("risk-tiered"), "copy: {copy}");
        assert!(copy.to_lowercase().contains("catastrophic"), "copy: {copy}");
        assert!(copy.to_lowercase().contains("yolo"), "copy: {copy}");
    }

    #[tokio::test]
    async fn first_catastrophic_prompt_under_yolo_shows_explainer_once() {
        let dir = tempdir().unwrap();
        // fake_config is Yolo; a catastrophic action still surfaces an approval.
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.submit_prompt("secret read action create a feature")
            .await
            .unwrap();
        assert!(app.state.pending_approval.is_some());
        assert!(
            app.state.show_first_approval_explainer,
            "first surfaced approval should show the explainer"
        );

        // Resolve and trigger a second approval; the once-only latch suppresses it.
        app.resolve_pending_approval(ApprovalResolution::Deny)
            .await
            .unwrap();
        app.submit_prompt("secret read action create another feature")
            .await
            .unwrap();
        assert!(app.state.pending_approval.is_some());
        assert!(
            !app.state.show_first_approval_explainer,
            "explainer must not repeat once the latch is set"
        );
    }

    #[tokio::test]
    async fn app_state_defaults_to_no_pending_clarification() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let app = App::new(config).await.unwrap();

        assert!(app.state.pending_clarification.is_none());
        assert!(app.pending_clarification.is_none());
    }

    /// Seed an active run paused on a governance decision, mirroring the state
    /// the early-abort gate (task_05) will produce. Returns the decision id.
    fn seed_pending_governance_decision(app: &mut App, prompt: &str) -> String {
        let run = app.build_follow_up_run(prompt.to_string()).unwrap();
        let run_id = run.run_id.clone();
        let decision_id = "gov-decision-1".to_string();
        let view = GovernanceDecisionView {
            run_id: run_id.clone(),
            decision_id: decision_id.clone(),
            kind: crate::governance::GovernanceKind::EarlyAbort,
            title: "Confirm intent before this run edits files".to_string(),
            intent: "Create a feature".to_string(),
            approach: vec!["Implement it".to_string()],
            agent: Some("fixer".to_string()),
            write_scope: vec!["src".to_string()],
            risk_label: "Edits source files".to_string(),
            plan: None,
        };
        app.pending_governance_decision = Some(PendingGovernanceDecision { run });
        app.state.pending_governance_decision = Some(PendingGovernanceDecisionView {
            run_id,
            decision_id: decision_id.clone(),
            view,
        });
        app.state.run_state = RunState::WaitingForUser;
        app.publish_state();
        decision_id
    }

    #[tokio::test]
    async fn app_state_defaults_to_no_pending_governance_decision() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let app = App::new(config).await.unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert!(app.pending_governance_decision.is_none());
    }

    #[tokio::test]
    async fn resolve_pending_governance_decision_with_no_pending_errors() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        let err = app
            .resolve_pending_governance_decision("any-id", GovernanceAnswer::Accept)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("no governance decision is pending"));
    }

    #[tokio::test]
    async fn resolve_pending_governance_decision_mismatched_id_errors() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        seed_pending_governance_decision(&mut app, "create a feature");

        let err = app
            .resolve_pending_governance_decision("wrong-id", GovernanceAnswer::Accept)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("does not match pending governance decision"));
        // The pending decision is left untouched after a mismatch.
        assert!(app.state.pending_governance_decision.is_some());
        assert!(app.pending_governance_decision.is_some());
    }

    #[tokio::test]
    async fn resolve_pending_governance_decision_accept_resumes_run() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let decision_id = seed_pending_governance_decision(&mut app, "create a feature");

        app.resolve_pending_governance_decision(&decision_id, GovernanceAnswer::Accept)
            .await
            .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert!(app.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Completed);

        let history = app.history.read_events().unwrap();
        assert!(history.iter().any(|event| {
            event.kind == "governance_decision_resolved"
                && event.payload.get("outcome").and_then(|v| v.as_str()) == Some("accept")
        }));
        // The run was actually driven after acceptance.
        assert!(history
            .iter()
            .any(|event| event.kind == "orchestrator_decision"));
    }

    #[tokio::test]
    async fn resolve_pending_governance_decision_reject_redirect_redrives() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let decision_id = seed_pending_governance_decision(&mut app, "create a feature");

        app.resolve_pending_governance_decision(
            &decision_id,
            GovernanceAnswer::Reject {
                redirect: Some("focus on tests".to_string()),
            },
        )
        .await
        .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Completed);

        let history = app.history.read_events().unwrap();
        let resolved = history
            .iter()
            .find(|event| event.kind == "governance_decision_resolved")
            .unwrap();
        assert_eq!(
            resolved.payload.get("outcome").and_then(|v| v.as_str()),
            Some("reject")
        );
        assert_eq!(
            resolved.payload.get("redirect").and_then(|v| v.as_str()),
            Some("focus on tests")
        );
        // The redirected run was re-driven.
        assert!(history
            .iter()
            .any(|event| event.kind == "orchestrator_decision"));
    }

    #[tokio::test]
    async fn resolve_pending_governance_decision_reject_abort_does_not_resume() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let decision_id = seed_pending_governance_decision(&mut app, "create a feature");

        // A follow-up queued during the governance pause must not be left stuck when
        // the run aborts (mirrors the interrupt() path).
        app.handle_queue_command("/queue follow up later")
            .await
            .unwrap();

        app.resolve_pending_governance_decision(
            &decision_id,
            GovernanceAnswer::Reject { redirect: None },
        )
        .await
        .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert!(app.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Interrupted);
        assert!(app.state.active_run_id.is_none());

        // The queued follow-up was paused (not left Pending and stuck).
        assert_eq!(
            app.state.queued_follow_ups.first().map(|f| &f.status),
            Some(&QueuedFollowUpStatus::Paused)
        );

        let history = app.history.read_events().unwrap();
        assert!(history.iter().any(|event| {
            event.kind == "governance_decision_resolved"
                && event.payload.get("outcome").and_then(|v| v.as_str()) == Some("reject")
                && event.payload.get("redirect").is_none()
        }));
        assert!(history.iter().any(|event| event.kind == "run_interrupted"));
        assert!(history
            .iter()
            .any(|event| event.kind == "follow_up_replay_paused"));
        // The run was aborted before any orchestration ran.
        assert!(!history
            .iter()
            .any(|event| event.kind == "orchestrator_decision"));
    }

    #[tokio::test]
    async fn interrupt_clears_pending_governance_decision_state() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        seed_pending_governance_decision(&mut app, "create a feature");
        assert!(app.state.pending_governance_decision.is_some());
        assert!(app.pending_governance_decision.is_some());

        app.interrupt().unwrap();

        assert_eq!(app.state.run_state, RunState::Interrupted);
        assert!(app.state.active_run_id.is_none());
        assert!(app.state.pending_governance_decision.is_none());
        assert!(app.pending_governance_decision.is_none());
    }

    #[tokio::test]
    async fn pending_governance_decision_exposed_on_app_state_and_cleared_after_accept() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let decision_id = seed_pending_governance_decision(&mut app, "create a feature");

        // The pause is visible to the render path via AppState.
        let view = app.state.pending_governance_decision.as_ref().unwrap();
        assert_eq!(view.decision_id, decision_id);
        assert_eq!(
            Some(view.run_id.as_str()),
            app.state.active_run_id.as_deref()
        );
        assert_eq!(view.view.intent, "Create a feature");

        app.resolve_pending_governance_decision(&decision_id, GovernanceAnswer::Accept)
            .await
            .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Completed);
    }

    // ── task_05 early-abort gate ──

    fn first_turn_run() -> RunDriveContext {
        let mut run = RunDriveContext::new(
            "run".to_string(),
            None,
            "do work".to_string(),
            "do work".to_string(),
            None,
            None,
            None,
        );
        // The orchestrator step bumps step_count to 1 before the decision is
        // returned, so the gate hook sees step_count == 1 on the first turn.
        run.step_count = 1;
        run
    }

    fn single_agent_decision(
        required_capabilities: Vec<Capability>,
    ) -> crate::orchestrator::OrchestratorDecision {
        crate::orchestrator::OrchestratorDecision {
            schema_version: 1,
            decision_id: "dec".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Apply the change".to_string()],
            next_agent: Some("fixer".to_string()),
            next_step: None,
            reason: "Edit the config loader".to_string(),
            required_capabilities,
            stop_condition: "done".to_string(),
            clarifying_question: None,
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
            final_summary: None,
        }
    }

    #[test]
    fn early_abort_triggers_on_first_turn_single_agent_write() {
        let run = first_turn_run();
        let decision = single_agent_decision(vec![Capability::Read, Capability::Edit]);
        assert!(early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_triggers_on_command_capability() {
        let run = first_turn_run();
        let decision = single_agent_decision(vec![Capability::Read, Capability::Command]);
        assert!(early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_does_not_trigger_for_read_only_decision() {
        let run = first_turn_run();
        let decision = single_agent_decision(vec![Capability::Read]);
        assert!(!early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_does_not_trigger_past_the_first_turn() {
        let mut run = first_turn_run();
        run.step_count = 2;
        let decision = single_agent_decision(vec![Capability::Read, Capability::Edit]);
        assert!(!early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_does_not_trigger_with_previous_results() {
        let mut run = first_turn_run();
        run.previous_results.push(RunStepResult::Agent {
            result: AgentResult::completed("explorer", "step", "done"),
        });
        let decision = single_agent_decision(vec![Capability::Read, Capability::Edit]);
        assert!(!early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_does_not_trigger_for_a_subtask() {
        let mut run = first_turn_run();
        run.subtask = Some(SubtaskContext {
            agent_id: "fixer".to_string(),
            request: "do it".to_string(),
            submitted_request: "do it".to_string(),
        });
        let decision = single_agent_decision(vec![Capability::Read, Capability::Edit]);
        assert!(!early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_does_not_trigger_for_a_parallel_group() {
        let run = first_turn_run();
        let mut decision = single_agent_decision(vec![Capability::Read, Capability::Edit]);
        // A parallel group uses schema_version 2 with next_step (no next_agent).
        decision.schema_version = 2;
        decision.next_agent = None;
        decision.next_step = Some(DecisionNextStep::ParallelGroup(ParallelGroupPlan {
            group_id: "g".to_string(),
            reason: "split".to_string(),
            steps: Vec::new(),
        }));
        assert!(!early_abort_triggers(&run, &decision));
    }

    #[test]
    fn early_abort_does_not_trigger_for_non_continue_status() {
        let run = first_turn_run();
        let mut decision = single_agent_decision(vec![Capability::Read, Capability::Edit]);
        decision.status = DecisionStatus::Complete;
        assert!(!early_abort_triggers(&run, &decision));
    }

    async fn app_with_early_abort(dir: &std::path::Path) -> App {
        let mut config = fake_config(dir);
        config.features.governance_early_abort = true;
        App::new(config).await.unwrap()
    }

    #[tokio::test]
    async fn early_abort_pauses_first_turn_write_run_before_any_write() {
        let dir = tempdir().unwrap();
        let mut app = app_with_early_abort(dir.path()).await;

        app.submit_prompt("governance early abort write action")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_governance_decision.is_some());
        assert!(app.pending_governance_decision.is_some());

        let history = app.history.read_events().unwrap();
        assert!(history
            .iter()
            .any(|event| event.kind == "governance_decision_requested"));
        // Nothing was written before the pause.
        assert!(!history
            .iter()
            .any(|event| event.kind == "file_edit_applied"));
        // The single agent never started.
        assert!(!history.iter().any(|event| {
            event.kind == "agent_step_started"
                && event.payload.get("agent").and_then(|v| v.as_str()) == Some("fixer")
        }));
    }

    #[tokio::test]
    async fn early_abort_accept_lets_the_write_proceed() {
        let dir = tempdir().unwrap();
        let mut app = app_with_early_abort(dir.path()).await;
        app.submit_prompt("governance early abort write action")
            .await
            .unwrap();
        let decision_id = app
            .state
            .pending_governance_decision
            .as_ref()
            .unwrap()
            .decision_id
            .clone();

        app.resolve_pending_governance_decision(&decision_id, GovernanceAnswer::Accept)
            .await
            .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Completed);
        let history = app.history.read_events().unwrap();
        assert!(history
            .iter()
            .any(|event| event.kind == "file_edit_applied"));
        assert!(dir.path().join("multiagent-action-output.txt").exists());
    }

    #[tokio::test]
    async fn early_abort_reject_redirect_redrives_without_regating() {
        let dir = tempdir().unwrap();
        let mut app = app_with_early_abort(dir.path()).await;
        app.submit_prompt("governance early abort write action")
            .await
            .unwrap();
        let decision_id = app
            .state
            .pending_governance_decision
            .as_ref()
            .unwrap()
            .decision_id
            .clone();

        app.resolve_pending_governance_decision(
            &decision_id,
            GovernanceAnswer::Reject {
                redirect: Some("only touch the readme".to_string()),
            },
        )
        .await
        .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Completed);

        let history = app.history.read_events().unwrap();
        let resolved = history
            .iter()
            .find(|event| event.kind == "governance_decision_resolved")
            .unwrap();
        assert_eq!(
            resolved.payload.get("outcome").and_then(|v| v.as_str()),
            Some("reject")
        );
        assert_eq!(
            resolved.payload.get("redirect").and_then(|v| v.as_str()),
            Some("only touch the readme")
        );
        // Exactly one pause — the re-drive does not re-gate.
        assert_eq!(
            history
                .iter()
                .filter(|event| event.kind == "governance_decision_requested")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn early_abort_does_not_pause_read_only_first_turn() {
        let dir = tempdir().unwrap();
        let mut app = app_with_early_abort(dir.path()).await;

        app.submit_prompt("explore the project").await.unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        let history = app.history.read_events().unwrap();
        assert!(!history
            .iter()
            .any(|event| event.kind == "governance_decision_requested"));
    }

    #[tokio::test]
    async fn early_abort_no_pause_when_flag_off() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("governance early abort write action")
            .await
            .unwrap();

        assert!(app.state.pending_governance_decision.is_none());
        assert_eq!(app.state.run_state, RunState::Completed);
        let history = app.history.read_events().unwrap();
        assert!(!history
            .iter()
            .any(|event| event.kind == "governance_decision_requested"));
    }

    #[tokio::test]
    async fn early_abort_card_carries_a_legible_intent_and_approach() {
        // The orchestrator prompt nudges turn-one reason/plan (task_06); with a
        // well-formed decision the gate's card shows a non-empty intent and at
        // least one approach bullet.
        let dir = tempdir().unwrap();
        let mut app = app_with_early_abort(dir.path()).await;
        app.submit_prompt("governance early abort write action")
            .await
            .unwrap();

        let view = &app.state.pending_governance_decision.as_ref().unwrap().view;
        assert!(!view.intent.trim().is_empty(), "intent must be legible");
        assert!(
            !view.approach.is_empty(),
            "the card must show at least one approach bullet"
        );
    }

    #[tokio::test]
    async fn interrupt_clears_pending_clarification_state() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_clarification.is_some());
        assert!(app.pending_clarification.is_some());

        app.interrupt().unwrap();

        assert_eq!(app.state.run_state, RunState::Interrupted);
        assert!(app.state.active_run_id.is_none());
        assert!(app.state.pending_clarification.is_none());
        assert!(app.pending_clarification.is_none());
    }

    #[tokio::test]
    async fn clarifying_answer_resumes_waiting_run() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.active_run_id.is_some());
        assert!(app.state.pending_approval.is_none());
        assert_eq!(agent_status(&app, "orchestrator"), "waiting_for_user");
        let view = app.state.pending_clarification.as_ref().unwrap();
        assert_eq!(
            Some(view.run_id.as_str()),
            app.state.active_run_id.as_deref()
        );
        assert!(!view.question_id.is_empty());
        assert_eq!(
            view.question,
            "Which target or constraint should guide this run?"
        );
        assert_eq!(
            view.options
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            vec!["target_scope", "success_criteria", "constraints"]
        );
        assert_eq!(view.recommended_option_id.as_deref(), Some("target_scope"));
        let events = app.state.events.join("\n");
        assert!(events.contains("Orchestrator asked a clarifying question."));
        let history_events = app.history.read_events().unwrap();
        let requested = history_events
            .iter()
            .find(|event| event.kind == "clarification_requested")
            .unwrap();
        assert_eq!(
            requested.run_id.as_deref(),
            app.state.active_run_id.as_deref()
        );
        assert_eq!(
            requested.payload.get("question_id").and_then(Value::as_str),
            Some(view.question_id.as_str())
        );
        assert_eq!(
            requested.payload.get("question").and_then(Value::as_str),
            Some(view.question.as_str())
        );
        let requested_options = requested
            .payload
            .get("options")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(
            requested_options
                .iter()
                .map(|option| option.get("id").and_then(Value::as_str).unwrap())
                .collect::<Vec<_>>(),
            vec!["target_scope", "success_criteria", "constraints"]
        );
        assert_eq!(
            requested
                .payload
                .get("recommended_option_id")
                .and_then(Value::as_str),
            Some("target_scope")
        );
        let decision_event = history_events
            .iter()
            .find(|event| event.kind == "orchestrator_decision")
            .unwrap();
        let options = decision_event
            .payload
            .get("clarifying_options")
            .and_then(Value::as_array)
            .unwrap();
        let option_pairs = options
            .iter()
            .map(|option| {
                (
                    option.get("id").and_then(Value::as_str).unwrap(),
                    option.get("label").and_then(Value::as_str).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            option_pairs,
            vec![
                ("target_scope", "Clarify the target scope"),
                ("success_criteria", "Clarify success criteria"),
                ("constraints", "Clarify constraints"),
            ]
        );
        assert_eq!(
            decision_event
                .payload
                .get("recommended_option_id")
                .and_then(Value::as_str),
            Some("target_scope")
        );

        let question_id = view.question_id.clone();
        let answer = ClarificationAnswer {
            question_id,
            answer: "use the CLI path".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.active_run_id.is_none());
        assert!(app.state.pending_clarification.is_none());
        assert!(app.pending_clarification.is_none());
        assert_eq!(agent_status(&app, "orchestrator"), "idle");
        let events = app.state.events.join("\n");
        assert!(events.contains("You: use the CLI path"));
        assert!(events.contains("explorer:"));
        assert!(events.contains("Run completed."));
    }

    #[tokio::test]
    async fn clarification_flow_chat_items_never_use_approval_kind() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();

        let pending_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.kind == ChatItemKind::Clarification)
            .unwrap();
        assert_eq!(pending_item.status, ChatItemStatus::WaitingForUser);
        assert!(!app
            .state
            .chat_items
            .iter()
            .any(|item| item.kind == ChatItemKind::Approval));
        assert!(!app
            .state
            .chat_items
            .iter()
            .any(|item| item.status == ChatItemStatus::WaitingApproval));

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();
        app.handle_event(AppEvent::ClarificationAnswered(ClarificationAnswer {
            question_id,
            answer: "use the CLI path".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        }))
        .await
        .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let answered_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.kind == ChatItemKind::Clarification)
            .unwrap();
        assert_eq!(answered_item.status, ChatItemStatus::Completed);
        assert!(answered_item
            .body
            .iter()
            .any(|line| line.text == "Answer: use the CLI path"));
        assert!(!app
            .state
            .chat_items
            .iter()
            .any(|item| item.kind == ChatItemKind::Approval));
        assert!(!app
            .state
            .chat_items
            .iter()
            .any(|item| item.status == ChatItemStatus::WaitingApproval));
    }

    #[tokio::test]
    async fn clarifying_answer_can_start_with_slash() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id,
            answer: "/tmp/project".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answered = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answered
                .payload
                .get("answer")
                .and_then(serde_json::Value::as_str),
            Some("/tmp/project")
        );
        assert!(events.iter().all(|event| event.kind != "skills_loaded"));
    }

    #[tokio::test]
    async fn clarifying_answer_with_skill_reference_does_not_load_new_skill() {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "reviewer",
            Some("reviewer"),
            "Review workflow guidance.",
        );
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id,
            answer: "/skill:reviewer".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answered = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answered
                .payload
                .get("answer")
                .and_then(serde_json::Value::as_str),
            Some("/skill:reviewer")
        );
        assert!(events.iter().all(|event| event.kind != "skills_loaded"));
    }

    #[tokio::test]
    async fn clarification_resume_preserves_existing_skill_context_without_resolving_answer_skill()
    {
        let dir = tempdir().unwrap();
        write_project_skill(
            dir.path(),
            "base",
            Some("base"),
            "SENTINEL_BASE_SKILL_BODY_PRIVATE",
        );
        write_project_skill(
            dir.path(),
            "new",
            Some("new"),
            "SENTINEL_NEW_SKILL_BODY_PRIVATE",
        );
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("/skill:base needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let pending = app.pending_clarification.as_ref().unwrap();
        let resume_prompt = format!(
            "{}\n\nUser clarification: /skill:new use the CLI path",
            pending.run.prompt
        );
        let resume_request = app
            .runtime_request(
                &pending.run.run_id,
                "resume-step",
                RuntimePrompt::new(&resume_prompt, pending.run.skill_context.as_ref()),
                app.agent("orchestrator").unwrap().clone(),
                pending.run.previous_results.clone(),
                "orchestrator_decision",
            )
            .unwrap();
        assert_eq!(
            count_occurrences(&resume_request.prompt, "SENTINEL_BASE_SKILL_BODY_PRIVATE"),
            1
        );
        assert!(!resume_request
            .prompt
            .contains("SENTINEL_NEW_SKILL_BODY_PRIVATE"));
        assert!(resume_request
            .prompt
            .contains("/skill:new use the CLI path"));

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id,
            answer: "/skill:new use the CLI path".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let loaded_events = events
            .iter()
            .filter(|event| event.kind == "skills_loaded")
            .collect::<Vec<_>>();
        assert_eq!(loaded_events.len(), 1);
        assert_eq!(
            loaded_events[0].payload["skills"][0]["display_name"],
            Value::String("base".to_string())
        );
        let answered = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answered
                .payload
                .get("answer")
                .and_then(serde_json::Value::as_str),
            Some("/skill:new use the CLI path")
        );
    }

    #[tokio::test]
    async fn normal_prompt_cannot_answer_pending_approval() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("approval action create a feature")
            .await
            .unwrap();

        let error = app.submit_prompt("/skill:missing yes").await.unwrap_err();
        assert!(error.to_string().contains("waiting for action approval"));
        assert!(!error.to_string().contains("unknown skill"));
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_approval.is_some());
        let events = app.history.read_events().unwrap();
        assert!(events.iter().all(|event| event.kind != "skills_loaded"));
    }

    #[tokio::test]
    async fn interrupting_pending_approval_records_step_cancellation() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("approval action create a feature")
            .await
            .unwrap();
        let run_id = app.state.active_run_id.clone().unwrap();
        assert!(app.active_step.is_some());

        app.interrupt().unwrap();

        assert_eq!(app.state.run_state, RunState::Interrupted);
        assert!(app.state.active_run_id.is_none());
        assert!(app.state.pending_approval.is_none());
        assert!(app.active_step.is_none());
        assert_eq!(agent_status(&app, "fixer"), "interrupted");
        assert!(!app
            .state
            .chat_items
            .iter()
            .any(|item| item.status == ChatItemStatus::WaitingApproval));
        assert!(!app
            .state
            .chat_items
            .iter()
            .any(|item| item.title.contains("Approval required")));
        let events = app.history.read_events().unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "step_cancel_requested"));
        assert!(events.iter().any(|event| event.kind == "step_cancelled"));
        assert!(events.iter().any(|event| event.kind == "run_interrupted"));
        let run_record_path = dir
            .path()
            .join(".atelier")
            .join("runs")
            .join(format!("{run_id}.json"));
        let run_record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(run_record_path).unwrap()).unwrap();
        assert_eq!(
            run_record.get("state").and_then(serde_json::Value::as_str),
            Some("interrupted")
        );
    }

    #[tokio::test]
    async fn resolving_pending_approval_denial_resumes_and_completes_run() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            r#"
approval_mode = "normal"

[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("approval action create a feature")
            .await
            .unwrap();

        let result = app
            .resolve_pending_approval(ApprovalResolution::Deny)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.status, ActionStatus::Denied);
        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.active_run_id.is_none());
        assert!(app.state.pending_approval.is_none());
        let events = app.state.events.join("\n");
        assert!(events.contains("Action approval denied."));
        assert!(events
            .contains("Action completed: run command cargo install pretend-package -> Denied"));
        assert!(events.contains("fixer: Fake Fixer validated"));
        assert!(events.contains("reviewer: Fake reviewer step completed."));
        assert!(events.contains("Run completed."));
        let denied_item = app
            .state
            .chat_items
            .iter()
            .find(|item| item.source.action_id.is_some())
            .unwrap();
        assert_eq!(denied_item.status, ChatItemStatus::Denied);
        assert_eq!(denied_item.severity, ChatSeverity::Warning);
    }

    #[tokio::test]
    async fn large_action_outputs_are_spilled_to_history_artifacts() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("README.md"),
            "x".repeat(LARGE_ACTION_CONTENT_BYTES + 256),
        )
        .unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("use action to create a feature")
            .await
            .unwrap();

        let events = app.history.read_events().unwrap();
        assert!(events.iter().any(|event| event.kind == "artifact_written"));
        let action_completed = events
            .iter()
            .find(|event| {
                event.kind == "action_completed"
                    && event
                        .payload
                        .get("artifact")
                        .and_then(serde_json::Value::as_object)
                        .is_some()
            })
            .unwrap();
        assert!(action_completed.payload.get("content").unwrap().is_null());
        let artifact_path = action_completed
            .payload
            .get("artifact")
            .and_then(|artifact| artifact.get("path"))
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert!(dir.path().join(artifact_path).exists());
    }

    #[tokio::test]
    async fn large_search_results_keep_history_preview_when_spilled() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let request = ActionRequest {
            schema_version: 1,
            action_id: "action".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::SearchText,
            params: json!({ "query": "npm distribution plan", "path": "." }),
        };
        let matches = (0..240)
            .map(|index| {
                json!({
                    "path": format!("docs/file-{index}.md"),
                    "line": index + 1,
                    "text": "npm distribution plan ".repeat(8)
                })
            })
            .collect::<Vec<_>>();
        let result = ActionResult {
            schema_version: 1,
            action_id: "action".to_string(),
            status: ActionStatus::Completed,
            summary: "Found 240 matches for \"npm distribution plan\".".to_string(),
            content: Some(json!({
                "query": "npm distribution plan",
                "path": ".",
                "matches": matches
            })),
            artifact: None,
            diagnostic: None,
            risk: None,
            gate_outcome: crate::actions::GateOutcome::Normal,
        };

        let durable = app
            .action_result_for_history_with_group("run", None, "step", &request, &result)
            .unwrap();

        assert!(durable.artifact.is_some());
        let preview = durable.content.unwrap();
        assert_eq!(
            preview
                .get("matches")
                .and_then(serde_json::Value::as_array)
                .unwrap()
                .len(),
            SEARCH_TEXT_HISTORY_PREVIEW_MATCHES
        );
        assert_eq!(
            preview.get("total_matches").and_then(Value::as_u64),
            Some(240)
        );
        assert_eq!(
            preview.get("truncated").and_then(Value::as_bool),
            Some(true)
        );
        let artifact_path = durable
            .artifact
            .as_ref()
            .and_then(|artifact| artifact.get("path"))
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert!(dir.path().join(artifact_path).exists());
    }

    #[tokio::test]
    async fn clarification_answered_with_recommended_option() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id: question_id.clone(),
            answer: "Option A selected".to_string(),
            selected_option_id: Some("opt-a".to_string()),
            selected_option_label: Some("Option A".to_string()),
            answer_source: "recommended".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer.clone()))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answered = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answered.payload.get("answer_source"),
            Some(&Value::String("recommended".to_string()))
        );
        assert_eq!(
            answered.payload.get("selected_option_id"),
            Some(&Value::String("opt-a".to_string()))
        );
        assert_eq!(
            answered.payload.get("selected_option_label"),
            Some(&Value::String("Option A".to_string()))
        );
    }

    #[tokio::test]
    async fn clarification_answered_with_custom_text() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id: question_id.clone(),
            answer: "Use the custom answer path".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer.clone()))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answered = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answered.payload.get("answer_source"),
            Some(&Value::String("custom".to_string()))
        );
        assert_eq!(
            answered.payload.get("selected_option_id"),
            Some(&Value::Null)
        );
    }

    #[tokio::test]
    async fn clarification_answered_with_slash_prefixed_custom_answer() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id: question_id.clone(),
            answer: "/tmp/project".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        app.handle_event(AppEvent::ClarificationAnswered(answer.clone()))
            .await
            .unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answered = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answered
                .payload
                .get("answer")
                .and_then(serde_json::Value::as_str),
            Some("/tmp/project")
        );
    }

    #[tokio::test]
    async fn clarification_answered_rejects_wrong_question_id() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let answer = ClarificationAnswer {
            question_id: "wrong-id".to_string(),
            answer: "Some answer".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        let result = app
            .handle_event(AppEvent::ClarificationAnswered(answer.clone()))
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("question id does not match"));
        assert!(app.pending_clarification.is_some());
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
    }

    #[tokio::test]
    async fn clarification_answered_rejects_empty_answer() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let question_id = app
            .state
            .pending_clarification
            .as_ref()
            .unwrap()
            .question_id
            .clone();

        let answer = ClarificationAnswer {
            question_id: question_id.clone(),
            answer: "   ".to_string(),
            selected_option_id: None,
            selected_option_label: None,
            answer_source: "custom".to_string(),
        };

        let result = app
            .handle_event(AppEvent::ClarificationAnswered(answer.clone()))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
        assert!(app.pending_clarification.is_some());
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
    }

    #[tokio::test]
    async fn submit_prompt_blocked_while_clarification_pending() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        assert_eq!(app.state.run_state, RunState::WaitingForUser);

        let result = app.submit_prompt("some answer").await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("waiting for clarification"));
        assert!(app.pending_clarification.is_some());
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
    }

    fn sample_roster_row() -> RosterRow {
        RosterRow {
            agent_id: "explorer".to_string(),
            name: "Explorer".to_string(),
            accent_index: 2,
            activity: ActivityState::Active,
            runtime_model: "fake/default".to_string(),
            effort: "medium".to_string(),
            thinking: true,
            current_step: Some("scan the workspace".to_string()),
            elapsed: Some("1m 20s".to_string()),
            status: "running".to_string(),
        }
    }

    #[test]
    fn activity_state_serializes_active() {
        let json = serde_json::to_string(&ActivityState::Active).unwrap();
        assert_eq!(json, "\"active\"");
        let parsed: ActivityState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ActivityState::Active);
    }

    #[test]
    fn activity_state_serializes_needs_input() {
        let json = serde_json::to_string(&ActivityState::NeedsInput).unwrap();
        assert_eq!(json, "\"needs_input\"");
        let parsed: ActivityState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ActivityState::NeedsInput);
    }

    #[test]
    fn activity_state_serializes_stalled() {
        let json = serde_json::to_string(&ActivityState::Stalled).unwrap();
        assert_eq!(json, "\"stalled\"");
        let parsed: ActivityState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ActivityState::Stalled);
    }

    #[test]
    fn activity_state_serializes_idle() {
        let json = serde_json::to_string(&ActivityState::Idle).unwrap();
        assert_eq!(json, "\"idle\"");
        let parsed: ActivityState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ActivityState::Idle);
    }

    #[test]
    fn roster_row_serializes_with_all_fields() {
        let row = sample_roster_row();
        let json = serde_json::to_string(&row).unwrap();
        let parsed: RosterRow = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, row);
    }

    #[test]
    fn roster_row_serializes_with_optional_nones() {
        let row = RosterRow {
            current_step: None,
            elapsed: None,
            activity: ActivityState::Idle,
            ..sample_roster_row()
        };
        let value: Value = serde_json::to_value(&row).unwrap();
        assert!(value["current_step"].is_null());
        assert!(value["elapsed"].is_null());
        let parsed: RosterRow = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, row);
    }

    #[tokio::test]
    async fn app_state_after_construction_has_roster_row_per_agent() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let app = App::new(config).await.unwrap();
        // Construction publishes once, so `roster_rows` is rebuilt to one row per
        // canonical agent (ADR-003); with no live steps every row is Idle.
        assert_eq!(app.state().roster_rows.len(), app.state().agents.len());
        assert!(app
            .state()
            .roster_rows
            .iter()
            .all(|row| row.activity == ActivityState::Idle));
    }

    #[tokio::test]
    async fn publish_state_carries_roster_rows_through_watch() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();
        let (sender, mut receiver) = watch::channel(app.state().clone());
        app.attach_state_sender(sender);

        // `publish_state` rebuilds `roster_rows` from agents/live_steps (ADR-003),
        // so any manually-set rows are replaced by the freshly built canonical set.
        app.state_mut().roster_rows = vec![sample_roster_row()];
        app.publish_state();

        let state = receiver.borrow_and_update();
        // With no live steps every row is Idle and the orchestrator sorts first
        // (canonical order); the row count tracks the agent count.
        assert_eq!(state.roster_rows.len(), state.agents.len());
        assert_eq!(state.roster_rows[0].agent_id, "orchestrator");
        assert!(state
            .roster_rows
            .iter()
            .all(|row| row.activity == ActivityState::Idle));
    }

    // --- task_04: publish_state hook + refresh_roster_tick (ADR-003/004) ------

    #[tokio::test]
    async fn publish_state_marks_active_agent_from_live_step() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.state_mut().live_steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Streaming,
        )];
        app.step_timings
            .insert("step-1".to_string(), timing_entry(Instant::now(), 5, 0));

        app.publish_state();

        let row = app
            .state()
            .roster_rows
            .iter()
            .find(|row| row.agent_id == "explorer")
            .expect("explorer row");
        assert_eq!(row.activity, ActivityState::Active);
    }

    #[tokio::test]
    async fn refresh_roster_tick_is_noop_when_idle() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        assert_eq!(app.state().run_state, RunState::Idle);
        // Idle runs never change the roster, so the tick gate short-circuits.
        assert!(!app.refresh_roster_tick());
    }

    #[tokio::test]
    async fn refresh_roster_tick_publishes_on_elapsed_bucket_change() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.state_mut().run_state = RunState::Running;
        app.state_mut().live_steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Running,
        )];
        app.step_timings.insert(
            "step-1".to_string(),
            StepTiming {
                started_at: Instant::now() - Duration::from_secs(15),
                last_activity: Instant::now(),
            },
        );
        app.publish_state();
        let before = app
            .state()
            .roster_rows
            .iter()
            .find(|row| row.agent_id == "explorer")
            .and_then(|row| row.elapsed.clone());
        assert_eq!(before.as_deref(), Some("15s"));

        // Simulate elapsed advancing across a coarse bucket by aging started_at.
        app.step_timings.get_mut("step-1").unwrap().started_at =
            Instant::now() - Duration::from_secs(35);
        assert!(app.refresh_roster_tick(), "bucket moved -> should publish");

        let after = app
            .state()
            .roster_rows
            .iter()
            .find(|row| row.agent_id == "explorer")
            .and_then(|row| row.elapsed.clone());
        assert_eq!(after.as_deref(), Some("35s"));
        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn refresh_roster_tick_suppresses_identical_rebuild() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.state_mut().run_state = RunState::Running;
        app.state_mut().live_steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Running,
        )];
        // Sit mid-bucket so two back-to-back ticks render the same coarse elapsed.
        app.step_timings.insert(
            "step-1".to_string(),
            StepTiming {
                started_at: Instant::now() - Duration::from_millis(8_200),
                last_activity: Instant::now(),
            },
        );

        assert!(app.refresh_roster_tick(), "first tick publishes the change");
        assert!(
            !app.refresh_roster_tick(),
            "identical rebuild must be change-gated"
        );
    }

    #[tokio::test]
    async fn step_timing_cleared_on_terminal_status() {
        let dir = tempdir().unwrap();
        let mut app = App::new(fake_config(dir.path())).await.unwrap();
        app.state_mut().live_steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Running,
        )];
        app.step_timings
            .insert("step-1".to_string(), timing_entry(Instant::now(), 5, 0));
        assert!(app.step_timings.contains_key("step-1"));

        app.set_live_step_status("step-1", LiveStepStatus::Completed);

        assert!(
            !app.step_timings.contains_key("step-1"),
            "a finished step must drop its timing entry"
        );
    }

    // --- build_roster_rows builder (task_03, ADR-003/004/005) -----------------

    fn roster_agent(id: &str, name: &str) -> AgentView {
        AgentView {
            id: id.to_string(),
            name: name.to_string(),
            runtime: "fake".to_string(),
            model: "default".to_string(),
            effort: "medium".to_string(),
            thinking: false,
            capabilities: Vec::new(),
            availability: None,
            status: "idle".to_string(),
        }
    }

    fn roster_live_step(agent: &str, step_id: &str, status: LiveStepStatus) -> LiveStepView {
        LiveStepView {
            run_id: "run-1".to_string(),
            group_id: None,
            step_id: step_id.to_string(),
            step_label: Some(format!("{agent} step")),
            file_scope: None,
            agent: agent.to_string(),
            status,
            streams: Vec::new(),
        }
    }

    /// A timing entry stamped `started_secs` ago, last active `idle_secs` ago,
    /// relative to the `now` the test will pass to `build_roster_rows`.
    fn timing_entry(now: Instant, started_secs: u64, idle_secs: u64) -> StepTiming {
        StepTiming {
            started_at: now - Duration::from_secs(started_secs),
            last_activity: now - Duration::from_secs(idle_secs),
        }
    }

    #[test]
    fn classify_active_within_threshold() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Streaming,
        )];
        let mut timing = BTreeMap::new();
        timing.insert("step-1".to_string(), timing_entry(now, 5, 0));

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].activity, ActivityState::Active);
        assert_eq!(rows[0].elapsed.as_deref(), Some("5s"));
        assert_eq!(rows[0].current_step.as_deref(), Some("explorer step"));
    }

    #[test]
    fn classify_stalled_at_exactly_threshold() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Running,
        )];
        let mut timing = BTreeMap::new();
        // started 30s ago, last activity exactly 30s ago -> stalled, not active.
        timing.insert("step-1".to_string(), timing_entry(now, 30, 30));

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].activity, ActivityState::Stalled);
        // Elapsed still shown on stalled rows.
        assert_eq!(rows[0].elapsed.as_deref(), Some("30s"));
    }

    #[test]
    fn classify_stalled_after_threshold() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Streaming,
        )];
        let mut timing = BTreeMap::new();
        timing.insert("step-1".to_string(), timing_entry(now, 35, 35));

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].activity, ActivityState::Stalled);
        assert_eq!(rows[0].elapsed.as_deref(), Some("35s"));
    }

    #[test]
    fn classify_needs_input_from_waiting_for_approval() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::WaitingForApproval,
        )];
        let timing = BTreeMap::new();

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].activity, ActivityState::NeedsInput);
        assert_eq!(rows[0].elapsed, None);
        assert_eq!(rows[0].current_step, None);
    }

    #[test]
    fn classify_needs_input_from_waiting_for_action() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::WaitingForAction,
        )];
        let timing = BTreeMap::new();

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].activity, ActivityState::NeedsInput);
    }

    #[test]
    fn classify_idle_when_no_step() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let rows = build_roster_rows(&agents, &[], &BTreeMap::new(), now);

        assert_eq!(rows[0].activity, ActivityState::Idle);
        assert_eq!(rows[0].elapsed, None);
        assert_eq!(rows[0].current_step, None);
    }

    #[test]
    fn classify_terminal_status_preserves_label() {
        let now = Instant::now();
        let mut agent = roster_agent("explorer", "Explorer");
        agent.status = "completed".to_string();
        let agents = vec![agent];
        let steps = vec![roster_live_step(
            "explorer",
            "step-1",
            LiveStepStatus::Completed,
        )];
        let timing = BTreeMap::new();

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].activity, ActivityState::Idle);
        assert_eq!(rows[0].status, "completed");
        assert_eq!(rows[0].elapsed, None);
        assert_eq!(rows[0].current_step, None);
    }

    #[test]
    fn elapsed_formatter_edge_cases() {
        assert_eq!(format_coarse_elapsed(Duration::from_secs(8)), "8s");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(59)), "59s");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(60)), "1m");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(80)), "1m 20s");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(120)), "2m");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(125)), "2m 5s");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(3600)), "1h");
        assert_eq!(format_coarse_elapsed(Duration::from_secs(3900)), "1h 5m");
        // Sub-minute remainder under an hour boundary collapses cleanly.
        assert_eq!(format_coarse_elapsed(Duration::from_secs(3661)), "1h 1m");
    }

    #[test]
    fn needs_input_pin_preserves_canonical_accent_index() {
        let now = Instant::now();
        // Canonical order: [explorer, fixer] (alphabetical, no orchestrator here).
        let agents = vec![
            roster_agent("explorer", "Explorer"),
            roster_agent("fixer", "Fixer"),
        ];
        let steps = vec![
            roster_live_step("explorer", "step-e", LiveStepStatus::Streaming),
            roster_live_step("fixer", "step-f", LiveStepStatus::WaitingForApproval),
        ];
        let mut timing = BTreeMap::new();
        timing.insert("step-e".to_string(), timing_entry(now, 5, 0));

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        // fixer (NeedsInput) pins to the top...
        assert_eq!(rows[0].agent_id, "fixer");
        assert_eq!(rows[1].agent_id, "explorer");
        // ...but keeps its canonical accent_index of 1 (not 0).
        assert_eq!(rows[0].accent_index, 1);
        assert_eq!(rows[1].accent_index, 0);
    }

    #[test]
    fn parallel_group_multiple_active_rows_each_correct() {
        let now = Instant::now();
        let agents = vec![
            roster_agent("explorer", "Explorer"),
            roster_agent("fixer", "Fixer"),
        ];
        let mut explorer_step = roster_live_step("explorer", "step-e", LiveStepStatus::Running);
        explorer_step.group_id = Some("group-1".to_string());
        let mut fixer_step = roster_live_step("fixer", "step-f", LiveStepStatus::Streaming);
        fixer_step.group_id = Some("group-1".to_string());
        let steps = vec![explorer_step, fixer_step];

        let mut timing = BTreeMap::new();
        timing.insert("step-e".to_string(), timing_entry(now, 10, 0));
        timing.insert("step-f".to_string(), timing_entry(now, 80, 0));

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        // No NeedsInput, so canonical order is preserved with stable accents.
        assert_eq!(rows[0].agent_id, "explorer");
        assert_eq!(rows[0].accent_index, 0);
        assert_eq!(rows[0].activity, ActivityState::Active);
        assert_eq!(rows[0].elapsed.as_deref(), Some("10s"));

        assert_eq!(rows[1].agent_id, "fixer");
        assert_eq!(rows[1].accent_index, 1);
        assert_eq!(rows[1].activity, ActivityState::Active);
        assert_eq!(rows[1].elapsed.as_deref(), Some("1m 20s"));
    }

    #[test]
    fn missing_step_label_falls_back_to_step_id() {
        let now = Instant::now();
        let agents = vec![roster_agent("explorer", "Explorer")];
        let mut step = roster_live_step("explorer", "step-1", LiveStepStatus::Streaming);
        step.step_label = None;
        let steps = vec![step];
        let mut timing = BTreeMap::new();
        timing.insert("step-1".to_string(), timing_entry(now, 5, 0));

        let rows = build_roster_rows(&agents, &steps, &timing, now);

        assert_eq!(rows[0].current_step.as_deref(), Some("step-1"));
    }

    #[test]
    fn build_roster_rows_preserves_canonical_order_without_pin() {
        let now = Instant::now();
        let agents = vec![
            roster_agent("orchestrator", "Orchestrator"),
            roster_agent("explorer", "Explorer"),
            roster_agent("fixer", "Fixer"),
        ];
        let rows = build_roster_rows(&agents, &[], &BTreeMap::new(), now);

        let order: Vec<&str> = rows.iter().map(|r| r.agent_id.as_str()).collect();
        assert_eq!(order, vec!["orchestrator", "explorer", "fixer"]);
        for (idx, row) in rows.iter().enumerate() {
            assert_eq!(row.accent_index, idx);
        }
    }

    // --- StepTiming lifecycle (task_02, ADR-004) ------------------------------

    #[tokio::test]
    async fn step_timing_stamped_on_registration() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run-1", "step-1", "explorer");

        let timing = app
            .step_timings
            .get("step-1")
            .expect("registering an active step stamps a timing entry");
        // Both timestamps start equal at registration time.
        assert_eq!(timing.started_at, timing.last_activity);
    }

    #[tokio::test]
    async fn step_timing_bumped_on_stream_keeps_started_at() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run-1", "step-1", "explorer");
        // Backdate the entry so the bump is observably later than `started_at`.
        let backdated = Instant::now() - Duration::from_secs(10);
        {
            let timing = app.step_timings.get_mut("step-1").unwrap();
            timing.started_at = backdated;
            timing.last_activity = backdated;
        }

        app.push_live_stream_content("step-1", "stdout".to_string(), "hi".to_string(), 1, false);

        let timing = app.step_timings.get("step-1").unwrap();
        assert_eq!(
            timing.started_at, backdated,
            "stream arrival must not move started_at"
        );
        assert!(
            timing.last_activity > timing.started_at,
            "stream arrival must advance last_activity"
        );
    }

    #[tokio::test]
    async fn step_timing_bumped_on_active_status_transition() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run-1", "step-1", "explorer");
        let backdated = Instant::now() - Duration::from_secs(10);
        {
            let timing = app.step_timings.get_mut("step-1").unwrap();
            timing.started_at = backdated;
            timing.last_activity = backdated;
        }

        app.set_live_step_status("step-1", LiveStepStatus::Running);

        let timing = app.step_timings.get("step-1").unwrap();
        assert_eq!(timing.started_at, backdated);
        assert!(
            timing.last_activity > timing.started_at,
            "Running transition must advance last_activity"
        );
    }

    #[tokio::test]
    async fn step_timing_not_bumped_on_terminal_status_transition() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run-1", "step-1", "explorer");
        let backdated = Instant::now() - Duration::from_secs(10);
        {
            let timing = app.step_timings.get_mut("step-1").unwrap();
            timing.started_at = backdated;
            timing.last_activity = backdated;
        }

        // Terminal/waiting transitions are not active states, so they must not
        // refresh the stall signal (the entry persists until clear_active_step).
        app.set_live_step_status("step-1", LiveStepStatus::WaitingForApproval);

        let timing = app.step_timings.get("step-1").unwrap();
        assert_eq!(
            timing.last_activity, backdated,
            "non-active status transition must leave last_activity untouched"
        );
    }

    #[tokio::test]
    async fn step_timing_cleared_on_step_end() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run-1", "step-1", "explorer");
        assert!(app.step_timings.contains_key("step-1"));

        app.clear_active_step("step-1");

        assert!(
            !app.step_timings.contains_key("step-1"),
            "clearing a step must remove its timing entry"
        );
    }

    #[tokio::test]
    async fn step_timing_parallel_steps_tracked_independently() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        // Two concurrent steps in a parallel group, keyed by distinct step_ids.
        app.set_active_step_with_metadata(
            "run-1",
            Some("group-1".to_string()),
            "step-a",
            None,
            None,
            "explorer",
        );
        app.set_active_step_with_metadata(
            "run-1",
            Some("group-1".to_string()),
            "step-b",
            None,
            None,
            "fixer",
        );

        // Backdate both to a common baseline, then bump only step-a.
        let backdated = Instant::now() - Duration::from_secs(10);
        for step_id in ["step-a", "step-b"] {
            let timing = app.step_timings.get_mut(step_id).unwrap();
            timing.started_at = backdated;
            timing.last_activity = backdated;
        }

        app.push_live_stream_content("step-a", "stdout".to_string(), "tick".to_string(), 1, false);

        let a = *app.step_timings.get("step-a").unwrap();
        let b = *app.step_timings.get("step-b").unwrap();
        assert!(
            a.last_activity > b.last_activity,
            "streaming step-a must not touch step-b's timing"
        );
        assert_eq!(
            b.last_activity, backdated,
            "the quiet peer keeps its original last_activity"
        );
    }

    #[tokio::test]
    async fn step_timing_multi_step_lifecycle_through_app_layer() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step_with_metadata(
            "run-1",
            Some("group-1".to_string()),
            "step-a",
            None,
            None,
            "explorer",
        );
        app.set_active_step_with_metadata(
            "run-1",
            Some("group-1".to_string()),
            "step-b",
            None,
            None,
            "fixer",
        );

        let backdated = Instant::now() - Duration::from_secs(10);
        for step_id in ["step-a", "step-b"] {
            let timing = app.step_timings.get_mut(step_id).unwrap();
            timing.started_at = backdated;
            timing.last_activity = backdated;
        }

        // Stream to the first, status-transition the second: both advance, each
        // independently from its own baseline.
        app.push_live_stream_content("step-a", "stdout".to_string(), "a".to_string(), 1, false);
        app.set_live_step_status("step-b", LiveStepStatus::Running);

        assert!(app.step_timings.get("step-a").unwrap().last_activity > backdated);
        assert!(app.step_timings.get("step-b").unwrap().last_activity > backdated);

        // Clearing one leaves the other intact.
        app.clear_active_step("step-a");
        assert!(!app.step_timings.contains_key("step-a"));
        assert!(app.step_timings.contains_key("step-b"));
    }

    #[tokio::test]
    async fn step_timing_never_leaks_into_serialized_state() {
        let dir = tempdir().unwrap();
        let config = fake_config(dir.path());
        let mut app = App::new(config).await.unwrap();

        app.set_active_step("run-1", "step-1", "explorer");
        app.push_live_stream_content("step-1", "stdout".to_string(), "hi".to_string(), 1, false);

        let json = serde_json::to_string(app.state()).unwrap();
        assert!(
            !json.contains("step_timings"),
            "the timing map must not be serialized onto AppState"
        );
        assert!(
            !json.contains("last_activity") && !json.contains("started_at"),
            "internal timing fields must not leak into serialized state (ADR-004)"
        );
    }
}
