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
use std::path::PathBuf;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeRequest {
    pub run_id: String,
    pub step_id: String,
    pub prompt: String,
    pub working_directory: PathBuf,
    pub agent_profile: AgentProfile,
    pub session_events: Vec<RuntimeHistoryEvent>,
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
    async fn stream_step(&self, request: RuntimeRequest) -> Result<RuntimeStepResult>;
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

    match runtime.kind {
        RuntimeKind::Codex => {
            codex::CodexRuntime::new(runtime.clone())
                .stream_step(request)
                .await
        }
        RuntimeKind::Zai => {
            zai::ZaiRuntime::new(runtime.clone())
                .stream_step(request)
                .await
        }
        RuntimeKind::Fake => {
            fake::FakeRuntime::new(runtime.clone())
                .stream_step(request)
                .await
        }
    }
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
        "session_events": request.session_events,
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
    use crate::config::{AgentEffort, AgentProfile, Capability, Limits};
    use serde_json::json;

    #[test]
    fn prompt_envelope_includes_session_events() {
        let request = RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "inspect context".to_string(),
            working_directory: PathBuf::from("/tmp/project"),
            agent_profile: AgentProfile {
                id: "explorer".to_string(),
                name: "Explorer".to_string(),
                runtime: "fake".to_string(),
                model: "default".to_string(),
                effort: AgentEffort::Medium,
                thinking: false,
                capabilities: vec![Capability::Read],
                instructions: "Read files.".to_string(),
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
        assert_eq!(
            envelope["session_events"][0]["payload"]["summary"],
            "prior finding"
        );
    }
}
