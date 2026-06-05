use super::{ChatDetailRef, ChatItemStatus, ChatLineView, ChatSeverity};

const MAX_DETAIL_CHARS: usize = 2 * 1024;
const MAX_BODY_LINES: usize = 12;
const MAX_SCANNED_LINES: usize = 400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSummaryInput {
    pub command: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSummaryView {
    pub category: CommandCategory,
    pub title: String,
    pub status: ChatItemStatus,
    pub severity: ChatSeverity,
    pub body: Vec<ChatLineView>,
    pub details: Vec<ChatDetailRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandCategory {
    Cargo,
    KnownReadOnly,
    ApprovalRequired,
    Unknown,
}

pub fn summarize_command(input: CommandSummaryInput) -> CommandSummaryView {
    let category = classify_command_for_summary(&input.command);
    let status = status_from_exit(input.exit_code, input.diagnostic.as_deref());
    let severity = severity_for_status(&status);
    let title = command_title(&category, &input.command);
    let mut body = match category {
        CommandCategory::Cargo => cargo_summary_lines(&input),
        CommandCategory::KnownReadOnly => known_command_lines(&input),
        CommandCategory::ApprovalRequired => approval_command_lines(&input),
        CommandCategory::Unknown => fallback_command_lines(&input),
    };

    if body.is_empty() {
        body.push(ChatLineView::muted(exit_summary(
            input.exit_code,
            input.diagnostic.as_deref(),
        )));
    }
    body.truncate(MAX_BODY_LINES);

    CommandSummaryView {
        category,
        title,
        status,
        severity,
        body,
        details: command_details(&input),
    }
}

pub fn classify_command_for_summary(command: &str) -> CommandCategory {
    let lower = command.trim().to_ascii_lowercase();
    if cargo_summary_command(&lower) {
        return CommandCategory::Cargo;
    }
    if approval_command(&lower) {
        return CommandCategory::ApprovalRequired;
    }
    if known_read_only_command(&lower) {
        return CommandCategory::KnownReadOnly;
    }
    CommandCategory::Unknown
}

fn cargo_summary_command(lower: &str) -> bool {
    [
        "cargo test",
        "cargo check",
        "cargo clippy",
        "cargo build",
        "cargo fmt",
    ]
    .iter()
    .any(|prefix| command_has_prefix(lower, prefix))
}

fn known_read_only_command(lower: &str) -> bool {
    [
        "pwd",
        "ls",
        "rg ",
        "grep ",
        "find ",
        "sed -n",
        "cat ",
        "wc ",
        "git status",
        "git diff",
        "git log",
        "git show",
    ]
    .iter()
    .any(|prefix| command_has_prefix(lower, prefix))
}

fn approval_command(lower: &str) -> bool {
    [
        "cargo install",
        "brew install",
        "brew upgrade",
        "npm install",
        "pnpm install",
        "yarn add",
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
        "curl ",
        "wget ",
    ]
    .iter()
    .any(|prefix| command_has_prefix(lower, prefix))
}

fn command_has_prefix(lower_command: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end();
    lower_command == prefix || lower_command.starts_with(&format!("{prefix} "))
}

fn command_title(category: &CommandCategory, command: &str) -> String {
    let command = concise(command, 120);
    match category {
        CommandCategory::Cargo => format!("Cargo: {command}"),
        CommandCategory::KnownReadOnly => format!("Command: {command}"),
        CommandCategory::ApprovalRequired => format!("Approval command: {command}"),
        CommandCategory::Unknown => format!("Command: {command}"),
    }
}

fn status_from_exit(exit_code: Option<i64>, diagnostic: Option<&str>) -> ChatItemStatus {
    match exit_code {
        Some(0) => ChatItemStatus::Completed,
        Some(_) => ChatItemStatus::Failed,
        None if diagnostic.is_some() => ChatItemStatus::Failed,
        None => ChatItemStatus::Completed,
    }
}

fn severity_for_status(status: &ChatItemStatus) -> ChatSeverity {
    match status {
        ChatItemStatus::Completed => ChatSeverity::Success,
        ChatItemStatus::Failed => ChatSeverity::Error,
        ChatItemStatus::Denied | ChatItemStatus::WaitingApproval => ChatSeverity::Warning,
        _ => ChatSeverity::Info,
    }
}

fn cargo_summary_lines(input: &CommandSummaryInput) -> Vec<ChatLineView> {
    let mut lines = Vec::new();
    lines.push(ChatLineView::code(format!(
        "$ {}",
        concise(&input.command, 160)
    )));
    for line in output_lines(input)
        .into_iter()
        .filter(|line| is_stable_cargo_line(line))
        .take(MAX_BODY_LINES.saturating_sub(2))
    {
        lines.push(line_for_output(&line));
    }
    lines.push(ChatLineView::muted(exit_summary(
        input.exit_code,
        input.diagnostic.as_deref(),
    )));
    lines
}

fn known_command_lines(input: &CommandSummaryInput) -> Vec<ChatLineView> {
    let mut lines = vec![ChatLineView::code(format!(
        "$ {}",
        concise(&input.command, 160)
    ))];
    for line in output_lines(input).into_iter().take(4) {
        lines.push(ChatLineView::plain(concise(&line, 180)));
    }
    lines.push(ChatLineView::muted(exit_summary(
        input.exit_code,
        input.diagnostic.as_deref(),
    )));
    lines
}

fn approval_command_lines(input: &CommandSummaryInput) -> Vec<ChatLineView> {
    let mut lines = vec![ChatLineView::code(format!(
        "$ {}",
        concise(&input.command, 160)
    ))];
    if let Some(diagnostic) = input.diagnostic.as_deref() {
        lines.push(ChatLineView::warning(concise(diagnostic, 220)));
    }
    lines.push(ChatLineView::muted(exit_summary(
        input.exit_code,
        input.diagnostic.as_deref(),
    )));
    lines
}

fn fallback_command_lines(input: &CommandSummaryInput) -> Vec<ChatLineView> {
    let mut lines = vec![ChatLineView::code(format!(
        "$ {}",
        concise(&input.command, 160)
    ))];
    if let Some(diagnostic) = input.diagnostic.as_deref() {
        lines.push(ChatLineView::error(concise(diagnostic, 220)));
    }
    lines.push(ChatLineView::muted(exit_summary(
        input.exit_code,
        input.diagnostic.as_deref(),
    )));
    lines
}

fn output_lines(input: &CommandSummaryInput) -> Vec<String> {
    input
        .stdout
        .iter()
        .chain(input.stderr.iter())
        .flat_map(|stream| stream.lines())
        .take(MAX_SCANNED_LINES)
        .filter_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect()
}

fn is_stable_cargo_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("running ")
        || trimmed.starts_with("test result:")
        || trimmed.starts_with("failures:")
        || trimmed.starts_with("error:")
        || trimmed.starts_with("Finished ")
        || trimmed.starts_with("Compiling ")
        || trimmed.starts_with("Checking ")
        || trimmed.starts_with("Doc-tests ")
        || trimmed.starts_with("     Running ")
        || trimmed.starts_with("   Doc-tests ")
}

fn line_for_output(line: &str) -> ChatLineView {
    let text = concise(line, 220);
    if line.trim().starts_with("error:") {
        ChatLineView::error(text)
    } else if line.trim().starts_with("failures:") {
        ChatLineView::warning(text)
    } else {
        ChatLineView::plain(text)
    }
}

fn exit_summary(exit_code: Option<i64>, diagnostic: Option<&str>) -> String {
    let exit = exit_code
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| "exit unavailable".to_string());
    match diagnostic {
        Some(diagnostic) => format!("{exit}; {}", concise(diagnostic, 160)),
        None => exit,
    }
}

fn command_details(input: &CommandSummaryInput) -> Vec<ChatDetailRef> {
    let mut details = Vec::new();
    if let Some(stdout) = input.stdout.as_deref().filter(|stdout| !stdout.is_empty()) {
        details.push(inline_detail("stdout", stdout));
    }
    if let Some(stderr) = input.stderr.as_deref().filter(|stderr| !stderr.is_empty()) {
        details.push(inline_detail("stderr", stderr));
    }
    details
}

fn inline_detail(label: &str, content: &str) -> ChatDetailRef {
    let truncated = content.chars().count() > MAX_DETAIL_CHARS;
    let content = if truncated {
        content.chars().take(MAX_DETAIL_CHARS).collect()
    } else {
        content.to_string()
    };
    ChatDetailRef::Inline {
        label: label.to_string(),
        content,
        truncated,
    }
}

fn concise(text: &str, max_chars: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_command_extracts_passing_cargo_test_result() {
        let summary = summarize_command(CommandSummaryInput {
            command: "cargo test".to_string(),
            exit_code: Some(0),
            stdout: Some("running 1 test\ntest result: ok. 1 passed; 0 failed\n".to_string()),
            stderr: None,
            diagnostic: None,
        });

        assert!(summary
            .body
            .iter()
            .any(|line| line.text.contains("test result: ok")));
    }

    #[test]
    fn summarize_command_marks_failed_cargo_output_as_error() {
        let summary = summarize_command(CommandSummaryInput {
            command: "cargo test".to_string(),
            exit_code: Some(101),
            stdout: Some("failures:\nerror: test failed\n".to_string()),
            stderr: None,
            diagnostic: Some("command returned a non-zero exit status".to_string()),
        });

        assert_eq!(summary.severity, ChatSeverity::Error);
    }

    #[test]
    fn summarize_command_keeps_pwd_output_short() {
        let summary = summarize_command(CommandSummaryInput {
            command: "pwd".to_string(),
            exit_code: Some(0),
            stdout: Some("/tmp/workspace\n".to_string()),
            stderr: None,
            diagnostic: None,
        });

        assert!(summary
            .body
            .iter()
            .any(|line| line.text == "/tmp/workspace"));
    }
}
