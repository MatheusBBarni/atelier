use super::{ChatLineStyle, ChatLineView};

const MAX_PREVIEW_LINES: usize = 12;
const MAX_PREVIEW_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffPreviewView {
    pub files: Vec<String>,
    pub added: usize,
    pub removed: usize,
    pub hunks: usize,
    pub preview_lines: Vec<ChatLineView>,
    pub truncated: bool,
}

pub fn build_diff_preview(diff: &str) -> DiffPreviewView {
    let mut files = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut hunks = 0usize;
    let mut preview_lines = Vec::new();
    let mut scanned_bytes = 0usize;
    let mut truncated = false;

    for line in diff.lines() {
        scanned_bytes = scanned_bytes.saturating_add(line.len()).saturating_add(1);
        if scanned_bytes > MAX_PREVIEW_BYTES {
            truncated = true;
            break;
        }

        if let Some(path) = line.strip_prefix("+++ b/") {
            push_unique(&mut files, path);
        } else if let Some(path) = line.strip_prefix("--- a/") {
            push_unique(&mut files, path);
        } else if line.starts_with("@@") {
            hunks += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }

        if is_preview_line(line) {
            if preview_lines.len() < MAX_PREVIEW_LINES {
                preview_lines.push(preview_line(line));
            } else {
                truncated = true;
            }
        }
    }

    DiffPreviewView {
        files,
        added,
        removed,
        hunks,
        preview_lines,
        truncated,
    }
}

/// A short, read-only preview of a newly-written file's contents (a created
/// file is not a diff, so its lines are shown as neutral context).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentPreviewView {
    pub preview_lines: Vec<ChatLineView>,
    pub truncated: bool,
}

pub fn build_content_preview(content: &str) -> ContentPreviewView {
    let mut preview_lines = Vec::new();
    let mut scanned_bytes = 0usize;
    let mut truncated = false;

    for line in content.lines() {
        scanned_bytes = scanned_bytes.saturating_add(line.len()).saturating_add(1);
        if scanned_bytes > MAX_PREVIEW_BYTES || preview_lines.len() >= MAX_PREVIEW_LINES {
            truncated = true;
            break;
        }
        preview_lines.push(ChatLineView {
            style: ChatLineStyle::DiffContext,
            text: concise(line, 220),
        });
    }

    ContentPreviewView {
        preview_lines,
        truncated,
    }
}

fn push_unique(files: &mut Vec<String>, path: &str) {
    if path == "/dev/null" || files.iter().any(|file| file == path) {
        return;
    }
    files.push(path.to_string());
}

fn is_preview_line(line: &str) -> bool {
    line.starts_with("@@")
        || (line.starts_with('+') && !line.starts_with("+++"))
        || (line.starts_with('-') && !line.starts_with("---"))
        || line.starts_with(' ')
}

fn preview_line(line: &str) -> ChatLineView {
    let style = if line.starts_with('+') && !line.starts_with("+++") {
        ChatLineStyle::DiffAdd
    } else if line.starts_with('-') && !line.starts_with("---") {
        ChatLineStyle::DiffRemove
    } else {
        ChatLineStyle::DiffContext
    };
    ChatLineView {
        style,
        text: concise(line, 220),
    }
}

fn concise(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
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
    fn build_diff_preview_counts_small_patch() {
        let preview = build_diff_preview(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n same\n",
        );

        assert_eq!(preview.files, vec!["src/lib.rs"]);
        assert_eq!(preview.added, 1);
        assert_eq!(preview.removed, 1);
        assert_eq!(preview.hunks, 1);
    }

    #[test]
    fn build_diff_preview_truncates_large_patch() {
        let diff = format!(
            "--- a/file\n+++ b/file\n@@ -1,1 +1,20 @@\n{}",
            (0..40)
                .map(|index| format!("+line {index}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let preview = build_diff_preview(&diff);

        assert!(preview.truncated);
    }
}
