use crate::actions::{
    execute_action_request, is_vcs_mutation, validate_action_request,
    vcs_action_explicitly_requested, ActionDecision, ActionExecutionContext, ActionKind,
    ActionRequest, ActionResult, ActionStatus,
};
use crate::config::{AgentProfile, ApprovalMode, EffectiveConfig, Limit};
use crate::history::{HistoryEvent, HistoryStore};
use crate::ids::new_id;
use crate::orchestrator::{
    validate_orchestrator_decision, AgentResult, AgentResultStatus, DecisionStatus, RunState,
};
use crate::runtime::{
    check_all_runtime_availability, execute_runtime_step, RuntimeAvailability, RuntimeHistoryEvent,
    RuntimeOutput, RuntimeRequest, RuntimeStreamDelta,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::sync::watch;

const LARGE_ACTION_CONTENT_BYTES: usize = 8 * 1024;
const RUNTIME_HISTORY_EVENT_LIMIT: usize = 100;
const RUNTIME_HISTORY_PAYLOAD_DEPTH: usize = 3;
const RUNTIME_HISTORY_PAYLOAD_FIELDS: usize = 20;
const RUNTIME_HISTORY_PAYLOAD_ITEMS: usize = 20;
const RUNTIME_HISTORY_STRING_CHARS: usize = 512;

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
    pub pending_approval: Option<PendingApprovalView>,
    pub agents: Vec<AgentView>,
    pub events: Vec<String>,
    pub input: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingApprovalView {
    pub run_id: String,
    pub step_id: String,
    pub action_id: String,
    pub agent: String,
    pub summary: String,
    pub diagnostic: Option<String>,
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
    pending_approval: Option<PendingApproval>,
    pending_clarification: Option<PendingClarification>,
    active_step: Option<ActiveStep>,
    debug_enabled: bool,
    session_ended: bool,
    state_sender: Option<watch::Sender<AppState>>,
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
struct PendingClarification {
    run: RunDriveContext,
}

#[derive(Clone, Debug)]
struct ActiveStep {
    run_id: String,
    step_id: String,
    agent: String,
}

#[derive(Clone, Debug)]
struct RunDriveContext {
    run_id: String,
    prompt: String,
    previous_results: Vec<AgentResult>,
    step_count: u32,
    started_at: Instant,
    parse_repair_attempts: u32,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunRecord {
    schema_version: u32,
    run_id: String,
    session_id: String,
    prompt: String,
    state: RunState,
    results: Vec<AgentResult>,
}

impl App {
    pub async fn new(config: EffectiveConfig) -> Result<Self> {
        Self::new_with_debug(config, false).await
    }

    pub async fn new_with_debug(config: EffectiveConfig, debug_enabled: bool) -> Result<Self> {
        let history = HistoryStore::create(&config.working_directory)?;
        let availability = check_all_runtime_availability(&config).await;
        let state = AppState {
            session_id: history.session_id().to_string(),
            run_state: RunState::Idle,
            active_run_id: None,
            pending_approval: None,
            agents: build_agent_views(&config, &availability),
            events: Vec::new(),
            input: String::new(),
        };
        let mut app = Self {
            config,
            history,
            availability,
            state,
            pending_approval: None,
            pending_clarification: None,
            active_step: None,
            debug_enabled: debug_enabled || debug_enabled_from_env(),
            session_ended: false,
            state_sender: None,
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
        if matches!(
            self.state.run_state,
            RunState::Planning | RunState::Running | RunState::WaitingForUser
        ) {
            bail!("a run is already active");
        }

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
            json!({ "prompt": prompt }),
            user_event_display(&prompt),
        )?;

        let run = RunDriveContext {
            run_id,
            prompt,
            previous_results: Vec::new(),
            step_count: 0,
            started_at: Instant::now(),
            parse_repair_attempts: 0,
        };
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
            self.state.pending_approval = None;
            self.pending_approval = None;
            self.pending_clarification = None;
            self.active_step = None;
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
                }
            }
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
                let next_agent_id = decision
                    .next_agent
                    .clone()
                    .context("validated continue decision missing next_agent")?;
                if self.review_fix_cycle_limit_reached(run, &next_agent_id) {
                    self.stop_for_review_fix_cycle_limit(run)?;
                    return Ok(false);
                }
                match self.run_agent_step(run, &next_agent_id, None).await? {
                    AgentStepOutcome::Completed => Ok(true),
                    AgentStepOutcome::Paused | AgentStepOutcome::Stop => Ok(false),
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
                &run.prompt,
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
                    run.previous_results.push(result);
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
                &run.prompt,
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
                    run.previous_results.push(result);
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
                    run.previous_results.push(result);
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

    fn write_run_record(&mut self, run: &RunDriveContext) -> Result<()> {
        let record = RunRecord {
            schema_version: 1,
            run_id: run.run_id.clone(),
            session_id: self.history.session_id().to_string(),
            prompt: run.prompt.clone(),
            state: self.state.run_state.clone(),
            results: run.previous_results.clone(),
        };
        self.history.write_run_record(&run.run_id, &record)?;
        Ok(())
    }

    fn set_active_step(&mut self, run_id: &str, step_id: &str, agent: &str) {
        self.active_step = Some(ActiveStep {
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            agent: agent.to_string(),
        });
        self.set_agent_status(agent, "running");
    }

    fn clear_active_step(&mut self, step_id: &str) {
        let agent = self
            .active_step
            .as_ref()
            .filter(|step| step.step_id == step_id)
            .map(|step| step.agent.clone());
        if let Some(agent) = agent {
            self.set_agent_status(&agent, "idle");
            self.active_step = None;
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
        let Some(step) = self.active_step.clone() else {
            return Ok(());
        };
        self.set_agent_status(&step.agent, "interrupted");
        let payload = json!({ "agent": step.agent });
        self.record_event(
            Some(step.run_id.clone()),
            Some(step.step_id.clone()),
            "step_cancel_requested",
            payload.clone(),
            "Step cancellation requested.",
        )?;
        self.record_event(
            Some(step.run_id),
            Some(step.step_id),
            "step_cancelled",
            payload,
            "Step cancelled.",
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

    fn schedule_parse_repair(
        &mut self,
        run: &mut RunDriveContext,
        step_id: &str,
        agent: &str,
    ) -> Result<bool> {
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
        prompt: &str,
        agent_profile: AgentProfile,
        previous_results: Vec<AgentResult>,
        output_schema: &str,
    ) -> Result<RuntimeRequest> {
        Ok(RuntimeRequest {
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            prompt: prompt.to_string(),
            working_directory: self.config.working_directory.clone(),
            capability_constraints: agent_profile.capabilities.clone(),
            agent_profile,
            session_events: self.runtime_history_events()?,
            previous_results,
            action_results: Vec::new(),
            output_schema: output_schema.to_string(),
            limits: self.config.limits.clone(),
        })
    }

    fn runtime_history_events(&self) -> Result<Vec<RuntimeHistoryEvent>> {
        let events = self.history.read_events()?;
        let start = events.len().saturating_sub(RUNTIME_HISTORY_EVENT_LIMIT);
        Ok(events[start..].iter().map(runtime_history_event).collect())
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
        loop {
            let runtime_step = execute_runtime_step(&self.config, request.clone());
            let step_result = match await_with_step_limit(
                runtime_step,
                &self.config.limits.max_step_minutes,
                step_started_at,
            )
            .await
            {
                Some(output) => output?,
                None => {
                    self.stop_for_step_time_limit(run, &step_id, step_started_at)?;
                    return Ok(StepOutcome::LimitReached);
                }
            };
            self.record_runtime_stream_deltas(
                run,
                &step_id,
                &request.agent_profile.id,
                &step_result.stream_deltas,
            )?;

            match step_result.output {
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
                    let context = ActionExecutionContext {
                        working_directory: self.config.working_directory.clone(),
                        workspace: self.config.workspace.clone(),
                        approval_mode: self.config.approval_mode.clone(),
                        command_timeout: command_timeout(&self.config.limits.max_command_minutes),
                        user_prompt: Some(request.prompt.clone()),
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
                        self.set_agent_status(&request.agent_profile.id, "waiting_approval");
                        let view = PendingApprovalView {
                            run_id: run_id.to_string(),
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

    fn record_event(
        &mut self,
        run_id: Option<String>,
        step_id: Option<String>,
        kind: &str,
        payload: serde_json::Value,
        display: impl Into<String>,
    ) -> Result<()> {
        let event = HistoryEvent::new(
            self.history.session_id().to_string(),
            run_id,
            step_id,
            kind,
            payload,
        );
        self.history.append_event(&event)?;
        if self.debug_enabled {
            self.history.append_debug_event(&event)?;
        }
        self.state.events.push(display.into());
        self.publish_state();
        Ok(())
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
        let durable_result = self.action_result_for_history(run_id, step_id, request, result)?;
        self.record_event(
            Some(run_id.to_string()),
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
        self.record_event(
            Some(run_id.to_string()),
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
        for delta in deltas {
            let payload = self.runtime_stream_delta_payload(agent_id, delta)?;
            self.record_event(
                Some(run.run_id.clone()),
                Some(step_id.to_string()),
                "runtime_stream_delta",
                payload,
                format!("Runtime stream: {}", delta.stream),
            )?;
        }
        Ok(())
    }

    fn runtime_stream_delta_payload(
        &mut self,
        agent_id: &str,
        delta: &RuntimeStreamDelta,
    ) -> Result<serde_json::Value> {
        let mut payload = json!({
            "agent": agent_id,
            "sequence": delta.sequence,
            "stream": delta.stream,
            "final_delta": delta.final_delta,
            "content": delta.content
        });
        let bytes = delta.content.as_bytes();
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
        match request.kind {
            ActionKind::RunCommand => {
                self.record_command_completed(run_id, step_id, request, result)
            }
            ActionKind::ApplyPatch | ActionKind::WriteFile => {
                self.record_file_edit_applied(run_id, step_id, request, result)
            }
            _ => Ok(()),
        }
    }

    fn record_command_completed(
        &mut self,
        run_id: &str,
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
        self.record_event(
            Some(run_id.to_string()),
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

    fn record_file_edit_applied(
        &mut self,
        run_id: &str,
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

        self.record_event(
            Some(run_id.to_string()),
            Some(step_id.to_string()),
            "file_edit_applied",
            payload,
            display,
        )
    }

    fn action_result_for_history(
        &mut self,
        run_id: &str,
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
        self.record_event(
            Some(run_id.to_string()),
            Some(step_id.to_string()),
            "artifact_written",
            serde_json::to_value(&artifact)?,
            format!(
                "Large output for {} stored as an artifact.",
                action_target_display(request)
            ),
        )?;
        durable_result.content = None;
        durable_result.artifact = Some(serde_json::to_value(artifact)?);
        Ok(durable_result)
    }
}

fn runtime_history_event(event: &HistoryEvent) -> RuntimeHistoryEvent {
    let (payload, payload_truncated) = compact_history_payload(&event.kind, &event.payload);
    RuntimeHistoryEvent {
        schema_version: event.schema_version,
        event_id: event.event_id.clone(),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        step_id: event.step_id.clone(),
        timestamp: event.timestamp.clone(),
        kind: event.kind.clone(),
        payload,
        payload_truncated,
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
    for key in ["agent", "sequence", "stream", "final_delta", "artifact"] {
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

fn action_executable_without_approval(
    agent_profile: &AgentProfile,
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> bool {
    if !matches!(
        validate_action_request(
            agent_profile,
            &context.workspace,
            &context.approval_mode,
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

fn review_fix_cycle_count(results: &[AgentResult]) -> u32 {
    let mut saw_review_since_last_fix = false;
    let mut cycles = 0;
    for result in results {
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
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use std::fs;
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
"#,
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
                "create another feature",
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
        let run = RunDriveContext {
            run_id,
            prompt: "create a feature".to_string(),
            previous_results: Vec::new(),
            step_count: 0,
            started_at: Instant::now() - Duration::from_secs(61),
            parse_repair_attempts: 0,
        };

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
                "create a feature",
                orchestrator,
                Vec::new(),
                "orchestrator_decision",
            )
            .unwrap();
        app.state.active_run_id = Some(run_id.clone());
        app.state.run_state = RunState::Planning;
        let run = RunDriveContext {
            run_id,
            prompt: "create a feature".to_string(),
            previous_results: Vec::new(),
            step_count: 1,
            started_at: Instant::now(),
            parse_repair_attempts: 0,
        };
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
            AgentResult::completed("explorer", "s1", "explored"),
            AgentResult::completed("fixer", "s2", "initial fix"),
            AgentResult::completed("reviewer", "s3", "reviewed"),
            AgentResult::completed("fixer", "s4", "review fix"),
            AgentResult::completed("reviewer", "s5", "reviewed again"),
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

        let error = app.submit_prompt("yes").await.unwrap_err();
        assert!(error.to_string().contains("waiting for action approval"));
        assert_eq!(app.state.run_state, RunState::WaitingForUser);
        assert!(app.state.pending_approval.is_some());
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
}
