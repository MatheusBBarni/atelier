pub mod codex;
pub mod fake;
pub mod zai;

use crate::actions::{ActionRequest, ActionResult};
use crate::config::{
    AgentProfile, Capability, EffectiveConfig, Limits, RuntimeConfig, RuntimeKind,
};
use crate::orchestrator::{AgentResult, OrchestratorDecision};
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
    pub previous_results: Vec<AgentResult>,
    pub action_results: Vec<ActionResult>,
    pub output_schema: String,
    pub capability_constraints: Vec<Capability>,
    pub limits: Limits,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStreamDelta {
    pub sequence: u32,
    pub stream: String,
    pub content: String,
    pub final_delta: bool,
}

impl RuntimeStreamDelta {
    pub fn new(sequence: u32, stream: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            sequence,
            stream: stream.into(),
            content: content.into(),
            final_delta: false,
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
            | Self::Status { sequence, .. }
            | Self::ToolCallProgress { sequence, .. }
            | Self::Diagnostic { sequence, .. } => *sequence,
        }
    }

    pub fn stream_name(&self) -> String {
        match self {
            Self::Delta { stream, .. } | Self::Diagnostic { stream, .. } => stream.clone(),
            Self::Status { .. } => "status".to_string(),
            Self::ToolCallProgress { name, .. } => format!("tool:{name}"),
        }
    }

    pub fn content(&self) -> String {
        match self {
            Self::Delta { content, .. } | Self::Diagnostic { content, .. } => content.clone(),
            Self::Status { message, .. } => message.clone(),
            Self::ToolCallProgress { summary, .. } => summary.clone(),
        }
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
        RuntimeKind::Zai => {
            zai::ZaiRuntime::new(runtime.clone())
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

    let availability = check_runtime_availability(runtime).await;
    if matches!(availability.status, RuntimeAvailabilityStatus::Unavailable) {
        bail!(
            "runtime {runtime_id} is unavailable: {}",
            availability.message
        );
    }

    let model_chain = request.agent_profile.model_chain();
    let mut last_retryable_error = None;
    for (index, model) in model_chain.iter().enumerate() {
        if cancellation.is_cancelled() {
            bail!("runtime step cancelled");
        }
        let mut attempt = request.clone();
        attempt.agent_profile.model = model.clone();
        match execute_runtime_step_once(runtime, attempt, events.clone(), cancellation.clone())
            .await
        {
            Ok(result) => return Ok(result),
            Err(error) if index + 1 < model_chain.len() && is_retryable_provider_error(&error) => {
                events
                    .status(format!(
                        "retryable provider error for model {model}; trying fallback model {}",
                        model_chain[index + 1]
                    ))
                    .await?;
                last_retryable_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

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
        RuntimeKind::Zai => {
            zai::ZaiRuntime::new(runtime.clone())
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
        events.delta(delta.stream, delta.content).await?;
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

    #[tokio::test]
    async fn execute_runtime_step_retries_retryable_provider_errors_with_fallback_model() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
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
    async fn execute_runtime_step_does_not_retry_parse_outputs_as_provider_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
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
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let result = execute_runtime_step(&config, request).await.unwrap();

        assert!(matches!(result.output, RuntimeOutput::ParseError { .. }));
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
        let config_path = dir.path().join("multiagent.toml");
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
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        };

        let error = execute_runtime_step(&config, request).await.unwrap_err();
        let message = error.to_string();

        assert!(message.contains("runtime codex is unavailable"));
        assert!(message.contains("Not logged in"));
        assert!(!message.contains("status 99"));
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
