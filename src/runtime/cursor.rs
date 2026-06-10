use super::{
    concise_runtime_text, concise_runtime_text_with_limit, parse_runtime_output,
    process_output_text, Runtime, RuntimeAvailability, RuntimeAvailabilityStatus, RuntimeEventSink,
    RuntimeOutput, RuntimeProviderError, RuntimeRequest,
};
use crate::config::{validate_cursor_runtime_args, RuntimeConfig};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, ExitStatus, Stdio};
use std::time::Duration;
use std::{env, io};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const DEFAULT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(600);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const CURSOR_API_KEY_ENV: &str = "CURSOR_API_KEY";

const PROTECTED_DEFAULT_ARGS: &[&str] = &["--print", "--output-format", "stream-json"];
const ISOLATED_DENY_PERMISSIONS: &[&str] = &[
    "Shell(*)",
    "Read(*)",
    "Read(**)",
    "Read(/*)",
    "Write(*)",
    "Write(**)",
    "Write(/*)",
    "WebFetch(*)",
    "Mcp(*)",
];

#[derive(Clone, Debug)]
pub struct CursorRuntime {
    config: RuntimeConfig,
    runtime_timeout: Duration,
}

impl CursorRuntime {
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
impl Runtime for CursorRuntime {
    async fn check_availability(&self) -> RuntimeAvailability {
        let Some(command) = &self.config.command else {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: "cursor command is not configured".to_string(),
                remediation: Some("Set [runtimes.cursor].command in multiagent.toml.".to_string()),
            };
        };

        if resolve_command(command).is_none() {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: format!("cursor command was not found: {command}"),
                remediation: Some(
                    "Install Cursor Agent CLI or set [runtimes.cursor].command.".to_string(),
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
                    status: RuntimeAvailabilityStatus::Unknown,
                    message: concise_runtime_text(&process_output_text(&output)),
                    remediation: Some(
                        "Run cursor-agent --version directly to inspect local setup.".to_string(),
                    ),
                };
            }
            Ok(Err(error)) => {
                return RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Unavailable,
                    message: error.to_string(),
                    remediation: Some(
                        "Run cursor-agent --version directly to inspect local setup.".to_string(),
                    ),
                };
            }
            Err(_) => {
                return RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Unknown,
                    message: "cursor-agent --version timed out".to_string(),
                    remediation: Some(
                        "Run cursor-agent --version directly to inspect local setup.".to_string(),
                    ),
                };
            }
        };

        let status = timeout(PROBE_TIMEOUT, Command::new(command).arg("status").output()).await;
        match status {
            Ok(Ok(output)) if output.status.success() => {
                let status_message = concise_runtime_text(&process_output_text(&output));
                RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: RuntimeAvailabilityStatus::Available,
                    message: join_status_parts(&version_message, &status_message),
                    remediation: None,
                }
            }
            Ok(Ok(output)) => {
                let status_message = concise_runtime_text(&process_output_text(&output));
                let api_key_set = env_var_is_set(CURSOR_API_KEY_ENV);
                RuntimeAvailability {
                    runtime_id: self.config.id.clone(),
                    status: if api_key_set {
                        RuntimeAvailabilityStatus::Unknown
                    } else {
                        RuntimeAvailabilityStatus::Unavailable
                    },
                    message: if api_key_set {
                        join_status_parts(
                            &join_status_parts(&version_message, &status_message),
                            "CURSOR_API_KEY is set; saved login may not be required",
                        )
                    } else {
                        join_status_parts(&version_message, &status_message)
                    },
                    remediation: Some(cursor_login_remediation()),
                }
            }
            Ok(Err(error)) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: join_status_parts(&version_message, &error.to_string()),
                remediation: Some(cursor_login_remediation()),
            },
            Err(_) => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: join_status_parts(&version_message, "cursor-agent status timed out"),
                remediation: Some(cursor_login_remediation()),
            },
        }
    }

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput> {
        validate_cursor_runtime_args(&self.config.id, &self.config.args)?;
        let command = self
            .config
            .command
            .as_ref()
            .context("cursor command is not configured")?;
        let prompt = cursor_prompt_text(&request)?;
        let args = cursor_step_args(&self.config.args, &request.agent_profile.model);
        validate_synthesized_args(&args)?;
        let permission_sandbox = CursorPermissionSandbox::create()
            .context("failed to prepare isolated Cursor permission sandbox")?;

        let mut child = Command::new(command)
            .args(&args)
            .current_dir(permission_sandbox.working_directory())
            .env("CURSOR_CONFIG_DIR", permission_sandbox.cursor_config_dir())
            .env("XDG_CONFIG_HOME", permission_sandbox.xdg_config_home())
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn cursor runtime command {command}"))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt envelope to cursor stdin")?;
        }
        drop(child.stdin.take());

        let stdout = child
            .stdout
            .take()
            .context("failed to capture cursor stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture cursor stderr")?;
        let mut stdout_reader = tokio::spawn(read_cursor_stdout(stdout, events.clone()));
        let stderr_reader = tokio::spawn(read_diagnostic_stream(stderr, events.clone(), "stderr"));

        let mut stdout_state = None;
        let status_result = tokio::select! {
            output = &mut stdout_reader => {
                match output.context("cursor stdout reader task failed")? {
                    Ok(state) => {
                        stdout_state = Some(state);
                        wait_for_child_or_cancel(&mut child, &cancellation, self.runtime_timeout).await
                    }
                    Err(error) => {
                        kill_child(&mut child).await;
                        let _ = stderr_reader.await;
                        let _ = events
                            .status(cursor_failure_status(
                                "Cursor stream invalidated attempt",
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
                    .status(cursor_failure_status(
                        "Cursor runtime attempt failed",
                        &error,
                    ))
                    .await;
                return Err(error);
            }
        };

        let stdout_state = match stdout_state {
            Some(state) => state,
            None => stdout_reader
                .await
                .context("cursor stdout reader task failed")??,
        };
        let stderr = stderr_reader
            .await
            .context("cursor stderr reader task failed")??;

        if !status.success() {
            let error = nonzero_exit_error(status, &stderr);
            let _ = events
                .status(cursor_failure_status(
                    "Cursor runtime attempt failed",
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
                    .status(cursor_failure_status(
                        "Cursor stream invalidated attempt",
                        &error,
                    ))
                    .await;
                return Err(error);
            }
        };
        parse_runtime_output(&request.agent_profile.id, final_text)
    }
}

pub(crate) fn protected_defaults_summary() -> Vec<&'static str> {
    vec![
        "print mode enabled",
        "stream-json enabled",
        "session resume disabled",
        "model flag owned by agent profiles",
        "isolated deny-all Cursor permission config",
        "Cursor tools must not bypass Harness Actions",
    ]
}

#[derive(Debug)]
struct CursorPermissionSandbox {
    root: PathBuf,
    cursor_config_dir: PathBuf,
    xdg_config_home: PathBuf,
    working_directory: PathBuf,
}

impl CursorPermissionSandbox {
    fn create() -> Result<Self> {
        let root = env::temp_dir().join(format!(
            "multiagent-cursor-{}-{}",
            process::id(),
            ulid::Ulid::new()
        ));
        let cursor_config_dir = root.join("cursor-config");
        let xdg_config_home = root.join("xdg-config");
        let xdg_cursor_config_dir = xdg_config_home.join("cursor");
        let working_directory = root.join("workspace");
        let project_cursor_dir = working_directory.join(".cursor");

        fs::create_dir_all(&cursor_config_dir)?;
        fs::create_dir_all(&xdg_cursor_config_dir)?;
        fs::create_dir_all(&project_cursor_dir)?;

        let config = isolated_cursor_permission_config()?;
        fs::write(cursor_config_dir.join("cli-config.json"), &config)?;
        fs::write(xdg_cursor_config_dir.join("cli-config.json"), &config)?;
        fs::write(project_cursor_dir.join("cli.json"), &config)?;

        Ok(Self {
            root,
            cursor_config_dir,
            xdg_config_home,
            working_directory,
        })
    }

    fn cursor_config_dir(&self) -> &Path {
        &self.cursor_config_dir
    }

    fn xdg_config_home(&self) -> &Path {
        &self.xdg_config_home
    }

    fn working_directory(&self) -> &Path {
        &self.working_directory
    }
}

impl Drop for CursorPermissionSandbox {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.root) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_error) => {}
        }
    }
}

fn isolated_cursor_permission_config() -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "editor": { "vimMode": false },
        "permissions": {
            "allow": [],
            "deny": ISOLATED_DENY_PERMISSIONS,
        }
    }))?)
}

fn cursor_step_args(config_args: &[String], model: &str) -> Vec<String> {
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
    require_arg(args, "--print")?;
    require_arg_value(args, "--output-format", "stream-json")?;
    validate_synthesized_model_args(args)?;
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--resume" | "resume" | "ls" | "--force" | "-f" | "--api-key" | "-a"
        )
    }) {
        bail!("cursor synthesized args contain forbidden session, force, or auth flags");
    }
    Ok(())
}

fn validate_synthesized_model_args(args: &[String]) -> Result<()> {
    for (index, arg) in args.iter().enumerate() {
        if arg == "--model" {
            let Some(model) = args.get(index + 1) else {
                return Err(RuntimeProviderError::non_retryable(
                    "Cursor synthesized args include --model without a model value",
                )
                .into());
            };
            if model.starts_with('-') {
                return Err(RuntimeProviderError::non_retryable(
                    "Cursor model value cannot start with '-' because it may be parsed as a CLI flag",
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
        bail!("cursor synthesized args are missing protected flag {flag}");
    }
}

fn require_arg_value(args: &[String], flag: &str, value: &str) -> Result<()> {
    if args
        .windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
    {
        Ok(())
    } else {
        bail!("cursor synthesized args are missing protected flag {flag} {value:?}");
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
            bail!("Cursor runtime cancelled");
        }
        _ = tokio::time::sleep(runtime_timeout) => {
            request_child_kill(child);
            Err(RuntimeProviderError::retryable("Cursor runtime timed out").into())
        }
        status = child.wait() => {
            status.context("failed to wait for Cursor runtime")
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
    stdout_reader: JoinHandle<Result<CursorStreamState>>,
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
            .with_context(|| format!("failed to read cursor {stream}"))?;
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

async fn read_cursor_stdout<R>(reader: R, events: RuntimeEventSink) -> Result<CursorStreamState>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut state = CursorStreamState::default();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .context("failed to read cursor stdout")?;
        if read == 0 {
            break;
        }
        state.apply_line(&line, &events).await?;
    }
    Ok(state)
}

#[derive(Clone, Debug, Deserialize)]
struct CursorStreamFrame {
    #[serde(default, rename = "type")]
    frame_type: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    message: Option<Value>,
    #[serde(default)]
    delta: Option<Value>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    tool_call: Option<Value>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: Option<bool>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default, rename = "apiKeySource")]
    api_key_source: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "permissionMode")]
    permission_mode: Option<String>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    duration_api_ms: Option<u64>,
}

#[derive(Clone, Debug)]
struct CursorResultFrame {
    result_text: String,
    metadata: CursorMetadata,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CursorMetadata {
    session_id: Option<String>,
    request_id: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    api_key_source: Option<String>,
    permission_mode: Option<String>,
    duration_ms: Option<u64>,
    duration_api_ms: Option<u64>,
    subtype: Option<String>,
}

impl CursorMetadata {
    fn from_init_frame(frame: &CursorStreamFrame) -> Self {
        Self {
            session_id: frame.session_id.clone(),
            request_id: frame.request_id.clone(),
            model: frame.model.clone(),
            cwd: frame.cwd.clone(),
            api_key_source: frame.api_key_source.clone(),
            permission_mode: frame.permission_mode.clone(),
            duration_ms: None,
            duration_api_ms: None,
            subtype: frame.subtype.clone(),
        }
    }

    fn from_result_frame(frame: &CursorStreamFrame, init_metadata: &Self) -> Self {
        Self {
            session_id: frame
                .session_id
                .clone()
                .or_else(|| init_metadata.session_id.clone()),
            request_id: frame
                .request_id
                .clone()
                .or_else(|| init_metadata.request_id.clone()),
            model: frame.model.clone().or_else(|| init_metadata.model.clone()),
            cwd: frame.cwd.clone().or_else(|| init_metadata.cwd.clone()),
            api_key_source: frame
                .api_key_source
                .clone()
                .or_else(|| init_metadata.api_key_source.clone()),
            permission_mode: frame
                .permission_mode
                .clone()
                .or_else(|| init_metadata.permission_mode.clone()),
            duration_ms: frame.duration_ms,
            duration_api_ms: frame.duration_api_ms,
            subtype: frame.subtype.clone(),
        }
    }

    fn summary(&self) -> Option<String> {
        let mut fields = Vec::new();
        if let Some(session_id) = &self.session_id {
            fields.push(format!("session_id={session_id}"));
        }
        if let Some(request_id) = &self.request_id {
            fields.push(format!("request_id={request_id}"));
        }
        if let Some(model) = &self.model {
            fields.push(format!("model={model}"));
        }
        if let Some(cwd) = &self.cwd {
            fields.push(format!("cwd={cwd}"));
        }
        if let Some(api_key_source) = &self.api_key_source {
            fields.push(format!("api_key_source={api_key_source}"));
        }
        if let Some(permission_mode) = &self.permission_mode {
            fields.push(format!("permission_mode={permission_mode}"));
        }
        if let Some(duration_ms) = self.duration_ms {
            fields.push(format!("duration_ms={duration_ms}"));
        }
        if let Some(duration_api_ms) = self.duration_api_ms {
            fields.push(format!("duration_api_ms={duration_api_ms}"));
        }
        if let Some(subtype) = &self.subtype {
            fields.push(format!("subtype={subtype}"));
        }
        (!fields.is_empty()).then(|| format!("Cursor metadata: {}", fields.join(", ")))
    }
}

#[derive(Clone, Debug, Default)]
struct CursorStreamState {
    final_result: Option<CursorResultFrame>,
    init_metadata: CursorMetadata,
    ignored_optional_frames: usize,
}

impl CursorStreamState {
    async fn apply_line(&mut self, line: &str, events: &RuntimeEventSink) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let value: Value = serde_json::from_str(trimmed).map_err(|error| {
            RuntimeProviderError::non_retryable(format!(
                "malformed Cursor stream JSONL: {}; frame: {}",
                error,
                concise_runtime_text(trimmed)
            ))
        })?;
        let frame: CursorStreamFrame = serde_json::from_value(value.clone()).map_err(|error| {
            RuntimeProviderError::non_retryable(format!(
                "invalid Cursor stream frame: {}; frame: {}",
                error,
                concise_runtime_text(trimmed)
            ))
        })?;
        let frame_type = frame.frame_type.as_deref().ok_or_else(|| {
            RuntimeProviderError::non_retryable(format!(
                "Cursor stream frame missing type: {}",
                concise_runtime_text(trimmed)
            ))
        })?;

        match frame_type {
            "system" => self.apply_system_frame(&frame),
            "assistant" => {
                if let Some(text) = assistant_text(&frame) {
                    events.delta("stdout", text).await?;
                }
                Ok(())
            }
            "tool_call" => self.apply_tool_call_frame(&frame, events).await,
            "result" => self.apply_result_frame(&frame, events).await,
            "user" => {
                self.ignored_optional_frames += 1;
                Ok(())
            }
            _ => {
                self.ignored_optional_frames += 1;
                Ok(())
            }
        }
    }

    fn apply_system_frame(&mut self, frame: &CursorStreamFrame) -> Result<()> {
        match frame.subtype.as_deref() {
            Some("init") => {
                self.init_metadata = CursorMetadata::from_init_frame(frame);
                Ok(())
            }
            Some(_) | None => {
                self.ignored_optional_frames += 1;
                Ok(())
            }
        }
    }

    async fn apply_tool_call_frame(
        &mut self,
        frame: &CursorStreamFrame,
        events: &RuntimeEventSink,
    ) -> Result<()> {
        let families = tool_call_families(frame);
        if families.is_empty() {
            return Err(RuntimeProviderError::non_retryable(
                "Cursor tool_call frame did not identify a tool family; failing closed",
            )
            .into());
        }

        if families.iter().all(|family| family == "readToolCall") {
            let family = &families[0];
            events
                .diagnostic("cursor_tool", read_tool_call_summary(frame, family))
                .await?;
            return Ok(());
        }

        let family = families
            .iter()
            .find(|family| family.as_str() != "readToolCall")
            .unwrap_or(&families[0]);
        Err(RuntimeProviderError::non_retryable(format!(
            "Cursor tool call {family} violates the harness-action boundary; request Harness Actions instead"
        ))
        .into())
    }

    async fn apply_result_frame(
        &mut self,
        frame: &CursorStreamFrame,
        events: &RuntimeEventSink,
    ) -> Result<()> {
        if self.final_result.is_some() {
            return Err(RuntimeProviderError::non_retryable(
                "Cursor stream emitted multiple final result frames",
            )
            .into());
        }

        let subtype = frame.subtype.as_deref().ok_or_else(|| {
            RuntimeProviderError::non_retryable("Cursor result frame missing subtype")
        })?;
        let is_error = frame.is_error.ok_or_else(|| {
            RuntimeProviderError::non_retryable("Cursor result frame missing is_error")
        })?;
        if is_error {
            let message = frame
                .result
                .clone()
                .unwrap_or_else(|| "Cursor result frame reported an error".to_string());
            return Err(RuntimeProviderError::non_retryable(format!(
                "Cursor result frame reported error ({subtype}): {}",
                concise_runtime_text(&message)
            ))
            .into());
        }
        if subtype.to_ascii_lowercase().contains("error") {
            return Err(RuntimeProviderError::non_retryable(format!(
                "Cursor result frame has error subtype {subtype}"
            ))
            .into());
        }
        let Some(result_text) = frame.result.clone() else {
            return Err(RuntimeProviderError::non_retryable(
                "Cursor final result frame omitted result text",
            )
            .into());
        };
        if result_text.trim().is_empty() {
            return Err(RuntimeProviderError::non_retryable(
                "Cursor final result frame contained empty result text",
            )
            .into());
        }

        if let Some(diagnostic) = metadata_mismatch_diagnostic(frame, &self.init_metadata) {
            events.diagnostic("metadata", diagnostic).await?;
        }
        let metadata = CursorMetadata::from_result_frame(frame, &self.init_metadata);
        self.final_result = Some(CursorResultFrame {
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
                    "Cursor stream ended without a final result frame",
                )
                .into()
            })
    }
}

fn metadata_mismatch_diagnostic(
    frame: &CursorStreamFrame,
    init_metadata: &CursorMetadata,
) -> Option<String> {
    let mut mismatches = Vec::new();
    if let (Some(init_session_id), Some(final_session_id)) =
        (&init_metadata.session_id, &frame.session_id)
    {
        if init_session_id != final_session_id {
            mismatches.push("session_id");
        }
    }
    if let (Some(init_model), Some(final_model)) = (&init_metadata.model, &frame.model) {
        if init_model != final_model {
            mismatches.push("model");
        }
    }
    (!mismatches.is_empty()).then(|| {
        concise_runtime_text(&format!(
            "Cursor init metadata differed from final result metadata for {}; using final result metadata",
            mismatches.join(", ")
        ))
    })
}

fn assistant_text(frame: &CursorStreamFrame) -> Option<String> {
    let mut content = String::new();
    if let Some(message) = &frame.message {
        collect_text_blocks(message, &mut content);
    }
    if content.is_empty() {
        if let Some(delta) = &frame.delta {
            collect_text_blocks(delta, &mut content);
        }
    }
    if content.is_empty() {
        if let Some(raw_content) = &frame.content {
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

fn tool_call_families(frame: &CursorStreamFrame) -> Vec<String> {
    let Some(Value::Object(map)) = frame.tool_call.as_ref() else {
        return Vec::new();
    };
    map.keys().cloned().collect()
}

fn read_tool_call_summary(frame: &CursorStreamFrame, family: &str) -> String {
    let subtype = frame.subtype.as_deref().unwrap_or("unknown");
    let mut fields = vec![format!("subtype={subtype}")];
    if let Some(call_id) = &frame.call_id {
        fields.push(format!("call_id={call_id}"));
    }
    if let Some(path) = tool_call_arg_path(frame, family) {
        fields.push(format!("path={path}"));
    }
    format!("Cursor readToolCall diagnostic: {}", fields.join(", "))
}

fn tool_call_arg_path(frame: &CursorStreamFrame, family: &str) -> Option<String> {
    frame
        .tool_call
        .as_ref()?
        .get(family)?
        .get("args")?
        .get("path")?
        .as_str()
        .map(str::to_string)
}

fn nonzero_exit_error(status: ExitStatus, stderr: &str) -> anyhow::Error {
    let status_message = status
        .code()
        .map(|code| format!("status {code}"))
        .unwrap_or_else(|| "signal".to_string());
    let detail = concise_runtime_text(stderr);
    let message = if detail.is_empty() {
        format!("Cursor runtime exited with {status_message}")
    } else {
        format!("Cursor runtime exited with {status_message}: {detail}")
    };
    classified_cursor_error(message).into()
}

fn classified_cursor_error(message: String) -> RuntimeProviderError {
    if cursor_error_is_retryable(&message) {
        RuntimeProviderError::retryable(message)
    } else {
        RuntimeProviderError::non_retryable(message)
    }
}

fn cursor_failure_status(prefix: &str, error: &anyhow::Error) -> String {
    let detail = concise_runtime_text(&error.to_string());
    if detail.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {detail}")
    }
}

fn cursor_error_is_retryable(message: &str) -> bool {
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

fn cursor_prompt_text(request: &RuntimeRequest) -> Result<String> {
    let envelope = super::prompt_envelope_json(request)?;
    Ok(format!(
        r#"You are running inside atelier through the Cursor Agent CLI as a structured runtime adapter, not as a standalone coding agent.

Follow this protocol exactly:
- Use the JSON envelope below as the only task input.
- The Cursor process is running from a harness-owned isolated permission sandbox; use the working_directory field in the envelope only when asking the harness for actions.
- Do not solve the user's prompt directly in prose.
- Do not edit files, run commands, inspect the repository, use Cursor tools, use MCP tools, or call local tool surfaces directly.
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

fn resolve_command(command: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists().then(|| path.to_path_buf());
    }
    which::which(command).ok()
}

fn join_status_parts(left: &str, right: &str) -> String {
    match (left.trim(), right.trim()) {
        ("", "") => "cursor status unknown".to_string(),
        (left, "") => left.to_string(),
        ("", right) => right.to_string(),
        (left, right) => format!("{left}; {right}"),
    }
}

fn cursor_login_remediation() -> String {
    "Run cursor-agent login and verify with cursor-agent status. For automation, set CURSOR_API_KEY in the environment; do not store it in multiagent.toml.".to_string()
}

fn env_var_is_set(name: &str) -> bool {
    env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
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
    async fn cursor_availability_reports_missing_command() {
        let runtime = CursorRuntime::new(RuntimeConfig {
            id: "cursor".to_string(),
            kind: RuntimeKind::Cursor,
            command: Some("/missing/cursor-agent".to_string()),
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
    async fn cursor_availability_checks_version_and_status() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("cursor-ok.sh");
        fs::write(
            &script_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'cursor-agent 1.0.0'; exit 0; fi\nif [ \"$1\" = \"status\" ]; then echo 'Authenticated as dev@example.com'; exit 0; fi\necho unexpected >&2\nexit 64\n",
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));

        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Available);
        assert!(availability.message.contains("cursor-agent 1.0.0"));
        assert!(availability.message.contains("Authenticated"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_availability_failed_status_uses_env_fallback_as_unknown() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = crate::runtime::CODEX_ENV_MUTEX.lock().await;
        let _env_guard = EnvGuard::set(&[(CURSOR_API_KEY_ENV, Some("key-from-env"))]);
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("cursor-env-auth.sh");
        fs::write(
            &script_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'cursor-agent 1.0.0'; exit 0; fi\nif [ \"$1\" = \"status\" ]; then echo 'Not authenticated' >&2; exit 1; fi\nexit 64\n",
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));

        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unknown);
        assert!(availability.message.contains(CURSOR_API_KEY_ENV));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_availability_failed_status_without_env_is_unavailable() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = crate::runtime::CODEX_ENV_MUTEX.lock().await;
        let _env_guard = EnvGuard::set(&[(CURSOR_API_KEY_ENV, None)]);
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("cursor-no-auth.sh");
        fs::write(
            &script_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'cursor-agent 1.0.0'; exit 0; fi\nif [ \"$1\" = \"status\" ]; then echo 'Not authenticated' >&2; exit 1; fi\nexit 64\n",
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));

        let availability = runtime.check_availability().await;

        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unavailable);
        assert!(availability
            .remediation
            .as_deref()
            .unwrap()
            .contains("cursor-agent login"));
    }

    #[test]
    fn cursor_step_args_synthesize_print_mode_and_model() {
        let args = cursor_step_args(&["--safe-compat".to_string()], "gpt-5");

        assert_eq!(args[0], "--print");
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--output-format", "stream-json"]));
        assert!(args.windows(2).any(|pair| pair == ["--model", "gpt-5"]));
        assert!(args.contains(&"--safe-compat".to_string()));
    }

    #[test]
    fn cursor_step_args_omit_model_for_default() {
        let args = cursor_step_args(&[], "default");

        assert!(!args.iter().any(|arg| arg == "--model"));
    }

    #[test]
    fn cursor_step_args_reject_flag_like_model_values() {
        let args = cursor_step_args(&[], "--force");

        let error = validate_synthesized_args(&args).unwrap_err();

        assert!(error.to_string().contains("cannot start with '-'"));
        assert!(!super::super::is_retryable_provider_error(&error));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_runtime_rejects_flag_like_model_before_spawn() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let spawned = dir.path().join("spawned");
        let script_path = dir.path().join("should-not-spawn-cursor.sh");
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
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer", "--force"),
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
    async fn cursor_runtime_captures_args_stdin_progress_tools_and_result() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let capture_stdin = dir.path().join("stdin.txt");
        let capture_args = dir.path().join("args.txt");
        let script_path = dir.path().join("fake-cursor.sh");
        let final_frame = final_result_frame("explorer", "step", "cursor completed");
        let init_frame = serde_json::json!({
            "type":"system",
            "subtype":"init",
            "session_id":"s1",
            "model":"gpt-5",
            "cwd": dir.path(),
            "apiKeySource":"login",
            "permissionMode":"default"
        });
        let assistant_frame = serde_json::json!({
            "type":"assistant",
            "message":{"role":"assistant","content":[{"type":"text","text":"live progress"}]},
            "session_id":"s1"
        });
        let read_tool_frame = serde_json::json!({
            "type":"tool_call",
            "subtype":"started",
            "call_id":"tool-1",
            "tool_call":{"readToolCall":{"args":{"path":"README.md"}}},
            "session_id":"s1"
        });
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > "{}"
cat > "{}"
printf '%s\n' {}
printf '%s\n' {}
printf '%s\n' {}
printf '%s\n' {}
"#,
                capture_args.display(),
                capture_stdin.display(),
                shell_single_quoted(&init_frame.to_string()),
                shell_single_quoted(&assistant_frame.to_string()),
                shell_single_quoted(&read_tool_frame.to_string()),
                shell_single_quoted(&final_frame),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));
        let request = runtime_request(dir.path().to_path_buf(), "explorer", "gpt-5");

        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();

        let args = fs::read_to_string(capture_args).unwrap();
        let stdin = fs::read_to_string(capture_stdin).unwrap();
        assert!(args.contains("--print\n"));
        assert!(args.contains("--output-format\nstream-json\n"));
        assert!(args.contains("--model\ngpt-5\n"));
        assert!(stdin.contains("through the Cursor Agent CLI as a structured runtime adapter"));
        assert!(stdin.contains("Do not edit files, run commands"));
        assert!(stdin.contains("\"runtime\": \"cursor\""));
        assert!(result
            .stream_deltas
            .iter()
            .any(|delta| delta.stream == "stdout" && delta.content.contains("live progress")));
        assert!(result.stream_deltas.iter().any(|delta| {
            delta.stream == "cursor_tool"
                && delta.content.contains("readToolCall")
                && delta.content.contains("README.md")
                && !delta.content.contains("file contents")
        }));
        assert!(result.stream_deltas.iter().any(|delta| {
            delta.stream == "status"
                && delta.content.contains("session_id=s1")
                && delta.content.contains("model=gpt-5")
                && delta.content.contains("api_key_source=login")
                && delta.content.contains("permission_mode=default")
                && delta.content.contains("duration_ms=25")
        }));
        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "cursor completed");
            }
            other => panic!("unexpected output: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_runtime_starts_with_isolated_permission_config() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let request_working_directory = dir.path().join("repo");
        fs::create_dir(&request_working_directory).unwrap();
        let capture_pwd = dir.path().join("pwd.txt");
        let capture_stdin = dir.path().join("stdin.txt");
        let capture_config_dir = dir.path().join("cursor-config-dir.txt");
        let capture_xdg_config_home = dir.path().join("xdg-config-home.txt");
        let capture_global_config = dir.path().join("global-config.json");
        let capture_project_config = dir.path().join("project-config.json");
        let script_path = dir.path().join("sandbox-cursor.sh");
        let final_frame = final_result_frame("explorer", "step", "sandboxed");
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$PWD" > "{}"
printf '%s\n' "$CURSOR_CONFIG_DIR" > "{}"
printf '%s\n' "$XDG_CONFIG_HOME" > "{}"
cat "$CURSOR_CONFIG_DIR/cli-config.json" > "{}"
cat "$PWD/.cursor/cli.json" > "{}"
cat > "{}"
printf '%s\n' {}
"#,
                capture_pwd.display(),
                capture_config_dir.display(),
                capture_xdg_config_home.display(),
                capture_global_config.display(),
                capture_project_config.display(),
                capture_stdin.display(),
                shell_single_quoted(&final_frame),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));
        let request = runtime_request(request_working_directory.clone(), "explorer", "default");

        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();

        assert!(matches!(result.output, RuntimeOutput::AgentResult { .. }));
        let sandbox_pwd = fs::read_to_string(capture_pwd).unwrap();
        assert_ne!(Path::new(sandbox_pwd.trim()), request_working_directory);
        assert!(!Path::new(sandbox_pwd.trim()).exists());
        let cursor_config_dir = fs::read_to_string(capture_config_dir).unwrap();
        let xdg_config_home = fs::read_to_string(capture_xdg_config_home).unwrap();
        assert!(cursor_config_dir.contains("multiagent-cursor"));
        assert!(xdg_config_home.contains("multiagent-cursor"));
        let global_config = fs::read_to_string(capture_global_config).unwrap();
        let project_config = fs::read_to_string(capture_project_config).unwrap();
        for config in [global_config, project_config] {
            assert!(config.contains("\"allow\": []"));
            assert!(config.contains("Shell(*)"));
            assert!(config.contains("Write(**)"));
            assert!(config.contains("Mcp(*)"));
        }
        let stdin = fs::read_to_string(capture_stdin).unwrap();
        assert!(stdin.contains(&request_working_directory.display().to_string()));
        assert!(stdin.contains("isolated permission sandbox"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_runtime_nonzero_exit_classifies_stderr() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("rate-limit-cursor.sh");
        fs::write(
            &script_path,
            "#!/bin/sh\ncat >/dev/null\necho 'rate limit exceeded' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer", "default"),
                RuntimeEventSink::channel(4).0,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("status 1"));
        assert!(error.to_string().contains("rate limit exceeded"));
        assert!(super::super::is_retryable_provider_error(&error));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_write_tool_call_fails_closed() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("write-tool-cursor.sh");
        let tool_frame = serde_json::json!({
            "type":"tool_call",
            "subtype":"started",
            "call_id":"tool-2",
            "tool_call":{"writeToolCall":{"args":{"path":"summary.txt","fileText":"content"}}}
        });
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' {}\n",
                shell_single_quoted(&tool_frame.to_string()),
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));

        let error = runtime
            .stream_step(
                runtime_request(dir.path().to_path_buf(), "explorer", "default"),
                RuntimeEventSink::channel(4).0,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("writeToolCall"));
        assert!(error.to_string().contains("harness-action boundary"));
        assert!(!super::super::is_retryable_provider_error(&error));
    }

    #[tokio::test]
    async fn cursor_unknown_tool_call_fails_closed() {
        let mut state = CursorStreamState::default();
        let frame = serde_json::json!({
            "type":"tool_call",
            "subtype":"started",
            "tool_call":{"futureToolCall":{"args":{}}}
        });

        let error = state
            .apply_line(&frame.to_string(), &RuntimeEventSink::channel(4).0)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("futureToolCall"));
        assert!(!super::super::is_retryable_provider_error(&error));
    }

    #[tokio::test]
    async fn cursor_stream_missing_result_is_provider_error() {
        let mut state = CursorStreamState::default();
        state
            .apply_line(
                &serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}).to_string(),
                &RuntimeEventSink::channel(4).0,
            )
            .await
            .unwrap();

        let error = state.final_result_text().unwrap_err();

        assert!(error.to_string().contains("without a final result"));
    }

    #[tokio::test]
    async fn cursor_result_error_frames_are_nonretryable() {
        let mut state = CursorStreamState::default();
        let frame = serde_json::json!({
            "type":"result",
            "subtype":"error",
            "is_error": true,
            "result":"rate limit exceeded"
        });

        let error = state
            .apply_line(&frame.to_string(), &RuntimeEventSink::channel(4).0)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("reported error"));
        assert!(!super::super::is_retryable_provider_error(&error));
    }

    #[tokio::test]
    async fn cursor_malformed_final_contract_becomes_parse_error() {
        let mut state = CursorStreamState::default();
        let frame = serde_json::json!({
            "type":"result",
            "subtype":"success",
            "is_error": false,
            "result":"plain prose"
        });
        state
            .apply_line(&frame.to_string(), &RuntimeEventSink::channel(4).0)
            .await
            .unwrap();

        let output = parse_runtime_output("explorer", state.final_result_text().unwrap()).unwrap();

        assert!(matches!(output, RuntimeOutput::ParseError { .. }));
    }

    #[tokio::test]
    async fn cursor_invalid_jsonl_is_provider_error() {
        let mut state = CursorStreamState::default();

        let error = state
            .apply_line("not-json", &RuntimeEventSink::channel(4).0)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("malformed Cursor stream JSONL"));
        assert!(!super::super::is_retryable_provider_error(&error));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_runtime_cancellation_kills_child_process() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let started = dir.path().join("started");
        let script_path = dir.path().join("sleeping-cursor.sh");
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
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path));
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
    async fn cursor_runtime_timeout_is_retryable_and_kills_child() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let started = dir.path().join("started");
        let pid_path = dir.path().join("pid");
        let script_path = dir.path().join("timeout-cursor.sh");
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
        let runtime = CursorRuntime::new(cursor_runtime_config(&script_path))
            .with_runtime_timeout(Duration::from_secs(2));
        let request = runtime_request(dir.path().to_path_buf(), "explorer", "default");

        let error = tokio::spawn(async move {
            runtime
                .stream_step(
                    request,
                    RuntimeEventSink::channel(4).0,
                    CancellationToken::new(),
                )
                .await
        })
        .await
        .unwrap()
        .unwrap_err();

        let pid = fs::read_to_string(pid_path).unwrap();
        assert!(started.exists());
        assert!(!process_exists(pid.trim()));
        assert!(super::super::is_retryable_provider_error(&error));
        assert!(error.to_string().contains("timed out"));
    }

    #[test]
    fn cursor_prompt_text_uses_harness_actions() {
        let prompt = cursor_prompt_text(&runtime_request(
            std::path::PathBuf::from("/tmp/project"),
            "orchestrator",
            "default",
        ))
        .unwrap();

        assert!(prompt.contains("through the Cursor Agent CLI as a structured runtime adapter"));
        assert!(prompt.contains("return one action_request JSON contract"));
        assert!(prompt.contains(crate::orchestrator::JSON_START));
        assert!(!prompt.contains("--print"));
    }

    #[test]
    fn cursor_prompt_text_describes_structured_clarification_contract() {
        let prompt = cursor_prompt_text(&runtime_request(
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
            "model": "gpt-5",
            "duration_ms": 25,
            "duration_api_ms": 25,
            "request_id": "req-1"
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

    fn cursor_runtime_config(command: &std::path::Path) -> RuntimeConfig {
        RuntimeConfig {
            id: "cursor".to_string(),
            kind: RuntimeKind::Cursor,
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
                runtime: "cursor".to_string(),
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
