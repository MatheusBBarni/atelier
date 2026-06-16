pub mod command_summary;
pub mod diff_preview;
mod projection;

pub use projection::ChatProjection;
pub(crate) use projection::FIRST_APPROVAL_EXPLAINER;

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
    /// A governance decision (e.g. the turn-1 early-abort) paused for the user
    /// to accept or reject before the run proceeds (governance spine, ADR-003).
    GovernanceDecision,
    Diagnostic,
    SkillContext,
    AgentResult,
    RunSummary,
    /// Synthetic branded welcome item, injected at startup (ADR-005). Not an
    /// orchestration event; carries no lifecycle key and never updates.
    Welcome,
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
            ChatItemKind::Welcome => "welcome",
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
}
