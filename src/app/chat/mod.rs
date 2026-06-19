pub mod command_summary;
pub mod diff_preview;
mod projection;

pub use projection::ChatProjection;
pub(crate) use projection::FIRST_APPROVAL_EXPLAINER;

use crate::history::HistoryStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatItemView {
    pub id: String,
    pub lifecycle_key: Option<ChatLifecycleKey>,
    pub kind: ChatItemKind,
    pub status: ChatItemStatus,
    pub severity: ChatSeverity,
    pub title: String,
    pub summary: Option<String>,
    pub body: Vec<ChatLineView>,
    pub details: Vec<ChatDetailRef>,
    pub source: ChatSourceRef,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ChatItemKind {
    UserPrompt,
    RoutingDecision,
    AgentProgress,
    ActionRequested,
    CommandResult,
    FileEdit,
    Approval,
    Clarification,
    /// A governance decision (e.g. the turn-1 early-abort) paused for the user
    /// to accept or reject before the run proceeds (governance spine, ADR-003).
    GovernanceDecision,
    Diagnostic,
    SkillContext,
    AgentResult,
    RunSummary,
    /// A lifecycle-hook execution (ADR-003), evolving from `hook_started` →
    /// `hook_completed` as a single transcript item: action, status, duration,
    /// and exit code. Recorded for transparency; never re-enters dispatch.
    HookInvocation,
    /// Synthetic branded welcome item, injected at startup (ADR-005). Not an
    /// orchestration event; carries no lifecycle key and never updates.
    Welcome,
    /// A sub-task DAG run rendered as one durable, in-place evolving item: the
    /// whole graph (every node's status + scope) and its WaitingApproval gate
    /// (ADR-005). Per-node live items are suppressed in favor of this one view.
    Plan,
    /// The externally-grounded auto-verification loop rendered as ONE evolving
    /// item ("verifying… → Round 1/2: FAIL → PASS"), keyed by `Grade { run_id }`
    /// (self-grading retry loop, ADR-003). Successive `grade_round` events
    /// collapse into this item with a visible round counter.
    GradeLoop,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatItemStatus {
    Pending,
    Running,
    WaitingApproval,
    WaitingForUser,
    Completed,
    Denied,
    Failed,
    Interrupted,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatLineView {
    pub style: ChatLineStyle,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatLineStyle {
    Plain,
    Muted,
    Code,
    DiffAdd,
    DiffRemove,
    DiffContext,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ChatDetailRef {
    HistoryEvent {
        event_id: String,
        label: String,
    },
    Artifact {
        label: String,
        artifact_id: Option<String>,
        path: Option<String>,
        media_type: Option<String>,
    },
    Inline {
        label: String,
        content: String,
        truncated: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSourceRef {
    pub event_ids: Vec<String>,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub action_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ChatLifecycleKey {
    Prompt {
        run_id: String,
    },
    Run {
        run_id: String,
    },
    Clarification {
        run_id: String,
        question_id: String,
    },
    /// A governance decision paused for user input; one chat item per decision
    /// (governance spine, ADR-003).
    GovernanceDecision {
        run_id: String,
        decision_id: String,
    },
    Workflow {
        run_id: String,
    },
    Step {
        run_id: String,
        step_id: String,
        item_kind: ChatItemKind,
    },
    Action {
        run_id: String,
        step_id: String,
        action_id: String,
    },
    FollowUp {
        follow_up_id: String,
    },
    /// One hook execution within a run (ADR-003): `hook_started` and
    /// `hook_completed` for the same matched handler collapse into one item.
    Hook {
        run_id: String,
        handler_index: usize,
        event: String,
    },
    /// One sub-task DAG run's single evolving Plan item, keyed by `graph_id`
    /// (ADR-005). Every graph/node event re-renders into this one item.
    Plan {
        graph_id: String,
    },
    /// One run's single evolving grade-loop item (self-grading retry loop): every
    /// `grade_round` event for the run collapses into this item.
    Grade {
        run_id: String,
    },
}

impl ChatLineView {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            style: ChatLineStyle::Plain,
            text: text.into(),
        }
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self {
            style: ChatLineStyle::Muted,
            text: text.into(),
        }
    }

    pub fn code(text: impl Into<String>) -> Self {
        Self {
            style: ChatLineStyle::Code,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            style: ChatLineStyle::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            style: ChatLineStyle::Error,
            text: text.into(),
        }
    }
}

impl ChatSourceRef {
    pub fn from_event(
        event_id: impl Into<String>,
        run_id: Option<String>,
        step_id: Option<String>,
        action_id: Option<String>,
    ) -> Self {
        Self {
            event_ids: vec![event_id.into()],
            run_id,
            step_id,
            action_id,
        }
    }

    pub fn merge_event(&mut self, event_id: &str) {
        if !self.event_ids.iter().any(|existing| existing == event_id) {
            self.event_ids.push(event_id.to_string());
        }
    }
}

impl ChatLifecycleKey {
    pub fn item_id(&self) -> String {
        match self {
            ChatLifecycleKey::Prompt { run_id } => format!("chat:prompt:{run_id}"),
            ChatLifecycleKey::Run { run_id } => format!("chat:run:{run_id}"),
            ChatLifecycleKey::Clarification {
                run_id,
                question_id,
            } => format!("chat:clarification:{run_id}:{question_id}"),
            ChatLifecycleKey::GovernanceDecision {
                run_id,
                decision_id,
            } => format!("chat:governance_decision:{run_id}:{decision_id}"),
            ChatLifecycleKey::Workflow { run_id } => format!("chat:workflow:{run_id}"),
            ChatLifecycleKey::Step {
                run_id,
                step_id,
                item_kind,
            } => format!("chat:step:{run_id}:{step_id}:{}", item_kind.slug()),
            ChatLifecycleKey::Action {
                run_id,
                step_id,
                action_id,
            } => format!("chat:action:{run_id}:{step_id}:{action_id}"),
            ChatLifecycleKey::FollowUp { follow_up_id } => {
                format!("chat:follow_up:{follow_up_id}")
            }
            ChatLifecycleKey::Hook {
                run_id,
                handler_index,
                event,
            } => format!("chat:hook:{run_id}:{handler_index}:{event}"),
            ChatLifecycleKey::Plan { graph_id } => format!("chat:plan:{graph_id}"),
            ChatLifecycleKey::Grade { run_id } => format!("chat:grade:{run_id}"),
        }
    }
}

impl ChatItemKind {
    pub fn slug(&self) -> &'static str {
        match self {
            ChatItemKind::UserPrompt => "user_prompt",
            ChatItemKind::RoutingDecision => "routing_decision",
            ChatItemKind::AgentProgress => "agent_progress",
            ChatItemKind::ActionRequested => "action_requested",
            ChatItemKind::CommandResult => "command_result",
            ChatItemKind::FileEdit => "file_edit",
            ChatItemKind::Approval => "approval",
            ChatItemKind::Clarification => "clarification",
            ChatItemKind::GovernanceDecision => "governance_decision",
            ChatItemKind::Diagnostic => "diagnostic",
            ChatItemKind::SkillContext => "skill_context",
            ChatItemKind::AgentResult => "agent_result",
            ChatItemKind::RunSummary => "run_summary",
            ChatItemKind::HookInvocation => "hook_invocation",
            ChatItemKind::Welcome => "welcome",
            ChatItemKind::Plan => "plan",
            ChatItemKind::GradeLoop => "grade_loop",
        }
    }
}

impl ChatItemView {
    /// The synthetic welcome chat item injected at startup (ADR-005). A stable
    /// marker with no lifecycle key; its facts/wordmark are rendered from live
    /// state by `tui::welcome`, so the item itself carries no body.
    pub fn welcome() -> Self {
        Self {
            id: "chat:welcome".to_string(),
            lifecycle_key: None,
            kind: ChatItemKind::Welcome,
            status: ChatItemStatus::Completed,
            severity: ChatSeverity::Info,
            title: "Atelier".to_string(),
            summary: None,
            body: Vec::new(),
            details: Vec::new(),
            source: ChatSourceRef {
                event_ids: Vec::new(),
                run_id: None,
                step_id: None,
                action_id: None,
            },
            updated_at: String::new(),
        }
    }
}

impl ChatItemStatus {
    pub fn label(&self) -> &'static str {
        match self {
            ChatItemStatus::Pending => "pending",
            ChatItemStatus::Running => "running",
            ChatItemStatus::WaitingApproval => "waiting approval",
            ChatItemStatus::WaitingForUser => "waiting for clarification",
            ChatItemStatus::Completed => "completed",
            ChatItemStatus::Denied => "denied",
            ChatItemStatus::Failed => "failed",
            ChatItemStatus::Interrupted => "interrupted",
            ChatItemStatus::Skipped => "skipped",
        }
    }
}

/// A read-only render of a stored session for the browser preview pane (task_08).
/// Built purely from the session's history via [`ChatProjection::rebuild`] —
/// deliberately WITHOUT the welcome item or the live-step / pending-approval
/// overlays that `App::sync_chat_items` adds to the live transcript — with every
/// rendered text field sanitized of terminal control/ANSI sequences (ADR-004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPreview {
    pub session_id: String,
    pub items: Vec<ChatItemView>,
}

/// Build a read-only [`SessionPreview`] for `session_id` under `root` (the
/// `.atelier` data root). Opens the session (schema-validated, task_02), folds
/// its log through `ChatProjection::rebuild` (incl. the `run_interrupted` /
/// `session_resumed` kinds, task_04), and sanitizes the projected items. A pure
/// read — it never touches live `App`/projection state.
pub fn build_session_preview(root: &Path, session_id: &str) -> Result<SessionPreview> {
    let store = HistoryStore::open(root, session_id)?;
    let events = store.read_events()?;
    let items = ChatProjection::rebuild(&events)
        .items()
        .iter()
        .cloned()
        .map(sanitize_chat_item)
        .collect();
    Ok(SessionPreview {
        session_id: session_id.to_string(),
        items,
    })
}

/// Sanitize every rendered text field of a chat item (title, summary, body, and
/// detail labels) against an untrusted/malformed transcript.
fn sanitize_chat_item(mut item: ChatItemView) -> ChatItemView {
    item.title = sanitize_transcript_text(&item.title);
    item.summary = item
        .summary
        .map(|summary| sanitize_transcript_text(&summary));
    for line in &mut item.body {
        line.text = sanitize_transcript_text(&line.text);
    }
    for detail in &mut item.details {
        match detail {
            ChatDetailRef::HistoryEvent { label, .. } | ChatDetailRef::Artifact { label, .. } => {
                *label = sanitize_transcript_text(label);
            }
            // Inline carries rendered `content` in addition to its label; both
            // are user-visible transcript text and must be sanitized.
            ChatDetailRef::Inline { label, content, .. } => {
                *label = sanitize_transcript_text(label);
                *content = sanitize_transcript_text(content);
            }
        }
    }
    item
}

/// Strip terminal control and ANSI escape sequences from untrusted transcript
/// text so a hostile or malformed stored line can't disrupt the terminal
/// (ADR-004). Removes C0/C1 control characters (keeping `\t`) and full CSI
/// (`ESC [ … final`) and OSC (`ESC ] … BEL|ST`) escape sequences.
pub fn sanitize_transcript_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    // CSI: consume params/intermediates until a final byte.
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: consume until BEL or ST (ESC \).
                    while let Some(&next) = chars.peek() {
                        if next == '\u{7}' {
                            chars.next();
                            break;
                        }
                        if next == '\u{1b}' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                _ => {} // lone ESC (or ESC + other): drop just the ESC.
            }
            continue;
        }
        if c.is_control() && c != '\t' {
            continue; // drop other control chars (newlines, DEL, C1, …).
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governance_decision_kind_slug_is_governance_decision() {
        assert_eq!(
            ChatItemKind::GovernanceDecision.slug(),
            "governance_decision"
        );
    }

    #[test]
    fn governance_decision_lifecycle_key_item_id_is_stable() {
        let key = ChatLifecycleKey::GovernanceDecision {
            run_id: "run-7".to_string(),
            decision_id: "dec-9".to_string(),
        };
        assert_eq!(key.item_id(), "chat:governance_decision:run-7:dec-9");
    }

    // ── read-only preview fold + sanitization (task_06) ──

    use crate::history::HistoryEvent;
    use serde_json::json;
    use tempfile::tempdir;

    fn seed_session(dir: &std::path::Path) -> (std::path::PathBuf, String) {
        let store = HistoryStore::create(dir).unwrap();
        let id = store.session_id().to_string();
        for (kind, payload) in [
            ("prompt_submitted", json!({ "prompt": "do the thing" })),
            ("run_started", json!({ "run_id": "r1" })),
            ("run_completed", json!({ "summary": "done" })),
        ] {
            store
                .append_event(&HistoryEvent::new(
                    id.clone(),
                    Some("r1".to_string()),
                    None,
                    kind,
                    payload,
                ))
                .unwrap();
        }
        (dir.join(".atelier"), id)
    }

    #[test]
    fn preview_equals_a_direct_rebuild_of_the_log() {
        let dir = tempdir().unwrap();
        let (root, id) = seed_session(dir.path());
        let preview = build_session_preview(&root, &id).unwrap();

        // Fidelity: the preview is exactly the sanitized rebuild of the log.
        let store = HistoryStore::open(&root, &id).unwrap();
        let expected: Vec<ChatItemView> = ChatProjection::rebuild(&store.read_events().unwrap())
            .items()
            .iter()
            .cloned()
            .map(sanitize_chat_item)
            .collect();
        assert_eq!(preview.items, expected);
        assert!(!preview.items.is_empty());
    }

    #[test]
    fn preview_excludes_welcome_and_live_overlays() {
        let dir = tempdir().unwrap();
        let (root, id) = seed_session(dir.path());
        let preview = build_session_preview(&root, &id).unwrap();
        // rebuild() never injects the synthetic welcome item or live-step /
        // pending-approval overlays (those live in App::sync_chat_items).
        assert!(preview
            .items
            .iter()
            .all(|item| item.kind != ChatItemKind::Welcome));
    }

    #[test]
    fn sanitize_strips_ansi_and_control_sequences() {
        // CSI color codes, an OSC sequence, and a bare BEL all vanish.
        assert_eq!(
            sanitize_transcript_text("\u{1b}[31mred\u{1b}[0m text"),
            "red text"
        );
        assert_eq!(sanitize_transcript_text("a\u{1b}]0;evil title\u{7}b"), "ab");
        assert_eq!(sanitize_transcript_text("x\u{7}\u{8}y"), "xy");
        // Tabs are preserved; printable text is untouched.
        assert_eq!(sanitize_transcript_text("col1\tcol2"), "col1\tcol2");
    }

    #[test]
    fn building_a_preview_does_not_mutate_a_live_projection() {
        let dir = tempdir().unwrap();
        let (root, id) = seed_session(dir.path());

        // A separate live projection with its own state.
        let mut live = ChatProjection::new();
        live.apply_history_event(&HistoryEvent::new(
            "live",
            Some("lr".to_string()),
            None,
            "prompt_submitted",
            json!({ "prompt": "live prompt" }),
        ));
        let before = live.items().to_vec();

        let _ = build_session_preview(&root, &id).unwrap();
        // The read-only preview build left the live projection untouched.
        assert_eq!(live.items(), before.as_slice());
    }

    #[test]
    fn preview_over_multi_run_session_matches_on_disk_fold() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let id = store.session_id().to_string();
        for (kind, payload) in [
            ("prompt_submitted", json!({ "prompt": "first" })),
            ("run_started", json!({ "run_id": "r1" })),
            (
                "run_interrupted",
                json!({ "run_id": "r1", "prior_state": "running" }),
            ),
            (
                "session_resumed",
                json!({ "resumed_at": "t", "cwd": "/tmp", "dirty": false, "prior_end_state": "interrupted", "approval_mode": "normal" }),
            ),
            ("run_started", json!({ "run_id": "r2" })),
            ("run_completed", json!({ "summary": "done" })),
        ] {
            store
                .append_event(&HistoryEvent::new(
                    id.clone(),
                    Some("run".to_string()),
                    None,
                    kind,
                    payload,
                ))
                .unwrap();
        }
        let root = dir.path().join(".atelier");
        let preview = build_session_preview(&root, &id).unwrap();
        let direct: Vec<ChatItemView> = ChatProjection::rebuild(&store.read_events().unwrap())
            .items()
            .iter()
            .cloned()
            .map(sanitize_chat_item)
            .collect();
        assert_eq!(preview.items, direct);
        // The resume divider folded through into the preview.
        assert!(preview
            .items
            .iter()
            .any(|item| item.title == "Session resumed"));
    }
}
