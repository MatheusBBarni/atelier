use super::{
    prompt_envelope_json, Runtime, RuntimeAvailability, RuntimeAvailabilityStatus,
    RuntimeEventSink, RuntimeOutput, RuntimeRequest,
};
use crate::config::RuntimeConfig;
use crate::orchestrator::{parse_agent_result, parse_contract, parse_orchestrator_decision};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct CodexRuntime {
    config: RuntimeConfig,
}

impl CodexRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }

    async fn check_availability_with_exec_env_auth_vars(
        &self,
        exec_env_auth_vars: &[&str],
    ) -> RuntimeAvailability {
        let Some(command) = &self.config.command else {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: "codex command is not configured".to_string(),
                remediation: Some("Set [runtimes.codex].command in multiagent.toml.".to_string()),
            };
        };

        if resolve_command(command).is_none() {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: format!("codex command was not found: {command}"),
                remediation: Some("Install Codex or set [runtimes.codex].command.".to_string()),
            };
        }

        let version = timeout(
            Duration::from_secs(2),
            Command::new(command).arg("--version").output(),
        )
        .await;
        match version {
            Ok(Ok(output)) if output.status.success() => {
                let version_message = concise_process_output(&process_output_text(&output));
                let login = timeout(
                    Duration::from_secs(2),
                    Command::new(command).args(["login", "status"]).output(),
                )
                .await;

                match login {
                    Ok(Ok(login_output)) if login_output.status.success() => {
                        let login_message =
                            concise_process_output(&process_output_text(&login_output));
                        RuntimeAvailability {
                            runtime_id: self.config.id.clone(),
                            status: RuntimeAvailabilityStatus::Available,
                            message: join_status_parts(&version_message, &login_message),
                            remediation: None,
                        }
                    }
                    Ok(Ok(login_output)) => {
                        let login_message =
                            concise_process_output(&process_output_text(&login_output));
                        let (status, message, remediation) = if !exec_env_auth_vars.is_empty() {
                            (
                                RuntimeAvailabilityStatus::Unknown,
                                join_status_parts(
                                    &join_status_parts(&version_message, &login_message),
                                    &codex_exec_env_auth_message(exec_env_auth_vars),
                                ),
                                Some(codex_exec_env_auth_remediation(exec_env_auth_vars)),
                            )
                        } else if codex_login_status_is_known_auth_failure(&login_message) {
                            (
                                RuntimeAvailabilityStatus::Unavailable,
                                join_status_parts(&version_message, &login_message),
                                Some(codex_login_remediation()),
                            )
                        } else {
                            (
                                RuntimeAvailabilityStatus::Unknown,
                                join_status_parts(&version_message, &login_message),
                                Some(codex_login_remediation()),
                            )
                        };
                        RuntimeAvailability {
                            runtime_id: self.config.id.clone(),
                            status,
                            message,
                            remediation,
                        }
                    }
                    Ok(Err(error)) => RuntimeAvailability {
                        runtime_id: self.config.id.clone(),
                        status: RuntimeAvailabilityStatus::Unknown,
                        message: join_status_parts(&version_message, &error.to_string()),
                        remediation: Some(codex_login_remediation()),
                    },
                    Err(_) => RuntimeAvailability {
                        runtime_id: self.config.id.clone(),
                        status: RuntimeAvailabilityStatus::Unknown,
                        message: join_status_parts(
                            &version_message,
                            "codex login status timed out",
                        ),
                        remediation: Some(codex_login_remediation()),
                    },
                }
            }
            Ok(Ok(output)) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: concise_process_output(&process_output_text(&output)),
                remediation: Some(
                    "Run codex directly to inspect local authentication/setup.".to_string(),
                ),
            },
            Ok(Err(error)) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: error.to_string(),
                remediation: Some(
                    "Run codex --version directly to inspect the failure.".to_string(),
                ),
            },
            Err(_) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: "codex --version timed out".to_string(),
                remediation: Some(
                    "Run codex --version directly to inspect local setup.".to_string(),
                ),
            },
        }
    }
}

#[async_trait]
impl Runtime for CodexRuntime {
    async fn check_availability(&self) -> RuntimeAvailability {
        let exec_env_auth_vars = codex_exec_env_auth_vars();
        self.check_availability_with_exec_env_auth_vars(&exec_env_auth_vars)
            .await
    }

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput> {
        let command = self
            .config
            .command
            .as_ref()
            .context("codex command is not configured")?;
        let prompt = codex_prompt_text(&request)?;
        let args = codex_step_args(&self.config.args, &request.agent_profile.model);
        let mut child = Command::new(command)
            .args(&args)
            .current_dir(&request.working_directory)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn codex runtime command {command}"))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt envelope to codex stdin")?;
        }
        drop(child.stdin.take());

        let stdout = child
            .stdout
            .take()
            .context("failed to capture codex stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture codex stderr")?;
        let stdout_events = events.clone();
        let stderr_events = events.clone();
        let stdout_reader = tokio::spawn(async move {
            read_process_stream(stdout, stdout_events, "stdout", false).await
        });
        let stderr_reader = tokio::spawn(async move {
            read_process_stream(stderr, stderr_events, "stderr", true).await
        });

        let status = tokio::select! {
            _ = cancellation.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                anyhow::bail!("codex runtime cancelled");
            }
            status = timeout(Duration::from_secs(600), child.wait()) => {
                status
                    .context("codex runtime timed out")?
                    .context("failed to wait for codex runtime")?
            }
        };
        let stdout = stdout_reader
            .await
            .context("codex stdout reader task failed")??;
        let stderr = stderr_reader
            .await
            .context("codex stderr reader task failed")??;
        let stdout = String::from_utf8_lossy(&stdout);
        let stderr = String::from_utf8_lossy(&stderr);
        let combined = if stderr.trim().is_empty() {
            stdout.to_string()
        } else {
            format!("{stdout}\n{stderr}")
        };
        if !status.success() {
            bail!(
                "codex runtime exited with {}: {}",
                status
                    .code()
                    .map(|code| format!("status {code}"))
                    .unwrap_or_else(|| "signal".to_string()),
                concise_process_output(&combined)
            );
        }

        let stdout = stdout.to_string();
        parse_runtime_output(&request.agent_profile.id, stdout)
    }
}

async fn read_process_stream<R>(
    mut reader: R,
    events: RuntimeEventSink,
    stream: &'static str,
    diagnostic: bool,
) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .with_context(|| format!("failed to read codex {stream}"))?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read]);
        let chunk = String::from_utf8_lossy(&buffer[..read]).to_string();
        if diagnostic {
            events.diagnostic(stream, chunk).await?;
        } else {
            events.delta(stream, chunk).await?;
        }
    }
    Ok(output)
}

fn codex_prompt_text(request: &RuntimeRequest) -> Result<String> {
    let envelope = prompt_envelope_json(request)?;
    Ok(format!(
        r#"You are running inside atelier as a structured runtime adapter, not as a standalone coding agent.

Follow this protocol exactly:
- Use the JSON envelope below as the only task input.
- Do not solve the user's prompt directly in prose.
- Do not edit files, run commands, inspect the repository, or use Codex tools directly.
- If you need repository data or a file/command/edit operation, return one action_request JSON contract. The harness will execute it and call you again with action_results.
- If you can complete your current step, return one {output_schema} JSON contract.
- Return no Markdown, no commentary, and no text outside the contract delimiters.

Contract delimiters:
{json_start}
{{ ...one JSON object... }}
{json_end}

When output_schema is orchestrator_decision, return:
{{
  "schema_version": 1,
  "decision_id": "stable-id-for-this-decision",
  "run_id": "{run_id}",
  "status": "continue|waiting_for_user|complete|failed",
  "plan": ["short step"],
  "next_agent": "enabled specialized agent id from the envelope or null",
  "reason": "why this decision is correct",
  "required_capabilities": ["read"],
  "stop_condition": "what should be true after the next step",
  "clarifying_question": null,
  "final_summary": null
}}

When output_schema is agent_result, return:
{{
  "schema_version": 1,
  "agent": "{agent_id}",
  "step_id": "{step_id}",
  "status": "completed|blocked|failed|cancelled|parse_error|limit_reached|approval_denied|no_changes",
  "summary": "brief result",
  "findings": [],
  "changed_files": [],
  "commands": [],
  "verification": [],
  "blocker": null,
  "artifacts": []
}}

When an action is needed instead, return:
{{
  "schema_version": 1,
  "action_id": "stable-id-for-this-action",
  "step_id": "{step_id}",
  "kind": "read_file|list_files|search_text|run_command|apply_patch|write_file|record_note",
  "params": {{}}
}}

Envelope JSON:
```json
{envelope}
```
"#,
        output_schema = request.output_schema,
        json_start = crate::orchestrator::JSON_START,
        json_end = crate::orchestrator::JSON_END,
        run_id = request.run_id,
        agent_id = request.agent_profile.id,
        step_id = request.step_id,
        envelope = envelope,
    ))
}

fn codex_step_args(config_args: &[String], model: &str) -> Vec<String> {
    let mut args = if config_args.is_empty() {
        vec![
            "exec".to_string(),
            "--skip-git-repo-check".to_string(),
            "--color".to_string(),
            "never".to_string(),
        ]
    } else {
        config_args.to_vec()
    };

    if !args.iter().any(|arg| arg == "exec" || arg == "e") {
        args.insert(0, "exec".to_string());
    }
    if !args.iter().any(|arg| arg == "--skip-git-repo-check") {
        args.push("--skip-git-repo-check".to_string());
    }
    if !args.iter().any(|arg| arg == "--color") {
        args.push("--color".to_string());
        args.push("never".to_string());
    }
    if model != "default" && !args_have_model(&args) {
        args.push("--model".to_string());
        args.push(model.to_string());
    }

    args
}

fn args_have_model(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--model" || arg == "-m" || arg.starts_with("--model="))
}

fn concise_process_output(output: &str) -> String {
    let output = output.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_OUTPUT_CHARS: usize = 240;
    if output.chars().count() <= MAX_OUTPUT_CHARS {
        return output;
    }
    format!(
        "{}...",
        output
            .chars()
            .take(MAX_OUTPUT_CHARS.saturating_sub(3))
            .collect::<String>()
    )
}

fn process_output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match (stdout.trim(), stderr.trim()) {
        ("", "") => "command produced no output".to_string(),
        (stdout, "") => stdout.to_string(),
        ("", stderr) => stderr.to_string(),
        (stdout, stderr) => format!("{stdout}; {stderr}"),
    }
}

fn join_status_parts(left: &str, right: &str) -> String {
    match (left.trim(), right.trim()) {
        ("", "") => "codex status unknown".to_string(),
        (left, "") => left.to_string(),
        ("", right) => right.to_string(),
        (left, right) => format!("{left}; {right}"),
    }
}

fn codex_login_remediation() -> String {
    "Run codex login, or codex login --device-auth on a remote/headless machine.".to_string()
}

fn codex_exec_env_auth_vars() -> Vec<&'static str> {
    ["CODEX_API_KEY", "CODEX_ACCESS_TOKEN"]
        .into_iter()
        .filter(|name| {
            std::env::var_os(name)
                .map(|value| !value.to_string_lossy().trim().is_empty())
                .unwrap_or(false)
        })
        .collect()
}

fn codex_exec_env_auth_message(vars: &[&str]) -> String {
    if vars.len() == 1 {
        format!("{} is set for codex exec", vars[0])
    } else {
        format!("{} are set for codex exec", vars.join(", "))
    }
}

fn codex_exec_env_auth_remediation(vars: &[&str]) -> String {
    format!(
        "Run codex exec directly to verify {}; use codex login for persistent auth.",
        vars.join(" or ")
    )
}

fn codex_login_status_is_known_auth_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("not logged in")
        || message.contains("not authenticated")
        || message.contains("logged out")
        || message.contains("no valid session")
        || message.contains("authentication required")
        || message.contains("unauthorized")
        || message.contains("forbidden")
}

fn parse_runtime_output(agent_id: &str, raw_output: String) -> Result<RuntimeOutput> {
    if let Ok(request) = parse_contract(&raw_output) {
        return Ok(RuntimeOutput::ActionRequest { request });
    }

    if agent_id == "orchestrator" {
        match parse_orchestrator_decision(&raw_output) {
            Ok(decision) => Ok(RuntimeOutput::OrchestratorDecision { decision }),
            Err(error) => Ok(RuntimeOutput::ParseError {
                agent: agent_id.to_string(),
                raw_output,
                diagnostic: error.to_string(),
            }),
        }
    } else {
        match parse_agent_result(&raw_output) {
            Ok(result) => Ok(RuntimeOutput::AgentResult { result }),
            Err(error) => Ok(RuntimeOutput::ParseError {
                agent: agent_id.to_string(),
                raw_output,
                diagnostic: error.to_string(),
            }),
        }
    }
}

fn resolve_command(command: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists().then(|| path.to_path_buf());
    }
    which::which(command).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentEffort, AgentProfile, AgentPromptMetadata, Capability, Limits, PromptMode, RuntimeKind,
    };
    use crate::orchestrator::{
        wrap_json_contract, AgentResult, DecisionStatus, OrchestratorDecision,
    };
    use crate::runtime::{collect_runtime_step_result, RuntimeEvent, CODEX_ENV_MUTEX};
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn codex_adapter_executes_fake_process_and_parses_agent_result() {
        let dir = tempdir().unwrap();
        let capture_path = dir.path().join("prompt.json");
        let capture_args_path = dir.path().join("args.txt");
        let result = AgentResult::completed("explorer", "step", "fake process completed");
        let wrapped = wrap_json_contract(&result).unwrap();
        let script_path = dir.path().join("fake-codex.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CAPTURE_ARGS_PATH\"\ncat > \"$CAPTURE_PATH\"\ncat <<'JSON'\n{wrapped}\nJSON\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::env::set_var("CAPTURE_PATH", &capture_path);
        std::env::set_var("CAPTURE_ARGS_PATH", &capture_args_path);

        let runtime = CodexRuntime::new(RuntimeConfig {
            id: "codex".to_string(),
            kind: RuntimeKind::Codex,
            command: Some(script_path.display().to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        });
        let request = runtime_request(dir.path().to_path_buf(), "explorer");
        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();
        assert!(!result.stream_deltas.is_empty());

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.agent, "explorer");
                assert_eq!(result.summary, "fake process completed");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }

        let captured = fs::read_to_string(capture_path).unwrap();
        let captured_args = fs::read_to_string(capture_args_path).unwrap();
        assert!(captured.contains("structured runtime adapter"));
        assert!(captured.contains("Do not edit files, run commands"));
        assert!(captured.contains("Return no Markdown"));
        assert!(captured.contains("\"output_schema\": \"agent_result\""));
        assert!(captured.contains("\"id\": \"explorer\""));
        assert!(captured.contains("\"model\": \"gpt-5.4\""));
        assert!(captured_args.contains("exec\n"));
        assert!(captured_args.contains("--skip-git-repo-check\n"));
        assert!(captured_args.contains("--color\nnever\n"));
        assert!(captured_args.contains("--model\ngpt-5.4\n"));
    }

    #[tokio::test]
    async fn codex_availability_reports_login_status() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("codex-status.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.137.0"
  exit 0
fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  echo "Logged in using ChatGPT"
  exit 0
fi
echo "unexpected args: $@" >&2
exit 64
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let runtime = CodexRuntime::new(codex_runtime_config(&script_path));
        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Available);
        assert!(availability.message.contains("codex-cli 0.137.0"));
        assert!(availability.message.contains("Logged in using ChatGPT"));
        assert!(availability.remediation.is_none());
    }

    #[tokio::test]
    async fn codex_availability_is_unavailable_when_login_status_fails() {
        let _env_lock = CODEX_ENV_MUTEX.lock().await;
        let _env_guard = clear_codex_exec_env_auth();
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("codex-status.sh");
        fs::write(
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
echo "unexpected args: $@" >&2
exit 64
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let runtime = CodexRuntime::new(codex_runtime_config(&script_path));
        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unavailable);
        assert!(availability.message.contains("codex-cli 0.137.0"));
        assert!(availability.message.contains("Not logged in"));
        assert!(availability
            .remediation
            .as_deref()
            .unwrap()
            .contains("codex login"));
    }

    #[tokio::test]
    async fn codex_availability_is_unknown_when_exec_env_auth_is_present() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("codex-status.sh");
        fs::write(
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
echo "unexpected args: $@" >&2
exit 64
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let runtime = CodexRuntime::new(codex_runtime_config(&script_path));
        let availability = runtime
            .check_availability_with_exec_env_auth_vars(&["CODEX_API_KEY"])
            .await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unknown);
        assert!(availability.message.contains("codex-cli 0.137.0"));
        assert!(availability.message.contains("Not logged in"));
        assert!(availability.message.contains("CODEX_API_KEY"));
        assert!(availability
            .remediation
            .as_deref()
            .unwrap()
            .contains("codex exec"));
    }

    #[tokio::test]
    async fn codex_availability_is_unknown_when_login_status_is_unsupported() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("codex-status.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.1.0"
  exit 0
fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  echo "unknown login subcommand: status" >&2
  exit 64
fi
echo "unexpected args: $@" >&2
exit 65
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let runtime = CodexRuntime::new(codex_runtime_config(&script_path));
        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unknown);
        assert!(availability.message.contains("codex-cli 0.1.0"));
        assert!(availability
            .message
            .contains("unknown login subcommand: status"));
        assert!(availability
            .remediation
            .as_deref()
            .unwrap()
            .contains("codex login"));
    }

    #[test]
    fn codex_step_args_preserve_explicit_model() {
        let args = codex_step_args(
            &[
                "exec".to_string(),
                "--model".to_string(),
                "custom-model".to_string(),
            ],
            "gpt-5.4",
        );

        assert_eq!(
            args.iter().filter(|arg| arg.as_str() == "--model").count(),
            1
        );
        assert!(args.contains(&"custom-model".to_string()));
    }

    #[test]
    fn codex_prompt_text_wraps_envelope_in_runtime_protocol() {
        let request = runtime_request(std::path::PathBuf::from("/tmp/project"), "orchestrator");
        let prompt = codex_prompt_text(&request).unwrap();

        assert!(prompt.contains("not as a standalone coding agent"));
        assert!(prompt.contains("return one orchestrator_decision JSON contract"));
        assert!(prompt.contains(crate::orchestrator::JSON_START));
        assert!(prompt.contains("\"agent\""));
        assert!(prompt.contains("\"id\": \"orchestrator\""));
    }

    #[tokio::test]
    async fn codex_nonzero_exit_is_runtime_error() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("failing-codex.sh");
        fs::write(&script_path, "#!/bin/sh\necho 'auth failed' >&2\nexit 42\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let runtime = CodexRuntime::new(RuntimeConfig {
            id: "codex".to_string(),
            kind: RuntimeKind::Codex,
            command: Some(script_path.display().to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        });

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer"),
                RuntimeEventSink::channel(1).0,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("status 42"));
        assert!(error.to_string().contains("auth failed"));
    }

    #[test]
    fn codex_malformed_output_becomes_parse_error() {
        let output = parse_runtime_output("explorer", "plain prose".to_string()).unwrap();
        match output {
            RuntimeOutput::ParseError {
                agent,
                raw_output,
                diagnostic,
            } => {
                assert_eq!(agent, "explorer");
                assert_eq!(raw_output, "plain prose");
                assert!(diagnostic.contains("missing JSON contract"));
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[test]
    fn codex_output_accepts_missing_angle_contract_delimiters() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Collect project context.".to_string()],
            next_agent: Some("explorer".to_string()),
            reason: "The orchestrator delegates read work.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Explorer returns context.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };
        let raw_output = format!(
            "<<<MULTIAGENT_JSON_START>>\n{}\n<<<MULTIAGENT_JSON_END>>",
            serde_json::to_string_pretty(&decision).unwrap()
        );

        match parse_runtime_output("orchestrator", raw_output).unwrap() {
            RuntimeOutput::OrchestratorDecision { decision: parsed } => {
                assert_eq!(parsed, decision);
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[tokio::test]
    async fn codex_adapter_parses_stdout_not_progress_stderr() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("fake-codex.sh");
        let result = AgentResult::completed("explorer", "step", "stdout contract parsed");
        let wrapped = wrap_json_contract(&result).unwrap();
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ncat >/dev/null\necho 'progress with invalid contract {start}{{}}{end}' >&2\ncat <<'JSON'\n{wrapped}\nJSON\n",
                start = crate::orchestrator::JSON_START,
                end = crate::orchestrator::JSON_END
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let runtime = CodexRuntime::new(RuntimeConfig {
            id: "codex".to_string(),
            kind: RuntimeKind::Codex,
            command: Some(script_path.display().to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        });

        let request = runtime_request(dir.path().to_path_buf(), "explorer");
        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();
        assert!(result
            .stream_deltas
            .iter()
            .any(|delta| delta.stream == "stderr"
                && delta.content.contains("progress with invalid contract")));
        assert!(result
            .stream_deltas
            .iter()
            .any(|delta| delta.stream == "stdout"
                && delta.content.contains("stdout contract parsed")));

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "stdout contract parsed");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[tokio::test]
    async fn codex_adapter_emits_stdout_before_process_exit() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("streaming-codex.sh");
        let result = AgentResult::completed("explorer", "step", "streamed stdout parsed");
        let wrapped = wrap_json_contract(&result).unwrap();
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ncat >/dev/null &\n/bin/echo early stdout\nsleep 1\ncat <<'JSON'\n{wrapped}\nJSON\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let runtime = CodexRuntime::new(RuntimeConfig {
            id: "codex".to_string(),
            kind: RuntimeKind::Codex,
            command: Some(script_path.display().to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        });
        let request = runtime_request(dir.path().to_path_buf(), "explorer");
        let (events, mut receiver) = RuntimeEventSink::channel(8);
        let cancellation = CancellationToken::new();
        let runtime_step =
            tokio::spawn(async move { runtime.stream_step(request, events, cancellation).await });

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(
            event,
            RuntimeEvent::Delta {
                stream,
                content,
                ..
            } if stream == "stdout" && content.contains("early stdout")
        ));
        assert!(!runtime_step.is_finished());

        match runtime_step.await.unwrap().unwrap() {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "streamed stdout parsed");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    fn runtime_request(working_directory: std::path::PathBuf, agent_id: &str) -> RuntimeRequest {
        RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "inspect context".to_string(),
            session_goal: None,
            working_directory,
            agent_profile: AgentProfile {
                id: agent_id.to_string(),
                name: "Explorer".to_string(),
                runtime: "codex".to_string(),
                model: "gpt-5.4".to_string(),
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
            session_events: Vec::new(),
            recent_context: crate::runtime::RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: if agent_id == "orchestrator" {
                "orchestrator_decision"
            } else {
                "agent_result"
            }
            .to_string(),
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        }
    }

    fn codex_runtime_config(command: &std::path::Path) -> RuntimeConfig {
        RuntimeConfig {
            id: "codex".to_string(),
            kind: RuntimeKind::Codex,
            command: Some(command.display().to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        }
    }

    fn clear_codex_exec_env_auth() -> EnvGuard {
        EnvGuard::clear(&["CODEX_API_KEY", "CODEX_ACCESS_TOKEN"])
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
