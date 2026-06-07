pub mod chat;

use self::chat::{ChatItemView, ChatProjection};
use crate::actions::{
    execute_action_request, is_vcs_mutation, vcs_action_explicitly_requested, ActionDecision,
    ActionExecutionContext, ActionKind, ActionRequest, ActionResult, ActionStatus,
};
use crate::config::{
    AgentProfile, AgentPromptMetadata, ApprovalMode, Capability, CouncilExecutionMode,
    CouncilMemberProfile, EffectiveConfig, Limit,
};
use crate::history::{HistoryEvent, HistoryStore};
use crate::ids::new_id;
use crate::orchestrator::{
    agent_results, build_orchestrator_prompt, validate_orchestrator_decision, AgentResult,
    AgentResultStatus, DecisionNextStep, DecisionStatus, ParallelBlockedScope,
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
use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::future::{pending, Future};
use std::path::Path;
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
    pub agents: Vec<AgentView>,
    pub chat_items: Vec<ChatItemView>,
    pub events: Vec<String>,
    pub input: String,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingApprovalView {
    pub run_id: String,
    pub group_id: Option<String>,
    pub step_id: String,
    pub action_id: String,
    pub agent: String,
    pub summary: String,
    pub diagnostic: Option<String>,
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

#[derive(Clone, Debug)]
pub struct ApprovalHandle {
    sender: watch::Sender<ApprovalSignal>,
}

impl ApprovalHandle {
    pub fn answer(&self, approved: bool) {
        let current = *self.sender.borrow();
        let _ = self.sender.send(ApprovalSignal {
            sequence: current.sequence.wrapping_add(1),
            approved,
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    PromptSubmitted(String),
    ApprovalAnswered(bool),
    InputCharacter(char),
    InputBackspace,
    RunInterruptRequested,
}

#[derive(Debug)]
pub struct App {
    config: EffectiveConfig,
    history: HistoryStore,
    availability: BTreeMap<String, RuntimeAvailability>,
    state: AppState,
    chat_projection: ChatProjection,
    pending_approval: Option<PendingApproval>,
    pending_clarification: Option<PendingClarification>,
    active_step: Option<ActiveStep>,
    active_steps: Vec<ActiveStep>,
    debug_enabled: bool,
    session_ended: bool,
    state_sender: Option<watch::Sender<AppState>>,
    interrupt_sender: watch::Sender<u64>,
    interrupt_receiver: watch::Receiver<u64>,
    approval_sender: watch::Sender<ApprovalSignal>,
    approval_receiver: watch::Receiver<ApprovalSignal>,
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
struct RunDriveContext {
    run_id: String,
    parent_run_id: Option<String>,
    submitted_prompt: String,
    prompt: String,
    skill_context: Option<SkillPromptContext>,
    subtask: Option<SubtaskContext>,
    previous_results: Vec<RunStepResult>,
    step_count: u32,
    started_at: Instant,
    parse_repair_attempts: u32,
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
    ) -> Self {
        Self {
            run_id: run_id.into(),
            parent_run_id,
            submitted_prompt: submitted_prompt.into(),
            prompt: prompt.into(),
            skill_context,
            subtask,
            previous_results: Vec::new(),
            step_count: 0,
            started_at: Instant::now(),
            parse_repair_attempts: 0,
        }
    }

    fn from_compiled(
        run_id: impl Into<String>,
        parent_run_id: Option<String>,
        compiled: CompiledPrompt,
        subtask: Option<SubtaskContext>,
    ) -> Self {
        Self::new(
            run_id,
            parent_run_id,
            compiled.submitted_prompt,
            compiled.user_prompt,
            compiled.skill_context,
            subtask,
        )
    }

    fn loaded_skill_metadata(&self) -> Vec<LoadedSkillMetadata> {
        self.skill_context
            .as_ref()
            .map(SkillPromptContext::metadata)
            .unwrap_or_default()
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
    approved: bool,
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

impl App {
    pub async fn new(config: EffectiveConfig) -> Result<Self> {
        Self::new_with_debug(config, false).await
    }

    pub async fn new_with_debug(config: EffectiveConfig, debug_enabled: bool) -> Result<Self> {
        let history = HistoryStore::create(&config.working_directory)?;
        let availability = check_all_runtime_availability(&config).await;
        let (interrupt_sender, interrupt_receiver) = watch::channel(0);
        let (approval_sender, approval_receiver) = watch::channel(ApprovalSignal {
            sequence: 0,
            approved: false,
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
            agents: build_agent_views(&config, &availability),
            chat_items: Vec::new(),
            events: Vec::new(),
            input: String::new(),
        };
        let mut app = Self {
            config,
            history,
            availability,
            state,
            chat_projection: ChatProjection::new(),
            pending_approval: None,
            pending_clarification: None,
            active_step: None,
            active_steps: Vec::new(),
            debug_enabled: debug_enabled || debug_enabled_from_env(),
            session_ended: false,
            state_sender: None,
            interrupt_sender,
            interrupt_receiver,
            approval_sender,
            approval_receiver,
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

    pub async fn handle_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::PromptSubmitted(prompt) => {
                self.state.input.clear();
                self.publish_state();
                self.submit_prompt(prompt).await
            }
            AppEvent::ApprovalAnswered(approved) => {
                self.state.input.clear();
                self.publish_state();
                self.resolve_pending_approval(approved).await?;
                Ok(())
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
        self.session_ended = true;
        Ok(())
    }

    pub async fn submit_prompt(&mut self, prompt: impl Into<String>) -> Result<()> {
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
        if self.handle_subtask_command(&prompt).await? {
            return Ok(());
        }
        if matches!(self.state.run_state, RunState::WaitingForUser) {
            if self.state.pending_approval.is_some() {
                bail!("a run is waiting for action approval");
            }
            if let Some(mut pending) = self.pending_clarification.take() {
                let answer = prompt.trim().to_string();
                self.record_event(
                    Some(pending.run.run_id.clone()),
                    None,
                    "clarification_answered",
                    json!({ "answer": answer }),
                    user_event_display(&answer),
                )?;
                pending.run.prompt =
                    format!("{}\n\nUser clarification: {}", pending.run.prompt, answer);
                self.state.run_state = RunState::Planning;
                return self.drive_run(pending.run, None).await;
            }
        }
        reject_unknown_slash_command(&prompt)?;
        if matches!(
            self.state.run_state,
            RunState::Planning | RunState::Running | RunState::WaitingForUser
        ) {
            bail!("a run is already active");
        }
        let compiled_prompt = compile_app_prompt(&self.config.working_directory, &prompt)?;

        self.reset_enabled_agent_statuses();
        let run_id = new_id();
        self.state.active_run_id = Some(run_id.clone());
        self.state.run_state = RunState::Planning;
        self.record_event(
            Some(run_id.clone()),
            None,
            "run_started",
            json!({ "run_id": run_id }),
            "Run started.",
        )?;
        self.record_event(
            Some(run_id.clone()),
            None,
            "prompt_submitted",
            json!({
                "prompt": compiled_prompt.user_prompt.clone(),
                "submitted_prompt": compiled_prompt.submitted_prompt.clone(),
            }),
            user_event_display(&compiled_prompt.user_prompt),
        )?;
        self.record_skills_loaded(run_id.as_str(), compiled_prompt.skill_context.as_ref())?;

        let run = RunDriveContext::from_compiled(run_id, None, compiled_prompt, None);
        self.drive_run(run, None).await
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
        );
        self.drive_run(run, None).await
    }

    pub async fn resolve_pending_approval(
        &mut self,
        approved: bool,
    ) -> Result<Option<ActionResult>> {
        let Some(mut pending) = self.pending_approval.take() else {
            return Ok(None);
        };
        self.state.pending_approval = None;
        self.record_event(
            Some(pending.run_id.clone()),
            Some(pending.step_id.clone()),
            "approval_resolved",
            json!({
                "action_id": pending.action_request.action_id.clone(),
                "approved": approved
            }),
            if approved {
                "Action approval granted."
            } else {
                "Action approval denied."
            },
        )?;

        if self.wall_clock_limit_reached(&pending.run) {
            self.stop_for_wall_clock_limit(&pending.run)?;
            self.write_run_record(&pending.run)?;
            self.state.active_run_id = None;
            return Ok(None);
        }
        if self.step_time_limit_reached(pending.step_started_at) {
            self.stop_for_step_time_limit(&pending.run, &pending.step_id, pending.step_started_at)?;
            self.write_run_record(&pending.run)?;
            self.state.active_run_id = None;
            return Ok(None);
        }

        let result = if approved {
            pending.context.approval_mode = ApprovalMode::Yolo;
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
            &pending.action_request,
            &result,
        )?;
        pending.request.action_results.push(result.clone());
        let resume = StepResume {
            step: pending.step,
            step_started_at: pending.step_started_at,
            request: pending.request,
        };
        self.drive_run(pending.run, Some(resume)).await?;
        Ok(Some(result))
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
            self.pending_approval = None;
            self.pending_clarification = None;
            self.active_step = None;
            self.active_steps.clear();
            self.sync_chat_items();
            self.publish_state();
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
            self.write_run_record(&run)?;
            if !matches!(self.state.run_state, RunState::WaitingForUser) {
                self.state.active_run_id = None;
                self.publish_state();
            }
        }
        drive_result
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
                }
            }
            DecisionStatus::WaitingForUser => {
                self.state.run_state = RunState::WaitingForUser;
                self.pending_clarification = Some(PendingClarification { run: run.clone() });
                self.set_agent_status("orchestrator", "waiting_for_user");
                self.record_event(
                    Some(run.run_id.clone()),
                    None,
                    "blocker_reported",
                    json!({ "question": decision.clarifying_question }),
                    "Orchestrator asked a clarifying question.",
                )?;
                Ok(false)
            }
            DecisionStatus::Complete => {
                self.state.run_state = RunState::Completed;
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
                        self.publish_parallel_approval_head(&approval_queue);
                        if let Some(child) = children.get_mut(&pending.step_id) {
                            if child.terminal_result.is_none() {
                                self.resolve_parallel_approval(
                                    run,
                                    child,
                                    pending,
                                    approval.approved,
                                    resume_handle.clone(),
                                )
                                .await?;
                            }
                        }
                        self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
                        self.publish_parallel_approval_head(&approval_queue);
                    }
                    continue;
                }
                _ = limit_tick.tick() => {
                    if self.apply_parallel_limit_checks(run, &mut children, &cancellation)? {
                        self.drop_terminal_parallel_approvals(&children, &mut approval_queue);
                        self.publish_parallel_approval_head(&approval_queue);
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
                            self.publish_parallel_approval_head(&approval_queue);
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
        self.sync_chat_items();
        self.publish_state();
        if interrupted {
            return Ok(AgentStepOutcome::Stop);
        }
        Ok(AgentStepOutcome::Completed)
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
            approval_mode: self.config.approval_mode.clone(),
            command_timeout: command_timeout(&self.config.limits.max_command_minutes),
            user_prompt: Some(run.prompt.clone()),
            action_scope: crate::actions::ActionScope::ParallelFileScope(child.file_scope.clone()),
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
        approved: bool,
        resume_handle: ParallelRuntimeResumeHandle,
    ) -> Result<()> {
        self.record_event_with_group(
            Some(pending.run_id.clone()),
            Some(pending.group_id.clone()),
            Some(pending.step_id.clone()),
            "approval_resolved",
            json!({
                "action_id": pending.action_request.action_id.clone(),
                "approved": approved
            }),
            if approved {
                "Action approval granted."
            } else {
                "Action approval denied."
            },
        )?;

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
            pending.context.approval_mode = ApprovalMode::Yolo;
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

    fn publish_parallel_approval_head(&mut self, queue: &VecDeque<PendingParallelApproval>) {
        self.state.pending_approval = queue.front().map(|pending| PendingApprovalView {
            run_id: pending.run_id.clone(),
            group_id: Some(pending.group_id.clone()),
            step_id: pending.step_id.clone(),
            action_id: pending.action_request.action_id.clone(),
            agent: pending.agent_profile.id.clone(),
            summary: action_requested_display(&pending.action_request).to_string(),
            diagnostic: pending.reason.clone(),
        });
        self.sync_chat_items();
        self.publish_state();
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
                child.terminal_result.is_none()
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
        run: &RunDriveContext,
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
            agent_profile.instructions = build_orchestrator_prompt(&self.config);
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
                        approval_mode: self.config.approval_mode.clone(),
                        command_timeout: command_timeout(&self.config.limits.max_command_minutes),
                        user_prompt: Some(run.prompt.clone()),
                        action_scope: crate::actions::ActionScope::Unrestricted,
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
                        let view = PendingApprovalView {
                            run_id: run_id.to_string(),
                            group_id: None,
                            step_id: step_id.clone(),
                            action_id: action_request.action_id.clone(),
                            agent: request.agent_profile.id.clone(),
                            summary: result.summary.clone(),
                            diagnostic: result.diagnostic.clone(),
                        };
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
                    self.record_action_completed(run_id, &step_id, &action_request, &result)?;
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

    fn record_event_with_group(
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
        if self.debug_enabled {
            self.history.append_debug_event(&event)?;
        }
        self.chat_projection.apply_history_event(&event);
        self.sync_chat_items();
        self.state.events.push(display.into());
        self.publish_state();
        Ok(())
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
        self.chat_projection
            .apply_pending_approval(self.state.pending_approval.as_ref());
        self.state.chat_items = self.chat_projection.items().to_vec();
    }

    fn publish_state(&self) {
        if let Some(sender) = &self.state_sender {
            let _ = sender.send(self.state.clone());
        }
    }

    fn record_action_completed(
        &mut self,
        run_id: &str,
        step_id: &str,
        request: &ActionRequest,
        result: &ActionResult,
    ) -> Result<()> {
        self.record_action_completed_with_group(run_id, None, step_id, request, result)
    }

    fn record_action_completed_with_group(
        &mut self,
        run_id: &str,
        group_id: Option<&str>,
        step_id: &str,
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
        )
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
        live_step.status = status;
        if final_delta {
            for stream in &mut live_step.streams {
                stream.final_delta = true;
            }
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
    }
}

fn config_warning_messages(config: &EffectiveConfig) -> Vec<String> {
    let agents_without_fallbacks = config
        .agents
        .values()
        .filter(|agent| agent.enabled && agent.model_fallbacks.is_empty())
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    if agents_without_fallbacks.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "enabled agents without model_fallbacks: {}",
            agents_without_fallbacks.join(", ")
        )]
    }
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
    if !matches!(
        crate::actions::validate_action_request_with_scope(
            agent_profile,
            &context.workspace,
            &context.approval_mode,
            &context.action_scope,
            request
        ),
        ActionDecision::Allowed
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

fn config_status_display(status: &ConfigStatusView) -> String {
    let sources = status.sources.join(", ");
    let preset = status.preset.as_deref().unwrap_or("none");
    if status.warnings.is_empty() {
        format!("Config: sources: {sources}; preset: {preset}; warnings: none.")
    } else {
        format!(
            "Config: sources: {sources}; preset: {preset}; warnings: {}.",
            status.warnings.join("; ")
        )
    }
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

fn reject_unknown_slash_command(prompt: &str) -> Result<()> {
    let trimmed = prompt.trim();
    if !trimmed.starts_with('/') {
        return Ok(());
    }
    if is_named_prompt_prefix(trimmed, "/agent:") || trimmed.starts_with("/skill:") {
        return Ok(());
    }
    let command = trimmed.split_whitespace().next().unwrap_or(trimmed);
    bail!(
        "unknown command {command}. Available commands: /help, /goal, /goal clear, /config, /subtask <agent> <task>, /agent:<name>, /skill:<name>"
    )
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
    use crate::app::chat::{ChatItemKind, ChatItemStatus, ChatSeverity};
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use crate::skills::{LoadedSkill, SkillLoadErrorKind};
    use serde_json::Value;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn fake_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("multiagent.toml");
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

    fn fake_parallel_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("multiagent.toml");
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

    fn fake_parallel_normal_approval_config(dir: &std::path::Path) -> EffectiveConfig {
        let config_path = dir.join("multiagent.toml");
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
        let config_path = dir.join("multiagent.toml");
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
        let config_path = dir.join("multiagent.toml");
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
        let config_path = dir.join("multiagent.toml");
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
        assert!(dir.path().join(".multiagent/runs").exists());
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
        approval_handle.answer(false);

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
        let mut run = RunDriveContext::new("run", None, "prompt", "prompt", None, None);
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
        let mut run = RunDriveContext::new("run", None, "slow stream", "slow stream", None, None);
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
        let mut run = RunDriveContext::new("run", None, "fix a typo", "fix a typo", None, None);
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

        app.handle_event(AppEvent::PromptSubmitted("create a feature".to_string()))
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
        let config_path = dir.path().join("multiagent.toml");
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
        assert_eq!(app.state.run_state, RunState::Idle);
        assert!(app.state.active_run_id.is_none());
        let events = app.history.read_events().unwrap();
        assert!(!events
            .iter()
            .any(|event| event.kind == "run_started" || event.kind == "prompt_submitted"));
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
        assert!(run_started_index < prompt_submitted_index);
        assert!(prompt_submitted_index < skills_loaded_index);
        assert!(skills_loaded_index < runtime_index);

        let prompt = events
            .iter()
            .find(|event| event.kind == "prompt_submitted")
            .unwrap();
        assert_eq!(
            prompt
                .payload
                .get("prompt")
                .and_then(serde_json::Value::as_str),
            Some("inspect README")
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
        let runs_dir = dir.path().join(".multiagent/runs");
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
        assert_eq!(prompt.payload["prompt"], "please use here");
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
        assert_eq!(
            events
                .iter()
                .find(|event| event.kind == "prompt_submitted")
                .unwrap()
                .payload["prompt"],
            "do x"
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
        let debug_log = fs::read_to_string(dir.path().join(".multiagent/debug.log")).unwrap();
        let run_record =
            fs::read_to_string(dir.path().join(format!(".multiagent/runs/{run_id}.json"))).unwrap();
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
            .join(".multiagent")
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
            fs::read_to_string(dir.path().join(format!(".multiagent/runs/{run_id}.json"))).unwrap();
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
            fs::read_dir(dir.path().join(".multiagent/runs"))
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
            .join(".multiagent")
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
        let debug_log = dir.path().join(".multiagent/debug.log");
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
    async fn fake_runtime_respects_agent_step_limit() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
        assert_eq!(agent_status(&app, "fixer"), "waiting_approval");
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
        let events = app.state.events.join("\n");
        assert!(events.contains("Orchestrator asked a clarifying question."));

        app.submit_prompt("use the CLI path").await.unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        assert!(app.state.active_run_id.is_none());
        assert_eq!(agent_status(&app, "orchestrator"), "idle");
        let events = app.state.events.join("\n");
        assert!(events.contains("You: use the CLI path"));
        assert!(events.contains("explorer:"));
        assert!(events.contains("Run completed."));
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

        app.submit_prompt("/tmp/project").await.unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answer = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answer
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

        app.submit_prompt("/skill:reviewer").await.unwrap();

        assert_eq!(app.state.run_state, RunState::Completed);
        let events = app.history.read_events().unwrap();
        let answer = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answer
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

        app.submit_prompt("/skill:new use the CLI path")
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
        let answer = events
            .iter()
            .find(|event| event.kind == "clarification_answered")
            .unwrap();
        assert_eq!(
            answer
                .payload
                .get("answer")
                .and_then(serde_json::Value::as_str),
            Some("/skill:new use the CLI path")
        );
    }

    #[tokio::test]
    async fn normal_prompt_cannot_answer_pending_approval() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
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
        let config_path = dir.path().join("multiagent.toml");
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
            .join(".multiagent")
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
        let config_path = dir.path().join("multiagent.toml");
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

        let result = app.resolve_pending_approval(false).await.unwrap().unwrap();
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
        assert!(dir.path().join(".multiagent").join(artifact_path).exists());
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
        assert!(dir.path().join(".multiagent").join(artifact_path).exists());
    }
}
