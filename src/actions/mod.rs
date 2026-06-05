use crate::config::{AgentProfile, ApprovalMode, Capability, ToolName, WorkspacePolicy};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    ReadFile,
    ListFiles,
    SearchText,
    RunCommand,
    ApplyPatch,
    WriteFile,
    RecordNote,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionRequest {
    pub schema_version: u32,
    pub action_id: String,
    pub step_id: String,
    pub kind: ActionKind,
    pub params: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Completed,
    Denied,
    ApprovalRequired,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionResult {
    pub schema_version: u32,
    pub action_id: String,
    pub status: ActionStatus,
    pub summary: String,
    pub content: Option<Value>,
    pub artifact: Option<Value>,
    pub diagnostic: Option<String>,
}

impl ActionResult {
    pub fn approval_denied(request: &ActionRequest, reason: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            action_id: request.action_id.clone(),
            status: ActionStatus::Denied,
            summary: "Action approval denied.".to_string(),
            content: None,
            artifact: None,
            diagnostic: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandClassification {
    Allow,
    Approve,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionDecision {
    Allowed,
    RequiresApproval(String),
    Denied(String),
}

#[derive(Clone, Debug)]
pub struct ActionExecutionContext {
    pub working_directory: PathBuf,
    pub workspace: WorkspacePolicy,
    pub approval_mode: ApprovalMode,
    pub command_timeout: Option<Duration>,
    pub user_prompt: Option<String>,
}

impl ActionExecutionContext {
    pub fn new(
        working_directory: PathBuf,
        workspace: WorkspacePolicy,
        approval_mode: ApprovalMode,
    ) -> Self {
        Self {
            working_directory,
            workspace,
            approval_mode,
            command_timeout: Some(Duration::from_secs(10 * 60)),
            user_prompt: None,
        }
    }
}

pub fn validate_action_request(
    agent: &AgentProfile,
    workspace: &WorkspacePolicy,
    approval_mode: &ApprovalMode,
    request: &ActionRequest,
) -> ActionDecision {
    if request.schema_version != 1 {
        return ActionDecision::Denied(format!(
            "unsupported action schema_version {}",
            request.schema_version
        ));
    }

    let tool = tool_name_for_action(&request.kind);
    if !agent.has_tool(&tool) {
        return ActionDecision::Denied(format!(
            "agent {} is not allowed to use tool {:?}",
            agent.id, tool
        ));
    }
    let Some(required) = required_capability(&request.kind) else {
        return ActionDecision::Allowed;
    };
    if !agent.has_capability(&required) {
        return ActionDecision::Denied(format!(
            "agent {} lacks required capability {:?}",
            agent.id, required
        ));
    }

    match request.kind {
        ActionKind::ReadFile | ActionKind::ListFiles | ActionKind::SearchText => {
            if let Some(path) = path_param(&request.params) {
                if let Err(error) = validate_model_path(path, &workspace.extra_read_roots) {
                    return ActionDecision::Denied(error.to_string());
                }
            }
            ActionDecision::Allowed
        }
        ActionKind::ApplyPatch => {
            let Some(diff) = request.params.get("diff").and_then(Value::as_str) else {
                return ActionDecision::Denied("apply_patch action is missing diff".to_string());
            };
            if let Err(error) = validate_unified_diff_for_policy(diff, &workspace.extra_write_roots)
            {
                return ActionDecision::Denied(error.to_string());
            }
            ActionDecision::Allowed
        }
        ActionKind::WriteFile => {
            if let Some(path) = path_param(&request.params) {
                if let Err(error) = validate_model_path(path, &workspace.extra_write_roots) {
                    return ActionDecision::Denied(error.to_string());
                }
            }
            ActionDecision::Allowed
        }
        ActionKind::RunCommand => {
            let Some(command) = request.params.get("command").and_then(Value::as_str) else {
                return ActionDecision::Denied("run_command action is missing command".to_string());
            };
            decision_for_command(command, approval_mode)
        }
        ActionKind::RecordNote => ActionDecision::Allowed,
    }
}

fn tool_name_for_action(kind: &ActionKind) -> ToolName {
    match kind {
        ActionKind::ReadFile => ToolName::ReadFile,
        ActionKind::ListFiles => ToolName::ListFiles,
        ActionKind::SearchText => ToolName::SearchText,
        ActionKind::RunCommand => ToolName::RunCommand,
        ActionKind::ApplyPatch => ToolName::ApplyPatch,
        ActionKind::WriteFile => ToolName::WriteFile,
        ActionKind::RecordNote => ToolName::RecordNote,
    }
}

pub async fn execute_action_request(
    agent: &AgentProfile,
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> ActionResult {
    match validate_action_request(agent, &context.workspace, &context.approval_mode, request) {
        ActionDecision::Denied(reason) => {
            return action_result(
                request,
                ActionStatus::Denied,
                "Action denied by harness policy.",
                None,
                Some(reason),
            );
        }
        ActionDecision::RequiresApproval(reason) => {
            return action_result(
                request,
                ActionStatus::ApprovalRequired,
                "Action requires action approval.",
                None,
                Some(reason),
            );
        }
        ActionDecision::Allowed => {}
    }

    if let ActionKind::RunCommand = request.kind {
        let Some(command) = request.params.get("command").and_then(Value::as_str) else {
            return action_result(
                request,
                ActionStatus::Denied,
                "Action denied by harness policy.",
                None,
                Some("run_command action is missing command".to_string()),
            );
        };
        if is_vcs_mutation(command)
            && !vcs_action_explicitly_requested(&context.user_prompt, command)
        {
            return action_result(
                request,
                ActionStatus::Denied,
                "Action denied by harness policy.",
                None,
                Some("VCS actions require an explicit user request in the prompt.".to_string()),
            );
        }
    }

    let result = match request.kind {
        ActionKind::ReadFile => execute_read_file(context, request),
        ActionKind::ListFiles => execute_list_files(context, request),
        ActionKind::SearchText => execute_search_text(context, request),
        ActionKind::RunCommand => execute_run_command(context, request).await,
        ActionKind::ApplyPatch => execute_apply_patch(context, request),
        ActionKind::WriteFile => execute_write_file(context, request),
        ActionKind::RecordNote => execute_record_note(request),
    };

    match result {
        Ok(result) => result,
        Err(error) => action_result(
            request,
            ActionStatus::Failed,
            "Action failed.",
            None,
            Some(format!("{error:#}")),
        ),
    }
}

pub fn decision_for_command(command: &str, approval_mode: &ApprovalMode) -> ActionDecision {
    match classify_command(command) {
        CommandClassification::Allow => ActionDecision::Allowed,
        CommandClassification::Deny => {
            ActionDecision::Denied(format!("command is denied by built-in policy: {command}"))
        }
        CommandClassification::Approve => match approval_mode {
            ApprovalMode::Yolo => ActionDecision::Allowed,
            ApprovalMode::Normal => ActionDecision::RequiresApproval(format!(
                "command requires action approval: {command}"
            )),
        },
    }
}

pub fn classify_command(command: &str) -> CommandClassification {
    let normalized = command.trim();
    let lower = normalized.to_ascii_lowercase();
    if lower.is_empty() {
        return CommandClassification::Deny;
    }

    let deny_patterns = [
        "rm -rf /",
        "rm -fr /",
        "mkfs",
        "dd if=",
        ":(){",
        "chmod 777",
        "chown -r",
        "sudo ",
        "su -",
        "curl ",
        "wget ",
        "security find-generic-password",
        "cat ~/.ssh",
        "cat ~/.config",
    ];
    if deny_patterns.iter().any(|pattern| lower.contains(pattern)) {
        if lower.contains("| sh") || lower.contains("| bash") || lower.contains("rm -rf /") {
            return CommandClassification::Deny;
        }
        if lower.starts_with("curl ") || lower.starts_with("wget ") {
            return CommandClassification::Approve;
        }
        return CommandClassification::Deny;
    }

    if is_default_read_only_command(&lower) {
        return CommandClassification::Allow;
    }

    let approve_prefixes = [
        "git commit",
        "git push",
        "git reset",
        "git checkout",
        "git switch",
        "git branch",
        "git add",
        "git rm",
        "git restore",
        "git merge",
        "git rebase",
        "git cherry-pick",
        "git revert",
        "git tag ",
        "rm ",
        "mv ",
        "cp ",
        "brew install",
        "brew upgrade",
        "cargo install",
        "npm install",
        "pnpm install",
        "yarn add",
    ];
    if approve_prefixes
        .iter()
        .any(|prefix| command_has_prefix(&lower, prefix))
    {
        return CommandClassification::Approve;
    }

    CommandClassification::Approve
}

fn is_default_read_only_command(lower: &str) -> bool {
    if has_shell_control_syntax(lower) {
        return false;
    }

    let allow_prefixes = [
        "cargo test",
        "cargo check",
        "cargo build",
        "cargo fmt",
        "cargo clippy",
        "cargo metadata",
        "cargo tree",
        "cargo locate-project",
        "git status",
        "git diff",
        "git log",
        "git show",
        "git rev-parse",
        "git ls-files",
        "git grep",
        "git blame",
        "git describe",
        "rg ",
        "ls",
        "pwd",
        "sed -n",
        "cat ",
        "grep ",
        "find ",
        "wc ",
        "multiagent --doctor",
        "multiagent --print-config",
        "multiagent --help",
        "multiagent --version",
    ];
    allow_prefixes
        .iter()
        .any(|prefix| command_has_prefix(lower, prefix))
        || is_read_only_git_branch_command(lower)
        || is_read_only_git_remote_command(lower)
}

fn has_shell_control_syntax(command: &str) -> bool {
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && !in_single {
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if in_single || in_double {
            continue;
        }

        match ch {
            '\n' | '\r' | ';' | '&' | '|' | '>' | '<' | '`' | '(' | ')' => return true,
            '$' if chars.peek() == Some(&'(') => return true,
            _ => {}
        }
    }

    false
}

pub fn is_vcs_mutation(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    if is_default_read_only_command(&lower) {
        return false;
    }

    let mutation_prefixes = [
        "git commit",
        "git push",
        "git reset",
        "git checkout",
        "git switch",
        "git branch",
        "git add",
        "git rm",
        "git restore",
        "git merge",
        "git rebase",
        "git cherry-pick",
        "git revert",
        "git tag",
    ];
    mutation_prefixes
        .iter()
        .any(|prefix| command_has_prefix(&lower, prefix))
}

pub fn vcs_action_explicitly_requested(user_prompt: &Option<String>, command: &str) -> bool {
    let Some(prompt) = user_prompt.as_deref() else {
        return false;
    };
    let prompt = prompt.to_ascii_lowercase();
    let command = command.to_ascii_lowercase();

    if command.starts_with("git commit") {
        prompt.contains("commit")
    } else if command.starts_with("git push") {
        prompt.contains("push")
    } else if command.starts_with("git reset") {
        prompt.contains("reset")
    } else if command.starts_with("git checkout") || command.starts_with("git switch") {
        prompt.contains("checkout")
            || prompt.contains("switch branch")
            || prompt.contains("change branch")
    } else if command.starts_with("git branch ") {
        prompt.contains("branch")
    } else if command.starts_with("git add") {
        prompt.contains("stage") || prompt.contains("git add") || prompt.contains("commit")
    } else if command.starts_with("git rm") {
        prompt.contains("git rm") || prompt.contains("remove from git")
    } else if command.starts_with("git restore") {
        prompt.contains("restore")
    } else if command.starts_with("git merge") {
        prompt.contains("merge")
    } else if command.starts_with("git rebase") {
        prompt.contains("rebase")
    } else if command.starts_with("git cherry-pick") {
        prompt.contains("cherry-pick") || prompt.contains("cherry pick")
    } else if command.starts_with("git revert") {
        prompt.contains("revert")
    } else if command.starts_with("git tag ") {
        prompt.contains("tag")
    } else {
        false
    }
}

fn is_read_only_git_branch_command(lower: &str) -> bool {
    lower == "git branch"
        || command_has_prefix(lower, "git branch --show-current")
        || command_has_prefix(lower, "git branch --list")
        || command_has_prefix(lower, "git branch --all")
        || command_has_prefix(lower, "git branch --remotes")
        || command_has_prefix(lower, "git branch --verbose")
        || command_has_prefix(lower, "git branch --contains")
        || command_has_prefix(lower, "git branch --merged")
        || command_has_prefix(lower, "git branch --no-merged")
        || command_has_prefix(lower, "git branch -a")
        || command_has_prefix(lower, "git branch -r")
        || command_has_prefix(lower, "git branch -v")
}

fn is_read_only_git_remote_command(lower: &str) -> bool {
    lower == "git remote"
        || command_has_prefix(lower, "git remote -v")
        || command_has_prefix(lower, "git remote show")
        || command_has_prefix(lower, "git remote get-url")
}

fn command_has_prefix(lower_command: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end();
    lower_command == prefix || lower_command.starts_with(&format!("{prefix} "))
}

pub fn validate_model_path(path: &str, extra_roots: &[PathBuf]) -> Result<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        if extra_roots.iter().any(|root| candidate.starts_with(root)) {
            return Ok(candidate.to_path_buf());
        }
        bail!("absolute paths are not allowed for model-requested actions: {path}");
    }

    for component in candidate.components() {
        match component {
            Component::ParentDir => bail!("path traversal is not allowed: {path}"),
            Component::Prefix(_) | Component::RootDir => {
                bail!("rooted paths are not allowed for model-requested actions: {path}")
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(candidate.to_path_buf())
}

fn required_capability(kind: &ActionKind) -> Option<Capability> {
    match kind {
        ActionKind::ReadFile | ActionKind::ListFiles | ActionKind::SearchText => {
            Some(Capability::Read)
        }
        ActionKind::RunCommand => Some(Capability::Command),
        ActionKind::ApplyPatch | ActionKind::WriteFile => Some(Capability::Edit),
        ActionKind::RecordNote => None,
    }
}

fn path_param(params: &Value) -> Option<&str> {
    params.get("path").and_then(Value::as_str)
}

fn action_result(
    request: &ActionRequest,
    status: ActionStatus,
    summary: impl Into<String>,
    content: Option<Value>,
    diagnostic: Option<String>,
) -> ActionResult {
    ActionResult {
        schema_version: 1,
        action_id: request.action_id.clone(),
        status,
        summary: summary.into(),
        content,
        artifact: None,
        diagnostic,
    }
}

fn execute_read_file(
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> Result<ActionResult> {
    let path = required_string_param(&request.params, "path")?;
    let resolved = resolve_action_path(
        &context.working_directory,
        path,
        &context.workspace.extra_read_roots,
        true,
    )?;
    let contents = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    Ok(action_result(
        request,
        ActionStatus::Completed,
        format!("Read {} bytes from {path}.", contents.len()),
        Some(json!({ "path": path, "content": contents })),
        None,
    ))
}

fn execute_list_files(
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> Result<ActionResult> {
    let path = optional_string_param(&request.params, "path").unwrap_or(".");
    let resolved = resolve_action_path(
        &context.working_directory,
        path,
        &context.workspace.extra_read_roots,
        true,
    )?;
    let max_entries = optional_u64_param(&request.params, "max_entries")
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(500);
    let mut entries = Vec::new();
    collect_file_entries(
        &context.working_directory,
        &resolved,
        &mut entries,
        max_entries,
    )?;
    entries.sort();
    let count = entries.len();
    Ok(action_result(
        request,
        ActionStatus::Completed,
        format!("Listed {count} paths under {path}."),
        Some(json!({ "path": path, "entries": entries })),
        None,
    ))
}

fn execute_search_text(
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> Result<ActionResult> {
    let query = required_string_param(&request.params, "query")?;
    let path = optional_string_param(&request.params, "path").unwrap_or(".");
    let resolved = resolve_action_path(
        &context.working_directory,
        path,
        &context.workspace.extra_read_roots,
        true,
    )?;
    let max_matches = optional_u64_param(&request.params, "max_matches")
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(200);
    let mut matches = Vec::new();
    search_text_entries(
        &context.working_directory,
        &resolved,
        query,
        &mut matches,
        max_matches,
    )?;
    let count = matches.len();
    Ok(action_result(
        request,
        ActionStatus::Completed,
        format!("Found {count} matches for {query:?}."),
        Some(json!({ "query": query, "path": path, "matches": matches })),
        None,
    ))
}

async fn execute_run_command(
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> Result<ActionResult> {
    let command = required_string_param(&request.params, "command")?;
    let mut child = Command::new("sh");
    child
        .arg("-c")
        .arg(command)
        .current_dir(&context.working_directory)
        .kill_on_drop(true);

    let output_future = child.output();
    let output = match context.command_timeout {
        Some(duration) => timeout(duration, output_future)
            .await
            .with_context(|| format!("command timed out after {} seconds", duration.as_secs()))??,
        None => output_future.await?,
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();
    let status = if output.status.success() {
        ActionStatus::Completed
    } else {
        ActionStatus::Failed
    };
    Ok(action_result(
        request,
        status,
        match exit_code {
            Some(code) => format!("Command exited with status {code}."),
            None => "Command terminated by signal.".to_string(),
        },
        Some(json!({
            "command": command,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        })),
        if output.status.success() {
            None
        } else {
            Some("command returned a non-zero exit status".to_string())
        },
    ))
}

fn execute_write_file(
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> Result<ActionResult> {
    let path = required_string_param(&request.params, "path")?;
    let contents = required_string_param(&request.params, "content")?;
    let resolved = resolve_action_path(
        &context.working_directory,
        path,
        &context.workspace.extra_write_roots,
        false,
    )?;
    if resolved.exists() {
        bail!("write_file refuses to overwrite existing files; use apply_patch for existing files");
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    }
    fs::write(&resolved, contents)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(action_result(
        request,
        ActionStatus::Completed,
        format!("Wrote {} bytes to {path}.", contents.len()),
        Some(json!({ "path": path, "bytes": contents.len() })),
        None,
    ))
}

fn execute_apply_patch(
    context: &ActionExecutionContext,
    request: &ActionRequest,
) -> Result<ActionResult> {
    let diff = required_string_param(&request.params, "diff")?;
    let applied = apply_unified_diff(
        &context.working_directory,
        &context.workspace.extra_write_roots,
        diff,
    )?;
    Ok(action_result(
        request,
        ActionStatus::Completed,
        format!("Applied patch to {} file(s).", applied.len()),
        Some(json!({ "changed_files": applied })),
        None,
    ))
}

fn execute_record_note(request: &ActionRequest) -> Result<ActionResult> {
    let note = required_string_param(&request.params, "note")?;
    Ok(action_result(
        request,
        ActionStatus::Completed,
        "Recorded note.",
        Some(json!({ "note": note })),
        None,
    ))
}

pub fn apply_unified_diff(
    working_directory: &Path,
    extra_write_roots: &[PathBuf],
    diff: &str,
) -> Result<Vec<String>> {
    if diff.contains("GIT binary patch") || diff.contains("Binary files ") {
        bail!("binary patches are not supported");
    }

    let file_patches = parse_unified_diff(diff)?;
    let mut seen_targets = BTreeSet::new();
    let mut rewritten = BTreeMap::new();
    let mut changed_files = Vec::new();

    for file_patch in file_patches {
        if !seen_targets.insert(file_patch.target_path.clone()) {
            bail!("patch contains duplicate target {}", file_patch.target_path);
        }
        let resolved = resolve_action_path(
            working_directory,
            &file_patch.target_path,
            extra_write_roots,
            true,
        )?;
        let original = fs::read_to_string(&resolved)
            .with_context(|| format!("failed to read patch target {}", resolved.display()))?;
        let updated = apply_hunks_to_text(&original, &file_patch.hunks)
            .with_context(|| format!("failed to apply patch to {}", file_patch.target_path))?;
        rewritten.insert(resolved, updated);
        changed_files.push(file_patch.target_path);
    }

    for (path, contents) in rewritten {
        fs::write(&path, contents)
            .with_context(|| format!("failed to write patched file {}", path.display()))?;
    }

    Ok(changed_files)
}

fn validate_unified_diff_for_policy(diff: &str, extra_write_roots: &[PathBuf]) -> Result<()> {
    if diff.contains("GIT binary patch") || diff.contains("Binary files ") {
        bail!("binary patches are not supported");
    }
    for file_patch in parse_unified_diff(diff)? {
        validate_model_path(&file_patch.target_path, extra_write_roots)?;
    }
    Ok(())
}

fn resolve_action_path(
    working_directory: &Path,
    path: &str,
    extra_roots: &[PathBuf],
    must_exist: bool,
) -> Result<PathBuf> {
    let validated = validate_model_path(path, extra_roots)?;
    let resolved = if validated.is_absolute() {
        validated
    } else {
        working_directory.join(validated)
    };
    if must_exist && !resolved.exists() {
        bail!("path does not exist: {path}");
    }
    Ok(resolved)
}

fn required_string_param<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing string parameter {key}"))
}

fn optional_string_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

fn optional_u64_param(params: &Value, key: &str) -> Option<u64> {
    params.get(key).and_then(Value::as_u64)
}

fn collect_file_entries(
    working_directory: &Path,
    path: &Path,
    entries: &mut Vec<String>,
    max_entries: usize,
) -> Result<()> {
    if entries.len() >= max_entries {
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to list {}", path.display()))? {
        if entries.len() >= max_entries {
            return Ok(());
        }
        let entry = entry?;
        let entry_path = entry.path();
        entries.push(display_action_path(working_directory, &entry_path));
        if entry.file_type()?.is_dir() {
            collect_file_entries(working_directory, &entry_path, entries, max_entries)?;
        }
    }
    Ok(())
}

fn search_text_entries(
    working_directory: &Path,
    path: &Path,
    query: &str,
    matches: &mut Vec<Value>,
    max_matches: usize,
) -> Result<()> {
    if matches.len() >= max_matches {
        return Ok(());
    }

    if path.is_file() {
        if let Ok(contents) = fs::read_to_string(path) {
            for (line_index, line) in contents.lines().enumerate() {
                if line.contains(query) {
                    matches.push(json!({
                        "path": display_action_path(working_directory, path),
                        "line": line_index + 1,
                        "text": line,
                    }));
                    if matches.len() >= max_matches {
                        return Ok(());
                    }
                }
            }
        }
        return Ok(());
    }

    for entry in
        fs::read_dir(path).with_context(|| format!("failed to search {}", path.display()))?
    {
        if matches.len() >= max_matches {
            return Ok(());
        }
        let entry = entry?;
        let entry_path = entry.path();
        search_text_entries(working_directory, &entry_path, query, matches, max_matches)?;
    }
    Ok(())
}

fn display_action_path(working_directory: &Path, path: &Path) -> String {
    path.strip_prefix(working_directory)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

#[derive(Debug)]
struct FilePatch {
    target_path: String,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    old_start: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug)]
enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

fn parse_unified_diff(diff: &str) -> Result<Vec<FilePatch>> {
    let lines = diff.lines().collect::<Vec<_>>();
    let mut index = 0;
    let mut patches = Vec::new();

    while index < lines.len() {
        if !lines[index].starts_with("--- ") {
            index += 1;
            continue;
        }
        let old_path = parse_diff_path(lines[index], "--- ")?;
        index += 1;
        if index >= lines.len() || !lines[index].starts_with("+++ ") {
            bail!("unified diff is missing +++ header after --- {old_path}");
        }
        let new_path = parse_diff_path(lines[index], "+++ ")?;
        index += 1;
        if old_path == "/dev/null" || new_path == "/dev/null" {
            bail!("file creation/deletion patches are not supported; use write_file for new files");
        }
        let old_target_path = normalize_diff_path(&old_path)?;
        let target_path = normalize_diff_path(&new_path)?;
        if old_target_path != target_path {
            bail!("rename patches are not supported: {old_target_path} -> {target_path}");
        }
        let mut hunks = Vec::new();

        while index < lines.len() {
            if lines[index].starts_with("--- ") {
                break;
            }
            if lines[index].starts_with("@@ ") {
                let (old_start, old_count, _new_start, new_count) =
                    parse_hunk_header(lines[index])?;
                index += 1;
                let mut hunk_lines = Vec::new();
                let mut actual_old_count = 0usize;
                let mut actual_new_count = 0usize;
                while index < lines.len()
                    && !lines[index].starts_with("@@ ")
                    && !lines[index].starts_with("--- ")
                {
                    let line = lines[index];
                    if line.starts_with("\\ No newline at end of file") {
                        index += 1;
                        continue;
                    }
                    let Some((marker, content)) = line.split_at_checked(1) else {
                        bail!("invalid hunk line");
                    };
                    match marker {
                        " " => {
                            actual_old_count += 1;
                            actual_new_count += 1;
                            hunk_lines.push(HunkLine::Context(content.to_string()));
                        }
                        "-" => {
                            actual_old_count += 1;
                            hunk_lines.push(HunkLine::Remove(content.to_string()));
                        }
                        "+" => {
                            actual_new_count += 1;
                            hunk_lines.push(HunkLine::Add(content.to_string()));
                        }
                        _ => bail!("invalid hunk marker {marker:?}"),
                    }
                    index += 1;
                }
                if hunk_lines.is_empty() {
                    bail!("hunk has no body lines");
                }
                if actual_old_count != old_count || actual_new_count != new_count {
                    bail!(
                        "hunk line count mismatch: header expects -{old_count} +{new_count}, body has -{actual_old_count} +{actual_new_count}"
                    );
                }
                hunks.push(Hunk {
                    old_start,
                    lines: hunk_lines,
                });
            } else {
                bail!("unexpected unified diff line: {}", lines[index]);
            }
        }

        if hunks.is_empty() {
            bail!("patch for {target_path} has no hunks");
        }
        patches.push(FilePatch { target_path, hunks });
    }

    if patches.is_empty() {
        bail!("no unified diff file patches found");
    }
    Ok(patches)
}

fn parse_diff_path(line: &str, prefix: &str) -> Result<String> {
    let raw = line
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow!("missing diff path prefix {prefix}"))?;
    Ok(raw.split_whitespace().next().unwrap_or("").to_string())
}

fn normalize_diff_path(path: &str) -> Result<String> {
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    validate_model_path(path, &[])?;
    Ok(path.to_string())
}

fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize)> {
    let rest = header
        .strip_prefix("@@ -")
        .ok_or_else(|| anyhow!("invalid hunk header"))?;
    let (old, rest) = rest
        .split_once(" +")
        .ok_or_else(|| anyhow!("invalid hunk header"))?;
    let (new, _) = rest
        .split_once(" @@")
        .ok_or_else(|| anyhow!("invalid hunk header"))?;
    let (old_start, old_count) = parse_hunk_range(old)?;
    let (new_start, new_count) = parse_hunk_range(new)?;
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_hunk_range(value: &str) -> Result<(usize, usize)> {
    if let Some((start, count)) = value.split_once(',') {
        Ok((start.parse()?, count.parse()?))
    } else {
        Ok((value.parse()?, 1))
    }
}

fn apply_hunks_to_text(original: &str, hunks: &[Hunk]) -> Result<String> {
    let source_lines = split_lines_preserving_endings(original);
    let mut output = Vec::new();
    let mut source_index = 0usize;

    for hunk in hunks {
        let target_index = hunk.old_start.saturating_sub(1);
        if target_index < source_index || target_index > source_lines.len() {
            bail!("hunk starts outside source bounds");
        }
        output.extend_from_slice(&source_lines[source_index..target_index]);
        source_index = target_index;

        for line in &hunk.lines {
            match line {
                HunkLine::Context(content) => {
                    assert_source_line(&source_lines, source_index, content)?;
                    output.push(source_lines[source_index].clone());
                    source_index += 1;
                }
                HunkLine::Remove(content) => {
                    assert_source_line(&source_lines, source_index, content)?;
                    source_index += 1;
                }
                HunkLine::Add(content) => {
                    output.push(format!("{content}\n"));
                }
            }
        }
    }

    output.extend_from_slice(&source_lines[source_index..]);
    Ok(output.concat())
}

fn split_lines_preserving_endings(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = text
        .split_inclusive('\n')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !text.ends_with('\n') {
        let trailing = text.rsplit_once('\n').map(|(_, tail)| tail).unwrap_or(text);
        if !trailing.is_empty()
            && lines
                .last()
                .map(|line| line.ends_with('\n'))
                .unwrap_or(false)
        {
            lines.push(trailing.to_string());
        }
    }
    lines
}

fn assert_source_line(source_lines: &[String], index: usize, expected: &str) -> Result<()> {
    let actual = source_lines
        .get(index)
        .ok_or_else(|| anyhow!("hunk references line {} beyond end of file", index + 1))?;
    if actual.trim_end_matches('\n') != expected {
        bail!(
            "patch context mismatch at line {}: expected {:?}, found {:?}",
            index + 1,
            expected,
            actual.trim_end_matches('\n')
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use serde_json::json;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    fn fixture_agent(id: &str) -> (crate::config::EffectiveConfig, AgentProfile) {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let agent = config.agents.get(id).unwrap().clone();
        (config, agent)
    }

    fn action_context(
        dir: &std::path::Path,
        config: &crate::config::EffectiveConfig,
    ) -> ActionExecutionContext {
        ActionExecutionContext {
            working_directory: dir.to_path_buf(),
            workspace: config.workspace.clone(),
            approval_mode: config.approval_mode.clone(),
            command_timeout: Some(Duration::from_secs(5)),
            user_prompt: None,
        }
    }

    #[test]
    fn reviewer_cannot_edit() {
        let (config, reviewer) = fixture_agent("reviewer");
        let request = ActionRequest {
            schema_version: 1,
            action_id: "a".to_string(),
            step_id: "s".to_string(),
            kind: ActionKind::WriteFile,
            params: json!({"path": "src/lib.rs"}),
        };
        let decision = validate_action_request(
            &reviewer,
            &config.workspace,
            &config.approval_mode,
            &request,
        );
        assert!(matches!(decision, ActionDecision::Denied(_)));
    }

    #[test]
    fn designer_cannot_run_commands() {
        let (config, designer) = fixture_agent("designer");
        let request = ActionRequest {
            schema_version: 1,
            action_id: "a".to_string(),
            step_id: "s".to_string(),
            kind: ActionKind::RunCommand,
            params: json!({"command": "pwd"}),
        };

        let decision = validate_action_request(
            &designer,
            &config.workspace,
            &config.approval_mode,
            &request,
        );

        assert!(
            matches!(decision, ActionDecision::Denied(reason) if reason.contains("lacks required capability"))
        );
    }

    #[test]
    fn tool_allowlist_can_deny_actions_beneath_a_capability() {
        let (config, mut explorer) = fixture_agent("explorer");
        explorer.tools = Some(vec![ToolName::ReadFile]);
        let request = ActionRequest {
            schema_version: 1,
            action_id: "a".to_string(),
            step_id: "s".to_string(),
            kind: ActionKind::SearchText,
            params: json!({"path": ".", "query": "needle"}),
        };

        let decision = validate_action_request(
            &explorer,
            &config.workspace,
            &config.approval_mode,
            &request,
        );

        assert!(
            matches!(decision, ActionDecision::Denied(reason) if reason.contains("not allowed to use tool"))
        );
    }

    #[test]
    fn path_scope_rejects_traversal() {
        let error = validate_model_path("../secret", &[]).unwrap_err();
        assert!(error.to_string().contains("traversal"));
    }

    #[test]
    fn command_policy_classifies_vcs_mutations_for_approval() {
        assert_eq!(
            classify_command("git status --short"),
            CommandClassification::Allow
        );
        assert_eq!(
            classify_command("git branch --show-current"),
            CommandClassification::Allow
        );
        assert_eq!(
            classify_command("git rev-parse --abbrev-ref HEAD"),
            CommandClassification::Allow
        );
        assert_eq!(
            classify_command("git remote -v"),
            CommandClassification::Allow
        );
        assert_eq!(
            classify_command("rg \"todo|fixme\" src"),
            CommandClassification::Allow
        );
        assert_eq!(
            classify_command("git rev-parse --abbrev-ref HEAD && rm -rf target"),
            CommandClassification::Approve
        );
        assert_eq!(
            classify_command("rg todo src | cat"),
            CommandClassification::Approve
        );
        assert_eq!(
            classify_command("git push origin main"),
            CommandClassification::Approve
        );
        assert_eq!(classify_command("rm -rf /"), CommandClassification::Deny);
    }

    #[test]
    fn vcs_mutations_are_detected_separately_from_safe_git_inspection() {
        assert!(!is_vcs_mutation("git status --short"));
        assert!(!is_vcs_mutation("git diff"));
        assert!(!is_vcs_mutation("git branch --show-current"));
        assert!(!is_vcs_mutation("git remote get-url origin"));
        assert!(is_vcs_mutation("git commit -m test"));
        assert!(is_vcs_mutation("git push origin main"));
        assert!(is_vcs_mutation("git branch feature/new"));
        assert!(is_vcs_mutation("git reset --hard HEAD~1"));
    }

    #[test]
    fn commit_request_allows_default_staging_command() {
        let prompt = Some("commit and push the current changes".to_string());

        assert!(vcs_action_explicitly_requested(&prompt, "git add -u"));
        assert!(vcs_action_explicitly_requested(&prompt, "git add ."));
        assert!(vcs_action_explicitly_requested(
            &prompt,
            "git commit --no-verify -m \"feat: add chat\""
        ));
        assert!(vcs_action_explicitly_requested(
            &prompt,
            "git push -u origin feat/chat"
        ));
        assert!(!vcs_action_explicitly_requested(
            &Some("inspect the branch state".to_string()),
            "git add ."
        ));
    }

    #[test]
    fn read_only_shell_suffix_requires_approval_in_normal_mode() {
        assert!(matches!(
            decision_for_command(
                "git rev-parse --abbrev-ref HEAD && rm -rf target",
                &ApprovalMode::Normal
            ),
            ActionDecision::RequiresApproval(_)
        ));
    }

    #[test]
    fn yolo_skips_approval_but_normal_prompts() {
        assert_eq!(
            decision_for_command("git commit -m test", &ApprovalMode::Yolo),
            ActionDecision::Allowed
        );
        assert!(matches!(
            decision_for_command("git commit -m test", &ApprovalMode::Normal),
            ActionDecision::RequiresApproval(_)
        ));
    }

    #[tokio::test]
    async fn executes_read_list_and_search_actions() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "fn main() {}\n// needle\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let explorer = config.agents["explorer"].clone();
        let context = action_context(dir.path(), &config);

        let read = ActionRequest {
            schema_version: 1,
            action_id: "read".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ReadFile,
            params: json!({ "path": "src/lib.rs" }),
        };
        let result = execute_action_request(&explorer, &context, &read).await;
        assert_eq!(result.status, ActionStatus::Completed);
        assert!(result
            .content
            .as_ref()
            .unwrap()
            .get("content")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("needle"));

        let list = ActionRequest {
            schema_version: 1,
            action_id: "list".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ListFiles,
            params: json!({ "path": "." }),
        };
        let result = execute_action_request(&explorer, &context, &list).await;
        assert_eq!(result.status, ActionStatus::Completed);
        assert!(result.content.unwrap().to_string().contains("src/lib.rs"));

        let search = ActionRequest {
            schema_version: 1,
            action_id: "search".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::SearchText,
            params: json!({ "path": ".", "query": "needle" }),
        };
        let result = execute_action_request(&explorer, &context, &search).await;
        assert_eq!(result.status, ActionStatus::Completed);
        assert!(result.content.unwrap().to_string().contains("\"line\":2"));
    }

    #[tokio::test]
    async fn execute_write_file_creates_new_files_only() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let fixer = config.agents["fixer"].clone();
        let context = action_context(dir.path(), &config);
        let request = ActionRequest {
            schema_version: 1,
            action_id: "write".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::WriteFile,
            params: json!({ "path": "notes/result.txt", "content": "created\n" }),
        };
        let result = execute_action_request(&fixer, &context, &request).await;
        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/result.txt")).unwrap(),
            "created\n"
        );

        let result = execute_action_request(&fixer, &context, &request).await;
        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result
            .diagnostic
            .unwrap()
            .contains("refuses to overwrite existing files"));
    }

    #[tokio::test]
    async fn execute_apply_patch_is_atomic_for_context_mismatch() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), "one\ntwo\nthree\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let fixer = config.agents["fixer"].clone();
        let context = action_context(dir.path(), &config);
        let patch = "--- a/file.txt\n+++ b/file.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let request = ActionRequest {
            schema_version: 1,
            action_id: "patch".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ApplyPatch,
            params: json!({ "diff": patch }),
        };
        let result = execute_action_request(&fixer, &context, &request).await;
        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );

        let stale_patch =
            "--- a/file.txt\n+++ b/file.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+dos\n three\n";
        let request = ActionRequest {
            schema_version: 1,
            action_id: "patch".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ApplyPatch,
            params: json!({ "diff": stale_patch }),
        };
        let result = execute_action_request(&fixer, &context, &request).await;
        assert_eq!(result.status, ActionStatus::Failed);
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
    }

    #[test]
    fn apply_patch_policy_rejects_invalid_targets_and_structure() {
        let (config, fixer) = fixture_agent("fixer");
        let traversal_patch =
            "--- a/../secret.txt\n+++ b/../secret.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let request = ActionRequest {
            schema_version: 1,
            action_id: "patch".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ApplyPatch,
            params: json!({ "diff": traversal_patch }),
        };

        let decision =
            validate_action_request(&fixer, &config.workspace, &config.approval_mode, &request);

        assert!(matches!(decision, ActionDecision::Denied(reason) if reason.contains("traversal")));
    }

    #[test]
    fn apply_patch_policy_rejects_rename_and_hunk_count_mismatch() {
        let (config, fixer) = fixture_agent("fixer");
        let rename_patch = "--- a/old.txt\n+++ b/new.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let request = ActionRequest {
            schema_version: 1,
            action_id: "patch".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ApplyPatch,
            params: json!({ "diff": rename_patch }),
        };
        let decision =
            validate_action_request(&fixer, &config.workspace, &config.approval_mode, &request);
        assert!(
            matches!(decision, ActionDecision::Denied(reason) if reason.contains("rename patches"))
        );

        let mismatched_patch = "--- a/file.txt\n+++ b/file.txt\n@@ -1,2 +1,1 @@\n-old\n+new\n";
        let request = ActionRequest {
            schema_version: 1,
            action_id: "patch".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::ApplyPatch,
            params: json!({ "diff": mismatched_patch }),
        };
        let decision =
            validate_action_request(&fixer, &config.workspace, &config.approval_mode, &request);
        assert!(
            matches!(decision, ActionDecision::Denied(reason) if reason.contains("hunk line count mismatch"))
        );
    }

    #[tokio::test]
    async fn execute_run_command_captures_status_and_output() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let fixer = config.agents["fixer"].clone();
        let context = action_context(dir.path(), &config);
        let request = ActionRequest {
            schema_version: 1,
            action_id: "command".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::RunCommand,
            params: json!({ "command": "pwd" }),
        };
        let result = execute_action_request(&fixer, &context, &request).await;
        assert_eq!(result.status, ActionStatus::Completed);
        assert!(result
            .content
            .as_ref()
            .unwrap()
            .get("stdout")
            .unwrap()
            .as_str()
            .unwrap()
            .contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn vcs_mutations_require_explicit_user_prompt_even_in_yolo() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let fixer = config.agents["fixer"].clone();
        let mut context = action_context(dir.path(), &config);
        let request = ActionRequest {
            schema_version: 1,
            action_id: "command".to_string(),
            step_id: "step".to_string(),
            kind: ActionKind::RunCommand,
            params: json!({ "command": "git commit -m test" }),
        };
        let result = execute_action_request(&fixer, &context, &request).await;
        assert_eq!(result.status, ActionStatus::Denied);
        assert!(result.diagnostic.unwrap().contains("explicit user request"));

        context.user_prompt = Some("commit the current changes".to_string());
        let result = execute_action_request(&fixer, &context, &request).await;
        assert_ne!(result.status, ActionStatus::Denied);
        assert!(!result
            .diagnostic
            .unwrap_or_default()
            .contains("explicit user request"));
    }
}
