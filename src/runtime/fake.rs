use super::{
    Runtime, RuntimeAvailability, RuntimeAvailabilityStatus, RuntimeEventSink, RuntimeOutput,
    RuntimeProviderError, RuntimeRequest,
};
use crate::actions::{ActionKind, ActionRequest, ActionStatus};
use crate::config::{Capability, RuntimeConfig};
use crate::ids::new_id;
use crate::orchestrator::{
    agent_results, AgentResult, AgentResultStatus, ClarificationOption, DecisionNextStep,
    DecisionStatus, OrchestratorDecision, ParallelChildStepPlan, ParallelFileScope,
    ParallelGroupPlan,
};
use anyhow::Result;
use async_trait::async_trait;
use std::borrow::Cow;
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
        if should_emit_fake_non_retryable_provider_error(&request) {
            return Err(RuntimeProviderError::non_retryable(format!(
                "fake non-retryable provider error for model {}",
                request.agent_profile.model
            ))
            .into());
        }
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
        } else if should_emit_fake_trusted_command_action_request(&request) {
            // A gray-area command that is harmless to actually run, so trust
            // tests can let it auto-execute instead of pausing.
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::RunCommand,
                    params: serde_json::json!({ "command": "echo trusted-run" }),
                },
            }
        } else if should_emit_fake_catastrophic_action_request(&request) {
            // Catastrophic (secret-read) classification, but harmless to execute: an
            // absolute path that cannot exist, so no real key is ever read even if the
            // workspace happens to contain an `id_rsa`. Preserves the `id_rsa` token so
            // the catastrophic secret-access classifier still fires.
            RuntimeOutput::ActionRequest {
                request: ActionRequest {
                    schema_version: 1,
                    action_id: new_id(),
                    step_id: request.step_id.clone(),
                    kind: ActionKind::RunCommand,
                    params: serde_json::json!({ "command": "cat /nonexistent/id_rsa" }),
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
    let prompt = fake_control_text(request);
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
            crate::orchestrator::RunStepResult::Dag { .. } => "dag",
        });
    let prompt = fake_control_text(request);
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
            clarifying_options: Vec::new(),
            recommended_option_id: None,
            multi_select: false,
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
        // Drives the governance early-abort gate: a first-turn single-agent run
        // that intends to write (Edit), so the gate (when flagged on) pauses
        // before any action runs.
        None if prompt.contains("governance early abort") => (
            DecisionStatus::Continue,
            Some("fixer".to_string()),
            vec![Capability::Read, Capability::Edit, Capability::Verify],
            "Apply the requested scoped edit directly on the first turn.".to_string(),
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
    let (clarifying_question, clarifying_options, recommended_option_id) =
        if matches!(status, DecisionStatus::WaitingForUser) {
            (
                Some("Which target or constraint should guide this run?".to_string()),
                vec![
                    ClarificationOption {
                        id: "target_scope".to_string(),
                        label: "Clarify the target scope".to_string(),
                        description: Some(
                            "Specify the file, workflow, or product area to prioritize."
                                .to_string(),
                        ),
                    },
                    ClarificationOption {
                        id: "success_criteria".to_string(),
                        label: "Clarify success criteria".to_string(),
                        description: Some(
                            "Describe the outcome that should make the run complete.".to_string(),
                        ),
                    },
                    ClarificationOption {
                        id: "constraints".to_string(),
                        label: "Clarify constraints".to_string(),
                        description: Some(
                            "Call out limits, dependencies, or risky areas.".to_string(),
                        ),
                    },
                ],
                Some("target_scope".to_string()),
            )
        } else {
            (None, Vec::new(), None)
        };
    // A clarification turns multi-select when the prompt opts in, so end-to-end
    // tests and manual runs can exercise the multi-select composer path.
    let multi_select =
        matches!(status, DecisionStatus::WaitingForUser) && prompt.contains("multi-select");

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
        clarifying_options,
        recommended_option_id,
        multi_select,
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
        && fake_control_text(request).contains("use action")
}

fn should_emit_fake_approval_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Command)
        && fake_control_text(request).contains("approval action")
}

fn should_emit_fake_command_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Command)
        && fake_control_text(request).contains("command action")
}

fn should_emit_fake_trusted_command_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Command)
        && fake_control_text(request).contains("trusted run action")
}

fn should_emit_fake_catastrophic_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Command)
        && fake_control_text(request).contains("secret read action")
}

fn should_emit_fake_write_action_request(request: &RuntimeRequest) -> bool {
    request.action_results.is_empty()
        && request.agent_profile.id == "fixer"
        && request.agent_profile.has_capability(&Capability::Edit)
        && fake_control_text(request).contains("write action")
}

fn fake_write_action_path(request: &RuntimeRequest) -> String {
    if fake_control_text(request).contains("scoped write action") {
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
    let prompt = fake_control_text(request);
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
    if prompt.contains("child parse error")
        && request.parallel_context.as_ref().is_some_and(|context| {
            context
                .file_scope
                .write_files
                .iter()
                .any(|path| path == "parallel-output/fixer-b.txt")
        })
    {
        return true;
    }

    let agent_id = request.agent_profile.id.as_str();
    let requested_once = (agent_id == "orchestrator"
        && prompt.contains("orchestrator parse error"))
        || (agent_id == "explorer" && prompt.contains("agent parse error"));
    requested_once && !has_prior_parse_error(request, agent_id)
}

fn should_emit_fake_retryable_provider_error(request: &RuntimeRequest) -> bool {
    let control = fake_control_text(request);
    control.contains("retryable provider error")
        // "non-retryable provider error" contains "retryable provider error" as
        // a substring; keep the two control phrases disjoint.
        && !control.contains("non-retryable provider error")
        && request.agent_profile.model == "primary-fails"
}

fn should_emit_fake_non_retryable_provider_error(request: &RuntimeRequest) -> bool {
    fake_control_text(request).contains("non-retryable provider error")
}

fn fake_control_text(request: &RuntimeRequest) -> String {
    fake_control_prompt(request).to_ascii_lowercase()
}

fn fake_control_prompt(request: &RuntimeRequest) -> Cow<'_, str> {
    const SYSTEM_PROMPT_OPEN: &str = "<System Prompt>\n";
    const USER_PROMPT_OPEN: &str = "<User Prompt>\n";
    const USER_PROMPT_CLOSE: &str = "\n</User Prompt>";
    const WORKFLOW_USER_PROMPT_OPEN: &str = "\nExtracted user prompt:\n";

    let prompt = request.prompt.as_str();
    if let Some(body_start) = prompt
        .find(WORKFLOW_USER_PROMPT_OPEN)
        .map(|start| start + WORKFLOW_USER_PROMPT_OPEN.len())
    {
        return Cow::Borrowed(&prompt[body_start..]);
    }
    if !prompt.starts_with(SYSTEM_PROMPT_OPEN) {
        return Cow::Borrowed(prompt);
    }
    let Some(body_start) = prompt
        .find(USER_PROMPT_OPEN)
        .map(|start| start + USER_PROMPT_OPEN.len())
    else {
        return Cow::Borrowed(prompt);
    };
    let Some(body_end) = prompt[body_start..]
        .find(USER_PROMPT_CLOSE)
        .map(|end| body_start + end)
    else {
        return Cow::Borrowed(prompt);
    };
    Cow::Borrowed(&prompt[body_start..body_end])
}

fn has_prior_parse_error(request: &RuntimeRequest, agent_id: &str) -> bool {
    agent_results(&request.previous_results).any(|result| {
        result.agent == agent_id && matches!(result.status, AgentResultStatus::ParseError)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentEffort, AgentPromptMetadata, Limits};
    use std::path::PathBuf;

    fn orchestrator_request(prompt: &str) -> RuntimeRequest {
        RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: prompt.to_string(),
            session_goal: None,
            working_directory: PathBuf::from("/tmp/project"),
            agent_profile: crate::config::AgentProfile {
                id: "orchestrator".to_string(),
                name: "Orchestrator".to_string(),
                runtime: "fake".to_string(),
                model: "default".to_string(),
                model_fallbacks: Vec::new(),
                effort: AgentEffort::Medium,
                thinking: false,
                capabilities: Vec::new(),
                tools: None,
                instructions: String::new(),
                orchestrator_description: None,
                prompt_metadata: AgentPromptMetadata::default(),
                enabled: true,
            },
            session_events: Vec::new(),
            recent_context: crate::runtime::RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "orchestrator_decision".to_string(),
            parallel_context: None,
            capability_constraints: Vec::new(),
            limits: Limits::default(),
        }
    }

    fn assert_scoped_parallel_write_plan(prompt: &str) {
        let request = orchestrator_request(prompt);
        let decision = fake_decision(&request);
        let Some(DecisionNextStep::ParallelGroup(group)) = decision.next_step else {
            panic!("expected fake runtime to select a parallel group");
        };

        assert_eq!(group.steps.len(), 2);
        assert_eq!(
            group.steps[0].file_scope.write_files,
            ["parallel-output/fixer-a.txt"]
        );
        assert_eq!(
            group.steps[1].file_scope.write_files,
            ["parallel-output/fixer-b.txt"]
        );
    }

    #[test]
    fn workflow_envelope_prompt_matching_recognizes_parallel_scoped_write_action() {
        let prompt = r#"Workflow mode instructions:
- Treat this as one normal app Run in workflow mode.
- Execute safe specialized-agent steps, using Parallel Step Groups only when file scopes are exact.

Original command:
/workflow parallel scoped write action create a feature

Extracted user prompt:
parallel scoped write action create a feature"#;

        assert_scoped_parallel_write_plan(prompt);
    }

    #[test]
    fn non_workflow_prompt_matching_still_recognizes_parallel_scoped_write_action() {
        assert_scoped_parallel_write_plan("parallel scoped write action create a feature");
    }

    #[test]
    fn needs_clarification_decision_has_deterministic_options() {
        let request = orchestrator_request("needs clarification create a feature");

        let decision = fake_decision(&request);

        assert_eq!(decision.status, DecisionStatus::WaitingForUser);
        assert_eq!(
            decision.clarifying_question.as_deref(),
            Some("Which target or constraint should guide this run?")
        );
        assert_eq!(
            decision
                .clarifying_options
                .iter()
                .map(|option| (option.id.as_str(), option.label.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("target_scope", "Clarify the target scope"),
                ("success_criteria", "Clarify success criteria"),
                ("constraints", "Clarify constraints"),
            ]
        );
        assert_eq!(
            decision.recommended_option_id.as_deref(),
            Some("target_scope")
        );
    }
}
