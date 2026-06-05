use super::command_summary::{summarize_command, CommandSummaryInput};
use super::diff_preview::build_diff_preview;
use super::{
    ChatDetailRef, ChatItemKind, ChatItemStatus, ChatItemView, ChatLifecycleKey, ChatLineStyle,
    ChatLineView, ChatSeverity, ChatSourceRef,
};
use crate::app::{LiveStepStatus, LiveStepView, PendingApprovalView};
use crate::history::HistoryEvent;
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_TITLE_CHARS: usize = 160;
const MAX_SUMMARY_CHARS: usize = 240;
const MAX_BODY_LINES: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct ChatProjection {
    items: Vec<ChatItemView>,
    index: BTreeMap<ChatLifecycleKey, usize>,
    action_context: BTreeMap<String, ActionContext>,
    live_key: Option<ChatLifecycleKey>,
    pending_key: Option<ChatLifecycleKey>,
}

#[derive(Clone, Debug, Default)]
struct ActionContext {
    action_id: String,
    kind: Option<String>,
    command: Option<String>,
    diff: Option<String>,
    path: Option<String>,
    content_bytes: Option<u64>,
}

impl ChatProjection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild(events: &[HistoryEvent]) -> Self {
        let mut projection = Self::new();
        for event in events {
            projection.apply_history_event(event);
        }
        projection
    }

    pub fn apply_history_event(&mut self, event: &HistoryEvent) {
        match event.kind.as_str() {
            "session_started" | "session_ended" => {}
            "run_started" => self.apply_run_started(event),
            "prompt_submitted" | "clarification_answered" => self.apply_user_prompt(event),
            "orchestrator_decision" => self.apply_orchestrator_decision(event),
            "agent_step_started" => self.apply_agent_step_started(event),
            "runtime_stream_delta" => self.apply_runtime_stream_delta(event),
            "action_requested" => self.apply_action_requested(event),
            "command_started" => self.apply_command_started(event),
            "command_completed" => self.apply_command_completed(event),
            "file_edit_applied" => self.apply_file_edit_applied(event),
            "approval_requested" => self.apply_approval_requested(event),
            "approval_resolved" => self.apply_approval_resolved(event),
            "action_denied" => self.apply_action_denied(event),
            "action_completed" => self.apply_action_completed(event),
            "artifact_written" => self.apply_artifact_written(event),
            "agent_result" | "councillor_agent_result" | "councillor_result" => {
                self.apply_agent_result(event)
            }
            "run_completed" | "run_failed" | "run_limit_reached" | "run_interrupted"
            | "subtask_completed" => self.apply_run_summary(event),
            "diagnostic" => self.apply_diagnostic(event),
            "blocker_reported" => self.apply_blocker(event),
            "config_viewed"
            | "session_goal_viewed"
            | "session_goal_set"
            | "session_goal_cleared"
            | "subtask_started"
            | "council_started"
            | "council_synthesized"
            | "council_completed"
            | "step_cancel_requested"
            | "step_cancelled"
            | "orchestrator_decision_invalid" => self.apply_diagnostic(event),
            _ => {}
        }
    }

    pub fn apply_live_step(&mut self, live_step: Option<&LiveStepView>) {
        let Some(live_step) = live_step else {
            let previous_key = self.live_key.take();
            self.remove_transient_key(previous_key);
            return;
        };
        let key = ChatLifecycleKey::Step {
            run_id: live_step.run_id.clone(),
            step_id: live_step.step_id.clone(),
            item_kind: ChatItemKind::AgentProgress,
        };
        if self.live_key.as_ref() != Some(&key) {
            let previous_key = self.live_key.replace(key.clone());
            self.remove_transient_key(previous_key);
        }
        let mut body = Vec::new();
        if live_step.streams.is_empty() {
            body.push(ChatLineView::muted(live_step_status_label(
                &live_step.status,
            )));
        } else {
            for stream in live_step.streams.iter().rev().take(4).rev() {
                let marker = if stream.final_delta { "final" } else { "live" };
                body.push(ChatLineView::muted(format!(
                    "[{}:{marker}:#{}] {}",
                    stream.stream,
                    stream.sequence_end,
                    concise(&stream.content, MAX_SUMMARY_CHARS)
                )));
            }
        }
        let status = chat_status_for_live_step(&live_step.status);
        let severity = chat_severity_for_live_step(&live_step.status);
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::AgentProgress,
            status,
            severity,
            title: format!(
                "{} {}",
                live_step.agent,
                live_step_status_title_suffix(&live_step.status)
            ),
            summary: Some(format!(
                "run:{} step:{}",
                live_step.run_id, live_step.step_id
            )),
            body,
            details: Vec::new(),
            source: ChatSourceRef {
                event_ids: Vec::new(),
                run_id: Some(live_step.run_id.clone()),
                step_id: Some(live_step.step_id.clone()),
                action_id: None,
            },
            updated_at: String::new(),
            fallback_event_id: format!("live:{}", live_step.step_id),
        });
    }

    pub fn apply_pending_approval(&mut self, approval: Option<&PendingApprovalView>) {
        let Some(approval) = approval else {
            let previous_key = self.pending_key.take();
            self.remove_waiting_approval_key(previous_key);
            return;
        };
        let key = ChatLifecycleKey::Action {
            run_id: approval.run_id.clone(),
            step_id: approval.step_id.clone(),
            action_id: approval.action_id.clone(),
        };
        self.pending_key = Some(key.clone());
        let mut body = vec![ChatLineView::warning(&approval.summary)];
        if let Some(diagnostic) = approval.diagnostic.as_deref() {
            body.push(ChatLineView::warning(concise(
                diagnostic,
                MAX_SUMMARY_CHARS,
            )));
        }
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::Approval,
            status: ChatItemStatus::WaitingApproval,
            severity: ChatSeverity::Warning,
            title: format!("Approval required for {}", approval.agent),
            summary: Some(format!("action {}", approval.action_id)),
            body,
            details: Vec::new(),
            source: ChatSourceRef {
                event_ids: Vec::new(),
                run_id: Some(approval.run_id.clone()),
                step_id: Some(approval.step_id.clone()),
                action_id: Some(approval.action_id.clone()),
            },
            updated_at: String::new(),
            fallback_event_id: format!("pending:{}", approval.action_id),
        });
    }

    pub fn items(&self) -> &[ChatItemView] {
        &self.items
    }

    fn apply_run_started(&mut self, event: &HistoryEvent) {
        let Some(run_id) = event
            .run_id
            .clone()
            .or_else(|| string_field(&event.payload, "run_id"))
        else {
            return;
        };
        self.upsert(ItemInput {
            lifecycle_key: Some(ChatLifecycleKey::Run {
                run_id: run_id.clone(),
            }),
            kind: ChatItemKind::RunSummary,
            status: ChatItemStatus::Running,
            severity: ChatSeverity::Info,
            title: "Run started".to_string(),
            summary: None,
            body: Vec::new(),
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_user_prompt(&mut self, event: &HistoryEvent) {
        let Some(prompt) = string_field(&event.payload, "prompt")
            .or_else(|| string_field(&event.payload, "answer"))
        else {
            return;
        };
        let lifecycle_key = if event.kind == "prompt_submitted" {
            event
                .run_id
                .clone()
                .map(|run_id| ChatLifecycleKey::Prompt { run_id })
        } else {
            None
        };
        self.upsert(ItemInput {
            lifecycle_key,
            kind: ChatItemKind::UserPrompt,
            status: ChatItemStatus::Completed,
            severity: ChatSeverity::Info,
            title: if event.kind == "clarification_answered" {
                "User clarification".to_string()
            } else {
                "User prompt".to_string()
            },
            summary: Some(concise(&prompt, MAX_SUMMARY_CHARS)),
            body: vec![ChatLineView::plain(prompt)],
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_orchestrator_decision(&mut self, event: &HistoryEvent) {
        let status =
            string_field(&event.payload, "status").unwrap_or_else(|| "continue".to_string());
        let reason = string_field(&event.payload, "reason");
        let next_agent = string_field(&event.payload, "next_agent");
        let title = match status.as_str() {
            "complete" => "Orchestrator completed the run".to_string(),
            "failed" => "Orchestrator stopped the run".to_string(),
            "waiting_for_user" => "Orchestrator needs clarification".to_string(),
            _ => next_agent
                .as_deref()
                .map(|agent| format!("Route to {agent}"))
                .unwrap_or_else(|| "Routing decision".to_string()),
        };
        let mut body = Vec::new();
        if let Some(reason) = reason.as_deref() {
            body.extend(message_body_lines(reason, ChatLineStyle::Plain));
        }
        if let Some(plan) = event.payload.get("plan").and_then(Value::as_array) {
            for step in plan.iter().filter_map(Value::as_str).take(4) {
                body.push(ChatLineView::muted(format!("plan: {step}")));
            }
        }
        let kind = if matches!(status.as_str(), "complete" | "failed") {
            ChatItemKind::RunSummary
        } else {
            ChatItemKind::RoutingDecision
        };
        let severity = match status.as_str() {
            "complete" => ChatSeverity::Success,
            "failed" => ChatSeverity::Error,
            "waiting_for_user" => ChatSeverity::Warning,
            _ => ChatSeverity::Info,
        };
        let item_status = match status.as_str() {
            "complete" => ChatItemStatus::Completed,
            "failed" => ChatItemStatus::Failed,
            "waiting_for_user" => ChatItemStatus::WaitingApproval,
            _ => ChatItemStatus::Completed,
        };
        let lifecycle_key =
            event
                .run_id
                .clone()
                .zip(event.step_id.clone())
                .map(|(run_id, step_id)| ChatLifecycleKey::Step {
                    run_id,
                    step_id,
                    item_kind: kind.clone(),
                });
        self.upsert(ItemInput {
            lifecycle_key,
            kind,
            status: item_status,
            severity,
            title,
            summary: next_agent.map(|agent| format!("selected {agent}")),
            body,
            details: history_detail(event, "decision"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_agent_step_started(&mut self, event: &HistoryEvent) {
        let Some(agent) = string_field(&event.payload, "agent") else {
            return;
        };
        let Some(key) = step_key(event, ChatItemKind::AgentProgress) else {
            return;
        };
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::AgentProgress,
            status: ChatItemStatus::Running,
            severity: ChatSeverity::Info,
            title: format!("{agent} step started"),
            summary: None,
            body: vec![ChatLineView::muted("waiting for runtime output")],
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_runtime_stream_delta(&mut self, event: &HistoryEvent) {
        let Some(agent) = string_field(&event.payload, "agent") else {
            return;
        };
        let Some(key) = step_key(event, ChatItemKind::AgentProgress) else {
            return;
        };
        let stream =
            string_field(&event.payload, "stream").unwrap_or_else(|| "runtime".to_string());
        let content = string_field(&event.payload, "content")
            .unwrap_or_else(|| "large stream content stored as artifact".to_string());
        let final_delta = event
            .payload
            .get("final_delta")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let sequence = runtime_stream_sequence_label(&event.payload);
        let marker = if final_delta { "final" } else { "live" };
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::AgentProgress,
            status: if final_delta {
                ChatItemStatus::Completed
            } else {
                ChatItemStatus::Running
            },
            severity: if final_delta {
                ChatSeverity::Success
            } else {
                ChatSeverity::Info
            },
            title: if final_delta {
                format!("{agent} completed runtime output")
            } else {
                format!("{agent} is working")
            },
            summary: Some(format!(
                "{stream}:{marker}{sequence}: {}",
                message_summary(&content)
            )),
            body: runtime_stream_body_lines(&stream, marker, &sequence, &content),
            details: history_detail(event, "runtime stream"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_action_requested(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            self.apply_projection_warning(event, "Malformed action request");
            return;
        };
        let mut context = ActionContext {
            action_id: action_id.clone(),
            kind: string_field(&event.payload, "kind"),
            command: event
                .payload
                .get("params")
                .and_then(|params| string_field(params, "command")),
            diff: event
                .payload
                .get("params")
                .and_then(|params| string_field(params, "diff")),
            path: event
                .payload
                .get("params")
                .and_then(|params| string_field(params, "path")),
            content_bytes: event
                .payload
                .get("params")
                .and_then(|params| string_field(params, "content"))
                .map(|content| content.len() as u64),
        };
        if context.kind.as_deref() == Some("write_file") && context.content_bytes.is_none() {
            context.content_bytes = event
                .payload
                .get("params")
                .and_then(|params| params.get("bytes"))
                .and_then(Value::as_u64);
        }
        self.action_context
            .insert(action_id.clone(), context.clone());
        let Some(key) = action_key(event, &action_id) else {
            self.apply_projection_warning(event, "Action request is missing run or step id");
            return;
        };
        let (kind, title, summary, body) = action_requested_view(&context);
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind,
            status: ChatItemStatus::Pending,
            severity: ChatSeverity::Info,
            title,
            summary,
            body,
            details: history_detail(event, "request"),
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_command_started(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let command = string_field(&event.payload, "command").unwrap_or_default();
        self.action_context
            .entry(action_id.clone())
            .or_insert_with(|| ActionContext {
                action_id: action_id.clone(),
                ..ActionContext::default()
            })
            .command = Some(command.clone());
        let Some(key) = action_key(event, &action_id) else {
            return;
        };
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::CommandResult,
            status: ChatItemStatus::Running,
            severity: ChatSeverity::Info,
            title: format!("Command running: {}", concise(&command, 120)),
            summary: Some("running".to_string()),
            body: vec![ChatLineView::code(format!("$ {command}"))],
            details: history_detail(event, "command"),
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_command_completed(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let command = string_field(&event.payload, "command")
            .or_else(|| self.command_for_action(&action_id))
            .unwrap_or_default();
        let summary = summarize_command(CommandSummaryInput {
            command: command.clone(),
            exit_code: event.payload.get("exit_code").and_then(Value::as_i64),
            stdout: None,
            stderr: None,
            diagnostic: string_field(&event.payload, "diagnostic"),
        });
        let Some(key) = action_key(event, &action_id) else {
            return;
        };
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::CommandResult,
            status: summary.status,
            severity: summary.severity,
            title: summary.title,
            summary: Some(exit_summary(
                event.payload.get("exit_code").and_then(Value::as_i64),
            )),
            body: summary.body,
            details: merge_details(history_detail(event, "command"), summary.details),
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_file_edit_applied(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let context = self
            .action_context
            .get(&action_id)
            .cloned()
            .unwrap_or_default();
        let (title, summary, mut body) = file_edit_view(&event.payload, &context);
        let details = history_detail(event, "file edit");
        let Some(key) = action_key(event, &action_id) else {
            return;
        };
        body.truncate(MAX_BODY_LINES);
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::FileEdit,
            status: ChatItemStatus::Completed,
            severity: ChatSeverity::Success,
            title,
            summary,
            body,
            details,
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_approval_requested(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let command = self.command_for_action(&action_id);
        let Some(key) = action_key(event, &action_id) else {
            return;
        };
        let diagnostic = string_field(&event.payload, "diagnostic");
        let mut body = Vec::new();
        if let Some(command) = command.as_deref() {
            body.push(ChatLineView::code(format!("$ {command}")));
        }
        if let Some(diagnostic) = diagnostic.as_deref() {
            body.push(ChatLineView::warning(concise(
                diagnostic,
                MAX_SUMMARY_CHARS,
            )));
        }
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::Approval,
            status: ChatItemStatus::WaitingApproval,
            severity: ChatSeverity::Warning,
            title: "Action approval required".to_string(),
            summary: command.map(|command| concise(&command, MAX_SUMMARY_CHARS)),
            body,
            details: history_detail(event, "approval"),
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_approval_resolved(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let Some(key) = action_key(event, &action_id) else {
            return;
        };
        let approved = event
            .payload
            .get("approved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let line = if approved {
            ChatLineView::plain("approval granted")
        } else {
            ChatLineView::warning("approval denied")
        };
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::Approval,
            status: if approved {
                ChatItemStatus::Running
            } else {
                ChatItemStatus::Denied
            },
            severity: if approved {
                ChatSeverity::Info
            } else {
                ChatSeverity::Warning
            },
            title: if approved {
                "Action approval granted".to_string()
            } else {
                "Action approval denied".to_string()
            },
            summary: None,
            body: vec![line],
            details: history_detail(event, "approval"),
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_action_denied(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let Some(key) = action_key(event, &action_id) else {
            return;
        };
        let diagnostic = string_field(&event.payload, "diagnostic");
        let mut body = Vec::new();
        if let Some(command) = self.command_for_action(&action_id) {
            body.push(ChatLineView::code(format!("$ {command}")));
        }
        if let Some(diagnostic) = diagnostic.as_deref() {
            body.push(ChatLineView::warning(concise(
                diagnostic,
                MAX_SUMMARY_CHARS,
            )));
        }
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::Approval,
            status: ChatItemStatus::Denied,
            severity: ChatSeverity::Warning,
            title: "Action denied".to_string(),
            summary: string_field(&event.payload, "summary"),
            body,
            details: history_detail(event, "denial"),
            source: source_from_event(event, Some(action_id)),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_action_completed(&mut self, event: &HistoryEvent) {
        let Some(action_id) = string_field(&event.payload, "action_id") else {
            return;
        };
        let status =
            string_field(&event.payload, "status").unwrap_or_else(|| "completed".to_string());
        if matches!(status.as_str(), "approval_required") {
            return;
        }
        let context = self
            .action_context
            .get(&action_id)
            .cloned()
            .unwrap_or_default();
        match context.kind.as_deref() {
            Some("run_command") => self.apply_action_completed_command(event, &action_id, &status),
            Some("apply_patch" | "write_file") => {
                self.apply_action_completed_file_edit(event, &action_id, &status, &context)
            }
            _ => self.apply_action_completed_generic(event, &action_id, &status, &context),
        }
    }

    fn apply_action_completed_command(
        &mut self,
        event: &HistoryEvent,
        action_id: &str,
        status: &str,
    ) {
        let content = event.payload.get("content");
        let command = content
            .and_then(|content| string_field(content, "command"))
            .or_else(|| self.command_for_action(action_id))
            .unwrap_or_else(|| "<unknown command>".to_string());
        let summary = summarize_command(CommandSummaryInput {
            command,
            exit_code: content
                .and_then(|content| content.get("exit_code"))
                .and_then(Value::as_i64),
            stdout: content.and_then(|content| string_field(content, "stdout")),
            stderr: content.and_then(|content| string_field(content, "stderr")),
            diagnostic: string_field(&event.payload, "diagnostic"),
        });
        let Some(key) = action_key(event, action_id) else {
            return;
        };
        let item_status = status_to_item_status(status, Some(summary.status));
        let severity = severity_for_action_status(status, Some(summary.severity));
        let details = merge_details(history_detail(event, "result"), summary.details);
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::CommandResult,
            status: item_status,
            severity,
            title: summary.title,
            summary: Some(status.replace('_', " ")),
            body: summary.body,
            details,
            source: source_from_event(event, Some(action_id.to_string())),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_action_completed_file_edit(
        &mut self,
        event: &HistoryEvent,
        action_id: &str,
        status: &str,
        context: &ActionContext,
    ) {
        let Some(key) = action_key(event, action_id) else {
            return;
        };
        let (title, summary, body) = action_file_edit_result_view(event, context, status);
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::FileEdit,
            status: status_to_item_status(status, None),
            severity: severity_for_action_status(status, None),
            title,
            summary,
            body,
            details: history_detail(event, "result"),
            source: source_from_event(event, Some(action_id.to_string())),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_action_completed_generic(
        &mut self,
        event: &HistoryEvent,
        action_id: &str,
        status: &str,
        context: &ActionContext,
    ) {
        let Some(key) = action_key(event, action_id) else {
            return;
        };
        let target = context
            .kind
            .as_deref()
            .map(action_kind_label)
            .unwrap_or("Action");
        let diagnostic = string_field(&event.payload, "diagnostic");
        let mut body = Vec::new();
        if matches!(context.kind.as_deref(), Some("search_text")) {
            body.extend(search_text_result_lines(&event.payload));
        }
        if let Some(diagnostic) = diagnostic.as_deref() {
            let line = if status == "failed" {
                ChatLineView::error(concise(diagnostic, MAX_SUMMARY_CHARS))
            } else {
                ChatLineView::warning(concise(diagnostic, MAX_SUMMARY_CHARS))
            };
            body.push(line);
        }
        self.upsert(ItemInput {
            lifecycle_key: Some(key),
            kind: ChatItemKind::ActionRequested,
            status: status_to_item_status(status, None),
            severity: severity_for_action_status(status, None),
            title: format!("{target} {}", status.replace('_', " ")),
            summary: string_field(&event.payload, "summary"),
            body,
            details: history_detail(event, "result"),
            source: source_from_event(event, Some(action_id.to_string())),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_artifact_written(&mut self, event: &HistoryEvent) {
        let details = artifact_detail(&event.payload)
            .map(|detail| vec![detail])
            .unwrap_or_else(|| history_detail(event, "artifact"));
        self.upsert(ItemInput {
            lifecycle_key: None,
            kind: ChatItemKind::Diagnostic,
            status: ChatItemStatus::Completed,
            severity: ChatSeverity::Info,
            title: "Artifact written".to_string(),
            summary: artifact_summary(&event.payload),
            body: Vec::new(),
            details,
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_agent_result(&mut self, event: &HistoryEvent) {
        let agent = string_field(&event.payload, "agent")
            .or_else(|| string_field(&event.payload, "member_id"))
            .unwrap_or_else(|| "agent".to_string());
        let status =
            string_field(&event.payload, "status").unwrap_or_else(|| "completed".to_string());
        let summary = string_field(&event.payload, "summary");
        let blocker = string_field(&event.payload, "blocker")
            .or_else(|| string_field(&event.payload, "diagnostic"));
        let mut body = Vec::new();
        if let Some(summary) = summary.as_deref() {
            body.extend(message_body_lines(summary, ChatLineStyle::Plain));
        }
        if let Some(blocker) = blocker.as_deref() {
            body.extend(message_body_lines(blocker, ChatLineStyle::Warning));
        }
        append_string_array(&mut body, &event.payload, "findings", "finding");
        append_string_array(&mut body, &event.payload, "changed_files", "changed");
        append_string_array(&mut body, &event.payload, "verification", "verified");
        let severity = match status.as_str() {
            "completed" | "no_changes" => ChatSeverity::Success,
            "blocked" | "approval_denied" | "parse_error" | "limit_reached" => {
                ChatSeverity::Warning
            }
            "failed" | "cancelled" => ChatSeverity::Error,
            _ => ChatSeverity::Info,
        };
        self.upsert(ItemInput {
            lifecycle_key: step_key(event, ChatItemKind::AgentResult),
            kind: ChatItemKind::AgentResult,
            status: match severity {
                ChatSeverity::Success => ChatItemStatus::Completed,
                ChatSeverity::Error => ChatItemStatus::Failed,
                ChatSeverity::Warning => ChatItemStatus::Completed,
                ChatSeverity::Info => ChatItemStatus::Completed,
            },
            severity,
            title: format!("{agent}: {}", status.replace('_', " ")),
            summary: summary.map(|summary| message_summary(&summary)),
            body,
            details: merge_details(
                history_detail(event, "result"),
                artifact_details_from_result(&event.payload),
            ),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_run_summary(&mut self, event: &HistoryEvent) {
        let Some(run_id) = event.run_id.clone() else {
            return;
        };
        let (title, status, severity) = match event.kind.as_str() {
            "run_completed" | "subtask_completed" => (
                "Run completed",
                ChatItemStatus::Completed,
                ChatSeverity::Success,
            ),
            "run_failed" => ("Run failed", ChatItemStatus::Failed, ChatSeverity::Error),
            "run_limit_reached" => (
                "Run limit reached",
                ChatItemStatus::Failed,
                ChatSeverity::Warning,
            ),
            "run_interrupted" => (
                "Run interrupted",
                ChatItemStatus::Interrupted,
                ChatSeverity::Warning,
            ),
            _ => ("Run summary", ChatItemStatus::Completed, ChatSeverity::Info),
        };
        let summary = string_field(&event.payload, "summary")
            .or_else(|| string_field(&event.payload, "reason"))
            .or_else(|| string_field(&event.payload, "limit"));
        let mut body = Vec::new();
        if let Some(summary) = summary.as_deref() {
            body.extend(message_body_lines(
                summary,
                if severity == ChatSeverity::Error {
                    ChatLineStyle::Error
                } else {
                    ChatLineStyle::Plain
                },
            ));
        }
        self.upsert(ItemInput {
            lifecycle_key: Some(ChatLifecycleKey::Run { run_id }),
            kind: ChatItemKind::RunSummary,
            status,
            severity,
            title: title.to_string(),
            summary: summary.map(|summary| message_summary(&summary)),
            body,
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_diagnostic(&mut self, event: &HistoryEvent) {
        if event.kind == "diagnostic"
            && string_field(&event.payload, "reason").as_deref() == Some("parse_error")
        {
            return;
        }
        let message = string_field(&event.payload, "message")
            .or_else(|| string_field(&event.payload, "reason"))
            .or_else(|| string_field(&event.payload, "goal"))
            .or_else(|| string_field(&event.payload, "previous_goal"))
            .unwrap_or_else(|| readable_kind(&event.kind));
        let severity = if event.kind.contains("invalid") || event.kind == "run_failed" {
            ChatSeverity::Error
        } else {
            ChatSeverity::Warning
        };
        let is_error = severity == ChatSeverity::Error;
        self.upsert(ItemInput {
            lifecycle_key: None,
            kind: ChatItemKind::Diagnostic,
            status: if is_error {
                ChatItemStatus::Failed
            } else {
                ChatItemStatus::Completed
            },
            severity,
            title: readable_kind(&event.kind),
            summary: Some(message_summary(&message)),
            body: message_body_lines(
                &message,
                if is_error {
                    ChatLineStyle::Error
                } else {
                    ChatLineStyle::Warning
                },
            ),
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_blocker(&mut self, event: &HistoryEvent) {
        let question = string_field(&event.payload, "question")
            .unwrap_or_else(|| "Orchestrator needs clarification.".to_string());
        self.upsert(ItemInput {
            lifecycle_key: event
                .run_id
                .clone()
                .map(|run_id| ChatLifecycleKey::Run { run_id }),
            kind: ChatItemKind::RunSummary,
            status: ChatItemStatus::WaitingApproval,
            severity: ChatSeverity::Warning,
            title: "Clarification needed".to_string(),
            summary: Some(concise(&question, MAX_SUMMARY_CHARS)),
            body: vec![ChatLineView::warning(question)],
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn apply_projection_warning(&mut self, event: &HistoryEvent, title: &str) {
        self.upsert(ItemInput {
            lifecycle_key: None,
            kind: ChatItemKind::Diagnostic,
            status: ChatItemStatus::Completed,
            severity: ChatSeverity::Warning,
            title: title.to_string(),
            summary: Some(readable_kind(&event.kind)),
            body: Vec::new(),
            details: history_detail(event, "history"),
            source: source_from_event(event, None),
            updated_at: event.timestamp.clone(),
            fallback_event_id: event.event_id.clone(),
        });
    }

    fn command_for_action(&self, action_id: &str) -> Option<String> {
        self.action_context
            .get(action_id)
            .and_then(|context| context.command.clone())
    }

    fn upsert(&mut self, input: ItemInput) {
        let lifecycle_key = input.lifecycle_key.clone();
        let id = lifecycle_key
            .as_ref()
            .map(ChatLifecycleKey::item_id)
            .unwrap_or_else(|| format!("chat:event:{}", input.fallback_event_id));
        let item = ChatItemView {
            id,
            lifecycle_key: lifecycle_key.clone(),
            kind: input.kind,
            status: input.status,
            severity: input.severity,
            title: concise(&input.title, MAX_TITLE_CHARS),
            summary: input
                .summary
                .map(|summary| concise(&summary, MAX_SUMMARY_CHARS)),
            body: bounded_body(input.body),
            details: input.details,
            source: input.source,
            updated_at: input.updated_at,
        };

        if let Some(key) = lifecycle_key {
            if let Some(index) = self.index.get(&key).copied() {
                let existing = &mut self.items[index];
                let mut source = item.source;
                for event_id in &existing.source.event_ids {
                    source.merge_event(event_id);
                }
                *existing = ChatItemView { source, ..item };
                return;
            }
            self.index.insert(key, self.items.len());
        }
        self.items.push(item);
    }

    fn remove_transient_key(&mut self, key: Option<ChatLifecycleKey>) {
        let Some(key) = key else {
            return;
        };
        let Some(index) = self.index.remove(&key) else {
            return;
        };
        self.items.remove(index);
        self.rebuild_index();
    }

    fn remove_waiting_approval_key(&mut self, key: Option<ChatLifecycleKey>) {
        let Some(key) = key else {
            return;
        };
        let Some(index) = self.index.get(&key).copied() else {
            return;
        };
        if self.items[index].status != ChatItemStatus::WaitingApproval {
            return;
        }
        self.index.remove(&key);
        self.items.remove(index);
        self.rebuild_index();
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (index, item) in self.items.iter().enumerate() {
            if let Some(key) = &item.lifecycle_key {
                self.index.insert(key.clone(), index);
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ItemInput {
    lifecycle_key: Option<ChatLifecycleKey>,
    kind: ChatItemKind,
    status: ChatItemStatus,
    severity: ChatSeverity,
    title: String,
    summary: Option<String>,
    body: Vec<ChatLineView>,
    details: Vec<ChatDetailRef>,
    source: ChatSourceRef,
    updated_at: String,
    fallback_event_id: String,
}

fn action_requested_view(
    context: &ActionContext,
) -> (ChatItemKind, String, Option<String>, Vec<ChatLineView>) {
    match context.kind.as_deref() {
        Some("run_command") => {
            let command = context
                .command
                .clone()
                .unwrap_or_else(|| "<missing command>".to_string());
            (
                ChatItemKind::CommandResult,
                format!("Command requested: {}", concise(&command, 120)),
                Some("pending".to_string()),
                vec![ChatLineView::code(format!("$ {command}"))],
            )
        }
        Some("apply_patch") => {
            let Some(diff) = context.diff.as_deref() else {
                return (
                    ChatItemKind::FileEdit,
                    "Patch requested".to_string(),
                    Some("missing diff preview".to_string()),
                    Vec::new(),
                );
            };
            let preview = build_diff_preview(diff);
            let title = if preview.files.is_empty() {
                "Patch requested".to_string()
            } else {
                format!("Patch requested: {}", preview.files.join(", "))
            };
            let summary = Some(format!("+{} -{}", preview.added, preview.removed));
            (
                ChatItemKind::FileEdit,
                title,
                summary,
                preview.preview_lines,
            )
        }
        Some("write_file") => {
            let path = context.path.as_deref().unwrap_or("<missing path>");
            let summary = context
                .content_bytes
                .map(|bytes| format!("{bytes} bytes"))
                .unwrap_or_else(|| "new file".to_string());
            (
                ChatItemKind::FileEdit,
                format!("File write requested: {path}"),
                Some(summary),
                Vec::new(),
            )
        }
        Some(kind) => (
            ChatItemKind::ActionRequested,
            format!("Action requested: {}", action_kind_label(kind)),
            Some(context.action_id.clone()),
            Vec::new(),
        ),
        None => (
            ChatItemKind::ActionRequested,
            "Action requested".to_string(),
            Some(context.action_id.clone()),
            Vec::new(),
        ),
    }
}

fn file_edit_view(
    payload: &Value,
    context: &ActionContext,
) -> (String, Option<String>, Vec<ChatLineView>) {
    match string_field(payload, "operation").as_deref() {
        Some("write_file") => {
            let path = string_field(payload, "path")
                .or_else(|| context.path.clone())
                .unwrap_or_else(|| "<unknown file>".to_string());
            let bytes = payload
                .get("bytes")
                .and_then(Value::as_u64)
                .or(context.content_bytes);
            let summary = bytes
                .map(|bytes| format!("{bytes} bytes"))
                .unwrap_or_else(|| "created".to_string());
            (format!("File created: {path}"), Some(summary), Vec::new())
        }
        Some("apply_patch") => {
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
            let preview = context
                .diff
                .as_deref()
                .map(build_diff_preview)
                .unwrap_or_else(|| build_diff_preview(""));
            let title = if files.is_empty() {
                "Files edited".to_string()
            } else {
                format!("Files edited: {}", summarize_items(&files, 4))
            };
            (
                title,
                Some(format!("+{} -{}", preview.added, preview.removed)),
                preview.preview_lines,
            )
        }
        _ => ("File edit applied".to_string(), None, Vec::new()),
    }
}

fn action_file_edit_result_view(
    event: &HistoryEvent,
    context: &ActionContext,
    status: &str,
) -> (String, Option<String>, Vec<ChatLineView>) {
    if status != "completed" {
        return action_file_edit_not_applied_view(event, context, status);
    }

    match context.kind.as_deref() {
        Some("write_file") => {
            let content = event.payload.get("content");
            let path = content
                .and_then(|content| string_field(content, "path"))
                .or_else(|| context.path.clone())
                .unwrap_or_else(|| "<unknown file>".to_string());
            let bytes = content
                .and_then(|content| content.get("bytes"))
                .and_then(Value::as_u64)
                .or(context.content_bytes);
            (
                format!("File created: {path}"),
                bytes.map(|bytes| format!("{bytes} bytes")),
                Vec::new(),
            )
        }
        Some("apply_patch") => {
            let preview = context
                .diff
                .as_deref()
                .map(build_diff_preview)
                .unwrap_or_else(|| build_diff_preview(""));
            let title = if preview.files.is_empty() {
                "Files edited".to_string()
            } else {
                format!("Files edited: {}", summarize_items(&preview.files, 4))
            };
            (
                title,
                Some(format!("+{} -{}", preview.added, preview.removed)),
                preview.preview_lines,
            )
        }
        _ => ("File edit completed".to_string(), None, Vec::new()),
    }
}

fn action_file_edit_not_applied_view(
    event: &HistoryEvent,
    context: &ActionContext,
    status: &str,
) -> (String, Option<String>, Vec<ChatLineView>) {
    match context.kind.as_deref() {
        Some("write_file") => {
            let content = event.payload.get("content");
            let path = content
                .and_then(|content| string_field(content, "path"))
                .or_else(|| context.path.clone())
                .unwrap_or_else(|| "<unknown file>".to_string());
            let bytes = content
                .and_then(|content| content.get("bytes"))
                .and_then(Value::as_u64)
                .or(context.content_bytes);
            let title = match status {
                "denied" => format!("File write denied: {path}"),
                "failed" => format!("File write failed: {path}"),
                other => format!("File write {}: {path}", other.replace('_', " ")),
            };
            let summary = Some(
                bytes
                    .map(|bytes| format!("not written ({bytes} bytes requested)"))
                    .unwrap_or_else(|| "not written".to_string()),
            );
            let mut body = not_applied_body(event, status, "requested file write was not applied");
            if let Some(bytes) = bytes {
                body.push(ChatLineView::muted(format!(
                    "requested content: {bytes} bytes"
                )));
            }
            (title, summary, body)
        }
        Some("apply_patch") => {
            let preview = context
                .diff
                .as_deref()
                .map(build_diff_preview)
                .unwrap_or_else(|| build_diff_preview(""));
            let target = file_list_suffix(&preview.files);
            let title = match status {
                "denied" => format!("Patch denied{target}"),
                "failed" => format!("Patch failed{target}"),
                other => format!("Patch {}{target}", other.replace('_', " ")),
            };
            let summary = if preview.added == 0 && preview.removed == 0 {
                Some("not applied".to_string())
            } else {
                Some(format!(
                    "not applied (+{} -{} requested)",
                    preview.added, preview.removed
                ))
            };
            let mut body = not_applied_body(event, status, "requested patch was not applied");
            if !preview.preview_lines.is_empty() {
                body.push(ChatLineView::muted("requested diff preview"));
                body.extend(preview.preview_lines);
            }
            (title, summary, body)
        }
        _ => {
            let title = match status {
                "denied" => "File edit denied".to_string(),
                "failed" => "File edit failed".to_string(),
                other => format!("File edit {}", other.replace('_', " ")),
            };
            (
                title,
                Some("not applied".to_string()),
                not_applied_body(event, status, "requested file edit was not applied"),
            )
        }
    }
}

fn not_applied_body(event: &HistoryEvent, status: &str, notice: &str) -> Vec<ChatLineView> {
    let mut body = Vec::new();
    if let Some(diagnostic) = string_field(&event.payload, "diagnostic") {
        let text = concise(&diagnostic, MAX_SUMMARY_CHARS);
        if status == "failed" {
            body.push(ChatLineView::error(text));
        } else {
            body.push(ChatLineView::warning(text));
        }
    }
    body.push(ChatLineView::muted(notice));
    body
}

fn file_list_suffix(files: &[String]) -> String {
    if files.is_empty() {
        String::new()
    } else {
        format!(": {}", summarize_items(files, 4))
    }
}

fn action_key(event: &HistoryEvent, action_id: &str) -> Option<ChatLifecycleKey> {
    Some(ChatLifecycleKey::Action {
        run_id: event.run_id.clone()?,
        step_id: event.step_id.clone()?,
        action_id: action_id.to_string(),
    })
}

fn step_key(event: &HistoryEvent, item_kind: ChatItemKind) -> Option<ChatLifecycleKey> {
    Some(ChatLifecycleKey::Step {
        run_id: event.run_id.clone()?,
        step_id: event.step_id.clone()?,
        item_kind,
    })
}

fn source_from_event(event: &HistoryEvent, action_id: Option<String>) -> ChatSourceRef {
    ChatSourceRef::from_event(
        event.event_id.clone(),
        event.run_id.clone(),
        event.step_id.clone(),
        action_id,
    )
}

fn history_detail(event: &HistoryEvent, label: &str) -> Vec<ChatDetailRef> {
    vec![ChatDetailRef::HistoryEvent {
        event_id: event.event_id.clone(),
        label: label.to_string(),
    }]
}

fn artifact_detail(payload: &Value) -> Option<ChatDetailRef> {
    Some(ChatDetailRef::Artifact {
        label: "artifact".to_string(),
        artifact_id: string_field(payload, "artifact_id").or_else(|| {
            payload
                .get("artifact")
                .and_then(|value| string_field(value, "artifact_id"))
        }),
        path: string_field(payload, "path").or_else(|| {
            payload
                .get("artifact")
                .and_then(|value| string_field(value, "path"))
        }),
        media_type: string_field(payload, "media_type").or_else(|| {
            payload
                .get("artifact")
                .and_then(|value| string_field(value, "media_type"))
        }),
    })
}

fn artifact_details_from_result(payload: &Value) -> Vec<ChatDetailRef> {
    payload
        .get("artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(artifact_detail)
        .collect()
}

fn artifact_summary(payload: &Value) -> Option<String> {
    string_field(payload, "path").or_else(|| {
        payload
            .get("artifact")
            .and_then(|value| string_field(value, "path"))
    })
}

fn merge_details(mut left: Vec<ChatDetailRef>, right: Vec<ChatDetailRef>) -> Vec<ChatDetailRef> {
    left.extend(right);
    left
}

fn bounded_body(body: Vec<ChatLineView>) -> Vec<ChatLineView> {
    body.into_iter()
        .take(MAX_BODY_LINES)
        .map(|mut line| {
            line.text = concise(&line.text, MAX_SUMMARY_CHARS);
            line
        })
        .collect()
}

fn append_string_array(body: &mut Vec<ChatLineView>, payload: &Value, field: &str, label: &str) {
    let Some(values) = payload.get(field).and_then(Value::as_array) else {
        return;
    };
    for value in values.iter().filter_map(Value::as_str).take(3) {
        body.push(ChatLineView::muted(format!("{label}: {value}")));
    }
}

fn message_summary(text: &str) -> String {
    json_value_from_text(text)
        .and_then(|value| human_json_summary(&value))
        .unwrap_or_else(|| concise(text, MAX_SUMMARY_CHARS))
}

fn message_body_lines(text: &str, fallback_style: ChatLineStyle) -> Vec<ChatLineView> {
    json_value_from_text(text)
        .map(|value| human_json_body_lines(&value))
        .filter(|lines| !lines.is_empty())
        .unwrap_or_else(|| {
            vec![styled_line(
                fallback_style,
                concise(text, MAX_SUMMARY_CHARS),
            )]
        })
}

fn runtime_stream_body_lines(
    stream: &str,
    marker: &str,
    sequence: &str,
    content: &str,
) -> Vec<ChatLineView> {
    let label = format!("[{stream}:{marker}{sequence}] ");
    message_body_lines(content, ChatLineStyle::Muted)
        .into_iter()
        .map(|mut line| {
            line.text = format!("{label}{}", line.text);
            line
        })
        .collect()
}

fn runtime_stream_sequence_label(payload: &Value) -> String {
    if let (Some(start), Some(end)) = (
        payload.get("sequence_start").and_then(Value::as_u64),
        payload.get("sequence_end").and_then(Value::as_u64),
    ) {
        if start == end {
            return format!(":#{start}");
        }
        return format!(":#{start}-{end}");
    }
    payload
        .get("sequence")
        .and_then(Value::as_u64)
        .map(|sequence| format!(":#{sequence}"))
        .unwrap_or_default()
}

fn json_value_from_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    if let Some(fenced) = fenced_payload(trimmed) {
        if let Ok(value) = serde_json::from_str::<Value>(fenced) {
            return Some(value);
        }
    }
    if let Some(slice) = object_or_array_slice(trimmed) {
        serde_json::from_str::<Value>(slice).ok()
    } else {
        None
    }
}

fn fenced_payload(text: &str) -> Option<&str> {
    if !text.starts_with("```") {
        return None;
    }
    let start = text.find('\n')?.saturating_add(1);
    let end = text.rfind("```")?;
    (end > start).then(|| text[start..end].trim())
}

fn object_or_array_slice(text: &str) -> Option<&str> {
    let object = text
        .find('{')
        .zip(text.rfind('}'))
        .filter(|(start, end)| start < end)
        .map(|(start, end)| &text[start..=end]);
    let array = text
        .find('[')
        .zip(text.rfind(']'))
        .filter(|(start, end)| start < end)
        .map(|(start, end)| &text[start..=end]);
    match (object, array) {
        (Some(object), Some(array)) => {
            if object.len() >= array.len() {
                Some(object)
            } else {
                Some(array)
            }
        }
        (Some(object), None) => Some(object),
        (None, Some(array)) => Some(array),
        (None, None) => None,
    }
}

fn human_json_summary(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    for key in [
        "summary",
        "final_summary",
        "reason",
        "message",
        "blocker",
        "diagnostic",
    ] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            return Some(concise(value, MAX_SUMMARY_CHARS));
        }
    }
    if let Some(next_agent) = object.get("next_agent").and_then(Value::as_str) {
        return Some(format!("Route to {next_agent}"));
    }
    if let Some(status) = object.get("status").and_then(Value::as_str) {
        if let Some(agent) = object.get("agent").and_then(Value::as_str) {
            return Some(format!("{agent}: {}", status.replace('_', " ")));
        }
        return Some(status.replace('_', " "));
    }
    Some(format!("{} fields", object.len()))
}

fn human_json_body_lines(value: &Value) -> Vec<ChatLineView> {
    match value {
        Value::Object(object) => human_json_object_lines(object),
        Value::Array(values) => values
            .iter()
            .take(6)
            .enumerate()
            .map(|(index, value)| {
                ChatLineView::plain(format!("item {}: {}", index + 1, human_value_text(value)))
            })
            .collect(),
        value => vec![ChatLineView::plain(human_value_text(value))],
    }
}

fn human_json_object_lines(object: &serde_json::Map<String, Value>) -> Vec<ChatLineView> {
    let mut lines = Vec::new();
    for key in [
        "agent",
        "status",
        "next_agent",
        "summary",
        "reason",
        "message",
        "final_summary",
        "blocker",
        "diagnostic",
        "clarifying_question",
        "stop_condition",
        "recommended_action",
        "confidence",
    ] {
        if let Some(value) = object.get(key).filter(|value| !value.is_null()) {
            lines.push(json_field_line(key, value));
        }
    }

    for key in [
        "plan",
        "findings",
        "changed_files",
        "commands",
        "verification",
        "risks",
        "dissent",
    ] {
        if let Some(values) = object.get(key).and_then(Value::as_array) {
            append_human_array_lines(&mut lines, key, values);
        }
    }

    for (key, value) in object {
        if known_or_internal_json_key(key) || value.is_null() {
            continue;
        }
        lines.push(json_field_line(key, value));
    }

    lines.truncate(MAX_BODY_LINES);
    lines
}

fn append_human_array_lines(lines: &mut Vec<ChatLineView>, key: &str, values: &[Value]) {
    if values.is_empty() {
        return;
    }
    let label = singular_json_label(key);
    for value in values.iter().take(4) {
        lines.push(ChatLineView::muted(format!(
            "{label}: {}",
            human_value_text(value)
        )));
    }
    if values.len() > 4 {
        lines.push(ChatLineView::muted(format!(
            "{}: +{} more",
            human_json_label(key),
            values.len() - 4
        )));
    }
}

fn json_field_line(key: &str, value: &Value) -> ChatLineView {
    let text = format!("{}: {}", human_json_label(key), human_value_text(value));
    let style = match key {
        "blocker" | "diagnostic" => ChatLineStyle::Warning,
        _ => ChatLineStyle::Plain,
    };
    styled_line(style, text)
}

fn human_value_text(value: &Value) -> String {
    match value {
        Value::String(value) => concise(value, MAX_SUMMARY_CHARS),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => "none".to_string(),
        Value::Array(values) => format!("{} item(s)", values.len()),
        Value::Object(object) => object
            .get("summary")
            .and_then(Value::as_str)
            .map(|summary| concise(summary, MAX_SUMMARY_CHARS))
            .unwrap_or_else(|| format!("{} field(s)", object.len())),
    }
}

fn known_or_internal_json_key(key: &str) -> bool {
    matches!(
        key,
        "schema_version"
            | "decision_id"
            | "run_id"
            | "step_id"
            | "artifact"
            | "artifacts"
            | "agent"
            | "status"
            | "next_agent"
            | "summary"
            | "reason"
            | "message"
            | "final_summary"
            | "blocker"
            | "diagnostic"
            | "clarifying_question"
            | "stop_condition"
            | "recommended_action"
            | "confidence"
            | "plan"
            | "findings"
            | "changed_files"
            | "commands"
            | "verification"
            | "risks"
            | "dissent"
            | "required_capabilities"
    )
}

fn human_json_label(key: &str) -> String {
    key.split('_').collect::<Vec<_>>().join(" ")
}

fn singular_json_label(key: &str) -> String {
    match key {
        "changed_files" => "changed".to_string(),
        "commands" => "command".to_string(),
        "findings" => "finding".to_string(),
        "risks" => "risk".to_string(),
        "verification" => "verified".to_string(),
        "dissent" => "dissent".to_string(),
        "plan" => "plan".to_string(),
        _ => human_json_label(key),
    }
}

fn styled_line(style: ChatLineStyle, text: impl Into<String>) -> ChatLineView {
    ChatLineView {
        style,
        text: text.into(),
    }
}

fn status_to_item_status(status: &str, fallback: Option<ChatItemStatus>) -> ChatItemStatus {
    match status {
        "completed" => ChatItemStatus::Completed,
        "denied" => ChatItemStatus::Denied,
        "failed" => ChatItemStatus::Failed,
        "approval_required" => ChatItemStatus::WaitingApproval,
        _ => fallback.unwrap_or(ChatItemStatus::Completed),
    }
}

fn severity_for_action_status(status: &str, fallback: Option<ChatSeverity>) -> ChatSeverity {
    match status {
        "completed" => ChatSeverity::Success,
        "denied" | "approval_required" => ChatSeverity::Warning,
        "failed" => ChatSeverity::Error,
        _ => fallback.unwrap_or(ChatSeverity::Info),
    }
}

fn exit_summary(exit_code: Option<i64>) -> String {
    exit_code
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| "exit unavailable".to_string())
}

fn action_kind_label(kind: &str) -> &'static str {
    match kind {
        "read_file" => "Read file",
        "list_files" => "List files",
        "search_text" => "Search text",
        "run_command" => "Run command",
        "apply_patch" => "Apply patch",
        "write_file" => "Write file",
        "record_note" => "Record note",
        _ => "Action",
    }
}

fn search_text_result_lines(payload: &Value) -> Vec<ChatLineView> {
    let Some(matches) = payload
        .get("content")
        .and_then(|content| content.get("matches"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let mut seen = Vec::new();
    for entry in matches {
        let Some(path) = string_field(entry, "path") else {
            continue;
        };
        let location = entry
            .get("line")
            .and_then(Value::as_u64)
            .map(|line| format!("{path}:{line}"))
            .unwrap_or(path);
        if seen.contains(&location) {
            continue;
        }
        seen.push(location.clone());
        lines.push(ChatLineView::plain(format!("match: {location}")));
        if lines.len() >= 6 {
            break;
        }
    }
    if let Some(total) = payload
        .get("content")
        .and_then(|content| content.get("total_matches"))
        .and_then(Value::as_u64)
        .and_then(|total| usize::try_from(total).ok())
        .filter(|total| *total > seen.len())
    {
        lines.push(ChatLineView::muted(format!(
            "showing {} of {total} matches",
            seen.len()
        )));
    }
    lines
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

fn readable_kind(kind: &str) -> String {
    let mut words = kind.split('_');
    let Some(first) = words.next() else {
        return String::new();
    };
    let mut chars = first.chars();
    let first = match chars.next() {
        Some(head) => format!("{}{}", head.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    };
    std::iter::once(first)
        .chain(words.map(str::to_string))
        .collect::<Vec<_>>()
        .join(" ")
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

fn concise(text: &str, max_chars: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max_chars {
        return text;
    }
    format!(
        "{}...",
        text.chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>()
    )
}

fn live_step_status_label(status: &LiveStepStatus) -> &'static str {
    match status {
        LiveStepStatus::Starting => "starting runtime work",
        LiveStepStatus::Running => "runtime is running",
        LiveStepStatus::Streaming => "runtime is running",
        LiveStepStatus::WaitingForAction => "waiting for harness action",
        LiveStepStatus::WaitingForApproval => "waiting for action approval",
        LiveStepStatus::Cancelling => "cancelling runtime work",
        LiveStepStatus::Interrupted => "runtime work interrupted",
        LiveStepStatus::Completed => "runtime work completed",
        LiveStepStatus::Failed => "runtime work failed",
    }
}

fn live_step_status_title_suffix(status: &LiveStepStatus) -> &'static str {
    match status {
        LiveStepStatus::Starting => "is starting",
        LiveStepStatus::Running => "is running",
        LiveStepStatus::Streaming => "is running",
        LiveStepStatus::WaitingForAction => "is waiting for action",
        LiveStepStatus::WaitingForApproval => "is waiting for approval",
        LiveStepStatus::Cancelling => "is cancelling",
        LiveStepStatus::Interrupted => "was interrupted",
        LiveStepStatus::Completed => "completed runtime output",
        LiveStepStatus::Failed => "failed",
    }
}

fn chat_status_for_live_step(status: &LiveStepStatus) -> ChatItemStatus {
    match status {
        LiveStepStatus::WaitingForApproval => ChatItemStatus::WaitingApproval,
        LiveStepStatus::Interrupted => ChatItemStatus::Interrupted,
        LiveStepStatus::Completed => ChatItemStatus::Completed,
        LiveStepStatus::Failed => ChatItemStatus::Failed,
        _ => ChatItemStatus::Running,
    }
}

fn chat_severity_for_live_step(status: &LiveStepStatus) -> ChatSeverity {
    match status {
        LiveStepStatus::WaitingForApproval | LiveStepStatus::Cancelling => ChatSeverity::Warning,
        LiveStepStatus::Interrupted | LiveStepStatus::Failed => ChatSeverity::Error,
        LiveStepStatus::Completed => ChatSeverity::Success,
        _ => ChatSeverity::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(
        kind: &str,
        run_id: Option<&str>,
        step_id: Option<&str>,
        payload: Value,
    ) -> HistoryEvent {
        HistoryEvent {
            schema_version: 1,
            event_id: format!("event-{kind}"),
            session_id: "session".to_string(),
            run_id: run_id.map(str::to_string),
            step_id: step_id.map(str::to_string),
            timestamp: "2026-06-05T00:00:00.000Z".to_string(),
            kind: kind.to_string(),
            payload,
        }
    }

    #[test]
    fn rebuild_produces_stable_prompt_id() {
        let events = vec![event(
            "prompt_submitted",
            Some("run"),
            None,
            json!({ "prompt": "build it" }),
        )];

        let projection = ChatProjection::rebuild(&events);

        assert_eq!(projection.items()[0].id, "chat:prompt:run");
    }

    #[test]
    fn runtime_stream_delta_updates_agent_progress_without_generic_item() {
        let events = vec![
            event(
                "agent_step_started",
                Some("run"),
                Some("step"),
                json!({ "agent": "fixer" }),
            ),
            event(
                "runtime_stream_delta",
                Some("run"),
                Some("step"),
                json!({ "agent": "fixer", "stream": "stdout", "content": "checking", "final_delta": false }),
            ),
        ];

        let projection = ChatProjection::rebuild(&events);

        assert_eq!(projection.items().len(), 1);
        assert_eq!(projection.items()[0].kind, ChatItemKind::AgentProgress);
    }

    #[test]
    fn coalesced_runtime_stream_delta_renders_sequence_range_and_final_marker() {
        let events = vec![event(
            "runtime_stream_delta",
            Some("run"),
            Some("step"),
            json!({
                "agent": "fixer",
                "sequence_start": 3,
                "sequence_end": 5,
                "stream": "stdout",
                "content": "checking\nverified",
                "final_delta": true,
                "coalesced": true
            }),
        )];

        let projection = ChatProjection::rebuild(&events);

        let item = &projection.items()[0];
        assert_eq!(item.status, ChatItemStatus::Completed);
        assert_eq!(item.severity, ChatSeverity::Success);
        assert!(item
            .summary
            .as_deref()
            .unwrap()
            .contains("stdout:final:#3-5"));
        assert!(item
            .body
            .iter()
            .any(|line| line.text.contains("[stdout:final:#3-5]")));
    }

    #[test]
    fn command_lifecycle_aggregates_into_one_item() {
        let events = vec![
            event(
                "action_requested",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "step_id": "step",
                    "kind": "run_command",
                    "params": { "command": "cargo test" }
                }),
            ),
            event(
                "command_started",
                Some("run"),
                Some("step"),
                json!({ "action_id": "action", "command": "cargo test" }),
            ),
            event(
                "action_completed",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "status": "completed",
                    "summary": "Command exited with status 0.",
                    "content": {
                        "command": "cargo test",
                        "exit_code": 0,
                        "stdout": "running 1 test\ntest result: ok. 1 passed\n",
                        "stderr": ""
                    },
                    "artifact": null,
                    "diagnostic": null
                }),
            ),
        ];

        let projection = ChatProjection::rebuild(&events);

        assert_eq!(projection.items().len(), 1);
        assert_eq!(projection.items()[0].kind, ChatItemKind::CommandResult);
        assert_eq!(projection.items()[0].severity, ChatSeverity::Success);
    }

    #[test]
    fn recoverable_denial_is_warning() {
        let events = vec![
            event(
                "action_requested",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "step_id": "step",
                    "kind": "run_command",
                    "params": { "command": "cargo install pretend-package" }
                }),
            ),
            event(
                "action_denied",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "status": "denied",
                    "summary": "Action approval denied.",
                    "content": null,
                    "artifact": null,
                    "diagnostic": "user denied action approval"
                }),
            ),
        ];

        let projection = ChatProjection::rebuild(&events);

        assert_eq!(projection.items()[0].severity, ChatSeverity::Warning);
    }

    #[test]
    fn apply_patch_uses_diff_preview_counts() {
        let events = vec![event(
            "action_requested",
            Some("run"),
            Some("step"),
            json!({
                "schema_version": 1,
                "action_id": "action",
                "step_id": "step",
                "kind": "apply_patch",
                "params": {
                    "diff": "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n"
                }
            }),
        )];

        let projection = ChatProjection::rebuild(&events);

        assert_eq!(projection.items()[0].summary.as_deref(), Some("+1 -1"));
    }

    #[test]
    fn denied_apply_patch_does_not_claim_files_were_edited() {
        let events = vec![
            event(
                "action_requested",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "step_id": "step",
                    "kind": "apply_patch",
                    "params": {
                        "diff": "--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n # Multiagent Harness\n+extra line\n"
                    }
                }),
            ),
            event(
                "action_denied",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "status": "denied",
                    "summary": "Action denied by harness policy.",
                    "content": null,
                    "artifact": null,
                    "diagnostic": "agent fixer lacks required capability WriteWorkspace"
                }),
            ),
            event(
                "action_completed",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "status": "denied",
                    "summary": "Action denied by harness policy.",
                    "content": null,
                    "artifact": null,
                    "diagnostic": "agent fixer lacks required capability WriteWorkspace"
                }),
            ),
        ];

        let projection = ChatProjection::rebuild(&events);
        let item = &projection.items()[0];

        assert_eq!(projection.items().len(), 1);
        assert_eq!(item.kind, ChatItemKind::FileEdit);
        assert_eq!(item.status, ChatItemStatus::Denied);
        assert_eq!(item.severity, ChatSeverity::Warning);
        assert!(item.title.starts_with("Patch denied"));
        assert!(!item.title.contains("Files edited"));
        assert_eq!(
            item.summary.as_deref(),
            Some("not applied (+1 -0 requested)")
        );
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "requested patch was not applied"));
        assert!(item.body.iter().any(|line| line.text == "+extra line"));
    }

    #[test]
    fn search_text_completion_surfaces_match_locations() {
        let events = vec![
            event(
                "action_requested",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "step_id": "step",
                    "kind": "search_text",
                    "params": {
                        "query": "npm distribution plan",
                        "path": "."
                    }
                }),
            ),
            event(
                "action_completed",
                Some("run"),
                Some("step"),
                json!({
                    "schema_version": 1,
                    "action_id": "action",
                    "status": "completed",
                    "summary": "Found 200 matches for \"npm distribution plan\".",
                    "content": {
                        "query": "npm distribution plan",
                        "path": ".",
                        "matches": [
                            { "path": "docs/npm-distribution-plan.md", "line": 1, "text": "# npm distribution plan" },
                            { "path": "README.md", "line": 12, "text": "See npm distribution plan." }
                        ],
                        "total_matches": 200,
                        "truncated": true
                    },
                    "artifact": {
                        "artifact_id": "artifact",
                        "path": "sessions/session/artifacts/artifact.json"
                    },
                    "diagnostic": null
                }),
            ),
        ];

        let projection = ChatProjection::rebuild(&events);
        let item = &projection.items()[0];

        assert_eq!(item.title, "Search text completed");
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "match: docs/npm-distribution-plan.md:1"));
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "match: README.md:12"));
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "showing 2 of 200 matches"));
    }

    #[test]
    fn agent_result_json_summary_is_rendered_as_human_lines() {
        let events = vec![event(
            "agent_result",
            Some("run"),
            Some("step"),
            json!({
                "schema_version": 1,
                "agent": "fixer",
                "step_id": "step",
                "status": "completed",
                "summary": r#"{"summary":"Implemented chat polish","changed_files":["src/tui/mod.rs"],"verification":["cargo test"]}"#,
                "findings": [],
                "changed_files": [],
                "commands": [],
                "verification": [],
                "blocker": null,
                "artifacts": []
            }),
        )];

        let projection = ChatProjection::rebuild(&events);

        let item = &projection.items()[0];
        assert_eq!(item.summary.as_deref(), Some("Implemented chat polish"));
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "summary: Implemented chat polish"));
        assert!(!item.body.iter().any(|line| line.text.contains('{')));
    }

    #[test]
    fn runtime_stream_enveloped_json_surfaces_final_summary() {
        let events = vec![event(
            "runtime_stream_delta",
            Some("run"),
            Some("step"),
            json!({
                "agent": "orchestrator",
                "stream": "stdout",
                "content": "<<<MULTIAGENT_JSON_START>>>\n{\"status\":\"complete\",\"final_summary\":\"Ready to ship\",\"plan\":[\"verify\",\"summarize\"]}\n<<<MULTIAGENT_JSON_END>>>",
                "final_delta": true
            }),
        )];

        let projection = ChatProjection::rebuild(&events);

        let item = &projection.items()[0];
        assert_eq!(item.summary.as_deref(), Some("stdout:final: Ready to ship"));
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "[stdout:final] final summary: Ready to ship"));
        assert!(item
            .body
            .iter()
            .any(|line| line.text == "[stdout:final] plan: verify"));
    }
}
