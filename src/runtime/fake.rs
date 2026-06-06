use super::{
    Runtime, RuntimeAvailability, RuntimeAvailabilityStatus, RuntimeEventSink, RuntimeOutput,
    RuntimeProviderError, RuntimeRequest,
};
use crate::actions::{ActionKind, ActionRequest, ActionStatus};
use crate::config::{Capability, RuntimeConfig};
use crate::ids::new_id;
use crate::orchestrator::{
    agent_results, AgentResult, AgentResultStatus, DecisionNextStep, DecisionStatus,
    OrchestratorDecision, ParallelChildStepPlan, ParallelFileScope, ParallelGroupPlan,
};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

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

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput> {
        if should_emit_fake_retryable_provider_error(&request) {
            return Err(RuntimeProviderError::retryable(format!(
                "fake retryable provider error for model {}",
                request.agent_profile.model
            ))
            .into());
        }
        emit_fake_progress(&request, &events, &cancellation).await?;
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
            let path = fake_write_action_path(&request);
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::WriteFile,
                    params: serde_json::json!({
                        "path": path,
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

        Ok(output)
    }
}

async fn emit_fake_progress(
    request: &RuntimeRequest,
    events: &RuntimeEventSink,
    cancellation: &CancellationToken,
) -> Result<()> {
    let delay = fake_stream_delay(request);
    events
        .status(format!(
            "fake runtime started {} step {}",
            request.agent_profile.id, request.step_id
        ))
        .await?;
    sleep_or_cancel(cancellation, delay).await?;
    events
        .delta(
            "fake",
            format!(
                "{} is preparing structured output\n",
                request.agent_profile.id
            ),
        )
        .await?;
    sleep_or_cancel(cancellation, delay).await?;
    events
        .delta(
            "fake",
            format!("{} is returning final contract\n", request.agent_profile.id),
        )
        .await
}

fn fake_stream_delay(request: &RuntimeRequest) -> Duration {
    let prompt = request.prompt.to_ascii_lowercase();
    if request.parallel_context.is_some() {
        if prompt.contains("approval action") && request.agent_profile.id == "fixer" {
            return Duration::from_millis(50);
        }
        if prompt.contains("approval action") {
            return Duration::from_millis(250);
        }
        Duration::from_millis(500)
    } else if prompt.contains("parallel") {
        Duration::from_millis(50)
    } else if prompt.contains("slow stream") {
        Duration::from_secs(5)
    } else {
        Duration::from_millis(5)
    }
}

async fn sleep_or_cancel(cancellation: &CancellationToken, duration: Duration) -> Result<()> {
    tokio::select! {
        _ = cancellation.cancelled() => anyhow::bail!("runtime step cancelled"),
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}

fn fake_decision(request: &RuntimeRequest) -> OrchestratorDecision {
    let last_agent = request
        .previous_results
        .iter()
        .next_back()
        .map(|result| match result {
            crate::orchestrator::RunStepResult::Agent { result } => result.agent.as_str(),
            crate::orchestrator::RunStepResult::ParallelGroup { .. } => "parallel_group",
        });
    let prompt = request.prompt.to_ascii_lowercase();
    if last_agent.is_none() && prompt.contains("parallel") {
        let steps = if prompt.contains("same agent") || prompt.contains("scoped write action") {
            vec![
                ParallelChildStepPlan {
                    step_label: "fix first scoped file".to_string(),
                    agent: "fixer".to_string(),
                    instruction: "Handle the first fake scoped file.".to_string(),
                    required_capabilities: vec![
                        Capability::Read,
                        Capability::Edit,
                        Capability::Command,
                        Capability::Verify,
                    ],
                    file_scope: ParallelFileScope {
                        write_files: vec!["parallel-output/fixer-a.txt".to_string()],
                        read_roots: vec!["src/runtime".to_string()],
                    },
                },
                ParallelChildStepPlan {
                    step_label: "fix second scoped file".to_string(),
                    agent: "fixer".to_string(),
                    instruction: "Handle the second fake scoped file.".to_string(),
                    required_capabilities: vec![
                        Capability::Read,
                        Capability::Edit,
                        Capability::Command,
                        Capability::Verify,
                    ],
                    file_scope: ParallelFileScope {
                        write_files: vec!["parallel-output/fixer-b.txt".to_string()],
                        read_roots: vec!["src/app".to_string()],
                    },
                },
            ]
        } else {
            vec![
                ParallelChildStepPlan {
                    step_label: "fix runtime scope".to_string(),
                    agent: "fixer".to_string(),
                    instruction: "Handle the fake runtime file scope.".to_string(),
                    required_capabilities: vec![
                        Capability::Read,
                        Capability::Edit,
                        Capability::Command,
                        Capability::Verify,
                    ],
                    file_scope: ParallelFileScope {
                        write_files: vec!["src/runtime/fake.rs".to_string()],
                        read_roots: vec!["src/runtime".to_string()],
                    },
                },
                ParallelChildStepPlan {
                    step_label: "review app scope".to_string(),
                    agent: "reviewer".to_string(),
                    instruction: "Review the fake app file scope.".to_string(),
                    required_capabilities: vec![
                        Capability::Read,
                        Capability::Command,
                        Capability::Verify,
                        Capability::Review,
                    ],
                    file_scope: ParallelFileScope {
                        write_files: Vec::new(),
                        read_roots: vec!["src/app".to_string()],
                    },
                },
            ]
        };
        return OrchestratorDecision {
            schema_version: 2,
            decision_id: new_id(),
            run_id: request.run_id.clone(),
            status: DecisionStatus::Continue,
            plan: vec![
                "Split independent file-scoped work into a Parallel Step Group.".to_string(),
                "Join child results before returning to the Orchestrator.".to_string(),
            ],
            next_agent: None,
            next_step: Some(DecisionNextStep::ParallelGroup(ParallelGroupPlan {
                group_id: new_id(),
                reason: "Fake runtime selected disjoint file scopes for parallel execution."
                    .to_string(),
                steps,
            })),
            reason: "Independent fake scopes can run concurrently.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "All parallel children have terminal results.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };
    }
    let (status, next_agent, required_capabilities, reason, stop_condition, final_summary) = match last_agent {
        None if prompt.contains("needs clarification") && !prompt.contains("user clarification:") => (
            DecisionStatus::WaitingForUser,
            None,
            Vec::new(),
            "The prompt needs one user clarification before routing.".to_string(),
            "User provides clarification.".to_string(),
            None,
        ),
        None if prompt.contains("council") || prompt.contains("high-risk") || prompt.contains("high risk") => (
            DecisionStatus::Continue,
            Some("council".to_string()),
            vec![Capability::Read, Capability::Challenge, Capability::Review],
            "The prompt asks for council or high-risk review, so the harness council workflow should run.".to_string(),
            "Council returns confidence, dissent, risks, and a recommended action.".to_string(),
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
        next_step: None,
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
    if let Some(denied) = request.action_results.iter().find(|result| {
        matches!(result.status, ActionStatus::Denied)
            && result.diagnostic.as_deref().is_some_and(|diagnostic| {
                diagnostic.contains("parallel step")
                    || diagnostic.contains("exact write_files")
                    || diagnostic.contains("read roots")
            })
    }) {
        result.status = AgentResultStatus::Blocked;
        result.summary = "Fake runtime blocked after a harness action denial.".to_string();
        result.blocker = denied.diagnostic.clone();
        return result;
    }
    if request.parallel_context.is_some() {
        if let Some(denied) = request
            .action_results
            .iter()
            .find(|result| matches!(result.status, ActionStatus::Denied))
        {
            result.status = AgentResultStatus::ApprovalDenied;
            result.summary =
                "Fake runtime stopped after parallel action approval was denied.".to_string();
            result.blocker = denied.diagnostic.clone();
            return result;
        }
    }
    match agent.as_str() {
        "explorer" => {
            result.findings = vec![
                "Repository context gathered from the prompt and effective configuration."
                    .to_string(),
            ];
        }
        "fixer" => {
            let changed_files = completed_action_paths(request);
            if changed_files.is_empty() {
                result.status = AgentResultStatus::NoChanges;
                result.summary =
                    "Fake Fixer validated the requested change path without modifying files."
                        .to_string();
            } else {
                result.status = AgentResultStatus::Completed;
                result.summary = "Fake Fixer wrote its assigned scoped file.".to_string();
                result.changed_files = changed_files;
            }
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
        agent if agent.starts_with("council.") => {
            result.findings = vec![format!(
                "Fake councillor {} found the proposed path acceptable with noted risk.",
                agent.trim_start_matches("council.")
            )];
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

fn fake_write_action_path(request: &RuntimeRequest) -> String {
    if request
        .prompt
        .to_ascii_lowercase()
        .contains("scoped write action")
    {
        if let Some(path) = request
            .parallel_context
            .as_ref()
            .and_then(|context| context.file_scope.write_files.first())
        {
            return path.clone();
        }
    }
    "multiagent-action-output.txt".to_string()
}

fn completed_action_paths(request: &RuntimeRequest) -> Vec<String> {
    request
        .action_results
        .iter()
        .filter(|result| matches!(result.status, ActionStatus::Completed))
        .filter_map(|result| {
            result
                .content
                .as_ref()
                .and_then(|content| content.get("path"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn should_emit_fake_parse_error(request: &RuntimeRequest) -> bool {
    let prompt = request.prompt.to_ascii_lowercase();
    if request
        .agent_profile
        .instructions
        .to_ascii_lowercase()
        .contains("fake parse error")
    {
        return true;
    }
    if prompt.contains("always parse error") {
        return true;
    }

    let agent_id = request.agent_profile.id.as_str();
    let requested_once = (agent_id == "orchestrator"
        && prompt.contains("orchestrator parse error"))
        || (agent_id == "explorer" && prompt.contains("agent parse error"));
    requested_once && !has_prior_parse_error(request, agent_id)
}

fn should_emit_fake_retryable_provider_error(request: &RuntimeRequest) -> bool {
    request
        .prompt
        .to_ascii_lowercase()
        .contains("retryable provider error")
        && request.agent_profile.model == "primary-fails"
}

fn has_prior_parse_error(request: &RuntimeRequest, agent_id: &str) -> bool {
    agent_results(&request.previous_results).any(|result| {
        result.agent == agent_id && matches!(result.status, AgentResultStatus::ParseError)
    })
}
