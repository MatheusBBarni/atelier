pub mod command_summary;
pub mod diff_preview;
mod projection;

pub use projection::ChatProjection;

use serde::{Deserialize, Serialize};

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
    Diagnostic,
    SkillContext,
    AgentResult,
    RunSummary,
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
            ChatItemKind::Diagnostic => "diagnostic",
            ChatItemKind::SkillContext => "skill_context",
            ChatItemKind::AgentResult => "agent_result",
            ChatItemKind::RunSummary => "run_summary",
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
