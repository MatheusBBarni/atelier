use super::{
    concise_runtime_text, concise_runtime_text_with_limit, parse_runtime_output,
    process_output_text, prompt_envelope_json, Runtime, RuntimeAvailability,
    RuntimeAvailabilityStatus, RuntimeEventSink, RuntimeOutput, RuntimeProviderError,
    RuntimeRequest,
};
use crate::config::{validate_claude_runtime_args, RuntimeConfig};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

pub(crate) const REQUIRED_HELP_FLAGS: &[&str] = &[
    "--output-format",
    "--include-partial-messages",
    "--no-session-persistence",
    "--tools",
    "--setting-sources",
];

const PROTECTED_DEFAULT_ARGS: &[&str] = &[
    "-p",
    "--output-format",
    "stream-json",
    "--include-partial-messages",
    "--no-session-persistence",
    "--tools",
    "",
    "--setting-sources",
    "user",
];

const DEFAULT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(600);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct ClaudeRuntime {
    config: RuntimeConfig,
    runtime_timeout: Duration,
}

impl ClaudeRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            config,
            runtime_timeout: DEFAULT_RUNTIME_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_runtime_timeout(mut self, runtime_timeout: Duration) -> Self {
        self.runtime_timeout = runtime_timeout;
        self
    }
}

#[async_trait]
impl Runtime for ClaudeRuntime {
    async fn check_availability(&self) -> RuntimeAvailability {
        let Some(command) = &self.config.command else {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: "claude command is not configured".to_string(),
                remediation: Some("Set [runtimes.claude].command in multiagent.toml.".to_string()),
            };
        };

        if resolve_command(command).is_none() {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: format!("claude command was not found: {command}"),
                remediation: Some(
                    "Install Claude Code or set [runtimes.claude].command.".to_string(),
                ),
            };
        }

        let version = timeout(
            PROBE_TIMEOUT,
            Command::new(command).arg("--version").output(),
        )
        .await;
        let version_message = match version {
            Ok(Ok(output)) if output.status.success() => {
                concise_runtime_text(&process_output_text(&output))
            }
            Ok(Ok(output)) => {
                return RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Unavailable,
                    message: concise_runtime_text(&process_output_text(&output)),
                    remediation: Some(
                        "Run claude --version directly to inspect local setup.".to_string(),
                    ),
                };
            }
            Ok(Err(error)) => {
                return RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Unavailable,
                    message: error.to_string(),
                    remediation: Some(
                        "Run claude --version directly to inspect local setup.".to_string(),
                    ),
                };
            }
            Err(_) => {
                return RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Unknown,
                    message: "claude --version timed out".to_string(),
                    remediation: Some(
                        "Run claude --version directly to inspect local setup.".to_string(),
                    ),
                };
            }
        };

        self.check_help(command, &version_message).await
    }

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput> {
        validate_claude_runtime_args(&self.config.id, &self.config.args)?;
        let command = self
            .config
            .command
            .as_ref()
            .context("claude command is not configured")?;
        let prompt = claude_prompt_text(&request)?;
        let args = claude_step_args(&self.config.args, &request.agent_profile.model);
        validate_synthesized_args(&args)?;

        let mut child = Command::new(command)
            .args(&args)
            .current_dir(&request.working_directory)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn claude runtime command {command}"))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt envelope to claude stdin")?;
        }
        drop(child.stdin.take());

        let stdout = child
            .stdout
            .take()
            .context("failed to capture claude stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture claude stderr")?;
        let mut stdout_reader = tokio::spawn(read_claude_stdout(stdout, events.clone()));
        let stderr_reader = tokio::spawn(read_diagnostic_stream(stderr, events.clone(), "stderr"));

        let mut stdout_state = None;
        let status_result = tokio::select! {
            output = &mut stdout_reader => {
                match output.context("claude stdout reader task failed")? {
                    Ok(state) => {
                        stdout_state = Some(state);
                        wait_for_child_or_cancel(&mut child, &cancellation, self.runtime_timeout).await
                    }
                    Err(error) => {
                        kill_child(&mut child).await;
                        let _ = stderr_reader.await;
                        let _ = events
                            .status(claude_failure_status(
                                "Claude stream invalidated attempt",
                                &error,
                            ))
                            .await;
                        return Err(error);
                    }
                }
            }
            status = wait_for_child_or_cancel(&mut child, &cancellation, self.runtime_timeout) => status,
        };
        let status = match status_result {
            Ok(status) => status,
            Err(error) => {
                kill_child(&mut child).await;
                await_reader_shutdown(stdout_reader, stderr_reader).await;
                let _ = events
                    .status(claude_failure_status(
                        "Claude runtime attempt failed",
                        &error,
                    ))
                    .await;
                return Err(error);
            }
        };

        let stdout_state = match stdout_state {
            Some(state) => state,
            None => match stdout_reader
                .await
                .context("claude stdout reader task failed")?
            {
                Ok(state) => state,
                Err(error) => {
                    let _ = stderr_reader.await;
                    let _ = events
                        .status(claude_failure_status(
                            "Claude stream invalidated attempt",
                            &error,
                        ))
                        .await;
                    return Err(error);
                }
            },
        };
        let stderr = stderr_reader
            .await
            .context("claude stderr reader task failed")??;

        if !status.success() {
            let error = nonzero_exit_error(status, &stderr);
            let _ = events
                .status(claude_failure_status(
                    "Claude runtime attempt failed",
                    &error,
                ))
                .await;
            return Err(error);
        }

        if let Some(summary) = stdout_state.metadata_summary() {
            events.status(summary).await?;
        }
        let final_text = match stdout_state.final_result_text() {
            Ok(final_text) => final_text,
            Err(error) => {
                let _ = events
                    .status(claude_failure_status(
                        "Claude stream invalidated attempt",
                        &error,
                    ))
                    .await;
                return Err(error);
            }
        };
        parse_runtime_output(&request.agent_profile.id, final_text)
    }
}

impl ClaudeRuntime {
    async fn check_help(&self, command: &str, version_message: &str) -> RuntimeAvailability {
        let help = timeout(PROBE_TIMEOUT, Command::new(command).arg("--help").output()).await;
        match help {
            Ok(Ok(output)) if output.status.success() => {
                let help_text = process_output_text(&output);
                let missing = REQUIRED_HELP_FLAGS
                    .iter()
                    .copied()
                    .filter(|flag| !help_text.contains(flag))
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    return RuntimeAvailability {
                        runtime_id: self.config.id.clone(),
                        status: RuntimeAvailabilityStatus::Unknown,
                        message: join_status_parts(
                            version_message,
                            &format!(
                                "claude --help did not list protected default flags: {}; help output may be incomplete; protected defaults: {}",
                                missing.join(", "),
                                protected_defaults_summary().join(", ")
                            ),
                        ),
                        remediation: Some(
                            "Run a normal agent step to verify Claude authentication and flag support; doctor does not run paid or interactive Claude probes.".to_string(),
                        ),
                    };
                }

                RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Unknown,
                    message: join_status_parts(
                        version_message,
                        &format!(
                            "required print-mode flags available; protected defaults: {}",
                            protected_defaults_summary().join(", ")
                        ),
                    ),
                    remediation: Some(
                        "Run a normal agent step to verify Claude authentication; doctor does not run paid or interactive Claude probes.".to_string(),
                    ),
                }
            }
            Ok(Ok(output)) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: join_status_parts(
                    version_message,
                    &format!(
                        "claude --help failed: {}",
                        concise_runtime_text(&process_output_text(&output))
                    ),
                ),
                remediation: Some("Run claude --help directly to inspect local setup.".to_string()),
            },
            Ok(Err(error)) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: join_status_parts(version_message, &error.to_string()),
                remediation: Some("Run claude --help directly to inspect local setup.".to_string()),
            },
            Err(_) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: join_status_parts(version_message, "claude --help timed out"),
                remediation: Some("Run claude --help directly to inspect local setup.".to_string()),
            },
        }
    }
}

pub(crate) fn protected_defaults_summary() -> Vec<&'static str> {
    vec![
        "tools disabled",
        "session persistence disabled",
        "stream-json enabled",
        "partial messages enabled",
        "project/local settings minimized",
    ]
}

fn claude_step_args(config_args: &[String], model: &str) -> Vec<String> {
    let mut args = PROTECTED_DEFAULT_ARGS
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    args.extend(config_args.iter().cloned());
    if model != "default" {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    args
}

fn validate_synthesized_args(args: &[String]) -> Result<()> {
    require_arg_value(args, "--output-format", "stream-json")?;
    require_arg(args, "--include-partial-messages")?;
    require_arg(args, "--no-session-persistence")?;
    require_arg_value(args, "--tools", "")?;
    require_arg_value(args, "--setting-sources", "user")?;
    validate_synthesized_model_args(args)?;
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--continue" | "-c" | "--resume" | "-r" | "--session-id" | "--fallback-model"
        )
    }) {
        bail!("claude synthesized args contain forbidden session or fallback flags");
    }
    Ok(())
}

fn validate_synthesized_model_args(args: &[String]) -> Result<()> {
    for (index, arg) in args.iter().enumerate() {
        if arg == "--model" {
            let Some(model) = args.get(index + 1) else {
                return Err(RuntimeProviderError::non_retryable(
                    "Claude synthesized args include --model without a model value",
                )
                .into());
            };
            if model.starts_with('-') {
                return Err(RuntimeProviderError::non_retryable(
                    "Claude model value cannot start with '-' because it may be parsed as a CLI flag",
                )
                .into());
            }
        }
    }
    Ok(())
}

fn require_arg(args: &[String], flag: &str) -> Result<()> {
    if args.iter().any(|arg| arg == flag) {
        Ok(())
    } else {
        bail!("claude synthesized args are missing protected flag {flag}");
    }
}

fn require_arg_value(args: &[String], flag: &str, value: &str) -> Result<()> {
    if args
        .windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
    {
        Ok(())
    } else {
        bail!("claude synthesized args are missing protected flag {flag} {value:?}");
    }
}

async fn wait_for_child_or_cancel(
    child: &mut Child,
    cancellation: &CancellationToken,
    runtime_timeout: Duration,
) -> Result<ExitStatus> {
    tokio::select! {
        _ = cancellation.cancelled() => {
            request_child_kill(child);
            bail!("Claude runtime cancelled");
        }
        _ = tokio::time::sleep(runtime_timeout) => {
            request_child_kill(child);
            Err(RuntimeProviderError::retryable("Claude runtime timed out").into())
        }
        status = child.wait() => {
            status
                .context("failed to wait for Claude runtime")
        }
    }
}

async fn kill_child(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn request_child_kill(child: &mut Child) {
    let _ = child.start_kill();
}

async fn await_reader_shutdown(
    stdout_reader: JoinHandle<Result<ClaudeStreamState>>,
    stderr_reader: JoinHandle<Result<String>>,
) {
    let _ = stdout_reader.await;
    let _ = stderr_reader.await;
}

async fn read_diagnostic_stream<R>(
    reader: R,
    events: RuntimeEventSink,
    stream: &'static str,
) -> Result<String>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut output = String::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .with_context(|| format!("failed to read claude {stream}"))?;
        if read == 0 {
            break;
        }
        output.push_str(&line);
        let diagnostic = concise_runtime_text_with_limit(&line, 1_000);
        if !diagnostic.trim().is_empty() {
            events.diagnostic(stream, diagnostic).await?;
        }
    }
    Ok(output)
}

async fn read_claude_stdout<R>(reader: R, events: RuntimeEventSink) -> Result<ClaudeStreamState>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut state = ClaudeStreamState::default();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .context("failed to read claude stdout")?;
        if read == 0 {
            break;
        }
        state.apply_line(&line, &events).await?;
    }
    Ok(state)
}

#[derive(Clone, Debug)]
struct ClaudeResultFrame {
    result_text: String,
    metadata: ClaudeMetadata,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ClaudeMetadata {
    session_id: Option<String>,
    model: Option<String>,
    duration_ms: Option<u64>,
    num_turns: Option<u64>,
    total_cost_usd: Option<f64>,
    subtype: Option<String>,
}

impl ClaudeMetadata {
    fn from_init_frame(value: &Value) -> Self {
        Self {
            session_id: string_field(value, "session_id"),
            model: string_field(value, "model").or_else(|| string_field(value, "model_name")),
            duration_ms: None,
            num_turns: None,
            total_cost_usd: None,
            subtype: None,
        }
    }

    fn from_result_frame(value: &Value, subtype: &str, init_metadata: &Self) -> Self {
        let mut metadata = Self {
            session_id: string_field(value, "session_id"),
            model: string_field(value, "model").or_else(|| string_field(value, "model_name")),
            duration_ms: u64_field(value, "duration_ms"),
            num_turns: u64_field(value, "num_turns").or_else(|| u64_field(value, "turn_count")),
            total_cost_usd: f64_field(value, "total_cost_usd"),
            subtype: Some(subtype.to_string()),
        };
        if metadata.session_id.is_none() {
            metadata.session_id.clone_from(&init_metadata.session_id);
        }
        if metadata.model.is_none() {
            metadata.model.clone_from(&init_metadata.model);
        }
        metadata
    }

    fn summary(&self) -> Option<String> {
        let mut fields = Vec::new();
        if let Some(session_id) = &self.session_id {
            fields.push(format!("session_id={session_id}"));
        }
        if let Some(model) = &self.model {
            fields.push(format!("model={model}"));
        }
        if let Some(duration_ms) = self.duration_ms {
            fields.push(format!("duration_ms={duration_ms}"));
        }
        if let Some(num_turns) = self.num_turns {
            fields.push(format!("num_turns={num_turns}"));
        }
        if let Some(total_cost_usd) = self.total_cost_usd {
            fields.push(format!("total_cost_usd={total_cost_usd}"));
        }
        if let Some(subtype) = &self.subtype {
            fields.push(format!("subtype={subtype}"));
        }
        (!fields.is_empty()).then(|| format!("Claude metadata: {}", fields.join(", ")))
    }
}

#[derive(Clone, Debug, Default)]
struct ClaudeStreamState {
    final_result: Option<ClaudeResultFrame>,
    init_metadata: ClaudeMetadata,
    ignored_optional_frames: usize,
}

impl ClaudeStreamState {
    async fn apply_line(&mut self, line: &str, events: &RuntimeEventSink) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let value: Value = serde_json::from_str(trimmed).map_err(|error| {
            RuntimeProviderError::non_retryable(format!(
                "malformed Claude stream JSONL: {}; frame: {}",
                error,
                concise_runtime_text(trimmed)
            ))
        })?;
        if frame_requests_local_action(&value) {
            return Err(RuntimeProviderError::non_retryable(
                "Claude stream requested tool use or local action execution; harness-action boundary violated",
            )
            .into());
        }
        let frame_type = value.get("type").and_then(Value::as_str).ok_or_else(|| {
            RuntimeProviderError::non_retryable(format!(
                "Claude stream frame missing type: {}",
                concise_runtime_text(trimmed)
            ))
        })?;

        match frame_type {
            "system" => self.apply_system_frame(&value)?,
            "assistant" => {
                if let Some(text) = assistant_text(&value) {
                    events.transient_delta("message", text).await?;
                }
            }
            "result" => self.apply_result_frame(&value, events).await?,
            "user" => {
                self.ignored_optional_frames += 1;
            }
            _ => {
                self.ignored_optional_frames += 1;
            }
        }
        Ok(())
    }

    fn apply_system_frame(&mut self, value: &Value) -> Result<()> {
        let subtype = value
            .get("subtype")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RuntimeProviderError::non_retryable(format!(
                    "Claude system frame missing subtype: {}",
                    concise_runtime_text(&value.to_string())
                ))
            })?;
        if subtype != "init" {
            self.ignored_optional_frames += 1;
        } else {
            self.init_metadata = ClaudeMetadata::from_init_frame(value);
        }
        Ok(())
    }

    async fn apply_result_frame(&mut self, value: &Value, events: &RuntimeEventSink) -> Result<()> {
        if self.final_result.is_some() {
            return Err(RuntimeProviderError::non_retryable(
                "Claude stream emitted multiple final result frames",
            )
            .into());
        }

        let subtype = value
            .get("subtype")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RuntimeProviderError::non_retryable(format!(
                    "Claude result frame missing subtype: {}",
                    concise_runtime_text(&value.to_string())
                ))
            })?;
        let is_error = value
            .get("is_error")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                RuntimeProviderError::non_retryable(format!(
                    "Claude result frame missing is_error: {}",
                    concise_runtime_text(&value.to_string())
                ))
            })?;
        let result_text = value
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_string);

        if is_error {
            let message = result_text.unwrap_or_else(|| concise_runtime_text(&value.to_string()));
            return Err(classified_claude_error(format!(
                "Claude result frame reported error ({subtype}): {}",
                concise_runtime_text(&message)
            ))
            .into());
        }
        if subtype.to_ascii_lowercase().contains("error") {
            return Err(RuntimeProviderError::non_retryable(format!(
                "Claude result frame has error subtype {subtype}"
            ))
            .into());
        }
        let Some(result_text) = result_text else {
            return Err(RuntimeProviderError::non_retryable(
                "Claude final result frame omitted result text",
            )
            .into());
        };
        if result_text.trim().is_empty() {
            return Err(RuntimeProviderError::non_retryable(
                "Claude final result frame contained empty result text",
            )
            .into());
        }

        if let Some(diagnostic) = metadata_mismatch_diagnostic(value, &self.init_metadata) {
            events.diagnostic("metadata", diagnostic).await?;
        }
        let metadata = ClaudeMetadata::from_result_frame(value, subtype, &self.init_metadata);
        self.final_result = Some(ClaudeResultFrame {
            result_text,
            metadata,
        });
        Ok(())
    }

    fn metadata_summary(&self) -> Option<String> {
        self.final_result
            .as_ref()
            .and_then(|frame| frame.metadata.summary())
    }

    fn final_result_text(self) -> Result<String> {
        self.final_result
            .map(|frame| frame.result_text)
            .ok_or_else(|| {
                RuntimeProviderError::non_retryable(
                    "Claude stream ended without a final result frame",
                )
                .into()
            })
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

fn u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(Value::as_u64)
}

fn f64_field(value: &Value, field: &str) -> Option<f64> {
    value.get(field).and_then(Value::as_f64)
}

fn metadata_mismatch_diagnostic(value: &Value, init_metadata: &ClaudeMetadata) -> Option<String> {
    let mut mismatches = Vec::new();
    if let (Some(init_session_id), Some(final_session_id)) =
        (&init_metadata.session_id, string_field(value, "session_id"))
    {
        if init_session_id != &final_session_id {
            mismatches.push("session_id");
        }
    }
    if let (Some(init_model), Some(final_model)) = (
        &init_metadata.model,
        string_field(value, "model").or_else(|| string_field(value, "model_name")),
    ) {
        if init_model != &final_model {
            mismatches.push("model");
        }
    }
    (!mismatches.is_empty()).then(|| {
        concise_runtime_text(&format!(
            "Claude init metadata differed from final result metadata for {}; using final result metadata",
            mismatches.join(", ")
        ))
    })
}

fn assistant_text(value: &Value) -> Option<String> {
    let mut content = String::new();
    if let Some(message) = value.get("message") {
        collect_text_blocks(message, &mut content);
    }
    if content.is_empty() {
        if let Some(delta) = value.get("delta") {
            collect_text_blocks(delta, &mut content);
        }
    }
    if content.is_empty() {
        if let Some(raw_content) = value.get("content") {
            collect_text_blocks(raw_content, &mut content);
        }
    }
    (!content.is_empty()).then_some(content)
}

fn collect_text_blocks(value: &Value, content: &mut String) {
    match value {
        Value::Object(map) => {
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "text")
            {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    content.push_str(text);
                }
            }
            if let Some(value) = map.get("content") {
                collect_text_blocks(value, content);
            }
            if let Some(value) = map.get("delta") {
                collect_text_blocks(value, content);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_text_blocks(value, content);
            }
        }
        Value::String(text) => content.push_str(text),
        _ => {}
    }
}

fn frame_requests_local_action(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(tool_type_is_local_action)
            {
                return true;
            }
            if map.contains_key("tool_use_id") {
                return true;
            }
            if map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(tool_name_is_local_action)
            {
                return true;
            }
            map.values().any(frame_requests_local_action)
        }
        Value::Array(values) => values.iter().any(frame_requests_local_action),
        _ => false,
    }
}

fn tool_type_is_local_action(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("tool_use")
        || kind.contains("tool_result")
        || kind.contains("local_action")
        || kind == "tool"
}

fn tool_name_is_local_action(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "bash"
            | "read"
            | "edit"
            | "write"
            | "multiedit"
            | "notebookedit"
            | "ls"
            | "glob"
            | "grep"
            | "webfetch"
            | "websearch"
            | "todowrite"
            | "task"
    ) || lower.starts_with("mcp__")
}

fn nonzero_exit_error(status: ExitStatus, stderr: &str) -> anyhow::Error {
    let status_message = status
        .code()
        .map(|code| format!("status {code}"))
        .unwrap_or_else(|| "signal".to_string());
    let detail = concise_runtime_text(stderr);
    let message = if detail.is_empty() {
        format!("Claude runtime exited with {status_message}")
    } else {
        format!("Claude runtime exited with {status_message}: {detail}")
    };
    classified_claude_error(message).into()
}

fn classified_claude_error(message: String) -> RuntimeProviderError {
    if claude_error_is_retryable(&message) {
        RuntimeProviderError::retryable(message)
    } else {
        RuntimeProviderError::non_retryable(message)
    }
}

fn claude_failure_status(prefix: &str, error: &anyhow::Error) -> String {
    let detail = concise_runtime_text(&error.to_string());
    if detail.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {detail}")
    }
}

fn claude_error_is_retryable(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("429")
        || lower.contains("overload")
        || lower.contains("capacity")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("temporarily")
        || lower.contains("temporary")
        || lower.contains("unavailable")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
}

fn claude_prompt_text(request: &RuntimeRequest) -> Result<String> {
    let envelope = prompt_envelope_json(request)?;
    Ok(format!(
        r#"You are running inside atelier through the Claude CLI as a structured runtime adapter, not as a standalone coding agent.

Follow this protocol exactly:
- Use the JSON envelope below as the only task input.
- Do not solve the user's prompt directly in prose.
- Do not edit files, run commands, inspect the repository, use Claude Code tools, use MCP tools, or call local tool surfaces directly.
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
  "clarifying_options": [],
  "recommended_option_id": null,
  "final_summary": null
}}

When status is waiting_for_user:
- Set clarifying_question to one targeted question.
- Set clarifying_options to 2-4 concise recommended answers, each shaped as {{"id": "stable-id", "label": "short answer", "description": "optional detail or null"}}.
- Keep option ids unique and every id and label non-empty.
- Set recommended_option_id to the strongest option id, or null when no option stands out.
- Do not add a custom, other, or free-text option; the app always provides its own custom text answer path.

When output_schema is agent_result, return (findings, changed_files, commands, and verification are each a list of plain strings — never objects or action descriptors):
{{
  "schema_version": 1,
  "agent": "{agent_id}",
  "step_id": "{step_id}",
  "status": "completed|blocked|failed|cancelled|parse_error|limit_reached|approval_denied|no_changes",
  "summary": "brief result",
  "findings": ["short factual finding"],
  "changed_files": ["relative/path/to/file"],
  "commands": ["short description of a command you ran, e.g. cargo test"],
  "verification": ["how you confirmed the result, e.g. cargo test passed"],
  "blocker": null,
  "artifacts": []
}}
Never embed action objects (read_file, list_files, run_command, ...) inside any agent_result field; to perform an action, return one action_request contract at a time.

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

fn resolve_command(command: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists().then(|| path.to_path_buf());
    }
    which::which(command).ok()
}

fn join_status_parts(left: &str, right: &str) -> String {
    match (left.trim(), right.trim()) {
        ("", "") => "claude status unknown".to_string(),
        (left, "") => left.to_string(),
        ("", right) => right.to_string(),
        (left, right) => format!("{left}; {right}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentEffort, AgentProfile, AgentPromptMetadata, Capability, Limits, PromptMode, RuntimeKind,
    };
    use crate::orchestrator::{wrap_json_contract, AgentResult};
    use crate::runtime::collect_runtime_step_result;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[tokio::test]
    async fn claude_availability_reports_missing_command() {
        let runtime = ClaudeRuntime::new(RuntimeConfig {
            id: "claude".to_string(),
            kind: RuntimeKind::Claude,
            command: Some("/missing/claude".to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        });

        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unavailable);
        assert!(availability.message.contains("not found"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_availability_checks_version_and_help_flags() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("claude-ok.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'claude 2.0.0'; exit 0; fi\nif [ \"$1\" = \"--help\" ]; then echo '{}'; exit 0; fi\necho unexpected >&2\nexit 64\n",
                REQUIRED_HELP_FLAGS.join(" ")
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));

        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unknown);
        assert!(availability.message.contains("claude 2.0.0"));
        assert!(availability.message.contains("tools disabled"));
        assert!(availability
            .remediation
            .as_deref()
            .unwrap()
            .contains("does not run paid"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_availability_warns_when_help_omits_default_flags() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("claude-old.sh");
        fs::write(
            &script_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'claude 1.0.0'; exit 0; fi\nif [ \"$1\" = \"--help\" ]; then echo '--output-format'; exit 0; fi\nexit 64\n",
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));

        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unknown);
        assert!(availability.message.contains("--include-partial-messages"));
        assert!(availability
            .message
            .contains("help output may be incomplete"));
        assert!(availability
            .remediation
            .as_deref()
            .unwrap()
            .contains("normal agent step"));
    }

    #[test]
    fn claude_step_args_synthesize_protected_defaults_and_model() {
        let args = claude_step_args(&["--safe-compat".to_string()], "claude-opus-4");

        assert_eq!(args[0], "-p");
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--output-format", "stream-json"]));
        assert!(args.windows(2).any(|pair| pair == ["--tools", ""]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--setting-sources", "user"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--model", "claude-opus-4"]));
        assert!(args.contains(&"--safe-compat".to_string()));
    }

    #[test]
    fn claude_step_args_omit_model_for_default() {
        let args = claude_step_args(&[], "default");

        assert!(!args.iter().any(|arg| arg == "--model"));
    }

    #[test]
    fn claude_step_args_reject_flag_like_primary_and_fallback_model_values() {
        for model in ["--tools=Bash", "--mcp-config"] {
            let args = claude_step_args(&[], model);

            let error = validate_synthesized_args(&args).unwrap_err();

            assert!(error.to_string().contains("cannot start with '-'"));
            assert!(!super::super::is_retryable_provider_error(&error));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_runtime_rejects_flag_like_model_before_spawn() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let spawned = dir.path().join("spawned");
        let script_path = dir.path().join("should-not-spawn-claude.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ntouch '{}'\ncat >/dev/null\nprintf '%s\\n' {}\n",
                spawned.display(),
                shell_single_quoted(&final_result_frame("explorer", "step", "spawned"))
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer", "--tools=Bash"),
                RuntimeEventSink::channel(4).0,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("cannot start with '-'"));
        assert!(!spawned.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_runtime_captures_args_and_stdin_without_help_preflight() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let capture_stdin = dir.path().join("stdin.txt");
        let capture_args = dir.path().join("args.txt");
        let script_path = dir.path().join("fake-claude.sh");
        let final_frame = final_result_frame("explorer", "step", "claude completed");
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  echo "execution should not run help" >&2
  exit 88
fi
printf '%s\n' "$@" > "{}"
cat > "{}"
printf '%s\n' {}
printf '%s\n' {}
printf '%s\n' {}
"#,
                capture_args.display(),
                capture_stdin.display(),
                shell_single_quoted(
                    &serde_json::json!({"type":"system","subtype":"init","session_id":"s1","model":"claude-sonnet"}).to_string()
                ),
                shell_single_quoted(
                    &serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"live progress"}]}}).to_string()
                ),
                shell_single_quoted(&final_frame),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));
        let request = runtime_request(dir.path().to_path_buf(), "explorer", "claude-opus-4");

        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();

        let args = fs::read_to_string(capture_args).unwrap();
        let stdin = fs::read_to_string(capture_stdin).unwrap();
        assert!(args.contains("-p\n"));
        assert!(args.contains("--output-format\nstream-json\n"));
        assert!(args.contains("--include-partial-messages\n"));
        assert!(args.contains("--no-session-persistence\n"));
        assert!(args.contains("--tools\n\n"));
        assert!(args.contains("--setting-sources\nuser\n"));
        assert!(args.contains("--model\nclaude-opus-4\n"));
        assert!(stdin.contains("through the Claude CLI as a structured runtime adapter"));
        assert!(stdin.contains("Do not edit files, run commands"));
        assert!(stdin.contains("\"runtime\": \"claude\""));
        assert!(result
            .stream_deltas
            .iter()
            .any(|delta| delta.stream == "message" && delta.content.contains("live progress")));
        assert!(result.stream_deltas.iter().any(|delta| {
            delta.stream == "status"
                && delta.content.contains("session_id=s1")
                && delta.content.contains("model=claude-sonnet")
                && delta.content.contains("duration_ms=25")
                && delta.content.contains("num_turns=1")
                && delta.content.contains("total_cost_usd=0.01")
                && delta.content.contains("subtype=success")
        }));
        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "claude completed");
            }
            other => panic!("unexpected output: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_runtime_runs_in_working_directory_and_inherits_environment_without_injection() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = crate::runtime::CODEX_ENV_MUTEX.lock().await;
        let _env_guard = EnvGuard::set(&[
            (
                "MULTIAGENT_CLAUDE_TEST_INHERITED",
                Some("inherited-from-harness"),
            ),
            ("MULTIAGENT_CLAUDE_RUNTIME_INJECTED", None),
        ]);

        let dir = tempdir().unwrap();
        let working_directory = dir.path().join("workspace");
        fs::create_dir(&working_directory).unwrap();
        let capture_cwd = dir.path().join("cwd.txt");
        let capture_inherited = dir.path().join("inherited.txt");
        let capture_injected = dir.path().join("injected.txt");
        let script_path = dir.path().join("cwd-env-claude.sh");
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
cat >/dev/null
pwd > "{}"
printf '%s\n' "${{MULTIAGENT_CLAUDE_TEST_INHERITED-unset}}" > "{}"
printf '%s\n' "${{MULTIAGENT_CLAUDE_RUNTIME_INJECTED-unset}}" > "{}"
printf '%s\n' {}
"#,
                capture_cwd.display(),
                capture_inherited.display(),
                capture_injected.display(),
                shell_single_quoted(&final_result_frame("explorer", "step", "cwd env ok")),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));
        let request = runtime_request(working_directory.clone(), "explorer", "default");

        collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();

        let captured_cwd = fs::read_to_string(capture_cwd).unwrap();
        assert_eq!(
            fs::canonicalize(captured_cwd.trim()).unwrap(),
            fs::canonicalize(working_directory).unwrap()
        );
        assert_eq!(
            fs::read_to_string(capture_inherited).unwrap().trim(),
            "inherited-from-harness"
        );
        assert_eq!(
            fs::read_to_string(capture_injected).unwrap().trim(),
            "unset"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_runtime_cancellation_kills_child_process() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let started = dir.path().join("started");
        let script_path = dir.path().join("sleeping-claude.sh");
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
touch "{}"
cat >/dev/null
exec sleep 30
"#,
                started.display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));
        let request = runtime_request(dir.path().to_path_buf(), "explorer", "default");
        let cancellation = CancellationToken::new();
        let task = tokio::spawn({
            let runtime = runtime.clone();
            let cancellation = cancellation.clone();
            async move {
                runtime
                    .stream_step(request, RuntimeEventSink::channel(4).0, cancellation)
                    .await
            }
        });

        wait_for_path(&started).await;
        cancellation.cancel();
        let error = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();

        assert!(error.to_string().contains("cancelled"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_runtime_timeout_is_retryable_kills_child_and_emits_status() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let started = dir.path().join("started");
        let pid_path = dir.path().join("pid");
        let script_path = dir.path().join("timeout-claude.sh");
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
echo $$ > "{}"
touch "{}"
cat >/dev/null
exec sleep 30
"#,
                pid_path.display(),
                started.display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path))
            .with_runtime_timeout(Duration::from_secs(2));
        let (events, mut receiver) = RuntimeEventSink::channel(8);
        let request = runtime_request(dir.path().to_path_buf(), "explorer", "default");
        let task = tokio::spawn(async move {
            runtime
                .stream_step(request, events, CancellationToken::new())
                .await
        });

        wait_for_path(&started).await;
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();

        let mut emitted = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            emitted.push(event);
        }
        let pid = fs::read_to_string(pid_path).unwrap();
        assert!(started.exists());
        assert!(!process_exists(pid.trim()));
        assert!(super::super::is_retryable_provider_error(&error));
        assert!(error.to_string().contains("timed out"));
        assert!(emitted.iter().any(|event| {
            event.stream_name() == "status"
                && event.content().contains("Claude runtime attempt failed")
                && event.content().contains("timed out")
        }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_nonzero_exit_wins_over_valid_result_frame() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("failing-claude.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' {}\necho 'auth failed sk-secret-token' >&2\nexit 42\n",
                shell_single_quoted(&final_result_frame("explorer", "step", "should not parse"))
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer", "default"),
                RuntimeEventSink::channel(4).0,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("status 42"));
        assert!(message.contains("<redacted secret>"));
        assert!(!message.contains("should not parse"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_late_tool_use_after_partial_emits_failure_status() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("late-tool-claude.sh");
        let partial = serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "partial before failure"}]}
        });
        let tool_use = serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": "pwd"}}]}
        });
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' {}\nprintf '%s\\n' {}\n",
                shell_single_quoted(&partial.to_string()),
                shell_single_quoted(&tool_use.to_string()),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));
        let (events, mut receiver) = RuntimeEventSink::channel(8);

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer", "default"),
                events,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        let mut emitted = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            emitted.push(event);
        }
        assert!(error.to_string().contains("harness-action boundary"));
        assert!(emitted.iter().any(|event| {
            event.is_transient()
                && event.stream_name() == "message"
                && event.content().contains("partial before failure")
        }));
        assert!(emitted.iter().any(|event| {
            event.stream_name() == "status"
                && event
                    .content()
                    .contains("Claude stream invalidated attempt")
                && event.content().contains("harness-action boundary")
        }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_success_allows_stderr_warnings() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("warning-claude.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ncat >/dev/null\necho 'warning Bearer test-token' >&2\nprintf '%s\\n' {}\n",
                shell_single_quoted(&final_result_frame("explorer", "step", "warning still succeeds"))
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = ClaudeRuntime::new(claude_runtime_config(&script_path));
        let request = runtime_request(dir.path().to_path_buf(), "explorer", "default");

        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();

        assert!(result.stream_deltas.iter().any(|delta| {
            delta.stream == "stderr"
                && delta.content.contains("Bearer <redacted>")
                && !delta.content.contains("test-token")
        }));
        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "warning still succeeds");
            }
            other => panic!("unexpected output: {other:?}"),
        }
    }

    #[tokio::test]
    async fn claude_stream_missing_result_is_provider_error() {
        let mut state = ClaudeStreamState::default();
        state
            .apply_line(
                &serde_json::json!({"type":"system","subtype":"init"}).to_string(),
                &RuntimeEventSink::channel(4).0,
            )
            .await
            .unwrap();

        let error = state.final_result_text().unwrap_err();

        assert!(error.to_string().contains("without a final result"));
    }

    #[tokio::test]
    async fn claude_stream_multiple_results_are_provider_error() {
        let mut state = ClaudeStreamState::default();
        let events = RuntimeEventSink::channel(4).0;
        let frame = final_result_frame("explorer", "step", "one");
        state.apply_line(&frame, &events).await.unwrap();

        let error = state.apply_line(&frame, &events).await.unwrap_err();

        assert!(error.to_string().contains("multiple final result"));
    }

    #[tokio::test]
    async fn claude_stream_tool_use_fails_closed() {
        let mut state = ClaudeStreamState::default();
        let frame = serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "tool_use", "name": "Bash", "input": {"command": "pwd"}}
                ]
            }
        });

        let error = state
            .apply_line(&frame.to_string(), &RuntimeEventSink::channel(4).0)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("harness-action boundary"));
    }

    #[tokio::test]
    async fn claude_result_error_frames_are_retryable_when_provider_is_temporary() {
        let mut state = ClaudeStreamState::default();
        let frame = serde_json::json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": true,
            "result": "rate limit exceeded"
        });

        let error = state
            .apply_line(&frame.to_string(), &RuntimeEventSink::channel(4).0)
            .await
            .unwrap_err();

        assert!(super::super::is_retryable_provider_error(&error));
    }

    #[tokio::test]
    async fn claude_malformed_final_contract_becomes_parse_error() {
        let mut state = ClaudeStreamState::default();
        let frame = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": "plain prose"
        });
        state
            .apply_line(&frame.to_string(), &RuntimeEventSink::channel(4).0)
            .await
            .unwrap();

        let output = parse_runtime_output("explorer", state.final_result_text().unwrap()).unwrap();

        assert!(matches!(output, RuntimeOutput::ParseError { .. }));
    }

    #[tokio::test]
    async fn claude_metadata_uses_init_fallback_for_session_and_model() {
        let mut state = ClaudeStreamState::default();
        let events = RuntimeEventSink::channel(4).0;
        state
            .apply_line(
                &serde_json::json!({
                    "type": "system",
                    "subtype": "init",
                    "session_id": "init-session",
                    "model": "claude-sonnet"
                })
                .to_string(),
                &events,
            )
            .await
            .unwrap();
        state
            .apply_line(
                &serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "is_error": false,
                    "result": wrap_json_contract(&AgentResult::completed("explorer", "step", "ok")).unwrap(),
                    "duration_ms": 9,
                    "num_turns": 1,
                    "total_cost_usd": 0.0
                })
                .to_string(),
                &events,
            )
            .await
            .unwrap();

        let summary = state.metadata_summary().unwrap();

        assert!(summary.contains("session_id=init-session"));
        assert!(summary.contains("model=claude-sonnet"));
        assert!(summary.contains("duration_ms=9"));
    }

    #[tokio::test]
    async fn claude_metadata_mismatch_emits_redacted_diagnostic_and_prefers_final() {
        let mut state = ClaudeStreamState::default();
        let (events, mut receiver) = RuntimeEventSink::channel(8);
        state
            .apply_line(
                &serde_json::json!({
                    "type": "system",
                    "subtype": "init",
                    "session_id": "init-session",
                    "model": "init-model"
                })
                .to_string(),
                &events,
            )
            .await
            .unwrap();
        state
            .apply_line(
                &serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "is_error": false,
                    "session_id": "final-session",
                    "model": "final-model",
                    "result": wrap_json_contract(&AgentResult::completed("explorer", "step", "ok")).unwrap(),
                })
                .to_string(),
                &events,
            )
            .await
            .unwrap();

        let summary = state.metadata_summary().unwrap();
        let mut emitted = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            emitted.push(event);
        }

        assert!(summary.contains("session_id=final-session"));
        assert!(summary.contains("model=final-model"));
        assert!(emitted.iter().any(|event| {
            event.stream_name() == "metadata"
                && event.content().contains("session_id, model")
                && !event.content().contains("init-session")
                && !event.content().contains("final-session")
                && !event.content().contains("init-model")
                && !event.content().contains("final-model")
        }));
    }

    #[test]
    fn claude_prompt_text_uses_harness_actions_not_system_prompt_flags() {
        let prompt = claude_prompt_text(&runtime_request(
            std::path::PathBuf::from("/tmp/project"),
            "orchestrator",
            "default",
        ))
        .unwrap();

        assert!(prompt.contains("through the Claude CLI as a structured runtime adapter"));
        assert!(prompt.contains("return one action_request JSON contract"));
        assert!(prompt.contains(crate::orchestrator::JSON_START));
        assert!(!prompt.contains("--system-prompt"));
    }

    #[test]
    fn claude_prompt_text_types_agent_result_arrays_as_strings() {
        let prompt = claude_prompt_text(&runtime_request(
            std::path::PathBuf::from("/tmp/project"),
            "explorer",
            "default",
        ))
        .unwrap();

        assert!(prompt.contains("each a list of plain strings"));
        assert!(prompt.contains("Never embed action objects"));
        assert!(!prompt.contains("\"commands\": []"));
    }

    #[test]
    fn claude_prompt_text_describes_structured_clarification_contract() {
        let prompt = claude_prompt_text(&runtime_request(
            std::path::PathBuf::from("/tmp/project"),
            "orchestrator",
            "default",
        ))
        .unwrap();

        assert!(prompt.contains("\"clarifying_options\": []"));
        assert!(prompt.contains("\"recommended_option_id\": null"));
        assert!(prompt.contains("2-4 concise recommended answers"));
        assert!(prompt.contains("Set recommended_option_id to the strongest option id"));
        assert!(prompt.contains("the app always provides its own custom text answer path"));
        assert!(!prompt.contains("question tool"));
        assert!(!prompt.contains("ask_user"));
    }

    fn final_result_frame(agent: &str, step_id: &str, summary: &str) -> String {
        let result = AgentResult::completed(agent, step_id, summary);
        let wrapped = wrap_json_contract(&result).unwrap();
        serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": wrapped,
            "session_id": "s1",
            "model": "claude-sonnet",
            "duration_ms": 25,
            "num_turns": 1,
            "total_cost_usd": 0.01
        })
        .to_string()
    }

    fn shell_single_quoted(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    #[cfg(unix)]
    fn process_exists(pid: &str) -> bool {
        if pid.is_empty() {
            return false;
        }
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid)
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    async fn wait_for_path(path: &Path) {
        for _ in 0..250 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for {}", path.display());
    }

    struct EnvGuard {
        vars: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn set(vars: &[(&'static str, Option<&'static str>)]) -> Self {
            let saved = vars
                .iter()
                .map(|(name, value)| {
                    let previous = std::env::var_os(name);
                    match value {
                        Some(value) => std::env::set_var(name, value),
                        None => std::env::remove_var(name),
                    }
                    (*name, previous)
                })
                .collect();
            Self { vars: saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.vars {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn claude_runtime_config(command: &std::path::Path) -> RuntimeConfig {
        RuntimeConfig {
            id: "claude".to_string(),
            kind: RuntimeKind::Claude,
            command: Some(command.display().to_string()),
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: None,
            api_key_env: None,
        }
    }

    fn runtime_request(
        working_directory: std::path::PathBuf,
        agent_id: &str,
        model: &str,
    ) -> RuntimeRequest {
        RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "inspect context".to_string(),
            session_goal: None,
            working_directory,
            agent_profile: AgentProfile {
                id: agent_id.to_string(),
                name: "Explorer".to_string(),
                runtime: "claude".to_string(),
                model: model.to_string(),
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
            parallel_context: None,
            capability_constraints: vec![Capability::Read],
            limits: Limits::default(),
        }
    }
}
