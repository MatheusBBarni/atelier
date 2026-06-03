use super::{
    Runtime, RuntimeAvailability, RuntimeAvailabilityStatus, RuntimeOutput, RuntimeRequest,
    RuntimeStepResult, RuntimeStreamDelta,
};
use crate::actions::{ActionKind, ActionRequest};
use crate::config::{Capability, RuntimeConfig};
use crate::ids::new_id;
use crate::orchestrator::{AgentResult, AgentResultStatus, DecisionStatus, OrchestratorDecision};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct FakeRuntime {
    config: RuntimeConfig,
}

impl FakeRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Runtime for FakeRuntime {
    async fn check_availability(&self) -> RuntimeAvailability {
        RuntimeAvailability {
            runtime_id: self.config.id.clone(),
            status: RuntimeAvailabilityStatus::Available,
            message: "fake runtime is available for deterministic local runs".to_string(),
            remediation: None,
        }
    }

    async fn stream_step(&self, request: RuntimeRequest) -> Result<RuntimeStepResult> {
        let output = if should_emit_fake_parse_error(&request) {
            RuntimeOutput::ParseError {
                agent: request.agent_profile.id.clone(),
                raw_output: "plain prose without the required JSON contract".to_string(),
                diagnostic: "fake runtime emitted malformed control output".to_string(),
            }
        } else if request.agent_profile.id == "orchestrator" {
            RuntimeOutput::OrchestratorDecision {
                decision: fake_decision(&request),
            }
        } else if should_emit_fake_command_action_request(&request) {
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::RunCommand,
                    params: serde_json::json!({ "command": "pwd" }),
                },
            }
        } else if should_emit_fake_write_action_request(&request) {
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::WriteFile,
                    params: serde_json::json!({
                        "path": "multiagent-action-output.txt",
                        "content": "created by fake runtime\n"
                    }),
                },
            }
        } else if should_emit_fake_approval_action_request(&request) {
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::RunCommand,
                    params: serde_json::json!({ "command": "cargo install pretend-package" }),
                },
            }
        } else if should_emit_fake_action_request(&request) {
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::ReadFile,
                    params: serde_json::json!({ "path": "README.md" }),
                },
            }
        } else {
            RuntimeOutput::AgentResult {
                result: fake_agent_result(&request),
            }
        };

        Ok(
            RuntimeStepResult::new(output).with_delta(RuntimeStreamDelta::final_delta(
                1,
                "fake",
                format!(
                    "fake runtime completed {} step {}",
                    request.agent_profile.id, request.step_id
                ),
            )),
        )
    }
}

fn fake_decision(request: &RuntimeRequest) -> OrchestratorDecision {
    let last_agent = request
        .previous_results
        .last()
        .map(|result| result.agent.as_str());
    let prompt = request.prompt.to_ascii_lowercase();
    let (status, next_agent, required_capabilities, reason, stop_condition, final_summary) = match last_agent {
        None if prompt.contains("needs clarification") && !prompt.contains("user clarification:") => (
            DecisionStatus::WaitingForUser,
            None,
            Vec::new(),
            "The prompt needs one user clarification before routing.".to_string(),
            "User provides clarification.".to_string(),
            None,
        ),
        None if prompt.contains('?') && !looks_like_code_change(&prompt) => (
            DecisionStatus::Continue,
            Some("oracle".to_string()),
            vec![Capability::Read, Capability::Answer],
            "The prompt is question-oriented, so Oracle should answer from available context.".to_string(),
            "Oracle returns a typed answer.".to_string(),
            None,
        ),
        None if prompt.contains("architecture") || prompt.contains("design") => (
            DecisionStatus::Continue,
            Some("consul".to_string()),
            vec![Capability::Read, Capability::Challenge],
            "The prompt touches architecture, so Consul should challenge the plan first.".to_string(),
            "Consul returns concerns or clears the plan.".to_string(),
            None,
        ),
        None if prompt.contains("typo") => (
            DecisionStatus::Continue,
            Some("fixer".to_string()),
            vec![Capability::Read, Capability::Edit, Capability::Verify],
            "This looks like a tiny edit, so Fixer can proceed directly.".to_string(),
            "Fixer reports the scoped edit result.".to_string(),
            None,
        ),
        None => (
            DecisionStatus::Continue,
            Some("explorer".to_string()),
            vec![Capability::Read],
            "Need repository context before changing anything.".to_string(),
            "Explorer returns structured findings.".to_string(),
            None,
        ),
        Some("consul") => (
            DecisionStatus::Continue,
            Some("explorer".to_string()),
            vec![Capability::Read],
            "Consul completed the challenge pass; Explorer should gather implementation context.".to_string(),
            "Explorer returns structured findings.".to_string(),
            None,
        ),
        Some("explorer") => (
            DecisionStatus::Continue,
            Some("fixer".to_string()),
            vec![Capability::Read, Capability::Edit, Capability::Command, Capability::Verify],
            "Explorer gathered context; Fixer should make scoped changes and verify.".to_string(),
            "Fixer reports changed files and verification.".to_string(),
            None,
        ),
        Some("fixer") => (
            DecisionStatus::Continue,
            Some("reviewer".to_string()),
            vec![Capability::Read, Capability::Command, Capability::Verify, Capability::Review],
            "Fixer completed the implementation step; Reviewer should inspect the result.".to_string(),
            "Reviewer returns review findings.".to_string(),
            None,
        ),
        Some("reviewer") if prompt.contains("review cycle") => (
            DecisionStatus::Continue,
            Some("fixer".to_string()),
            vec![Capability::Read, Capability::Edit, Capability::Command, Capability::Verify],
            "Reviewer requested a follow-up fix pass.".to_string(),
            "Fixer handles review feedback or hits the configured review/fix limit.".to_string(),
            None,
        ),
        _ => (
            DecisionStatus::Complete,
            None,
            Vec::new(),
            "The fake run completed the planned agent loop.".to_string(),
            "Run complete.".to_string(),
            Some("Fake runtime completed the run through the orchestrator and specialized agents.".to_string()),
        ),
    };
    let clarifying_question = if matches!(status, DecisionStatus::WaitingForUser) {
        Some("Which target or constraint should guide this run?".to_string())
    } else {
        None
    };

    OrchestratorDecision {
        schema_version: 1,
        decision_id: new_id(),
        run_id: request.run_id.clone(),
        status,
        plan: vec![
            "Route prompt through Orchestrator.".to_string(),
            "Run the selected specialized agent.".to_string(),
            "Review result and stop when complete.".to_string(),
        ],
        next_agent,
        reason,
        required_capabilities,
        stop_condition,
        clarifying_question,
        final_summary,
    }
}

fn fake_agent_result(request: &RuntimeRequest) -> AgentResult {
    let agent = request.agent_profile.id.clone();
    let mut result = AgentResult::completed(
        agent.clone(),
        request.step_id.clone(),
        format!("Fake {agent} step completed."),
    );
    match agent.as_str() {
        "explorer" => {
            result.findings = vec![
                "Repository context gathered from the prompt and effective configuration."
                    .to_string(),
            ];
        }
        "fixer" => {
            result.status = AgentResultStatus::NoChanges;
            result.summary =
                "Fake Fixer validated the requested change path without modifying files."
                    .to_string();
            result.verification = vec!["fake verification passed".to_string()];
        }
        "reviewer" => {
            result.findings = vec!["No fake-runtime review findings.".to_string()];
            result.verification = vec!["fake review passed".to_string()];
        }
        "oracle" => {
            result.findings =
                vec!["Fake Oracle answer generated from available context.".to_string()];
        }
        "consul" => {
            result.findings =
                vec!["Fake Consul found no blocking architecture concern.".to_string()];
        }
        _ => {}
    }
    result
}

fn looks_like_code_change(prompt: &str) -> bool {
    ["fix", "change", "implement", "add", "create", "bug", "code"]
        .iter()
        .any(|needle| prompt.contains(needle))
}

fn should_emit_fake_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.has_capability(&Capability::Read)
        && request.prompt.to_ascii_lowercase().contains("use action")
}

fn should_emit_fake_approval_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Command)
        && request
            .prompt
            .to_ascii_lowercase()
            .contains("approval action")
}

fn should_emit_fake_command_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Command)
        && request
            .prompt
            .to_ascii_lowercase()
            .contains("command action")
}

fn should_emit_fake_write_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Edit)
        && request.prompt.to_ascii_lowercase().contains("write action")
}

fn should_emit_fake_parse_error(request: &RuntimeRequest) -> bool {
    let prompt = request.prompt.to_ascii_lowercase();
    if prompt.contains("always parse error") {
        return true;
    }

    let agent_id = request.agent_profile.id.as_str();
    let requested_once = (agent_id == "orchestrator"
        && prompt.contains("orchestrator parse error"))
        || (agent_id == "explorer" && prompt.contains("agent parse error"));
    requested_once && !has_prior_parse_error(request, agent_id)
}

fn has_prior_parse_error(request: &RuntimeRequest, agent_id: &str) -> bool {
    request.previous_results.iter().any(|result| {
        result.agent == agent_id && matches!(result.status, AgentResultStatus::ParseError)
    })
}
