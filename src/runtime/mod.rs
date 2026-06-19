pub mod claude;
pub mod codex;
pub mod cursor;
pub mod fake;
pub mod http_api;
pub mod http_util;
pub mod status;

// Re-export the shared HTTP utilities so existing import paths keep working
// after the extraction in ADR-003: `crate::runtime::redact_sensitive_text`
// (used by the hooks and history modules) and `crate::runtime::parse_runtime_output`
// (imported via `super::` by the claude and cursor runtimes). `redact_bearer_tokens`
// stays reachable as `http_util::redact_bearer_tokens`; it has no cross-module
// consumer, so re-exporting it here would be an unused import.
pub(crate) use http_util::{parse_runtime_output, redact_sensitive_text};

use crate::actions::{ActionRequest, ActionResult};
use crate::config::{
    AgentProfile, Capability, EffectiveConfig, Limits, RuntimeConfig, RuntimeKind,
};
use crate::orchestrator::{AgentResult, OrchestratorDecision, ParallelFileScope, RunStepResult};
use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const RUNTIME_EVENT_CHANNEL_CAPACITY: usize = 64;

#[cfg(test)]
pub(crate) static CODEX_ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAvailabilityStatus {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeAvailability {
    pub runtime_id: String,
    pub status: RuntimeAvailabilityStatus,
    pub message: String,
    pub remediation: Option<String>,
}

#[derive(Debug)]
pub struct RuntimeProviderError {
    retryable: bool,
    message: String,
}

impl RuntimeProviderError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            retryable: true,
            message: message.into(),
        }
    }

    pub fn non_retryable(message: impl Into<String>) -> Self {
        Self {
            retryable: false,
            message: message.into(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl fmt::Display for RuntimeProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl StdError for RuntimeProviderError {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeHistoryEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub group_id: Option<String>,
    pub step_id: Option<String>,
    pub timestamp: String,
    pub kind: String,
    pub payload: Value,
    pub payload_truncated: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRecentContext {
    pub files: Vec<RuntimeRecentFile>,
    pub actions: Vec<RuntimeRecentAction>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRecentFile {
    pub path: String,
    pub operation: String,
    pub event_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRecentAction {
    pub event_kind: String,
    pub action_id: Option<String>,
    pub action_kind: Option<String>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub event_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeRequest {
    pub run_id: String,
    pub step_id: String,
    pub prompt: String,
    pub session_goal: Option<String>,
    pub working_directory: PathBuf,
    pub agent_profile: AgentProfile,
    pub session_events: Vec<RuntimeHistoryEvent>,
    pub recent_context: RuntimeRecentContext,
    pub previous_results: Vec<RunStepResult>,
    pub action_results: Vec<ActionResult>,
    pub output_schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_context: Option<ParallelRuntimeContext>,
    pub capability_constraints: Vec<Capability>,
    pub limits: Limits,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelRuntimeContext {
    pub group_id: String,
    pub step_label: String,
    pub file_scope: ParallelFileScope,
    pub parallel_siblings: Vec<ParallelSiblingContext>,
    pub scope_policy_summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelSiblingContext {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStreamDelta {
    pub sequence: u32,
    pub stream: String,
    pub content: String,
    pub final_delta: bool,
    #[serde(default)]
    pub transient: bool,
}

impl RuntimeStreamDelta {
    pub fn new(sequence: u32, stream: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            sequence,
            stream: stream.into(),
            content: content.into(),
            final_delta: false,
            transient: false,
        }
    }

    pub fn final_delta(
        sequence: u32,
        stream: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            sequence,
            stream: stream.into(),
            content: content.into(),
            final_delta: true,
            transient: false,
        }
    }

    pub fn transient(sequence: u32, stream: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            sequence,
            stream: stream.into(),
            content: content.into(),
            final_delta: false,
            transient: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RuntimeEvent {
    Delta {
        sequence: u32,
        stream: String,
        content: String,
    },
    TransientDelta {
        sequence: u32,
        stream: String,
        content: String,
    },
    Status {
        sequence: u32,
        message: String,
    },
    ToolCallProgress {
        sequence: u32,
        name: String,
        summary: String,
    },
    Diagnostic {
        sequence: u32,
        stream: String,
        content: String,
    },
}

impl RuntimeEvent {
    pub fn sequence(&self) -> u32 {
        match self {
            Self::Delta { sequence, .. }
            | Self::TransientDelta { sequence, .. }
            | Self::Status { sequence, .. }
            | Self::ToolCallProgress { sequence, .. }
            | Self::Diagnostic { sequence, .. } => *sequence,
        }
    }

    pub fn stream_name(&self) -> String {
        match self {
            Self::Delta { stream, .. }
            | Self::TransientDelta { stream, .. }
            | Self::Diagnostic { stream, .. } => stream.clone(),
            Self::Status { .. } => "status".to_string(),
            Self::ToolCallProgress { name, .. } => format!("tool:{name}"),
        }
    }

    pub fn content(&self) -> String {
        match self {
            Self::Delta { content, .. }
            | Self::TransientDelta { content, .. }
            | Self::Diagnostic { content, .. } => content.clone(),
            Self::Status { message, .. } => message.clone(),
            Self::ToolCallProgress { summary, .. } => summary.clone(),
        }
    }

    pub fn is_transient(&self) -> bool {
        matches!(self, Self::TransientDelta { .. })
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeEventSink {
    sender: mpsc::Sender<RuntimeEvent>,
    next_sequence: Arc<AtomicU32>,
}

impl RuntimeEventSink {
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<RuntimeEvent>) {
        Self::channel_from(capacity, 1)
    }

    pub fn channel_from(
        capacity: usize,
        first_sequence: u32,
    ) -> (Self, mpsc::Receiver<RuntimeEvent>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                next_sequence: Arc::new(AtomicU32::new(first_sequence)),
            },
            receiver,
        )
    }

    pub async fn delta(&self, stream: impl Into<String>, content: impl Into<String>) -> Result<()> {
        self.send(RuntimeEvent::Delta {
            sequence: self.next_sequence(),
            stream: stream.into(),
            content: content.into(),
        })
        .await
    }

    pub async fn transient_delta(
        &self,
        stream: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<()> {
        self.send(RuntimeEvent::TransientDelta {
            sequence: self.next_sequence(),
            stream: stream.into(),
            content: content.into(),
        })
        .await
    }

    pub async fn status(&self, message: impl Into<String>) -> Result<()> {
        self.send(RuntimeEvent::Status {
            sequence: self.next_sequence(),
            message: message.into(),
        })
        .await
    }

    pub async fn diagnostic(
        &self,
        stream: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<()> {
        self.send(RuntimeEvent::Diagnostic {
            sequence: self.next_sequence(),
            stream: stream.into(),
            content: content.into(),
        })
        .await
    }

    pub async fn tool_call_progress(
        &self,
        name: impl Into<String>,
        summary: impl Into<String>,
    ) -> Result<()> {
        self.send(RuntimeEvent::ToolCallProgress {
            sequence: self.next_sequence(),
            name: name.into(),
            summary: summary.into(),
        })
        .await
    }

    async fn send(&self, event: RuntimeEvent) -> Result<()> {
        self.sender
            .send(event)
            .await
            .map_err(|_| anyhow::anyhow!("runtime event receiver closed"))
    }

    fn next_sequence(&self) -> u32 {
        self.next_sequence.fetch_add(1, Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStepResult {
    pub output: RuntimeOutput,
    pub stream_deltas: Vec<RuntimeStreamDelta>,
}

impl RuntimeStepResult {
    pub fn new(output: RuntimeOutput) -> Self {
        Self {
            output,
            stream_deltas: Vec::new(),
        }
    }

    pub fn with_delta(mut self, delta: RuntimeStreamDelta) -> Self {
        self.stream_deltas.push(delta);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RuntimeOutput {
    OrchestratorDecision {
        decision: OrchestratorDecision,
    },
    AgentResult {
        result: AgentResult,
    },
    ActionRequest {
        request: ActionRequest,
    },
    ParseError {
        agent: String,
        raw_output: String,
        diagnostic: String,
    },
}

#[async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;
    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput>;
}

pub async fn check_all_runtime_availability(
    config: &EffectiveConfig,
) -> BTreeMap<String, RuntimeAvailability> {
    let mut availability = BTreeMap::new();
    for (runtime_id, runtime) in &config.runtimes {
        availability.insert(
            runtime_id.clone(),
            check_runtime_availability(runtime).await,
        );
    }
    availability
}

pub async fn check_runtime_availability(runtime: &RuntimeConfig) -> RuntimeAvailability {
    match runtime.kind {
        RuntimeKind::Codex => {
            codex::CodexRuntime::new(runtime.clone())
                .check_availability()
                .await
        }
        RuntimeKind::Claude => {
            claude::ClaudeRuntime::new(runtime.clone())
                .check_availability()
                .await
        }
        RuntimeKind::Cursor => {
            cursor::CursorRuntime::new(runtime.clone())
                .check_availability()
                .await
        }
        RuntimeKind::HttpApi => {
            http_api::HttpApiRuntime::new(runtime.clone())
                .check_availability()
                .await
        }
        RuntimeKind::Fake => {
            fake::FakeRuntime::new(runtime.clone())
                .check_availability()
                .await
        }
    }
}

/// Total attempts per model on transient (retryable) provider errors before
/// moving to the next model in the chain. Two means one retry — enough to
/// recover from an occasional empty Claude turn without amplifying load.
const SAME_MODEL_PROVIDER_ATTEMPTS: u32 = 2;

pub async fn execute_runtime_step(
    config: &EffectiveConfig,
    request: RuntimeRequest,
) -> Result<RuntimeStepResult> {
    collect_runtime_step_result(|events, cancellation| {
        execute_runtime_step_streaming(config, request, events, cancellation)
    })
    .await
}

pub async fn execute_runtime_step_streaming(
    config: &EffectiveConfig,
    request: RuntimeRequest,
    events: RuntimeEventSink,
    cancellation: CancellationToken,
) -> Result<RuntimeOutput> {
    let runtime_id = &request.agent_profile.runtime;
    let runtime = config
        .runtimes
        .get(runtime_id)
        .ok_or_else(|| anyhow::anyhow!("agent references undefined runtime {runtime_id}"))?;

    if !matches!(runtime.kind, RuntimeKind::Claude) {
        let availability = check_runtime_availability(runtime).await;
        if matches!(availability.status, RuntimeAvailabilityStatus::Unavailable) {
            bail!(
                "runtime {runtime_id} is unavailable: {}",
                availability.message
            );
        }
    }

    let model_chain = request.agent_profile.model_chain();
    let mut last_retryable_error = None;
    for (index, model) in model_chain.iter().enumerate() {
        let has_fallback_model = index + 1 < model_chain.len();
        // Retry the same model on transient provider errors (e.g. a Claude turn
        // that ends with an empty result) before falling back to the next model
        // in the chain. Single-model agents otherwise die on the first hiccup.
        for attempt_number in 1..=SAME_MODEL_PROVIDER_ATTEMPTS {
            if cancellation.is_cancelled() {
                bail!("runtime step cancelled");
            }
            let mut attempt = request.clone();
            attempt.agent_profile.model = model.clone();
            match execute_runtime_step_once(runtime, attempt, events.clone(), cancellation.clone())
                .await
            {
                Ok(result) => return Ok(result),
                Err(error) if is_retryable_provider_error(&error) => {
                    if attempt_number < SAME_MODEL_PROVIDER_ATTEMPTS {
                        events
                            .status(format!(
                                "retryable provider error for model {model}; retrying same model (attempt {}/{})",
                                attempt_number + 1,
                                SAME_MODEL_PROVIDER_ATTEMPTS
                            ))
                            .await?;
                        continue;
                    }
                    if has_fallback_model {
                        events
                            .status(format!(
                                "retryable provider error for model {model}; trying fallback model {}",
                                model_chain[index + 1]
                            ))
                            .await?;
                        last_retryable_error = Some(error);
                        break;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
    }

    // Defensive only: model_chain() is never empty and the final model always
    // returns from inside the loop (Ok, or its own error), so neither of these
    // is reachable in practice. They keep `last_retryable_error` honest and give
    // a sane result if model_chain() ever becomes empty.
    if let Some(error) = last_retryable_error {
        return Err(error);
    }

    bail!("runtime {runtime_id} did not execute any model attempt")
}

async fn execute_runtime_step_once(
    runtime: &RuntimeConfig,
    request: RuntimeRequest,
    events: RuntimeEventSink,
    cancellation: CancellationToken,
) -> Result<RuntimeOutput> {
    match runtime.kind {
        RuntimeKind::Codex => {
            codex::CodexRuntime::new(runtime.clone())
                .stream_step(request, events, cancellation)
                .await
        }
        RuntimeKind::Claude => {
            claude::ClaudeRuntime::new(runtime.clone())
                .stream_step(request, events, cancellation)
                .await
        }
        RuntimeKind::Cursor => {
            cursor::CursorRuntime::new(runtime.clone())
                .stream_step(request, events, cancellation)
                .await
        }
        RuntimeKind::HttpApi => {
            http_api::HttpApiRuntime::new(runtime.clone())
                .stream_step(request, events, cancellation)
                .await
        }
        RuntimeKind::Fake => {
            fake::FakeRuntime::new(runtime.clone())
                .stream_step(request, events, cancellation)
                .await
        }
    }
}

pub async fn emit_legacy_step_result(
    result: RuntimeStepResult,
    events: &RuntimeEventSink,
) -> Result<RuntimeOutput> {
    for delta in result.stream_deltas {
        if delta.transient {
            events.transient_delta(delta.stream, delta.content).await?;
        } else {
            events.delta(delta.stream, delta.content).await?;
        }
    }
    Ok(result.output)
}

pub async fn collect_runtime_step_result<F, Fut>(operation: F) -> Result<RuntimeStepResult>
where
    F: FnOnce(RuntimeEventSink, CancellationToken) -> Fut,
    Fut: Future<Output = Result<RuntimeOutput>>,
{
    let (events, mut receiver) = RuntimeEventSink::channel(RUNTIME_EVENT_CHANNEL_CAPACITY);
    let cancellation = CancellationToken::new();
    let operation = operation(events, cancellation);
    tokio::pin!(operation);

    let mut runtime_events = Vec::new();
    let output = loop {
        tokio::select! {
            event = receiver.recv() => {
                if let Some(event) = event {
                    runtime_events.push(event);
                }
            }
            output = &mut operation => break output?,
        }
    };

    while let Ok(event) = receiver.try_recv() {
        runtime_events.push(event);
    }

    let last_sequence = runtime_events.last().map(RuntimeEvent::sequence);
    let stream_deltas = runtime_events
        .into_iter()
        .map(|event| {
            let sequence = event.sequence();
            RuntimeStreamDelta {
                sequence,
                stream: event.stream_name(),
                content: event.content(),
                final_delta: Some(sequence) == last_sequence,
                transient: event.is_transient(),
            }
        })
        .collect();

    Ok(RuntimeStepResult {
        output,
        stream_deltas,
    })
}

pub fn is_retryable_provider_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<RuntimeProviderError>()
            .map(RuntimeProviderError::is_retryable)
            .unwrap_or(false)
    })
}

pub(crate) fn process_output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match (stdout.trim(), stderr.trim()) {
        ("", "") => "command produced no output".to_string(),
        (stdout, "") => stdout.to_string(),
        ("", stderr) => stderr.to_string(),
        (stdout, stderr) => format!("{stdout}; {stderr}"),
    }
}

pub(crate) fn concise_runtime_text(text: &str) -> String {
    concise_runtime_text_with_limit(text, 240)
}

pub(crate) fn concise_runtime_text_with_limit(text: &str, max_chars: usize) -> String {
    let text = redact_sensitive_text(&text.split_whitespace().collect::<Vec<_>>().join(" "));
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

pub fn prompt_envelope_json(request: &RuntimeRequest) -> Result<String> {
    let envelope = serde_json::json!({
        "schema_version": 1,
        "agent": {
            "id": request.agent_profile.id,
            "name": request.agent_profile.name,
            "runtime": request.agent_profile.runtime,
            "model": request.agent_profile.model,
            "effort": request.agent_profile.effort,
            "thinking": request.agent_profile.thinking,
            "capabilities": request.agent_profile.capabilities,
            "instructions": request.agent_profile.instructions,
        },
        "run_id": request.run_id,
        "step_id": request.step_id,
        "working_directory": request.working_directory,
        "prompt": request.prompt,
        "session_goal": request.session_goal,
        "session_events": request.session_events,
        "recent_context": request.recent_context,
        "previous_results": request.previous_results,
        "action_results": request.action_results,
        "parallel_context": request.parallel_context,
        "capability_constraints": request.capability_constraints,
        "limits": request.limits,
        "output_schema": request.output_schema,
        "contract": {
            "json_start": crate::orchestrator::JSON_START,
            "json_end": crate::orchestrator::JSON_END
        }
    });
    Ok(serde_json::to_string_pretty(&envelope)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        load_effective_config, AgentEffort, AgentProfile, AgentPromptMetadata, Capability,
        ConfigLoadOptions, Limits,
    };
    use crate::orchestrator::{
        wrap_json_contract, AgentResultStatus, ParallelChildResultRef, ParallelGroupResult,
        ParallelGroupStatus,
    };
    use serde_json::json;

    #[tokio::test]
    async fn runtime_event_sink_assigns_monotonic_sequences() {
        let (events, mut receiver) = RuntimeEventSink::channel(4);

        events.delta("stdout", "first").await.unwrap();
        events.status("second").await.unwrap();
        events.diagnostic("stderr", "third").await.unwrap();
        events.tool_call_progress("search", "fourth").await.unwrap();

        let mut sequences = Vec::new();
        for _ in 0..4 {
            sequences.push(receiver.recv().await.unwrap().sequence());
        }

        assert_eq!(sequences, vec![1, 2, 3, 4]);
    }

    #[test]
    fn prompt_envelope_includes_session_events() {
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "inspect context".to_string(),
            session_goal: Some("Keep changes scoped.".to_string()),
            working_directory: PathBuf::from("/tmp/project"),
            agent_profile: AgentProfile {
                id: "explorer".to_string(),
                name: "Explorer".to_string(),
                runtime: "fake".to_string(),
                model: "default".to_string(),
                model_fallbacks: Vec::new(),
                effort: AgentEffort::Medium,
                thinking: false,
                capabilities: vec![Capability::Read],
                tools: None,
                instructions: "Read files.".to_string(),
                orchestrator_description: None,
                prompt_metadata: AgentPromptMetadata::default(),
                enabled: true,
            },
            session_events: vec![RuntimeHistoryEvent {
                schema_version: 1,
                event_id: "event".to_string(),
                session_id: "session".to_string(),
                run_id: Some("previous-run".to_string()),
                group_id: None,
                step_id: Some("previous-step".to_string()),
                timestamp: "2026-06-03T00:00:00.000Z".to_string(),
                kind: "agent_result".to_string(),
                payload: json!({ "summary": "prior finding" }),
                payload_truncated: false,
            }],
            recent_context: RuntimeRecentContext {
                files: vec![RuntimeRecentFile {
                    path: "src/lib.rs".to_string(),
                    operation: "apply_patch".to_string(),
                    event_id: "file-event".to_string(),
                }],
                actions: vec![RuntimeRecentAction {
                    event_kind: "action_completed".to_string(),
                    action_id: Some("action".to_string()),
                    action_kind: None,
                    status: Some("completed".to_string()),
                    summary: Some("Applied patch.".to_string()),
                    event_id: "action-event".to_string(),
                }],
            },
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let envelope: Value =
            serde_json::from_str(&prompt_envelope_json(&request).unwrap()).unwrap();

        assert_eq!(envelope["session_events"][0]["kind"], "agent_result");
        assert_eq!(envelope["agent"]["effort"], "medium");
        assert_eq!(envelope["agent"]["thinking"], false);
        assert_eq!(envelope["session_goal"], "Keep changes scoped.");
        assert_eq!(envelope["recent_context"]["files"][0]["path"], "src/lib.rs");
        assert_eq!(
            envelope["recent_context"]["actions"][0]["status"],
            "completed"
        );
        assert_eq!(
            envelope["session_events"][0]["payload"]["summary"],
            "prior finding"
        );
    }

    #[test]
    fn prompt_envelope_serializes_parallel_context_and_group_results() {
        let scope = ParallelFileScope {
            write_files: vec!["src/runtime/fake.rs".to_string()],
            read_roots: vec!["src/runtime".to_string()],
        };
        let agent_result = AgentResult::completed(
            "fixer".to_string(),
            "child-step".to_string(),
            "child completed".to_string(),
        );
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "child-step".to_string(),
            prompt: "parallel child".to_string(),
            session_goal: None,
            working_directory: PathBuf::from("/tmp/project"),
            agent_profile: AgentProfile {
                id: "fixer".to_string(),
                name: "Fixer".to_string(),
                runtime: "fake".to_string(),
                model: "default".to_string(),
                model_fallbacks: Vec::new(),
                effort: AgentEffort::Medium,
                thinking: false,
                capabilities: vec![Capability::Read, Capability::Edit],
                tools: None,
                instructions: "Edit files.".to_string(),
                orchestrator_description: None,
                prompt_metadata: AgentPromptMetadata::default(),
                enabled: true,
            },
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: vec![
                RunStepResult::Agent {
                    result: agent_result,
                },
                RunStepResult::ParallelGroup {
                    result: ParallelGroupResult {
                        schema_version: 1,
                        group_id: "group".to_string(),
                        run_id: "run".to_string(),
                        status: ParallelGroupStatus::Completed,
                        summary: "group joined".to_string(),
                        children: vec![ParallelChildResultRef {
                            step_id: "child-step".to_string(),
                            step_label: "fix runtime".to_string(),
                            agent: "fixer".to_string(),
                            file_scope: scope.clone(),
                            status: AgentResultStatus::Completed,
                            result_index: 0,
                        }],
                        counts: BTreeMap::from([("completed".to_string(), 1)]),
                        changed_files: vec!["src/runtime/fake.rs".to_string()],
                        blocked_scopes: Vec::new(),
                        failed_scopes: Vec::new(),
                        approval_denials: Vec::new(),
                        started_at: "2026-06-06T00:00:00.000Z".to_string(),
                        completed_at: "2026-06-06T00:00:01.000Z".to_string(),
                    },
                },
            ],
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: Some(ParallelRuntimeContext {
                group_id: "group".to_string(),
                step_label: "fix runtime".to_string(),
                file_scope: scope.clone(),
                parallel_siblings: vec![ParallelSiblingContext {
                    step_id: "sibling-step".to_string(),
                    step_label: "review app".to_string(),
                    agent: "reviewer".to_string(),
                    file_scope: ParallelFileScope {
                        write_files: Vec::new(),
                        read_roots: vec!["src/app".to_string()],
                    },
                }],
                scope_policy_summary: "Write files: src/runtime/fake.rs. Read roots: src/runtime."
                    .to_string(),
            }),
            capability_constraints: vec![Capability::Read, Capability::Edit],
            limits: Limits::default(),
        };

        let envelope: Value =
            serde_json::from_str(&prompt_envelope_json(&request).unwrap()).unwrap();

        assert_eq!(envelope["parallel_context"]["group_id"], "group");
        assert_eq!(
            envelope["parallel_context"]["parallel_siblings"][0]["agent"],
            "reviewer"
        );
        assert_eq!(envelope["previous_results"][0]["kind"], "agent");
        assert_eq!(envelope["previous_results"][1]["kind"], "parallel_group");
        assert_eq!(
            envelope["previous_results"][1]["result"]["children"][0]["file_scope"],
            serde_json::to_value(scope).unwrap()
        );
    }

    #[tokio::test]
    async fn execute_runtime_step_retries_retryable_provider_errors_with_fallback_model() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.explorer]
runtime = "fake"
model = "primary-fails"
model_fallbacks = ["fallback-succeeds"]
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "retryable provider error".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let result = execute_runtime_step(&config, request).await.unwrap();

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.agent, "explorer");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_runtime_step_retries_same_model_before_falling_back() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.explorer]
runtime = "fake"
model = "primary-fails"
model_fallbacks = ["fallback-succeeds"]
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "retryable provider error".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let result = execute_runtime_step(&config, request).await.unwrap();

        let statuses: Vec<String> = result
            .stream_deltas
            .iter()
            .filter(|delta| delta.stream == "status")
            .map(|delta| delta.content.clone())
            .collect();
        assert!(
            statuses.iter().any(|s| s.contains("retrying same model")),
            "expected a same-model retry status; got {statuses:?}"
        );
        assert!(
            statuses
                .iter()
                .any(|s| s.contains("trying fallback model fallback-succeeds")),
            "expected a fallback status after retries; got {statuses:?}"
        );
        match result.output {
            RuntimeOutput::AgentResult { result } => assert_eq!(result.agent, "explorer"),
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_runtime_step_surfaces_retryable_error_when_no_fallback_remains() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.explorer]
runtime = "fake"
model = "primary-fails"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "retryable provider error".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        // No fallback model: same-model retries are exhausted and the retryable
        // error is surfaced (the run does not hang or panic).
        let error = execute_runtime_step(&config, request).await.unwrap_err();
        assert!(is_retryable_provider_error(&error));
    }

    #[tokio::test]
    async fn execute_runtime_step_does_not_retry_parse_outputs_as_provider_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.explorer]
runtime = "fake"
model = "primary-fails"
model_fallbacks = ["fallback-succeeds"]
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "agent parse error".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let result = execute_runtime_step(&config, request).await.unwrap();

        assert!(matches!(result.output, RuntimeOutput::ParseError { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn execute_runtime_step_retries_cursor_provider_errors_with_fallback_model() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("cursor-fallback.sh");
        let attempts_path = dir.path().join("attempts.txt");
        let result = AgentResult::completed("explorer", "step", "fallback completed");
        let final_frame = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": wrap_json_contract(&result).unwrap(),
            "model": "fallback-succeeds"
        });
        std::fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "cursor-agent 1.0.0"
  exit 0
fi
if [ "$1" = "status" ]; then
  echo "Authenticated"
  exit 0
fi
printf '%s\n' "$@" >> "{}"
cat >/dev/null
case " $* " in
  *" --model primary-fails "*)
    echo "rate limit exceeded" >&2
    exit 1
    ;;
  *" --model fallback-succeeds "*)
    printf '%s\n' {}
    exit 0
    ;;
  *)
    echo "unexpected model args: $*" >&2
    exit 64
    ;;
esac
"#,
                attempts_path.display(),
                shell_single_quoted(&final_frame.to_string()),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
[runtimes.cursor]
command = "{}"

[agents.explorer]
runtime = "cursor"
model = "primary-fails"
model_fallbacks = ["fallback-succeeds"]
"#,
                script_path.display()
            ),
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "retry cursor provider error".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let result = execute_runtime_step(&config, request).await.unwrap();

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "fallback completed");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
        let attempts = std::fs::read_to_string(attempts_path).unwrap();
        assert!(attempts.contains("--model\nprimary-fails\n"));
        assert!(attempts.contains("--model\nfallback-succeeds\n"));
        assert!(result.stream_deltas.iter().any(|delta| {
            delta.stream == "status"
                && delta
                    .content
                    .contains("retryable provider error for model primary-fails")
        }));
    }

    #[tokio::test]
    async fn execute_runtime_step_does_not_retry_non_retryable_provider_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.explorer]
runtime = "fake"
model = "only-model"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "non-retryable provider error".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        // A non-retryable provider error must fail fast: it takes the catch-all
        // error arm (no same-model retry, no fallback) and surfaces unchanged.
        let error = execute_runtime_step(&config, request).await.unwrap_err();
        assert!(!is_retryable_provider_error(&error));
        assert!(error.to_string().contains("non-retryable provider error"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn execute_runtime_step_recovers_from_empty_claude_result_via_same_model_retry() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("claude-empty-then-recover.sh");
        let attempts_path = dir.path().join("attempts.txt");
        let init_frame = serde_json::json!({
            "type": "system",
            "subtype": "init",
            "session_id": "s1",
            "model": "claude-test"
        });
        let empty_frame = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": ""
        });
        let valid_frame = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": wrap_json_contract(&AgentResult::completed("explorer", "step", "claude recovered")).unwrap()
        });
        // First invocation returns an empty (thinking-only) result; the second
        // returns a valid contract. A single-model agent must recover on the
        // second SAME-model attempt without any fallback.
        std::fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "claude 2.0.0"; exit 0; fi
printf 'x' >> "{attempts}"
cat >/dev/null
count=$(wc -c < "{attempts}" | tr -d ' ')
printf '%s\n' {init}
if [ "$count" -le 1 ]; then
  printf '%s\n' {empty}
else
  printf '%s\n' {valid}
fi
exit 0
"#,
                attempts = attempts_path.display(),
                init = shell_single_quoted(&init_frame.to_string()),
                empty = shell_single_quoted(&empty_frame.to_string()),
                valid = shell_single_quoted(&valid_frame.to_string()),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
[runtimes.claude]
command = "{}"

[agents.explorer]
runtime = "claude"
model = "claude-test"
"#,
                script_path.display()
            ),
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "explore the repository".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let result = execute_runtime_step(&config, request).await.unwrap();

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "claude recovered");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
        // Exactly two spawns: the empty turn plus the recovering retry.
        let attempts = std::fs::read_to_string(&attempts_path).unwrap();
        assert_eq!(attempts.len(), 2, "expected exactly two claude invocations");
        let statuses: Vec<String> = result
            .stream_deltas
            .iter()
            .filter(|delta| delta.stream == "status")
            .map(|delta| delta.content.clone())
            .collect();
        assert!(
            statuses.iter().any(|s| s.contains("retrying same model")),
            "expected a same-model retry status; got {statuses:?}"
        );
        assert!(
            !statuses.iter().any(|s| s.contains("trying fallback model")),
            "recovery must come from the same model, not a fallback; got {statuses:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn execute_runtime_step_blocks_codex_when_login_status_fails() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = CODEX_ENV_MUTEX.lock().await;
        let _env_guard = EnvGuard::clear(&["CODEX_API_KEY", "CODEX_ACCESS_TOKEN"]);
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("codex-status.sh");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.137.0"
  exit 0
fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  echo "Not logged in" >&2
  exit 1
fi
if [ "$1" = "exec" ]; then
  echo "exec should not run" >&2
  exit 99
fi
echo "unexpected args: $@" >&2
exit 64
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config_path = dir.path().join("atelier.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
[runtimes.codex]
type = "codex"
command = "{}"

[agents.explorer]
runtime = "codex"
model = "default"
"#,
                script_path.display()
            ),
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "inspect context".to_string(),
            session_goal: None,
            working_directory: dir.path().to_path_buf(),
            agent_profile: config.agents["explorer"].clone(),
            session_events: Vec::new(),
            recent_context: RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let error = execute_runtime_step(&config, request).await.unwrap_err();
        let message = error.to_string();

        assert!(message.contains("runtime codex is unavailable"));
        assert!(message.contains("Not logged in"));
        assert!(!message.contains("status 99"));
    }

    #[cfg(unix)]
    fn shell_single_quoted(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    struct EnvGuard {
        vars: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn clear(names: &[&'static str]) -> Self {
            let vars = names
                .iter()
                .map(|name| {
                    let value = std::env::var_os(name);
                    std::env::remove_var(name);
                    (*name, value)
                })
                .collect();
            Self { vars }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.vars {
                match value {
                    Some(value) => std::env::set_var(*name, value),
                    None => std::env::remove_var(*name),
                }
            }
        }
    }
}
