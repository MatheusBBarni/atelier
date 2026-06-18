use crate::app::chat::{
    ChatDetailRef, ChatItemKind, ChatItemView, ChatLineStyle, ChatLineView, ChatSeverity,
    SessionPreview,
};
use crate::app::git::GitContext;
use crate::app::{
    ActivityState, AgentView, App, AppEvent, AppState, ApprovalHandle, ApprovalResolution,
    InterruptHandle, PendingApprovalView, PendingClarificationView, PendingPlanApprovalView,
    PlanApprovalAnswer, PromptSource, QueuedFollowUpStatus, QueuedFollowUpView, RosterRow,
};
use crate::config::EffectiveConfig;
use crate::file_index::{FileEntry, FileIndex, FileSuggestion};
use crate::governance::{GovernanceAnswer, GovernanceDecisionView};
use crate::history::SessionSummary;
use crate::hooks::{self, HookLifecycleRecord};
use crate::keybindings::{self, KeyAction, Keymap};
use crate::orchestrator::RunState;
use crate::skills::{
    self, SkillSourceTag, SkillSuggestion, SKILL_DISCOVERY_MAX_DEPTH, SKILL_FILE_NAME,
    SKILL_SUGGESTION_CACHE_SCHEMA_VERSION,
};
use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

pub mod theme;
pub mod welcome;

use theme::{TerminalCaps, Theme};
use welcome::WelcomeFacts;

// Input box (4) + status line (1) + ambient footer line (1).
const INPUT_COMPOSER_HEIGHT: u16 = 6;
const INPUT_BOX_HEIGHT: u16 = 4;
const INPUT_PROMPT: &str = "> ";
const INPUT_PROMPT_WIDTH: usize = 2;
const AGENT_PREFIX: &str = "/agent:";
const SKILL_PREFIX: &str = "/skill:";
const FILE_MENTION_PREFIX: &str = "@";
const RELOAD_SKILLS_COMMAND: &str = "/reload:skills";
const DROPDOWN_MAX_ITEMS: usize = 6;
const WORK_HINT: &str = "/help";
/// Contextual hint shown in place of `WORK_HINT` when recall is available
/// (input empty, history loaded, no active work). Advertises ↑/↓ recall while
/// keeping `/help` discoverable; concise like `QUEUE_HINT` (ADR-002, task_07).
const HISTORY_HINT: &str = "↑ recall · /help";
const WORK_INDICATOR_HEIGHT: u16 = 1;
/// Ambient status footer line (repo·branch · run state · agents), below the
/// work-indicator/hint line.
const FOOTER_HEIGHT: u16 = 1;
/// Agent statuses counted as actively running in the footer summary.
const RUNNING_AGENT_STATUSES: [&str; 3] = ["running", "streaming", "running_parallel"];
const WORK_LABEL: &str = "Working";
const MOUSE_SCROLL_LINES: usize = 3;
const WORK_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
/// Row prefix for the focused / unfocused clarification option.
const CLARIFICATION_FOCUS_MARKER: &str = "❯ ";
const CLARIFICATION_BLUR_MARKER: &str = "  ";
/// Checkbox glyphs shown for multi-select options.
const CLARIFICATION_CHECK_ON: &str = "[x] ";
const CLARIFICATION_CHECK_OFF: &str = "[ ] ";
/// Prompt shown on the synthetic "custom answer" row while it is focused.
const CLARIFICATION_CUSTOM_PROMPT: &str = "Custom: ";
/// Placeholder shown on the custom row while it is not focused.
const CLARIFICATION_CUSTOM_PLACEHOLDER: &str = "Custom…";
const CLARIFICATION_RECOMMENDED_SUFFIX: &str = "  ★ recommended";
const CLARIFICATION_HINT_SINGLE: &str =
    "↑/↓ or 1-9 move · type for custom · Enter confirm · Ctrl-C interrupt";
const CLARIFICATION_HINT_MULTI: &str =
    "↑/↓ or 1-9 move · Space toggle · type for custom · Enter confirm · Ctrl-C interrupt";
const GOVERNANCE_DECISION_HINT: &str =
    "Ctrl-Y accept · Esc reject · type a redirect then Enter to reject with guidance · Ctrl-C interrupt";
const PLAN_APPROVAL_HINT: &str =
    "Ctrl-Y accept plan · Esc reject · type a reason then Enter to reject with guidance · Ctrl-C interrupt";
const QUEUE_VISIBLE_MAX: usize = 6;
const QUEUE_SELECTED_MARKER: &str = "> ";
const QUEUE_UNSELECTED_MARKER: &str = "  ";
const QUEUE_HINT: &str = "↑/↓ select · Del cancel · Ctrl-R resume (clear input to focus)";

/// Identifies the six tabs of the help modal and provides ordered iteration
/// plus wrap-around navigation. Pure value type — carries no rendering or state.
///
/// The active tab lives in `TuiUiState.help_active_tab`; `render_help_modal`
/// dispatches on it to the per-tab builders. Wrap-around navigation
/// (`next`/`prev`) is wired into key routing by task 07.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpTab {
    GettingStarted,
    Commands,
    Keys,
    Skills,
    Approvals,
    Cli,
}

// `ALL`/`title` are consumed by the tabbed render (task 06); `next`/`prev` are
// consumed by tab navigation (task 07).
impl HelpTab {
    /// Tabs in declared left-to-right order (Getting Started first, CLI last).
    const ALL: [HelpTab; 6] = [
        HelpTab::GettingStarted,
        HelpTab::Commands,
        HelpTab::Keys,
        HelpTab::Skills,
        HelpTab::Approvals,
        HelpTab::Cli,
    ];

    /// Human-readable tab title shown in the tab strip.
    fn title(self) -> &'static str {
        match self {
            HelpTab::GettingStarted => "Getting Started",
            HelpTab::Commands => "Commands",
            HelpTab::Keys => "Keys",
            HelpTab::Skills => "Skills",
            HelpTab::Approvals => "Approvals",
            HelpTab::Cli => "CLI",
        }
    }

    /// Next tab in `ALL`, wrapping from the last back to the first.
    fn next(self) -> HelpTab {
        let index = HelpTab::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or(0);
        HelpTab::ALL[(index + 1) % HelpTab::ALL.len()]
    }

    /// Previous tab in `ALL`, wrapping from the first back to the last.
    fn prev(self) -> HelpTab {
        let index = HelpTab::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or(0);
        HelpTab::ALL[(index + HelpTab::ALL.len() - 1) % HelpTab::ALL.len()]
    }
}

/// Row presentation style for the shared agent-roster builder: `Full` renders
/// three lines per agent (Ctrl-L roster), `Compact` renders one (Getting Started).
///
/// Consumed by the `agent_roster_items` builder (task 02). `Full` is live in the
/// Ctrl-L roster; `Compact` lands with the Getting Started tab (task 05), so the
/// `allow(dead_code)` stays until that variant is constructed in production.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RosterRowStyle {
    Full,
    Compact,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TuiCommand {
    Dispatch(AppEvent),
    DispatchAndQuit(AppEvent),
    ToggleRoster,
    ToggleHelp,
    HelpNextTab,
    HelpPrevTab,
    HelpFilterCharacter(char),
    HelpFilterBackspace,
    ScrollEvents(EventScrollCommand),
    MoveInputCursor(InputCursorCommand),
    AgentDropdown(DropdownCommand),
    SkillDropdown(DropdownCommand),
    CommandDropdown(CommandDropdownCommand),
    FileMentionDropdown(FileMentionDropdownCommand),
    Clarification(ClarificationCommand),
    ClarificationInputCharacter(char),
    ClarificationInputBackspace,
    QueueSelection(QueueSelectionCommand),
    SessionBrowser(SessionBrowserCommand),
    ReloadSkills,
    InputCharacter(char),
    InputBackspace,
    InputKill(InputKillCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EventScrollCommand {
    PageUp,
    PageDown,
    LinesUp(usize),
    LinesDown(usize),
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputCursorCommand {
    Left,
    Right,
    Up,
    Down,
    /// Jump the cursor to the start of the composer line (readline `Ctrl-A`).
    LineStart,
    /// Jump the cursor to the end of the composer line (readline `Ctrl-E`).
    LineEnd,
}

/// Readline-style kill operations over the single-line composer. Kills discard
/// text (no kill-ring/yank in V1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputKillCommand {
    /// Delete from the cursor to the end of the line (readline `Ctrl-K`).
    ToLineEnd,
    /// Delete from the start of the line up to the cursor (readline `Ctrl-U`,
    /// `unix-line-discard`).
    ToLineStart,
    /// Delete the whitespace-and-word immediately before the cursor (readline
    /// `Ctrl-W`).
    WordBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropdownCommand {
    Previous,
    Next,
    Accept,
}

/// Command-dropdown actions. Distinct from `DropdownCommand` because the command
/// dropdown also supports Escape dismissal and trapping `Enter` in the no-match
/// state (ADR-004).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandDropdownCommand {
    Previous,
    Next,
    /// Insert the selected command's text only; never dispatches an app event.
    Accept,
    /// Escape: suppress the dropdown for the current input without mutating it.
    Dismiss,
    /// Consume `Enter` while the no-match state is visible so invalid slash
    /// input is not submitted.
    TrapNoMatch,
}

/// File-mention dropdown actions (ADR-005). Like `CommandDropdownCommand` it
/// supports Escape dismissal, but — unlike the command dropdown — it does NOT
/// trap `Enter` in the no-match state: a typed `@query` with no matches still
/// submits normally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileMentionDropdownCommand {
    Previous,
    Next,
    /// Insert the selected suggestion's bare path; never dispatches an app event.
    Accept,
    /// Escape: suppress the dropdown for the current input without mutating it.
    Dismiss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClarificationCommand {
    PreviousOption,
    NextOption,
    /// Toggle the focused option's checkbox (multi-select only).
    ToggleOption,
    /// Jump focus to a specific row (digit quick-select). Index is 0-based over
    /// the option rows; the synthetic custom row is not digit-selectable.
    FocusOption(usize),
    Submit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueSelectionCommand {
    Previous,
    Next,
}

#[derive(Debug)]
enum AppWorkerCommand {
    Event(AppEvent),
    /// A `hook_started`/`hook_completed` record from the off-thread dispatcher,
    /// forwarded here so the worker (the only `&mut App`) records it (ADR-003).
    RecordHookLifecycle(HookLifecycleRecord),
    Shutdown,
}

/// Which view the session browser is showing: the list, or a selected session's
/// read-only transcript preview (task_08).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BrowserMode {
    #[default]
    List,
    Preview,
}

/// One browser action, routed while the modal is visible (task_07/08). The
/// resume action arrives in task_11.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SessionBrowserCommand {
    Open,
    Close,
    Up,
    Down,
    FilterChar(char),
    FilterBackspace,
    /// `→` on a list row → load that session's preview and switch to it.
    OpenPreview,
    /// `Esc` in preview → return to the list.
    Back,
    /// Scroll the preview transcript (reuses the chat scroll vocabulary).
    ScrollPreview(EventScrollCommand),
    /// `Enter` (list or preview) → resume this session: dispatch
    /// `AppEvent::ResumeSession` to the worker and close the modal (task_11). The
    /// id is resolved at keypress so the worker need not see browser state.
    Resume(String),
}

/// Ephemeral session-browser modal state, held in `TuiUiState` like `help_*`
/// (ADR-001). Summaries arrive off-thread over a watch channel; the filter is a
/// case-insensitive substring narrow (fuzzy deferred, ADR-001).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SessionBrowserState {
    visible: bool,
    mode: BrowserMode,
    summaries: Vec<SessionSummary>,
    selection_index: usize,
    filter: String,
    /// The selected session's loaded preview (task_08); `None` while loading or
    /// in list mode. Arrives off-thread over a watch channel.
    preview: Option<SessionPreview>,
    /// Which session the current preview is for, so `run_loop` knows when to
    /// spawn a fresh off-thread load.
    preview_session_id: Option<String>,
    /// Scroll offset (in transcript lines) for the preview pane.
    preview_scroll: usize,
}

impl SessionBrowserState {
    /// Indices of `summaries` matching the case-insensitive substring filter, in
    /// list (newest-first) order. An empty filter matches everything.
    fn filtered_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.summaries.len()).collect();
        }
        let needle = self.filter.to_lowercase();
        self.summaries
            .iter()
            .enumerate()
            .filter(|(_, summary)| summary.label.to_lowercase().contains(&needle))
            .map(|(index, _)| index)
            .collect()
    }

    /// Keep `selection_index` within the current filtered range.
    fn clamp_selection(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.selection_index = 0;
        } else if self.selection_index >= len {
            self.selection_index = len - 1;
        }
    }

    /// The session id the Resume action targets: the previewed session in preview
    /// mode, else the highlighted list row. `None` when the (filtered) list is
    /// empty, so Resume is a no-op rather than resuming nothing.
    fn selected_session_id(&self) -> Option<String> {
        match self.mode {
            BrowserMode::Preview => self.preview_session_id.clone(),
            BrowserMode::List => self
                .filtered_indices()
                .get(self.selection_index)
                .map(|&index| self.summaries[index].session_id.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TuiUiState {
    roster_visible: bool,
    help_visible: bool,
    /// Active tab in the help modal. Ephemeral UI state only — never enters the
    /// event-sourced `AppState` snapshot. Reset to `GettingStarted` on close so
    /// reopening always lands on the front-door tab.
    help_active_tab: HelpTab,
    /// Type-to-filter buffer for the Commands help tab (Phase 2). Ephemeral UI
    /// state, never in the snapshot and fully isolated from the composer
    /// `state.input`. Cleared on tab change and on modal close so each tab/open
    /// starts unfiltered.
    help_filter: String,
    event_scroll: usize,
    event_follow: bool,
    event_content_lines: usize,
    event_viewport_lines: usize,
    event_area: Rect,
    working_directory: Option<PathBuf>,
    input_cursor: usize,
    input_preferred_col: Option<usize>,
    input_width: usize,
    /// Recall ring: this project's past prompts, newest-first, deduped and
    /// capped (ADR-001/004). Seeded once off-thread at startup over a `watch`
    /// channel; empty until that load lands, or when recall is disabled.
    prompt_history: Vec<String>,
    /// Recall cursor: `0` = the live draft; `N` = the Nth-newest entry. Drives
    /// the `Fresh`/`Recalled` provenance tag (task_05/06).
    prompt_history_cursor: usize,
    /// The in-progress draft saved while browsing history, restored when ↓ steps
    /// back past the newest entry (task_05).
    prompt_history_draft: String,
    /// Whether recall is enabled (config `ui.prompt_history_enabled`). Gates the
    /// in-session prepend so a disabled session never builds a recallable ring.
    prompt_history_enabled: bool,
    /// Upper bound on the in-memory ring (config `ui.prompt_history_max`); the
    /// in-session prepend truncates to this after adding a submission.
    prompt_history_max: usize,
    agent_selection_index: usize,
    skill_suggestions: Vec<SkillSuggestion>,
    skill_selection_index: usize,
    command_selection_index: usize,
    /// Input value the command dropdown was last dismissed for (Escape). The
    /// dropdown stays suppressed only while the raw input matches, so editing
    /// the text re-activates discovery without mutating the input.
    command_dropdown_dismissed: Option<String>,
    /// Latest file-index snapshot from the background worker walk (ADR-003);
    /// the `@` dropdown queries this in-memory list per keystroke.
    file_mention_entries: Vec<FileEntry>,
    file_mention_selection_index: usize,
    /// Input value the file-mention dropdown was last dismissed for (Escape),
    /// mirroring `command_dropdown_dismissed`.
    file_mention_dropdown_dismissed: Option<String>,
    clarification_option_index: usize,
    clarification_custom_answer: String,
    /// Option indices currently checked in a multi-select clarification.
    clarification_selected: BTreeSet<usize>,
    /// question_id of the clarification the above selection state belongs to, so
    /// the composer resets cleanly when a new question arrives.
    clarification_question_id: Option<String>,
    /// True once an answer has been submitted but the worker has not yet cleared
    /// `pending_clarification`; freezes the composer so a fast second Enter can't
    /// queue a duplicate answer that the worker would later reject.
    clarification_submitting: bool,
    queue_selection_index: usize,
    status_message: Option<String>,
    work_spinner_frame: usize,
    theme: Theme,
    hide_banner: bool,
    /// Resolved key → action map consulted in the normal-input branch only
    /// (ADR-003). Built once at TUI init; Wave 1 uses `DEFAULTS` (no config),
    /// task_08 swaps in the config-resolved map. No overrides ⇒ byte-identical
    /// default routing.
    keymap: Keymap,
    /// Session-browser modal state (task_07): visibility, off-thread-loaded
    /// summaries, selection, and filter. Default = closed/empty.
    browser: SessionBrowserState,
}

impl Default for TuiUiState {
    fn default() -> Self {
        Self {
            roster_visible: true,
            help_visible: false,
            help_active_tab: HelpTab::GettingStarted,
            help_filter: String::new(),
            event_scroll: 0,
            event_follow: true,
            event_content_lines: 0,
            event_viewport_lines: 1,
            event_area: Rect::ZERO,
            working_directory: None,
            input_cursor: 0,
            input_preferred_col: None,
            input_width: 1,
            prompt_history: Vec::new(),
            prompt_history_cursor: 0,
            prompt_history_draft: String::new(),
            prompt_history_enabled: true,
            prompt_history_max: 200,
            agent_selection_index: 0,
            skill_suggestions: Vec::new(),
            skill_selection_index: 0,
            command_selection_index: 0,
            command_dropdown_dismissed: None,
            file_mention_entries: Vec::new(),
            file_mention_selection_index: 0,
            file_mention_dropdown_dismissed: None,
            clarification_option_index: 0,
            clarification_custom_answer: String::new(),
            clarification_selected: BTreeSet::new(),
            clarification_question_id: None,
            clarification_submitting: false,
            queue_selection_index: 0,
            status_message: None,
            work_spinner_frame: 0,
            // Tests default to a fixed truecolor theme so style assertions are
            // deterministic; production overrides this in `run_tui` with the
            // capability-detected theme.
            theme: Theme::resolve(TerminalCaps {
                no_color: false,
                truecolor: true,
            }),
            hide_banner: false,
            keymap: default_keymap(),
            browser: SessionBrowserState::default(),
        }
    }
}

/// The Wave-1 keymap: `DEFAULTS` resolved with no user overrides. Byte-identical
/// to the pre-feature routing for the keys it owns. task_08 replaces the call site
/// at TUI init with the config-resolved map.
fn default_keymap() -> Keymap {
    Keymap::resolve(
        &keybindings::DEFAULTS,
        &keybindings::KeybindingOverrides::new(),
    )
}

/// The single bridge from a remappable [`KeyAction`] to its concrete [`TuiCommand`]
/// (ADR-003). Exhaustive by construction: adding a `KeyAction` variant forces a new
/// arm here, so action names can never drift from the command enum.
fn command_for_action(action: KeyAction) -> TuiCommand {
    match action {
        KeyAction::ToggleRoster => TuiCommand::ToggleRoster,
        KeyAction::ScrollPageUp => TuiCommand::ScrollEvents(EventScrollCommand::PageUp),
        KeyAction::ScrollPageDown => TuiCommand::ScrollEvents(EventScrollCommand::PageDown),
        KeyAction::ScrollTop => TuiCommand::ScrollEvents(EventScrollCommand::Top),
        KeyAction::ScrollBottom => TuiCommand::ScrollEvents(EventScrollCommand::Bottom),
        KeyAction::InputLineStart => TuiCommand::MoveInputCursor(InputCursorCommand::LineStart),
        KeyAction::InputLineEnd => TuiCommand::MoveInputCursor(InputCursorCommand::LineEnd),
        KeyAction::InputKillToEnd => TuiCommand::InputKill(InputKillCommand::ToLineEnd),
        KeyAction::InputKillToStart => TuiCommand::InputKill(InputKillCommand::ToLineStart),
        KeyAction::InputKillWordBack => TuiCommand::InputKill(InputKillCommand::WordBack),
    }
}

impl TuiUiState {
    fn with_skill_suggestions(
        working_directory: PathBuf,
        skill_suggestions: Vec<SkillSuggestion>,
    ) -> Self {
        Self {
            working_directory: Some(working_directory),
            skill_suggestions,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PromptToken {
    value_start: usize,
    value_end: usize,
    query: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentSuggestion {
    id: String,
    name: String,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentDropdown {
    token: PromptToken,
    suggestions: Vec<AgentSuggestion>,
    selected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SkillDropdown {
    token: PromptToken,
    suggestions: Vec<SkillSuggestion>,
    selected: usize,
}

/// The top-level slash-command discovery dropdown (ADR-004). Unlike the agent
/// and skill dropdowns it is not token-based: it activates only while the whole
/// input is a single `/`-prefixed word, and it carries a compact no-match state
/// (`empty`) that still renders even though nothing is selectable.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandDropdown {
    suggestions: Vec<&'static crate::slash_commands::SlashCommandSpec>,
    selected: Option<usize>,
    empty: bool,
}

/// The `@`-mention file/folder dropdown (ADR-005). Token-based like the skill
/// dropdown (it carries a `PromptToken` and activates mid-prompt), plus a
/// command-style `empty` no-match flag. Its suggestions are the ranked
/// `FileSuggestion`s from `FileIndex::query` over the cached index.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileMentionDropdown {
    token: PromptToken,
    suggestions: Vec<crate::file_index::FileSuggestion>,
    selected: usize,
    empty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SkillFileFingerprint {
    path: String,
    byte_len: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SkillSuggestionCache {
    schema_version: u32,
    fingerprint: Vec<SkillFileFingerprint>,
    suggestions: Vec<SkillSuggestion>,
}

pub async fn run_tui(config: EffectiveConfig, debug_enabled: bool) -> Result<()> {
    if !io::stdout().is_terminal() {
        println!("atelier TUI requires an interactive terminal. Use --doctor or --print-config for non-interactive checks.");
        return Ok(());
    }

    let working_directory = config.working_directory.clone();
    let hide_banner = config.ui.hide_banner;
    let prompt_history_enabled = config.ui.prompt_history_enabled;
    let prompt_history_max = config.ui.prompt_history_max;
    // Captured before `config` is moved into the app; drives whether the hook
    // dispatcher is spawned (zero cost when no handlers) and which notifier backend.
    let hooks_config = config.hooks.clone();
    // Resolve the active keymap from defaults + validated config overrides (task_08).
    // Captured before `config` is moved into the app; no overrides ⇒ default keymap.
    let keymap = Keymap::resolve(&keybindings::DEFAULTS, &config.keybindings);
    let theme = Theme::resolve(TerminalCaps::detect());
    let mut app = App::new_with_debug(config, debug_enabled).await?;
    let (state_sender, state_receiver) = watch::channel(app.state().clone());
    app.attach_state_sender(state_sender);
    let interrupt_handle = app.interrupt_handle();
    let approval_handle = app.approval_handle();
    let (command_sender, command_receiver) = mpsc::channel(1024);
    // Worker→TUI file-index snapshot channel (ADR-003). The worker walks the
    // working directory off-thread and publishes the latest `Vec<FileEntry>`
    // here; the render loop consumes it.
    let (file_index_sender, file_index_receiver) = watch::channel(Vec::<FileEntry>::new());
    // One-time recall load → TUI channel (ADR-004). The loader (spawned below,
    // gated on the toggle) projects `prompt_submitted` history off-thread and
    // publishes the ring once; the render loop syncs it into `TuiUiState`.
    let (prompt_history_sender, prompt_history_receiver) = watch::channel(Vec::<String>::new());

    // Lifecycle hooks (ADR-003): only when handlers are configured, create the
    // bounded dispatch channel + drop counter, wire the event tap
    // (`App.hook_sender`), spawn the off-thread dispatcher, and forward its
    // `hook_started`/`hook_completed` records back into the worker (the only
    // `&mut App`) to be recorded. Skipped entirely otherwise, so the write path
    // stays zero-cost when no hooks are present.
    if !hooks_config.handlers.is_empty() {
        let (hook_sender, hook_receiver) = hooks::hook_channel();
        let (lifecycle_sender, mut lifecycle_receiver) =
            mpsc::channel::<HookLifecycleRecord>(hooks::HOOK_CHANNEL_CAPACITY);
        app.attach_hook_sender(hook_sender, hooks::DroppedHookCounter::new());
        let notifier: Arc<dyn hooks::Notifier> = match hooks_config.notify_fallback_command.clone()
        {
            Some(command) => Arc::new(hooks::CommandNotifier::new(command)),
            None => Arc::new(hooks::OscNotifier::to_terminal()),
        };
        tokio::spawn(hooks::run_hook_dispatcher(
            hook_receiver,
            lifecycle_sender,
            notifier,
            hooks::DEFAULT_HOOK_TIMEOUT,
        ));
        let lifecycle_command_sender = command_sender.clone();
        tokio::spawn(async move {
            while let Some(record) = lifecycle_receiver.recv().await {
                if lifecycle_command_sender
                    .send(AppWorkerCommand::RecordHookLifecycle(record))
                    .await
                    .is_err()
                {
                    break; // Worker is gone; stop forwarding.
                }
            }
        });
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let worker = tokio::spawn(run_app_worker(
        app,
        command_receiver,
        file_index_sender,
        Some(working_directory.clone()),
    ));
    // Detached recall load: never blocks the first paint, and skipped entirely
    // when recall is disabled so the ring stays empty (ADR-004).
    maybe_spawn_prompt_history_load(
        prompt_history_enabled,
        Some(working_directory.clone()),
        prompt_history_max,
        prompt_history_sender,
    );

    // No loading interstitial: the main UI (with the branded welcome item)
    // renders on the first frame, and skill scanning happens behind it inside
    // `run_loop` so startup stays under the first-frame budget (ADR-005).
    let mut ui_state = TuiUiState::with_skill_suggestions(working_directory, Vec::new());
    ui_state.theme = theme;
    ui_state.hide_banner = hide_banner;
    ui_state.prompt_history_enabled = prompt_history_enabled;
    ui_state.prompt_history_max = prompt_history_max;
    // Replace the DEFAULTS-only keymap (task_04) with the config-resolved one.
    ui_state.keymap = keymap;
    let result = run_loop(
        &mut terminal,
        state_receiver,
        command_sender.clone(),
        interrupt_handle,
        approval_handle,
        file_index_receiver,
        prompt_history_receiver,
        ui_state,
    )
    .await;

    let cleanup_result = (|| -> Result<()> {
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    })();
    let shutdown_result = shutdown_app_worker(command_sender, worker).await;

    result?;
    cleanup_result?;
    shutdown_result
}

// The render loop legitimately wires together every long-lived TUI channel and
// handle (state, commands, interrupt/approval, the two background-load
// receivers, and the UI state); bundling them into a struct would only move the
// same fields behind one more indirection.
#[allow(clippy::too_many_arguments)]
async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state_receiver: watch::Receiver<AppState>,
    command_sender: mpsc::Sender<AppWorkerCommand>,
    interrupt_handle: InterruptHandle,
    approval_handle: ApprovalHandle,
    mut file_index_receiver: watch::Receiver<Vec<FileEntry>>,
    mut prompt_history_receiver: watch::Receiver<Vec<String>>,
    mut ui_state: TuiUiState,
) -> Result<()> {
    let mut state = state_receiver.borrow_and_update().clone();
    // Session-browser list channel (task_07): the loader runs off-thread on
    // browser-open and publishes summaries here; the render loop syncs them in,
    // so opening the browser never blocks the render loop.
    let (session_summaries_sender, mut session_summaries_receiver) =
        watch::channel(Vec::<SessionSummary>::new());
    // Session-preview channel (task_08): on selecting a row, the sanitized
    // history-only fold is built off-thread and published here.
    let (session_preview_sender, mut session_preview_receiver) =
        watch::channel(None::<SessionPreview>);
    // Render the first frame (welcome visible) before the blocking skill scan,
    // then load suggestions behind it so the /skill dropdown is ready by the
    // next interaction.
    terminal.draw(|frame| render(frame, &state, &mut ui_state))?;
    if let Some(working_directory) = ui_state.working_directory.clone() {
        ui_state.skill_suggestions = load_skill_suggestions(&working_directory);
    }
    loop {
        sync_worker_state(&mut state, &mut state_receiver);
        sync_file_index(&mut ui_state, &mut file_index_receiver);
        sync_prompt_history(&mut ui_state, &mut prompt_history_receiver);
        sync_session_summaries(&mut ui_state, &mut session_summaries_receiver);
        sync_session_preview(&mut ui_state, &mut session_preview_receiver);
        clamp_input_cursor(&mut ui_state, &state.input);
        terminal.draw(|frame| render(frame, &state, &mut ui_state))?;

        if event::poll(Duration::from_millis(50))? {
            let command = match event::read()? {
                Event::Key(key) => key_event_to_tui_command_with_ui(&state, &ui_state, key),
                Event::Mouse(mouse) => mouse_event_to_tui_command(&ui_state, mouse),
                _ => None,
            };
            if let Some(command) = command {
                let browser_was_visible = ui_state.browser.visible;
                let preview_target_before = ui_state.browser.preview_session_id.clone();
                // Skill reload reports via the status line (set in `reload_skills`)
                // rather than a full-screen takeover.
                if !execute_tui_command_with_interrupt(
                    &mut state,
                    &mut ui_state,
                    &command_sender,
                    Some(&interrupt_handle),
                    Some(&approval_handle),
                    command,
                )
                .await?
                {
                    break;
                }
                // The browser just opened → load its session list off-thread.
                if ui_state.browser.visible && !browser_was_visible {
                    spawn_session_summaries_load(
                        ui_state.working_directory.clone(),
                        session_summaries_sender.clone(),
                    );
                }
                // A new session was selected for preview → fold it off-thread.
                if let Some(session_id) = ui_state.browser.preview_session_id.clone() {
                    if Some(&session_id) != preview_target_before.as_ref() {
                        spawn_session_preview_load(
                            ui_state.working_directory.clone(),
                            session_id,
                            session_preview_sender.clone(),
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
async fn execute_tui_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command_sender: &mpsc::Sender<AppWorkerCommand>,
    command: TuiCommand,
) -> Result<bool> {
    execute_tui_command_with_interrupt(state, ui_state, command_sender, None, None, command).await
}

async fn execute_tui_command_with_interrupt(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command_sender: &mpsc::Sender<AppWorkerCommand>,
    interrupt_handle: Option<&InterruptHandle>,
    approval_handle: Option<&ApprovalHandle>,
    command: TuiCommand,
) -> Result<bool> {
    match command {
        TuiCommand::ToggleRoster => {
            ui_state.roster_visible = !ui_state.roster_visible;
            Ok(true)
        }
        TuiCommand::ToggleHelp => {
            ui_state.help_visible = !ui_state.help_visible;
            if !ui_state.help_visible {
                // Reset to the front-door tab so reopening always starts on
                // Getting Started. Ephemeral UI state, never in the snapshot.
                ui_state.help_active_tab = HelpTab::GettingStarted;
                // Drop any Commands-tab filter so the next open starts clean.
                ui_state.help_filter.clear();
            }
            clear_input(state, ui_state);
            Ok(true)
        }
        TuiCommand::HelpNextTab => {
            ui_state.help_active_tab = ui_state.help_active_tab.next();
            // Filtering is Commands-tab-local; reset it on every tab change.
            ui_state.help_filter.clear();
            Ok(true)
        }
        TuiCommand::HelpPrevTab => {
            ui_state.help_active_tab = ui_state.help_active_tab.prev();
            ui_state.help_filter.clear();
            Ok(true)
        }
        TuiCommand::HelpFilterCharacter(ch) => {
            ui_state.help_filter.push(ch);
            Ok(true)
        }
        TuiCommand::HelpFilterBackspace => {
            ui_state.help_filter.pop();
            Ok(true)
        }
        TuiCommand::ScrollEvents(command) => {
            scroll_events(ui_state, command);
            Ok(true)
        }
        TuiCommand::MoveInputCursor(command) => {
            // ↑/↓ walk the recall ring at the input's top/bottom boundary;
            // otherwise (and for ←/→) they move the cursor as before.
            if !try_recall_history(ui_state, state, command) {
                move_input_cursor(ui_state, &state.input, command);
            }
            Ok(true)
        }
        TuiCommand::AgentDropdown(command) => {
            apply_agent_dropdown_command(state, ui_state, command);
            Ok(true)
        }
        TuiCommand::SkillDropdown(command) => {
            apply_skill_dropdown_command(state, ui_state, command);
            Ok(true)
        }
        TuiCommand::CommandDropdown(command) => {
            apply_command_dropdown_command(state, ui_state, command);
            Ok(true)
        }
        TuiCommand::FileMentionDropdown(command) => {
            apply_file_mention_dropdown_command(state, ui_state, command);
            Ok(true)
        }
        TuiCommand::Clarification(command) => {
            apply_clarification_command(state, ui_state, command, command_sender).await
        }
        TuiCommand::ClarificationInputCharacter(ch) => {
            // Typing implies the custom answer: move focus onto the custom row so
            // the input field is revealed and the cursor lands there.
            if let Some(clarification) = &state.pending_clarification {
                ui_state.clarification_option_index = clarification.options.len();
            }
            ui_state.clarification_custom_answer.push(ch);
            Ok(true)
        }
        TuiCommand::ClarificationInputBackspace => {
            ui_state.clarification_custom_answer.pop();
            Ok(true)
        }
        TuiCommand::QueueSelection(command) => {
            apply_queue_selection_command(state, ui_state, command);
            Ok(true)
        }
        TuiCommand::SessionBrowser(command) => {
            match command {
                // Resume dispatches to the worker (off-thread read → adopt_session
                // → lifecycle events), then closes the modal + clears the composer
                // so the re-rendered transcript shows unobstructed (task_11).
                SessionBrowserCommand::Resume(session_id) => {
                    queue_app_event(command_sender, AppEvent::ResumeSession(session_id)).await?;
                    apply_session_browser_command(ui_state, SessionBrowserCommand::Close);
                    clear_input(state, ui_state);
                }
                // Opening clears the composer so a `/sessions` trigger doesn't
                // linger (a Ctrl-R open has an empty composer, so it's a no-op there).
                SessionBrowserCommand::Open => {
                    apply_session_browser_command(ui_state, SessionBrowserCommand::Open);
                    clear_input(state, ui_state);
                }
                other => apply_session_browser_command(ui_state, other),
            }
            Ok(true)
        }
        TuiCommand::ReloadSkills => {
            reload_skills(state, ui_state);
            Ok(true)
        }
        TuiCommand::InputCharacter(ch) => {
            insert_input_character(state, ui_state, ch);
            Ok(true)
        }
        TuiCommand::InputBackspace => {
            remove_input_character_before_cursor(state, ui_state);
            Ok(true)
        }
        TuiCommand::InputKill(command) => {
            kill_input(state, ui_state, command);
            Ok(true)
        }
        TuiCommand::Dispatch(event) => {
            if matches_help_command(&event) {
                ui_state.help_visible = !ui_state.help_visible;
                clear_input(state, ui_state);
                return Ok(true);
            }
            // Finalize submission provenance and maintain the in-session recall
            // ring here, where `ui_state.prompt_history_cursor` is visible
            // (ADR-003/004): tag `Recalled` iff the composition originated from
            // the ring, then prepend the prompt so it is recallable this session.
            // `clear_input` (below) resets the cursor to a fresh draft.
            let event = if let AppEvent::PromptSubmitted(prompt, _) = event {
                let source = if ui_state.prompt_history_cursor != 0 {
                    PromptSource::Recalled
                } else {
                    PromptSource::Fresh
                };
                record_in_session_prompt(ui_state, &prompt);
                AppEvent::PromptSubmitted(prompt, source)
            } else {
                event
            };
            let clears_input = matches!(
                event,
                AppEvent::PromptSubmitted(_, _)
                    | AppEvent::ApprovalAnswered(_)
                    | AppEvent::GovernanceDecisionResolved(_, _)
                    | AppEvent::PlanApprovalResolved(_, _)
            );
            if let AppEvent::ApprovalAnswered(resolution) = &event {
                if let (Some(approval_handle), Some(pending)) =
                    (approval_handle, state.pending_approval.as_ref())
                {
                    approval_handle.resolve(*resolution);
                    if pending.group_id.is_some() {
                        if clears_input {
                            clear_input(state, ui_state);
                        }
                        return Ok(true);
                    }
                }
            }
            queue_app_event(command_sender, event).await?;
            if clears_input {
                clear_input(state, ui_state);
            }
            Ok(true)
        }
        TuiCommand::DispatchAndQuit(event) => {
            if matches!(event, AppEvent::RunInterruptRequested) {
                if let Some(interrupt_handle) = interrupt_handle {
                    interrupt_handle.request_interrupt();
                }
            }
            queue_app_event(command_sender, event).await?;
            Ok(false)
        }
    }
}

fn matches_help_command(event: &AppEvent) -> bool {
    matches!(event, AppEvent::PromptSubmitted(prompt, _) if prompt.trim() == "/help")
}

fn reload_skills(state: &mut AppState, ui_state: &mut TuiUiState) {
    let Some(working_directory) = ui_state.working_directory.as_deref() else {
        clear_input(state, ui_state);
        ui_state.status_message = Some("Skill reload unavailable".to_string());
        return;
    };
    let skill_suggestions = reload_skill_suggestions(working_directory);
    let skill_count = skill_suggestions.len();
    ui_state.skill_suggestions = skill_suggestions;
    ui_state.skill_selection_index = 0;
    clear_input(state, ui_state);
    ui_state.status_message = Some(format!("Skills reloaded: {skill_count}"));
}

fn sync_worker_state(state: &mut AppState, state_receiver: &mut watch::Receiver<AppState>) {
    while state_receiver.has_changed().unwrap_or(false) {
        let input = state.input.clone();
        let mut worker_state = state_receiver.borrow_and_update().clone();
        worker_state.input = input;
        *state = worker_state;
    }
}

/// Non-blockingly adopt the latest file-index snapshot from the worker, the
/// same shape as `sync_worker_state`. Only clones when a new snapshot has
/// arrived, so an idle draw loop does no work.
fn sync_file_index(
    ui_state: &mut TuiUiState,
    file_index_receiver: &mut watch::Receiver<Vec<FileEntry>>,
) {
    if file_index_receiver.has_changed().unwrap_or(false) {
        ui_state.file_mention_entries = file_index_receiver.borrow_and_update().clone();
    }
}

/// Adopt the latest off-thread session-list snapshot into the browser state
/// (task_07), keeping the selection within range after the list lands.
fn sync_session_summaries(
    ui_state: &mut TuiUiState,
    receiver: &mut watch::Receiver<Vec<SessionSummary>>,
) {
    if receiver.has_changed().unwrap_or(false) {
        ui_state.browser.summaries = receiver.borrow_and_update().clone();
        ui_state.browser.clamp_selection();
    }
}

/// Load the session summaries off the render thread and publish them to the
/// browser (task_07), mirroring `spawn_file_index_refresh`.
/// `list_session_summaries` is synchronous file I/O, so it runs inside
/// `spawn_blocking`; a join error (panic) or a closed receiver leaves the list
/// unchanged. No-op without a working directory.
fn spawn_session_summaries_load(
    working_directory: Option<PathBuf>,
    sender: watch::Sender<Vec<SessionSummary>>,
) {
    tokio::spawn(async move {
        let Some(working_directory) = working_directory else {
            return;
        };
        let data_root = working_directory.join(".atelier");
        if let Ok(summaries) =
            tokio::task::spawn_blocking(move || crate::history::list_session_summaries(&data_root))
                .await
        {
            let _ = sender.send(summaries);
        }
    });
}

/// Adopt the latest off-thread preview into the browser (task_08). The build is
/// pure and read-only; this only updates `ui_state`.
fn sync_session_preview(
    ui_state: &mut TuiUiState,
    receiver: &mut watch::Receiver<Option<SessionPreview>>,
) {
    if receiver.has_changed().unwrap_or(false) {
        let incoming = receiver.borrow_and_update().clone();
        // Drop a stale preview from a previously-selected session: a slow load
        // that resolves after the user moved on must not overwrite the preview
        // for the session now selected. A `None` (loading/cleared) always applies.
        let applies = match (&incoming, &ui_state.browser.preview_session_id) {
            (Some(preview), Some(current)) => &preview.session_id == current,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if applies {
            ui_state.browser.preview = incoming;
        }
    }
}

/// Build the read-only session preview off the render thread and publish it
/// (task_08), reusing the task_06 builder. `build_session_preview` is synchronous
/// file I/O, so it runs inside `spawn_blocking`; a build error or join failure
/// leaves the loading placeholder in place. Strictly read-only — no `App` touch.
fn spawn_session_preview_load(
    working_directory: Option<PathBuf>,
    session_id: String,
    sender: watch::Sender<Option<SessionPreview>>,
) {
    tokio::spawn(async move {
        let Some(working_directory) = working_directory else {
            return;
        };
        let data_root = working_directory.join(".atelier");
        let built = tokio::task::spawn_blocking(move || {
            crate::app::chat::build_session_preview(&data_root, &session_id)
        })
        .await;
        if let Ok(Ok(preview)) = built {
            let _ = sender.send(Some(preview));
        }
    });
}

/// Project this project's recall list off the render thread and publish it to
/// the TUI (ADR-004). `project_prompt_history` is synchronous file I/O, so it
/// runs inside `spawn_blocking`; a join error (panic) or a closed receiver
/// simply leaves the ring unchanged. No-op without a working directory.
async fn refresh_prompt_history(
    working_directory: Option<&Path>,
    max: usize,
    prompt_history_sender: &watch::Sender<Vec<String>>,
) {
    let Some(working_directory) = working_directory.map(Path::to_path_buf) else {
        return;
    };
    // Recall reads the `.atelier` data root (mirrors `HistoryStore::create`),
    // not the workspace root itself.
    let data_root = working_directory.join(".atelier");
    if let Ok(history) =
        tokio::task::spawn_blocking(move || crate::history::project_prompt_history(&data_root, max))
            .await
    {
        let _ = prompt_history_sender.send(history);
    }
}

/// Kick off the one-time recall load on a detached task (mirrors
/// `spawn_file_index_refresh`). Unlike the file-index walk this fires once at
/// startup: recall is seeded from disk and then maintained in memory (ADR-004),
/// so there is no periodic re-scan and the cold read never blocks the first
/// paint.
fn spawn_prompt_history_load(
    working_directory: Option<PathBuf>,
    max: usize,
    prompt_history_sender: watch::Sender<Vec<String>>,
) {
    tokio::spawn(async move {
        refresh_prompt_history(working_directory.as_deref(), max, &prompt_history_sender).await;
    });
}

/// Spawn the recall load only when the toggle is on, returning whether it
/// spawned. Disabled → no load and the ring stays empty (ADR-004). Split from
/// `run_tui` so the gate is unit-testable without a terminal.
fn maybe_spawn_prompt_history_load(
    enabled: bool,
    working_directory: Option<PathBuf>,
    max: usize,
    prompt_history_sender: watch::Sender<Vec<String>>,
) -> bool {
    if !enabled {
        return false;
    }
    spawn_prompt_history_load(working_directory, max, prompt_history_sender);
    true
}

/// Adopt the latest published recall list into UI state (mirrors
/// `sync_file_index`). The load publishes once at startup; later in-session
/// submissions mutate the ring directly (task_06), and the channel does not
/// change again, so this never clobbers them after the initial delivery.
fn sync_prompt_history(
    ui_state: &mut TuiUiState,
    prompt_history_receiver: &mut watch::Receiver<Vec<String>>,
) {
    if prompt_history_receiver.has_changed().unwrap_or(false) {
        ui_state.prompt_history = prompt_history_receiver.borrow_and_update().clone();
    }
}

async fn queue_app_event(
    command_sender: &mpsc::Sender<AppWorkerCommand>,
    event: AppEvent,
) -> Result<()> {
    command_sender
        .send(AppWorkerCommand::Event(event))
        .await
        .context("app worker is not accepting TUI events")
}

/// Cadence of the background git-context poll (ADR-006).
const GIT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Cadence of the background file-index refresh walk (ADR-003). Coarse on
/// purpose: the walk is off-thread but still real work, so it runs rarely and
/// only exists to surface files created mid-session.
const FILE_INDEX_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

/// Cadence of the roster refresh tick (ADR-004). 1 Hz keeps coarse elapsed and
/// stall detection current during a quiet step; cheap and change-gated, so it
/// only publishes when a row actually moves.
const ROSTER_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Walk the working directory off the worker's async thread and publish the
/// snapshot to the TUI. Extracted from the worker `select!` so it is
/// unit-testable in isolation (ADR-003). The `ignore`-crate walk is
/// synchronous, so it runs inside `spawn_blocking`; a join error (panic) or a
/// closed receiver simply leaves the previous snapshot in place. No-op when
/// there is no working directory.
async fn refresh_file_index(
    working_directory: Option<&Path>,
    file_index_sender: &watch::Sender<Vec<FileEntry>>,
    cancel: Arc<AtomicBool>,
) {
    let Some(root) = working_directory.map(Path::to_path_buf) else {
        return;
    };
    // A join error (walk panic) leaves the previous snapshot in place; a send
    // error means every receiver is gone (TUI shutting down).
    if let Ok(entries) =
        tokio::task::spawn_blocking(move || FileIndex::walk_cancellable(&root, &cancel)).await
    {
        let _ = file_index_sender.send(entries);
    }
}

/// Kick off a file-index refresh on a detached task instead of awaiting it.
///
/// The walk is unbounded (no timeout, one `canonicalize` syscall per entry) and
/// can outlive the 15s poll cadence on a large repo. Awaiting it inside the
/// worker `select!` would park the worker at that `await`, so queued
/// `AppWorkerCommand`s (including `Shutdown`) could not be serviced until the
/// walk returned. Detaching keeps the command arm pollable; the snapshot lands
/// via the `watch` channel whenever the walk finishes (last-writer-wins, so an
/// occasional overlap between two in-flight walks is harmless).
///
/// `cancel` is the worker's shutdown flag: it is shared across every spawned
/// walk so that, on quit, any in-flight walk stops promptly rather than holding
/// a `spawn_blocking` thread alive and delaying runtime (process) shutdown.
fn spawn_file_index_refresh(
    working_directory: Option<PathBuf>,
    file_index_sender: watch::Sender<Vec<FileEntry>>,
    cancel: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        refresh_file_index(working_directory.as_deref(), &file_index_sender, cancel).await;
    });
}

async fn run_app_worker(
    mut app: App,
    mut command_receiver: mpsc::Receiver<AppWorkerCommand>,
    file_index_sender: watch::Sender<Vec<FileEntry>>,
    working_directory: Option<PathBuf>,
) -> Result<()> {
    // Immediate startup refresh, then a change-gated poll every 5s. The poll
    // lives inside the worker's select loop, so it stops as soon as the loop
    // exits on shutdown — no separate task to abort (ADR-006).
    app.refresh_git_context().await;
    let mut git_poll = tokio::time::interval(GIT_POLL_INTERVAL);
    git_poll.tick().await; // consume the immediate first tick (startup covered it)

    // Shared shutdown flag for every detached file-index walk. Set on worker
    // exit so an in-flight `spawn_blocking` walk bails instead of keeping its
    // blocking thread (and thus process exit) alive on a large workspace.
    let walk_cancel = Arc::new(AtomicBool::new(false));

    // Initial off-thread file-index walk at startup, then a coarse periodic
    // refresh — mirrors the git poller (ADR-003). Both are detached
    // (`spawn_file_index_refresh`) rather than awaited so the cold startup walk
    // never blocks the command loop from servicing the first prompt, and so a
    // slow walk cannot starve `Shutdown`.
    spawn_file_index_refresh(
        working_directory.clone(),
        file_index_sender.clone(),
        walk_cancel.clone(),
    );
    let mut file_index_poll = tokio::time::interval(FILE_INDEX_REFRESH_INTERVAL);
    // A walk that overruns the interval must not trigger a burst of catch-up
    // walks; skip missed ticks and resume the coarse cadence.
    file_index_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    file_index_poll.tick().await; // consume the immediate first tick (startup covered it)

    // Bounded 1 Hz roster refresh (ADR-004). Stream deltas already refresh the
    // roster under load, so this only needs to fire while a step is quiet;
    // `refresh_roster_tick` self-gates to active runs and change-gates before
    // publishing. Skip missed ticks so heavy streaming can't queue a burst.
    let mut roster_poll = tokio::time::interval(ROSTER_REFRESH_INTERVAL);
    roster_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    roster_poll.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            command = command_receiver.recv() => match command {
                Some(AppWorkerCommand::Event(event)) => {
                    if let Err(error) = app.handle_event(event).await {
                        app.record_diagnostic(error.to_string())?;
                    }
                }
                Some(AppWorkerCommand::RecordHookLifecycle(record)) => {
                    if let Err(error) = app.record_hook_lifecycle(record) {
                        app.record_diagnostic(error.to_string())?;
                    }
                }
                Some(AppWorkerCommand::Shutdown) => {
                    walk_cancel.store(true, Ordering::Relaxed);
                    app.end_session()?;
                    return Ok(());
                }
                None => break,
            },
            _ = git_poll.tick() => {
                app.refresh_git_context().await;
            }
            _ = file_index_poll.tick() => {
                spawn_file_index_refresh(
                    working_directory.clone(),
                    file_index_sender.clone(),
                    walk_cancel.clone(),
                );
            }
            _ = roster_poll.tick() => {
                app.refresh_roster_tick();
            }
        }
    }

    // Channel closed (sender dropped) — signal any in-flight walk to stop too.
    walk_cancel.store(true, Ordering::Relaxed);
    app.end_session()
}

async fn shutdown_app_worker(
    command_sender: mpsc::Sender<AppWorkerCommand>,
    mut worker: JoinHandle<Result<()>>,
) -> Result<()> {
    let _ = command_sender.try_send(AppWorkerCommand::Shutdown);
    drop(command_sender);

    tokio::select! {
        result = &mut worker => result.context("app worker task failed")?,
        _ = tokio::time::sleep(Duration::from_millis(500)) => {
            worker.abort();
            match worker.await {
                Ok(result) => result?,
                Err(error) if error.is_cancelled() => {}
                Err(error) => return Err(error).context("app worker task failed"),
            }
            Ok(())
        }
    }
}

fn key_event_to_tui_command_with_ui(
    state: &AppState,
    ui_state: &TuiUiState,
    key: KeyEvent,
) -> Option<TuiCommand> {
    // Ignore key-RELEASE events. crossterm reports them on Windows (always) and
    // under the Kitty keyboard protocol; since `KeyChord` lookups and the modal
    // arms match on code+modifiers regardless of `kind`, routing a release would
    // fire every action twice (e.g. Ctrl-W deleting two words). Press and Repeat
    // (held-key autorepeat) still route. On Unix without enhancement flags every
    // event is already a Press, so this is a no-op there.
    if key.kind == KeyEventKind::Release {
        return None;
    }

    // Reserved-key chokepoint (ADR-004): Ctrl-C is the interrupt/quit kill-switch.
    // It is enforced here — before the modal cascade, normal routing, and (task_04)
    // any user-keymap lookup — so no context or future config can shadow it. This is
    // the single runtime definition of the reserved binding; the bindable allowlist
    // (`keybindings::is_portable`) excludes Ctrl-C on the validation side.
    if is_reserved_interrupt(&key) {
        return Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested));
    }

    if ui_state.help_visible {
        match key {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => Some(TuiCommand::ToggleHelp),
            // Tab navigation is handled entirely within the help-visible branch so
            // these keys never leak to the base handler. Right/Tab advance; Left/
            // Shift-Tab retreat. Shift-Tab is distinguished by the SHIFT modifier.
            KeyEvent {
                code: KeyCode::Right,
                ..
            }
            | KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                ..
            } => Some(TuiCommand::HelpNextTab),
            KeyEvent {
                code: KeyCode::Left,
                ..
            }
            | KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::SHIFT,
                ..
            }
            | KeyEvent {
                code: KeyCode::BackTab,
                ..
            } => Some(TuiCommand::HelpPrevTab),
            // Commands-tab type-to-filter (Phase 2): printable characters and
            // Backspace narrow the command list via `help_filter`. Captured only
            // on the Commands tab; every other tab leaves typed text inert. Nav
            // keys above (arrows/Tab) are not `Char` codes, so they never conflict.
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } if ui_state.help_active_tab == HelpTab::Commands => {
                Some(TuiCommand::HelpFilterBackspace)
            }
            KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            } if ui_state.help_active_tab == HelpTab::Commands
                && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) =>
            {
                Some(TuiCommand::HelpFilterCharacter(ch))
            }
            _ => None,
        }
    } else if ui_state.browser.visible {
        // Full modal (task_07/08): below help, above every other context. While
        // visible it consumes keys (nav / filter / preview-scroll / close);
        // unmatched keys are swallowed so nothing leaks to the composer. Help
        // still wins above. Routing is mode-aware (list vs preview).
        session_browser_key_command(&ui_state.browser, key)
    } else if state.pending_clarification.is_some() {
        // Ctrl-C is handled by the reserved-key guard above.
        clarification_key_command(state, ui_state, key)
    } else if state.pending_governance_decision.is_some() {
        // Ctrl-C is handled by the reserved-key guard above.
        governance_decision_key_command(state, key)
    } else if state.pending_plan_approval.is_some() {
        // The whole-plan DAG approval gate (ADR-005); Ctrl-C handled above.
        plan_approval_key_command(state, key)
    } else if state.pending_approval.is_some() {
        // Modal contexts keep default chat-scroll (PageUp/PageDown/Home/End) as a
        // fallback so keyboard users can still scroll while the modal is open, but the
        // rebindable keymap is not consulted here (ADR-003). Same for the dropdowns.
        key_event_to_tui_command(state, key).or_else(|| chat_scroll_command(&key))
    } else if agent_dropdown(state, ui_state).is_some() {
        agent_dropdown_key_command(key)
            .or_else(|| key_event_to_tui_command(state, key))
            .or_else(|| chat_scroll_command(&key))
    } else if skill_dropdown(&state.input, ui_state).is_some() {
        skill_dropdown_key_command(key)
            .or_else(|| key_event_to_tui_command(state, key))
            .or_else(|| chat_scroll_command(&key))
    } else if let Some(dropdown) = file_mention_dropdown(state, ui_state) {
        file_mention_dropdown_key_command(&dropdown, key)
            .or_else(|| key_event_to_tui_command(state, key))
            .or_else(|| chat_scroll_command(&key))
    } else if let Some(dropdown) = command_dropdown(state, ui_state) {
        command_dropdown_key_command(&dropdown, key)
            .or_else(|| key_event_to_tui_command(state, key))
            .or_else(|| chat_scroll_command(&key))
    } else if queue_control_active(state, ui_state) {
        queue_control_key_command(state, ui_state, key)
            .or_else(|| key_event_to_tui_command(state, key))
            .or_else(|| chat_scroll_command(&key))
    } else {
        // Normal-input context only (ADR-003): consult the active keymap first. A hit
        // maps through the exhaustive `command_for_action` bridge; a miss falls through
        // to the hardcoded handler. For keys the keymap does not own, default routing is
        // unchanged; the remappable actions (scroll/toggle/editing) are owned solely by
        // the keymap here so rebinds and unbinds take effect. The keymap is never
        // consulted in the modal branches above.
        if let Some(action) = ui_state.keymap.action_for(&key) {
            Some(command_for_action(action))
        } else {
            key_event_to_tui_command(state, key)
        }
    }
}

/// Map a key to a [`SessionBrowserCommand`] while the browser modal is visible
/// (task_07). `Esc` closes; `↑/↓` navigate; `Backspace` and printable characters
/// narrow the filter. Every other key returns `None` and is swallowed by the
/// modal (Ctrl-C is intercepted earlier by the reserved-key guard).
/// Build the Resume command for the browser's current selection, or `None` when
/// the (filtered) list is empty so `Enter` is a no-op rather than resuming
/// nothing. The session id is resolved here so the worker never reads UI state.
fn resume_command(browser: &SessionBrowserState) -> Option<TuiCommand> {
    browser
        .selected_session_id()
        .map(|id| TuiCommand::SessionBrowser(SessionBrowserCommand::Resume(id)))
}

fn session_browser_key_command(browser: &SessionBrowserState, key: KeyEvent) -> Option<TuiCommand> {
    use SessionBrowserCommand as Cmd;
    let command = match browser.mode {
        // List: Enter resumes the selection, → previews it, ↑/↓ navigate, typing
        // filters, Esc closes.
        BrowserMode::List => match key {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => Cmd::Close,
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => return resume_command(browser),
            KeyEvent {
                code: KeyCode::Right,
                ..
            } => Cmd::OpenPreview,
            KeyEvent {
                code: KeyCode::Up, ..
            } => Cmd::Up,
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => Cmd::Down,
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => Cmd::FilterBackspace,
            KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => Cmd::FilterChar(ch),
            _ => return None,
        },
        // Preview: Enter resumes, Esc returns to the list, chat-scroll keys move
        // the transcript.
        BrowserMode::Preview => match key {
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => return resume_command(browser),
            KeyEvent {
                code: KeyCode::Esc, ..
            } => Cmd::Back,
            KeyEvent {
                code: KeyCode::PageUp,
                ..
            } => Cmd::ScrollPreview(EventScrollCommand::PageUp),
            KeyEvent {
                code: KeyCode::PageDown,
                ..
            } => Cmd::ScrollPreview(EventScrollCommand::PageDown),
            KeyEvent {
                code: KeyCode::Up, ..
            } => Cmd::ScrollPreview(EventScrollCommand::LinesUp(1)),
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => Cmd::ScrollPreview(EventScrollCommand::LinesDown(1)),
            KeyEvent {
                code: KeyCode::Home,
                ..
            } => Cmd::ScrollPreview(EventScrollCommand::Top),
            KeyEvent {
                code: KeyCode::End, ..
            } => Cmd::ScrollPreview(EventScrollCommand::Bottom),
            _ => return None,
        },
    };
    Some(TuiCommand::SessionBrowser(command))
}

/// Queue focus is active when the input composer is empty and there are queued
/// follow-ups, with no higher-priority mode (help / clarification / approval)
/// open. The `/agent:` and `/skill:` dropdowns require non-empty input, so they
/// are never active at the same time as queue focus.
fn queue_control_active(state: &AppState, ui_state: &TuiUiState) -> bool {
    state.input.is_empty()
        && !state.queued_follow_ups.is_empty()
        && !ui_state.help_visible
        && state.pending_clarification.is_none()
        && state.pending_governance_decision.is_none()
        && state.pending_plan_approval.is_none()
        && state.pending_approval.is_none()
}

fn selected_queue_item<'a>(
    state: &'a AppState,
    ui_state: &TuiUiState,
) -> Option<&'a QueuedFollowUpView> {
    let items = &state.queued_follow_ups;
    if items.is_empty() {
        return None;
    }
    let index = ui_state
        .queue_selection_index
        .min(items.len().saturating_sub(1));
    items.get(index)
}

fn queue_control_key_command(
    state: &AppState,
    ui_state: &TuiUiState,
    key: KeyEvent,
) -> Option<TuiCommand> {
    match key {
        KeyEvent {
            code: KeyCode::Up, ..
        } => Some(TuiCommand::QueueSelection(QueueSelectionCommand::Previous)),
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => Some(TuiCommand::QueueSelection(QueueSelectionCommand::Next)),
        KeyEvent {
            code: KeyCode::Delete,
            ..
        } => {
            let item = selected_queue_item(state, ui_state)?;
            // Cancellation is only meaningful before an item is replaying or
            // already cancelled.
            if matches!(
                item.status,
                QueuedFollowUpStatus::Pending | QueuedFollowUpStatus::Paused
            ) {
                Some(TuiCommand::Dispatch(AppEvent::FollowUpCancelled(
                    item.id.clone(),
                )))
            } else {
                None
            }
        }
        KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => {
            let item = selected_queue_item(state, ui_state)?;
            if item.status == QueuedFollowUpStatus::Paused {
                Some(TuiCommand::Dispatch(AppEvent::FollowUpResumeRequested(
                    item.id.clone(),
                )))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn apply_queue_selection_command(
    state: &AppState,
    ui_state: &mut TuiUiState,
    command: QueueSelectionCommand,
) {
    let count = state.queued_follow_ups.len();
    if count == 0 {
        ui_state.queue_selection_index = 0;
        return;
    }
    let current = ui_state.queue_selection_index.min(count - 1);
    ui_state.queue_selection_index = match command {
        QueueSelectionCommand::Previous => {
            if current == 0 {
                count - 1
            } else {
                current - 1
            }
        }
        QueueSelectionCommand::Next => (current + 1) % count,
    };
}

/// Apply a [`SessionBrowserCommand`] to the modal state (task_07). The summaries
/// refresh on `Open` is triggered by `run_loop` (it owns the watch channel), so
/// this only mutates visibility / selection / filter.
fn apply_session_browser_command(ui_state: &mut TuiUiState, command: SessionBrowserCommand) {
    let browser = &mut ui_state.browser;
    match command {
        SessionBrowserCommand::Open => {
            browser.visible = true;
            browser.mode = BrowserMode::List;
            browser.filter.clear();
            browser.selection_index = 0;
            browser.preview = None;
            browser.preview_session_id = None;
            browser.preview_scroll = 0;
        }
        SessionBrowserCommand::Close => {
            // Full reset so the next open starts clean (no stale summaries/filter).
            *browser = SessionBrowserState::default();
        }
        SessionBrowserCommand::Up => {
            browser.selection_index = browser.selection_index.saturating_sub(1);
        }
        SessionBrowserCommand::Down => {
            let len = browser.filtered_indices().len();
            if len > 0 && browser.selection_index + 1 < len {
                browser.selection_index += 1;
            }
        }
        SessionBrowserCommand::FilterChar(ch) => {
            browser.filter.push(ch);
            browser.selection_index = 0;
        }
        SessionBrowserCommand::FilterBackspace => {
            browser.filter.pop();
            browser.selection_index = 0;
        }
        SessionBrowserCommand::OpenPreview => {
            // Enter on the selected list row: switch to preview mode and mark which
            // session to load. `run_loop` spawns the off-thread fold; the preview
            // stays `None` (loading placeholder) until it lands.
            if let Some(&index) = browser.filtered_indices().get(browser.selection_index) {
                let session_id = browser.summaries[index].session_id.clone();
                browser.mode = BrowserMode::Preview;
                browser.preview = None;
                browser.preview_session_id = Some(session_id);
                browser.preview_scroll = 0;
            }
        }
        SessionBrowserCommand::Back => {
            // Return to the list, discarding the loaded preview.
            browser.mode = BrowserMode::List;
            browser.preview = None;
            browser.preview_session_id = None;
            browser.preview_scroll = 0;
        }
        SessionBrowserCommand::ScrollPreview(scroll) => {
            apply_preview_scroll(browser, scroll);
        }
        SessionBrowserCommand::Resume(_) => {
            // Resume is intercepted in `execute_tui_command_with_interrupt` (it
            // dispatches `AppEvent::ResumeSession` to the worker and then closes
            // the modal); it never reaches this UI-only mutator.
        }
    }
}

/// Number of transcript lines `render_session_browser` produces for a preview —
/// one title + optional summary + body lines + a blank separator per item. Kept
/// in sync with the preview line builder so scroll clamping matches the render.
fn preview_total_lines(preview: &SessionPreview) -> usize {
    preview
        .items
        .iter()
        .map(|item| 1 + usize::from(item.summary.is_some()) + item.body.len() + 1)
        .sum()
}

/// Apply a chat-style scroll command to the preview offset, clamped to the
/// transcript length. A no-op while the preview is still loading.
fn apply_preview_scroll(browser: &mut SessionBrowserState, scroll: EventScrollCommand) {
    let Some(preview) = browser.preview.as_ref() else {
        return;
    };
    const PAGE: usize = 10;
    let max = preview_total_lines(preview).saturating_sub(1);
    browser.preview_scroll = match scroll {
        EventScrollCommand::Top => 0,
        EventScrollCommand::Bottom => max,
        EventScrollCommand::PageUp => browser.preview_scroll.saturating_sub(PAGE),
        EventScrollCommand::PageDown => (browser.preview_scroll + PAGE).min(max),
        EventScrollCommand::LinesUp(n) => browser.preview_scroll.saturating_sub(n),
        EventScrollCommand::LinesDown(n) => (browser.preview_scroll + n).min(max),
    };
}

fn agent_dropdown_key_command(key: KeyEvent) -> Option<TuiCommand> {
    match key {
        KeyEvent {
            code: KeyCode::Up, ..
        } => Some(TuiCommand::AgentDropdown(DropdownCommand::Previous)),
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => Some(TuiCommand::AgentDropdown(DropdownCommand::Next)),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => Some(TuiCommand::AgentDropdown(DropdownCommand::Accept)),
        _ => None,
    }
}

fn skill_dropdown_key_command(key: KeyEvent) -> Option<TuiCommand> {
    match key {
        KeyEvent {
            code: KeyCode::Up, ..
        } => Some(TuiCommand::SkillDropdown(DropdownCommand::Previous)),
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => Some(TuiCommand::SkillDropdown(DropdownCommand::Next)),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => Some(TuiCommand::SkillDropdown(DropdownCommand::Accept)),
        _ => None,
    }
}

fn command_dropdown_key_command(dropdown: &CommandDropdown, key: KeyEvent) -> Option<TuiCommand> {
    use CommandDropdownCommand::{Accept, Dismiss, Next, Previous, TrapNoMatch};
    match key.code {
        // Escape dismisses in either state; the input is left untouched.
        KeyCode::Esc => Some(TuiCommand::CommandDropdown(Dismiss)),
        // Selection and acceptance only apply when there are selectable rows.
        KeyCode::Up if !dropdown.empty => Some(TuiCommand::CommandDropdown(Previous)),
        KeyCode::Down if !dropdown.empty => Some(TuiCommand::CommandDropdown(Next)),
        KeyCode::Tab | KeyCode::Enter if !dropdown.empty => {
            Some(TuiCommand::CommandDropdown(Accept))
        }
        // No-match: trap Enter so invalid slash input is never submitted. Other
        // keys (chars, Backspace) fall through so editing re-filters the list.
        KeyCode::Enter => Some(TuiCommand::CommandDropdown(TrapNoMatch)),
        _ => None,
    }
}

fn file_mention_dropdown_key_command(
    dropdown: &FileMentionDropdown,
    key: KeyEvent,
) -> Option<TuiCommand> {
    use FileMentionDropdownCommand::{Accept, Dismiss, Next, Previous};
    match key.code {
        // Escape dismisses in either state; the input is left untouched.
        KeyCode::Esc => Some(TuiCommand::FileMentionDropdown(Dismiss)),
        // Selection and acceptance only apply when there are selectable rows.
        KeyCode::Up if !dropdown.empty => Some(TuiCommand::FileMentionDropdown(Previous)),
        KeyCode::Down if !dropdown.empty => Some(TuiCommand::FileMentionDropdown(Next)),
        KeyCode::Tab | KeyCode::Enter if !dropdown.empty => {
            Some(TuiCommand::FileMentionDropdown(Accept))
        }
        // No-match: unlike the command dropdown, do NOT trap Enter — a typed
        // `@query` with no matches submits normally. All other keys fall through
        // so editing re-filters the list.
        _ => None,
    }
}

fn clarification_key_command(
    state: &AppState,
    ui_state: &TuiUiState,
    key: KeyEvent,
) -> Option<TuiCommand> {
    let clarification = state.pending_clarification.as_ref()?;
    // An answer is already in flight; freeze the composer (Ctrl-C still falls
    // through to the caller) until the worker clears the pending clarification.
    if ui_state.clarification_submitting {
        return None;
    }
    // The custom answer lives on a synthetic row past the real options. Typing
    // is only routed into the custom field while that row is focused.
    let custom_row = clarification.options.len();
    let focused = ui_state.clarification_option_index.min(custom_row);
    let custom_focused = focused == custom_row;
    let multi_select = clarification.multi_select;

    match key {
        KeyEvent {
            code: KeyCode::Up, ..
        } => Some(TuiCommand::Clarification(
            ClarificationCommand::PreviousOption,
        )),
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => Some(TuiCommand::Clarification(ClarificationCommand::NextOption)),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => Some(TuiCommand::Clarification(ClarificationCommand::Submit)),
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } if custom_focused => Some(TuiCommand::ClarificationInputBackspace),
        // Space toggles the focused option only in multi-select; in single
        // select (and while not typing custom) it is ignored.
        KeyEvent {
            code: KeyCode::Char(' '),
            ..
        } if !custom_focused && multi_select => Some(TuiCommand::Clarification(
            ClarificationCommand::ToggleOption,
        )),
        KeyEvent {
            code: KeyCode::Char(' '),
            ..
        } if !custom_focused => None,
        // Digits 1-9 jump focus to that option (Claude-CLI style quick-select)
        // while not editing the custom answer. Out-of-range digits are ignored.
        KeyEvent {
            code: KeyCode::Char(ch @ '1'..='9'),
            modifiers,
            ..
        } if !custom_focused && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) => {
            let index = (ch as usize) - ('1' as usize);
            (index < clarification.options.len()).then_some(TuiCommand::Clarification(
                ClarificationCommand::FocusOption(index),
            ))
        }
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            Some(TuiCommand::ClarificationInputCharacter(ch))
        }
        _ => None,
    }
}

/// Key routing while a governance decision is pending. Accept is deliberately a
/// Ctrl-modified key so it can never be hit by accident (or while composing a
/// redirect); the safe default keys (Enter on an empty line, unknown keys) never
/// accept. A redirect is composed in the normal input line and sent with Enter;
/// Esc rejects/aborts outright.
fn governance_decision_key_command(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    let decision_id = state
        .pending_governance_decision
        .as_ref()?
        .decision_id
        .clone();
    match key {
        // Explicit, deliberate accept — Ctrl-modified so it cannot collide with
        // typed redirect text or be triggered accidentally.
        KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
            decision_id,
            GovernanceAnswer::Accept,
        ))),
        // Esc rejects/aborts the run (any composed redirect is discarded).
        KeyEvent {
            code: KeyCode::Esc, ..
        } => Some(TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
            decision_id,
            GovernanceAnswer::Reject { redirect: None },
        ))),
        // Enter rejects *with* the composed redirect when one was typed; with an
        // empty line it is a no-op, so the default key never accepts.
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => {
            let redirect = state.input.trim();
            (!redirect.is_empty()).then(|| {
                TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
                    decision_id,
                    GovernanceAnswer::Reject {
                        redirect: Some(redirect.to_string()),
                    },
                ))
            })
        }
        // Compose the optional redirect in the normal input line.
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => Some(TuiCommand::InputBackspace),
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            Some(TuiCommand::InputCharacter(ch))
        }
        _ => None,
    }
}

/// Key routing for the whole-plan DAG approval gate (ADR-005), mirroring
/// `governance_decision_key_command`: Ctrl-Y accepts; Esc rejects; Enter rejects
/// with the composed reason (empty line = no-op so the default key never
/// accepts); other keys compose the optional reason.
fn plan_approval_key_command(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    let question_id = state.pending_plan_approval.as_ref()?.question_id.clone();
    match key {
        KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(TuiCommand::Dispatch(AppEvent::PlanApprovalResolved(
            question_id,
            PlanApprovalAnswer::Accept,
        ))),
        KeyEvent {
            code: KeyCode::Esc, ..
        } => Some(TuiCommand::Dispatch(AppEvent::PlanApprovalResolved(
            question_id,
            PlanApprovalAnswer::Reject { reason: None },
        ))),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => {
            let reason = state.input.trim();
            (!reason.is_empty()).then(|| {
                TuiCommand::Dispatch(AppEvent::PlanApprovalResolved(
                    question_id,
                    PlanApprovalAnswer::Reject {
                        reason: Some(reason.to_string()),
                    },
                ))
            })
        }
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => Some(TuiCommand::InputBackspace),
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            Some(TuiCommand::InputCharacter(ch))
        }
        _ => None,
    }
}

/// The reserved interrupt/quit chord: `Ctrl-C`. Matched at the single chokepoint in
/// `key_event_to_tui_command_with_ui` so it can never be shadowed by a modal context
/// or a user keymap. Structurally non-bindable (excluded from `keybindings::is_portable`).
fn is_reserved_interrupt(key: &KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}

/// Chat-viewport scroll for the default navigation keys (`PageUp`/`PageDown`/
/// `Home`/`End`), modifier-agnostic. Used as a fallback in the modal/dropdown
/// contexts — where the rebindable keymap is deliberately not consulted (ADR-003)
/// — so keyboard-only users keep chat scrollback while an approval or dropdown is
/// open, matching the pre-keymap behavior. The normal-input branch does NOT use
/// this (the keymap owns scroll there, honoring rebinds/unbinds).
fn chat_scroll_command(key: &KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::PageUp => Some(TuiCommand::ScrollEvents(EventScrollCommand::PageUp)),
        KeyCode::PageDown => Some(TuiCommand::ScrollEvents(EventScrollCommand::PageDown)),
        KeyCode::Home => Some(TuiCommand::ScrollEvents(EventScrollCommand::Top)),
        KeyCode::End => Some(TuiCommand::ScrollEvents(EventScrollCommand::Bottom)),
        _ => None,
    }
}

fn key_event_to_tui_command(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    match key {
        // Ctrl-C (interrupt/quit) is owned by the reserved-key guard in
        // `key_event_to_tui_command_with_ui`; it never reaches this handler in prod.
        //
        // The remappable normal-mode actions (Ctrl-L toggle-roster, PageUp/PageDown
        // and Home/End scroll) are NOT handled here: they are owned by the active
        // `Keymap`, consulted in the normal-input branch of the wrapper (task_04/08).
        // Keeping them here would shadow a user rebind/unbind of their default key,
        // and would also leak them into modal fallbacks (the keymap is normal-only).
        //
        // Ctrl-R opens the session browser (task_07; hardcoded — no keybinding
        // config yet). Guarded off while a blocking modal (approval / clarification
        // / governance) is pending so it can't shadow those answer paths.
        KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } if state.pending_approval.is_none()
            && state.pending_clarification.is_none()
            && state.pending_governance_decision.is_none()
            && state.pending_plan_approval.is_none() =>
        {
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Open))
        }
        KeyEvent {
            code: KeyCode::Up, ..
        } => Some(TuiCommand::MoveInputCursor(InputCursorCommand::Up)),
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => Some(TuiCommand::MoveInputCursor(InputCursorCommand::Down)),
        KeyEvent {
            code: KeyCode::Left,
            ..
        } => Some(TuiCommand::MoveInputCursor(InputCursorCommand::Left)),
        KeyEvent {
            code: KeyCode::Right,
            ..
        } => Some(TuiCommand::MoveInputCursor(InputCursorCommand::Right)),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.input.trim() == "/help" => Some(TuiCommand::ToggleHelp),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.input.trim() == "/sessions" => {
            // /sessions opens the same browser as Ctrl-R (task_09 discoverability).
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Open))
        }
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.input.trim() == RELOAD_SKILLS_COMMAND => Some(TuiCommand::ReloadSkills),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.pending_approval.is_some() => {
            // Tier-aware resolution (ADR-001/002): approve-once, a distinct
            // approve-and-trust token, deny — with the High tier requiring the
            // explicit word and the catastrophic core requiring type-to-confirm.
            let resolution = state
                .pending_approval
                .as_ref()
                .map(|view| parse_approval_resolution(&state.input, view))
                .unwrap_or(ApprovalResolution::Deny);
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(resolution)))
        }
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
            state.input.clone(),
            // Placeholder: this routing layer has no `ui_state`, so the real
            // provenance (Recalled iff the composition came from the ring) is
            // finalized in the `Dispatch` arm of `execute_tui_command`.
            PromptSource::Fresh,
        ))),
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => Some(TuiCommand::InputBackspace),
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            Some(TuiCommand::InputCharacter(ch))
        }
        _ => None,
    }
}

fn mouse_event_to_tui_command(ui_state: &TuiUiState, mouse: MouseEvent) -> Option<TuiCommand> {
    if ui_state.help_visible || !rect_contains(ui_state.event_area, mouse.column, mouse.row) {
        return None;
    }

    match mouse.kind {
        MouseEventKind::ScrollUp => Some(TuiCommand::ScrollEvents(EventScrollCommand::LinesUp(
            MOUSE_SCROLL_LINES,
        ))),
        MouseEventKind::ScrollDown => Some(TuiCommand::ScrollEvents(
            EventScrollCommand::LinesDown(MOUSE_SCROLL_LINES),
        )),
        _ => None,
    }
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

/// Map the typed approval input to a resolution, honoring the habituation
/// controls (ADR-001/002): a distinct approve-and-trust token (only when a trust
/// target exists); the catastrophic core requires retyping the exact resolved
/// command (type-to-confirm); the High tier requires the explicit word `approve`
/// (a reflexive `y`/Enter denies); Low/Medium accept the usual short affirmations.
fn parse_approval_resolution(input: &str, view: &PendingApprovalView) -> ApprovalResolution {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();

    // Catastrophic confirmation is checked FIRST: a catastrophic action can never be
    // trusted away (ADR-001/002), so its type-to-confirm requirement must not be
    // bypassable by the `t`/`trust` shortcut even if a trust_target were ever set on a
    // catastrophic view.
    if view.catastrophic {
        let confirmed = match view.resolved_command.as_deref() {
            Some(command) => trimmed == command,
            None => lower == "confirm",
        };
        return if confirmed {
            ApprovalResolution::ApproveOnce
        } else {
            ApprovalResolution::Deny
        };
    }

    if view.trust_target.is_some() && matches!(lower.as_str(), "t" | "trust") {
        return ApprovalResolution::ApproveAndTrust;
    }

    let is_high = view.tier.map(crate::app::risk_tier_label) == Some("high");
    if is_high {
        return if matches!(lower.as_str(), "approve" | "approved") {
            ApprovalResolution::ApproveOnce
        } else {
            ApprovalResolution::Deny
        };
    }

    if matches!(lower.as_str(), "y" | "yes" | "approve" | "approved") {
        ApprovalResolution::ApproveOnce
    } else {
        ApprovalResolution::Deny
    }
}

/// Render the rich decision-support modal lines from the enriched
/// `PendingApprovalView` (ADR-001). The tier is conveyed by an explicit text
/// label (so it survives `NO_COLOR`) plus a tier-colored accent; details
/// (resolved command, affected paths, boundary, reversibility) follow the lead
/// line, and the key hint adapts to the tier.
fn approval_modal_lines(
    pending: &PendingApprovalView,
    theme: &Theme,
    show_first_approval_explainer: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if show_first_approval_explainer {
        lines.push(Line::styled(
            crate::app::chat::FIRST_APPROVAL_EXPLAINER,
            Style::default().fg(theme.text_muted),
        ));
    }

    let tier_label = pending.tier.map(crate::app::risk_tier_label);
    let (tier_text, tier_color) = match tier_label {
        Some("low") => ("LOW", theme.risk_low),
        Some("medium") => ("MEDIUM", theme.risk_medium),
        Some("high") => ("HIGH", theme.risk_high),
        _ => ("RISK", theme.text_muted),
    };
    // Lead line: explicit tier label (+ catastrophic marker) then the agent.
    let mut lead = format!("[{tier_text}]");
    if pending.catastrophic {
        lead.push_str(" [CATASTROPHIC]");
    }
    lead.push_str(&format!(" Approval required for {}", pending.agent));
    lines.push(Line::styled(
        lead,
        Style::default().fg(tier_color).add_modifier(Modifier::BOLD),
    ));

    // One-line reason.
    if let Some(reason) = pending.reason.as_deref() {
        lines.push(Line::styled(
            reason.to_string(),
            Style::default().fg(theme.text),
        ));
    } else if let Some(diagnostic) = pending.diagnostic.as_deref() {
        lines.push(Line::styled(
            diagnostic.to_string(),
            Style::default().fg(theme.text),
        ));
    }

    // Detail: resolved command, or a diff preview when present.
    if let Some(command) = pending.resolved_command.as_deref() {
        lines.push(Line::styled(
            format!("$ {command}"),
            Style::default().fg(theme.text_muted),
        ));
    }
    if let Some(diff) = pending.diff.as_deref() {
        for diff_line in diff.lines().take(8) {
            lines.push(Line::styled(
                diff_line.to_string(),
                Style::default().fg(theme.text_dim),
            ));
        }
    }
    if !pending.affected_paths.is_empty() {
        lines.push(Line::styled(
            format!("Affected: {}", pending.affected_paths.join(", ")),
            Style::default().fg(theme.text_muted),
        ));
    }
    if let Some(boundary) = pending.boundary_crossed.as_deref() {
        lines.push(Line::styled(
            format!("Boundary: {boundary}"),
            Style::default().fg(theme.status_warn),
        ));
    }
    if let Some(reversible) = pending.reversible {
        lines.push(Line::styled(
            format!("Reversible: {}", if reversible { "yes" } else { "no" }),
            Style::default().fg(theme.text_muted),
        ));
    }
    // Drift interlock context for the first mutation after a drifted resume
    // (ADR-004): surface it prominently so a reflexive approve cannot silently
    // write to a moved tree.
    if let Some(drift) = pending.drift_notice.as_deref() {
        lines.push(Line::styled(
            format!("⚠ {drift}"),
            Style::default()
                .fg(theme.status_warn)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Key hint, adapted to the tier and trust availability.
    let trust_hint = if pending.trust_target.is_some() {
        " · t = approve & trust"
    } else {
        ""
    };
    let hint = if pending.catastrophic {
        match pending.resolved_command.as_deref() {
            Some(command) => format!("Type the command exactly to confirm: {command} · n = deny"),
            None => "Type confirm to approve · n = deny".to_string(),
        }
    } else if tier_label == Some("high") {
        format!("Type approve to allow{trust_hint} · n = deny")
    } else {
        format!("y = approve{trust_hint} · n = deny")
    };
    lines.push(Line::styled(hint, Style::default().fg(theme.accent)));

    lines
}

fn clear_input(state: &mut AppState, ui_state: &mut TuiUiState) {
    state.input.clear();
    ui_state.input_cursor = 0;
    ui_state.input_preferred_col = None;
    // Clearing returns the composer to a fresh live draft, so recall starts over
    // from the newest entry and the next submission is Fresh (ADR-003).
    ui_state.prompt_history_cursor = 0;
    ui_state.prompt_history_draft.clear();
    clear_command_dropdown_dismissal(ui_state);
    clear_file_mention_dropdown_dismissal(ui_state);
    reset_dropdown_selections(ui_state);
}

fn clamp_input_cursor(ui_state: &mut TuiUiState, input: &str) {
    ui_state.input_cursor = ui_state.input_cursor.min(input_char_count(input));
}

fn input_char_count(input: &str) -> usize {
    input.chars().count()
}

fn byte_index_for_char(input: &str, char_index: usize) -> usize {
    input
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(input.len())
}

fn insert_input_character(state: &mut AppState, ui_state: &mut TuiUiState, ch: char) {
    clamp_input_cursor(ui_state, &state.input);
    let byte_index = byte_index_for_char(&state.input, ui_state.input_cursor);
    state.input.insert(byte_index, ch);
    ui_state.input_cursor += 1;
    ui_state.input_preferred_col = None;
    ui_state.status_message = None;
    clear_command_dropdown_dismissal(ui_state);
    clear_file_mention_dropdown_dismissal(ui_state);
    reset_dropdown_selections(ui_state);
}

fn remove_input_character_before_cursor(state: &mut AppState, ui_state: &mut TuiUiState) {
    clamp_input_cursor(ui_state, &state.input);
    if ui_state.input_cursor == 0 {
        return;
    }

    let removed_char_index = ui_state.input_cursor - 1;
    let start = byte_index_for_char(&state.input, removed_char_index);
    let end = byte_index_for_char(&state.input, ui_state.input_cursor);
    state.input.replace_range(start..end, "");
    ui_state.input_cursor = removed_char_index;
    ui_state.input_preferred_col = None;
    ui_state.status_message = None;
    // Backspacing a recalled composition all the way to empty returns to a fresh
    // live draft, so a delete-all-then-retype is tagged Fresh (ADR-003).
    if state.input.is_empty() {
        ui_state.prompt_history_cursor = 0;
        ui_state.prompt_history_draft.clear();
    }
    clear_command_dropdown_dismissal(ui_state);
    clear_file_mention_dropdown_dismissal(ui_state);
    reset_dropdown_selections(ui_state);
}

/// Apply a readline-style kill to the composer, mutating `state.input` and the
/// char-indexed `input_cursor`. UTF-8 safe (char-indexed logic, byte-indexed
/// `replace_range`) and a no-op at edge cursors / on empty input. Mirrors the
/// dropdown/status/history housekeeping of the other input mutators.
fn kill_input(state: &mut AppState, ui_state: &mut TuiUiState, command: InputKillCommand) {
    clamp_input_cursor(ui_state, &state.input);
    let cursor = ui_state.input_cursor;
    let char_count = input_char_count(&state.input);
    // Char-index half-open range [start, end) to delete, and the resulting cursor.
    let (start_char, end_char, new_cursor) = match command {
        InputKillCommand::ToLineEnd => (cursor, char_count, cursor),
        InputKillCommand::ToLineStart => (0, cursor, 0),
        InputKillCommand::WordBack => {
            let start = word_back_start(&state.input, cursor);
            (start, cursor, start)
        }
    };
    if start_char >= end_char {
        return; // empty range: nothing to kill (covers empty input + edge cursors)
    }

    let start_byte = byte_index_for_char(&state.input, start_char);
    let end_byte = byte_index_for_char(&state.input, end_char);
    state.input.replace_range(start_byte..end_byte, "");
    ui_state.input_cursor = new_cursor;
    ui_state.input_preferred_col = None;
    ui_state.status_message = None;
    // Killing a recalled composition down to empty returns to a fresh draft, matching
    // `remove_input_character_before_cursor`.
    if state.input.is_empty() {
        ui_state.prompt_history_cursor = 0;
        ui_state.prompt_history_draft.clear();
    }
    clear_command_dropdown_dismissal(ui_state);
    clear_file_mention_dropdown_dismissal(ui_state);
    reset_dropdown_selections(ui_state);
}

/// The char index where the word before `cursor` begins, for `WordBack`: skip a
/// trailing run of whitespace, then the run of non-whitespace word characters
/// (readline `unix-word-rubout`).
fn word_back_start(input: &str, cursor: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let mut i = cursor.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

fn move_input_cursor(ui_state: &mut TuiUiState, input: &str, command: InputCursorCommand) {
    clamp_input_cursor(ui_state, input);
    let input_len = input_char_count(input);
    match command {
        InputCursorCommand::Left => {
            ui_state.input_cursor = ui_state.input_cursor.saturating_sub(1);
            ui_state.input_preferred_col = None;
        }
        InputCursorCommand::Right => {
            ui_state.input_cursor = ui_state.input_cursor.saturating_add(1).min(input_len);
            ui_state.input_preferred_col = None;
        }
        InputCursorCommand::Up | InputCursorCommand::Down => {
            move_input_cursor_vertically(ui_state, input_len, command);
        }
        InputCursorCommand::LineStart => {
            ui_state.input_cursor = 0;
            ui_state.input_preferred_col = None;
        }
        InputCursorCommand::LineEnd => {
            ui_state.input_cursor = input_len;
            ui_state.input_preferred_col = None;
        }
    }
    reset_dropdown_selections(ui_state);
}

fn move_input_cursor_vertically(
    ui_state: &mut TuiUiState,
    input_len: usize,
    command: InputCursorCommand,
) {
    let width = ui_state.input_width.max(1);
    let current_line = ui_state.input_cursor / width;
    let current_col = ui_state.input_cursor % width;
    let preferred_col = ui_state.input_preferred_col.unwrap_or(current_col);
    let target_line = match command {
        InputCursorCommand::Up if current_line > 0 => current_line - 1,
        InputCursorCommand::Down if current_line < input_len / width => current_line + 1,
        _ => {
            ui_state.input_preferred_col = Some(preferred_col);
            return;
        }
    };
    let line_start = target_line * width;
    let line_end = line_start.saturating_add(width).min(input_len);
    ui_state.input_cursor = line_start.saturating_add(preferred_col).min(line_end);
    ui_state.input_preferred_col = Some(preferred_col);
}

/// Walk the recall ring in response to ↑/↓ at the input's top/bottom visual-row
/// boundary, returning whether the key was consumed (the caller falls back to
/// ordinary cursor navigation on `false`). ADR-001/003:
///
/// - Recall fires only at the boundary (`current_line == 0` for ↑, the last
///   visual row for ↓), reusing the same row math as
///   `move_input_cursor_vertically`. Inside a wrapped draft or recalled entry,
///   ↑/↓ keep moving the cursor — this gate is what avoids the #1 competitor
///   bug (multi-line collision).
/// - The live draft is saved when entering history (cursor `0 → 1`) and restored
///   when ↓ steps back past the newest entry (cursor `1 → 0`).
/// - Each step replaces `state.input` and parks the cursor at the end of the
///   recalled text; `prompt_history_cursor` tracks depth (`0` = live draft,
///   `N` = Nth-newest entry).
///
/// Yields entirely when the ring is empty (which also covers the disabled case,
/// since the loader never populates it). Dropdown / queue / clarification
/// precedence is handled upstream in `key_event_to_tui_command_with_ui`, so this
/// is only ever reached for plain-input ↑/↓.
fn try_recall_history(
    ui_state: &mut TuiUiState,
    state: &mut AppState,
    command: InputCursorCommand,
) -> bool {
    if ui_state.prompt_history.is_empty() {
        return false;
    }
    clamp_input_cursor(ui_state, &state.input);
    let width = ui_state.input_width.max(1);
    let input_len = input_char_count(&state.input);
    let current_line = ui_state.input_cursor / width;
    let last_line = input_len / width;
    match command {
        InputCursorCommand::Up => {
            if current_line != 0 {
                return false; // mid-draft: let the cursor move up a visual row
            }
            if ui_state.prompt_history_cursor >= ui_state.prompt_history.len() {
                return true; // already at the oldest entry — consume, no change
            }
            if ui_state.prompt_history_cursor == 0 {
                ui_state.prompt_history_draft = state.input.clone();
            }
            ui_state.prompt_history_cursor += 1;
            let entry = ui_state.prompt_history[ui_state.prompt_history_cursor - 1].clone();
            set_recalled_input(ui_state, state, entry);
            true
        }
        InputCursorCommand::Down => {
            if current_line != last_line {
                return false; // mid-draft: let the cursor move down a visual row
            }
            if ui_state.prompt_history_cursor == 0 {
                return false; // on the live draft — nothing newer to recall
            }
            ui_state.prompt_history_cursor -= 1;
            let text = if ui_state.prompt_history_cursor == 0 {
                // Stepped past the newest entry → restore the saved live draft.
                ui_state.prompt_history_draft.clone()
            } else {
                ui_state.prompt_history[ui_state.prompt_history_cursor - 1].clone()
            };
            set_recalled_input(ui_state, state, text);
            true
        }
        InputCursorCommand::Left
        | InputCursorCommand::Right
        | InputCursorCommand::LineStart
        | InputCursorCommand::LineEnd => false,
    }
}

/// Replace the composer with a recalled entry (or the restored draft): set the
/// text, park the cursor at its end, and run the same dropdown/status
/// housekeeping as ordinary input edits so discovery re-activates cleanly.
fn set_recalled_input(ui_state: &mut TuiUiState, state: &mut AppState, text: String) {
    ui_state.input_cursor = input_char_count(&text);
    ui_state.input_preferred_col = None;
    ui_state.status_message = None;
    state.input = text;
    clear_command_dropdown_dismissal(ui_state);
    clear_file_mention_dropdown_dismissal(ui_state);
    reset_dropdown_selections(ui_state);
}

/// Keep the in-session recall ring current by prepending a just-submitted prompt
/// (ADR-004), so this session's prompts are recallable without a disk reload:
/// newest-first, consecutive-deduped, capped to `prompt_history_max`. A no-op
/// when recall is disabled (so a disabled session never builds a recallable
/// ring), for empty submissions, and for leading-space prompts (the secrets
/// escape hatch — matching the projection's filters).
fn record_in_session_prompt(ui_state: &mut TuiUiState, prompt: &str) {
    if !ui_state.prompt_history_enabled || prompt.trim().is_empty() || prompt.starts_with(' ') {
        return;
    }
    // Consecutive-dedup: don't stack an identical prompt on the current front.
    if ui_state.prompt_history.first().map(String::as_str) == Some(prompt) {
        return;
    }
    ui_state.prompt_history.insert(0, prompt.to_string());
    ui_state
        .prompt_history
        .truncate(ui_state.prompt_history_max);
}

fn scroll_events(ui_state: &mut TuiUiState, command: EventScrollCommand) {
    let max_scroll = event_max_scroll(ui_state);
    let page = ui_state.event_viewport_lines.saturating_sub(1).max(1);
    match command {
        EventScrollCommand::PageUp => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_sub(page);
            ui_state.event_follow = false;
        }
        EventScrollCommand::PageDown => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_add(page).min(max_scroll);
            ui_state.event_follow = ui_state.event_scroll == max_scroll;
        }
        EventScrollCommand::LinesUp(lines) => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_sub(lines);
            ui_state.event_follow = false;
        }
        EventScrollCommand::LinesDown(lines) => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_add(lines).min(max_scroll);
            ui_state.event_follow = ui_state.event_scroll == max_scroll;
        }
        EventScrollCommand::Top => {
            ui_state.event_scroll = 0;
            ui_state.event_follow = false;
        }
        EventScrollCommand::Bottom => {
            ui_state.event_scroll = max_scroll;
            ui_state.event_follow = true;
        }
    }
}

fn event_max_scroll(ui_state: &TuiUiState) -> usize {
    ui_state
        .event_content_lines
        .saturating_sub(ui_state.event_viewport_lines.max(1))
}

/// Maps our scroll offset onto ratatui's scrollbar position space.
///
/// ratatui places the thumb at the bottom of the track only when
/// `position == content_length - 1` (its model treats `position` as the index
/// of the line shown at the *top* of the viewport). Our `event_scroll` instead
/// tops out at `content_lines - viewport_lines` — the last line sitting at the
/// *bottom* of the viewport, i.e. fully scrolled. Without this remap the thumb
/// stops `viewport_lines - 1` rows short of the end even when the chat is
/// scrolled to the latest message. Remapping `0..=max_scroll` onto
/// `0..=content_lines-1` (with rounding) keeps the thumb proportional while
/// letting it reach the very bottom.
fn scrollbar_position(event_scroll: usize, max_scroll: usize, content_lines: usize) -> usize {
    let span = content_lines.saturating_sub(1);
    if max_scroll == 0 {
        return span;
    }
    let scrolled = event_scroll.min(max_scroll);
    (scrolled * span + max_scroll / 2) / max_scroll
}

fn load_skill_suggestions(working_directory: &Path) -> Vec<SkillSuggestion> {
    let roots = skills::skill_roots(working_directory);
    load_skill_suggestions_from_roots(working_directory, &roots)
}

fn load_skill_suggestions_from_roots(
    working_directory: &Path,
    roots: &[skills::SkillRoot],
) -> Vec<SkillSuggestion> {
    let fingerprint = skill_file_fingerprints(roots);
    if let Some(suggestions) = read_cached_skill_suggestions(working_directory, &fingerprint) {
        return suggestions;
    }

    refresh_skill_suggestions(working_directory, roots, &fingerprint)
}

fn reload_skill_suggestions(working_directory: &Path) -> Vec<SkillSuggestion> {
    let roots = skills::skill_roots(working_directory);
    let fingerprint = skill_file_fingerprints(&roots);
    refresh_skill_suggestions(working_directory, &roots, &fingerprint)
}

fn refresh_skill_suggestions(
    working_directory: &Path,
    roots: &[skills::SkillRoot],
    fingerprint: &[SkillFileFingerprint],
) -> Vec<SkillSuggestion> {
    let suggestions = skills::discover_skill_suggestions(roots).unwrap_or_default();
    let _ = write_skill_suggestion_cache(working_directory, fingerprint, &suggestions);
    suggestions
}

fn skill_cache_path(working_directory: &Path) -> PathBuf {
    working_directory.join(".atelier").join("skills-cache.json")
}

fn read_cached_skill_suggestions(
    working_directory: &Path,
    fingerprint: &[SkillFileFingerprint],
) -> Option<Vec<SkillSuggestion>> {
    let contents = fs::read_to_string(skill_cache_path(working_directory)).ok()?;
    let cache: SkillSuggestionCache = serde_json::from_str(&contents).ok()?;
    if cache.schema_version == SKILL_SUGGESTION_CACHE_SCHEMA_VERSION
        && cache.fingerprint == fingerprint
    {
        Some(cache.suggestions)
    } else {
        None
    }
}

fn write_skill_suggestion_cache(
    working_directory: &Path,
    fingerprint: &[SkillFileFingerprint],
    suggestions: &[SkillSuggestion],
) -> Result<()> {
    let path = skill_cache_path(working_directory);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let cache = SkillSuggestionCache {
        schema_version: SKILL_SUGGESTION_CACHE_SCHEMA_VERSION,
        fingerprint: fingerprint.to_vec(),
        suggestions: suggestions.to_vec(),
    };
    fs::write(&path, serde_json::to_vec_pretty(&cache)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn skill_file_fingerprints(roots: &[skills::SkillRoot]) -> Vec<SkillFileFingerprint> {
    let mut fingerprints = Vec::new();
    for root in roots {
        collect_skill_file_fingerprints(&root.path, 0, &mut fingerprints);
    }
    fingerprints.sort_by(|left, right| left.path.cmp(&right.path));
    fingerprints
}

fn collect_skill_file_fingerprints(
    directory: &Path,
    depth: usize,
    fingerprints: &mut Vec<SkillFileFingerprint>,
) {
    if depth > SKILL_DISCOVERY_MAX_DEPTH {
        return;
    }
    let skill_file = directory.join(SKILL_FILE_NAME);
    if let Ok(metadata) = fs::metadata(&skill_file) {
        if metadata.is_file() {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok());
            fingerprints.push(SkillFileFingerprint {
                path: skill_file.to_string_lossy().to_string(),
                byte_len: metadata.len(),
                modified_secs: modified.map_or(0, |duration| duration.as_secs()),
                modified_nanos: modified.map_or(0, |duration| duration.subsec_nanos()),
            });
        }
    }

    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_skill_file_fingerprints(&path, depth + 1, fingerprints);
        }
    }
}

fn reset_dropdown_selections(ui_state: &mut TuiUiState) {
    reset_agent_dropdown_selection(ui_state);
    reset_skill_dropdown_selection(ui_state);
    reset_command_dropdown_selection(ui_state);
    reset_file_mention_dropdown_selection(ui_state);
}

fn reset_file_mention_dropdown_selection(ui_state: &mut TuiUiState) {
    // Only the selection index. Like the command dropdown, the Escape dismissal
    // is keyed to the raw input and cleared on a content edit — never on a
    // cursor move — so Escape survives Left/Right/Up after dismissal.
    ui_state.file_mention_selection_index = 0;
}

fn reset_agent_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.agent_selection_index = 0;
}

fn reset_skill_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.skill_selection_index = 0;
}

fn reset_command_dropdown_selection(ui_state: &mut TuiUiState) {
    // Only the selection index. The Escape dismissal is keyed to the raw input
    // and is cleared on a content edit (insert/backspace) or clear_input — never
    // on a cursor move — so Escape survives Left/Right/Up after dismissal.
    ui_state.command_selection_index = 0;
}

/// Clear the Escape dismissal so a content edit re-activates command discovery.
fn clear_command_dropdown_dismissal(ui_state: &mut TuiUiState) {
    ui_state.command_dropdown_dismissed = None;
}

/// Clear the file-mention Escape dismissal so a content edit re-activates the
/// `@` picker. Mirrors `clear_command_dropdown_dismissal`: called on edits
/// (insert/backspace/clear), never on cursor moves.
fn clear_file_mention_dropdown_dismissal(ui_state: &mut TuiUiState) {
    ui_state.file_mention_dropdown_dismissed = None;
}

fn apply_agent_dropdown_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command: DropdownCommand,
) {
    let Some(dropdown) = agent_dropdown(state, ui_state) else {
        return;
    };
    let suggestion_count = dropdown.suggestions.len();
    if suggestion_count == 0 {
        return;
    }

    match command {
        DropdownCommand::Previous => {
            ui_state.agent_selection_index = if dropdown.selected == 0 {
                suggestion_count - 1
            } else {
                dropdown.selected - 1
            };
        }
        DropdownCommand::Next => {
            ui_state.agent_selection_index = (dropdown.selected + 1) % suggestion_count;
        }
        DropdownCommand::Accept => {
            let suggestion = dropdown.suggestions[dropdown.selected].clone();
            apply_agent_suggestion(state, ui_state, &dropdown.token, &suggestion);
        }
    }
}

fn apply_agent_suggestion(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    token: &PromptToken,
    suggestion: &AgentSuggestion,
) {
    let start = byte_index_for_char(&state.input, token.value_start);
    let end = byte_index_for_char(&state.input, token.value_end);
    state.input.replace_range(start..end, &suggestion.id);

    let inserted_len = input_char_count(&suggestion.id);
    let agent_end = token.value_start + inserted_len;
    let after_agent = byte_index_for_char(&state.input, agent_end);
    if !state.input[after_agent..]
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        state.input.insert(after_agent, ' ');
    }
    ui_state.input_cursor = agent_end.saturating_add(1);
    ui_state.input_preferred_col = None;
    reset_dropdown_selections(ui_state);
}

fn agent_dropdown(state: &AppState, ui_state: &TuiUiState) -> Option<AgentDropdown> {
    let token = active_prompt_token(&state.input, ui_state.input_cursor, AGENT_PREFIX)?;
    let suggestions = agent_suggestions(state, &token.query);
    if suggestions.is_empty() {
        return None;
    }
    let selected = ui_state
        .agent_selection_index
        .min(suggestions.len().saturating_sub(1));
    Some(AgentDropdown {
        token,
        suggestions,
        selected,
    })
}

fn apply_skill_dropdown_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command: DropdownCommand,
) {
    let Some(dropdown) = skill_dropdown(&state.input, ui_state) else {
        return;
    };
    let suggestion_count = dropdown.suggestions.len();
    if suggestion_count == 0 {
        return;
    }

    match command {
        DropdownCommand::Previous => {
            ui_state.skill_selection_index = if dropdown.selected == 0 {
                suggestion_count - 1
            } else {
                dropdown.selected - 1
            };
        }
        DropdownCommand::Next => {
            ui_state.skill_selection_index = (dropdown.selected + 1) % suggestion_count;
        }
        DropdownCommand::Accept => {
            let suggestion = dropdown.suggestions[dropdown.selected].clone();
            apply_skill_suggestion(state, ui_state, &dropdown.token, &suggestion);
        }
    }
}

async fn apply_clarification_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command: ClarificationCommand,
    command_sender: &mpsc::Sender<AppWorkerCommand>,
) -> Result<bool> {
    let Some(clarification) = &state.pending_clarification else {
        return Ok(true);
    };

    // Rows span the real options plus one synthetic "custom answer" row.
    let row_count = clarification.options.len() + 1;
    match command {
        ClarificationCommand::PreviousOption => {
            ui_state.clarification_option_index = if ui_state.clarification_option_index == 0 {
                row_count - 1
            } else {
                (ui_state.clarification_option_index - 1).min(row_count - 1)
            };
            Ok(true)
        }
        ClarificationCommand::NextOption => {
            ui_state.clarification_option_index =
                (ui_state.clarification_option_index + 1) % row_count;
            Ok(true)
        }
        ClarificationCommand::ToggleOption => {
            let focused = ui_state.clarification_option_index;
            if clarification.multi_select && focused < clarification.options.len() {
                toggle_selection(&mut ui_state.clarification_selected, focused);
            }
            Ok(true)
        }
        ClarificationCommand::FocusOption(index) => {
            if index < clarification.options.len() {
                ui_state.clarification_option_index = index;
            }
            Ok(true)
        }
        ClarificationCommand::Submit => {
            let Some(answer) = build_clarification_answer(clarification, ui_state) else {
                // Nothing chosen yet — keep waiting rather than submitting an
                // empty answer.
                return Ok(true);
            };

            let event = AppEvent::ClarificationAnswered(answer);

            queue_app_event(command_sender, event).await?;
            ui_state.clarification_custom_answer.clear();
            ui_state.clarification_option_index = 0;
            ui_state.clarification_selected.clear();
            // Keep `clarification_question_id` so `sync_clarification_state`
            // does not treat the still-pending question as new; the submitting
            // gate is cleared there once the worker drops the pending answer.
            ui_state.clarification_submitting = true;
            Ok(true)
        }
    }
}

/// Toggles an index in the multi-select set: removes it when present, otherwise
/// inserts it.
fn toggle_selection(selected: &mut BTreeSet<usize>, index: usize) {
    if !selected.insert(index) {
        selected.remove(&index);
    }
}

/// Builds the answer payload from the current selection, or `None` when nothing
/// has been chosen yet. Multi-select joins the chosen labels (plus any custom
/// text); single-select returns either the focused option or the custom text.
fn build_clarification_answer(
    clarification: &PendingClarificationView,
    ui_state: &TuiUiState,
) -> Option<crate::app::ClarificationAnswer> {
    let custom = ui_state.clarification_custom_answer.trim().to_string();
    let options = &clarification.options;
    let focused = ui_state.clarification_option_index;

    let (answer, selected_option_id, selected_option_label, answer_source) =
        if clarification.multi_select {
            let mut checked_labels = Vec::new();
            let mut first_id = None;
            let mut first_label = None;
            for (index, option) in options.iter().enumerate() {
                if ui_state.clarification_selected.contains(&index) {
                    if first_id.is_none() {
                        first_id = Some(option.id.clone());
                        first_label = Some(option.label.clone());
                    }
                    checked_labels.push(option.label.clone());
                }
            }
            let has_custom = !custom.is_empty();
            let checked_count = checked_labels.len();
            if checked_count == 0 && !has_custom {
                // Nothing checked and no custom text: fall back to the focused
                // option so Enter is never a silent dead-end (e.g. on the
                // pre-focused recommended row). The empty custom row submits
                // nothing.
                if focused < options.len() {
                    let option = &options[focused];
                    (
                        option.label.clone(),
                        Some(option.id.clone()),
                        Some(option.label.clone()),
                        "recommended".to_string(),
                    )
                } else {
                    return None;
                }
            } else {
                let mut parts = checked_labels;
                if has_custom {
                    parts.push(custom);
                }
                // Report the true selection shape so the transcript reads
                // naturally instead of always claiming "multiple".
                let (source, id, label) = if checked_count == 0 {
                    ("custom".to_string(), None, None)
                } else if checked_count == 1 && !has_custom {
                    ("recommended".to_string(), first_id, first_label)
                } else {
                    ("multi".to_string(), first_id, first_label)
                };
                (parts.join("; "), id, label, source)
            }
        } else if focused < options.len() {
            // A real option is focused: submit it (focus-driven). Any stale
            // custom text is ignored until the user focuses the custom row.
            let option = &options[focused];
            (
                option.label.clone(),
                Some(option.id.clone()),
                Some(option.label.clone()),
                "recommended".to_string(),
            )
        } else if !custom.is_empty() {
            // The custom row is focused with text entered.
            (custom, None, None, "custom".to_string())
        } else {
            return None;
        };

    Some(crate::app::ClarificationAnswer {
        question_id: clarification.question_id.clone(),
        answer,
        selected_option_id,
        selected_option_label,
        answer_source,
    })
}

fn apply_skill_suggestion(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    token: &PromptToken,
    suggestion: &SkillSuggestion,
) {
    let start = byte_index_for_char(&state.input, token.value_start);
    let end = byte_index_for_char(&state.input, token.value_end);
    state.input.replace_range(start..end, &suggestion.alias);

    let inserted_len = input_char_count(&suggestion.alias);
    let skill_end = token.value_start + inserted_len;
    let after_skill = byte_index_for_char(&state.input, skill_end);
    if !state.input[after_skill..]
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        state.input.insert(after_skill, ' ');
    }
    ui_state.input_cursor = skill_end.saturating_add(1);
    ui_state.input_preferred_col = None;
    reset_dropdown_selections(ui_state);
}

fn apply_command_dropdown_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command: CommandDropdownCommand,
) {
    let Some(dropdown) = command_dropdown(state, ui_state) else {
        return;
    };
    match command {
        CommandDropdownCommand::Previous => {
            let count = dropdown.suggestions.len();
            if count == 0 {
                return;
            }
            let current = dropdown.selected.unwrap_or(0);
            ui_state.command_selection_index = if current == 0 { count - 1 } else { current - 1 };
        }
        CommandDropdownCommand::Next => {
            let count = dropdown.suggestions.len();
            if count == 0 {
                return;
            }
            let current = dropdown.selected.unwrap_or(0);
            ui_state.command_selection_index = (current + 1) % count;
        }
        CommandDropdownCommand::Accept => {
            if let Some(index) = dropdown.selected {
                let spec = dropdown.suggestions[index];
                apply_command_suggestion(state, ui_state, spec);
            }
        }
        CommandDropdownCommand::Dismiss => {
            ui_state.command_dropdown_dismissed = Some(state.input.clone());
        }
        CommandDropdownCommand::TrapNoMatch => {}
    }
}

fn apply_file_mention_dropdown_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command: FileMentionDropdownCommand,
) {
    let Some(dropdown) = file_mention_dropdown(state, ui_state) else {
        return;
    };
    match command {
        FileMentionDropdownCommand::Previous => {
            let count = dropdown.suggestions.len();
            if count == 0 {
                return;
            }
            ui_state.file_mention_selection_index = if dropdown.selected == 0 {
                count - 1
            } else {
                dropdown.selected - 1
            };
        }
        FileMentionDropdownCommand::Next => {
            let count = dropdown.suggestions.len();
            if count == 0 {
                return;
            }
            ui_state.file_mention_selection_index = (dropdown.selected + 1) % count;
        }
        FileMentionDropdownCommand::Accept => {
            if dropdown.empty {
                return;
            }
            let suggestion = dropdown.suggestions[dropdown.selected].clone();
            apply_file_mention_suggestion(state, ui_state, &dropdown.token, &suggestion);
        }
        FileMentionDropdownCommand::Dismiss => {
            ui_state.file_mention_dropdown_dismissed = Some(state.input.clone());
        }
    }
}

/// Accept a file suggestion by replacing the `@token` — INCLUDING the leading
/// `@` — with the bare path, so the inserted text is a plain path (ADR-005).
/// Folders get a trailing `/`; a trailing space is added when none follows so
/// the cursor lands ready to keep typing (and a second `@` re-opens the
/// picker). Text-only: no app event is dispatched.
fn apply_file_mention_suggestion(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    token: &PromptToken,
    suggestion: &FileSuggestion,
) {
    let mut inserted = suggestion.rel_path.clone();
    if suggestion.is_dir {
        inserted.push('/');
    }

    // Extend the replaced range back over the `@` prefix so it is consumed.
    let prefix_len = input_char_count(FILE_MENTION_PREFIX);
    let replace_start = token.value_start.saturating_sub(prefix_len);
    let start = byte_index_for_char(&state.input, replace_start);
    let end = byte_index_for_char(&state.input, token.value_end);
    state.input.replace_range(start..end, &inserted);

    let inserted_len = input_char_count(&inserted);
    let path_end = replace_start + inserted_len;
    let after_path = byte_index_for_char(&state.input, path_end);
    if !state.input[after_path..]
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        state.input.insert(after_path, ' ');
    }
    ui_state.input_cursor = path_end.saturating_add(1);
    ui_state.input_preferred_col = None;
    reset_dropdown_selections(ui_state);
}

/// Accept a command suggestion by replacing the whole `/`-token input with its
/// insert text. Text-only: no app event is dispatched. The dropdown is then
/// dismissed for the inserted text so `Enter` does not re-accept the same row
/// (it can submit or take arguments), while `/agent:`/`/skill:` still hand off
/// to their specialized dropdowns, which do not consult this dismissal.
fn apply_command_suggestion(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    spec: &crate::slash_commands::SlashCommandSpec,
) {
    state.input = spec.insert_text.to_string();
    ui_state.input_cursor = input_char_count(spec.insert_text);
    ui_state.input_preferred_col = None;
    ui_state.command_selection_index = 0;
    ui_state.command_dropdown_dismissed = Some(spec.insert_text.to_string());
}

fn skill_dropdown(input: &str, ui_state: &TuiUiState) -> Option<SkillDropdown> {
    let token = active_prompt_token(input, ui_state.input_cursor, SKILL_PREFIX)?;
    let suggestions = skill_suggestions(&ui_state.skill_suggestions, &token.query);
    if suggestions.is_empty() {
        return None;
    }
    let selected = ui_state
        .skill_selection_index
        .min(suggestions.len().saturating_sub(1));
    Some(SkillDropdown {
        token,
        suggestions,
        selected,
    })
}

/// Activation + filtering model for the command discovery dropdown (ADR-004).
///
/// Active only while the entire input is a single `/`-prefixed token (no
/// whitespace yet). This keeps discovery off for paths and inline slashes
/// (`please /tmp`, char 0 is not `/`), and — crucially — releases the dropdown
/// the moment the user types a space, so argument-taking commands like
/// `/goal <text>`, `/subtask <agent> <task>`, and `/queue <message>` stay
/// normal input instead of getting trapped by the no-match state. Disabled
/// during pending approval and `WaitingForUser`, and while the user has
/// dismissed it for the current input (Escape). The `/agent:` and `/skill:`
/// specialized dropdowns take precedence via the routing/render order, so this
/// is only consulted once those are inactive.
fn command_dropdown(state: &AppState, ui_state: &TuiUiState) -> Option<CommandDropdown> {
    // Disabled while the run is waiting on the user: pending approval, a pending
    // clarification, or the WaitingForUser run state. Clarification answers and
    // approvals can legitimately start with `/` (e.g. `/tmp/project`).
    if state.pending_approval.is_some()
        || state.pending_clarification.is_some()
        || matches!(state.run_state, RunState::WaitingForUser)
    {
        return None;
    }
    let input = state.input.as_str();
    if !input.starts_with('/') || input.chars().any(char::is_whitespace) {
        return None;
    }
    if ui_state.command_dropdown_dismissed.as_deref() == Some(input) {
        return None;
    }
    let query = input.to_ascii_lowercase();
    let suggestions: Vec<&'static crate::slash_commands::SlashCommandSpec> =
        crate::slash_commands::catalog()
            .iter()
            .filter(|spec| spec.label.starts_with(query.as_str()))
            .collect();
    let (selected, empty) = if suggestions.is_empty() {
        (None, true)
    } else {
        let index = ui_state.command_selection_index.min(suggestions.len() - 1);
        (Some(index), false)
    };
    Some(CommandDropdown {
        suggestions,
        selected,
        empty,
    })
}

/// Activation + ranking model for the `@`-mention file dropdown (ADR-005).
///
/// Token-based: it activates on an `@` token at the cursor via the shared
/// `active_prompt_token` detector, exactly like `/agent:` and `/skill:`, so it
/// works mid-prompt and for a second `@` later in the line. A bare `@` lists
/// recents (most-recently-modified); a non-empty query with zero matches sets
/// `empty` so the renderer can show the "No matching files" row. Suppressed
/// while the run waits on the user (pending approval, pending clarification, or
/// `WaitingForUser`) so a literal `@` in an answer stays normal text, and while
/// the user has dismissed it for the current input (Escape). Returns `None`
/// when there is nothing to show (e.g. a bare `@` before the index has loaded),
/// so the dropdown never appears with a misleading empty body.
fn file_mention_dropdown(state: &AppState, ui_state: &TuiUiState) -> Option<FileMentionDropdown> {
    if state.pending_approval.is_some()
        || state.pending_clarification.is_some()
        || matches!(state.run_state, RunState::WaitingForUser)
    {
        return None;
    }
    let input = state.input.as_str();
    let token = active_prompt_token(input, ui_state.input_cursor, FILE_MENTION_PREFIX)?;
    if ui_state.file_mention_dropdown_dismissed.as_deref() == Some(input) {
        return None;
    }

    let suggestions = FileIndex::query(
        &ui_state.file_mention_entries,
        &token.query,
        DROPDOWN_MAX_ITEMS,
    );
    // No-match only when the user has actually typed a query; a bare `@` with no
    // candidates (empty/loading index) is not a "no match" and shows nothing.
    let empty = !token.query.is_empty() && suggestions.is_empty();
    if suggestions.is_empty() && !empty {
        return None;
    }
    let selected = ui_state
        .file_mention_selection_index
        .min(suggestions.len().saturating_sub(1));
    Some(FileMentionDropdown {
        token,
        suggestions,
        selected,
        empty,
    })
}

fn active_prompt_token(input: &str, cursor: usize, prefix: &str) -> Option<PromptToken> {
    let input_len = input_char_count(input);
    let cursor = cursor.min(input_len);
    let token_start = input
        .chars()
        .enumerate()
        .take(cursor)
        .filter_map(|(index, ch)| ch.is_whitespace().then_some(index + 1))
        .last()
        .unwrap_or(0);
    let prefix_len = input_char_count(prefix);
    let value_start = token_start.saturating_add(prefix_len);
    if cursor < value_start {
        return None;
    }
    let prefix_start = byte_index_for_char(input, token_start);
    let prefix_end = byte_index_for_char(input, value_start);
    if input.get(prefix_start..prefix_end) != Some(prefix) {
        return None;
    }

    let value_len = input[prefix_end..]
        .chars()
        .take_while(|ch| !ch.is_whitespace())
        .count();
    let value_end = value_start + value_len;
    if cursor > value_end {
        return None;
    }
    let start = byte_index_for_char(input, value_start);
    let end = byte_index_for_char(input, value_end);
    Some(PromptToken {
        value_start,
        value_end,
        query: input[start..end].to_string(),
    })
}

fn agent_suggestions(state: &AppState, query: &str) -> Vec<AgentSuggestion> {
    let query = query.to_ascii_lowercase();
    state
        .agents
        .iter()
        .filter(|agent| agent.status != "disabled")
        .filter(|agent| {
            query.is_empty()
                || agent.id.to_ascii_lowercase().contains(&query)
                || agent.name.to_ascii_lowercase().contains(&query)
        })
        .map(|agent| AgentSuggestion {
            id: agent.id.clone(),
            name: agent.name.clone(),
            detail: agent_suggestion_detail(agent),
        })
        .collect()
}

fn skill_suggestions(skills: &[SkillSuggestion], query: &str) -> Vec<SkillSuggestion> {
    let query = query.to_ascii_lowercase();
    skills
        .iter()
        .filter(|skill| query.is_empty() || skill.alias.to_ascii_lowercase().contains(&query))
        .cloned()
        .collect()
}

fn agent_suggestion_detail(agent: &crate::app::AgentView) -> String {
    let capabilities = if agent.capabilities.is_empty() {
        String::new()
    } else {
        format!(" {}", agent.capabilities.join(","))
    };
    format!("{}/{}{}", agent.runtime, agent.model, capabilities)
}

fn render(frame: &mut Frame, state: &AppState, ui_state: &mut TuiUiState) {
    let theme = ui_state.theme;
    // The session browser is a full-screen modal (task_07): when visible it takes
    // over the frame, so it renders correctly regardless of any composer /
    // clarification / approval context that may exist underneath.
    if ui_state.browser.visible {
        render_session_browser(frame, &ui_state.browser, &theme);
        return;
    }
    // Reset / default the clarification selection before sizing the composer so
    // the measured height matches what we render this frame.
    sync_clarification_state(state, ui_state);
    let queue_height = queue_panel_height(state);
    let composer_height = composer_height(state, ui_state, frame.area(), queue_height);
    let outer = if queue_height > 0 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(6),
                Constraint::Length(queue_height),
                Constraint::Length(composer_height),
            ])
            .split(frame.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(composer_height)])
            .split(frame.area())
    };
    let main_area = outer[0];
    let queue_area = if queue_height > 0 {
        Some(outer[1])
    } else {
        None
    };
    let composer_area = outer[outer.len() - 1];
    let event_area = if ui_state.roster_visible {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(28), Constraint::Percentage(72)])
            .split(main_area);

        let roster_items =
            agent_roster_items(&state.roster_rows, ui_state.work_spinner_frame, &theme);
        let roster = List::new(roster_items).block(
            Block::default()
                .title(" Agent Roster ")
                .title_style(Style::default().fg(theme.accent))
                .border_style(Style::default().fg(theme.border))
                .borders(Borders::ALL),
        );
        frame.render_widget(roster, main[0]);
        main[1]
    } else {
        main_area
    };

    render_chat(frame, event_area, state, ui_state);

    if let Some(queue_area) = queue_area {
        render_queue_panel(frame, queue_area, state, ui_state);
    }

    if let Some(clarification) = &state.pending_clarification {
        let areas = clarification_input_areas(composer_area);
        render_clarification_composer(frame, areas.input, clarification, ui_state);
        render_clarification_status(frame, areas.status, &theme, clarification.multi_select);
        if ui_state.help_visible {
            render_help_modal(frame, state, ui_state, &theme);
        } else {
            set_clarification_cursor(frame, areas.input, clarification, ui_state);
        }
        return;
    }

    if let Some(pending) = &state.pending_governance_decision {
        let areas = clarification_input_areas(composer_area);
        render_governance_decision_composer(
            frame,
            areas.input,
            &pending.view,
            &state.input,
            ui_state,
        );
        render_governance_decision_status(frame, areas.status, &theme);
        if ui_state.help_visible {
            render_help_modal(frame, state, ui_state, &theme);
        }
        return;
    }

    if let Some(pending) = &state.pending_plan_approval {
        let areas = clarification_input_areas(composer_area);
        render_plan_approval_composer(frame, areas.input, pending, &state.input, ui_state);
        render_plan_approval_status(frame, areas.status, &theme);
        if ui_state.help_visible {
            render_help_modal(frame, state, ui_state, &theme);
        }
        return;
    }

    let work_active = work_indicator_active(state);
    let input_areas = input_areas(composer_area);
    let input_layout = input_layout(input_areas.input, &state.input, ui_state.input_cursor);
    ui_state.input_width = input_layout.width;
    let input = Paragraph::new(wrapped_input_lines(
        &theme,
        &state.input,
        input_layout.width,
    ))
    .style(Style::default().fg(theme.text))
    .block(
        Block::default()
            .border_style(Style::default().fg(theme.border_focused))
            .borders(Borders::ALL),
    )
    .scroll((input_layout.scroll.min(usize::from(u16::MAX)) as u16, 0));
    frame.render_widget(input, input_areas.input);
    render_input_status(
        frame,
        input_areas.status,
        ui_state,
        work_active,
        state.input.is_empty(),
    );
    render_footer(frame, input_areas.footer, state, &theme);
    // Dropdown precedence mirrors key routing: agent, then skill, then the
    // `@` file mention, then the top-level command dropdown. Help takes over the
    // screen, so suppress all dropdown rendering while it is open. This chain
    // MUST stay in sync with `key_event_to_tui_command_with_ui`.
    if !ui_state.help_visible {
        if let Some(dropdown) = agent_dropdown(state, ui_state) {
            render_agent_dropdown(frame, input_areas.input, &dropdown, &theme, &state.agents);
        } else if let Some(dropdown) = skill_dropdown(&state.input, ui_state) {
            render_skill_dropdown(frame, input_areas.input, &dropdown, &theme);
        } else if let Some(dropdown) = file_mention_dropdown(state, ui_state) {
            render_file_mention_dropdown(frame, input_areas.input, &dropdown, &theme);
        } else if let Some(dropdown) = command_dropdown(state, ui_state) {
            render_command_dropdown(frame, input_areas.input, &dropdown, &theme);
        }
    }
    if ui_state.help_visible {
        render_help_modal(frame, state, ui_state, &theme);
    } else {
        set_input_cursor(frame, input_areas.input, input_layout);
    }
}

/// Height of the compact queue panel: borders (2) + up to `QUEUE_VISIBLE_MAX`
/// item rows + an optional "more" row + a hint row. Zero when the queue is
/// empty so the existing two-row layout is unchanged.
fn queue_panel_height(state: &AppState) -> u16 {
    let count = state.queued_follow_ups.len();
    if count == 0 {
        return 0;
    }
    let visible = count.min(QUEUE_VISIBLE_MAX);
    let more_row = usize::from(count > QUEUE_VISIBLE_MAX);
    let rows = 2 + visible + more_row + 1; // borders + items + (more) + hint
    rows.min(usize::from(u16::MAX)) as u16
}

fn queue_status_label(status: &QueuedFollowUpStatus) -> &'static str {
    match status {
        QueuedFollowUpStatus::Pending => "pending",
        QueuedFollowUpStatus::Paused => "paused",
        QueuedFollowUpStatus::Replaying => "replaying",
        QueuedFollowUpStatus::Cancelled => "cancelled",
    }
}

fn queue_status_style(theme: &Theme, status: &QueuedFollowUpStatus) -> Style {
    match status {
        QueuedFollowUpStatus::Pending => Style::default().fg(theme.accent),
        QueuedFollowUpStatus::Paused => Style::default().fg(theme.status_warn),
        QueuedFollowUpStatus::Replaying => Style::default()
            .fg(theme.status_ok)
            .add_modifier(Modifier::BOLD),
        QueuedFollowUpStatus::Cancelled => Style::default().fg(theme.text_dim),
    }
}

fn render_queue_panel(frame: &mut Frame, area: Rect, state: &AppState, ui_state: &TuiUiState) {
    let theme = ui_state.theme;
    let items = &state.queued_follow_ups;
    let selected = ui_state
        .queue_selection_index
        .min(items.len().saturating_sub(1));
    let visible = items.len().min(QUEUE_VISIBLE_MAX);

    let mut lines: Vec<Line> = Vec::new();
    for (index, item) in items.iter().take(visible).enumerate() {
        let is_selected = index == selected && queue_control_active(state, ui_state);
        let marker = if is_selected {
            QUEUE_SELECTED_MARKER
        } else {
            QUEUE_UNSELECTED_MARKER
        };
        let marker_style = if is_selected {
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dim)
        };
        let mut spans = vec![
            Span::styled(marker, marker_style),
            Span::styled(
                format!("[{}] ", queue_status_label(&item.status)),
                queue_status_style(&theme, &item.status),
            ),
            Span::styled(
                queue_prompt_summary(&item.prompt),
                Style::default().fg(theme.text),
            ),
        ];
        if let Some(reason) = item.pause_reason.as_deref() {
            spans.push(Span::styled(
                format!(" — {reason}"),
                Style::default().fg(theme.status_warn),
            ));
        }
        lines.push(Line::from(spans));
    }
    if items.len() > visible {
        lines.push(Line::from(Span::styled(
            format!("  …and {} more", items.len() - visible),
            Style::default().fg(theme.text_dim),
        )));
    }
    lines.push(Line::from(Span::styled(
        QUEUE_HINT,
        Style::default().fg(theme.text_dim),
    )));

    let panel = Paragraph::new(lines).block(
        Block::default()
            .title(format!(" Queue ({}) ", items.len()))
            .title_style(Style::default().fg(theme.accent))
            .border_style(Style::default().fg(theme.border))
            .borders(Borders::ALL),
    );
    frame.render_widget(panel, area);
}

fn render_chat(frame: &mut Frame, event_area: Rect, state: &AppState, ui_state: &mut TuiUiState) {
    let theme = ui_state.theme;
    let hide_banner = ui_state.hide_banner;
    let working_directory = ui_state.working_directory.clone();
    let block = Block::default()
        .title(" Chat ")
        .title_style(Style::default().fg(theme.accent))
        .border_style(Style::default().fg(theme.border))
        .borders(Borders::ALL);
    let inner_area = block.inner(event_area);
    let paragraph_width = inner_area.width.saturating_sub(1).max(1);
    // Facts shown in the welcome item, read from live state. `git` is `None`
    // until task_05 supplies `AppState.git_context`.
    let welcome_facts = WelcomeFacts {
        version: env!("CARGO_PKG_VERSION"),
        working_directory: working_directory.as_deref(),
        agents: &state.agents,
        preset: state.config_status.preset.as_deref(),
        warnings: state.config_status.warnings.len(),
        git: state
            .git_context
            .as_ref()
            .map(|git| (git.repo_name.as_str(), git.branch.as_str())),
        recoverable_session: state.recoverable_session,
    };
    let event_lines = if !state.chat_items.is_empty() {
        chat_item_lines(
            &theme,
            &state.chat_items,
            &state.agents,
            paragraph_width,
            hide_banner,
            &welcome_facts,
        )
    } else if let Some(pending) = &state.pending_approval {
        approval_modal_lines(pending, &theme, state.show_first_approval_explainer)
    } else if state.events.is_empty() {
        vec![Line::from("No chat yet.")]
    } else {
        state
            .events
            .iter()
            .map(|event| legacy_chat_line(&theme, event))
            .collect::<Vec<_>>()
    };
    let viewport_lines = usize::from(inner_area.height.max(1));
    let content_lines = wrapped_event_line_count(&event_lines, paragraph_width);
    ui_state.event_content_lines = content_lines;
    ui_state.event_viewport_lines = viewport_lines;
    ui_state.event_area = event_area;
    let max_scroll = event_max_scroll(ui_state);
    if ui_state.event_follow {
        ui_state.event_scroll = max_scroll;
    } else {
        ui_state.event_scroll = ui_state.event_scroll.min(max_scroll);
    }

    let events = Paragraph::new(event_lines)
        .style(Style::default().fg(theme.text))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((ui_state.event_scroll.min(usize::from(u16::MAX)) as u16, 0));
    frame.render_widget(events, event_area);
    if content_lines > viewport_lines {
        let mut scrollbar_state = ScrollbarState::new(content_lines)
            .viewport_content_length(viewport_lines)
            .position(scrollbar_position(
                ui_state.event_scroll,
                max_scroll,
                content_lines,
            ));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            event_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

// ── Surface → token mapping (one color = one meaning, PRD F2) ──────────────
// Border tokens carry exactly one role each:
//   • `border`          — structural panels: roster, queue, chat.
//   • `border_focused`  — the focused input composer (and only that).
//   • `accent`          — transient/attention overlays: agent & skill
//                         dropdowns, help modal, clarification composer.
// Titles on every panel/overlay use `accent`. The help modal additionally
// paints an `ink` backdrop (the only surface with a fill). Selected items in
// both dropdowns and clarification options share one treatment via
// `selection_style` (ink on accent). Per-agent identity colors (task_07) ride
// the roster/chat/dropdown id+name text, never the chrome.

/// Shared selection highlight: ink foreground on the brand accent. Used for the
/// selected dropdown marker and the selected clarification option so the
/// "this is selected" cue is identical across surfaces.
fn selection_style(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.ink)
        .bg(theme.accent)
        .add_modifier(Modifier::BOLD)
}

fn render_agent_dropdown(
    frame: &mut Frame,
    input_area: Rect,
    dropdown: &AgentDropdown,
    theme: &Theme,
    agents: &[AgentView],
) {
    if input_area.y == 0 || input_area.width < 8 || dropdown.suggestions.is_empty() {
        return;
    }

    let available_height = input_area.y.saturating_sub(frame.area().y);
    if available_height < 3 {
        return;
    }
    let visible_count = dropdown
        .suggestions
        .len()
        .min(DROPDOWN_MAX_ITEMS)
        .min(usize::from(available_height.saturating_sub(2)));
    if visible_count == 0 {
        return;
    }
    let height = (visible_count as u16).saturating_add(2);
    let selected = dropdown
        .selected
        .min(dropdown.suggestions.len().saturating_sub(1));
    let first_visible = selected.saturating_sub(visible_count.saturating_sub(1));
    let items = dropdown
        .suggestions
        .iter()
        .enumerate()
        .skip(first_visible)
        .take(visible_count)
        .map(|(index, suggestion)| {
            // Single-source accent rule (ADR-005, task_07; see `item_agent_accent`):
            // resolve the accent by the agent's canonical identity index — its
            // position in the canonical `agents` slice, found by `id` — so the
            // dropdown matches the roster/chat color regardless of dropdown rank.
            let accent = agents
                .iter()
                .position(|agent| agent.id == suggestion.id)
                .map(|canonical_index| theme.accent_for(canonical_index))
                .unwrap_or(theme.accent);
            agent_dropdown_item(theme, suggestion, index == selected, accent)
        })
        .collect::<Vec<_>>();
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width: input_area.width,
        height,
    };
    let list = List::new(items).block(
        Block::default()
            .title(" Agents ")
            .title(Line::from(" Up/Down Enter ").right_aligned())
            .title_style(Style::default().fg(theme.accent))
            .border_style(Style::default().fg(theme.accent))
            .borders(Borders::ALL),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(list, area);
}

fn agent_dropdown_item(
    theme: &Theme,
    suggestion: &AgentSuggestion,
    selected: bool,
    accent: Color,
) -> ListItem<'static> {
    // Selection cue is the shared brand-accent highlight; the agent's own
    // accent stays on the id/name text below.
    let marker_style = if selected {
        selection_style(theme)
    } else {
        Style::default().fg(theme.text_dim)
    };
    // Id and name both wear the agent's accent so the dropdown teaches the
    // color mapping used in the roster and chat headers.
    let id_style = if selected {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(accent)
    };
    let line = Line::from(vec![
        Span::styled(if selected { "> " } else { "  " }, marker_style),
        Span::styled(suggestion.id.clone(), id_style),
        Span::raw("  "),
        Span::styled(suggestion.name.clone(), Style::default().fg(accent)),
        Span::raw("  "),
        Span::styled(
            suggestion.detail.clone(),
            Style::default().fg(theme.text_muted),
        ),
    ]);
    let item = ListItem::new(line);
    if selected {
        item.style(Style::default().bg(theme.border))
    } else {
        item
    }
}

fn render_skill_dropdown(
    frame: &mut Frame,
    input_area: Rect,
    dropdown: &SkillDropdown,
    theme: &Theme,
) {
    if input_area.y == 0 || input_area.width < 8 || dropdown.suggestions.is_empty() {
        return;
    }

    let available_height = input_area.y.saturating_sub(frame.area().y);
    if available_height < 3 {
        return;
    }
    let visible_count = dropdown
        .suggestions
        .len()
        .min(DROPDOWN_MAX_ITEMS)
        .min(usize::from(available_height.saturating_sub(2)));
    if visible_count == 0 {
        return;
    }
    let height = (visible_count as u16).saturating_add(2);
    let selected = dropdown
        .selected
        .min(dropdown.suggestions.len().saturating_sub(1));
    let row_width = input_area.width.saturating_sub(2);
    let first_visible = selected.saturating_sub(visible_count.saturating_sub(1));
    let items = dropdown
        .suggestions
        .iter()
        .enumerate()
        .skip(first_visible)
        .take(visible_count)
        .map(|(index, suggestion)| {
            skill_dropdown_item(theme, suggestion, index == selected, row_width)
        })
        .collect::<Vec<_>>();
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width: input_area.width,
        height,
    };
    let list = List::new(items).block(
        Block::default()
            .title(" Skills ")
            .title(Line::from(" Up/Down Enter ").right_aligned())
            .title_style(Style::default().fg(theme.accent))
            .border_style(Style::default().fg(theme.accent))
            .borders(Borders::ALL),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(list, area);
}

fn skill_dropdown_item(
    theme: &Theme,
    suggestion: &SkillSuggestion,
    selected: bool,
    row_width: u16,
) -> ListItem<'static> {
    let marker_style = if selected {
        selection_style(theme)
    } else {
        Style::default().fg(theme.text_dim)
    };
    let id_style = if selected {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    let tag_style = Style::default()
        .fg(theme.ink)
        .bg(match suggestion.source_tag {
            SkillSourceTag::Project => theme.status_ok,
            SkillSourceTag::Personal => theme.accent,
        })
        .add_modifier(Modifier::BOLD);
    let tag = format!(" {} ", suggestion.source_tag.label());
    let tag_width = input_char_count(&tag);
    let marker = if selected { "> " } else { "  " };
    let marker_width = input_char_count(marker);
    let row_width = usize::from(row_width);
    let content_width = row_width.saturating_sub(marker_width + tag_width + 1);
    let id = truncate_to_char_width(&suggestion.alias, content_width);
    let id_width = input_char_count(&id);
    let remaining_width = content_width.saturating_sub(id_width);
    let origin_width = remaining_width.saturating_sub(2);
    let origin = if origin_width > 0 && id_width < content_width {
        truncate_to_char_width(&suggestion.source_origin, origin_width)
    } else {
        String::new()
    };
    let separator = if origin.is_empty() { "" } else { "  " };
    let left_width = marker_width
        .saturating_add(id_width)
        .saturating_add(input_char_count(separator))
        .saturating_add(input_char_count(&origin));
    let spacer_width = row_width.saturating_sub(left_width.saturating_add(tag_width));
    let line = Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(id, id_style),
        Span::raw(separator.to_string()),
        Span::styled(origin, Style::default().fg(theme.text_muted)),
        Span::raw(" ".repeat(spacer_width)),
        Span::styled(tag, tag_style),
    ]);
    let item = ListItem::new(line);
    if selected {
        item.style(Style::default().bg(theme.border))
    } else {
        item
    }
}

fn render_command_dropdown(
    frame: &mut Frame,
    input_area: Rect,
    dropdown: &CommandDropdown,
    theme: &Theme,
) {
    if input_area.y == 0 || input_area.width < 8 {
        return;
    }
    let available_height = input_area.y.saturating_sub(frame.area().y);
    if available_height < 3 {
        return;
    }

    // No-match: a single compact "No commands found" row, still framed as the
    // command dropdown so the user knows they are in discovery, not submitting.
    if dropdown.empty {
        let height = 3u16;
        let area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(height),
            width: input_area.width,
            height,
        };
        let row = ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("No commands found", Style::default().fg(theme.text_muted)),
        ]));
        frame.render_widget(Clear, area);
        frame.render_widget(
            List::new(vec![row]).block(command_dropdown_block(theme)),
            area,
        );
        return;
    }

    let visible_count = dropdown
        .suggestions
        .len()
        .min(DROPDOWN_MAX_ITEMS)
        .min(usize::from(available_height.saturating_sub(2)));
    if visible_count == 0 {
        return;
    }
    let height = (visible_count as u16).saturating_add(2);
    let selected = dropdown
        .selected
        .unwrap_or(0)
        .min(dropdown.suggestions.len().saturating_sub(1));
    let row_width = input_area.width.saturating_sub(2);
    // Align descriptions into a column behind the widest visible label.
    let label_col = dropdown
        .suggestions
        .iter()
        .map(|spec| input_char_count(spec.label))
        .max()
        .unwrap_or(0);
    let first_visible = selected.saturating_sub(visible_count.saturating_sub(1));
    let items = dropdown
        .suggestions
        .iter()
        .enumerate()
        .skip(first_visible)
        .take(visible_count)
        .map(|(index, spec)| {
            command_dropdown_item(theme, spec, index == selected, row_width, label_col)
        })
        .collect::<Vec<_>>();
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width: input_area.width,
        height,
    };
    frame.render_widget(Clear, area);
    frame.render_widget(List::new(items).block(command_dropdown_block(theme)), area);
}

fn command_dropdown_block(theme: &Theme) -> Block<'static> {
    Block::default()
        .title(" Commands ")
        .title(Line::from(" Up/Down Tab/Enter ").right_aligned())
        .title_style(Style::default().fg(theme.accent))
        .border_style(Style::default().fg(theme.accent))
        .borders(Borders::ALL)
}

fn command_dropdown_item(
    theme: &Theme,
    spec: &crate::slash_commands::SlashCommandSpec,
    selected: bool,
    row_width: u16,
    label_col: usize,
) -> ListItem<'static> {
    let marker_style = if selected {
        selection_style(theme)
    } else {
        Style::default().fg(theme.text_dim)
    };
    let label_style = if selected {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    let marker = if selected { "> " } else { "  " };
    let marker_width = input_char_count(marker);
    let row_width = usize::from(row_width);
    let gap = 2;
    let desc_width = row_width.saturating_sub(marker_width + label_col + gap);
    let description = truncate_to_char_width(spec.description, desc_width);
    let label = format!("{:label_col$}", spec.label);
    let line = Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(label, label_style),
        Span::raw("  "),
        Span::styled(description, Style::default().fg(theme.text_muted)),
    ]);
    let item = ListItem::new(line);
    if selected {
        item.style(Style::default().bg(theme.border))
    } else {
        item
    }
}

fn render_file_mention_dropdown(
    frame: &mut Frame,
    input_area: Rect,
    dropdown: &FileMentionDropdown,
    theme: &Theme,
) {
    if input_area.y == 0 || input_area.width < 8 {
        return;
    }
    let available_height = input_area.y.saturating_sub(frame.area().y);
    if available_height < 3 {
        return;
    }

    // No-match: a single compact "No matching files" row, still framed as the
    // file dropdown so the user knows they are in discovery (it does not trap
    // Enter — see task_07).
    if dropdown.empty {
        let height = 3u16;
        let area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(height),
            width: input_area.width,
            height,
        };
        let row = ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("No matching files", Style::default().fg(theme.text_muted)),
        ]));
        frame.render_widget(Clear, area);
        frame.render_widget(
            List::new(vec![row]).block(file_mention_dropdown_block(theme, " Esc ")),
            area,
        );
        return;
    }

    if dropdown.suggestions.is_empty() {
        return;
    }
    let visible_count = dropdown
        .suggestions
        .len()
        .min(DROPDOWN_MAX_ITEMS)
        .min(usize::from(available_height.saturating_sub(2)));
    if visible_count == 0 {
        return;
    }
    let height = (visible_count as u16).saturating_add(2);
    let selected = dropdown
        .selected
        .min(dropdown.suggestions.len().saturating_sub(1));
    let row_width = input_area.width.saturating_sub(2);
    let first_visible = selected.saturating_sub(visible_count.saturating_sub(1));
    let items = dropdown
        .suggestions
        .iter()
        .enumerate()
        .skip(first_visible)
        .take(visible_count)
        .map(|(index, suggestion)| {
            file_mention_dropdown_item(theme, suggestion, index == selected, row_width)
        })
        .collect::<Vec<_>>();
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width: input_area.width,
        height,
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        List::new(items).block(file_mention_dropdown_block(theme, " Up/Down Tab/Enter ")),
        area,
    );
}

/// `hint` is the right-aligned key affordance. The populated list advertises
/// navigation/acceptance; the no-match row only honors `Esc`, so it passes a
/// narrower hint rather than implying `Up/Down`/`Tab`/`Enter` do something.
fn file_mention_dropdown_block(theme: &Theme, hint: &str) -> Block<'static> {
    Block::default()
        .title(" Files ")
        .title(Line::from(hint.to_string()).right_aligned())
        .title_style(Style::default().fg(theme.accent))
        .border_style(Style::default().fg(theme.accent))
        .borders(Borders::ALL)
}

fn file_mention_dropdown_item(
    theme: &Theme,
    suggestion: &FileSuggestion,
    selected: bool,
    row_width: u16,
) -> ListItem<'static> {
    let marker_style = if selected {
        selection_style(theme)
    } else {
        Style::default().fg(theme.text_dim)
    };
    let marker = if selected { "> " } else { "  " };
    let marker_width = input_char_count(marker);
    let content_width = usize::from(row_width).saturating_sub(marker_width);

    // Folder affordance: a trailing `/` distinguishes folders from files.
    let mut display = suggestion.rel_path.clone();
    if suggestion.is_dir {
        display.push('/');
    }
    let display = truncate_to_char_width(&display, content_width);
    let matched: std::collections::HashSet<usize> = suggestion
        .match_indices
        .iter()
        .map(|&index| index as usize)
        .collect();

    let mut spans = vec![Span::styled(marker.to_string(), marker_style)];
    spans.extend(highlighted_path_spans(theme, &display, &matched, selected));
    let item = ListItem::new(Line::from(spans));
    if selected {
        item.style(Style::default().bg(theme.border))
    } else {
        item
    }
}

/// Build one styled span per character of `display`, emphasizing the matched
/// offsets (bold accent) so the fuzzy match is confirmable at a glance
/// (ADR-002 — highlighting is a V1 trust mechanism, not optional polish).
fn highlighted_path_spans(
    theme: &Theme,
    display: &str,
    matched: &std::collections::HashSet<usize>,
    selected: bool,
) -> Vec<Span<'static>> {
    let matched_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let base_style = if selected {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.text)
    };
    display
        .chars()
        .enumerate()
        .map(|(index, ch)| {
            let style = if matched.contains(&index) {
                matched_style
            } else {
                base_style
            };
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

fn queue_prompt_summary(prompt: &str) -> String {
    prompt.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_to_char_width(value: &str, max_width: usize) -> String {
    value.chars().take(max_width).collect()
}

fn chat_item_lines(
    theme: &Theme,
    items: &[ChatItemView],
    agents: &[AgentView],
    width: u16,
    hide_banner: bool,
    welcome_facts: &WelcomeFacts,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for item in items {
        if item.kind == ChatItemKind::Welcome {
            lines.extend(welcome::welcome_lines(
                theme,
                width,
                hide_banner,
                welcome_facts,
            ));
            lines.push(Line::from(""));
            continue;
        }
        if item.kind == ChatItemKind::UserPrompt {
            lines.extend(user_prompt_lines(theme, item));
            lines.push(Line::from(""));
            continue;
        }
        let agent_accent = item_agent_accent(theme, agents, item);
        lines.push(chat_item_header_line(theme, item, agent_accent));
        if let Some(summary) = item
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
            // Run/workflow summaries set both `summary` and the first `body`
            // line from the same plain-text string; rendering both would print
            // the line twice. Skip the muted summary line when the first body
            // line already carries it (structured/JSON summaries differ from
            // their body and still render both).
            .filter(|summary| item.body.first().map(|line| line.text.as_str()) != Some(*summary))
        {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(summary.to_string(), Style::default().fg(theme.text_muted)),
            ]));
        }
        for body in &item.body {
            lines.push(chat_body_line(theme, body));
        }
        if !item.details.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  details: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    item.details
                        .iter()
                        .map(detail_label)
                        .collect::<Vec<_>>()
                        .join(", "),
                    Style::default().fg(theme.text_dim),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }
    if lines.last().is_some_and(|line| line.width() == 0) {
        lines.pop();
    }
    lines
}

fn user_prompt_lines(theme: &Theme, item: &ChatItemView) -> Vec<Line<'static>> {
    let mut texts = item
        .body
        .iter()
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();
    if texts.is_empty() {
        if let Some(summary) = item.summary.clone() {
            texts.push(summary);
        }
    }
    if texts.is_empty() {
        texts.push(item.title.clone());
    }

    let label = if item.title.to_ascii_lowercase().contains("clarification") {
        " You / clarification "
    } else {
        " You "
    };
    let continuation_prefix = " ".repeat(label.chars().count());
    texts
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            if index == 0 {
                Line::from(vec![
                    Span::styled(
                        label,
                        Style::default()
                            .fg(theme.ink)
                            .bg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ", Style::default().bg(theme.user_prompt_bg)),
                    Span::styled(
                        text,
                        Style::default().fg(theme.text).bg(theme.user_prompt_bg),
                    ),
                    Span::styled(" ", Style::default().bg(theme.user_prompt_bg)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        continuation_prefix.clone(),
                        Style::default().bg(theme.user_prompt_bg),
                    ),
                    Span::styled(
                        text,
                        Style::default().fg(theme.text).bg(theme.user_prompt_bg),
                    ),
                    Span::styled(" ", Style::default().bg(theme.user_prompt_bg)),
                ])
            }
        })
        .collect()
}

/// True when `agent`'s id or display name is the leading token of `title`
/// (AgentProgress: "{agent} …"; AgentResult: "{agent}: …"). The space/colon
/// delimiter prevents partial matches (e.g. "fix" vs "fixer").
fn title_names_agent(title: &str, agent: &AgentView) -> bool {
    [agent.id.as_str(), agent.name.as_str()]
        .into_iter()
        .filter(|name| !name.is_empty())
        .any(|name| {
            title
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with(':'))
        })
}

/// Roster index of the agent that owns `title`, or `None` if no agent matches.
fn agent_index_for_title(agents: &[AgentView], title: &str) -> Option<usize> {
    agents
        .iter()
        .position(|agent| title_names_agent(title, agent))
}

/// Accent for an agent-attributed item's title: the owning agent's round-robin
/// color. `None` for non-attributed kinds or unmatched agents, so the caller
/// falls back to severity styling.
///
/// Single-source accent rule (ADR-005, task_07): an agent's color is anchored to
/// its **canonical identity index**, never its render-time position. All three
/// surfaces resolve the same index for a given agent:
///
/// 1. roster — `RosterRow.accent_index` (canonical, fixed before the
///    `NeedsInput` pin reorders rows), read in `roster_row_item`;
/// 2. chat — `agent_index_for_title` looks the agent up in the canonical
///    `agents` slice (this function);
/// 3. `/agent:` dropdown — `agents.position(|a| a.id == suggestion.id)`.
///
/// So the pin can never recolor an agent or break its link to the transcript.
/// RISK: a future fourth surface that derives accent from a display position
/// would silently break this contract — route any new accent through one of the
/// canonical-index lookups above.
fn item_agent_accent(theme: &Theme, agents: &[AgentView], item: &ChatItemView) -> Option<Color> {
    if !matches!(
        item.kind,
        ChatItemKind::AgentProgress | ChatItemKind::AgentResult
    ) {
        return None;
    }
    agent_index_for_title(agents, &item.title).map(|index| theme.accent_for(index))
}

fn chat_item_header_line(
    theme: &Theme,
    item: &ChatItemView,
    agent_accent: Option<Color>,
) -> Line<'static> {
    // Agent-attributed kinds take the owning agent's accent; everything else
    // (including the agent-spanning run summary) stays severity-driven.
    let title_style = match agent_accent {
        Some(accent) => Style::default().fg(accent),
        None => severity_title_style(theme, &item.severity),
    }
    .add_modifier(Modifier::BOLD);
    // The run summary is the styled run conclusion — emphasize its kind label.
    let kind_style = if item.kind == ChatItemKind::RunSummary {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_dim)
    };
    Line::from(vec![
        Span::styled(
            format!(" {} ", item.status.label()),
            severity_badge_style(theme, &item.severity),
        ),
        Span::raw(" "),
        Span::styled(item.title.clone(), title_style),
        Span::styled(format!("  {}", chat_kind_label(&item.kind)), kind_style),
    ])
}

fn chat_body_line(theme: &Theme, line: &ChatLineView) -> Line<'static> {
    let value_style = match line.style {
        ChatLineStyle::Plain => Style::default().fg(theme.text),
        ChatLineStyle::Muted => Style::default().fg(theme.text_muted),
        ChatLineStyle::Code => Style::default().fg(theme.accent),
        ChatLineStyle::DiffAdd => Style::default().fg(theme.status_ok),
        ChatLineStyle::DiffRemove => Style::default().fg(theme.status_error),
        ChatLineStyle::DiffContext => Style::default().fg(theme.text_dim),
        ChatLineStyle::Warning => Style::default().fg(theme.status_warn),
        ChatLineStyle::Error => Style::default().fg(theme.status_error),
    };
    // Prose-style lines carry a "label: value" structure (finding/verified/plan/
    // …) baked into the text. Render the recognized label distinctly so agent
    // output reads as a scannable list. Text is byte-identical; only styling
    // changes. Code/diff lines are left verbatim.
    if matches!(line.style, ChatLineStyle::Plain | ChatLineStyle::Muted) {
        if let Some(label_style) = body_label_style(theme, &line.text) {
            if let Some((label, value)) = line.text.split_once(": ") {
                return Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(format!("{label}: "), label_style),
                    Span::styled(value.to_string(), value_style),
                ]);
            }
        }
    }
    Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(line.text.clone(), value_style),
    ])
}

/// Style for a recognized leading semantic label in a body line ("finding: …",
/// "verified: …", etc.), or `None` when the line is plain prose. Only the fixed
/// set of labels the projection emits is recognized, so arbitrary "Word:" prose
/// is never mis-styled.
fn body_label_style(theme: &Theme, text: &str) -> Option<Style> {
    let label = text.split_once(": ")?.0;
    let color = match label {
        "verified" | "verification" | "changed" => theme.status_ok,
        "risk" => theme.status_warn,
        "blocker" => theme.status_error,
        "finding" | "plan" => theme.accent,
        "command" | "summary" => theme.text_dim,
        _ => return None,
    };
    Some(Style::default().fg(color).add_modifier(Modifier::BOLD))
}

fn detail_label(detail: &ChatDetailRef) -> String {
    match detail {
        ChatDetailRef::HistoryEvent { label, .. }
        | ChatDetailRef::Artifact { label, .. }
        | ChatDetailRef::Inline { label, .. } => label.clone(),
    }
}

fn chat_kind_label(kind: &ChatItemKind) -> &'static str {
    match kind {
        ChatItemKind::UserPrompt => "prompt",
        ChatItemKind::RoutingDecision => "route",
        ChatItemKind::AgentProgress => "progress",
        ChatItemKind::ActionRequested => "action",
        ChatItemKind::CommandResult => "command",
        ChatItemKind::FileEdit => "file edit",
        ChatItemKind::Approval => "approval",
        ChatItemKind::Clarification => "clarification",
        ChatItemKind::GovernanceDecision => "governance",
        ChatItemKind::Diagnostic => "diagnostic",
        ChatItemKind::SkillContext => "skills",
        ChatItemKind::AgentResult => "agent",
        ChatItemKind::RunSummary => "run",
        ChatItemKind::HookInvocation => "hook",
        ChatItemKind::Welcome => "welcome",
        ChatItemKind::Plan => "plan",
    }
}

fn severity_badge_style(theme: &Theme, severity: &ChatSeverity) -> Style {
    match severity {
        ChatSeverity::Info => Style::default().fg(theme.ink).bg(theme.accent),
        ChatSeverity::Success => Style::default().fg(theme.ink).bg(theme.status_ok),
        ChatSeverity::Warning => Style::default().fg(theme.ink).bg(theme.status_warn),
        ChatSeverity::Error => Style::default().fg(theme.text).bg(theme.status_error),
    }
}

fn severity_title_style(theme: &Theme, severity: &ChatSeverity) -> Style {
    match severity {
        ChatSeverity::Info => Style::default().fg(theme.text),
        ChatSeverity::Success => Style::default().fg(theme.status_ok),
        ChatSeverity::Warning => Style::default().fg(theme.status_warn),
        ChatSeverity::Error => Style::default().fg(theme.status_error),
    }
}

fn wrapped_event_line_count(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

/// Tabbed help overlay. Pure function of `(AppState, TuiUiState, Theme)` — reads
/// no `App` internals. Draws a theme-token tab strip (active tab highlighted) and
/// dispatches on `ui_state.help_active_tab` to the matching per-tab builder. The
/// default tab (`GettingStarted`) renders on open. Tab navigation is handled in the
/// key-routing layer (task 07); this render just reflects `help_active_tab`.
/// Render the session-browser modal (task_07): a centered, newest-first list of
/// `label · timestamp · outcome` rows with the selection highlighted and a filter
/// line. All color flows through theme tokens; transcript-derived text is
/// sanitized (ADR-004) before display.
fn render_session_browser(frame: &mut Frame, browser: &SessionBrowserState, theme: &Theme) {
    let area = centered_rect(78, 80, frame.area());
    if browser.mode == BrowserMode::Preview {
        render_session_preview(frame, area, browser, theme);
        return;
    }
    let filtered = browser.filtered_indices();

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(if browser.filter.is_empty() {
        Span::styled(
            "type to filter".to_string(),
            Style::default().fg(theme.text_dim),
        )
    } else {
        Span::styled(
            format!(
                "filter: {}",
                crate::app::chat::sanitize_transcript_text(&browser.filter)
            ),
            Style::default().fg(theme.text_muted),
        )
    }));
    lines.push(Line::from(""));

    if filtered.is_empty() {
        let message = if browser.summaries.is_empty() {
            "No sessions yet."
        } else {
            "No sessions match the filter."
        };
        lines.push(Line::from(Span::styled(
            message.to_string(),
            Style::default().fg(theme.text_dim),
        )));
    }

    for (row, &index) in filtered.iter().enumerate() {
        let summary = &browser.summaries[index];
        let selected = row == browser.selection_index;
        let label_style = if selected {
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        };
        lines.push(Line::from(vec![
            Span::styled(if selected { "▶ " } else { "  " }.to_string(), label_style),
            Span::styled(
                crate::app::chat::sanitize_transcript_text(&summary.label),
                label_style,
            ),
            Span::raw("  "),
            Span::styled(
                crate::app::chat::sanitize_transcript_text(&summary.started_at),
                Style::default().fg(theme.text_dim),
            ),
            Span::raw("  "),
            Span::styled(
                run_state_label(&summary.outcome).to_string(),
                run_state_style(theme, &summary.outcome),
            ),
        ]));
    }

    let widget = Paragraph::new(lines)
        .style(Style::default().fg(theme.text).bg(theme.ink))
        .block(
            Block::default()
                .title(" Sessions ")
                .title(
                    Line::from(" ↑↓ select · Enter resume · → preview · Esc close ")
                        .right_aligned(),
                )
                .title_style(
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
                .border_style(Style::default().fg(theme.accent))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

/// Render the read-only transcript preview pane (task_08): a loading placeholder
/// until the off-thread fold lands, then the sanitized, scrollable transcript.
fn render_session_preview(
    frame: &mut Frame,
    area: Rect,
    browser: &SessionBrowserState,
    theme: &Theme,
) {
    let (lines, scroll) = match &browser.preview {
        None => (
            vec![Line::from(Span::styled(
                "Loading transcript…".to_string(),
                Style::default().fg(theme.text_dim),
            ))],
            0u16,
        ),
        Some(preview) if preview.items.is_empty() => (
            vec![Line::from(Span::styled(
                "This session has no transcript.".to_string(),
                Style::default().fg(theme.text_dim),
            ))],
            0,
        ),
        Some(preview) => (
            build_preview_lines(preview, theme),
            browser.preview_scroll.min(u16::MAX as usize) as u16,
        ),
    };
    let widget = Paragraph::new(lines)
        .style(Style::default().fg(theme.text).bg(theme.ink))
        .scroll((scroll, 0))
        .block(
            Block::default()
                .title(" Session preview ")
                .title(Line::from(" Enter resume · Esc back · PgUp/PgDn scroll ").right_aligned())
                .title_style(
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
                .border_style(Style::default().fg(theme.accent))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

/// Build the styled transcript lines for a preview (already sanitized by the
/// task_06 builder): a title + optional summary + body lines + a separator per
/// item. The line count matches `preview_total_lines` so scroll clamping is exact.
fn build_preview_lines(preview: &SessionPreview, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(preview_total_lines(preview));
    for item in &preview.items {
        lines.push(Line::from(Span::styled(
            item.title.clone(),
            severity_title_style(theme, &item.severity).add_modifier(Modifier::BOLD),
        )));
        if let Some(summary) = &item.summary {
            lines.push(Line::from(Span::styled(
                summary.clone(),
                Style::default().fg(theme.text_muted),
            )));
        }
        for line in &item.body {
            lines.push(chat_body_line(theme, line));
        }
        lines.push(Line::from(""));
    }
    lines
}

fn render_help_modal(frame: &mut Frame, state: &AppState, ui_state: &TuiUiState, theme: &Theme) {
    let area = centered_rect(78, 100, frame.area());
    let active = ui_state.help_active_tab;

    // Tab strip: every `HelpTab::ALL` title, the active one highlighted via theme
    // tokens only (no inline color literals — ADR-003 rejects `ratatui::Tabs`).
    let mut strip: Vec<Span<'static>> = Vec::new();
    for (index, tab) in HelpTab::ALL.iter().enumerate() {
        if index > 0 {
            strip.push(Span::styled("  ", Style::default().fg(theme.text_dim)));
        }
        let style = if *tab == active {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.text_muted)
        };
        strip.push(Span::styled(tab.title(), style));
    }
    let mut lines = vec![Line::from(strip), Line::from("")];

    // Render the active tab body by dispatching to its per-tab builder.
    let body = match active {
        HelpTab::GettingStarted => getting_started_lines(state, theme),
        HelpTab::Commands => commands_tab_lines(&ui_state.help_filter, theme),
        HelpTab::Keys => keys_tab_lines(&ui_state.keymap, theme),
        HelpTab::Skills => skills_tab_lines(ui_state, theme),
        HelpTab::Approvals => approvals_tab_lines(theme),
        HelpTab::Cli => cli_tab_lines(theme),
    };
    lines.extend(body);

    let help = Paragraph::new(lines)
        .style(Style::default().fg(theme.text).bg(theme.ink))
        .block(
            Block::default()
                .title(" Help ")
                .title(Line::from(" Esc ").right_aligned())
                .title_style(
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
                .border_style(Style::default().fg(theme.accent))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(help, area);
}

/// Human-readable label for a remappable action, shown beside its key in the
/// Keys tab. (Distinct from `keybindings::action_name`, which is the kebab-case
/// config identifier.)
fn keys_action_label(action: KeyAction) -> &'static str {
    match action {
        KeyAction::ToggleRoster => "show or hide the Agent Roster",
        KeyAction::ScrollPageUp => "scroll Chat up one page",
        KeyAction::ScrollPageDown => "scroll Chat down one page",
        KeyAction::ScrollTop => "jump Chat to the top",
        KeyAction::ScrollBottom => "jump Chat to the latest",
        KeyAction::InputLineStart => "move cursor to line start",
        KeyAction::InputLineEnd => "move cursor to line end",
        KeyAction::InputKillToEnd => "delete to end of line",
        KeyAction::InputKillToStart => "delete to start of line",
        KeyAction::InputKillWordBack => "delete the previous word",
    }
}

/// Structurally fixed keys: reserved (`Ctrl-C`), composer-structural, and
/// approval-context keys. None are rebindable, so they are rendered locked,
/// separate from the data-driven remappable section.
const FIXED_KEY_ROWS: &[(&str, &str)] = &[
    ("ctrl+c", "interrupt active run and exit"),
    ("enter", "submit prompt or answer an approval"),
    ("backspace", "delete the character before the cursor"),
    (
        "arrows",
        "move the input cursor; ↑/↓ at edges recall recent prompts",
    ),
    ("mouse wheel", "scroll Chat by line"),
    (
        "y / approve",
        "approve a pending action (high tier: type approve)",
    ),
    (
        "t / trust",
        "approve & trust for this session, when offered",
    ),
    ("n", "deny a pending action"),
];

/// Keybinding rows for the Keys help tab, rendered from the active [`Keymap`]
/// (ADR-003): one line per remappable binding via `keybindings::format_key` plus
/// a short label, then the structurally fixed keys shown locked. Reflects user
/// customizations automatically once the keymap is config-resolved (task_08).
/// Theme tokens only (honors `colors_live_only_in_theme_module`).
fn keys_tab_lines(keymap: &Keymap, theme: &Theme) -> Vec<Line<'static>> {
    let header = |label: &'static str| {
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let body = |text: String| Line::from(Span::styled(text, Style::default().fg(theme.text)));
    let locked =
        |text: String| Line::from(Span::styled(text, Style::default().fg(theme.text_muted)));

    // Active remappable bindings, sorted by action for a stable display order.
    let mut entries: Vec<_> = keymap.entries().collect();
    entries.sort_by_key(|(action, _)| *action);

    let mut lines = vec![header("Remappable keys")];
    if entries.is_empty() {
        lines.push(locked("  (every action unbound)".to_string()));
    }
    for (action, chord) in entries {
        lines.push(body(format!(
            "{:<14} {}",
            keybindings::format_key(&chord),
            keys_action_label(action)
        )));
    }

    lines.push(Line::from(""));
    lines.push(header("Fixed keys (not rebindable)"));
    for (keys, desc) in FIXED_KEY_ROWS {
        lines.push(locked(format!("{keys:<14} {desc}  (locked)")));
    }
    lines
}

/// CLI flag rows for the CLI help tab. Relocated verbatim from the pre-tab
/// `render_help_modal` literals; pure builder consumed by the tabbed render
/// (task 06). No `AppState`/`TuiUiState` reads.
fn cli_tab_lines(_theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from("atelier                            open the TUI"),
        Line::from("atelier --cwd <path>               run from a workspace"),
        Line::from("atelier --config <path>            use a config file"),
        Line::from("atelier --doctor [--json]          check runtimes and history"),
        Line::from("atelier --print-config             print merged config"),
        Line::from("atelier --init-config              create config files"),
        Line::from("atelier --codemap init|changes|update manage repo maps"),
        Line::from("atelier --clean-sessions [--yes]   delete local history"),
        Line::from("atelier --debug                    write debug events"),
        Line::from("atelier --help                     print CLI help"),
    ]
}

/// Static Approvals & Modes prose for the help tab (ADR-001: static in V1).
/// Plain-language explanation of the two approval modes, agent capabilities,
/// and the workspace read/write-roots concept. Net-new content; pure builder
/// consumed by the tabbed render (task 06). No `AppState`/`TuiUiState` reads.
fn approvals_tab_lines(theme: &Theme) -> Vec<Line<'static>> {
    let header = |label: &'static str| {
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let body = |text: &'static str| Line::from(Span::styled(text, Style::default().fg(theme.text)));
    vec![
        header("Approval modes"),
        body("yolo (default): agents act without asking — file writes and"),
        body("commands run automatically. Best for trusted, fast iteration."),
        body("normal: every write or command pauses for your approval before"),
        body("it runs. Read-only actions still proceed without a prompt."),
        Line::from(""),
        header("Risk tiers"),
        body("Each gated action is tiered low / medium / high, with a small"),
        body("catastrophic core (home/root deletion, force-push, secret reads,"),
        body("fetch-and-run). A catastrophic action ALWAYS prompts — even in"),
        body("yolo — and requires retyping the command to confirm; it can never"),
        body("be trusted away. High-tier prompts need the explicit word approve."),
        Line::from(""),
        header("Gray-area floor"),
        body("The gray-area floor governs the in-between actions. [approval]"),
        body("floor = warn (default) surfaces a 'would have blocked' note but"),
        body("still runs them in yolo; floor = enforce makes them prompt instead."),
        Line::from(""),
        header("Session trust"),
        body("approve & trust (the t key) remembers an exact command or write"),
        body("path for this session only, so identical repeats auto-run without"),
        body("a prompt. Trust is in-memory and never persisted. Manage it with"),
        body("/trust (list), /trust revoke <n>, and /trust clear."),
        Line::from(""),
        header("Capabilities"),
        body("Each agent is granted only the capabilities it needs (read,"),
        body("edit, command, plan, review …). An agent cannot take an action"),
        body("its profile does not allow, regardless of approval mode."),
        Line::from(""),
        header("Read / write roots"),
        body("The workspace sets which paths agents may touch. Writes are"),
        body("confined to the write roots; reads to the read roots. Anything"),
        body("outside those roots is off-limits even in yolo mode."),
    ]
}

/// Getting Started tab body — the default help front door. Renders, in order:
/// a one-line routing mental model, two copy-pasteable example prompts, then a
/// compact live agent summary (one row per configured agent via
/// `agent_compact_line`, the shared `Compact` row definition). Pure builder
/// consumed by the tabbed render (task 06).
fn getting_started_lines(state: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let header = |label: &'static str| {
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let body = |text: &'static str| Line::from(Span::styled(text, Style::default().fg(theme.text)));
    // Copy-pasteable example prompts (PRD Open Question): real, runnable, and
    // read-first so a newcomer can try them safely. Marked with `> `.
    let example = |text: &'static str| {
        Line::from(Span::styled(
            format!("> {text}"),
            Style::default().fg(theme.text_muted),
        ))
    };
    let mut lines = vec![
        header("How Atelier works"),
        body("Your prompt -> orchestrator -> named agents do the work."),
        body("In normal mode, approvals gate every file write and command."),
        Line::from(""),
        header("Try a prompt"),
        example("Summarize what this project does and how the run loop works."),
        example("Find where approval mode is enforced and add a test for it."),
        Line::from(""),
        header("Your agents"),
    ];
    if state.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "No agents configured.",
            Style::default().fg(theme.text_muted),
        )));
    } else {
        lines.extend(
            state
                .agents
                .iter()
                .enumerate()
                .map(|(index, agent)| agent_compact_line(index, agent, theme)),
        );
    }
    lines
}

/// Commands tab body — derived from `slash_commands::catalog()` so the tab never
/// drifts from the dropdown or unknown-command guidance. A leading filter line
/// echoes the current `help_filter`; rows are narrowed by a case-insensitive
/// `.contains()` over each command's usage/label (mirroring `skill_suggestions`
/// filtering). An empty filter shows every command; a no-match filter renders an
/// empty-result indicator. Pure builder consumed by the tabbed render. The usage
/// column is padded from the full catalog so alignment stays stable while filtered.
fn commands_tab_lines(filter: &str, theme: &Theme) -> Vec<Line<'static>> {
    let needle = filter.to_ascii_lowercase();
    let catalog = crate::slash_commands::catalog();
    let usage_width = catalog
        .iter()
        .map(|spec| spec.usage.chars().count())
        .max()
        .unwrap_or(0);

    // Filter line: echo the typed text so the user sees what they're narrowing
    // by; a dim hint stands in when the buffer is empty.
    let filter_line = if filter.is_empty() {
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(theme.accent)),
            Span::styled(
                "(type to narrow commands)",
                Style::default().fg(theme.text_dim),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(theme.accent)),
            Span::styled(filter.to_string(), Style::default().fg(theme.text)),
        ])
    };
    let mut lines = vec![filter_line, Line::from("")];

    let matches: Vec<&crate::slash_commands::SlashCommandSpec> = catalog
        .iter()
        .filter(|spec| {
            needle.is_empty()
                || spec.usage.to_ascii_lowercase().contains(&needle)
                || spec.label.to_ascii_lowercase().contains(&needle)
        })
        .collect();

    if matches.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("No commands match \"{filter}\"."),
            Style::default().fg(theme.text_muted),
        )));
    } else {
        lines.extend(
            matches.into_iter().map(|spec| {
                Line::from(format!("{:usage_width$}  {}", spec.usage, spec.description))
            }),
        );
    }
    lines
}

/// Skills tab body — live from `ui_state.skill_suggestions` (project + personal
/// discovery). Lists each skill's `/skill:` alias with its source tag, renders
/// an empty-state line when no skills are discovered, and always closes with the
/// guidance disclaimer (skills do not bypass approvals/permissions). Pure builder
/// consumed by the tabbed render (task 06).
fn skills_tab_lines(ui_state: &TuiUiState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if ui_state.skill_suggestions.is_empty() {
        lines.push(Line::from(Span::styled(
            "No skills discovered in .agents/skills or .claude/skills.",
            Style::default().fg(theme.text_muted),
        )));
    } else {
        for suggestion in &ui_state.skill_suggestions {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("/skill:{} ", suggestion.alias),
                    Style::default().fg(theme.accent),
                ),
                Span::styled(
                    format!("[{}]", suggestion.source_tag.label()),
                    Style::default().fg(theme.text_muted),
                ),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Skills are guidance and do not bypass approvals or permissions.",
        Style::default().fg(theme.text_dim),
    )));
    lines
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn legacy_chat_line<'a>(theme: &Theme, event: &'a str) -> Line<'a> {
    if let Some(message) = event.strip_prefix("You: ") {
        return Line::from(vec![
            Span::styled(
                " You ",
                Style::default()
                    .fg(theme.ink)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(theme.user_prompt_bg)),
            Span::styled(
                message,
                Style::default().fg(theme.text).bg(theme.user_prompt_bg),
            ),
            Span::styled(" ", Style::default().bg(theme.user_prompt_bg)),
        ]);
    }

    if event.contains("failed") || event.contains("Failed") {
        return Line::from(Span::styled(event, Style::default().fg(theme.status_error)));
    }

    Line::from(event)
}

fn status_style(theme: &Theme, status: &str) -> Style {
    match status {
        "running" | "streaming" | "running_parallel" => Style::default()
            .fg(theme.status_ok)
            .add_modifier(Modifier::BOLD),
        "waiting_action" | "waiting_approval" | "waiting_for_user" | "cancelling" => {
            Style::default().fg(theme.status_warn)
        }
        "interrupted" => Style::default().fg(theme.status_error),
        "disabled" => Style::default().fg(theme.text_dim),
        _ => Style::default().fg(theme.text_muted),
    }
}

fn agent_status_label(status: &str) -> &str {
    match status {
        "streaming" => "running",
        "running_parallel" => "running parallel",
        _ => status,
    }
}

/// Glyph vocabulary for the four-state activity model (ADR-002). Set 1 uses
/// portable BMP circles so each state reads without color; `ascii` swaps to a
/// 7-bit fallback for constrained terminals. Plain text only — no inline color
/// literals — so legibility is carried by the glyph plus [`activity_label`]
/// alone (NO_COLOR criterion). Consumed by the roster render (`roster_row_item`).
fn activity_glyph(state: ActivityState, ascii: bool) -> &'static str {
    match (state, ascii) {
        (ActivityState::Active, false) => "◐",
        (ActivityState::NeedsInput, false) => "◔",
        (ActivityState::Stalled, false) => "○",
        (ActivityState::Idle, false) => "·",
        (ActivityState::Active, true) => ">",
        (ActivityState::NeedsInput, true) => "?",
        (ActivityState::Stalled, true) => "!",
        (ActivityState::Idle, true) => ".",
    }
}

/// Distinct, non-empty text label per activity state (ADR-002). Pairs with
/// [`activity_glyph`] so the roster stays legible under `NO_COLOR`. Consumed by
/// the roster render (`roster_row_item`).
fn activity_label(state: ActivityState) -> &'static str {
    match state {
        ActivityState::Active => "working",
        ActivityState::NeedsInput => "waiting",
        ActivityState::Stalled => "stalled?",
        ActivityState::Idle => "idle",
    }
}

fn availability_style(
    theme: &Theme,
    availability: &Option<crate::runtime::RuntimeAvailability>,
) -> Style {
    match availability
        .as_ref()
        .map(|availability| &availability.status)
    {
        Some(crate::runtime::RuntimeAvailabilityStatus::Available) => {
            Style::default().fg(theme.status_ok)
        }
        Some(crate::runtime::RuntimeAvailabilityStatus::Unavailable) => {
            Style::default().fg(theme.status_error)
        }
        Some(crate::runtime::RuntimeAvailabilityStatus::Unknown) | None => {
            Style::default().fg(theme.status_warn)
        }
    }
}

fn availability_label(availability: &Option<crate::runtime::RuntimeAvailability>) -> &'static str {
    match availability
        .as_ref()
        .map(|availability| &availability.status)
    {
        Some(crate::runtime::RuntimeAvailabilityStatus::Available) => "ok",
        Some(crate::runtime::RuntimeAvailabilityStatus::Unavailable) => "down",
        Some(crate::runtime::RuntimeAvailabilityStatus::Unknown) | None => "?",
    }
}

/// Single compact agent row (`name · runtime/model · availability`) shared by
/// the `Compact` roster style and the Getting Started help tab. Extracted so
/// both surfaces render a compact row from one definition: the Getting Started
/// builder returns `Vec<Line>` and cannot consume `agent_roster_items`' opaque
/// `ListItem`s, so the shared core is the line, not the list item. Agent colors
/// cycle via `theme.accent_for(index)`.
fn agent_compact_line(index: usize, agent: &AgentView, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{} ", agent.name),
            Style::default()
                .fg(theme.accent_for(index))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}/{} ", agent.runtime, agent.model),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(
            availability_label(&agent.availability),
            availability_style(theme, &agent.availability),
        ),
    ])
}

/// Builds the per-agent roster rows shared by the Ctrl-L Agent Roster and the
/// Getting Started help tab. `Full` reproduces the three-line roster row
/// (name + status / `runtime/model` + availability / effort + thinking state);
/// `Compact` renders a single line (name · `runtime/model` · availability label).
/// Quarter-circle frames cycled by `work_spinner_frame` so an `Active` agent's
/// glyph visibly turns (ADR-002). Frame 0 is `◐` so a single static render
/// matches [`activity_glyph`]`(Active, _)`. All BMP, no emoji presentation.
const ROSTER_ACTIVE_SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];

/// Build the roster list from the pre-computed [`RosterRow`] view-model (ADR-003):
/// a summary-header line followed by one item per row. The renderer stays a pure
/// function of `AppState` — every time-dependent value is read from the row, and
/// agent colors come from `row.accent_index` (canonical identity, ADR-005), never
/// the render-time position, so the `NeedsInput` pin cannot recolor an agent.
fn agent_roster_items(
    rows: &[RosterRow],
    spinner_frame: usize,
    theme: &Theme,
) -> Vec<ListItem<'static>> {
    let mut items = vec![roster_summary_header_item(rows, theme)];
    items.extend(
        rows.iter()
            .map(|row| roster_row_item(row, spinner_frame, theme)),
    );
    items
}

/// One-line activity census above the roster (ADR-001 item: summary header).
/// At rest it shows a calm lineup count; during a run it shows working/waiting/
/// stalled counts with portable glyphs. Theme tokens only — no inline colors.
fn roster_summary_header_item(rows: &[RosterRow], theme: &Theme) -> ListItem<'static> {
    let mut working = 0usize;
    let mut waiting = 0usize;
    let mut stalled = 0usize;
    for row in rows {
        match row.activity {
            ActivityState::Active => working += 1,
            ActivityState::NeedsInput => waiting += 1,
            ActivityState::Stalled => stalled += 1,
            ActivityState::Idle => {}
        }
    }
    let line = if working == 0 && waiting == 0 && stalled == 0 {
        Line::from(Span::styled(
            format!("● {} agents idle", rows.len()),
            Style::default().fg(theme.text_dim),
        ))
    } else {
        Line::from(Span::styled(
            format!("▶ {working} working · ◔ {waiting} waiting · ○ {stalled} stalled"),
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::BOLD),
        ))
    };
    ListItem::new(line)
}

/// Render one roster row: glyph + activity label + name on the lead line (weight
/// driven by activity), then runtime/model + effort/thinking, and — for active
/// states only — the pre-formatted current step and coarse elapsed. Terminal and
/// plain-idle rows keep the existing status label instead of an activity glyph.
fn roster_row_item(row: &RosterRow, spinner_frame: usize, theme: &Theme) -> ListItem<'static> {
    // Single-source accent rule (ADR-005, task_07; see `item_agent_accent`):
    // canonical identity index, never the render-time row position — so the
    // `NeedsInput` pin reorders rows without recoloring any agent.
    let accent = theme.accent_for(row.accent_index);
    let mut lines: Vec<Line<'static>> = Vec::new();

    let mut lead: Vec<Span<'static>> = Vec::new();
    let active_states = !matches!(row.activity, ActivityState::Idle);
    if active_states {
        let glyph = if matches!(row.activity, ActivityState::Active) {
            ROSTER_ACTIVE_SPINNER[spinner_frame % ROSTER_ACTIVE_SPINNER.len()]
        } else {
            activity_glyph(row.activity.clone(), false)
        };
        // Active rows are bold and brightly named; waiting/stalled stay normal
        // weight but keep the prominent glyph + label (ADR-001).
        let name_style = if matches!(row.activity, ActivityState::Active) {
            Style::default().fg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(accent)
        };
        lead.push(Span::styled(
            format!("{glyph} {} ", activity_label(row.activity.clone())),
            Style::default().fg(theme.text),
        ));
        lead.push(Span::styled(row.name.clone(), name_style));
    } else {
        // Idle recedes via the DIM modifier, but the name keeps its identity
        // accent (ADR-005) — never `text_dim`, or it loses the agent color that
        // links the roster to the chat transcript. Terminal statuses keep their
        // labelled badge.
        lead.push(Span::styled(
            format!("{} ", row.name),
            Style::default().fg(accent).add_modifier(Modifier::DIM),
        ));
        if row.status != "idle" {
            lead.push(Span::styled(
                agent_status_label(&row.status).to_string(),
                status_style(theme, &row.status),
            ));
        }
    }
    lines.push(Line::from(lead));

    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", row.runtime_model),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(
            format!("effort:{} ", row.effort),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(
            if row.thinking {
                "thinking:on"
            } else {
                "thinking:off"
            },
            if row.thinking {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.text_dim)
            },
        ),
    ]));

    // Step + elapsed are pre-formatted on the row (active states only).
    if row.current_step.is_some() || row.elapsed.is_some() {
        let mut detail = String::new();
        if let Some(step) = &row.current_step {
            detail.push_str(step);
        }
        if let Some(elapsed) = &row.elapsed {
            if !detail.is_empty() {
                detail.push_str(" | ");
            }
            detail.push_str(elapsed);
        }
        lines.push(Line::from(Span::styled(
            detail,
            Style::default().fg(theme.text_dim),
        )));
    }

    ListItem::new(lines)
}

fn work_indicator_active(state: &AppState) -> bool {
    matches!(state.run_state, RunState::Planning | RunState::Running)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputAreas {
    input: Rect,
    status: Rect,
    footer: Rect,
}

fn input_areas(composer_area: Rect) -> InputAreas {
    if composer_area.height <= WORK_INDICATOR_HEIGHT {
        return InputAreas {
            input: composer_area,
            status: Rect::ZERO,
            footer: Rect::ZERO,
        };
    }

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(INPUT_BOX_HEIGHT),
            Constraint::Length(WORK_INDICATOR_HEIGHT),
            Constraint::Length(FOOTER_HEIGHT),
            Constraint::Min(0),
        ])
        .split(composer_area);

    InputAreas {
        input: areas[0],
        status: areas[1],
        footer: areas[2],
    }
}

fn render_input_status(
    frame: &mut Frame,
    status_area: Rect,
    ui_state: &mut TuiUiState,
    work_active: bool,
    input_empty: bool,
) {
    if status_area.width == 0 || status_area.height == 0 {
        return;
    }
    let theme = ui_state.theme;
    let line_area = Rect {
        x: status_area.x + 1,
        y: status_area.y,
        width: status_area.width.saturating_sub(2),
        height: 1,
    };
    let line_width = usize::from(line_area.width);
    let status_message = (!work_active)
        .then_some(ui_state.status_message.as_deref())
        .flatten();
    let left_width = if work_active {
        1 + 1 + WORK_LABEL.chars().count()
    } else if let Some(message) = status_message {
        message.chars().count()
    } else {
        0
    };
    // Advertise ↑/↓ recall only when it can actually fire: no active work, an
    // empty composer, and a non-empty ring. Otherwise keep the plain /help hint.
    let hint = if !work_active && input_empty && !ui_state.prompt_history.is_empty() {
        HISTORY_HINT
    } else {
        WORK_HINT
    };
    let hint_width = hint.chars().count();
    let mut spans = Vec::new();
    if work_active {
        let spinner = WORK_SPINNER_FRAMES[ui_state.work_spinner_frame % WORK_SPINNER_FRAMES.len()];
        ui_state.work_spinner_frame = ui_state.work_spinner_frame.wrapping_add(1);
        spans.extend([
            Span::styled(spinner, Style::default().fg(theme.accent)),
            Span::raw(" "),
            Span::styled(
                WORK_LABEL,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    } else {
        ui_state.work_spinner_frame = 0;
        if let Some(message) = status_message {
            spans.push(Span::styled(
                message.to_string(),
                Style::default().fg(theme.status_ok),
            ));
        }
    }
    if line_width >= left_width.saturating_add(hint_width) {
        spans.push(Span::raw(
            " ".repeat(line_width.saturating_sub(left_width + hint_width)),
        ));
        spans.push(Span::styled(
            hint,
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Clear, status_area);
    frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
}

/// Render the ambient status footer (line 2 of the composer): repo·branch (when
/// in a git repo) · run state · agent summary. Single line, never wrapped, so
/// narrow terminals clip rather than reflow (req: no panic/wrap).
fn render_footer(frame: &mut Frame, footer_area: Rect, state: &AppState, theme: &Theme) {
    if footer_area.width == 0 || footer_area.height == 0 {
        return;
    }
    let line_area = Rect {
        x: footer_area.x + 1,
        y: footer_area.y,
        width: footer_area.width.saturating_sub(2),
        height: 1,
    };
    let line = footer_line(
        theme,
        state.git_context.as_ref(),
        &state.run_state,
        &state.agents,
        usize::from(line_area.width),
    );
    frame.render_widget(Clear, footer_area);
    frame.render_widget(Paragraph::new(line), line_area);
}

/// Build the footer line. The git segment is omitted entirely (no separator
/// artifact) outside a repo; on narrow widths the branch is truncated first so
/// the run state and agent summary stay legible.
fn footer_line(
    theme: &Theme,
    git: Option<&GitContext>,
    run_state: &RunState,
    agents: &[AgentView],
    width: usize,
) -> Line<'static> {
    const SEP: &str = " · ";
    let sep_style = Style::default().fg(theme.text_dim);
    let run = run_state_label(run_state).to_string();
    let agents_text = agent_summary(agents);
    let tail_width = run.chars().count() + SEP.chars().count() + agents_text.chars().count();

    let mut spans = Vec::new();
    if let Some(git) = git {
        // Budget for the git segment ("repo · branch · "), then truncate the
        // branch to whatever remains after the always-shown tail.
        let repo_segment = format!("{}{SEP}", git.repo_name);
        let reserved = tail_width + SEP.chars().count() + repo_segment.chars().count();
        let branch_budget = width.saturating_sub(reserved);
        let branch = truncate_branch(&git.branch, branch_budget);
        spans.push(Span::styled(
            git.repo_name.clone(),
            Style::default().fg(theme.text_muted),
        ));
        spans.push(Span::styled(SEP, sep_style));
        spans.push(Span::styled(branch, Style::default().fg(theme.accent)));
        spans.push(Span::styled(SEP, sep_style));
    }
    spans.push(Span::styled(run, run_state_style(theme, run_state)));
    spans.push(Span::styled(SEP, sep_style));
    spans.push(Span::styled(
        agents_text,
        Style::default().fg(theme.text_muted),
    ));
    Line::from(spans)
}

fn run_state_label(run_state: &RunState) -> &'static str {
    match run_state {
        RunState::Idle => "idle",
        RunState::Planning => "planning",
        RunState::Running => "running",
        RunState::WaitingForUser => "waiting for user",
        RunState::Interrupted => "interrupted",
        RunState::Completed => "completed",
        RunState::Failed => "failed",
        RunState::LimitReached => "limit reached",
    }
}

fn run_state_style(theme: &Theme, run_state: &RunState) -> Style {
    let color = match run_state {
        RunState::Idle => theme.text_dim,
        RunState::Planning | RunState::Running => theme.accent,
        RunState::WaitingForUser => theme.status_warn,
        RunState::Completed => theme.status_ok,
        RunState::Interrupted | RunState::Failed | RunState::LimitReached => theme.status_error,
    };
    Style::default().fg(color)
}

/// "{n} agents" plus "· {r} running" when any agent is in a running status.
fn agent_summary(agents: &[AgentView]) -> String {
    let running = agents
        .iter()
        .filter(|agent| RUNNING_AGENT_STATUSES.contains(&agent.status.as_str()))
        .count();
    if running > 0 {
        format!("{} agents · {running} running", agents.len())
    } else {
        format!("{} agents", agents.len())
    }
}

/// Truncate a branch name to `max` display cells, appending an ellipsis when cut.
fn truncate_branch(branch: &str, max: usize) -> String {
    if branch.chars().count() <= max {
        return branch.to_string();
    }
    match max {
        0 => String::new(),
        1 => "…".to_string(),
        _ => {
            let kept: String = branch.chars().take(max - 1).collect();
            format!("{kept}…")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputLayout {
    width: usize,
    cursor_col: u16,
    cursor_row: u16,
    scroll: usize,
}

fn input_layout(input_area: Rect, input: &str, cursor: usize) -> InputLayout {
    let inner_width = usize::from(input_area.width.saturating_sub(2).max(1));
    let width = inner_width.saturating_sub(INPUT_PROMPT_WIDTH).max(1);
    let visible_rows = input_area.height.saturating_sub(2).max(1);
    let visible_rows = usize::from(visible_rows);
    let cursor_cells = cursor.min(input_char_count(input));
    let cursor_line = cursor_cells / width;
    let cursor_col = cursor_cells % width;
    let scroll = cursor_line.saturating_sub(visible_rows.saturating_sub(1));
    let visible_cursor_row = cursor_line.saturating_sub(scroll);
    InputLayout {
        width,
        cursor_col: INPUT_PROMPT_WIDTH
            .saturating_add(cursor_col)
            .min(inner_width.saturating_sub(1)) as u16,
        cursor_row: visible_cursor_row.min(visible_rows.saturating_sub(1)) as u16,
        scroll,
    }
}

fn wrapped_input_lines(theme: &Theme, input: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    if input.is_empty() {
        return vec![prompted_input_line(theme, "", true)];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0usize;
    for ch in input.chars() {
        line.push(ch);
        line_len += 1;
        if line_len == width {
            lines.push(prompted_input_line(theme, &line, lines.is_empty()));
            line.clear();
            line_len = 0;
        }
    }
    if !line.is_empty() || input.chars().count().is_multiple_of(width) {
        lines.push(prompted_input_line(theme, &line, lines.is_empty()));
    }
    lines
}

fn prompted_input_line(theme: &Theme, input: &str, first_line: bool) -> Line<'static> {
    let prefix = if first_line { INPUT_PROMPT } else { "  " };
    Line::from(vec![
        Span::styled(prefix, Style::default().fg(theme.accent)),
        Span::raw(input.to_string()),
    ])
}

fn set_input_cursor(frame: &mut Frame, input_area: Rect, input_layout: InputLayout) {
    frame.set_cursor_position(Position::new(
        input_area.x + 1 + input_layout.cursor_col,
        input_area.y + 1 + input_layout.cursor_row,
    ));
}

/// Rows the chat (and any queue panel) must keep when the clarification composer
/// grows to fit a long question or many options.
const CLARIFICATION_MIN_CHAT_ROWS: u16 = 6;

fn clarification_inner_width(area: Rect) -> u16 {
    area.width.saturating_sub(2).max(1)
}

fn composer_height(state: &AppState, ui_state: &TuiUiState, area: Rect, reserved_rows: u16) -> u16 {
    let inner_width = clarification_inner_width(area);
    let content_rows = if let Some(clarification) = &state.pending_clarification {
        let layout = clarification_layout(clarification, ui_state, &ui_state.theme);
        wrapped_event_line_count(&layout.lines, inner_width)
    } else if let Some(pending) = &state.pending_governance_decision {
        // Mirror the lines the governance composer renders (card + redirect echo)
        // so the composer is tall enough to show the decision.
        let mut lines = governance_decision_card_lines(&pending.view, &ui_state.theme);
        lines.push(Line::from(String::new()));
        lines.push(Line::from(format!("Redirect (optional): {}", state.input)));
        wrapped_event_line_count(&lines, inner_width)
    } else if let Some(pending) = &state.pending_plan_approval {
        // Mirror render_plan_approval_composer's lines so the composer is tall
        // enough to show the plan summary + the reject-reason line.
        let lines = vec![
            Line::from(pending.summary.clone()),
            Line::from("Accept to run the plan, or reject to send it back to the orchestrator."),
            Line::from(String::new()),
            Line::from(format!("Reject reason (optional): {}", state.input)),
        ];
        wrapped_event_line_count(&lines, inner_width)
    } else {
        return INPUT_COMPOSER_HEIGHT;
    };
    // borders (2) + wrapped content + status hint line.
    let desired = content_rows
        .saturating_add(2)
        .saturating_add(usize::from(WORK_INDICATOR_HEIGHT));
    // Never crowd the chat/queue below the reserved minimum; the composer
    // truncates gracefully on very short terminals instead.
    let reserved =
        usize::from(CLARIFICATION_MIN_CHAT_ROWS).saturating_add(usize::from(reserved_rows));
    let cap = usize::from(area.height)
        .saturating_sub(reserved)
        .max(usize::from(INPUT_COMPOSER_HEIGHT));
    desired.min(cap).min(usize::from(u16::MAX)) as u16
}

fn clarification_input_areas(composer_area: Rect) -> InputAreas {
    // The clarification composer keeps the original single status line (its own
    // hint), so it carries no ambient footer.
    if composer_area.height <= WORK_INDICATOR_HEIGHT {
        return InputAreas {
            input: composer_area,
            status: Rect::ZERO,
            footer: Rect::ZERO,
        };
    }
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(composer_area.height - WORK_INDICATOR_HEIGHT),
            Constraint::Length(WORK_INDICATOR_HEIGHT),
        ])
        .split(composer_area);
    InputAreas {
        input: areas[0],
        status: areas[1],
        footer: Rect::ZERO,
    }
}

/// The fully-built clarification composer body, the line index of the focused
/// row (so the composer can scroll to keep it visible), and — when the custom
/// row is focused — the cursor's display-column offset within the last line.
struct ClarificationLayout {
    lines: Vec<Line<'static>>,
    focused_line: usize,
    custom_cursor_col: Option<usize>,
}

/// Builds the composer body: a wrapped bold question, one row per option (a
/// number in single-select, a checkbox in multi-select) with an optional muted
/// description beneath, and a synthetic custom-answer row that reveals an input
/// when focused.
fn clarification_layout(
    clarification: &PendingClarificationView,
    ui_state: &TuiUiState,
    theme: &Theme,
) -> ClarificationLayout {
    let custom_row = clarification.options.len();
    let focused = ui_state.clarification_option_index.min(custom_row);
    let multi = clarification.multi_select;
    let span_width = |s: &str| Span::raw(s.to_string()).width();

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut focused_line = 0;
    lines.push(Line::from(Span::styled(
        clarification.question.clone(),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(String::new()));

    for (index, option) in clarification.options.iter().enumerate() {
        let is_focused = index == focused;
        if is_focused {
            focused_line = lines.len();
        }
        let checked = ui_state.clarification_selected.contains(&index);
        let recommended = clarification
            .recommended_option_id
            .as_deref()
            .is_some_and(|id| id == option.id);

        let marker = if is_focused {
            CLARIFICATION_FOCUS_MARKER
        } else {
            CLARIFICATION_BLUR_MARKER
        };
        let selector = if multi {
            if checked {
                CLARIFICATION_CHECK_ON.to_string()
            } else {
                CLARIFICATION_CHECK_OFF.to_string()
            }
        } else {
            format!("{}. ", index + 1)
        };
        let label_style = if is_focused {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.accent)),
            Span::styled(
                selector.clone(),
                Style::default().fg(if is_focused {
                    theme.accent
                } else {
                    theme.text_muted
                }),
            ),
            Span::styled(option.label.clone(), label_style),
        ];
        if recommended {
            spans.push(Span::styled(
                CLARIFICATION_RECOMMENDED_SUFFIX.to_string(),
                Style::default().fg(theme.text_dim),
            ));
        }
        lines.push(Line::from(spans));

        if let Some(description) = option.description.as_deref() {
            if !description.trim().is_empty() {
                let indent = " ".repeat(span_width(marker) + span_width(&selector));
                lines.push(Line::from(Span::styled(
                    format!("{indent}{description}"),
                    Style::default().fg(theme.text_dim),
                )));
            }
        }
    }

    // Synthetic custom row (always last so the cursor math stays simple).
    let custom_focused = focused == custom_row;
    if custom_focused {
        focused_line = lines.len();
    }
    let marker = if custom_focused {
        CLARIFICATION_FOCUS_MARKER
    } else {
        CLARIFICATION_BLUR_MARKER
    };
    let selector = if multi {
        "    ".to_string()
    } else {
        format!("{}. ", custom_row + 1)
    };
    let typed = ui_state.clarification_custom_answer.clone();
    let mut custom_cursor_col = None;
    let custom_spans = if custom_focused {
        custom_cursor_col = Some(
            span_width(marker)
                + span_width(&selector)
                + span_width(CLARIFICATION_CUSTOM_PROMPT)
                + span_width(&typed),
        );
        vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.accent)),
            Span::styled(selector, Style::default().fg(theme.accent)),
            Span::styled(
                CLARIFICATION_CUSTOM_PROMPT.to_string(),
                Style::default().fg(theme.accent),
            ),
            Span::styled(typed, Style::default().fg(theme.text)),
        ]
    } else if typed.is_empty() {
        vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.text_muted)),
            Span::styled(selector, Style::default().fg(theme.text_muted)),
            Span::styled(
                CLARIFICATION_CUSTOM_PLACEHOLDER.to_string(),
                Style::default().fg(theme.text_dim),
            ),
        ]
    } else {
        vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.text_muted)),
            Span::styled(selector, Style::default().fg(theme.text_muted)),
            Span::styled(
                CLARIFICATION_CUSTOM_PROMPT.to_string(),
                Style::default().fg(theme.text_muted),
            ),
            Span::styled(typed, Style::default().fg(theme.text_dim)),
        ]
    };
    lines.push(Line::from(custom_spans));

    ClarificationLayout {
        lines,
        focused_line,
        custom_cursor_col,
    }
}

/// Vertical scroll offset (in wrapped rows) that keeps the focused row's last
/// wrapped row inside an `inner_height`-tall viewport. Zero whenever everything
/// fits, so normal-sized terminals are unaffected.
fn clarification_scroll_offset(
    lines: &[Line<'_>],
    focused_line: usize,
    inner_width: u16,
    inner_height: usize,
) -> usize {
    if inner_height == 0 || focused_line >= lines.len() {
        return 0;
    }
    let width = usize::from(inner_width.max(1));
    let rows_above = wrapped_event_line_count(&lines[..focused_line], inner_width);
    let focused_rows = lines[focused_line].width().max(1).div_ceil(width);
    (rows_above + focused_rows).saturating_sub(inner_height)
}

fn render_clarification_composer(
    frame: &mut Frame,
    area: Rect,
    clarification: &PendingClarificationView,
    ui_state: &TuiUiState,
) {
    let theme = ui_state.theme;
    let layout = clarification_layout(clarification, ui_state, &theme);
    let title = if clarification.multi_select {
        " Clarifying question · select any "
    } else {
        " Clarifying question "
    };
    // Scroll so the focused row stays visible when the body is taller than the
    // (capped) composer on short terminals; zero on roomy terminals.
    let inner_height = usize::from(area.height.saturating_sub(2));
    let scroll = clarification_scroll_offset(
        &layout.lines,
        layout.focused_line,
        clarification_inner_width(area),
        inner_height,
    );
    let composer = Paragraph::new(layout.lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0))
        .block(
            Block::default()
                .title(title)
                .title_style(Style::default().fg(theme.accent))
                .border_style(Style::default().fg(theme.accent))
                .borders(Borders::ALL),
        );
    frame.render_widget(composer, area);
}

fn render_clarification_status(
    frame: &mut Frame,
    status_area: Rect,
    theme: &Theme,
    multi_select: bool,
) {
    if status_area.width == 0 || status_area.height == 0 {
        return;
    }
    let line_area = Rect {
        x: status_area.x + 1,
        y: status_area.y,
        width: status_area.width.saturating_sub(2),
        height: 1,
    };
    let hint = if multi_select {
        CLARIFICATION_HINT_MULTI
    } else {
        CLARIFICATION_HINT_SINGLE
    };
    frame.render_widget(Clear, status_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.text_muted),
        ))),
        line_area,
    );
}

/// Build the governance decision card body: the headline, the interpreted
/// intent, the approach bullets, the responsible agent, the write-scope, and the
/// plain-language risk label. Risk is an explicit text label (words, never color
/// alone) so it stays legible under monochrome / `NO_COLOR`.
fn governance_decision_card_lines(
    view: &GovernanceDecisionView,
    theme: &Theme,
) -> Vec<Line<'static>> {
    // The headline, a spacer, the interpreted intent, and the risk label are
    // always present. Risk sits right after the intent — an explicit text label,
    // not color alone, so it reads under NO_COLOR and survives the body cap on
    // short terminals.
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            view.title.clone(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(String::new()),
        Line::from(vec![
            Span::styled("Intent: ", Style::default().fg(theme.text_muted)),
            Span::styled(view.intent.clone(), Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Risk: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                view.risk_label.clone(),
                Style::default()
                    .fg(theme.status_warn)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    if !view.approach.is_empty() {
        lines.push(Line::from(Span::styled(
            "Approach:".to_string(),
            Style::default().fg(theme.text_muted),
        )));
        for bullet in &view.approach {
            lines.push(Line::from(Span::styled(
                format!("  - {bullet}"),
                Style::default().fg(theme.text_dim),
            )));
        }
    }

    if let Some(agent) = view
        .agent
        .as_deref()
        .filter(|agent| !agent.trim().is_empty())
    {
        lines.push(Line::from(vec![
            Span::styled("Agent: ", Style::default().fg(theme.text_muted)),
            Span::styled(agent.to_string(), Style::default().fg(theme.text)),
        ]));
    }

    if !view.write_scope.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Write scope: ", Style::default().fg(theme.text_muted)),
            Span::styled(view.write_scope.join(", "), Style::default().fg(theme.text)),
        ]));
    }

    lines
}

fn render_governance_decision_composer(
    frame: &mut Frame,
    area: Rect,
    view: &GovernanceDecisionView,
    input: &str,
    ui_state: &TuiUiState,
) {
    let theme = ui_state.theme;
    let mut lines = governance_decision_card_lines(view, &theme);
    // Echo the redirect being composed so the user sees what Enter will send.
    lines.push(Line::from(String::new()));
    lines.push(Line::from(vec![
        Span::styled(
            "Redirect (optional): ",
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(input.to_string(), Style::default().fg(theme.text)),
    ]));

    let inner_height = usize::from(area.height.saturating_sub(2));
    // When the user is composing a redirect, keep the redirect line (the last
    // line) visible; otherwise show the decision content from the top so the
    // intent and risk are never scrolled away.
    let focus_line = if input.is_empty() {
        0
    } else {
        lines.len().saturating_sub(1)
    };
    let scroll = clarification_scroll_offset(
        &lines,
        focus_line,
        clarification_inner_width(area),
        inner_height,
    );
    let composer = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0))
        .block(
            Block::default()
                .title(" Confirm intent · governance ")
                .title_style(Style::default().fg(theme.accent))
                .border_style(Style::default().fg(theme.accent))
                .borders(Borders::ALL),
        );
    frame.render_widget(composer, area);
}

fn render_governance_decision_status(frame: &mut Frame, status_area: Rect, theme: &Theme) {
    if status_area.width == 0 || status_area.height == 0 {
        return;
    }
    let line_area = Rect {
        x: status_area.x + 1,
        y: status_area.y,
        width: status_area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(Clear, status_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            GOVERNANCE_DECISION_HINT,
            Style::default().fg(theme.text_muted),
        ))),
        line_area,
    );
}

fn render_plan_approval_composer(
    frame: &mut Frame,
    area: Rect,
    view: &PendingPlanApprovalView,
    input: &str,
    ui_state: &TuiUiState,
) {
    let theme = ui_state.theme;
    // The full graph is already rendered in the durable Plan chat item; the
    // composer just restates the decision and echoes the reject reason being typed.
    let lines = vec![
        Line::from(Span::styled(
            view.summary.clone(),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "Accept to run the plan, or reject to send it back to the orchestrator.",
            Style::default().fg(theme.text_muted),
        )),
        Line::from(String::new()),
        Line::from(vec![
            Span::styled(
                "Reject reason (optional): ",
                Style::default().fg(theme.text_muted),
            ),
            Span::styled(input.to_string(), Style::default().fg(theme.text)),
        ]),
    ];
    let inner_height = usize::from(area.height.saturating_sub(2));
    let focus_line = if input.is_empty() {
        0
    } else {
        lines.len().saturating_sub(1)
    };
    let scroll = clarification_scroll_offset(
        &lines,
        focus_line,
        clarification_inner_width(area),
        inner_height,
    );
    let composer = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0))
        .block(
            Block::default()
                .title(" Review plan · approval ")
                .title_style(Style::default().fg(theme.accent))
                .border_style(Style::default().fg(theme.accent))
                .borders(Borders::ALL),
        );
    frame.render_widget(composer, area);
}

fn render_plan_approval_status(frame: &mut Frame, status_area: Rect, theme: &Theme) {
    if status_area.width == 0 || status_area.height == 0 {
        return;
    }
    let line_area = Rect {
        x: status_area.x + 1,
        y: status_area.y,
        width: status_area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(Clear, status_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            PLAN_APPROVAL_HINT,
            Style::default().fg(theme.text_muted),
        ))),
        line_area,
    );
}

fn set_clarification_cursor(
    frame: &mut Frame,
    area: Rect,
    clarification: &PendingClarificationView,
    ui_state: &TuiUiState,
) {
    if area.width < 3 || area.height < 3 {
        return;
    }
    let layout = clarification_layout(clarification, ui_state, &ui_state.theme);
    let Some(cursor_col) = layout.custom_cursor_col else {
        return;
    };
    let inner_width_u16 = clarification_inner_width(area);
    let inner_width = usize::from(inner_width_u16);
    let inner_height = usize::from(area.height.saturating_sub(2));
    // The custom row is the last line; rows above it consume the wrapped height
    // of everything before, and the cursor wraps within the custom line itself.
    // Subtract the same scroll offset the composer renders with so the caret
    // tracks the visible custom input rather than clamping onto a clipped row.
    let last = layout.lines.len().saturating_sub(1);
    let rows_above = wrapped_event_line_count(&layout.lines[..last], inner_width_u16);
    let scroll = clarification_scroll_offset(
        &layout.lines,
        layout.focused_line,
        inner_width_u16,
        inner_height,
    );
    let row = (rows_above + cursor_col / inner_width).saturating_sub(scroll);
    let col = cursor_col % inner_width;
    frame.set_cursor_position(Position::new(
        area.x + 1 + col.min(inner_width.saturating_sub(1)) as u16,
        area.y + 1 + row.min(inner_height.saturating_sub(1)) as u16,
    ));
}

/// Resets the composer's transient selection when a new question arrives (and
/// clears it once the clarification is dismissed), defaulting focus to the
/// recommended option.
fn sync_clarification_state(state: &AppState, ui_state: &mut TuiUiState) {
    match &state.pending_clarification {
        Some(view) => {
            if ui_state.clarification_question_id.as_deref() != Some(view.question_id.as_str()) {
                ui_state.clarification_question_id = Some(view.question_id.clone());
                ui_state.clarification_selected.clear();
                ui_state.clarification_custom_answer.clear();
                ui_state.clarification_submitting = false;
                ui_state.clarification_option_index = clarification_default_focus(view);
                // In multi-select, pre-check the recommended option so the
                // highlighted "★ recommended" row honestly shows [x] and Enter
                // confirms it. Only do so when the recommended id resolves to a
                // real option — otherwise the default-focus fallback would
                // silently pre-check the first option for a stale/missing id.
                if view.multi_select {
                    if let Some(index) = view
                        .recommended_option_id
                        .as_deref()
                        .and_then(|id| view.options.iter().position(|option| option.id == id))
                    {
                        ui_state.clarification_selected.insert(index);
                    }
                }
            } else {
                let row_count = view.options.len() + 1;
                ui_state.clarification_option_index =
                    ui_state.clarification_option_index.min(row_count - 1);
            }
        }
        None => {
            if ui_state.clarification_question_id.is_some() || ui_state.clarification_submitting {
                ui_state.clarification_question_id = None;
                ui_state.clarification_selected.clear();
                ui_state.clarification_custom_answer.clear();
                ui_state.clarification_option_index = 0;
                ui_state.clarification_submitting = false;
            }
        }
    }
}

fn clarification_default_focus(view: &PendingClarificationView) -> usize {
    view.recommended_option_id
        .as_deref()
        .and_then(|id| view.options.iter().position(|option| option.id == id))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::chat::ChatProjection;
    use crate::app::{
        AgentView, ConfigStatusView, LiveStepStatus, LiveStepView, LiveStreamView,
        PendingClarificationView, PendingGovernanceDecisionView,
    };
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use crate::governance::GovernanceKind;
    use crate::history::HistoryEvent;
    use crate::orchestrator::{ClarificationOption, RunState};
    use crate::runtime::{RuntimeAvailability, RuntimeAvailabilityStatus};
    use ratatui::backend::TestBackend;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn help_tab_all_is_ordered_and_complete() {
        assert_eq!(HelpTab::ALL.len(), 6);
        assert_eq!(HelpTab::ALL[0], HelpTab::GettingStarted);
        assert_eq!(*HelpTab::ALL.last().unwrap(), HelpTab::Cli);
        assert_eq!(
            HelpTab::ALL,
            [
                HelpTab::GettingStarted,
                HelpTab::Commands,
                HelpTab::Keys,
                HelpTab::Skills,
                HelpTab::Approvals,
                HelpTab::Cli,
            ]
        );
    }

    #[test]
    fn help_tab_next_wraps_around() {
        assert_eq!(HelpTab::GettingStarted.next(), HelpTab::Commands);
        assert_eq!(HelpTab::Commands.next(), HelpTab::Keys);
        assert_eq!(HelpTab::Cli.next(), HelpTab::GettingStarted);
    }

    #[test]
    fn help_tab_prev_wraps_around() {
        assert_eq!(HelpTab::GettingStarted.prev(), HelpTab::Cli);
        assert_eq!(HelpTab::Commands.prev(), HelpTab::GettingStarted);
        assert_eq!(HelpTab::Cli.prev(), HelpTab::Approvals);
    }

    #[test]
    fn help_tab_next_prev_round_trip_for_every_tab() {
        for tab in HelpTab::ALL {
            assert_eq!(tab.next().prev(), tab);
            assert_eq!(tab.prev().next(), tab);
        }
    }

    #[test]
    fn help_tab_titles_are_correct() {
        assert_eq!(HelpTab::GettingStarted.title(), "Getting Started");
        assert_eq!(HelpTab::Commands.title(), "Commands");
        assert_eq!(HelpTab::Keys.title(), "Keys");
        assert_eq!(HelpTab::Skills.title(), "Skills");
        assert_eq!(HelpTab::Approvals.title(), "Approvals");
        assert_eq!(HelpTab::Cli.title(), "CLI");
    }

    #[test]
    fn roster_row_style_variants_are_distinct() {
        assert_ne!(RosterRowStyle::Full, RosterRowStyle::Compact);
    }

    /// Renders a built list of roster items to flattened buffer text so tests can
    /// assert on the visible content without reaching into `ListItem` internals.
    fn roster_items_to_text(items: Vec<ListItem<'static>>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| frame.render_widget(List::new(items), frame.area()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn unavailable_agent_view() -> AgentView {
        AgentView {
            id: "fixer".to_string(),
            name: "Fixer".to_string(),
            runtime: "codex".to_string(),
            model: "default".to_string(),
            effort: "high".to_string(),
            thinking: false,
            capabilities: vec!["read".to_string()],
            availability: Some(RuntimeAvailability {
                runtime_id: "codex".to_string(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: "missing command".to_string(),
                remediation: None,
            }),
            status: "running".to_string(),
        }
    }

    fn roster_row(
        agent_id: &str,
        name: &str,
        accent_index: usize,
        activity: ActivityState,
        current_step: Option<&str>,
        elapsed: Option<&str>,
    ) -> RosterRow {
        RosterRow {
            agent_id: agent_id.to_string(),
            name: name.to_string(),
            accent_index,
            activity,
            runtime_model: "fake/default".to_string(),
            effort: "medium".to_string(),
            thinking: false,
            current_step: current_step.map(str::to_string),
            elapsed: elapsed.map(str::to_string),
            status: "idle".to_string(),
        }
    }

    /// Populate `state.roster_rows` from `agents`/`live_steps` the way
    /// `publish_state` does in production, so full-frame render tests see the
    /// roster they expect (the renderer reads `roster_rows`, never `agents`).
    fn populate_roster_rows(state: &mut AppState) {
        state.roster_rows = crate::app::build_roster_rows(
            &state.agents,
            &state.live_steps,
            &std::collections::BTreeMap::new(),
            std::time::Instant::now(),
        );
    }

    #[test]
    fn agent_roster_items_emits_header_then_one_item_per_row() {
        let theme = TuiUiState::default().theme;
        let rows = vec![
            roster_row(
                "fixer",
                "Fixer",
                0,
                ActivityState::Active,
                Some("patching"),
                Some("12s"),
            ),
            roster_row("planner", "Planner", 1, ActivityState::Idle, None, None),
        ];
        let items = agent_roster_items(&rows, 0, &theme);
        // One summary header item plus one item per row.
        assert_eq!(items.len(), rows.len() + 1);
    }

    #[test]
    fn roster_header_counts_active_states() {
        let theme = TuiUiState::default().theme;
        let rows = vec![
            roster_row("a", "Anna", 0, ActivityState::Active, None, None),
            roster_row("b", "Bret", 1, ActivityState::Active, None, None),
            roster_row("c", "Cleo", 2, ActivityState::NeedsInput, None, None),
            roster_row("d", "Dane", 3, ActivityState::Stalled, None, None),
        ];
        let text = roster_items_to_text(agent_roster_items(&rows, 0, &theme), 80, 20);
        assert!(
            text.contains("▶ 2 working · ◔ 1 waiting · ○ 1 stalled"),
            "summary header counts: {text}"
        );
    }

    #[test]
    fn roster_header_at_rest_shows_idle_lineup() {
        let theme = TuiUiState::default().theme;
        let rows = vec![
            roster_row("a", "Anna", 0, ActivityState::Idle, None, None),
            roster_row("b", "Bret", 1, ActivityState::Idle, None, None),
            roster_row("c", "Cleo", 2, ActivityState::Idle, None, None),
        ];
        let text = roster_items_to_text(agent_roster_items(&rows, 0, &theme), 80, 20);
        assert!(text.contains("● 3 agents idle"), "at-rest header: {text}");
    }

    #[test]
    fn roster_active_row_shows_glyph_label_step_and_elapsed() {
        let theme = TuiUiState::default().theme;
        let rows = vec![roster_row(
            "explorer",
            "Explorer",
            0,
            ActivityState::Active,
            Some("exploring options"),
            Some("45s"),
        )];
        let text = roster_items_to_text(agent_roster_items(&rows, 0, &theme), 80, 20);
        assert!(text.contains('◐'), "active glyph (frame 0): {text}");
        assert!(text.contains("working"));
        assert!(text.contains("Explorer"));
        assert!(text.contains("exploring options"));
        assert!(text.contains("45s"));
    }

    #[test]
    fn roster_idle_row_has_no_activity_glyph_or_label() {
        let theme = TuiUiState::default().theme;
        let rows = vec![roster_row(
            "planner",
            "Planner",
            0,
            ActivityState::Idle,
            None,
            None,
        )];
        let text = roster_items_to_text(agent_roster_items(&rows, 0, &theme), 80, 20);
        assert!(text.contains("Planner"));
        assert!(
            !text.contains("working"),
            "idle row shows no activity label"
        );
        assert!(!text.contains('◐'), "idle row shows no active glyph");
    }

    #[test]
    fn roster_stalled_row_shows_frozen_glyph_and_elapsed() {
        let theme = TuiUiState::default().theme;
        let rows = vec![roster_row(
            "explorer",
            "Explorer",
            0,
            ActivityState::Stalled,
            None,
            Some("34s"),
        )];
        let text = roster_items_to_text(agent_roster_items(&rows, 0, &theme), 80, 20);
        assert!(text.contains('○'), "stalled glyph: {text}");
        assert!(text.contains("stalled?"));
        assert!(text.contains("34s"));
    }

    #[test]
    fn roster_render_is_deterministic_when_idle() {
        // Pure render of a clock-free row vec: repeated frames are byte-identical
        // with no `now` advance (no spinner, no elapsed tick).
        let mut state = state_with_input("", false);
        state.agents = vec![agent_view("explorer", "Explorer", "idle", &[])];
        state.roster_rows = vec![roster_row(
            "explorer",
            "Explorer",
            0,
            ActivityState::Idle,
            None,
            None,
        )];
        let a = render_to_text(&state, 100, 24);
        let b = render_to_text(&state, 100, 24);
        let c = render_to_text(&state, 100, 24);
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn roster_renders_glyph_at_narrow_width_without_breakage() {
        let mut state = state_with_input("", false);
        state.run_state = RunState::Running;
        state.agents = vec![agent_view("explorer", "Explorer", "running", &[])];
        state.roster_rows = vec![roster_row(
            "explorer",
            "Explorer",
            0,
            ActivityState::Active,
            Some("a-very-long-step-label-that-overflows-the-sidebar"),
            Some("9s"),
        )];
        // The List clips overflow to the ~28% sidebar; the row stays legible and
        // the frame renders without panicking or breaking layout.
        let text = render_to_text(&state, 40, 24);
        assert!(
            text.contains('◐'),
            "active glyph survives narrow width: {text}"
        );
    }

    #[test]
    fn roster_no_color_states_legible_by_glyph_and_label() {
        // Under NO_COLOR every state must read from glyph + label alone.
        let mut state = state_with_input("", false);
        state.run_state = RunState::Running;
        state.roster_rows = vec![
            roster_row("a", "Aida", 0, ActivityState::Active, None, None),
            roster_row("b", "Bram", 1, ActivityState::NeedsInput, None, None),
            roster_row("c", "Cody", 2, ActivityState::Stalled, None, None),
        ];
        let no_color_ui = TuiUiState {
            theme: Theme::resolve(TerminalCaps {
                no_color: true,
                truecolor: false,
            }),
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &no_color_ui, 100, 24);
        assert!(text.contains('◐') && text.contains("working"));
        assert!(text.contains('◔') && text.contains("waiting"));
        assert!(text.contains('○') && text.contains("stalled?"));
    }

    #[test]
    fn ctrl_l_roster_render_shows_each_agent_after_extraction() {
        let mut state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Running,
            active_run_id: Some("run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: vec![
                AgentView {
                    runtime: "codex".to_string(),
                    ..agent_view("fixer", "Fixer", "running", &["read"])
                },
                AgentView {
                    runtime: "claude".to_string(),
                    ..unavailable_agent_view()
                },
            ],
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: String::new(),
            git_context: None,
            recoverable_session: false,
        };
        populate_roster_rows(&mut state);
        let ui_state = TuiUiState {
            roster_visible: true,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 24);
        assert!(text.contains("Agent Roster"));
        // The roster view-model carries name + runtime/model (availability is no
        // longer a roster field — the rewrite shows activity instead, ADR-003).
        assert!(text.contains("Fixer"));
        assert!(text.contains("codex/default"));
        assert!(text.contains("claude/default"));
    }

    #[test]
    fn renders_empty_tui_surfaces() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Idle,
            active_run_id: None,
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: String::new(),
            git_context: None,
            recoverable_session: false,
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Agent Roster"));
        assert!(text.contains("Chat"));
        assert!(text.contains(">"));
        assert!(!text.contains("Input Composer"));
        assert!(text.contains("No chat yet."));
    }

    #[test]
    fn renders_agent_availability_events_and_input() {
        let mut state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Running,
            active_run_id: Some("run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: vec![AgentView {
                id: "fixer".to_string(),
                name: "Fixer".to_string(),
                runtime: "codex".to_string(),
                model: "default".to_string(),
                effort: "high".to_string(),
                thinking: false,
                capabilities: vec!["read".to_string(), "edit".to_string()],
                availability: Some(RuntimeAvailability {
                    runtime_id: "codex".to_string(),
                    status: RuntimeAvailabilityStatus::Unavailable,
                    message: "missing command".to_string(),
                    remediation: None,
                }),
                status: "running".to_string(),
            }],
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec![
                "Run started.".to_string(),
                "Fixer step started.".to_string(),
            ],
            input: "follow up".to_string(),
            git_context: None,
            recoverable_session: false,
        };
        populate_roster_rows(&mut state);
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Fixer"));
        assert!(text.contains("codex/default"));
        assert!(text.contains("effort:high"));
        // Availability ("down") is no longer a roster field (ADR-003 view-model).
        assert!(text.contains("Fixer step started."));
        assert!(text.contains("follow up"));
    }

    #[test]
    fn renders_work_indicator_below_input_while_run_is_active() {
        let mut state = state_with_input("next prompt", false);
        state.run_state = RunState::Running;
        state.active_run_id = Some("run".to_string());
        let mut ui_state = TuiUiState::default();

        let first_lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 100, 24);
        let second = render_to_text_with_ui_mut(&state, &mut ui_state, 100, 24);
        let first = first_lines.join("\n");
        let prompt_row = first_lines
            .iter()
            .position(|line| line.contains("> next prompt"))
            .unwrap();
        let working_row = first_lines
            .iter()
            .position(|line| line.contains("| Working"))
            .unwrap();

        assert!(first.contains("next prompt"));
        assert!(first.contains("| Working"));
        assert!(second.contains("/ Working"));
        assert!(working_row > prompt_row);
        assert!(first_lines[working_row.saturating_sub(1)].contains("└"));
        assert!(!first_lines[working_row].contains("│"));
        assert!(first_lines[working_row].trim_end().ends_with("/help"));
    }

    #[test]
    fn hides_work_indicator_when_run_is_idle() {
        let state = state_with_input("", false);

        let text = render_to_text(&state, 100, 24);

        assert!(!text.contains("Working"));
        assert!(text.contains("/help"));
    }

    #[test]
    fn input_area_height_is_stable_between_idle_and_running() {
        let idle = state_with_input("stable", false);
        let mut running = state_with_input("stable", false);
        running.run_state = RunState::Running;
        running.active_run_id = Some("run".to_string());
        let mut idle_ui = TuiUiState::default();
        let mut running_ui = TuiUiState::default();
        let idle_lines = render_to_lines_with_ui_mut(&idle, &mut idle_ui, 100, 24);
        let running_lines = render_to_lines_with_ui_mut(&running, &mut running_ui, 100, 24);
        let idle_prompt_row = idle_lines
            .iter()
            .position(|line| line.contains("> stable"))
            .unwrap();
        let running_prompt_row = running_lines
            .iter()
            .position(|line| line.contains("> stable"))
            .unwrap();
        let idle_border_row = idle_lines
            .iter()
            .enumerate()
            .skip(idle_prompt_row)
            .find_map(|(index, line)| line.contains("└").then_some(index))
            .unwrap();
        let running_border_row = running_lines
            .iter()
            .enumerate()
            .skip(running_prompt_row)
            .find_map(|(index, line)| line.contains("└").then_some(index))
            .unwrap();

        assert_eq!(
            idle_border_row.saturating_sub(idle_prompt_row),
            running_border_row.saturating_sub(running_prompt_row)
        );
    }

    #[test]
    fn roster_displays_streaming_status_as_running() {
        let mut state = state_with_input("", false);
        state.agents = vec![AgentView {
            id: "fixer".to_string(),
            name: "Fixer".to_string(),
            runtime: "fake".to_string(),
            model: "default".to_string(),
            effort: "medium".to_string(),
            thinking: false,
            capabilities: vec!["read".to_string()],
            availability: None,
            status: "streaming".to_string(),
        }];
        populate_roster_rows(&mut state);

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("Fixer running"));
        assert!(!text.contains("Fixer streaming"));
    }

    #[test]
    fn renders_live_step_stream_detail_as_chat_progress() {
        let mut state = state_with_input("", false);
        let live_step = LiveStepView {
            run_id: "run".to_string(),
            group_id: None,
            step_id: "step".to_string(),
            step_label: None,
            file_scope: None,
            agent: "fixer".to_string(),
            status: LiveStepStatus::Streaming,
            streams: vec![LiveStreamView {
                stream: "stdout".to_string(),
                content: "compiling target".to_string(),
                sequence_end: 1,
                final_delta: false,
            }],
        };
        state.live_step = Some(live_step.clone());
        state.live_steps = vec![live_step];
        state.events = vec!["Fixer step started.".to_string()];
        let mut projection = ChatProjection::new();
        projection.apply_live_steps(&state.live_steps);
        state.chat_items = projection.items().to_vec();

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("fixer is running"));
        assert!(text.contains("[stdout:live:#1] compiling target"));
        assert!(!text.contains("Fixer step started."));
    }

    #[test]
    fn renders_live_step_running_state_before_stream_content() {
        let mut state = state_with_input("", false);
        let live_step = LiveStepView {
            run_id: "run".to_string(),
            group_id: None,
            step_id: "step".to_string(),
            step_label: None,
            file_scope: None,
            agent: "fixer".to_string(),
            status: LiveStepStatus::Running,
            streams: Vec::new(),
        };
        state.live_step = Some(live_step.clone());
        state.live_steps = vec![live_step];
        let mut projection = ChatProjection::new();
        projection.apply_live_steps(&state.live_steps);
        state.chat_items = projection.items().to_vec();

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("fixer is running"));
        assert!(text.contains("runtime is running"));
    }

    #[test]
    fn omits_config_status_from_input_prompt_at_80x24_and_120x40() {
        let mut state = state_with_input("", false);
        state.config_status = ConfigStatusView {
            summary: "Config: sources=2 preset=research warnings=1".to_string(),
            sources: vec![
                "/home/user/.config/.atelier/atelier.toml".to_string(),
                "atelier.toml".to_string(),
            ],
            preset: Some("research".to_string()),
            warnings: vec!["enabled agents without model_fallbacks: explorer".to_string()],
            approval_mode: crate::config::ApprovalMode::Yolo,
            execution_graph_enabled: false,
            max_parallel_agent_steps: 2,
        };

        let small = render_to_text(&state, 80, 24);
        let large = render_to_text(&state, 120, 40);

        assert!(small.contains(">"));
        assert!(large.contains(">"));
        assert!(!small.contains("Config: sources=2 preset=research warnings=1"));
        assert!(!large.contains("Config: sources=2 preset=research warnings=1"));
    }

    #[test]
    fn renders_user_prompt_events_with_message_text() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Running,
            active_run_id: Some("run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["You: build a feature".to_string()],
            input: String::new(),
            git_context: None,
            recoverable_session: false,
        };

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("You"));
        assert!(text.contains("build a feature"));
    }

    #[test]
    fn renders_typed_user_prompt_as_dedicated_prompt_row() {
        let mut state = state_with_input("", false);
        let mut projection = ChatProjection::new();
        projection.apply_history_event(&HistoryEvent {
            schema_version: 1,
            event_id: "event-prompt".to_string(),
            session_id: "session".to_string(),
            run_id: Some("run".to_string()),
            group_id: None,
            graph_id: None,
            step_id: None,
            timestamp: "2026-06-05T00:00:00.000Z".to_string(),
            kind: "prompt_submitted".to_string(),
            payload: json!({ "prompt": "build a feature" }),
            payload_truncated: false,
        });
        state.chat_items = projection.items().to_vec();

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("You"));
        assert!(text.contains("build a feature"));
        assert!(!text.contains("User prompt"));
        assert!(!text.contains("completed"));
    }

    #[test]
    fn render_places_cursor_at_end_of_input() {
        let state = state_with_input("edit", false);
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(7, 19));
    }

    #[test]
    fn long_input_wraps_and_moves_cursor_to_wrapped_row() {
        let state = state_with_input("abcdefghijklmnopqrstuvwxyz1234", false);
        let backend = TestBackend::new(24, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();

        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("> abcdefghijklmnopqrst"));
        assert!(text.contains("  uvwxyz1234"));
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(13, 8));
    }

    #[test]
    fn renders_pending_approval_prompt() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::WaitingForUser,
            active_run_id: Some("run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: Some(crate::app::PendingApprovalView {
                run_id: "run".to_string(),
                group_id: None,
                step_id: "step".to_string(),
                action_id: "action".to_string(),
                agent: "fixer".to_string(),
                summary: "Action requires action approval.".to_string(),
                diagnostic: Some("command requires action approval: cargo install x".to_string()),
                ..Default::default()
            }),
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["Action approval required.".to_string()],
            input: String::new(),
            git_context: None,
            recoverable_session: false,
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Approval required for fixer"));
        assert!(text.contains("command requires action approval"));
        // No explainer when the show-once latch is already set.
        assert!(!text.contains(crate::app::chat::FIRST_APPROVAL_EXPLAINER));
    }

    fn fallback_approval_state(show_first_approval_explainer: bool) -> AppState {
        AppState {
            session_id: "session".to_string(),
            run_state: RunState::WaitingForUser,
            active_run_id: Some("run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: Some(crate::app::PendingApprovalView {
                run_id: "run".to_string(),
                group_id: None,
                step_id: "step".to_string(),
                action_id: "action".to_string(),
                agent: "fixer".to_string(),
                summary: "Action requires action approval.".to_string(),
                diagnostic: Some("command requires action approval: cargo install x".to_string()),
                ..Default::default()
            }),
            show_first_approval_explainer,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            // Empty so the pending-approval fallback render path is exercised.
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["Action approval required.".to_string()],
            input: String::new(),
            git_context: None,
            recoverable_session: false,
        }
    }

    #[test]
    fn renders_first_approval_explainer_when_flag_set() {
        let state = fallback_approval_state(true);
        // Wide enough that the single explainer line is not wrapped by the
        // paragraph renderer, so the full text is contiguous in the output.
        let text = render_to_text(&state, 260, 24);
        // The explainer line shows alongside the (unchanged) approval prompt.
        assert!(text.contains(crate::app::chat::FIRST_APPROVAL_EXPLAINER));
        assert!(text.contains("Approval required for fixer"));
    }

    #[test]
    fn suppresses_first_approval_explainer_when_flag_unset() {
        let state = fallback_approval_state(false);
        let text = render_to_text(&state, 100, 24);
        assert!(!text.contains(crate::app::chat::FIRST_APPROVAL_EXPLAINER));
        assert!(text.contains("Approval required for fixer"));
    }

    // ---- Rich approval modal & resolution key routing (task_07) -------

    fn approval_view(
        tier: Option<crate::actions::RiskTier>,
        catastrophic: bool,
        trust_target: Option<crate::actions::TrustTarget>,
    ) -> PendingApprovalView {
        PendingApprovalView {
            agent: "fixer".to_string(),
            reason: Some("installs software".to_string()),
            resolved_command: Some("npm install left-pad".to_string()),
            tier,
            catastrophic,
            trust_target,
            ..Default::default()
        }
    }

    fn modal_text(view: &PendingApprovalView, no_color: bool) -> String {
        let theme = Theme::resolve(TerminalCaps {
            no_color,
            truecolor: !no_color,
        });
        approval_modal_lines(view, &theme, false)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn medium_tier_modal_shows_label_reason_and_command() {
        let view = approval_view(Some(crate::actions::RiskTier::Medium), false, None);
        let text = modal_text(&view, false);
        assert!(text.contains("MEDIUM"));
        assert!(text.contains("installs software"));
        assert!(text.contains("npm install left-pad"));
    }

    #[test]
    fn trust_option_visible_only_when_target_present() {
        let with_trust = approval_view(
            Some(crate::actions::RiskTier::Medium),
            false,
            Some(crate::actions::TrustTarget::Command(
                "npm install left-pad".to_string(),
            )),
        );
        assert!(modal_text(&with_trust, false).contains("approve & trust"));

        let without = approval_view(Some(crate::actions::RiskTier::Medium), false, None);
        assert!(!modal_text(&without, false).contains("approve & trust"));
    }

    #[test]
    fn high_tier_requires_explicit_approve_word() {
        let view = approval_view(Some(crate::actions::RiskTier::High), false, None);
        // A reflexive "y" / empty Enter must not approve a High-tier action.
        assert_eq!(
            parse_approval_resolution("y", &view),
            ApprovalResolution::Deny
        );
        assert_eq!(
            parse_approval_resolution("", &view),
            ApprovalResolution::Deny
        );
        assert_eq!(
            parse_approval_resolution("approve", &view),
            ApprovalResolution::ApproveOnce
        );
    }

    #[test]
    fn catastrophic_requires_type_to_confirm() {
        let view = approval_view(Some(crate::actions::RiskTier::High), true, None);
        assert_eq!(
            parse_approval_resolution("y", &view),
            ApprovalResolution::Deny
        );
        assert_eq!(
            parse_approval_resolution("approve", &view),
            ApprovalResolution::Deny
        );
        // Only retyping the exact resolved command confirms.
        assert_eq!(
            parse_approval_resolution("npm install left-pad", &view),
            ApprovalResolution::ApproveOnce
        );
    }

    #[test]
    fn catastrophic_cannot_be_trusted_away() {
        // Even if a catastrophic view carried a trust_target, the `t`/`trust` shortcut
        // must NOT bypass type-to-confirm (ADR-001/002: catastrophic is never trusted).
        let view = approval_view(
            Some(crate::actions::RiskTier::High),
            true,
            Some(crate::actions::TrustTarget::Command(
                "npm install left-pad".to_string(),
            )),
        );
        assert_eq!(
            parse_approval_resolution("t", &view),
            ApprovalResolution::Deny
        );
        assert_eq!(
            parse_approval_resolution("trust", &view),
            ApprovalResolution::Deny
        );
        // Retyping the exact resolved command is still the only way through.
        assert_eq!(
            parse_approval_resolution("npm install left-pad", &view),
            ApprovalResolution::ApproveOnce
        );
    }

    #[test]
    fn resolution_keys_map_to_distinct_outcomes() {
        let view = approval_view(
            Some(crate::actions::RiskTier::Medium),
            false,
            Some(crate::actions::TrustTarget::Command(
                "npm install left-pad".to_string(),
            )),
        );
        assert_eq!(
            parse_approval_resolution("y", &view),
            ApprovalResolution::ApproveOnce
        );
        assert_eq!(
            parse_approval_resolution("t", &view),
            ApprovalResolution::ApproveAndTrust
        );
        assert_eq!(
            parse_approval_resolution("n", &view),
            ApprovalResolution::Deny
        );
    }

    #[test]
    fn tier_label_survives_no_color() {
        let view = approval_view(Some(crate::actions::RiskTier::Medium), false, None);
        // Under NO_COLOR the tier must still be conveyed by its text label.
        assert!(modal_text(&view, true).contains("MEDIUM"));
    }

    #[test]
    fn approvals_help_tab_documents_tiers_trust_and_floor() {
        let theme = Theme::resolve(TerminalCaps {
            no_color: false,
            truecolor: true,
        });
        let text = help_tab_text(&approvals_tab_lines(&theme));
        assert!(text.contains("/trust"), "missing /trust: {text}");
        assert!(text.contains("trust"));
        assert!(text.contains("floor"), "missing floor posture");
        assert!(text.contains("catastrophic"));
        assert!(text.contains("tier"), "missing risk tiers");
    }

    #[test]
    fn keys_help_tab_lists_approval_resolution_keys() {
        let theme = Theme::resolve(TerminalCaps {
            no_color: false,
            truecolor: true,
        });
        // Approval-resolution keys are non-rebindable, so they live in the Keys tab's
        // fixed-keys section.
        let text = help_tab_text(&keys_tab_lines(&default_keymap(), &theme));
        assert!(text.contains("approve"));
        assert!(text.contains("trust"));
        assert!(text.contains("deny"));
    }

    #[test]
    fn high_tier_enter_routing_denies_then_dedicated_word_approves() {
        // Integration through the key router: a High-tier pending approval is not
        // approved by Enter+"y", but is by Enter+"approve".
        let mut state = state_with_input("y", true);
        if let Some(view) = state.pending_approval.as_mut() {
            view.tier = Some(crate::actions::RiskTier::High);
            view.reason = Some("matches a high-risk pattern".to_string());
            view.resolved_command = Some("sudo rm file".to_string());
        }
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            key_event_to_tui_command(&state, enter),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(
                ApprovalResolution::Deny
            )))
        );

        state.input = "approve".to_string();
        assert_eq!(
            key_event_to_tui_command(&state, enter),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(
                ApprovalResolution::ApproveOnce
            )))
        );
    }

    #[test]
    fn enter_key_submits_current_input_as_app_event() {
        let state = state_with_input("build this", false);

        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "build this".to_string(),
                PromptSource::Fresh
            )))
        );
    }

    #[tokio::test]
    async fn prompt_submission_is_queued_for_app_worker_and_clears_local_input() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("slow prompt", false);
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        let keep_running = execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "slow prompt".to_string(),
                PromptSource::Fresh,
            )),
        )
        .await
        .unwrap();

        assert!(keep_running);
        assert!(state.input.is_empty());
        assert_eq!(ui_state.input_cursor, 0);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(prompt, _)) if prompt == "slow prompt"
        ));
    }

    #[tokio::test]
    async fn governance_decision_resolution_clears_the_redirect_input() {
        // The redirect composer reads state.input; resolving the decision consumes it,
        // so the composer must reset (no stale redirect text left behind).
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("please redirect to X", false);
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        let event = AppEvent::GovernanceDecisionResolved(
            "gov-1".to_string(),
            GovernanceAnswer::Reject {
                redirect: Some("please redirect to X".to_string()),
            },
        );
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::Dispatch(event),
        )
        .await
        .unwrap();

        assert!(state.input.is_empty(), "redirect input should be cleared");
        assert_eq!(ui_state.input_cursor, 0);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::GovernanceDecisionResolved(_, _))
        ));
    }

    #[tokio::test]
    async fn help_command_toggles_modal_without_app_event() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/help", false);
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::ToggleHelp)
        );

        let keep_running =
            execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::ToggleHelp)
                .await
                .unwrap();

        assert!(keep_running);
        assert!(state.input.is_empty());
        assert_eq!(ui_state.input_cursor, 0);
        assert!(ui_state.help_visible);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn help_active_tab_defaults_to_getting_started() {
        assert_eq!(
            TuiUiState::default().help_active_tab,
            HelpTab::GettingStarted
        );
    }

    #[tokio::test]
    async fn toggle_help_close_resets_active_tab() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Cli,
            ..TuiUiState::default()
        };

        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::ToggleHelp)
            .await
            .unwrap();

        assert!(!ui_state.help_visible);
        assert_eq!(ui_state.help_active_tab, HelpTab::GettingStarted);
    }

    #[tokio::test]
    async fn toggle_help_open_preserves_default_active_tab() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState::default();
        assert!(!ui_state.help_visible);

        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::ToggleHelp)
            .await
            .unwrap();

        // Opening flips visibility and leaves the active tab at the default.
        assert!(ui_state.help_visible);
        assert_eq!(ui_state.help_active_tab, HelpTab::GettingStarted);
    }

    #[test]
    fn reload_skills_command_is_local_tui_command() {
        let state = state_with_input(RELOAD_SKILLS_COMMAND, false);

        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::ReloadSkills)
        );
    }

    #[tokio::test]
    async fn reload_skills_command_refreshes_cache_and_clears_input() {
        let dir = tempdir().unwrap();
        let project_agents = dir.path().join(".agents/skills");
        write_skill(&project_agents, "fresh-skill", "fresh-skill");
        let roots = skills::skill_roots(dir.path());
        let fingerprint = skill_file_fingerprints(&roots);
        let stale_suggestion =
            test_skill_suggestion("stale-skill", SkillSourceTag::Project, ".agents/skills");
        write_skill_suggestion_cache(
            dir.path(),
            &fingerprint,
            std::slice::from_ref(&stale_suggestion),
        )
        .unwrap();
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input(RELOAD_SKILLS_COMMAND, false);
        let mut ui_state = TuiUiState {
            working_directory: Some(dir.path().to_path_buf()),
            input_cursor: input_char_count(RELOAD_SKILLS_COMMAND),
            skill_suggestions: vec![stale_suggestion],
            ..TuiUiState::default()
        };

        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::ReloadSkills)
            .await
            .unwrap();

        assert!(state.input.is_empty());
        assert_eq!(ui_state.input_cursor, 0);
        assert!(ui_state
            .skill_suggestions
            .iter()
            .any(|skill| skill.alias == "fresh-skill"));
        assert!(!ui_state
            .skill_suggestions
            .iter()
            .any(|skill| skill.alias == "stale-skill"));
        assert_eq!(
            ui_state.status_message,
            Some(format!(
                "Skills reloaded: {}",
                ui_state.skill_suggestions.len()
            ))
        );
        assert_eq!(ui_state.skill_selection_index, 0);
        assert!(receiver.try_recv().is_err());

        let cached = read_cached_skill_suggestions(dir.path(), &fingerprint).unwrap();
        assert!(cached.iter().any(|skill| skill.alias == "fresh-skill"));
        assert!(!cached.iter().any(|skill| skill.alias == "stale-skill"));
    }

    #[test]
    fn renders_help_modal_commands() {
        let state = state_with_input("", false);
        // Tabbed help renders one tab at a time, so each assertion group is routed
        // to its tab. The modal frame + Commands content render on the Commands tab.
        let commands_ui = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &commands_ui, 120, 32);

        assert!(text.contains("Help"));
        let header = text.lines().find(|line| line.contains("Help")).unwrap();
        assert!(header.contains("Esc"));
        assert!(text.contains("/help"));
        assert!(text.contains("toggle the help overlay"));
        assert!(text.contains("/agent:<agent_name>"));
        assert!(text.contains("/skill:<skill_name>"));
        assert!(text.contains("load skill context"));
        assert!(!text.contains("prefix prompt with skill name"));
        assert!(text.contains("/reload:skills"));
        assert!(text.contains("enabled agent"));
        assert!(text.contains("/goal <text>"));
        assert!(text.contains("/goal clear"));
        assert!(text.contains("/subtask <agent>"));
        assert!(text.contains("/workflow <prompt>"));
        assert!(text.contains("execute a broad prompt with workflow evidence"));
        assert!(text.contains("/config"));
        assert!(!text.contains("close this help"));

        // Keybinding rows live on the Keys tab.
        let keys_ui = TuiUiState {
            help_active_tab: HelpTab::Keys,
            ..commands_ui.clone()
        };
        let keys_text = render_to_text_with_ui(&state, &keys_ui, 120, 32);
        // Keys tab is data-driven from the keymap (canonical lowercase via format_key).
        assert!(keys_text.contains("mouse wheel"));
        assert!(keys_text.contains("ctrl+l"));
        assert!(keys_text.contains("arrows"));
        assert!(keys_text.contains("pageup"));
        assert!(keys_text.contains("home"));

        // CLI flag rows live on the CLI tab.
        let cli_ui = TuiUiState {
            help_active_tab: HelpTab::Cli,
            ..commands_ui
        };
        let cli_text = render_to_text_with_ui(&state, &cli_ui, 120, 32);
        assert!(cli_text.contains("atelier --doctor"));
        assert!(cli_text.contains("atelier --clean-sessions"));
    }

    #[test]
    fn help_modal_command_rows_are_catalog_derived() {
        let state = state_with_input("", false);
        // Catalog-derived contract lives on the Commands tab.
        let ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 40);

        // Every fixed V1 command's usage and description renders exactly once,
        // proving the rows come from the catalog and are not duplicated.
        for spec in crate::slash_commands::catalog() {
            assert!(
                text.contains(spec.usage),
                "help missing usage {}",
                spec.usage
            );
            let occurrences = text.matches(spec.description).count();
            assert_eq!(
                occurrences, 1,
                "description {:?} rendered {occurrences} times",
                spec.description
            );
        }
        // The amended catalog commands are visible.
        assert!(text.contains("/reload:skills"));
        assert!(text.contains("/workflow <prompt>"));
        assert!(text.contains("/queue <message>"));

        // Non-command rows survive on the Keys tab (catalog routing did not drop
        // the literal keybinding rows).
        let keys_ui = TuiUiState {
            help_active_tab: HelpTab::Keys,
            ..ui_state
        };
        let keys_text = render_to_text_with_ui(&state, &keys_ui, 120, 40);
        assert!(keys_text.contains("ctrl+l"));
        assert!(keys_text.contains("arrows"));
        assert!(keys_text.contains("mouse wheel"));
    }

    #[test]
    fn readme_skill_command_wording_matches_help_language() {
        let state = state_with_input("", false);
        // `/skill:` wording lives on the Commands tab.
        let ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };
        let help_text = render_to_text_with_ui(&state, &ui_state, 120, 32);
        let readme = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"));

        assert!(help_text.contains("/skill:<skill_name>"));
        assert!(readme.contains("`/skill:<skill_name>`"));
        assert!(help_text.contains("load skill context"));
        assert!(readme.contains("load skill context"));
        assert!(!readme.contains("prefix a prompt with a selected skill"));
        assert!(!readme.contains("prefix prompt with skill name"));
    }

    #[test]
    fn readme_workflow_command_wording_matches_v1_limits() {
        let state = state_with_input("", false);
        // `/workflow` wording lives on the Commands tab.
        let ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };
        let help_text = render_to_text_with_ui(&state, &ui_state, 120, 32);
        let readme = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"));

        assert!(help_text.contains("/workflow <prompt>"));
        assert!(readme.contains("`/workflow <prompt>`"));
        assert!(readme.contains("one normal run"));
        assert!(readme.contains("plan, child-outcome, verification, and risk evidence"));
        assert!(readme.contains(
            "V1 does not support saved workflow scripts, worktree-isolated workflow children, or background workflow execution"
        ));
        assert!(!readme.contains("supports saved workflow"));
        assert!(!readme.contains("supports worktree-isolated workflow"));
        assert!(!readme.contains("supports background workflow"));
    }

    #[test]
    fn renders_agent_dropdown_for_agent_prefix() {
        let state = state_with_agent_roster("/agent:");
        let ui_state = TuiUiState {
            roster_visible: false,
            input_cursor: input_char_count("/agent:"),
            ..TuiUiState::default()
        };

        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(text.contains("Agents"));
        assert!(text.contains("explorer"));
        assert!(text.contains("fixer"));
        assert!(text.contains("Explorer"));
        assert!(text.contains("fake/default read,edit,verify"));
        assert!(!text.contains("archived"));
    }

    #[test]
    fn renders_agent_dropdown_for_mid_prompt_agent_token() {
        let state = state_with_agent_roster("please use /agent:fi then inspect");
        let ui_state = TuiUiState {
            roster_visible: false,
            input_cursor: input_char_count("please use /agent:fi"),
            ..TuiUiState::default()
        };

        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(text.contains("Agents"));
        assert!(text.contains("fixer"));
        assert!(!text.contains("explorer"));
    }

    #[test]
    fn agent_dropdown_filters_by_typed_agent_query() {
        let state = state_with_agent_roster("/agent:fix");
        let ui_state = TuiUiState {
            roster_visible: false,
            input_cursor: input_char_count("/agent:fix"),
            ..TuiUiState::default()
        };

        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(text.contains("fixer"));
        assert!(!text.contains("explorer"));
    }

    #[test]
    fn agent_dropdown_arrow_keys_override_wrapped_input_cursor_movement() {
        let state = state_with_agent_roster("/agent:");
        let ui_state = ui_state_with_cursor_at_end(&state.input);

        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
            ),
            Some(TuiCommand::AgentDropdown(DropdownCommand::Previous))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
            ),
            Some(TuiCommand::AgentDropdown(DropdownCommand::Next))
        );
    }

    #[tokio::test]
    async fn agent_dropdown_selection_cycles_without_app_event() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_agent_roster("/agent:");
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::AgentDropdown(DropdownCommand::Next),
        )
        .await
        .unwrap();

        assert_eq!(ui_state.agent_selection_index, 1);
        assert_eq!(ui_state.input_cursor, input_char_count("/agent:"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn agent_dropdown_accept_replaces_query_and_preserves_prompt_rest() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_agent_roster("/agent:fi inspect docs");
        let mut ui_state = TuiUiState {
            input_cursor: input_char_count("/agent:fi"),
            ..TuiUiState::default()
        };

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::AgentDropdown(DropdownCommand::Accept),
        )
        .await
        .unwrap();

        assert_eq!(state.input, "/agent:fixer inspect docs");
        assert_eq!(ui_state.input_cursor, input_char_count("/agent:fixer "));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn agent_dropdown_accept_replaces_mid_prompt_query_and_preserves_rest() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_agent_roster("please use /agent:fi then inspect");
        let mut ui_state = TuiUiState {
            input_cursor: input_char_count("please use /agent:fi"),
            ..TuiUiState::default()
        };

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::AgentDropdown(DropdownCommand::Accept),
        )
        .await
        .unwrap();

        assert_eq!(state.input, "please use /agent:fixer then inspect");
        assert_eq!(
            ui_state.input_cursor,
            input_char_count("please use /agent:fixer ")
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn renders_skill_dropdown_for_skill_prefix() {
        let state = state_with_input("/skill:", false);
        let ui_state = ui_state_with_skills_at_end(&state.input);

        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(text.contains("Skills"));
        assert!(text.contains("project-alpha"));
        assert!(text.contains("personal-beta"));
        assert!(text.contains("Project"));
        assert!(text.contains("Personal"));
    }

    #[test]
    fn renders_skill_dropdown_for_mid_prompt_skill_token() {
        let state = state_with_input("test 123 /skill:personal then inspect", false);
        let ui_state = TuiUiState {
            input_cursor: input_char_count("test 123 /skill:personal"),
            skill_suggestions: test_skill_suggestions(),
            ..TuiUiState::default()
        };

        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(text.contains("Skills"));
        assert!(text.contains("personal-beta"));
        assert!(!text.contains("project-alpha"));
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SkillDropdown(DropdownCommand::Next))
        );
    }

    #[test]
    fn skill_dropdown_renders_tag_at_row_end() {
        let state = state_with_input("/skill:", false);
        let mut ui_state = ui_state_with_skills_at_end(&state.input);

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 100, 24);
        let row = lines
            .iter()
            .find(|line| line.contains("project-alpha"))
            .unwrap();
        let origin_col = char_column(row, ".agents/skills");
        let tag_col = char_column(row, "Project");
        let right_border_col = row
            .chars()
            .enumerate()
            .filter_map(|(index, ch)| (ch == '│').then_some(index))
            .last()
            .unwrap();

        assert!(tag_col > origin_col);
        assert!(right_border_col.saturating_sub(tag_col + "Project".len()) <= 2);
    }

    #[test]
    fn skill_dropdown_limits_visible_rows_and_truncates_narrow_rows() {
        let state = state_with_input("/skill:", false);
        let skill_suggestions = (0..8)
            .map(|index| {
                test_skill_suggestion(
                    &format!("skill-{index}"),
                    SkillSourceTag::Project,
                    ".agents/skills",
                )
            })
            .collect::<Vec<_>>();
        let ui_state = TuiUiState {
            input_cursor: input_char_count("/skill:"),
            skill_suggestions,
            ..TuiUiState::default()
        };

        let mut ui_state_for_rows = ui_state.clone();
        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state_for_rows, 100, 24);
        let visible_skill_rows = lines.iter().filter(|line| line.contains("skill-")).count();

        assert_eq!(visible_skill_rows, DROPDOWN_MAX_ITEMS);

        let narrow_ui_state = TuiUiState {
            input_cursor: input_char_count("/skill:"),
            skill_suggestions: vec![test_skill_suggestion(
                "very-long-project-alpha-overflow",
                SkillSourceTag::Project,
                ".agents/skills/very-long-project-alpha-overflow",
            )],
            ..TuiUiState::default()
        };
        let narrow = render_to_text_with_ui(&state, &narrow_ui_state, 36, 24);

        assert!(narrow.contains("Skills"));
        assert!(narrow.contains("Project"));
        assert!(!narrow.contains("very-long-project-alpha-overflow"));
    }

    #[test]
    fn skill_dropdown_filters_by_typed_skill_query() {
        let state = state_with_input("/skill:personal", false);
        let ui_state = ui_state_with_skills_at_end(&state.input);

        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(text.contains("personal-beta"));
        assert!(!text.contains("project-alpha"));
    }

    #[test]
    fn skill_dropdown_arrow_keys_override_wrapped_input_cursor_movement() {
        let state = state_with_input("/skill:", false);
        let ui_state = ui_state_with_skills_at_end(&state.input);

        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SkillDropdown(DropdownCommand::Previous))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SkillDropdown(DropdownCommand::Next))
        );
    }

    #[tokio::test]
    async fn skill_dropdown_selection_cycles_without_app_event() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/skill:", false);
        let mut ui_state = ui_state_with_skills_at_end(&state.input);

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::SkillDropdown(DropdownCommand::Next),
        )
        .await
        .unwrap();

        assert_eq!(ui_state.skill_selection_index, 1);
        assert_eq!(ui_state.input_cursor, input_char_count("/skill:"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn skill_dropdown_accept_replaces_query_and_preserves_prompt_rest() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/skill:personal inspect docs", false);
        let mut ui_state = TuiUiState {
            input_cursor: input_char_count("/skill:personal"),
            skill_suggestions: test_skill_suggestions(),
            ..TuiUiState::default()
        };

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::SkillDropdown(DropdownCommand::Accept),
        )
        .await
        .unwrap();

        assert_eq!(state.input, "/skill:personal-beta inspect docs");
        assert_eq!(
            ui_state.input_cursor,
            input_char_count("/skill:personal-beta ")
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn skill_dropdown_accept_replaces_mid_prompt_query_and_preserves_rest() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("test 123 /skill:personal then inspect", false);
        let mut ui_state = TuiUiState {
            input_cursor: input_char_count("test 123 /skill:personal"),
            skill_suggestions: test_skill_suggestions(),
            ..TuiUiState::default()
        };

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::SkillDropdown(DropdownCommand::Accept),
        )
        .await
        .unwrap();

        assert_eq!(state.input, "test 123 /skill:personal-beta then inspect");
        assert_eq!(
            ui_state.input_cursor,
            input_char_count("test 123 /skill:personal-beta ")
        );
        assert!(receiver.try_recv().is_err());
    }

    // ── command dropdown model + activation (task_04) ──

    fn command_state(input: &str) -> (AppState, TuiUiState) {
        (
            state_with_input(input, false),
            ui_state_with_cursor_at_end(input),
        )
    }

    #[test]
    fn command_dropdown_lists_all_fixed_commands_for_slash() {
        let (state, ui_state) = command_state("/");
        let dropdown = command_dropdown(&state, &ui_state).expect("active for /");
        assert!(!dropdown.empty);
        assert_eq!(dropdown.selected, Some(0));
        let labels: Vec<&str> = dropdown.suggestions.iter().map(|spec| spec.label).collect();
        let catalog_labels: Vec<&str> = crate::slash_commands::catalog()
            .iter()
            .map(|spec| spec.label)
            .collect();
        assert_eq!(labels, catalog_labels);
    }

    #[test]
    fn command_dropdown_filters_goal_commands_for_slash_g() {
        let (state, ui_state) = command_state("/g");
        let dropdown = command_dropdown(&state, &ui_state).expect("active for /g");
        let labels: Vec<&str> = dropdown.suggestions.iter().map(|spec| spec.label).collect();
        assert_eq!(labels, ["/goal", "/goal clear"]);
    }

    #[test]
    fn command_dropdown_inactive_for_mid_prompt_slash() {
        let (state, ui_state) = command_state("please /g");
        assert!(command_dropdown(&state, &ui_state).is_none());
    }

    #[test]
    fn command_dropdown_models_no_match_without_selection() {
        let (state, ui_state) = command_state("/unknown");
        let dropdown = command_dropdown(&state, &ui_state).expect("active for /unknown");
        assert!(dropdown.empty);
        assert!(dropdown.suggestions.is_empty());
        assert_eq!(dropdown.selected, None);
    }

    #[test]
    fn command_dropdown_releases_once_args_begin() {
        // A space ends the command word, so argument-taking commands stay normal
        // input instead of being trapped by the no-match state.
        for input in [
            "/goal ",
            "/goal new objective",
            "/subtask fixer do it",
            "/queue ship it",
        ] {
            let (state, ui_state) = command_state(input);
            assert!(
                command_dropdown(&state, &ui_state).is_none(),
                "dropdown should be inactive for {input:?}"
            );
        }
    }

    #[test]
    fn command_dropdown_suppressed_during_pending_approval() {
        let mut state = state_with_input("/", true);
        // Isolate the pending-approval gate from the WaitingForUser gate.
        state.run_state = RunState::Idle;
        let ui_state = ui_state_with_cursor_at_end("/");
        assert!(state.pending_approval.is_some());
        assert!(command_dropdown(&state, &ui_state).is_none());
    }

    #[test]
    fn command_dropdown_suppressed_while_waiting_for_user() {
        let mut state = state_with_input("/", false);
        state.run_state = RunState::WaitingForUser;
        let ui_state = ui_state_with_cursor_at_end("/");
        assert!(state.pending_approval.is_none());
        assert!(command_dropdown(&state, &ui_state).is_none());
    }

    #[test]
    fn command_dropdown_dismissed_only_for_matching_input() {
        let (state, mut ui_state) = command_state("/g");
        ui_state.command_dropdown_dismissed = Some("/g".to_string());
        assert!(command_dropdown(&state, &ui_state).is_none());

        // The same dismissal does not suppress a different input.
        let (state2, mut ui2) = command_state("/go");
        ui2.command_dropdown_dismissed = Some("/g".to_string());
        assert!(command_dropdown(&state2, &ui2).is_some());
    }

    #[test]
    fn agent_and_skill_dropdowns_take_precedence_over_command_dropdown() {
        // /agent: with a roster resolves to the agent dropdown.
        let agent_state = state_with_agent_roster("/agent:");
        let agent_ui = ui_state_with_cursor_at_end("/agent:");
        assert!(agent_dropdown(&agent_state, &agent_ui).is_some());

        // /skill: with suggestions resolves to the skill dropdown.
        let skill_state = state_with_input("/skill:", false);
        let skill_ui = ui_state_with_skills_at_end("/skill:");
        assert!(skill_dropdown(&skill_state.input, &skill_ui).is_some());

        // /g resolves to neither specialized dropdown, so the command dropdown
        // is the one that activates.
        let (cmd_state, cmd_ui) = command_state("/g");
        assert!(agent_dropdown(&cmd_state, &cmd_ui).is_none());
        assert!(skill_dropdown(&cmd_state.input, &cmd_ui).is_none());
        assert!(command_dropdown(&cmd_state, &cmd_ui).is_some());
    }

    // ── command dropdown rendering + empty state (task_05) ──

    #[test]
    fn renders_command_dropdown_for_slash() {
        let state = state_with_input("/", false);
        let ui_state = ui_state_with_cursor_at_end("/");
        let text = render_to_text_with_ui(&state, &ui_state, 80, 24);
        assert!(text.contains("Commands"));
        assert!(text.contains("/help"));
        assert!(text.contains("/config"));
        assert!(text.contains("toggle the help overlay"));
    }

    #[test]
    fn renders_command_dropdown_filtered_for_slash_g() {
        let state = state_with_input("/g", false);
        let ui_state = ui_state_with_cursor_at_end("/g");
        let text = render_to_text_with_ui(&state, &ui_state, 80, 24);
        assert!(text.contains("Commands"));
        assert!(text.contains("/goal"));
        assert!(text.contains("clear the session goal"));
        // Non-matching commands are filtered out of the rendered rows.
        assert!(!text.contains("show config files, preset, warnings"));
    }

    #[test]
    fn renders_no_commands_found_for_unmatched_slash() {
        let state = state_with_input("/zz", false);
        let ui_state = ui_state_with_cursor_at_end("/zz");
        let text = render_to_text_with_ui(&state, &ui_state, 80, 24);
        assert!(text.contains("Commands"));
        assert!(text.contains("No commands found"));
    }

    #[test]
    fn command_dropdown_rows_truncate_to_input_width() {
        // At a narrow width the longest description is truncated, so command
        // rows never overflow the input area.
        let state = state_with_input("/", false);
        let mut ui_state = ui_state_with_cursor_at_end("/");
        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 30, 24);
        assert!(lines.iter().any(|line| line.contains("Commands")));
        assert!(lines.iter().all(|line| line.chars().count() <= 30));
        let text: String = lines.join("");
        assert!(!text.contains("execute a broad prompt with workflow evidence"));
    }

    #[test]
    fn help_modal_suppresses_command_dropdown_rendering() {
        let state = state_with_input("/", false);
        let ui_state = TuiUiState {
            input_cursor: input_char_count("/"),
            help_visible: true,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 80, 32);
        assert!(text.contains("Help"));
        // The tabbed help strip legitimately renders the "Commands" tab title, so
        // suppression is verified by the absence of the command dropdown's unique
        // right-aligned navigation hint rather than the bare word "Commands".
        assert!(!text.contains("Up/Down Tab/Enter"));
    }

    #[test]
    fn command_dropdown_yields_to_active_agent_dropdown_render() {
        // With a roster, /agent: resolves to the agent dropdown, not the command
        // dropdown — command rendering only follows once specialized dropdowns
        // are inactive.
        let state = state_with_agent_roster("/agent:");
        let ui_state = ui_state_with_cursor_at_end("/agent:");
        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);
        assert!(text.contains("Agents"));
        assert!(!text.contains("Commands"));
    }

    // ── command dropdown keyboard handling + insertion (task_06) ──

    #[test]
    fn command_dropdown_arrow_keys_route_to_selection() {
        let state = state_with_input("/g", false);
        let ui_state = ui_state_with_cursor_at_end("/g");
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
            ),
            Some(TuiCommand::CommandDropdown(
                CommandDropdownCommand::Previous
            ))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
            ),
            Some(TuiCommand::CommandDropdown(CommandDropdownCommand::Next))
        );
    }

    #[test]
    fn command_dropdown_selection_cycles_and_wraps() {
        // /g -> [/goal, /goal clear].
        let mut state = state_with_input("/g", false);
        let mut ui_state = ui_state_with_cursor_at_end("/g");
        apply_command_dropdown_command(&mut state, &mut ui_state, CommandDropdownCommand::Next);
        assert_eq!(ui_state.command_selection_index, 1);
        apply_command_dropdown_command(&mut state, &mut ui_state, CommandDropdownCommand::Next);
        assert_eq!(ui_state.command_selection_index, 0); // wraps forward
        apply_command_dropdown_command(&mut state, &mut ui_state, CommandDropdownCommand::Previous);
        assert_eq!(ui_state.command_selection_index, 1); // wraps back
    }

    #[test]
    fn command_dropdown_tab_and_enter_map_to_accept() {
        let state = state_with_input("/config", false);
        let ui_state = ui_state_with_cursor_at_end("/config");
        for code in [KeyCode::Tab, KeyCode::Enter] {
            assert_eq!(
                key_event_to_tui_command_with_ui(
                    &state,
                    &ui_state,
                    KeyEvent::new(code, KeyModifiers::NONE)
                ),
                Some(TuiCommand::CommandDropdown(CommandDropdownCommand::Accept)),
                "{code:?} should accept",
            );
        }
    }

    #[tokio::test]
    async fn command_dropdown_accept_inserts_text_without_app_event() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/config", false);
        let mut ui_state = ui_state_with_cursor_at_end("/config");
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/config");
        assert_eq!(ui_state.input_cursor, input_char_count("/config"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn command_dropdown_enter_accepts_help_without_toggling() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/help", false);
        let mut ui_state = ui_state_with_cursor_at_end("/help");
        // Enter is intercepted as Accept while the dropdown is open, not as the
        // /help toggle.
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(
            command,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept)
        );
        execute_tui_command(&mut state, &mut ui_state, &sender, command)
            .await
            .unwrap();
        assert_eq!(state.input, "/help");
        assert!(!ui_state.help_visible);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn command_dropdown_accept_goal_leaves_cursor_ready_for_text() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/goa", false);
        let mut ui_state = ui_state_with_cursor_at_end("/goa");
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/goal");
        assert_eq!(ui_state.input_cursor, input_char_count("/goal"));
        // Dismissed so a second Enter can submit / arguments can be typed,
        // instead of re-accepting the same row.
        assert!(command_dropdown(&state, &ui_state).is_none());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn command_dropdown_escape_dismisses_and_preserves_input() {
        let state = state_with_input("/g", false);
        let ui_state = ui_state_with_cursor_at_end("/g");
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            Some(TuiCommand::CommandDropdown(CommandDropdownCommand::Dismiss))
        );

        let mut state = state.clone();
        let mut ui_state = ui_state.clone();
        apply_command_dropdown_command(&mut state, &mut ui_state, CommandDropdownCommand::Dismiss);
        assert_eq!(state.input, "/g"); // raw input untouched
        assert_eq!(ui_state.command_dropdown_dismissed, Some("/g".to_string()));
        assert!(command_dropdown(&state, &ui_state).is_none());
    }

    #[tokio::test]
    async fn command_dropdown_no_match_enter_is_trapped() {
        let state = state_with_input("/zz", false);
        let ui_state = ui_state_with_cursor_at_end("/zz");
        // Enter on the no-match state maps to a trap, not a submit.
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(
            command,
            TuiCommand::CommandDropdown(CommandDropdownCommand::TrapNoMatch)
        );

        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state.clone();
        let mut ui_state = ui_state.clone();
        execute_tui_command(&mut state, &mut ui_state, &sender, command)
            .await
            .unwrap();
        assert_eq!(state.input, "/zz");
        assert!(receiver.try_recv().is_err()); // no PromptSubmitted dispatched
    }

    #[test]
    fn normal_enter_still_submits_without_command_dropdown() {
        let state = state_with_input("hello world", false);
        let ui_state = ui_state_with_cursor_at_end("hello world");
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "hello world".to_string(),
                PromptSource::Fresh
            )))
        );
    }

    #[test]
    fn enter_submits_command_with_arguments_normally() {
        // Once a space ends the command word the dropdown is inactive, so Enter
        // submits as usual — preserving `/goal <text>`, `/subtask`, `/queue`.
        let state = state_with_input("/goal ship v2", false);
        let ui_state = ui_state_with_cursor_at_end("/goal ship v2");
        assert!(command_dropdown(&state, &ui_state).is_none());
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "/goal ship v2".to_string(),
                PromptSource::Fresh
            )))
        );
    }

    // ── prefix handoff + final regression coverage (task_07) ──

    #[tokio::test]
    async fn accepting_agent_prefix_hands_off_to_agent_dropdown() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_agent_roster("/agent");
        let mut ui_state = ui_state_with_cursor_at_end("/agent");
        // The command dropdown offers /agent: for the partial input.
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();

        // Inserted text only, no trailing text, no app event.
        assert_eq!(state.input, "/agent:");
        assert!(!state.input.ends_with(' '));
        assert!(receiver.try_recv().is_err());
        // The agent dropdown takes over immediately and the command dropdown
        // yields to it in both the model and the render.
        assert!(agent_dropdown(&state, &ui_state).is_some());
        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);
        assert!(text.contains("Agents"));
        assert!(!text.contains("Commands"));
        // Filtering still works after handoff (no trailing text blocked it).
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputCharacter('f'),
        )
        .await
        .unwrap();
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputCharacter('i'),
        )
        .await
        .unwrap();
        let filtered = render_to_text_with_ui(&state, &ui_state, 100, 24);
        assert!(filtered.contains("fixer"));
        assert!(!filtered.contains("explorer"));
    }

    #[tokio::test]
    async fn accepting_skill_prefix_hands_off_to_skill_dropdown() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/skill", false);
        let mut ui_state = TuiUiState {
            input_cursor: input_char_count("/skill"),
            skill_suggestions: test_skill_suggestions(),
            ..TuiUiState::default()
        };
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();

        assert_eq!(state.input, "/skill:");
        assert!(!state.input.ends_with(' '));
        assert!(receiver.try_recv().is_err());
        assert!(skill_dropdown(&state.input, &ui_state).is_some());
        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);
        assert!(text.contains("Skills"));
        assert!(!text.contains("Commands"));
    }

    #[tokio::test]
    async fn goal_follow_on_text_submits_normally_after_acceptance() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/goal", false);
        let mut ui_state = ui_state_with_cursor_at_end("/goal");
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/goal");

        // Typing a space releases the dropdown; the rest is normal input.
        for ch in " ship v2".chars() {
            execute_tui_command(
                &mut state,
                &mut ui_state,
                &sender,
                TuiCommand::InputCharacter(ch),
            )
            .await
            .unwrap();
        }
        assert_eq!(state.input, "/goal ship v2");
        assert!(command_dropdown(&state, &ui_state).is_none());
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "/goal ship v2".to_string(),
                PromptSource::Fresh
            )))
        );
        // Nothing was dispatched during acceptance or typing.
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn command_dropdown_stays_disabled_during_approval_clarification_and_waiting() {
        // Pending approval (also WaitingForUser via the helper).
        let mut state = state_with_input("/", true);
        let ui_state = ui_state_with_cursor_at_end("/");
        assert!(command_dropdown(&state, &ui_state).is_none());
        assert!(!render_to_text_with_ui(&state, &ui_state, 80, 24).contains("Commands"));

        // WaitingForUser without approval.
        state.pending_approval = None;
        state.run_state = RunState::WaitingForUser;
        assert!(command_dropdown(&state, &ui_state).is_none());

        // Pending clarification: a `/`-prefixed answer must stay normal input.
        let mut clar_state = state_with_input("/tmp/project", false);
        clar_state.pending_clarification = Some(clarification_view(vec![]));
        let clar_ui = ui_state_with_cursor_at_end("/tmp/project");
        assert!(command_dropdown(&clar_state, &clar_ui).is_none());
    }

    #[tokio::test]
    async fn command_dropdown_escape_survives_cursor_move() {
        // Regression: a cursor move is not an edit, so it must not clear the
        // Escape dismissal and re-open the dropdown for unchanged input.
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("/g", false);
        let mut ui_state = ui_state_with_cursor_at_end("/g");
        apply_command_dropdown_command(&mut state, &mut ui_state, CommandDropdownCommand::Dismiss);
        assert!(command_dropdown(&state, &ui_state).is_none());

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::MoveInputCursor(InputCursorCommand::Left),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/g");
        assert_eq!(ui_state.command_dropdown_dismissed, Some("/g".to_string()));
        assert!(command_dropdown(&state, &ui_state).is_none());
    }

    #[tokio::test]
    async fn command_dropdown_reactivates_after_edit_following_dismiss() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("/g", false);
        let mut ui_state = ui_state_with_cursor_at_end("/g");
        apply_command_dropdown_command(&mut state, &mut ui_state, CommandDropdownCommand::Dismiss);
        assert!(command_dropdown(&state, &ui_state).is_none());

        // Editing the input clears the dismissal and re-activates discovery.
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputCharacter('o'),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/go");
        assert_eq!(ui_state.command_dropdown_dismissed, None);
        assert!(command_dropdown(&state, &ui_state).is_some());
    }

    #[tokio::test]
    async fn command_dropdown_second_enter_submits_no_arg_command() {
        // The re-accept guard: after accepting /config, a second Enter is no
        // longer intercepted and submits the command.
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/config", false);
        let mut ui_state = ui_state_with_cursor_at_end("/config");
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/config");
        assert!(receiver.try_recv().is_err());

        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "/config".to_string(),
                PromptSource::Fresh
            )))
        );
    }

    #[tokio::test]
    async fn command_dropdown_second_enter_toggles_help() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("/help", false);
        let mut ui_state = ui_state_with_cursor_at_end("/help");
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::CommandDropdown(CommandDropdownCommand::Accept),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "/help");

        // Second Enter falls through to the /help toggle, no longer intercepted.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::ToggleHelp)
        );
    }

    #[test]
    fn discovers_project_and_personal_skills_from_agent_and_claude_roots() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("home");
        let project_agents = project.join(".agents/skills");
        let project_claude = project.join(".claude/skills");
        let personal_agents = home.join(".agents/skills");
        let personal_claude = home.join(".claude/skills");
        write_skill(&project_agents, "project-agent", "frontmatter-project");
        write_skill(
            &project_agents.join(".system"),
            "project-system",
            "nested-project",
        );
        write_skill_without_name(&project_claude, "project-claude");
        write_skill(&personal_agents, "personal-agent", "frontmatter-personal");
        write_skill(&personal_claude, "personal-claude", "personal-claude");

        let roots = skills::skill_roots_with_home(&project, Some(&home));
        let suggestions = skills::discover_skill_suggestions(&roots).unwrap();

        assert_eq!(
            suggestion_aliases(&suggestions),
            vec![
                (
                    "frontmatter-project".to_string(),
                    ".agents/skills".to_string()
                ),
                ("nested-project".to_string(), ".agents/skills".to_string()),
                ("project-agent".to_string(), ".agents/skills".to_string()),
                ("project-system".to_string(), ".agents/skills".to_string()),
                ("project-claude".to_string(), ".claude/skills".to_string()),
                (
                    "frontmatter-personal".to_string(),
                    "~/.agents/skills".to_string()
                ),
                ("personal-agent".to_string(), "~/.agents/skills".to_string()),
                (
                    "personal-claude".to_string(),
                    "~/.claude/skills".to_string()
                ),
            ]
        );
        assert!(suggestions.iter().any(|skill| {
            skill.alias == "project-agent"
                && skill.display_name == "frontmatter-project"
                && skill.canonical_id == ".agents/skills/project-agent/SKILL.md"
        }));
    }

    #[test]
    fn reads_cached_skill_suggestions_when_fingerprint_matches() {
        let dir = tempdir().unwrap();
        let fingerprint = vec![SkillFileFingerprint {
            path: "project/SKILL.md".to_string(),
            byte_len: 10,
            modified_secs: 20,
            modified_nanos: 30,
        }];
        let suggestions = test_skill_suggestions();

        write_skill_suggestion_cache(dir.path(), &fingerprint, &suggestions).unwrap();

        assert_eq!(
            read_cached_skill_suggestions(dir.path(), &fingerprint),
            Some(suggestions)
        );
    }

    #[test]
    fn ignores_cached_skill_suggestions_when_fingerprint_changes() {
        let dir = tempdir().unwrap();
        let cached_fingerprint = vec![SkillFileFingerprint {
            path: "project/SKILL.md".to_string(),
            byte_len: 10,
            modified_secs: 20,
            modified_nanos: 30,
        }];
        let current_fingerprint = vec![SkillFileFingerprint {
            path: "project/SKILL.md".to_string(),
            byte_len: 11,
            modified_secs: 20,
            modified_nanos: 30,
        }];

        write_skill_suggestion_cache(dir.path(), &cached_fingerprint, &test_skill_suggestions())
            .unwrap();

        assert_eq!(
            read_cached_skill_suggestions(dir.path(), &current_fingerprint),
            None
        );
    }

    #[test]
    fn load_skill_suggestions_uses_cache_only_when_fingerprint_matches() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let roots = skills::skill_roots_with_home(&project, None);
        write_skill(
            &project.join(".agents/skills"),
            "fresh-skill",
            "fresh-skill",
        );
        let cached_only =
            test_skill_suggestion("cached-only", SkillSourceTag::Project, ".agents/skills");
        let original_fingerprint = skill_file_fingerprints(&roots);
        write_skill_suggestion_cache(&project, &original_fingerprint, &[cached_only]).unwrap();

        let cached = load_skill_suggestions_from_roots(&project, &roots);
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].alias, "cached-only");

        write_skill_with_body(
            &project.join(".agents/skills"),
            "fresh-skill",
            "fresh-skill",
            "UPDATED_SKILL_BODY_WITH_EXTRA_BYTES",
        );

        let refreshed = load_skill_suggestions_from_roots(&project, &roots);

        assert!(refreshed.iter().any(|skill| skill.alias == "fresh-skill"));
        assert!(!refreshed.iter().any(|skill| skill.alias == "cached-only"));
    }

    #[test]
    fn writes_metadata_only_skill_suggestion_cache() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let roots = skills::skill_roots_with_home(&project, None);
        write_skill_with_body(
            &project.join(".agents/skills"),
            "metadata-skill",
            "metadata-skill",
            "SECRET_SKILL_BODY_SHOULD_NOT_BE_CACHED",
        );
        let fingerprint = skill_file_fingerprints(&roots);

        let suggestions = refresh_skill_suggestions(&project, &roots, &fingerprint);
        let cache = fs::read_to_string(skill_cache_path(&project)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&cache).unwrap();

        assert!(suggestions
            .iter()
            .any(|skill| skill.alias == "metadata-skill"));
        assert!(cache.contains("\"alias\": \"metadata-skill\""));
        assert!(cache.contains("\"source_origin\": \".agents/skills\""));
        assert!(cache.contains("metadata-skill/SKILL.md"));
        assert!(!cache.contains("SECRET_SKILL_BODY_SHOULD_NOT_BE_CACHED"));
        assert!(value["suggestions"][0].get("content").is_none());
        assert!(value["suggestions"][0].get("body").is_none());
    }

    #[test]
    fn cached_tui_suggestion_is_not_authoritative_for_app_resolution() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("home");
        let roots = skills::skill_roots_with_home(&project, Some(&home));
        write_skill_with_body(
            &project.join(".agents/skills"),
            "cached-skill",
            "cached-skill",
            "Cached skill body.",
        );
        let fingerprint = skill_file_fingerprints(&roots);
        refresh_skill_suggestions(&project, &roots, &fingerprint);

        fs::remove_dir_all(project.join(".agents/skills/cached-skill")).unwrap();
        let cached = read_cached_skill_suggestions(&project, &fingerprint).unwrap();
        let error =
            skills::compile_prompt_with_home(&project, Some(&home), "/skill:cached-skill inspect")
                .unwrap_err();

        assert!(cached.iter().any(|skill| skill.alias == "cached-skill"));
        assert!(matches!(error.kind, skills::SkillLoadErrorKind::Unknown));
    }

    #[test]
    fn enter_submits_after_agent_selection_has_trailing_space() {
        let state = state_with_agent_roster("/agent:fixer inspect docs");
        let ui_state = ui_state_with_cursor_at_end(&state.input);

        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "/agent:fixer inspect docs".to_string(),
                PromptSource::Fresh
            )))
        );
    }

    #[tokio::test]
    async fn input_editing_is_local_to_tui_state() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState::default();

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputCharacter('x'),
        )
        .await
        .unwrap();
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputBackspace,
        )
        .await
        .unwrap();

        assert!(state.input.is_empty());
        assert_eq!(ui_state.input_cursor, 0);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn input_editing_inserts_and_deletes_at_cursor() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("abcd", false);
        let mut ui_state = TuiUiState {
            input_cursor: 2,
            ..TuiUiState::default()
        };

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputCharacter('X'),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "abXcd");
        assert_eq!(ui_state.input_cursor, 3);

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputBackspace,
        )
        .await
        .unwrap();

        assert_eq!(state.input, "abcd");
        assert_eq!(ui_state.input_cursor, 2);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn worker_state_sync_preserves_local_input() {
        let mut local_state = state_with_input("draft prompt", false);
        let mut ui_state = TuiUiState {
            input_cursor: 5,
            ..TuiUiState::default()
        };
        let worker_state = AppState {
            events: vec!["Run started.".to_string()],
            ..state_with_input("", false)
        };
        let (sender, mut receiver) = watch::channel(state_with_input("", false));

        sender.send(worker_state).unwrap();
        sync_worker_state(&mut local_state, &mut receiver);
        clamp_input_cursor(&mut ui_state, &local_state.input);

        assert_eq!(local_state.input, "draft prompt");
        assert_eq!(ui_state.input_cursor, 5);
        assert_eq!(local_state.events, vec!["Run started."]);
    }

    #[test]
    fn user_prompt_event_line_has_background() {
        let theme = TuiUiState::default().theme;
        let line = legacy_chat_line(&theme, "You: build a feature");

        assert!(line.spans.iter().all(|span| span.style.bg.is_some()));
    }

    #[test]
    fn enter_key_answers_pending_approval() {
        let yes_state = state_with_input("yes", true);
        let no_state = state_with_input("no", true);

        assert_eq!(
            key_event_to_tui_command(
                &yes_state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(
                ApprovalResolution::ApproveOnce
            )))
        );
        assert_eq!(
            key_event_to_tui_command(&no_state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(
                ApprovalResolution::Deny
            )))
        );
    }

    #[tokio::test]
    async fn parallel_approval_answer_signals_without_queuing_stale_event() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let app = App::new(config).await.unwrap();
        let approval_handle = app.approval_handle();
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("yes", true);
        state.pending_approval.as_mut().unwrap().group_id = Some("group".to_string());
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);

        let keep_running = execute_tui_command_with_interrupt(
            &mut state,
            &mut ui_state,
            &sender,
            None,
            Some(&approval_handle),
            TuiCommand::Dispatch(AppEvent::ApprovalAnswered(ApprovalResolution::ApproveOnce)),
        )
        .await
        .unwrap();

        assert!(keep_running);
        assert!(state.input.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn edit_keys_become_local_input_commands() {
        let state = state_with_input("abc", false);

        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::InputCharacter('x'))
        );
        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
            ),
            Some(TuiCommand::InputBackspace)
        );
        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Left))
        );
        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Right))
        );
        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Up))
        );
        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Down))
        );
    }

    #[test]
    fn mouse_wheel_over_event_stream_becomes_scroll_commands() {
        let ui_state = TuiUiState {
            event_area: Rect::new(10, 2, 30, 12),
            ..TuiUiState::default()
        };

        assert_eq!(
            mouse_event_to_tui_command(
                &ui_state,
                MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: 12,
                    row: 4,
                    modifiers: KeyModifiers::NONE,
                },
            ),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::LinesUp(
                MOUSE_SCROLL_LINES
            )))
        );
        assert_eq!(
            mouse_event_to_tui_command(
                &ui_state,
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 12,
                    row: 4,
                    modifiers: KeyModifiers::NONE,
                },
            ),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::LinesDown(
                MOUSE_SCROLL_LINES
            )))
        );
    }

    #[test]
    fn mouse_wheel_ignores_non_event_stream_areas_and_help_modal() {
        let ui_state = TuiUiState {
            event_area: Rect::new(10, 2, 30, 12),
            ..TuiUiState::default()
        };
        let outside_event_stream = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 4,
            row: 4,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            mouse_event_to_tui_command(&ui_state, outside_event_stream),
            None
        );

        let help_state = TuiUiState {
            help_visible: true,
            ..ui_state
        };
        let inside_event_stream = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 12,
            row: 4,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            mouse_event_to_tui_command(&help_state, inside_event_stream),
            None
        );
    }

    #[tokio::test]
    async fn arrow_commands_move_input_cursor_without_app_event() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("abcd", false);
        let mut ui_state = TuiUiState {
            input_cursor: 2,
            ..TuiUiState::default()
        };

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::MoveInputCursor(InputCursorCommand::Left),
        )
        .await
        .unwrap();
        assert_eq!(ui_state.input_cursor, 1);

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::MoveInputCursor(InputCursorCommand::Right),
        )
        .await
        .unwrap();

        assert_eq!(ui_state.input_cursor, 2);
        assert_eq!(state.input, "abcd");
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn wrapped_input_arrow_keys_move_visible_cursor_and_edit_position() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("abcdefghijklmnopqrstuvwxyz1234", false);
        let mut ui_state = ui_state_with_cursor_at_end(&state.input);
        let backend = TestBackend::new(24, 12);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        assert_eq!(ui_state.input_width, 20);

        for key in [
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE),
        ] {
            let command = key_event_to_tui_command(&state, key).unwrap();
            execute_tui_command(&mut state, &mut ui_state, &sender, command)
                .await
                .unwrap();
        }

        assert_eq!(state.input, "abcdefghiXjklmnopqrstuvwxyz1234");
        assert_eq!(ui_state.input_cursor, 10);
        assert!(receiver.try_recv().is_err());

        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(13, 7));
    }

    #[test]
    fn up_down_move_cursor_across_wrapped_input_lines() {
        let state = state_with_input("abcdefghij", false);
        let mut ui_state = TuiUiState {
            input_cursor: 7,
            input_width: 5,
            ..TuiUiState::default()
        };

        move_input_cursor(&mut ui_state, &state.input, InputCursorCommand::Up);
        assert_eq!(ui_state.input_cursor, 2);

        move_input_cursor(&mut ui_state, &state.input, InputCursorCommand::Down);
        assert_eq!(ui_state.input_cursor, 7);
    }

    #[test]
    fn down_cursor_movement_clamps_to_short_wrapped_line() {
        let state = state_with_input("abcdefg", false);
        let mut ui_state = TuiUiState {
            input_cursor: 3,
            input_width: 5,
            ..TuiUiState::default()
        };

        move_input_cursor(&mut ui_state, &state.input, InputCursorCommand::Down);

        assert_eq!(ui_state.input_cursor, 7);
    }

    #[test]
    fn ctrl_c_is_the_only_exit_key() {
        let state = state_with_input("", false);

        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::InputCharacter('q'))
        );
        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            None
        );
        // Ctrl-C is now owned by the single reserved-key guard in the wrapper, not the
        // base handler — assert it via the real entry point.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &TuiUiState::default(),
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested))
        );
    }

    #[test]
    fn ctrl_c_interrupts_in_every_context() {
        let ctrl_c = key_with_modifiers(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let interrupt = Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested));

        // help visible
        let help_state = state_with_input("", false);
        let help_ui = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };
        assert_eq!(
            key_event_to_tui_command_with_ui(&help_state, &help_ui, ctrl_c),
            interrupt,
            "help context"
        );

        // clarification pending
        let mut clar_state = state_with_input("draft", false);
        clar_state.pending_clarification = Some(clarification_view(vec![clarification_option(
            "opt1", "Option 1",
        )]));
        assert_eq!(
            key_event_to_tui_command_with_ui(&clar_state, &TuiUiState::default(), ctrl_c),
            interrupt,
            "clarification context"
        );

        // governance decision pending
        let gov_state = state_with_governance_decision("draft");
        assert_eq!(
            key_event_to_tui_command_with_ui(&gov_state, &TuiUiState::default(), ctrl_c),
            interrupt,
            "governance context"
        );

        // approval pending
        let appr_state = state_with_input("", true);
        assert_eq!(
            key_event_to_tui_command_with_ui(&appr_state, &TuiUiState::default(), ctrl_c),
            interrupt,
            "approval context"
        );

        // plain normal input
        let normal_state = state_with_input("typing", false);
        assert_eq!(
            key_event_to_tui_command_with_ui(&normal_state, &TuiUiState::default(), ctrl_c),
            interrupt,
            "normal context"
        );
    }

    #[test]
    fn non_ctrl_c_keys_route_normally_in_each_context() {
        // help: Esc still toggles the modal
        let help_state = state_with_input("", false);
        let help_ui = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };
        assert_eq!(
            key_event_to_tui_command_with_ui(&help_state, &help_ui, key(KeyCode::Esc)),
            Some(TuiCommand::ToggleHelp),
            "help Esc"
        );

        // normal: a plain char still inserts
        let normal_state = state_with_input("", false);
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &normal_state,
                &TuiUiState::default(),
                key(KeyCode::Char('q'))
            ),
            Some(TuiCommand::InputCharacter('q')),
            "normal char"
        );

        // clarification: Up still cycles options (unaffected by the reserved guard)
        let mut clar_state = state_with_input("", false);
        clar_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Option 1"),
            clarification_option("opt2", "Option 2"),
        ]));
        assert_eq!(
            key_event_to_tui_command_with_ui(&clar_state, &TuiUiState::default(), key(KeyCode::Up)),
            Some(TuiCommand::Clarification(
                ClarificationCommand::PreviousOption
            )),
            "clarification Up"
        );
    }

    // ── default keymap wiring (config-driven-keybindings task_04) ──

    #[test]
    fn command_for_action_maps_all_ten_actions() {
        use KeyAction::*;
        assert_eq!(command_for_action(ToggleRoster), TuiCommand::ToggleRoster);
        assert_eq!(
            command_for_action(ScrollPageUp),
            TuiCommand::ScrollEvents(EventScrollCommand::PageUp)
        );
        assert_eq!(
            command_for_action(ScrollPageDown),
            TuiCommand::ScrollEvents(EventScrollCommand::PageDown)
        );
        assert_eq!(
            command_for_action(ScrollTop),
            TuiCommand::ScrollEvents(EventScrollCommand::Top)
        );
        assert_eq!(
            command_for_action(ScrollBottom),
            TuiCommand::ScrollEvents(EventScrollCommand::Bottom)
        );
        assert_eq!(
            command_for_action(InputLineStart),
            TuiCommand::MoveInputCursor(InputCursorCommand::LineStart)
        );
        assert_eq!(
            command_for_action(InputLineEnd),
            TuiCommand::MoveInputCursor(InputCursorCommand::LineEnd)
        );
        assert_eq!(
            command_for_action(InputKillToEnd),
            TuiCommand::InputKill(InputKillCommand::ToLineEnd)
        );
        assert_eq!(
            command_for_action(InputKillToStart),
            TuiCommand::InputKill(InputKillCommand::ToLineStart)
        );
        assert_eq!(
            command_for_action(InputKillWordBack),
            TuiCommand::InputKill(InputKillCommand::WordBack)
        );
    }

    #[test]
    fn default_keymap_routes_all_ten_actions_by_their_default_keys() {
        let state = state_with_input("hello", false);
        let ui = TuiUiState::default(); // built from DEFAULTS
        let route = |k: KeyEvent| key_event_to_tui_command_with_ui(&state, &ui, k);
        let ctrl = |c: char| key_with_modifiers(KeyCode::Char(c), KeyModifiers::CONTROL);

        assert_eq!(route(ctrl('l')), Some(TuiCommand::ToggleRoster));
        assert_eq!(
            route(key(KeyCode::PageUp)),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::PageUp))
        );
        assert_eq!(
            route(key(KeyCode::PageDown)),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::PageDown))
        );
        assert_eq!(
            route(key(KeyCode::Home)),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::Top))
        );
        assert_eq!(
            route(key(KeyCode::End)),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::Bottom))
        );
        assert_eq!(
            route(ctrl('a')),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::LineStart))
        );
        assert_eq!(
            route(ctrl('e')),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::LineEnd))
        );
        assert_eq!(
            route(ctrl('k')),
            Some(TuiCommand::InputKill(InputKillCommand::ToLineEnd))
        );
        assert_eq!(
            route(ctrl('u')),
            Some(TuiCommand::InputKill(InputKillCommand::ToLineStart))
        );
        assert_eq!(
            route(ctrl('w')),
            Some(TuiCommand::InputKill(InputKillCommand::WordBack))
        );
    }

    #[test]
    fn default_keymap_preserves_pre_feature_routing() {
        let state = state_with_input("draft", false);
        let ui = TuiUiState::default();
        let route = |k: KeyEvent| key_event_to_tui_command_with_ui(&state, &ui, k);

        // Remappable keys (now resolved via the keymap) — identical commands to before.
        assert_eq!(
            route(key_with_modifiers(
                KeyCode::Char('l'),
                KeyModifiers::CONTROL
            )),
            Some(TuiCommand::ToggleRoster)
        );
        assert_eq!(
            route(key(KeyCode::PageUp)),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::PageUp))
        );
        assert_eq!(
            route(key(KeyCode::Home)),
            Some(TuiCommand::ScrollEvents(EventScrollCommand::Top))
        );
        // Keys the keymap does not own — unchanged via the fallback handler.
        assert_eq!(
            route(key(KeyCode::Up)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Up))
        );
        assert_eq!(
            route(key(KeyCode::Left)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Left))
        );
        assert_eq!(
            route(key(KeyCode::Backspace)),
            Some(TuiCommand::InputBackspace)
        );
    }

    #[test]
    fn unmapped_key_falls_through_to_input_character() {
        let state = state_with_input("", false);
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui, key(KeyCode::Char('z'))),
            Some(TuiCommand::InputCharacter('z'))
        );
    }

    #[test]
    fn keymap_is_gated_to_the_normal_context() {
        let ui = TuiUiState::default();
        let ctrl_a = key_with_modifiers(KeyCode::Char('a'), KeyModifiers::CONTROL);

        // In the approval modal, the normal-mode Ctrl-A editing binding must NOT be
        // interpreted by the keymap — it stays inert (the base handler returns None).
        let appr_state = state_with_input("", true);
        assert_eq!(
            key_event_to_tui_command_with_ui(&appr_state, &ui, ctrl_a),
            None,
            "Ctrl-A must not trigger line-start inside the approval modal"
        );

        // In the normal context the same key resolves via the keymap.
        let normal_state = state_with_input("", false);
        assert_eq!(
            key_event_to_tui_command_with_ui(&normal_state, &ui, ctrl_a),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::LineStart))
        );
    }

    // ── resolve customizations end-to-end (config-driven-keybindings task_08) ──

    fn ui_state_with_overrides(overrides: keybindings::KeybindingOverrides) -> TuiUiState {
        TuiUiState {
            keymap: Keymap::resolve(&keybindings::DEFAULTS, &overrides),
            ..TuiUiState::default()
        }
    }

    #[test]
    fn rebind_routes_new_key_and_drops_old_default() {
        let mut overrides = keybindings::KeybindingOverrides::new();
        overrides.insert(
            KeyAction::ToggleRoster,
            Some(keybindings::parse_key("ctrl+g").unwrap()),
        );
        let ui = ui_state_with_overrides(overrides);
        let state = state_with_input("", false);

        // The new key toggles the roster…
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                key_with_modifiers(KeyCode::Char('g'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::ToggleRoster)
        );
        // …and the displaced default no longer does (falls through to None).
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                key_with_modifiers(KeyCode::Char('l'), KeyModifiers::CONTROL)
            ),
            None
        );
    }

    #[test]
    fn unbind_removes_the_action_key_entirely() {
        let mut overrides = keybindings::KeybindingOverrides::new();
        overrides.insert(KeyAction::ToggleRoster, None);
        let ui = ui_state_with_overrides(overrides);
        let state = state_with_input("", false);

        // The old default no longer toggles, and nothing else picks it up.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                key_with_modifiers(KeyCode::Char('l'), KeyModifiers::CONTROL)
            ),
            None
        );
    }

    #[test]
    fn reserved_ctrl_c_survives_keybinding_overrides() {
        let mut overrides = keybindings::KeybindingOverrides::new();
        overrides.insert(
            KeyAction::ToggleRoster,
            Some(keybindings::parse_key("ctrl+g").unwrap()),
        );
        let ui = ui_state_with_overrides(overrides);
        let state = state_with_input("", false);
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                key_with_modifiers(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested))
        );
    }

    #[test]
    fn config_keybindings_resolve_into_routing_and_keys_tab() {
        use crate::config::{load_effective_config, ConfigLoadOptions};
        // A user-scope config (explicit --config) rebinds toggle-roster to ctrl+g.
        let cfg_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let config_path = cfg_dir.path().join("home-config.toml");
        std::fs::write(
            &config_path,
            "[keybindings.normal]\ntoggle-roster = \"ctrl+g\"\n",
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: work_dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        // Resolve exactly as `run_tui` does.
        let ui = TuiUiState {
            keymap: Keymap::resolve(&keybindings::DEFAULTS, &config.keybindings),
            ..TuiUiState::default()
        };
        let state = state_with_input("", false);

        // Routing reflects the rebind end-to-end.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                key_with_modifiers(KeyCode::Char('g'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::ToggleRoster)
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                key_with_modifiers(KeyCode::Char('l'), KeyModifiers::CONTROL)
            ),
            None
        );

        // And the Keys tab shows the customized binding, not the old default.
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&keys_tab_lines(&ui.keymap, &theme));
        assert!(text.contains("ctrl+g"), "keys tab shows rebound key");
        assert!(
            !text.contains("ctrl+l"),
            "old default key gone from keys tab"
        );
    }

    // ── code-review fixes (config-driven-keybindings) ──

    #[test]
    fn release_key_events_are_ignored_to_avoid_double_fire() {
        // crossterm emits Release events on Windows / under the Kitty protocol; routing
        // one would fire the action a second time. Press still routes; Release does not.
        let state = state_with_input("", false);
        let ui = TuiUiState::default();
        let press = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui, press),
            Some(TuiCommand::InputKill(InputKillCommand::WordBack))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui, release),
            None,
            "release events must not route"
        );
        // The reserved interrupt also must not double-fire on release.
        let ctrl_c_release = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui, ctrl_c_release),
            None
        );
    }

    #[test]
    fn chat_scroll_keys_still_work_in_modal_contexts() {
        let ui = TuiUiState::default();
        // The approval branch forces the modal path (the keymap is not consulted), so
        // these assertions prove the dedicated chat-scroll fallback, not the keymap.
        let approval = state_with_input("", true);

        for (code, expected) in [
            (KeyCode::PageUp, EventScrollCommand::PageUp),
            (KeyCode::PageDown, EventScrollCommand::PageDown),
            (KeyCode::Home, EventScrollCommand::Top),
            (KeyCode::End, EventScrollCommand::Bottom),
        ] {
            assert_eq!(
                key_event_to_tui_command_with_ui(&approval, &ui, key(code)),
                Some(TuiCommand::ScrollEvents(expected)),
                "{code:?} should scroll the chat in the approval modal"
            );
        }

        // But a normal-mode editing key (Ctrl-A) stays inert in the approval modal —
        // the rebindable keymap is still gated out of modal contexts.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &approval,
                &ui,
                key_with_modifiers(KeyCode::Char('a'), KeyModifiers::CONTROL)
            ),
            None
        );
    }

    #[test]
    fn esc_closes_help_modal_only_when_visible() {
        let state = state_with_input("", false);
        let hidden = TuiUiState::default();
        let visible = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };

        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &hidden,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            None
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            Some(TuiCommand::ToggleHelp)
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            None
        );
    }

    #[test]
    fn help_visible_arrows_and_tab_navigate_tabs() {
        let state = state_with_input("", false);
        let visible = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };

        // Right + Tab advance to the next tab.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpNextTab)
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpNextTab)
        );

        // Left + Shift-Tab (and the terminal BackTab variant) retreat.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpPrevTab)
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)
            ),
            Some(TuiCommand::HelpPrevTab)
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)
            ),
            Some(TuiCommand::HelpPrevTab)
        );

        // Esc still closes; an unrelated key does not leak to the base handler.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            Some(TuiCommand::ToggleHelp)
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &visible,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            None
        );

        // Navigation keys are inert when help is closed (no leakage the other way).
        let hidden = TuiUiState::default();
        assert_ne!(
            key_event_to_tui_command_with_ui(
                &state,
                &hidden,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpNextTab)
        );
    }

    #[tokio::test]
    async fn help_next_prev_tab_commands_advance_active_tab() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::GettingStarted,
            ..TuiUiState::default()
        };

        // Next from the first tab moves to Commands.
        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::HelpNextTab)
            .await
            .unwrap();
        assert_eq!(ui_state.help_active_tab, HelpTab::Commands);

        // Next from the last tab (Cli) wraps back to Getting Started.
        ui_state.help_active_tab = HelpTab::Cli;
        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::HelpNextTab)
            .await
            .unwrap();
        assert_eq!(ui_state.help_active_tab, HelpTab::GettingStarted);

        // Prev from the first tab wraps back to Cli.
        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::HelpPrevTab)
            .await
            .unwrap();
        assert_eq!(ui_state.help_active_tab, HelpTab::Cli);

        // Navigation is local UI state — no app event emitted.
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn ctrl_l_toggles_roster_visibility_without_app_event() {
        let state = state_with_input("", false);
        let mut local_state = state.clone();
        let mut ui_state = TuiUiState::default();
        let (sender, mut receiver) = mpsc::channel(1);

        // Ctrl-L is owned by the active keymap (default), routed via the wrapper.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::ToggleRoster)
        );

        let keep_running = execute_tui_command(
            &mut local_state,
            &mut ui_state,
            &sender,
            TuiCommand::ToggleRoster,
        )
        .await
        .unwrap();

        assert!(keep_running);
        assert!(!ui_state.roster_visible);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn hidden_roster_gives_event_stream_full_width() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Running,
            active_run_id: Some("run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: None,
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: vec![AgentView {
                id: "fixer".to_string(),
                name: "Fixer".to_string(),
                runtime: "codex".to_string(),
                model: "default".to_string(),
                effort: "high".to_string(),
                thinking: false,
                capabilities: Vec::new(),
                availability: None,
                status: "idle".to_string(),
            }],
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["Run started.".to_string()],
            input: String::new(),
            git_context: None,
            recoverable_session: false,
        };
        let ui_state = TuiUiState {
            roster_visible: false,
            help_visible: false,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 100, 24);

        assert!(!text.contains("Agent Roster"));
        assert!(text.contains("Chat"));
        assert!(text.contains("Run started."));
    }

    #[test]
    fn event_stream_auto_follows_latest_event() {
        let mut state = state_with_input("", false);
        state.events = (0..30).map(|index| format!("event {index:03}")).collect();
        let mut ui_state = TuiUiState::default();

        render_to_text_with_ui_mut(&state, &mut ui_state, 80, 12);
        let first_scroll = ui_state.event_scroll;

        state.events.push("event 030".to_string());
        let text = render_to_text_with_ui_mut(&state, &mut ui_state, 80, 12);

        assert!(ui_state.event_follow);
        assert!(ui_state.event_scroll >= first_scroll);
        assert!(text.contains("event 030"));
        assert!(!text.contains("event 000"));
    }

    #[test]
    fn event_stream_scroll_keys_leave_follow_mode() {
        let mut ui_state = TuiUiState {
            event_scroll: 90,
            event_follow: true,
            event_content_lines: 100,
            event_viewport_lines: 10,
            ..TuiUiState::default()
        };

        scroll_events(&mut ui_state, EventScrollCommand::PageUp);

        assert_eq!(ui_state.event_scroll, 81);
        assert!(!ui_state.event_follow);

        scroll_events(&mut ui_state, EventScrollCommand::Bottom);

        assert_eq!(ui_state.event_scroll, 90);
        assert!(ui_state.event_follow);
    }

    #[test]
    fn event_stream_mouse_scroll_moves_by_lines() {
        let mut ui_state = TuiUiState {
            event_scroll: 90,
            event_follow: true,
            event_content_lines: 100,
            event_viewport_lines: 10,
            ..TuiUiState::default()
        };

        scroll_events(
            &mut ui_state,
            EventScrollCommand::LinesUp(MOUSE_SCROLL_LINES),
        );

        assert_eq!(ui_state.event_scroll, 87);
        assert!(!ui_state.event_follow);

        scroll_events(
            &mut ui_state,
            EventScrollCommand::LinesDown(MOUSE_SCROLL_LINES),
        );

        assert_eq!(ui_state.event_scroll, 90);
        assert!(ui_state.event_follow);
    }

    fn state_with_input(input: &str, pending_approval: bool) -> AppState {
        AppState {
            session_id: "session".to_string(),
            run_state: if pending_approval {
                RunState::WaitingForUser
            } else {
                RunState::Idle
            },
            active_run_id: pending_approval.then(|| "run".to_string()),
            session_goal: None,
            config_status: default_config_status(),
            live_step: None,
            live_steps: Vec::new(),
            pending_approval: pending_approval.then(|| crate::app::PendingApprovalView {
                run_id: "run".to_string(),
                group_id: None,
                step_id: "step".to_string(),
                action_id: "action".to_string(),
                agent: "fixer".to_string(),
                summary: "Action requires approval.".to_string(),
                diagnostic: None,
                ..Default::default()
            }),
            show_first_approval_explainer: false,
            pending_clarification: None,
            pending_governance_decision: None,
            pending_plan_approval: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: input.to_string(),
            git_context: None,
            recoverable_session: false,
        }
    }

    fn governance_decision_view(decision_id: &str) -> GovernanceDecisionView {
        GovernanceDecisionView {
            run_id: "run".to_string(),
            decision_id: decision_id.to_string(),
            kind: GovernanceKind::EarlyAbort,
            title: "Confirm intent before this run edits files".to_string(),
            intent: "Refactor the config loader".to_string(),
            approach: vec!["Split the loader".to_string()],
            agent: Some("fixer".to_string()),
            write_scope: vec!["src/config".to_string()],
            risk_label: "High - edits source files".to_string(),
            plan: None,
        }
    }

    fn state_with_governance_decision(input: &str) -> AppState {
        let mut state = state_with_input(input, false);
        state.run_state = RunState::WaitingForUser;
        state.active_run_id = Some("run".to_string());
        state.pending_governance_decision = Some(PendingGovernanceDecisionView {
            run_id: "run".to_string(),
            decision_id: "gov-1".to_string(),
            view: governance_decision_view("gov-1"),
        });
        state
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone())
            .collect::<String>()
    }

    #[test]
    fn governance_accept_key_routes_to_resolve_accept() {
        let state = state_with_governance_decision("");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
                "gov-1".to_string(),
                GovernanceAnswer::Accept
            )))
        );
    }

    #[test]
    fn governance_reject_key_routes_to_resolve_reject() {
        let state = state_with_governance_decision("");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
                "gov-1".to_string(),
                GovernanceAnswer::Reject { redirect: None }
            )))
        );
    }

    #[test]
    fn governance_reject_redirect_comes_from_the_input_line() {
        let state = state_with_governance_decision("focus on tests");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
                "gov-1".to_string(),
                GovernanceAnswer::Reject {
                    redirect: Some("focus on tests".to_string())
                }
            )))
        );
    }

    #[test]
    fn governance_enter_on_empty_line_never_accepts() {
        let state = state_with_governance_decision("");
        let ui = TuiUiState::default();
        let cmd = key_event_to_tui_command_with_ui(
            &state,
            &ui,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        // The safe default must never land on Accept; an empty line is a no-op.
        assert_ne!(
            cmd,
            Some(TuiCommand::Dispatch(AppEvent::GovernanceDecisionResolved(
                "gov-1".to_string(),
                GovernanceAnswer::Accept
            )))
        );
        assert_eq!(cmd, None);
    }

    #[test]
    fn governance_typing_routes_to_the_input_line() {
        let state = state_with_governance_decision("");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::InputCharacter('x'))
        );
    }

    #[test]
    fn queue_control_inactive_during_pending_governance_decision() {
        let mut state = state_with_governance_decision("");
        state.queued_follow_ups = vec![QueuedFollowUpView {
            id: "q1".to_string(),
            prompt: "later".to_string(),
            created_at: "t".to_string(),
            status: QueuedFollowUpStatus::Pending,
            pause_reason: None,
        }];
        // A pending governance decision suppresses queue keys even with an empty
        // composer and queued items present.
        assert!(!queue_control_active(&state, &TuiUiState::default()));
    }

    // ── whole-plan DAG approval gate key routing (review fix) ──

    fn state_with_plan_approval(input: &str) -> AppState {
        let mut state = state_with_input(input, false);
        state.run_state = RunState::WaitingForUser;
        state.active_run_id = Some("run".to_string());
        state.pending_plan_approval = Some(PendingPlanApprovalView {
            run_id: "run".to_string(),
            graph_id: "graph-1".to_string(),
            question_id: "plan-approval:graph-1".to_string(),
            summary: "Review the proposed plan: 4 node(s), 4 edge(s).".to_string(),
        });
        state
    }

    #[test]
    fn plan_approval_accept_key_routes_to_resolve_accept() {
        let state = state_with_plan_approval("");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PlanApprovalResolved(
                "plan-approval:graph-1".to_string(),
                PlanApprovalAnswer::Accept
            )))
        );
    }

    #[test]
    fn plan_approval_esc_rejects_without_reason() {
        let state = state_with_plan_approval("");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PlanApprovalResolved(
                "plan-approval:graph-1".to_string(),
                PlanApprovalAnswer::Reject { reason: None }
            )))
        );
    }

    #[test]
    fn plan_approval_enter_rejects_with_typed_reason() {
        let state = state_with_plan_approval("too risky");
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::PlanApprovalResolved(
                "plan-approval:graph-1".to_string(),
                PlanApprovalAnswer::Reject {
                    reason: Some("too risky".to_string())
                }
            )))
        );
    }

    #[test]
    fn plan_approval_enter_on_empty_line_never_accepts() {
        let state = state_with_plan_approval("");
        let ui = TuiUiState::default();
        // The safe default must never land on Accept; an empty line is a no-op.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            None
        );
    }

    #[test]
    fn queue_control_inactive_during_pending_plan_approval() {
        let mut state = state_with_plan_approval("");
        state.queued_follow_ups = vec![QueuedFollowUpView {
            id: "q1".to_string(),
            prompt: "later".to_string(),
            created_at: "t".to_string(),
            status: QueuedFollowUpStatus::Pending,
            pause_reason: None,
        }];
        assert!(!queue_control_active(&state, &TuiUiState::default()));
    }

    #[test]
    fn governance_decision_card_lines_show_intent_agent_scope_and_risk() {
        let theme = TuiUiState::default().theme;
        let lines = governance_decision_card_lines(&governance_decision_view("gov-1"), &theme);
        let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(text.contains("Refactor the config loader"));
        assert!(text.contains("Split the loader"));
        assert!(text.contains("fixer"));
        assert!(text.contains("src/config"));
        // Risk is an explicit tier word, not color alone.
        assert!(text.contains("Risk:"));
        assert!(text.contains("High"));
    }

    #[test]
    fn governance_decision_card_renders_in_frame_under_no_color() {
        let state = state_with_governance_decision("");
        let no_color_ui = TuiUiState {
            theme: Theme::resolve(TerminalCaps {
                no_color: true,
                truecolor: false,
            }),
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &no_color_ui, 100, 24);
        assert!(text.contains("Refactor the config loader"));
        assert!(text.contains("fixer"));
        assert!(text.contains("src/config"));
        assert!(text.contains("High"));
    }

    fn render_to_text(state: &AppState, width: u16, height: u16) -> String {
        render_to_text_with_ui(state, &TuiUiState::default(), width, height)
    }

    fn ui_state_with_cursor_at_end(input: &str) -> TuiUiState {
        TuiUiState {
            input_cursor: input_char_count(input),
            ..TuiUiState::default()
        }
    }

    fn ui_state_with_skills_at_end(input: &str) -> TuiUiState {
        TuiUiState {
            input_cursor: input_char_count(input),
            skill_suggestions: test_skill_suggestions(),
            ..TuiUiState::default()
        }
    }

    fn test_skill_suggestions() -> Vec<SkillSuggestion> {
        vec![
            test_skill_suggestion("project-alpha", SkillSourceTag::Project, ".agents/skills"),
            test_skill_suggestion(
                "personal-beta",
                SkillSourceTag::Personal,
                "~/.agents/skills",
            ),
        ]
    }

    fn test_skill_suggestion(
        alias: &str,
        source_tag: SkillSourceTag,
        source_origin: &str,
    ) -> SkillSuggestion {
        SkillSuggestion {
            alias: alias.to_string(),
            display_name: alias.to_string(),
            description: None,
            source_tag,
            source_origin: source_origin.to_string(),
            canonical_id: format!("{source_origin}/{alias}/SKILL.md"),
            skill_dir: PathBuf::from(format!("{source_origin}/{alias}")),
            source_path: PathBuf::from(format!("{source_origin}/{alias}/SKILL.md")),
        }
    }

    fn suggestion_aliases(suggestions: &[SkillSuggestion]) -> Vec<(String, String)> {
        suggestions
            .iter()
            .map(|suggestion| (suggestion.alias.clone(), suggestion.source_origin.clone()))
            .collect()
    }

    fn write_skill(root: &Path, directory: &str, name: &str) {
        write_skill_with_body(root, directory, name, "");
    }

    fn write_skill_with_body(root: &Path, directory: &str, name: &str, body: &str) {
        let skill_dir = root.join(directory);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test skill\n---\n{body}\n"),
        )
        .unwrap();
    }

    fn write_skill_without_name(root: &Path, directory: &str) {
        let skill_dir = root.join(directory);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\ndescription: test skill\n---\n",
        )
        .unwrap();
    }

    fn state_with_agent_roster(input: &str) -> AppState {
        let mut state = state_with_input(input, false);
        state.agents = vec![
            agent_view("explorer", "Explorer", "idle", &["read"]),
            agent_view("fixer", "Fixer", "idle", &["read", "edit", "verify"]),
            agent_view("archived", "Archived", "disabled", &["read"]),
        ];
        state
    }

    fn agent_view(id: &str, name: &str, status: &str, capabilities: &[&str]) -> AgentView {
        AgentView {
            id: id.to_string(),
            name: name.to_string(),
            runtime: "fake".to_string(),
            model: "default".to_string(),
            effort: "medium".to_string(),
            thinking: false,
            capabilities: capabilities
                .iter()
                .map(|capability| (*capability).to_string())
                .collect(),
            availability: None,
            status: status.to_string(),
        }
    }

    fn render_to_text_with_ui(
        state: &AppState,
        ui_state: &TuiUiState,
        width: u16,
        height: u16,
    ) -> String {
        let mut ui_state = ui_state.clone();
        render_to_text_with_ui_mut(state, &mut ui_state, width, height)
    }

    fn render_to_text_with_ui_mut(
        state: &AppState,
        ui_state: &mut TuiUiState,
        width: u16,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, state, ui_state))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    fn render_to_lines_with_ui_mut(
        state: &AppState,
        ui_state: &mut TuiUiState,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, state, ui_state))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect()
    }

    fn char_column(value: &str, needle: &str) -> usize {
        let byte_index = value.find(needle).unwrap();
        value[..byte_index].chars().count()
    }

    fn default_config_status() -> ConfigStatusView {
        ConfigStatusView {
            summary: "Config: sources=1 preset=none warnings=0".to_string(),
            sources: vec!["built-in defaults".to_string()],
            preset: None,
            warnings: Vec::new(),
            approval_mode: crate::config::ApprovalMode::Yolo,
            execution_graph_enabled: false,
            max_parallel_agent_steps: 2,
        }
    }

    fn clarification_option(id: &str, label: &str) -> ClarificationOption {
        ClarificationOption {
            id: id.to_string(),
            label: label.to_string(),
            description: None,
        }
    }

    fn clarification_view(options: Vec<ClarificationOption>) -> PendingClarificationView {
        PendingClarificationView {
            run_id: "run".to_string(),
            question_id: "q1".to_string(),
            question: "Test question".to_string(),
            options,
            recommended_option_id: None,
            multi_select: false,
        }
    }

    #[tokio::test]
    async fn clarification_up_key_cycles_options() {
        let mut ui_state = TuiUiState {
            clarification_option_index: 1,
            input_cursor: 3,
            ..Default::default()
        };

        let mut app_state = state_with_input("draft", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Option 1"),
            clarification_option("opt2", "Option 2"),
            clarification_option("opt3", "Option 3"),
        ]));

        let command = key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Up));
        assert_eq!(
            command,
            Some(TuiCommand::Clarification(
                ClarificationCommand::PreviousOption
            ))
        );

        let (sender, mut receiver) = mpsc::channel(1);
        execute_tui_command(&mut app_state, &mut ui_state, &sender, command.unwrap())
            .await
            .unwrap();

        assert_eq!(ui_state.clarification_option_index, 0);
        assert_eq!(ui_state.input_cursor, 3);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn clarification_down_key_cycles_options() {
        let mut ui_state = TuiUiState {
            clarification_option_index: 0,
            input_cursor: 2,
            ..Default::default()
        };

        let mut app_state = state_with_input("draft", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Option 1"),
            clarification_option("opt2", "Option 2"),
        ]));

        let command = key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Down));
        assert_eq!(
            command,
            Some(TuiCommand::Clarification(ClarificationCommand::NextOption))
        );

        let (sender, mut receiver) = mpsc::channel(1);
        execute_tui_command(&mut app_state, &mut ui_state, &sender, command.unwrap())
            .await
            .unwrap();

        assert_eq!(ui_state.clarification_option_index, 1);
        assert_eq!(ui_state.input_cursor, 2);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn clarification_character_input_updates_custom_answer() {
        let mut ui_state = TuiUiState::default();
        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![]));

        let command =
            key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Char('t')));
        assert_eq!(command, Some(TuiCommand::ClarificationInputCharacter('t')));

        let (sender, mut receiver) = mpsc::channel(1);
        execute_tui_command(&mut app_state, &mut ui_state, &sender, command.unwrap())
            .await
            .unwrap();

        assert_eq!(ui_state.clarification_custom_answer, "t");
        assert!(app_state.input.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn clarification_backspace_removes_character() {
        let mut ui_state = TuiUiState {
            clarification_custom_answer: "test".to_string(),
            ..Default::default()
        };

        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![]));

        let command =
            key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Backspace));
        assert_eq!(command, Some(TuiCommand::ClarificationInputBackspace));

        let (sender, mut receiver) = mpsc::channel(1);
        execute_tui_command(&mut app_state, &mut ui_state, &sender, command.unwrap())
            .await
            .unwrap();

        assert_eq!(ui_state.clarification_custom_answer, "tes");
        assert!(app_state.input.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn clarification_enter_on_option_dispatches_answer_with_metadata() {
        let mut ui_state = TuiUiState {
            clarification_option_index: 1,
            ..Default::default()
        };

        let mut app_state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("opt1", "Option 1"),
            clarification_option("opt2", "Option 2"),
        ]);
        view.recommended_option_id = Some("opt2".to_string());
        app_state.pending_clarification = Some(view);

        let command = key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(TuiCommand::Clarification(ClarificationCommand::Submit))
        );

        let (sender, mut receiver) = mpsc::channel(1);
        execute_tui_command(&mut app_state, &mut ui_state, &sender, command.unwrap())
            .await
            .unwrap();

        let queued = receiver.try_recv().unwrap();
        let AppWorkerCommand::Event(AppEvent::ClarificationAnswered(answer)) = queued else {
            panic!("expected clarification answer event, got {queued:?}");
        };
        assert_eq!(answer.question_id, "q1");
        assert_eq!(answer.answer, "Option 2");
        assert_eq!(answer.selected_option_id.as_deref(), Some("opt2"));
        assert_eq!(answer.selected_option_label.as_deref(), Some("Option 2"));
        assert_eq!(answer.answer_source, "recommended");
        assert_eq!(ui_state.clarification_option_index, 0);
        assert!(ui_state.clarification_custom_answer.is_empty());
    }

    #[tokio::test]
    async fn clarification_enter_with_custom_text_dispatches_custom_answer() {
        // Focus the custom row (index == options.len()); the custom answer is
        // submitted because that row is focused.
        let mut ui_state = TuiUiState {
            clarification_option_index: 2,
            clarification_custom_answer: "  my own answer  ".to_string(),
            ..Default::default()
        };

        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Option 1"),
            clarification_option("opt2", "Option 2"),
        ]));

        let (sender, mut receiver) = mpsc::channel(1);
        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();

        let queued = receiver.try_recv().unwrap();
        let AppWorkerCommand::Event(AppEvent::ClarificationAnswered(answer)) = queued else {
            panic!("expected clarification answer event, got {queued:?}");
        };
        assert_eq!(answer.answer, "my own answer");
        assert_eq!(answer.answer_source, "custom");
        assert!(answer.selected_option_id.is_none());
        assert!(answer.selected_option_label.is_none());
        assert!(ui_state.clarification_custom_answer.is_empty());
        assert_eq!(ui_state.clarification_option_index, 0);
    }

    #[test]
    fn enter_with_pending_approval_routes_to_approval_not_clarification() {
        let yes_state = state_with_input("yes", true);
        let no_state = state_with_input("no", true);
        let ui_state = TuiUiState::default();

        assert!(yes_state.pending_clarification.is_none());
        assert_eq!(
            key_event_to_tui_command_with_ui(&yes_state, &ui_state, key(KeyCode::Enter)),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(
                ApprovalResolution::ApproveOnce
            )))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(&no_state, &ui_state, key(KeyCode::Enter)),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(
                ApprovalResolution::Deny
            )))
        );
    }

    #[tokio::test]
    async fn clarification_movement_emits_no_app_event_until_enter() {
        let mut ui_state = TuiUiState::default();
        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Option 1"),
            clarification_option("opt2", "Option 2"),
        ]));

        let (sender, mut receiver) = mpsc::channel(4);
        // Typing moves focus onto the custom row; FocusOption(0) returns focus to
        // a real option so the trailing Submit has a concrete answer to send.
        for command in [
            TuiCommand::Clarification(ClarificationCommand::NextOption),
            TuiCommand::Clarification(ClarificationCommand::PreviousOption),
            TuiCommand::ClarificationInputCharacter('h'),
            TuiCommand::ClarificationInputBackspace,
            TuiCommand::Clarification(ClarificationCommand::FocusOption(0)),
        ] {
            execute_tui_command(&mut app_state, &mut ui_state, &sender, command)
                .await
                .unwrap();
            assert!(receiver.try_recv().is_err());
        }

        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();

        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::ClarificationAnswered(_))
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn renders_clarification_question_options_and_custom_field_at_80x24() {
        let mut state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("opt1", "Feature scope"),
            clarification_option("opt2", "Bug fix scope"),
        ]);
        view.recommended_option_id = Some("opt1".to_string());
        state.pending_clarification = Some(view);
        let mut ui_state = TuiUiState::default();

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 80, 24);

        let question_row = lines
            .iter()
            .position(|line| line.contains("Test question"))
            .unwrap();
        // Focus defaults to the recommended option (opt1).
        let selected_row = lines
            .iter()
            .position(|line| line.contains("❯") && line.contains("Feature scope"))
            .unwrap();
        let other_row = lines
            .iter()
            .position(|line| line.contains("Bug fix scope"))
            .unwrap();
        let custom_row = lines
            .iter()
            .position(|line| line.contains("Custom"))
            .unwrap();
        assert!(question_row < selected_row);
        assert!(selected_row < other_row);
        assert!(other_row < custom_row);
        // Single-select options are numbered.
        assert!(lines[selected_row].contains("1. Feature scope"));
        assert!(lines[other_row].contains("2. Bug fix scope"));
        assert!(lines[selected_row].contains("★ recommended"));
        assert!(!lines[other_row].contains("❯"));
        assert!(lines.join("\n").contains("Ctrl-C interrupt"));
    }

    #[test]
    fn renders_recommended_marker_distinct_from_selection_marker() {
        let mut state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("opt1", "Feature scope"),
            clarification_option("opt2", "Bug fix scope"),
        ]);
        view.recommended_option_id = Some("opt1".to_string());
        state.pending_clarification = Some(view);
        // Pre-sync the question id so the explicit focus on opt2 is preserved
        // through the in-render reset.
        let mut ui_state = TuiUiState {
            clarification_question_id: Some("q1".to_string()),
            clarification_option_index: 1,
            ..Default::default()
        };

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 80, 24);

        let recommended_row = lines
            .iter()
            .position(|line| line.contains("★ recommended"))
            .unwrap();
        let selected_row = lines
            .iter()
            .position(|line| line.contains("❯") && line.contains("Bug fix scope"))
            .unwrap();
        assert!(lines[recommended_row].contains("Feature scope"));
        assert!(!lines[recommended_row].contains("❯"));
        assert_ne!(recommended_row, selected_row);
        assert!(!lines[selected_row].contains("★"));
    }

    #[test]
    fn renders_four_options_without_overlap_at_120x40() {
        let mut state = state_with_input("", false);
        state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Option one"),
            clarification_option("opt2", "Option two"),
            clarification_option("opt3", "Option three"),
            clarification_option("opt4", "Option four"),
        ]));
        let mut ui_state = TuiUiState::default();

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 120, 40);

        let rows = [
            "Option one",
            "Option two",
            "Option three",
            "Option four",
            "Custom",
        ]
        .iter()
        .map(|needle| {
            lines
                .iter()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("missing row: {needle}"))
        })
        .collect::<Vec<_>>();
        for pair in rows.windows(2) {
            assert!(pair[0] < pair[1], "rows must not overlap: {rows:?}");
        }
        let question_row = lines
            .iter()
            .position(|line| line.contains("Test question"))
            .unwrap();
        assert!(question_row < rows[0]);
    }

    #[test]
    fn cursor_lands_in_custom_answer_field_while_clarification_pending() {
        let mut state = state_with_input("", false);
        state.pending_clarification = Some(clarification_view(vec![
            clarification_option("opt1", "Feature scope"),
            clarification_option("opt2", "Bug fix scope"),
        ]));
        // Focus the synthetic custom row (index == options.len()) with the
        // question id pre-synced so the typed text survives the in-render reset.
        let mut ui_state = TuiUiState {
            clarification_question_id: Some("q1".to_string()),
            clarification_option_index: 2,
            clarification_custom_answer: "abc".to_string(),
            ..Default::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();

        // Composer body (5 rows: question, spacer, 2 options, custom) → height 8,
        // so the box starts at y = 24 - 8 = 16; the custom row is the 5th body row
        // (4 rows above) at y = 16 + border(1) + 4 = 21.
        // cursor col = border(1) + "❯ "(2) + "3. "(3) + "Custom: "(8) + "abc"(3) = 17
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(17, 21));
    }

    #[test]
    fn scrollbar_position_reaches_bottom_when_fully_scrolled() {
        // content 100 lines, viewport 10 → max_scroll 90.
        // ratatui parks the thumb at the bottom only at position content_len-1.
        assert_eq!(scrollbar_position(90, 90, 100), 99);
        assert_eq!(scrollbar_position(0, 90, 100), 0);
        let mid = scrollbar_position(45, 90, 100);
        assert!(
            mid > 0 && mid < 99,
            "mid-scroll thumb must be interior: {mid}"
        );
        // Monotonic and clamped.
        assert!(scrollbar_position(30, 90, 100) <= scrollbar_position(60, 90, 100));
        assert_eq!(scrollbar_position(200, 90, 100), 99);
        // Degenerate: nothing to scroll.
        assert_eq!(scrollbar_position(0, 0, 1), 0);
    }

    #[tokio::test]
    async fn multi_select_toggles_and_submits_joined_answer() {
        let mut ui_state = TuiUiState::default();
        let mut app_state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
            clarification_option("c", "Gamma"),
        ]);
        view.multi_select = true;
        app_state.pending_clarification = Some(view);

        let (sender, mut receiver) = mpsc::channel(4);
        for command in [
            TuiCommand::Clarification(ClarificationCommand::FocusOption(0)),
            TuiCommand::Clarification(ClarificationCommand::ToggleOption),
            TuiCommand::Clarification(ClarificationCommand::FocusOption(2)),
            TuiCommand::Clarification(ClarificationCommand::ToggleOption),
        ] {
            execute_tui_command(&mut app_state, &mut ui_state, &sender, command)
                .await
                .unwrap();
            assert!(receiver.try_recv().is_err());
        }
        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();

        match receiver.try_recv().unwrap() {
            AppWorkerCommand::Event(AppEvent::ClarificationAnswered(answer)) => {
                assert_eq!(answer.answer_source, "multi");
                assert_eq!(answer.answer, "Alpha; Gamma");
                assert_eq!(answer.selected_option_id.as_deref(), Some("a"));
                assert_eq!(answer.selected_option_label.as_deref(), Some("Alpha"));
            }
            other => panic!("unexpected worker command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_multi_select_submit_falls_back_to_focused_option() {
        // With nothing checked and no custom text, Enter is never a dead-end:
        // it submits the focused option (as a single "recommended" choice).
        let mut ui_state = TuiUiState {
            clarification_option_index: 1,
            ..Default::default()
        };
        let mut app_state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        app_state.pending_clarification = Some(view);

        let (sender, mut receiver) = mpsc::channel(4);
        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();
        match receiver.try_recv().unwrap() {
            AppWorkerCommand::Event(AppEvent::ClarificationAnswered(answer)) => {
                assert_eq!(answer.answer, "Beta");
                assert_eq!(answer.answer_source, "recommended");
                assert_eq!(answer.selected_option_id.as_deref(), Some("b"));
            }
            other => panic!("unexpected worker command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_multi_select_submit_on_custom_row_does_not_emit() {
        // Focused on the (empty) custom row with nothing checked → no answer.
        let mut ui_state = TuiUiState {
            clarification_option_index: 2,
            ..Default::default()
        };
        let mut app_state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        app_state.pending_clarification = Some(view);

        let (sender, mut receiver) = mpsc::channel(4);
        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn single_checked_multi_select_reports_recommended_source() {
        let mut ui_state = TuiUiState {
            clarification_selected: BTreeSet::from([1]),
            ..Default::default()
        };
        let mut app_state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        app_state.pending_clarification = Some(view);

        let (sender, mut receiver) = mpsc::channel(4);
        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();
        match receiver.try_recv().unwrap() {
            AppWorkerCommand::Event(AppEvent::ClarificationAnswered(answer)) => {
                // Exactly one box checked, no custom text → not "multi".
                assert_eq!(answer.answer, "Beta");
                assert_eq!(answer.answer_source, "recommended");
                assert_eq!(answer.selected_option_label.as_deref(), Some("Beta"));
            }
            other => panic!("unexpected worker command: {other:?}"),
        }
    }

    #[test]
    fn multi_select_pre_checks_recommended_option_on_arrival() {
        let mut state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        view.recommended_option_id = Some("b".to_string());
        state.pending_clarification = Some(view);
        // Fresh ui_state (question_id None) → sync treats this as a new question.
        let mut ui_state = TuiUiState::default();

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 80, 24);
        let joined = lines.join("\n");
        // The recommended option is pre-checked and focused, so Enter confirms it.
        assert!(
            joined.contains("[x] Beta"),
            "recommended pre-checked: {joined}"
        );
        assert!(ui_state.clarification_selected.contains(&1));
    }

    #[test]
    fn multi_select_does_not_pre_check_unknown_recommended_option() {
        let mut state = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        // A recommended id that matches no option must not pre-check anything.
        view.recommended_option_id = Some("unknown".to_string());
        state.pending_clarification = Some(view);
        let mut ui_state = TuiUiState::default();

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 80, 24);
        let joined = lines.join("\n");

        // Nothing is pre-checked when the recommended id resolves to no option,
        // and focus still falls back to the first row.
        assert!(
            !joined.contains("[x]"),
            "no option should be pre-checked: {joined}"
        );
        assert!(ui_state.clarification_selected.is_empty());
        assert_eq!(ui_state.clarification_option_index, 0);
    }

    #[tokio::test]
    async fn second_enter_after_submit_does_not_queue_duplicate_answer() {
        let mut ui_state = TuiUiState::default();
        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]));

        let (sender, mut receiver) = mpsc::channel(4);
        // First submit dispatches the answer and arms the submitting gate.
        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::ClarificationAnswered(_))
        ));
        assert!(ui_state.clarification_submitting);

        // A second Enter while the clarification is still pending is swallowed
        // (only Ctrl-C falls through), so no duplicate answer is queued.
        let command = key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Enter));
        assert_eq!(command, None);
        assert!(receiver.try_recv().is_err());

        // Once the worker clears the pending clarification, the gate releases.
        app_state.pending_clarification = None;
        sync_clarification_state(&app_state, &mut ui_state);
        assert!(!ui_state.clarification_submitting);
    }

    #[tokio::test]
    async fn typing_focuses_custom_and_submits_custom_answer() {
        let mut ui_state = TuiUiState::default();
        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]));

        let (sender, mut receiver) = mpsc::channel(4);
        for ch in ['h', 'i'] {
            execute_tui_command(
                &mut app_state,
                &mut ui_state,
                &sender,
                TuiCommand::ClarificationInputCharacter(ch),
            )
            .await
            .unwrap();
        }
        // Typing moves focus onto the synthetic custom row.
        assert_eq!(ui_state.clarification_option_index, 2);

        execute_tui_command(
            &mut app_state,
            &mut ui_state,
            &sender,
            TuiCommand::Clarification(ClarificationCommand::Submit),
        )
        .await
        .unwrap();

        match receiver.try_recv().unwrap() {
            AppWorkerCommand::Event(AppEvent::ClarificationAnswered(answer)) => {
                assert_eq!(answer.answer_source, "custom");
                assert_eq!(answer.answer, "hi");
                assert!(answer.selected_option_id.is_none());
            }
            other => panic!("unexpected worker command: {other:?}"),
        }
    }

    #[test]
    fn digit_key_focuses_option_during_clarification() {
        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
            clarification_option("c", "Gamma"),
        ]));
        let ui_state = TuiUiState::default();

        assert_eq!(
            key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Char('2'))),
            Some(TuiCommand::Clarification(
                ClarificationCommand::FocusOption(1)
            ))
        );
        // Out-of-range digits are ignored rather than typed into the custom field.
        assert_eq!(
            key_event_to_tui_command_with_ui(&app_state, &ui_state, key(KeyCode::Char('9'))),
            None
        );
    }

    #[test]
    fn space_toggles_in_multi_select_but_is_ignored_in_single_select() {
        let ui_state = TuiUiState::default();

        let mut multi = state_with_input("", false);
        let mut view = clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        multi.pending_clarification = Some(view);
        assert_eq!(
            key_event_to_tui_command_with_ui(&multi, &ui_state, key(KeyCode::Char(' '))),
            Some(TuiCommand::Clarification(
                ClarificationCommand::ToggleOption
            ))
        );

        let mut single = state_with_input("", false);
        single.pending_clarification = Some(clarification_view(vec![
            clarification_option("a", "Alpha"),
            clarification_option("b", "Beta"),
        ]));
        assert_eq!(
            key_event_to_tui_command_with_ui(&single, &ui_state, key(KeyCode::Char(' '))),
            None
        );
    }

    #[test]
    fn multi_select_renders_checkboxes_and_descriptions() {
        let mut state = state_with_input("", false);
        let mut view = clarification_view(vec![
            ClarificationOption {
                id: "a".to_string(),
                label: "Alpha".to_string(),
                description: Some("The first path".to_string()),
            },
            clarification_option("b", "Beta"),
        ]);
        view.multi_select = true;
        state.pending_clarification = Some(view);
        let mut ui_state = TuiUiState {
            clarification_question_id: Some("q1".to_string()),
            clarification_selected: BTreeSet::from([0]),
            ..Default::default()
        };

        let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 80, 24);
        let joined = lines.join("\n");

        assert!(joined.contains("[x] Alpha"), "checked option: {joined}");
        assert!(joined.contains("[ ] Beta"), "unchecked option: {joined}");
        assert!(joined.contains("The first path"), "description: {joined}");
        assert!(joined.contains("Space toggle"), "multi hint: {joined}");
    }

    #[test]
    fn pending_approval_rendering_shows_no_clarification_labels() {
        let state = state_with_input("", true);

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("Approval required for fixer"));
        assert!(!text.contains("Clarifying question"));
        assert!(!text.contains("Custom:"));
        assert!(!text.contains("★ recommended"));
    }

    #[tokio::test]
    async fn fake_runtime_clarification_renders_chat_context_and_composer_controls() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let mut app = App::new(config).await.unwrap();
        app.submit_prompt("needs clarification create a feature")
            .await
            .unwrap();
        let view = app.state().pending_clarification.clone().unwrap();
        assert_eq!(view.options.len(), 3);

        let mut ui_state = TuiUiState::default();
        let text = render_to_text_with_ui_mut(app.state(), &mut ui_state, 120, 40);

        // Chat context from the projection (status badge is chat-only).
        assert!(text.contains("waiting for clarification"));
        // Composer answer controls.
        assert!(text.contains("Which target or constraint should guide this run?"));
        assert!(text.contains("1. Clarify the target scope"));
        // Per-option descriptions are surfaced in the composer.
        assert!(text.contains("Specify the file, workflow, or product area to prioritize."));
        assert!(text.contains("★ recommended"));
        assert!(text.contains("Custom"));
        assert!(text.contains("Ctrl-C interrupt"));
    }

    #[test]
    fn clarification_chat_kind_label_is_distinct_from_approval() {
        assert_eq!(
            chat_kind_label(&ChatItemKind::Clarification),
            "clarification"
        );
        assert_eq!(chat_kind_label(&ChatItemKind::Approval), "approval");
        assert_ne!(
            chat_kind_label(&ChatItemKind::Clarification),
            chat_kind_label(&ChatItemKind::Approval)
        );
    }

    #[test]
    fn governance_decision_chat_kind_label_is_governance() {
        assert_eq!(
            chat_kind_label(&ChatItemKind::GovernanceDecision),
            "governance"
        );
    }

    #[test]
    fn ctrl_c_still_works_during_clarification() {
        let ui_state = TuiUiState::default();
        let mut app_state = state_with_input("", false);
        app_state.pending_clarification = Some(PendingClarificationView {
            run_id: "run".to_string(),
            question_id: "q1".to_string(),
            question: "Test question".to_string(),
            options: vec![],
            recommended_option_id: None,
            multi_select: false,
        });

        let command = key_event_to_tui_command_with_ui(
            &app_state,
            &ui_state,
            key_with_modifiers(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            command,
            Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested))
        );
    }

    fn key(code: KeyCode) -> KeyEvent {
        key_with_modifiers(code, KeyModifiers::NONE)
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::empty(),
        }
    }

    fn queue_view(
        id: &str,
        prompt: &str,
        status: QueuedFollowUpStatus,
        pause_reason: Option<&str>,
    ) -> QueuedFollowUpView {
        QueuedFollowUpView {
            id: id.to_string(),
            prompt: prompt.to_string(),
            created_at: "2026-06-11T00:00:00.000Z".to_string(),
            status,
            pause_reason: pause_reason.map(str::to_string),
        }
    }

    fn state_with_queue(items: Vec<QueuedFollowUpView>) -> AppState {
        let mut state = state_with_input("", false);
        state.queued_follow_ups = items;
        state
    }

    #[test]
    fn queue_panel_renders_single_pending_item_with_count() {
        let state = state_with_queue(vec![queue_view(
            "q1",
            "update the docs",
            QueuedFollowUpStatus::Pending,
            None,
        )]);

        let text = render_to_text(&state, 100, 30);

        assert!(text.contains("Queue (1)"));
        assert!(text.contains("update the docs"));
        assert!(text.contains("pending"));
    }

    #[test]
    fn queue_panel_preserves_fifo_display_order() {
        let state = state_with_queue(vec![
            queue_view("q1", "first item", QueuedFollowUpStatus::Pending, None),
            queue_view("q2", "second item", QueuedFollowUpStatus::Pending, None),
        ]);

        let text = render_to_text(&state, 100, 30);

        assert!(text.contains("Queue (2)"));
        let first = text.find("first item").expect("first item rendered");
        let second = text.find("second item").expect("second item rendered");
        assert!(first < second, "queue items should render in FIFO order");
    }

    #[test]
    fn queue_panel_renders_paused_reason() {
        let state = state_with_queue(vec![queue_view(
            "q1",
            "blocked item",
            QueuedFollowUpStatus::Paused,
            Some("run is waiting for clarification"),
        )]);

        let text = render_to_text(&state, 120, 30);

        assert!(text.contains("paused"));
        assert!(text.contains("blocked item"));
        assert!(text.contains("run is waiting for clarification"));
    }

    #[test]
    fn queue_panel_distinguishes_replaying_item() {
        let state = state_with_queue(vec![
            queue_view("q1", "running item", QueuedFollowUpStatus::Replaying, None),
            queue_view("q2", "waiting item", QueuedFollowUpStatus::Pending, None),
        ]);

        let text = render_to_text(&state, 100, 30);

        assert!(text.contains("replaying"));
        assert!(text.contains("pending"));
        assert!(text.contains("running item"));
        assert!(text.contains("waiting item"));
    }

    #[test]
    fn delete_key_cancels_selected_queue_item() {
        let state = state_with_queue(vec![queue_view(
            "q1",
            "cancel me",
            QueuedFollowUpStatus::Pending,
            None,
        )]);
        let ui_state = TuiUiState::default();

        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui_state, key(KeyCode::Delete)),
            Some(TuiCommand::Dispatch(AppEvent::FollowUpCancelled(
                "q1".to_string()
            )))
        );
    }

    #[test]
    fn ctrl_r_resumes_selected_paused_item() {
        let state = state_with_queue(vec![queue_view(
            "q1",
            "paused item",
            QueuedFollowUpStatus::Paused,
            Some("previous run failed"),
        )]);
        let ui_state = TuiUiState::default();

        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                key_with_modifiers(KeyCode::Char('r'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::Dispatch(AppEvent::FollowUpResumeRequested(
                "q1".to_string()
            )))
        );
    }

    #[test]
    fn ctrl_r_does_not_resume_pending_item() {
        let state = state_with_queue(vec![queue_view(
            "q1",
            "pending item",
            QueuedFollowUpStatus::Pending,
            None,
        )]);
        let ui_state = TuiUiState::default();

        // Ctrl-R resumes only PAUSED items; on a pending item it does NOT resume —
        // it falls through to opening the session browser (task_07). The key point
        // is that no FollowUpResumeRequested is produced for a pending item.
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            key_with_modifiers(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            command,
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Open))
        );
    }

    #[tokio::test]
    async fn queue_navigation_selects_and_cancel_targets_selected_item() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_queue(vec![
            queue_view("q1", "first", QueuedFollowUpStatus::Pending, None),
            queue_view("q2", "second", QueuedFollowUpStatus::Pending, None),
        ]);
        let mut ui_state = TuiUiState::default();

        // Down navigates the queue selection without dispatching an event.
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui_state, key(KeyCode::Down)),
            Some(TuiCommand::QueueSelection(QueueSelectionCommand::Next))
        );
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::QueueSelection(QueueSelectionCommand::Next),
        )
        .await
        .unwrap();
        assert_eq!(ui_state.queue_selection_index, 1);
        assert!(receiver.try_recv().is_err());

        // Delete now cancels the selected (second) item.
        let command = key_event_to_tui_command_with_ui(&state, &ui_state, key(KeyCode::Delete))
            .expect("delete dispatches a cancel");
        assert_eq!(
            command,
            TuiCommand::Dispatch(AppEvent::FollowUpCancelled("q2".to_string()))
        );
        execute_tui_command(&mut state, &mut ui_state, &sender, command)
            .await
            .unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::FollowUpCancelled(id)) if id == "q2"
        ));
    }

    #[test]
    fn agent_and_skill_dropdown_routing_unchanged_when_queue_control_inactive() {
        // Queued items present, but a non-empty `/agent:` input keeps the agent
        // dropdown active and queue control inactive.
        let mut agent_state = state_with_agent_roster("/agent:");
        agent_state.queued_follow_ups = vec![queue_view(
            "q1",
            "queued",
            QueuedFollowUpStatus::Pending,
            None,
        )];
        let agent_ui = ui_state_with_cursor_at_end(&agent_state.input);
        assert_eq!(
            key_event_to_tui_command_with_ui(&agent_state, &agent_ui, key(KeyCode::Up)),
            Some(TuiCommand::AgentDropdown(DropdownCommand::Previous))
        );

        let mut skill_state = state_with_input("/skill:", false);
        skill_state.queued_follow_ups = vec![queue_view(
            "q1",
            "queued",
            QueuedFollowUpStatus::Pending,
            None,
        )];
        let skill_ui = ui_state_with_skills_at_end(&skill_state.input);
        assert_eq!(
            key_event_to_tui_command_with_ui(&skill_state, &skill_ui, key(KeyCode::Down)),
            Some(TuiCommand::SkillDropdown(DropdownCommand::Next))
        );
    }

    #[test]
    fn queue_control_inactive_while_composing_input() {
        let mut state = state_with_input("typing a message", false);
        state.queued_follow_ups = vec![queue_view(
            "q1",
            "queued",
            QueuedFollowUpStatus::Pending,
            None,
        )];
        let ui_state = ui_state_with_cursor_at_end(&state.input);

        // While composing (non-empty input) queue focus is inactive, so Delete is
        // not a cancel and Up/Down move the input cursor.
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui_state, key(KeyCode::Delete)),
            None
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(&state, &ui_state, key(KeyCode::Up)),
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Up))
        );
    }

    // ── task_03 migration guards ──

    /// The single-source-of-color invariant: no inline color literal may appear
    /// in any `src/tui/*.rs` file except `theme.rs`. The needle is assembled at
    /// runtime so this very file does not contain the literal it forbids.
    #[test]
    fn colors_live_only_in_theme_module() {
        let needle = concat!("Color", "::");
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui");
        let mut theme_had_needle = false;
        let mut scanned = 0;
        for entry in fs::read_dir(dir).expect("src/tui is readable") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            scanned += 1;
            let content = fs::read_to_string(&path).unwrap();
            let is_theme = path.file_name().and_then(|name| name.to_str()) == Some("theme.rs");
            if is_theme {
                theme_had_needle = content.contains(needle);
            } else {
                assert!(
                    !content.contains(needle),
                    "inline color literal found outside theme.rs in {}",
                    path.display()
                );
            }
        }
        assert!(scanned >= 2, "expected to scan mod.rs and theme.rs");
        assert!(
            theme_had_needle,
            "theme.rs should define the color literals (guards against a wrong scan path)"
        );
    }

    #[test]
    fn status_style_maps_to_semantic_tokens() {
        let theme = TuiUiState::default().theme;

        let running = status_style(&theme, "running");
        assert_eq!(running.fg, Some(theme.status_ok));
        assert!(running.add_modifier.contains(Modifier::BOLD));

        let disabled = status_style(&theme, "disabled");
        assert_eq!(disabled.fg, Some(theme.text_dim));
    }

    // --- task_05: activity_glyph / activity_label vocabulary (ADR-002) --------

    #[test]
    fn activity_glyph_uses_set_1_unicode() {
        assert_eq!(activity_glyph(ActivityState::Active, false), "◐");
        assert_eq!(activity_glyph(ActivityState::NeedsInput, false), "◔");
        assert_eq!(activity_glyph(ActivityState::Stalled, false), "○");
        assert_eq!(activity_glyph(ActivityState::Idle, false), "·");
    }

    #[test]
    fn activity_glyph_ascii_fallback() {
        assert_eq!(activity_glyph(ActivityState::Active, true), ">");
        assert_eq!(activity_glyph(ActivityState::NeedsInput, true), "?");
        assert_eq!(activity_glyph(ActivityState::Stalled, true), "!");
        assert_eq!(activity_glyph(ActivityState::Idle, true), ".");
    }

    #[test]
    fn activity_label_vocabulary() {
        assert_eq!(activity_label(ActivityState::Active), "working");
        assert_eq!(activity_label(ActivityState::NeedsInput), "waiting");
        assert_eq!(activity_label(ActivityState::Stalled), "stalled?");
        assert_eq!(activity_label(ActivityState::Idle), "idle");
    }

    #[test]
    fn activity_labels_are_distinct_and_non_empty() {
        let labels = [
            activity_label(ActivityState::Active),
            activity_label(ActivityState::NeedsInput),
            activity_label(ActivityState::Stalled),
            activity_label(ActivityState::Idle),
        ];
        assert!(labels.iter().all(|label| !label.is_empty()));
        let unique: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "labels must be pairwise distinct"
        );
    }

    #[test]
    fn activity_glyphs_are_single_portable_bmp_chars() {
        // ADR-002: portable BMP glyphs only — no emoji-presentation or
        // double-width characters, so each state reads on a constrained terminal.
        for state in [
            ActivityState::Active,
            ActivityState::NeedsInput,
            ActivityState::Stalled,
            ActivityState::Idle,
        ] {
            for ascii in [false, true] {
                let glyph = activity_glyph(state.clone(), ascii);
                assert_eq!(glyph.chars().count(), 1, "single char: {glyph:?}");
                let ch = glyph.chars().next().unwrap();
                assert!((ch as u32) <= 0xFFFF, "BMP only: {glyph:?}");
            }
        }
    }

    #[test]
    fn severity_badge_error_uses_status_error_background() {
        let theme = TuiUiState::default().theme;
        let badge = severity_badge_style(&theme, &ChatSeverity::Error);
        assert_eq!(badge.bg, Some(theme.status_error));
    }

    /// The migration is color-only: text content is identical whether the theme
    /// resolves to truecolor or to the `NO_COLOR` (terminal-default) tier.
    #[test]
    fn no_color_render_matches_truecolor_text_content() {
        let mut state = state_with_agent_roster("draft prompt");
        populate_roster_rows(&mut state);
        let truecolor = render_to_text(&state, 80, 24);

        let no_color_ui = TuiUiState {
            theme: Theme::resolve(TerminalCaps {
                no_color: true,
                truecolor: false,
            }),
            ..TuiUiState::default()
        };
        let no_color = render_to_text_with_ui(&state, &no_color_ui, 80, 24);

        assert_eq!(truecolor, no_color);
        assert!(no_color.contains("Explorer"));
    }

    // ── task_04 welcome screen ──

    fn user_prompt_chat_item(text: &str) -> ChatItemView {
        ChatItemView {
            id: format!("u:{text}"),
            lifecycle_key: None,
            kind: ChatItemKind::UserPrompt,
            status: crate::app::chat::ChatItemStatus::Completed,
            severity: ChatSeverity::Info,
            title: "You".to_string(),
            summary: None,
            body: vec![ChatLineView {
                style: ChatLineStyle::Plain,
                text: text.to_string(),
            }],
            details: Vec::new(),
            source: crate::app::chat::ChatSourceRef {
                event_ids: Vec::new(),
                run_id: None,
                step_id: None,
                action_id: None,
            },
            updated_at: String::new(),
        }
    }

    #[test]
    fn welcome_item_renders_facts_and_replaces_empty_state() {
        let mut state = state_with_agent_roster("");
        state.chat_items = vec![ChatItemView::welcome()];

        let text = render_to_text(&state, 80, 24);

        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "welcome facts version present"
        );
        assert!(text.contains("Atelier"), "wordmark present");
        assert!(!text.contains("No chat yet."), "empty state replaced");
    }

    #[test]
    fn welcome_shows_routing_onboarding_hint_on_empty_chat() {
        // task_08: a newcomer on an empty chat (only the synthetic Welcome item)
        // sees the routing hint pointing through the orchestrator to /help.
        let mut state = state_with_agent_roster("");
        state.chat_items = vec![ChatItemView::welcome()];

        let text = render_to_text(&state, 80, 24);

        assert!(
            text.contains("orchestrator"),
            "routing onboarding hint visible in welcome area"
        );
        assert!(
            text.contains("type /help for commands"),
            "existing /help cue retained beside the hint"
        );
    }

    #[test]
    fn welcome_persists_above_later_chat_items() {
        let mut state = state_with_agent_roster("");
        state.chat_items = vec![
            ChatItemView::welcome(),
            user_prompt_chat_item("hello world"),
        ];
        let ui_state = TuiUiState {
            event_follow: false,
            event_scroll: 0,
            ..TuiUiState::default()
        };

        let text = render_to_text_with_ui(&state, &ui_state, 80, 24);

        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "welcome facts still visible at the top of scrollback"
        );
        assert!(text.contains("hello world"), "later chat item rendered too");
    }

    // ── task_05 git poller / worker lifecycle ──

    fn fake_worker_config(dir: &Path) -> EffectiveConfig {
        let config_path = dir.join("multiagent.toml");
        fs::write(
            &config_path,
            "[runtimes.fake]\ntype = \"fake\"\n\n[agents.orchestrator]\nruntime = \"fake\"\n",
        )
        .unwrap();
        crate::config::load_effective_config(crate::config::ConfigLoadOptions {
            working_directory: dir.to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn worker_shuts_down_promptly_with_git_poller_active() {
        let dir = tempdir().unwrap();
        let app = App::new(fake_worker_config(dir.path())).await.unwrap();
        let (sender, receiver) = mpsc::channel(8);
        let (file_index_sender, _file_index_receiver) = watch::channel(Vec::<FileEntry>::new());
        let worker = tokio::spawn(run_app_worker(
            app,
            receiver,
            file_index_sender,
            Some(dir.path().to_path_buf()),
        ));

        // The 5s poll lives in the worker's select loop; a shutdown must win
        // immediately rather than waiting for a tick (no hang).
        let result =
            tokio::time::timeout(Duration::from_secs(2), shutdown_app_worker(sender, worker)).await;
        assert!(
            result.is_ok(),
            "worker shutdown hung with the poller active"
        );
        result.unwrap().unwrap();
    }

    // ── task_04 background file-index acquisition ──

    /// A never-set cancellation flag for refresh tests that don't exercise the
    /// shutdown path.
    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn file_index_refresh_interval_has_expected_default() {
        assert_eq!(FILE_INDEX_REFRESH_INTERVAL, Duration::from_secs(15));
    }

    #[tokio::test]
    async fn refresh_file_index_publishes_walk_snapshot() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "readme").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        let (sender, receiver) = watch::channel(Vec::<FileEntry>::new());
        refresh_file_index(Some(dir.path()), &sender, no_cancel()).await;

        let published = receiver.borrow().clone();
        assert_eq!(published, FileIndex::walk(dir.path()));
        // The snapshot is non-empty and contains a known file.
        assert!(published.iter().any(|entry| entry.rel_path == "README.md"));
    }

    #[tokio::test]
    async fn refresh_file_index_is_noop_without_working_directory() {
        let (sender, receiver) = watch::channel(vec![FileEntry {
            rel_path: "kept.rs".to_string(),
            is_dir: false,
            mtime: UNIX_EPOCH,
            depth: 1,
        }]);
        refresh_file_index(None, &sender, no_cancel()).await;
        // No working directory → the previous snapshot is left untouched.
        assert_eq!(receiver.borrow().len(), 1);
        assert_eq!(receiver.borrow()[0].rel_path, "kept.rs");
    }

    #[tokio::test]
    async fn refresh_file_index_honors_cancellation() {
        // A pre-set cancel flag makes the spawned walk bail immediately, so the
        // refresh publishes an empty snapshot rather than scanning the tree —
        // the mechanism that keeps quit from blocking on a large workspace.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/b.rs"), "fn b() {}").unwrap();

        let (sender, receiver) = watch::channel(Vec::<FileEntry>::new());
        refresh_file_index(Some(dir.path()), &sender, Arc::new(AtomicBool::new(true))).await;
        assert!(receiver.borrow().is_empty());
    }

    #[tokio::test]
    async fn refresh_file_index_surfaces_files_created_between_walks() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("first.rs"), "fn main() {}").unwrap();

        let (sender, receiver) = watch::channel(Vec::<FileEntry>::new());
        refresh_file_index(Some(dir.path()), &sender, no_cancel()).await;
        assert!(receiver
            .borrow()
            .iter()
            .all(|entry| entry.rel_path != "second.rs"));

        // A file created mid-session appears on the next refresh.
        fs::write(dir.path().join("second.rs"), "fn second() {}").unwrap();
        refresh_file_index(Some(dir.path()), &sender, no_cancel()).await;
        assert!(receiver
            .borrow()
            .iter()
            .any(|entry| entry.rel_path == "second.rs"));
    }

    // ── task_04 prompt-history recall load ──

    fn write_prompt_history_session(root: &Path, session: &str, prompts: &[(&str, &str)]) {
        // Write a `.atelier/sessions/<session>/events.jsonl` of prompt_submitted
        // events with explicit (timestamp, prompt), as a real run records them.
        let dir = root.join(".atelier/sessions").join(session);
        fs::create_dir_all(&dir).unwrap();
        let mut contents = String::new();
        for (timestamp, prompt) in prompts {
            let mut event = HistoryEvent::new(
                "session",
                None,
                None,
                "prompt_submitted",
                json!({ "prompt": prompt }),
            );
            event.timestamp = (*timestamp).to_string();
            contents.push_str(&serde_json::to_string(&event).unwrap());
            contents.push('\n');
        }
        fs::write(dir.join("events.jsonl"), contents).unwrap();
    }

    #[test]
    fn default_ui_state_initializes_prompt_history_fields() {
        let ui_state = TuiUiState::default();
        assert!(ui_state.prompt_history.is_empty());
        assert_eq!(ui_state.prompt_history_cursor, 0);
        assert!(ui_state.prompt_history_draft.is_empty());
    }

    #[test]
    fn sync_prompt_history_adopts_published_ring() {
        let (sender, mut receiver) = watch::channel(Vec::<String>::new());
        let mut ui_state = TuiUiState::default();
        sender.send(vec!["b".to_string(), "a".to_string()]).unwrap();

        sync_prompt_history(&mut ui_state, &mut receiver);
        assert_eq!(
            ui_state.prompt_history,
            vec!["b".to_string(), "a".to_string()]
        );
    }

    #[test]
    fn disabled_recall_skips_load_and_leaves_ring_empty() {
        // Gate off: nothing is spawned, the channel never publishes, and the ring
        // stays empty after a sync tick (a closed channel reads as "no change").
        let (sender, mut receiver) = watch::channel(Vec::<String>::new());
        let spawned = maybe_spawn_prompt_history_load(
            false,
            Some(PathBuf::from("/nonexistent")),
            200,
            sender,
        );
        assert!(!spawned);

        let mut ui_state = TuiUiState::default();
        sync_prompt_history(&mut ui_state, &mut receiver);
        assert!(ui_state.prompt_history.is_empty());
    }

    #[tokio::test]
    async fn refresh_prompt_history_publishes_recall_newest_first() {
        // Integration: two prompt_submitted events on disk → the load publishes
        // them newest-first, and a sync tick adopts them into the ring.
        let dir = tempdir().unwrap();
        write_prompt_history_session(
            dir.path(),
            "s",
            &[
                ("2026-06-06T00:00:01.000Z", "alpha"),
                ("2026-06-06T00:00:02.000Z", "beta"),
            ],
        );

        let (sender, mut receiver) = watch::channel(Vec::<String>::new());
        refresh_prompt_history(Some(dir.path()), 200, &sender).await;

        let mut ui_state = TuiUiState::default();
        sync_prompt_history(&mut ui_state, &mut receiver);
        assert_eq!(
            ui_state.prompt_history,
            vec!["beta".to_string(), "alpha".to_string()]
        );
    }

    #[tokio::test]
    async fn enabled_recall_load_spawns_and_publishes_ring() {
        let dir = tempdir().unwrap();
        write_prompt_history_session(dir.path(), "s", &[("2026-06-06T00:00:01.000Z", "alpha")]);

        let (sender, mut receiver) = watch::channel(Vec::<String>::new());
        let spawned =
            maybe_spawn_prompt_history_load(true, Some(dir.path().to_path_buf()), 200, sender);
        assert!(spawned);

        // The detached load publishes exactly once; await that and assert it landed.
        receiver.changed().await.unwrap();
        assert_eq!(*receiver.borrow(), vec!["alpha".to_string()]);
    }

    // ── prompt-history recall interaction (task_05) ──

    fn ui_state_with_history(history: &[&str]) -> TuiUiState {
        TuiUiState {
            prompt_history: history.iter().map(|s| (*s).to_string()).collect(),
            // Wide so single-word entries stay on one visual row (cursor on the
            // top/bottom boundary), isolating recall from cursor-nav.
            input_width: 80,
            ..TuiUiState::default()
        }
    }

    #[test]
    fn up_down_walk_recall_ring_newest_first() {
        let mut state = state_with_input("", false);
        let mut ui_state = ui_state_with_history(&["b", "a"]); // newest-first

        // ↑ → newest "b", cursor parked at the end, depth 1.
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "b");
        assert_eq!(ui_state.input_cursor, 1);
        assert_eq!(ui_state.prompt_history_cursor, 1);

        // ↑ again → older "a".
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "a");

        // ↓ → newer "b".
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Down
        ));
        assert_eq!(state.input, "b");
    }

    #[test]
    fn up_at_oldest_entry_is_consumed_noop() {
        let mut state = state_with_input("", false);
        let mut ui_state = ui_state_with_history(&["b", "a"]);
        try_recall_history(&mut ui_state, &mut state, InputCursorCommand::Up); // "b"
        try_recall_history(&mut ui_state, &mut state, InputCursorCommand::Up); // "a" (oldest)
        assert_eq!(state.input, "a");

        // A further ↑ is consumed (so it never moves the cursor) but is a no-op.
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "a");
        assert_eq!(ui_state.prompt_history_cursor, 2);
    }

    #[test]
    fn draft_is_saved_on_entry_and_restored_past_newest() {
        let mut state = state_with_input("draft", false);
        let mut ui_state = TuiUiState {
            prompt_history: vec!["b".to_string()],
            input_width: 80,
            input_cursor: input_char_count("draft"),
            ..TuiUiState::default()
        };

        // ↑ saves the in-progress draft and shows the newest entry.
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "b");
        assert_eq!(ui_state.prompt_history_draft, "draft");

        // ↓ past the newest entry restores the exact draft and cursor.
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Down
        ));
        assert_eq!(state.input, "draft");
        assert_eq!(ui_state.prompt_history_cursor, 0);
        assert_eq!(ui_state.input_cursor, input_char_count("draft"));
    }

    #[test]
    fn wrapped_draft_moves_cursor_before_recalling_at_top_row() {
        // width 5, "abcdefgh" wraps to two rows: row 0 "abcde", row 1 "fgh".
        let mut state = state_with_input("abcdefgh", false);
        let mut ui_state = TuiUiState {
            prompt_history: vec!["recalled".to_string()],
            input_width: 5,
            input_cursor: 7, // col 2 of row 1
            ..TuiUiState::default()
        };

        // On row 1, ↑ is NOT recall — it yields so the cursor moves up a row.
        assert!(!try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "abcdefgh"); // draft untouched
        move_input_cursor(&mut ui_state, &state.input, InputCursorCommand::Up);
        assert_eq!(ui_state.input_cursor / 5, 0); // now on the top row

        // Only at the top row does a further ↑ recall.
        assert!(try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "recalled");
        assert_eq!(ui_state.prompt_history_draft, "abcdefgh");
    }

    #[test]
    fn empty_ring_does_not_recall() {
        // No history (e.g. recall disabled → loader never populated the ring).
        let mut state = state_with_input("hello", false);
        let mut ui_state = TuiUiState {
            input_cursor: input_char_count("hello"),
            input_width: 80,
            ..TuiUiState::default()
        };
        assert!(!try_recall_history(
            &mut ui_state,
            &mut state,
            InputCursorCommand::Up
        ));
        assert_eq!(state.input, "hello");
    }

    #[test]
    fn up_drives_queue_not_recall_when_queue_focused() {
        // Empty input + a queued follow-up → queue focus owns ↑ (precedence),
        // even though the ring is non-empty, so recall is never reached.
        let mut state = state_with_queue(vec![queue_view(
            "q1",
            "do it",
            QueuedFollowUpStatus::Pending,
            None,
        )]);
        state.input.clear();
        let ui_state = ui_state_with_history(&["b", "a"]);

        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        );
        assert_eq!(
            command,
            Some(TuiCommand::QueueSelection(QueueSelectionCommand::Previous))
        );
    }

    #[test]
    fn up_drives_command_dropdown_not_recall_when_open() {
        // "/g" is a single slash-word → the command dropdown owns ↑ (precedence).
        let state = state_with_input("/g", false);
        let ui_state = TuiUiState {
            prompt_history: vec!["b".to_string(), "a".to_string()],
            input_cursor: 2,
            ..TuiUiState::default()
        };
        assert!(command_dropdown(&state, &ui_state).is_some());

        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        );
        assert!(matches!(command, Some(TuiCommand::CommandDropdown(_))));
    }

    #[tokio::test]
    async fn recall_then_enter_submits_recalled_text_to_worker() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("", false);
        let mut ui_state = ui_state_with_history(&["b", "a"]);

        // ↑ through the real command handler recalls the newest entry.
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::MoveInputCursor(InputCursorCommand::Up),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "b");

        // Enter routes to a submit of the recalled text.
        let submit = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();
        execute_tui_command(&mut state, &mut ui_state, &sender, submit)
            .await
            .unwrap();

        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(prompt, _)) if prompt == "b"
        ));
        // Submitting cleared the composer and reset recall to a fresh draft.
        assert!(state.input.is_empty());
        assert_eq!(ui_state.prompt_history_cursor, 0);
    }

    // ── prompt-history provenance + in-session ring (task_06) ──

    #[test]
    fn in_session_prepend_dedups_caps_and_skips_disabled_and_leading_space() {
        let mut ui_state = TuiUiState {
            prompt_history_max: 2,
            ..TuiUiState::default()
        };
        record_in_session_prompt(&mut ui_state, "a");
        record_in_session_prompt(&mut ui_state, "b");
        assert_eq!(
            ui_state.prompt_history,
            vec!["b".to_string(), "a".to_string()]
        );

        // A consecutive duplicate of the front is ignored.
        record_in_session_prompt(&mut ui_state, "b");
        assert_eq!(
            ui_state.prompt_history,
            vec!["b".to_string(), "a".to_string()]
        );

        // Cap respected (max 2): adding "c" drops the oldest "a".
        record_in_session_prompt(&mut ui_state, "c");
        assert_eq!(
            ui_state.prompt_history,
            vec!["c".to_string(), "b".to_string()]
        );

        // Leading-space and empty submissions never enter the ring.
        record_in_session_prompt(&mut ui_state, " secret");
        record_in_session_prompt(&mut ui_state, "   ");
        assert_eq!(
            ui_state.prompt_history,
            vec!["c".to_string(), "b".to_string()]
        );

        // Disabled recall → no prepend, the ring stays empty.
        let mut disabled = TuiUiState {
            prompt_history_enabled: false,
            ..TuiUiState::default()
        };
        record_in_session_prompt(&mut disabled, "x");
        assert!(disabled.prompt_history.is_empty());
    }

    #[tokio::test]
    async fn submit_from_recall_tags_recalled() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("a", false);
        let mut ui_state = TuiUiState {
            prompt_history: vec!["a".to_string(), "b".to_string()],
            prompt_history_cursor: 2, // composition originated from the ring
            input_cursor: 1,
            ..TuiUiState::default()
        };

        let submit = TuiCommand::Dispatch(AppEvent::PromptSubmitted(
            "a".to_string(),
            PromptSource::Fresh, // placeholder; the handler finalizes provenance
        ));
        execute_tui_command(&mut state, &mut ui_state, &sender, submit)
            .await
            .unwrap();

        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(p, PromptSource::Recalled)) if p == "a"
        ));
    }

    #[tokio::test]
    async fn submit_freshly_typed_tags_fresh() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("typed", false);
        // cursor 0 → a fresh live draft.
        let mut ui_state = ui_state_with_cursor_at_end("typed");

        let submit = TuiCommand::Dispatch(AppEvent::PromptSubmitted(
            "typed".to_string(),
            PromptSource::Fresh,
        ));
        execute_tui_command(&mut state, &mut ui_state, &sender, submit)
            .await
            .unwrap();

        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(p, PromptSource::Fresh)) if p == "typed"
        ));
    }

    #[tokio::test]
    async fn recall_then_clear_to_empty_then_retype_tags_fresh() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("", false);
        let mut ui_state = ui_state_with_history(&["b", "a"]);

        // ↑ recalls "b" (cursor → 1).
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::MoveInputCursor(InputCursorCommand::Up),
        )
        .await
        .unwrap();
        assert_eq!(ui_state.prompt_history_cursor, 1);

        // Backspacing the recalled text to empty resets the cursor to a draft.
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::InputBackspace,
        )
        .await
        .unwrap();
        assert!(state.input.is_empty());
        assert_eq!(ui_state.prompt_history_cursor, 0);

        // Type fresh text and submit → Fresh (only the submit reaches the worker).
        for ch in "new".chars() {
            execute_tui_command(
                &mut state,
                &mut ui_state,
                &sender,
                TuiCommand::InputCharacter(ch),
            )
            .await
            .unwrap();
        }
        let submit = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();
        execute_tui_command(&mut state, &mut ui_state, &sender, submit)
            .await
            .unwrap();

        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(p, PromptSource::Fresh)) if p == "new"
        ));
    }

    #[tokio::test]
    async fn submit_prepends_to_in_session_ring_and_resets_cursor() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("gamma", false);
        let mut ui_state = ui_state_with_history(&["b", "a"]);
        ui_state.input_cursor = input_char_count("gamma");

        let submit = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();
        execute_tui_command(&mut state, &mut ui_state, &sender, submit)
            .await
            .unwrap();

        // "gamma" is prepended newest-first; the cursor resets and input clears.
        assert_eq!(
            ui_state.prompt_history,
            vec!["gamma".to_string(), "b".to_string(), "a".to_string()]
        );
        assert_eq!(ui_state.prompt_history_cursor, 0);
        assert!(state.input.is_empty());
    }

    // ── prompt-history discoverability hint + help (task_07) ──

    #[test]
    fn recall_hint_shows_with_empty_input_and_history() {
        let state = state_with_input("", false); // Idle → work not active
        let ui_state = ui_state_with_history(&["b", "a"]);
        let text = render_to_text_with_ui(&state, &ui_state, 120, 24);
        assert!(text.contains("↑ recall"));
    }

    #[test]
    fn recall_hint_hidden_with_nonempty_input() {
        let state = state_with_input("typing", false);
        let ui_state = ui_state_with_history(&["b", "a"]);
        let text = render_to_text_with_ui(&state, &ui_state, 120, 24);
        assert!(!text.contains("↑ recall"));
        assert!(text.contains("/help"));
    }

    #[test]
    fn recall_hint_hidden_with_empty_history() {
        let state = state_with_input("", false);
        let ui_state = TuiUiState::default(); // empty ring
        let text = render_to_text_with_ui(&state, &ui_state, 120, 24);
        assert!(!text.contains("↑ recall"));
        assert!(text.contains("/help"));
    }

    #[test]
    fn recall_hint_suppressed_while_work_active() {
        let mut state = state_with_input("", false);
        state.run_state = RunState::Running; // the work indicator wins
        let ui_state = ui_state_with_history(&["b", "a"]);
        let text = render_to_text_with_ui(&state, &ui_state, 120, 24);
        assert!(!text.contains("↑ recall"));
    }

    #[test]
    fn help_overlay_documents_recall_keys() {
        let state = state_with_input("", false);
        let ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Keys,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);
        assert!(text.contains("recall recent prompts"));
    }

    // ── composer line-editing: cursor jumps + kills (config-driven-keybindings task_02) ──

    fn ui_state_with_cursor(cursor: usize) -> TuiUiState {
        TuiUiState {
            input_cursor: cursor,
            ..TuiUiState::default()
        }
    }

    #[test]
    fn kill_to_line_end_deletes_suffix_and_keeps_cursor() {
        let mut state = state_with_input("hello world", false);
        let mut ui = ui_state_with_cursor(5); // just after "hello"
        kill_input(&mut state, &mut ui, InputKillCommand::ToLineEnd);
        assert_eq!(state.input, "hello");
        assert_eq!(ui.input_cursor, 5);
    }

    #[test]
    fn kill_to_line_start_deletes_prefix_and_moves_cursor_to_zero() {
        let mut state = state_with_input("hello world", false);
        let mut ui = ui_state_with_cursor(6); // just before "world"
        kill_input(&mut state, &mut ui, InputKillCommand::ToLineStart);
        assert_eq!(state.input, "world");
        assert_eq!(ui.input_cursor, 0);
    }

    #[test]
    fn kill_word_back_deletes_word_and_trailing_spaces() {
        let mut state = state_with_input("foo bar ", false);
        let mut ui = ui_state_with_cursor(input_char_count("foo bar ")); // 8, at end
        kill_input(&mut state, &mut ui, InputKillCommand::WordBack);
        // The trailing space + the word "bar" are both removed.
        assert_eq!(state.input, "foo ");
        assert_eq!(ui.input_cursor, 4);
    }

    #[test]
    fn line_start_and_line_end_move_cursor_without_changing_text() {
        let input = "hello";
        let mut ui = ui_state_with_cursor(2);
        move_input_cursor(&mut ui, input, InputCursorCommand::LineEnd);
        assert_eq!(ui.input_cursor, input_char_count(input));
        move_input_cursor(&mut ui, input, InputCursorCommand::LineStart);
        assert_eq!(ui.input_cursor, 0);
    }

    #[test]
    fn kills_are_utf8_safe_for_multibyte_input() {
        let text = "héllo🚀 wörld";
        let mut state = state_with_input(text, false);
        let mut ui = ui_state_with_cursor(input_char_count(text)); // 12, at end
        kill_input(&mut state, &mut ui, InputKillCommand::WordBack);
        assert_eq!(state.input, "héllo🚀 ");
        assert_eq!(ui.input_cursor, 7);
        // Cursor (char index) must still map to a valid byte boundary at the end.
        assert_eq!(
            byte_index_for_char(&state.input, ui.input_cursor),
            state.input.len()
        );
    }

    #[test]
    fn kills_are_noops_at_edges_and_on_empty_input() {
        // Empty input: every kill is a no-op.
        for cmd in [
            InputKillCommand::ToLineEnd,
            InputKillCommand::ToLineStart,
            InputKillCommand::WordBack,
        ] {
            let mut state = state_with_input("", false);
            let mut ui = ui_state_with_cursor(0);
            kill_input(&mut state, &mut ui, cmd);
            assert_eq!(state.input, "");
            assert_eq!(ui.input_cursor, 0);
        }

        // ToLineEnd with the cursor already at the end is a no-op.
        let mut state = state_with_input("abc", false);
        let mut ui = ui_state_with_cursor(3);
        kill_input(&mut state, &mut ui, InputKillCommand::ToLineEnd);
        assert_eq!(state.input, "abc");
        assert_eq!(ui.input_cursor, 3);

        // ToLineStart with the cursor at 0 is a no-op.
        let mut ui = ui_state_with_cursor(0);
        kill_input(&mut state, &mut ui, InputKillCommand::ToLineStart);
        assert_eq!(state.input, "abc");
        assert_eq!(ui.input_cursor, 0);
    }

    #[tokio::test]
    async fn line_start_then_kill_to_end_clears_the_line_through_the_handler() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("hello world", false);
        let mut ui = ui_state_with_cursor_at_end(&state.input);

        execute_tui_command(
            &mut state,
            &mut ui,
            &sender,
            TuiCommand::MoveInputCursor(InputCursorCommand::LineStart),
        )
        .await
        .unwrap();
        assert_eq!(ui.input_cursor, 0);

        execute_tui_command(
            &mut state,
            &mut ui,
            &sender,
            TuiCommand::InputKill(InputKillCommand::ToLineEnd),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "");
        assert_eq!(ui.input_cursor, 0);
    }

    // ── task_05 TUI file-index state and consumer ──

    fn file_entry(rel_path: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            rel_path: rel_path.to_string(),
            is_dir,
            mtime: UNIX_EPOCH,
            depth: rel_path.split('/').count(),
        }
    }

    #[test]
    fn default_ui_state_initializes_file_mention_fields() {
        let ui_state = TuiUiState::default();
        assert!(ui_state.file_mention_entries.is_empty());
        assert_eq!(ui_state.file_mention_selection_index, 0);
        assert_eq!(ui_state.file_mention_dropdown_dismissed, None);
    }

    #[test]
    fn reset_dropdown_selections_resets_file_mention_selection() {
        let mut ui_state = TuiUiState {
            file_mention_selection_index: 3,
            ..Default::default()
        };
        reset_dropdown_selections(&mut ui_state);
        assert_eq!(ui_state.file_mention_selection_index, 0);
    }

    #[test]
    fn sync_file_index_adopts_published_snapshot() {
        let (sender, mut receiver) = watch::channel(Vec::<FileEntry>::new());
        let mut ui_state = TuiUiState::default();
        let snapshot = vec![file_entry("src/main.rs", false), file_entry("src", true)];
        sender.send(snapshot.clone()).unwrap();

        sync_file_index(&mut ui_state, &mut receiver);
        assert_eq!(ui_state.file_mention_entries, snapshot);
    }

    #[test]
    fn content_edit_clears_file_mention_dismissal_but_cursor_move_does_not() {
        let mut state = state_with_input("@mod", false);
        let mut ui_state = TuiUiState {
            input_cursor: 4,
            file_mention_dropdown_dismissed: Some("@mod".to_string()),
            ..Default::default()
        };

        // A cursor move preserves the Escape dismissal.
        move_input_cursor(&mut ui_state, &state.input, InputCursorCommand::Left);
        assert_eq!(
            ui_state.file_mention_dropdown_dismissed,
            Some("@mod".to_string())
        );

        // A content edit (insert) clears it so discovery re-activates.
        insert_input_character(&mut state, &mut ui_state, 'x');
        assert_eq!(ui_state.file_mention_dropdown_dismissed, None);
    }

    #[test]
    fn backspace_clears_file_mention_dismissal() {
        let mut state = state_with_input("@mod", false);
        let mut ui_state = TuiUiState {
            input_cursor: 4,
            file_mention_dropdown_dismissed: Some("@mod".to_string()),
            ..Default::default()
        };
        remove_input_character_before_cursor(&mut state, &mut ui_state);
        assert_eq!(ui_state.file_mention_dropdown_dismissed, None);
    }

    // ── task_06 file-mention dropdown model and activation ──

    fn file_entry_at(rel_path: &str, is_dir: bool, mtime_secs: u64) -> FileEntry {
        FileEntry {
            rel_path: rel_path.to_string(),
            is_dir,
            mtime: UNIX_EPOCH + Duration::from_secs(mtime_secs),
            depth: rel_path.split('/').count(),
        }
    }

    /// A representative seeded index with distinct mtimes for recency tests.
    fn seeded_file_entries() -> Vec<FileEntry> {
        vec![
            file_entry_at("src/tui/mod.rs", false, 50),
            file_entry_at("src/runtime/claude.rs", false, 40),
            file_entry_at("src/runtime/mod.rs", false, 30),
            file_entry_at("README.md", false, 20),
            file_entry_at("src", true, 10),
        ]
    }

    fn ui_state_with_file_entries(input: &str, entries: Vec<FileEntry>) -> TuiUiState {
        TuiUiState {
            input_cursor: input_char_count(input),
            file_mention_entries: entries,
            ..TuiUiState::default()
        }
    }

    #[test]
    fn file_mention_activates_for_token_at_cursor() {
        let state = state_with_input("see @run", false);
        let ui_state = ui_state_with_file_entries("see @run", seeded_file_entries());
        let dropdown = file_mention_dropdown(&state, &ui_state).expect("active for @run");
        assert!(!dropdown.empty);
        assert!(!dropdown.suggestions.is_empty());
        assert!(dropdown
            .suggestions
            .iter()
            .all(|s| s.rel_path.contains("runtime")));
    }

    #[test]
    fn bare_at_lists_recents_most_recent_first() {
        let state = state_with_input("@", false);
        let ui_state = ui_state_with_file_entries("@", seeded_file_entries());
        let dropdown = file_mention_dropdown(&state, &ui_state).expect("active for @");
        assert!(!dropdown.empty);
        assert_eq!(dropdown.selected, 0);
        assert_eq!(
            dropdown.suggestions.first().unwrap().rel_path,
            "src/tui/mod.rs"
        );
    }

    #[test]
    fn no_match_query_sets_empty_with_no_rows() {
        let state = state_with_input("@zzzz", false);
        let ui_state = ui_state_with_file_entries("@zzzz", seeded_file_entries());
        let dropdown = file_mention_dropdown(&state, &ui_state).expect("active no-match");
        assert!(dropdown.empty);
        assert!(dropdown.suggestions.is_empty());
    }

    #[test]
    fn cursor_outside_token_does_not_activate() {
        let state = state_with_input("@run done", false);
        // Cursor at the end sits in the "done" token, not the `@run` token.
        let ui_state = ui_state_with_file_entries("@run done", seeded_file_entries());
        assert!(file_mention_dropdown(&state, &ui_state).is_none());
    }

    #[test]
    fn pending_states_suppress_activation() {
        let entries = seeded_file_entries();
        let ui_state = ui_state_with_file_entries("@run", entries);

        // Pending approval (state_with_input sets approval + WaitingForUser).
        let approval = state_with_input("@run", true);
        assert!(file_mention_dropdown(&approval, &ui_state).is_none());

        // Pending clarification.
        let mut clarification = state_with_input("@run", false);
        clarification.pending_clarification = Some(clarification_view(vec![clarification_option(
            "a", "Option A",
        )]));
        assert!(file_mention_dropdown(&clarification, &ui_state).is_none());

        // WaitingForUser run state on its own.
        let mut waiting = state_with_input("@run", false);
        waiting.run_state = RunState::WaitingForUser;
        assert!(file_mention_dropdown(&waiting, &ui_state).is_none());
    }

    #[test]
    fn dismissed_input_suppresses_until_edited() {
        let state = state_with_input("@run", false);
        let mut ui_state = ui_state_with_file_entries("@run", seeded_file_entries());
        ui_state.file_mention_dropdown_dismissed = Some("@run".to_string());
        assert!(file_mention_dropdown(&state, &ui_state).is_none());

        // Clearing the dismissal (as a content edit does) re-activates it.
        ui_state.file_mention_dropdown_dismissed = None;
        assert!(file_mention_dropdown(&state, &ui_state).is_some());
    }

    #[test]
    fn second_at_token_activates_for_token_at_cursor() {
        let input = "a @one b @mod";
        let state = state_with_input(input, false);
        let ui_state = ui_state_with_file_entries(input, seeded_file_entries());
        let dropdown = file_mention_dropdown(&state, &ui_state).expect("active for 2nd @");
        assert_eq!(dropdown.token.query, "mod");
        assert!(dropdown
            .suggestions
            .iter()
            .any(|s| s.rel_path == "src/tui/mod.rs"));
    }

    #[test]
    fn file_mention_model_produces_ranked_suggestions() {
        let state = state_with_input("@mod", false);
        let ui_state = ui_state_with_file_entries("@mod", seeded_file_entries());
        let dropdown = file_mention_dropdown(&state, &ui_state).expect("active for @mod");
        let paths: Vec<&str> = dropdown
            .suggestions
            .iter()
            .map(|s| s.rel_path.as_str())
            .collect();
        // Both mod.rs files match; equal depth/score, so the more recent wins.
        assert_eq!(paths.first(), Some(&"src/tui/mod.rs"));
        assert!(paths.contains(&"src/runtime/mod.rs"));
        // Highlights are present for fuzzy matches.
        assert!(dropdown
            .suggestions
            .iter()
            .all(|s| !s.match_indices.is_empty()));
    }

    // ── task_07 file-mention interaction and insertion ──

    fn file_dropdown_with(empty: bool) -> FileMentionDropdown {
        FileMentionDropdown {
            token: PromptToken {
                value_start: 1,
                value_end: 4,
                query: "mod".to_string(),
            },
            suggestions: if empty {
                Vec::new()
            } else {
                vec![FileSuggestion {
                    rel_path: "src/tui/mod.rs".to_string(),
                    is_dir: false,
                    match_indices: vec![4, 5, 6],
                }]
            },
            selected: 0,
            empty,
        }
    }

    #[test]
    fn file_mention_key_mapping_matches_spec() {
        let dropdown = file_dropdown_with(false);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        use FileMentionDropdownCommand::{Accept, Dismiss, Next, Previous};
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Up)),
            Some(TuiCommand::FileMentionDropdown(Previous))
        );
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Down)),
            Some(TuiCommand::FileMentionDropdown(Next))
        );
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Tab)),
            Some(TuiCommand::FileMentionDropdown(Accept))
        );
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Enter)),
            Some(TuiCommand::FileMentionDropdown(Accept))
        );
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Esc)),
            Some(TuiCommand::FileMentionDropdown(Dismiss))
        );
    }

    #[test]
    fn file_mention_no_match_does_not_trap_enter_but_esc_dismisses() {
        let dropdown = file_dropdown_with(true);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        // Enter falls through (None) so the prompt submits normally.
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Enter)),
            None
        );
        // Up/Down also fall through with no selectable rows.
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Up)),
            None
        );
        // Esc still dismisses.
        assert_eq!(
            file_mention_dropdown_key_command(&dropdown, key(KeyCode::Esc)),
            Some(TuiCommand::FileMentionDropdown(
                FileMentionDropdownCommand::Dismiss
            ))
        );
    }

    #[test]
    fn file_mention_selection_wraps_at_both_ends() {
        let mut state = state_with_input("@mod", false);
        let mut ui_state = ui_state_with_file_entries("@mod", seeded_file_entries());
        // Exactly two matches (src/tui/mod.rs, src/runtime/mod.rs).
        assert_eq!(
            file_mention_dropdown(&state, &ui_state)
                .unwrap()
                .suggestions
                .len(),
            2
        );

        // Previous from 0 wraps to the last row.
        apply_file_mention_dropdown_command(
            &mut state,
            &mut ui_state,
            FileMentionDropdownCommand::Previous,
        );
        assert_eq!(ui_state.file_mention_selection_index, 1);
        // Next from the last row wraps back to the first.
        apply_file_mention_dropdown_command(
            &mut state,
            &mut ui_state,
            FileMentionDropdownCommand::Next,
        );
        assert_eq!(ui_state.file_mention_selection_index, 0);
    }

    #[tokio::test]
    async fn file_mention_accept_consumes_at_and_inserts_bare_path() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("see @run", false);
        let mut ui_state = ui_state_with_file_entries("see @run", seeded_file_entries());

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::FileMentionDropdown(FileMentionDropdownCommand::Accept),
        )
        .await
        .unwrap();

        // The `@` is gone, a bare path is inserted, a trailing space added, and
        // the cursor follows — with surrounding text intact. No app event.
        assert_eq!(state.input, "see src/runtime/claude.rs ");
        assert_eq!(
            ui_state.input_cursor,
            input_char_count("see src/runtime/claude.rs ")
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn file_mention_accept_folder_gets_trailing_slash() {
        let mut state = state_with_input("@src", false);
        let mut ui_state = ui_state_with_file_entries("@src", Vec::new());
        let token = active_prompt_token("@src", 4, FILE_MENTION_PREFIX).unwrap();
        let folder = FileSuggestion {
            rel_path: "src/tui".to_string(),
            is_dir: true,
            match_indices: Vec::new(),
        };
        apply_file_mention_suggestion(&mut state, &mut ui_state, &token, &folder);
        assert_eq!(state.input, "src/tui/ ");
        assert_eq!(ui_state.input_cursor, input_char_count("src/tui/ "));
    }

    #[tokio::test]
    async fn file_mention_esc_records_input_in_dismissal() {
        let (sender, _receiver) = mpsc::channel(1);
        let mut state = state_with_input("@run", false);
        let mut ui_state = ui_state_with_file_entries("@run", seeded_file_entries());

        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::FileMentionDropdown(FileMentionDropdownCommand::Dismiss),
        )
        .await
        .unwrap();

        assert_eq!(
            ui_state.file_mention_dropdown_dismissed,
            Some("@run".to_string())
        );
    }

    #[tokio::test]
    async fn file_mention_accept_and_continue_supports_second_reference() {
        let (sender, mut receiver) = mpsc::channel(2);
        let input = "look at @mod";
        let mut state = state_with_input(input, false);
        let mut ui_state = ui_state_with_file_entries(input, seeded_file_entries());

        // Navigate to the second match, then accept.
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::FileMentionDropdown(FileMentionDropdownCommand::Next),
        )
        .await
        .unwrap();
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::FileMentionDropdown(FileMentionDropdownCommand::Accept),
        )
        .await
        .unwrap();
        assert_eq!(state.input, "look at src/runtime/mod.rs ");
        assert!(receiver.try_recv().is_err());

        // Typing a second `@` afterward re-opens the picker at the new cursor.
        insert_input_character(&mut state, &mut ui_state, '@');
        assert_eq!(state.input, "look at src/runtime/mod.rs @");
        assert!(file_mention_dropdown(&state, &ui_state).is_some());
    }

    // ── task_08 render the file-mention dropdown ──

    fn file_suggestion(rel_path: &str, is_dir: bool, match_indices: Vec<u32>) -> FileSuggestion {
        FileSuggestion {
            rel_path: rel_path.to_string(),
            is_dir,
            match_indices,
        }
    }

    fn dropdown_with(
        suggestions: Vec<FileSuggestion>,
        selected: usize,
        empty: bool,
    ) -> FileMentionDropdown {
        FileMentionDropdown {
            token: PromptToken {
                value_start: 1,
                value_end: 1,
                query: String::new(),
            },
            suggestions,
            selected,
            empty,
        }
    }

    /// Draw only the file-mention dropdown onto a test backend and return its
    /// rows. The dropdown opens upward above an input area near the bottom.
    fn render_file_dropdown_rows(
        dropdown: &FileMentionDropdown,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = TuiUiState::default().theme;
        let input_area = Rect {
            x: 0,
            y: height.saturating_sub(INPUT_COMPOSER_HEIGHT),
            width,
            height: INPUT_BOX_HEIGHT,
        };
        terminal
            .draw(|frame| render_file_mention_dropdown(frame, input_area, dropdown, &theme))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect()
    }

    #[test]
    fn render_shows_both_paths_and_the_selection_marker() {
        let dropdown = dropdown_with(
            vec![
                file_suggestion("src/tui/mod.rs", false, Vec::new()),
                file_suggestion("README.md", false, Vec::new()),
            ],
            0,
            false,
        );
        let joined = render_file_dropdown_rows(&dropdown, 60, 24).join("\n");
        assert!(joined.contains("Files"));
        assert!(joined.contains("src/tui/mod.rs"));
        assert!(joined.contains("README.md"));
        // The selected (first) row carries the `> ` marker.
        assert!(joined.contains("> src/tui/mod.rs"));
    }

    #[test]
    fn render_emphasizes_matched_characters() {
        let theme = TuiUiState::default().theme;
        let matched: std::collections::HashSet<usize> = [4usize, 5, 6].into_iter().collect();
        let spans = highlighted_path_spans(&theme, "src/tui/mod.rs", &matched, false);
        let bold: Vec<bool> = spans
            .iter()
            .map(|span| span.style.add_modifier.contains(Modifier::BOLD))
            .collect();
        for (index, is_bold) in bold.iter().enumerate() {
            assert_eq!(*is_bold, [4, 5, 6].contains(&index), "char index {index}");
        }
    }

    #[test]
    fn render_shows_folder_trailing_slash() {
        let dropdown = dropdown_with(vec![file_suggestion("src/tui", true, Vec::new())], 0, false);
        let joined = render_file_dropdown_rows(&dropdown, 60, 24).join("\n");
        assert!(joined.contains("src/tui/"));
    }

    #[test]
    fn render_no_match_shows_single_row() {
        let dropdown = dropdown_with(Vec::new(), 0, true);
        let joined = render_file_dropdown_rows(&dropdown, 60, 24).join("\n");
        assert!(joined.contains("No matching files"));
        // The no-match row only honors Esc, so the hint must not advertise the
        // Up/Down/Tab/Enter affordances that do nothing in this state.
        assert!(joined.contains("Esc"));
        assert!(!joined.contains("Tab/Enter"));
    }

    #[test]
    fn render_caps_visible_rows_at_six() {
        let suggestions: Vec<FileSuggestion> = (0..8)
            .map(|i| file_suggestion(&format!("f{i}.rs"), false, Vec::new()))
            .collect();
        let dropdown = dropdown_with(suggestions, 0, false);
        let joined = render_file_dropdown_rows(&dropdown, 60, 24).join("\n");
        let shown = (0..8)
            .filter(|i| joined.contains(&format!("f{i}.rs")))
            .count();
        assert_eq!(shown, DROPDOWN_MAX_ITEMS);
    }

    #[test]
    fn render_truncates_paths_wider_than_the_row() {
        let long = "src/very/deeply/nested/directory/structure/with/a/long/file_name.rs";
        let dropdown = dropdown_with(vec![file_suggestion(long, false, Vec::new())], 0, false);
        let joined = render_file_dropdown_rows(&dropdown, 40, 24).join("\n");
        assert!(!joined.contains(long));
        assert!(joined.contains("src/very/deeply"));
    }

    #[test]
    fn render_integration_overlay_layout_on_standard_backend() {
        let dropdown = dropdown_with(
            vec![
                file_suggestion("src/tui/mod.rs", false, vec![4, 5, 6]),
                file_suggestion("src/runtime/mod.rs", false, vec![12, 13, 14]),
            ],
            1,
            false,
        );
        let rows = render_file_dropdown_rows(&dropdown, 80, 24);
        let joined = rows.join("\n");
        assert!(joined.contains("Files"));
        assert!(joined.contains("src/tui/mod.rs"));
        // The selected second row carries the marker.
        assert!(joined.contains("> src/runtime/mod.rs"));
    }

    // ── task_09 routing/render wiring and parity ──

    #[test]
    fn routing_returns_file_mention_command_for_active_token() {
        let state = state_with_input("@run", false);
        let ui_state = ui_state_with_file_entries("@run", seeded_file_entries());
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_eq!(
            command,
            Some(TuiCommand::FileMentionDropdown(
                FileMentionDropdownCommand::Next
            ))
        );
    }

    #[test]
    fn routing_skips_file_mention_during_pending_approval() {
        // pending_approval routes through normal input before the dropdowns.
        let state = state_with_input("@run", true);
        let ui_state = ui_state_with_file_entries("@run", seeded_file_entries());
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_eq!(
            command,
            Some(TuiCommand::MoveInputCursor(InputCursorCommand::Down))
        );
    }

    #[test]
    fn render_chain_draws_file_dropdown_and_help_suppresses_it() {
        let state = state_with_input("@run", false);
        let mut ui_state = ui_state_with_file_entries("@run", seeded_file_entries());

        let text = render_to_text_with_ui(&state, &ui_state, 80, 24);
        assert!(text.contains("Files"));

        // Help takes over the screen and suppresses all dropdown rendering.
        ui_state.help_visible = true;
        let text_help = render_to_text_with_ui(&state, &ui_state, 80, 24);
        assert!(!text_help.contains("Files"));
    }

    #[tokio::test]
    async fn end_to_end_down_then_enter_inserts_path_without_dispatching_a_run() {
        let (sender, mut receiver) = mpsc::channel(2);
        let mut state = state_with_input("look at @run", false);
        let mut ui_state = ui_state_with_file_entries("look at @run", seeded_file_entries());

        // Drive the real routing: Down selects the next match.
        let down = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .expect("Down routes to the dropdown");
        execute_tui_command(&mut state, &mut ui_state, &sender, down)
            .await
            .unwrap();

        // Enter accepts (the dropdown is still active with matches).
        let enter = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .expect("Enter routes to the dropdown");
        execute_tui_command(&mut state, &mut ui_state, &sender, enter)
            .await
            .unwrap();

        assert_eq!(state.input, "look at src/runtime/mod.rs ");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn routing_and_render_chains_agree_on_activation() {
        // Whenever activation is `Some`, the render pass draws the overlay; when
        // it is `None`, it does not (the two chains stay in sync).
        for input in ["@run", "see @mod", "@zzzz", "hello world"] {
            let state = state_with_input(input, false);
            let ui_state = ui_state_with_file_entries(input, seeded_file_entries());
            let active = file_mention_dropdown(&state, &ui_state).is_some();
            let rendered = render_to_text_with_ui(&state, &ui_state, 80, 24).contains("Files");
            assert_eq!(
                active, rendered,
                "routing/render parity mismatch for {input:?}"
            );
        }
    }

    // ── task_06 status footer ──

    fn footer_text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn footer_line_shows_git_segment_when_present_and_omits_it_when_absent() {
        let theme = TuiUiState::default().theme;
        let git = GitContext {
            repo_name: "atelier".to_string(),
            head_sha: None,
            dirty: false,
            branch: "main".to_string(),
        };
        let with = footer_text(&footer_line(&theme, Some(&git), &RunState::Idle, &[], 80));
        assert!(with.contains("atelier"));
        assert!(with.contains("main"));

        let without = footer_text(&footer_line(&theme, None, &RunState::Idle, &[], 80));
        assert!(!without.contains("atelier"));
        assert!(!without.contains("main"));
        // No leading separator artifact when the git segment is omitted.
        assert!(!without.trim_start().starts_with('·'));
    }

    #[test]
    fn agent_summary_counts_running_statuses() {
        let agents = vec![
            agent_view("a", "A", "running", &[]),
            agent_view("b", "B", "idle", &[]),
            agent_view("c", "C", "streaming", &[]),
        ];
        assert_eq!(agent_summary(&agents), "3 agents · 2 running");

        let idle = vec![
            agent_view("a", "A", "idle", &[]),
            agent_view("b", "B", "idle", &[]),
            agent_view("c", "C", "idle", &[]),
        ];
        assert_eq!(agent_summary(&idle), "3 agents");
    }

    // ── session browser modal (task_07) ──

    fn browser_summary(label: &str) -> SessionSummary {
        SessionSummary {
            session_id: format!("id-{label}"),
            label: label.to_string(),
            started_at: "2026-06-17T00:00:00.000Z".to_string(),
            outcome: RunState::Completed,
            working_directory: std::path::PathBuf::from("."),
        }
    }

    #[test]
    fn session_browser_keys_route_to_filter_nav_and_close() {
        use SessionBrowserCommand as Cmd;
        // Default browser is in List mode.
        let browser = SessionBrowserState::default();
        let cmd =
            |code| session_browser_key_command(&browser, KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(
            cmd(KeyCode::Char('a')),
            Some(TuiCommand::SessionBrowser(Cmd::FilterChar('a')))
        );
        assert_eq!(cmd(KeyCode::Up), Some(TuiCommand::SessionBrowser(Cmd::Up)));
        assert_eq!(
            cmd(KeyCode::Down),
            Some(TuiCommand::SessionBrowser(Cmd::Down))
        );
        assert_eq!(
            cmd(KeyCode::Backspace),
            Some(TuiCommand::SessionBrowser(Cmd::FilterBackspace))
        );
        assert_eq!(
            cmd(KeyCode::Esc),
            Some(TuiCommand::SessionBrowser(Cmd::Close))
        );
    }

    #[test]
    fn browser_takes_precedence_over_normal_but_help_wins() {
        let state = state_with_input("hello", false);
        let mut ui = TuiUiState::default();
        ui.browser.visible = true;
        // A printable key narrows the browser filter, not the composer.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::SessionBrowser(
                SessionBrowserCommand::FilterChar('x')
            ))
        );
        // Help still wins if both are somehow set.
        ui.help_visible = true;
        assert!(!matches!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::SessionBrowser(_))
        ));
    }

    #[test]
    fn ctrl_r_opens_browser_from_normal_context() {
        let state = state_with_input("", false);
        let ui = TuiUiState::default();
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Open))
        );
    }

    #[test]
    fn browser_filter_narrows_rows_case_insensitively() {
        let mut ui = TuiUiState::default();
        ui.browser.summaries = vec![
            browser_summary("Fix the parser"),
            browser_summary("Add hooks"),
            browser_summary("PARSER cleanup"),
        ];
        ui.browser.filter = "parser".to_string();
        assert_eq!(ui.browser.filtered_indices(), vec![0, 2]);

        // Down moves within the filtered set and clamps at its end; Up returns.
        apply_session_browser_command(&mut ui, SessionBrowserCommand::Down);
        assert_eq!(ui.browser.selection_index, 1);
        apply_session_browser_command(&mut ui, SessionBrowserCommand::Down);
        assert_eq!(
            ui.browser.selection_index, 1,
            "clamped at the last filtered row"
        );
        apply_session_browser_command(&mut ui, SessionBrowserCommand::Up);
        assert_eq!(ui.browser.selection_index, 0);
    }

    #[test]
    fn sync_session_summaries_adopts_published_snapshot() {
        let (sender, mut receiver) = watch::channel(Vec::<SessionSummary>::new());
        let mut ui = TuiUiState::default();
        sender
            .send(vec![browser_summary("loaded off-thread")])
            .unwrap();
        sync_session_summaries(&mut ui, &mut receiver);
        assert_eq!(ui.browser.summaries.len(), 1);
        assert_eq!(ui.browser.summaries[0].label, "loaded off-thread");
    }

    #[test]
    fn browser_renders_summaries_newest_first() {
        let state = state_with_input("", false);
        let mut ui = TuiUiState::default();
        ui.browser.visible = true;
        ui.browser.summaries = vec![
            browser_summary("newest session"),
            browser_summary("older session"),
        ];
        let text = render_to_text_with_ui_mut(&state, &mut ui, 100, 30);
        let newest = text.find("newest session").expect("newest rendered");
        let older = text.find("older session").expect("older rendered");
        assert!(newest < older, "newest-first order in the rendered list");
    }

    // ── session preview pane (task_08) ──

    fn preview_with_items(n: usize) -> SessionPreview {
        SessionPreview {
            session_id: "s".to_string(),
            items: (0..n)
                .map(|i| chat_item(&format!("item {i}"), ChatItemKind::RunSummary))
                .collect(),
        }
    }

    #[test]
    fn sync_session_preview_drops_a_stale_session_preview() {
        let (sender, mut receiver) = watch::channel(None::<SessionPreview>);
        let mut ui = TuiUiState::default();
        ui.browser.preview_session_id = Some("current".to_string());

        // A slow preview for a previously-selected session must not overwrite.
        sender
            .send(Some(SessionPreview {
                session_id: "stale".to_string(),
                items: Vec::new(),
            }))
            .unwrap();
        sync_session_preview(&mut ui, &mut receiver);
        assert!(
            ui.browser.preview.is_none(),
            "stale preview must be dropped"
        );

        // The preview for the currently-selected session applies.
        sender
            .send(Some(SessionPreview {
                session_id: "current".to_string(),
                items: Vec::new(),
            }))
            .unwrap();
        sync_session_preview(&mut ui, &mut receiver);
        assert_eq!(
            ui.browser.preview.as_ref().map(|p| p.session_id.as_str()),
            Some("current")
        );
    }

    #[test]
    fn right_opens_preview_and_esc_returns_to_list() {
        let mut ui = TuiUiState::default();
        ui.browser.visible = true;
        ui.browser.summaries = vec![browser_summary("a session")];

        // List-mode → (Right) routes to OpenPreview (Enter is reserved for Resume).
        assert_eq!(
            session_browser_key_command(
                &ui.browser,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SessionBrowser(
                SessionBrowserCommand::OpenPreview
            ))
        );
        apply_session_browser_command(&mut ui, SessionBrowserCommand::OpenPreview);
        assert_eq!(ui.browser.mode, BrowserMode::Preview);
        assert_eq!(
            ui.browser.preview_session_id.as_deref(),
            Some("id-a session")
        );

        // Preview-mode Esc routes to Back, returning to the list.
        assert_eq!(
            session_browser_key_command(
                &ui.browser,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Back))
        );
        apply_session_browser_command(&mut ui, SessionBrowserCommand::Back);
        assert_eq!(ui.browser.mode, BrowserMode::List);
        assert_eq!(ui.browser.preview_session_id, None);
    }

    #[test]
    fn enter_resumes_selected_session_from_list_and_preview() {
        let mut browser = SessionBrowserState {
            visible: true,
            summaries: vec![browser_summary("first"), browser_summary("second")],
            selection_index: 1,
            ..SessionBrowserState::default()
        };

        // List mode: Enter resumes the highlighted row.
        assert_eq!(
            session_browser_key_command(
                &browser,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Resume(
                "id-second".to_string()
            )))
        );

        // Preview mode: Enter resumes the previewed session.
        browser.mode = BrowserMode::Preview;
        browser.preview_session_id = Some("id-first".to_string());
        assert_eq!(
            session_browser_key_command(
                &browser,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Resume(
                "id-first".to_string()
            )))
        );

        // Empty (filtered-out) list: Enter is a no-op, not a resume of nothing.
        let empty = SessionBrowserState {
            visible: true,
            ..SessionBrowserState::default()
        };
        assert_eq!(
            session_browser_key_command(&empty, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn preview_shows_loading_placeholder_then_transcript() {
        let state = state_with_input("", false);
        let mut ui = TuiUiState::default();
        ui.browser.visible = true;
        ui.browser.mode = BrowserMode::Preview;

        // No preview yet → loading placeholder.
        let loading = render_to_text_with_ui_mut(&state, &mut ui, 100, 30);
        assert!(
            loading.contains("Loading"),
            "expected loading state: {loading}"
        );

        // The off-thread fold lands → the transcript renders.
        ui.browser.preview = Some(SessionPreview {
            session_id: "s".to_string(),
            items: vec![chat_item("a remembered step", ChatItemKind::RunSummary)],
        });
        let rendered = render_to_text_with_ui_mut(&state, &mut ui, 100, 30);
        assert!(
            rendered.contains("a remembered step"),
            "expected transcript: {rendered}"
        );
    }

    #[test]
    fn preview_scroll_stays_within_bounds() {
        let mut ui = TuiUiState::default();
        ui.browser.mode = BrowserMode::Preview;
        // 5 items × (title + separator) = 10 lines ⇒ max scroll 9.
        ui.browser.preview = Some(preview_with_items(5));

        let scroll = |ui: &mut TuiUiState, cmd| {
            apply_session_browser_command(ui, SessionBrowserCommand::ScrollPreview(cmd))
        };
        scroll(&mut ui, EventScrollCommand::Bottom);
        assert_eq!(ui.browser.preview_scroll, 9);
        scroll(&mut ui, EventScrollCommand::PageDown);
        assert_eq!(ui.browser.preview_scroll, 9, "clamped at the end");
        scroll(&mut ui, EventScrollCommand::Top);
        assert_eq!(ui.browser.preview_scroll, 0);
        scroll(&mut ui, EventScrollCommand::LinesUp(3));
        assert_eq!(ui.browser.preview_scroll, 0, "clamped at the top");
        scroll(&mut ui, EventScrollCommand::LinesDown(2));
        assert_eq!(ui.browser.preview_scroll, 2);
    }

    #[tokio::test]
    async fn entering_preview_does_not_mutate_app_state() {
        let mut state = state_with_input("", false);
        state.chat_items = vec![chat_item("live transcript", ChatItemKind::UserPrompt)];
        let mut ui = TuiUiState::default();
        ui.browser.visible = true;
        ui.browser.summaries = vec![browser_summary("x")];

        let chat_before = state.chat_items.clone();
        let run_before = state.run_state.clone();
        let (sender, _receiver) = mpsc::channel(1);
        execute_tui_command(
            &mut state,
            &mut ui,
            &sender,
            TuiCommand::SessionBrowser(SessionBrowserCommand::OpenPreview),
        )
        .await
        .unwrap();
        // The UI switched to preview, but live app state is untouched (read-only).
        assert_eq!(ui.browser.mode, BrowserMode::Preview);
        assert_eq!(state.chat_items, chat_before);
        assert_eq!(state.run_state, run_before);
    }

    #[test]
    fn preview_matches_the_on_disk_fold() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let store = crate::history::HistoryStore::create(dir.path()).unwrap();
        let id = store.session_id().to_string();
        for (kind, payload) in [
            ("prompt_submitted", serde_json::json!({ "prompt": "do it" })),
            ("run_completed", serde_json::json!({ "summary": "done" })),
        ] {
            store
                .append_event(&crate::history::HistoryEvent::new(
                    id.clone(),
                    Some("r".to_string()),
                    None,
                    kind,
                    payload,
                ))
                .unwrap();
        }
        let root = dir.path().join(".atelier");
        let preview = crate::app::chat::build_session_preview(&root, &id).unwrap();
        assert!(!preview.items.is_empty());
        // What the off-thread loader publishes equals a fresh on-disk fold.
        assert_eq!(
            preview.items,
            crate::app::chat::build_session_preview(&root, &id)
                .unwrap()
                .items
        );
    }

    // ── /sessions discoverability (task_09) ──

    #[test]
    fn slash_sessions_routes_to_browser_open() {
        // The base routing layer owns the /sessions → browser-open binding (just
        // like /help → ToggleHelp); the command dropdown handles completion first,
        // then this fires on submit.
        let state = state_with_input("/sessions", false);
        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::SessionBrowser(SessionBrowserCommand::Open))
        );
    }

    #[tokio::test]
    async fn submitting_slash_sessions_opens_browser_and_clears_input() {
        let mut state = state_with_input("/sessions", false);
        // Once the command dropdown has been dismissed (after completion), the
        // submit routes to the browser-open binding — the same end state as Ctrl-R.
        let mut ui = TuiUiState {
            command_dropdown_dismissed: Some("/sessions".to_string()),
            ..TuiUiState::default()
        };
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .expect("a command for /sessions + Enter");
        let (sender, _receiver) = mpsc::channel(1);
        execute_tui_command(&mut state, &mut ui, &sender, command)
            .await
            .unwrap();
        assert!(ui.browser.visible, "/sessions opens the browser");
        assert!(state.input.is_empty(), "the /sessions trigger is cleared");
    }

    #[test]
    fn run_state_labels_render_for_every_variant() {
        for run_state in [
            RunState::Idle,
            RunState::Planning,
            RunState::Running,
            RunState::WaitingForUser,
            RunState::Interrupted,
            RunState::Completed,
            RunState::Failed,
            RunState::LimitReached,
        ] {
            assert!(!run_state_label(&run_state).is_empty());
        }
        assert_eq!(
            run_state_label(&RunState::WaitingForUser),
            "waiting for user"
        );
    }

    #[test]
    fn footer_truncates_branch_on_narrow_width_without_panicking() {
        let theme = TuiUiState::default().theme;
        let git = GitContext {
            repo_name: "atelier".to_string(),
            head_sha: None,
            dirty: false,
            branch: "feature/a-very-long-branch-name-that-will-not-fit".to_string(),
        };
        // At 40 cols the branch is shortened first while the run state stays.
        let text = footer_text(&footer_line(&theme, Some(&git), &RunState::Idle, &[], 40));
        assert!(text.contains("idle"));
        assert!(
            text.contains('…'),
            "long branch is truncated with an ellipsis"
        );
        assert!(
            !text.contains("not-fit"),
            "the far end of the branch is dropped"
        );
    }

    #[test]
    fn footer_renders_below_status_line_in_idle_and_running() {
        let mut state = state_with_agent_roster("");
        state.git_context = Some(GitContext {
            repo_name: "atelier".to_string(),
            head_sha: None,
            dirty: false,
            branch: "main".to_string(),
        });

        for run_state in [RunState::Idle, RunState::Running] {
            state.run_state = run_state.clone();
            let mut ui_state = TuiUiState::default();
            let lines = render_to_lines_with_ui_mut(&state, &mut ui_state, 80, 24);

            let help_row = lines.iter().position(|l| l.contains("/help")).unwrap();
            let footer_row = lines.iter().position(|l| l.contains("atelier")).unwrap();
            assert!(
                footer_row > help_row,
                "footer renders below the status line"
            );
            assert!(lines[footer_row].contains("main"));
            assert!(lines[footer_row].contains("agents"));
            assert!(lines[footer_row].contains(run_state_label(&run_state)));
        }
    }

    #[test]
    fn footer_reflects_branch_change_across_state_updates() {
        let mut state = state_with_input("", false);
        state.git_context = Some(GitContext {
            repo_name: "atelier".to_string(),
            head_sha: None,
            dirty: false,
            branch: "feat/one".to_string(),
        });
        let before = render_to_text(&state, 80, 24);
        assert!(before.contains("feat/one"));

        state.git_context = Some(GitContext {
            repo_name: "atelier".to_string(),
            head_sha: None,
            dirty: false,
            branch: "feat/two".to_string(),
        });
        let after = render_to_text(&state, 80, 24);
        assert!(after.contains("feat/two"));
        assert!(!after.contains("feat/one"));
    }

    // ── task_07 per-agent accents ──

    fn chat_item(title: &str, kind: ChatItemKind) -> ChatItemView {
        ChatItemView {
            id: format!("i:{title}"),
            lifecycle_key: None,
            kind,
            status: crate::app::chat::ChatItemStatus::Running,
            severity: ChatSeverity::Info,
            title: title.to_string(),
            summary: None,
            body: Vec::new(),
            details: Vec::new(),
            source: crate::app::chat::ChatSourceRef {
                event_ids: Vec::new(),
                run_id: None,
                step_id: None,
                action_id: None,
            },
            updated_at: String::new(),
        }
    }

    /// fg of the first cell of `needle` in the rendered buffer, reconstructing
    /// text cell-by-cell so multi-byte borders/badges don't skew the column.
    fn title_cell_fg(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
        let area = *buffer.area();
        for y in 0..area.height {
            for x in 0..area.width {
                let mut text = String::new();
                let mut cx = x;
                while cx < area.width && text.chars().count() < needle.chars().count() {
                    text.push_str(buffer.cell((cx, y)).map(|c| c.symbol()).unwrap_or(""));
                    cx += 1;
                }
                if text.starts_with(needle) {
                    return buffer.cell((x, y)).and_then(|cell| cell.style().fg);
                }
            }
        }
        None
    }

    #[test]
    fn agent_index_resolves_in_roster_order_and_wraps_the_pool() {
        let agents: Vec<AgentView> = (0..7)
            .map(|i| agent_view(&format!("agent{i}"), &format!("agent{i}"), "idle", &[]))
            .collect();
        for i in 0..3 {
            assert_eq!(
                agent_index_for_title(&agents, &format!("agent{i} step started")),
                Some(i)
            );
        }
        let index5 = agent_index_for_title(&agents, "agent5 step started").unwrap();
        assert_eq!(index5, 5);
        // Pool wraps at AGENT_ACCENT_COUNT: accent_for(5) == accent_for(0).
        let theme = TuiUiState::default().theme;
        assert_eq!(theme.accent_for(index5), theme.accent_for(0));
    }

    #[test]
    fn absent_agent_title_resolves_to_no_accent() {
        let agents = vec![agent_view("fixer", "Fixer", "idle", &[])];
        let theme = TuiUiState::default().theme;
        assert_eq!(agent_index_for_title(&agents, "ghost step started"), None);
        let item = chat_item("ghost step started", ChatItemKind::AgentProgress);
        assert!(item_agent_accent(&theme, &agents, &item).is_none());
    }

    #[test]
    fn progress_and_result_title_formats_resolve_to_same_agent() {
        let agents = vec![
            agent_view("orchestrator", "Orchestrator", "idle", &[]),
            agent_view("fixer", "Fixer", "idle", &[]),
        ];
        // AgentProgress "{agent} …" and AgentResult "{agent}: …" formats.
        assert_eq!(agent_index_for_title(&agents, "fixer running"), Some(1));
        assert_eq!(agent_index_for_title(&agents, "fixer: done"), Some(1));
    }

    #[test]
    fn agent_progress_headers_carry_distinct_accents() {
        let theme = TuiUiState::default().theme;
        let mut state = state_with_input("", false);
        state.agents = vec![
            agent_view("explorer", "Explorer", "idle", &[]),
            agent_view("fixer", "Fixer", "idle", &[]),
        ];
        state.chat_items = vec![
            chat_item("explorer is working", ChatItemKind::AgentProgress),
            chat_item("fixer is working", ChatItemKind::AgentProgress),
        ];
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui_state = TuiUiState {
            roster_visible: false,
            ..TuiUiState::default()
        };
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let explorer_fg = title_cell_fg(buffer, "explorer is working").unwrap();
        let fixer_fg = title_cell_fg(buffer, "fixer is working").unwrap();
        assert_eq!(explorer_fg, theme.accent_for(0));
        assert_eq!(fixer_fg, theme.accent_for(1));
        assert_ne!(explorer_fg, fixer_fg);
    }

    #[test]
    fn hook_invocation_item_renders_without_panicking() {
        // The new exhaustive arm resolves...
        assert_eq!(chat_kind_label(&ChatItemKind::HookInvocation), "hook");
        // ...and a HookInvocation item renders through the transcript path without
        // panicking (the render path is a pure function of chat items).
        let mut state = state_with_input("", false);
        state.chat_items = vec![chat_item(
            "Hook running: command on run_completed",
            ChatItemKind::HookInvocation,
        )];
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui_state = TuiUiState::default();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
    }

    #[test]
    fn roster_names_carry_same_accents_as_chat() {
        // ADR-005: accent follows canonical identity, not render position. Fixer
        // is canonical index 1 but pinned to the top row (NeedsInput); its name
        // must still render `accent_for(1)` — the same accent the chat transcript
        // resolves for that agent (by its canonical position in `agents`).
        let theme = TuiUiState::default().theme;
        let mut state = state_with_input("", false);
        state.agents = vec![
            agent_view("explorer", "Explorer", "idle", &[]),
            agent_view("fixer", "Fixer", "idle", &[]),
        ];
        // Pinned-reorder roster: NeedsInput Fixer (canonical accent 1) at row 0,
        // Explorer (canonical accent 0) below it.
        state.roster_rows = vec![
            roster_row("fixer", "Fixer", 1, ActivityState::NeedsInput, None, None),
            roster_row("explorer", "Explorer", 0, ActivityState::Idle, None, None),
        ];
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui_state = TuiUiState::default();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        let buffer = terminal.backend().buffer();

        // Pinned to row 0, Fixer keeps its canonical accent — not the row-0 accent.
        assert_eq!(title_cell_fg(buffer, "Fixer").unwrap(), theme.accent_for(1));
        assert_eq!(
            title_cell_fg(buffer, "Explorer").unwrap(),
            theme.accent_for(0)
        );
    }

    #[test]
    fn run_summary_uses_severity_styling_not_agent_accent() {
        let theme = TuiUiState::default().theme;
        let agents = vec![agent_view("fixer", "Fixer", "idle", &[])];
        // Titled like an agent result, but a run summary spans agents and must
        // not pick up an agent accent.
        let item = chat_item("fixer: run complete", ChatItemKind::RunSummary);
        assert!(item_agent_accent(&theme, &agents, &item).is_none());

        let line = chat_item_header_line(&theme, &item, None);
        let title_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("fixer: run complete"))
            .unwrap();
        assert_eq!(title_span.style.fg, Some(theme.text)); // Info severity token
        assert_ne!(title_span.style.fg, Some(theme.accent_for(0)));
    }

    #[test]
    fn agent_dropdown_ids_carry_same_accents_as_roster() {
        // Cross-surface consistency under the pin (ADR-005, task_07): with the
        // roster pinned so fixer (canonical 1) sits at row 0, both the roster
        // name and the `/agent:` dropdown id must still show `accent_for(1)` —
        // the dropdown resolves by canonical `id`, never by dropdown rank.
        let theme = TuiUiState::default().theme;
        // Canonical order: explorer (0), fixer (1).
        let mut state = state_with_agent_roster("/agent:");
        state.roster_rows = vec![
            roster_row("fixer", "Fixer", 1, ActivityState::NeedsInput, None, None),
            roster_row("explorer", "Explorer", 0, ActivityState::Idle, None, None),
        ];
        let mut ui_state = TuiUiState {
            roster_visible: true,
            input_cursor: input_char_count("/agent:"),
            ..TuiUiState::default()
        };
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        let buffer = terminal.backend().buffer();

        // Dropdown ids (lowercase) — canonical accent regardless of dropdown rank.
        assert_eq!(
            title_cell_fg(buffer, "explorer").unwrap(),
            theme.accent_for(0)
        );
        assert_eq!(title_cell_fg(buffer, "fixer").unwrap(), theme.accent_for(1));
        // Roster names (capitalized) — fixer keeps accent_for(1) at pinned row 0.
        assert_eq!(title_cell_fg(buffer, "Fixer").unwrap(), theme.accent_for(1));
        assert_eq!(
            title_cell_fg(buffer, "Explorer").unwrap(),
            theme.accent_for(0)
        );
    }

    // ── task_08 surface polish ──

    fn render_to_buffer(
        state: &AppState,
        ui_state: &mut TuiUiState,
        width: u16,
        height: u16,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, state, ui_state))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// bg of the first cell of `needle`, reconstructing text cell-by-cell.
    fn cell_bg_for_text(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
        let area = *buffer.area();
        for y in 0..area.height {
            for x in 0..area.width {
                let mut text = String::new();
                let mut cx = x;
                while cx < area.width && text.chars().count() < needle.chars().count() {
                    text.push_str(buffer.cell((cx, y)).map(|c| c.symbol()).unwrap_or(""));
                    cx += 1;
                }
                if text.starts_with(needle) {
                    return buffer.cell((x, y)).and_then(|cell| cell.style().bg);
                }
            }
        }
        None
    }

    /// True if the row containing `row_needle` has any cell with `bg`.
    fn row_contains_bg(buffer: &ratatui::buffer::Buffer, row_needle: &str, bg: Color) -> bool {
        let area = *buffer.area();
        for y in 0..area.height {
            let row: String = (0..area.width)
                .map(|x| buffer.cell((x, y)).map(|c| c.symbol()).unwrap_or(""))
                .collect();
            if row.contains(row_needle) {
                return (0..area.width)
                    .any(|x| buffer.cell((x, y)).and_then(|c| c.style().bg) == Some(bg));
            }
        }
        false
    }

    /// fg of a titled box's top-left corner (┌ sits two cells left of the title:
    /// `┌` + leading space + title).
    fn box_corner_fg(buffer: &ratatui::buffer::Buffer, title: &str) -> Option<Color> {
        let area = *buffer.area();
        for y in 0..area.height {
            let symbols: Vec<String> = (0..area.width)
                .map(|x| {
                    buffer
                        .cell((x, y))
                        .map(|c| c.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect();
            let row: String = symbols.concat();
            if let Some(byte_idx) = row.find(title) {
                let mut bytes = 0usize;
                for (cell_x, symbol) in symbols.iter().enumerate() {
                    if bytes == byte_idx {
                        let corner = (cell_x as u16).saturating_sub(2);
                        return buffer.cell((corner, y)).and_then(|c| c.style().fg);
                    }
                    bytes += symbol.len();
                }
            }
        }
        None
    }

    #[test]
    fn selection_style_is_ink_on_accent() {
        let theme = TuiUiState::default().theme;
        let style = selection_style(&theme);
        assert_eq!(style.fg, Some(theme.ink));
        assert_eq!(style.bg, Some(theme.accent));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn both_dropdowns_share_the_accent_selection_treatment() {
        let theme = TuiUiState::default().theme;

        let agent_state = state_with_agent_roster("/agent:");
        let mut agent_ui = TuiUiState {
            roster_visible: false,
            input_cursor: input_char_count("/agent:"),
            ..TuiUiState::default()
        };
        let agent_buf = render_to_buffer(&agent_state, &mut agent_ui, 100, 24);
        assert!(
            row_contains_bg(&agent_buf, "explorer", theme.accent),
            "selected agent row carries the accent selection bg"
        );

        let skill_state = state_with_input("/skill:", false);
        let mut skill_ui = ui_state_with_skills_at_end("/skill:");
        skill_ui.roster_visible = false;
        let skill_buf = render_to_buffer(&skill_state, &mut skill_ui, 100, 24);
        assert!(
            row_contains_bg(&skill_buf, "project-alpha", theme.accent),
            "selected skill row carries the accent selection bg"
        );
    }

    #[test]
    fn skill_tags_use_distinct_tokens_for_project_and_personal() {
        let theme = TuiUiState::default().theme;
        let state = state_with_input("/skill:", false);
        let mut ui = ui_state_with_skills_at_end("/skill:");
        ui.roster_visible = false;
        let buf = render_to_buffer(&state, &mut ui, 100, 24);

        let project = cell_bg_for_text(&buf, "Project");
        let personal = cell_bg_for_text(&buf, "Personal");
        assert_eq!(project, Some(theme.status_ok));
        assert_eq!(personal, Some(theme.accent));
        assert_ne!(project, personal);
    }

    #[test]
    fn help_and_clarification_borders_differ_from_input_composer_token() {
        let theme = TuiUiState::default().theme;
        // Auditable mapping: input composer = border_focused; overlays = accent.
        assert_ne!(theme.border_focused, theme.accent);

        // Help modal border is accent (≠ the input composer's border_focused).
        let state = state_with_input("hi", false);
        let mut help_ui = TuiUiState {
            roster_visible: false,
            help_visible: true,
            ..TuiUiState::default()
        };
        let help_buf = render_to_buffer(&state, &mut help_ui, 80, 24);
        let help_border = box_corner_fg(&help_buf, "Help");
        assert_eq!(help_border, Some(theme.accent));
        assert_ne!(help_border, Some(theme.border_focused));

        // Clarification composer border is accent too (≠ border_focused).
        let mut clar_state = state_with_input("", false);
        clar_state.pending_clarification = Some(clarification_view(vec![clarification_option(
            "opt1",
            "Feature scope",
        )]));
        let mut clar_ui = TuiUiState {
            roster_visible: false,
            ..TuiUiState::default()
        };
        let clar_buf = render_to_buffer(&clar_state, &mut clar_ui, 80, 24);
        let clar_border = box_corner_fg(&clar_buf, "Clarifying question");
        assert_eq!(clar_border, Some(theme.accent));
        assert_ne!(clar_border, Some(theme.border_focused));
    }

    // ── agent-output readability (label/value styling) ──

    fn body(style: ChatLineStyle, text: &str) -> ChatLineView {
        ChatLineView {
            style,
            text: text.to_string(),
        }
    }

    #[test]
    fn run_summary_renders_shared_summary_and_body_text_once() {
        let theme = TuiUiState::default().theme;
        let facts = WelcomeFacts {
            version: "0.0.0",
            working_directory: None,
            agents: &[],
            preset: None,
            warnings: 0,
            git: None,
            recoverable_session: false,
        };
        let line_text = |line: &Line<'static>| -> String {
            line.spans.iter().map(|s| s.content.as_ref()).collect()
        };

        // Run/workflow summaries set `summary` and the first `body` line from the
        // same plain text; it must render exactly once, not twice.
        let mut dup = chat_item("Run completed", ChatItemKind::RunSummary);
        dup.summary = Some("Created CLI_README.md".to_string());
        dup.body = vec![body(ChatLineStyle::Plain, "Created CLI_README.md")];
        let lines = chat_item_lines(&theme, std::slice::from_ref(&dup), &[], 80, true, &facts);
        let occurrences = lines
            .iter()
            .filter(|line| line_text(line).contains("Created CLI_README.md"))
            .count();
        assert_eq!(
            occurrences, 1,
            "duplicated summary/body text must render once"
        );

        // Distinct summary and body both still render (no over-suppression).
        let mut distinct = chat_item("Run completed", ChatItemKind::RunSummary);
        distinct.summary = Some("short gist".to_string());
        distinct.body = vec![body(ChatLineStyle::Plain, "full detail line")];
        let lines = chat_item_lines(
            &theme,
            std::slice::from_ref(&distinct),
            &[],
            80,
            true,
            &facts,
        );
        assert!(lines
            .iter()
            .any(|line| line_text(line).contains("short gist")));
        assert!(lines
            .iter()
            .any(|line| line_text(line).contains("full detail line")));
    }

    #[test]
    fn chat_body_line_styles_known_label_distinctly_from_value() {
        let theme = TuiUiState::default().theme;

        // "finding" -> accent label; value keeps the muted base; text identical.
        let line = chat_body_line(
            &theme,
            &body(ChatLineStyle::Muted, "finding: Entrypoint: src/main.rs"),
        );
        let label = line
            .spans
            .iter()
            .find(|s| s.content.starts_with("finding"))
            .unwrap();
        let value = line
            .spans
            .iter()
            .find(|s| s.content.contains("Entrypoint"))
            .unwrap();
        assert_eq!(label.style.fg, Some(theme.accent));
        assert!(label.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(value.style.fg, Some(theme.text_muted));
        assert_ne!(label.style.fg, value.style.fg);
        let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "  finding: Entrypoint: src/main.rs");

        // "verified" -> status_ok label (split only on the first ": ").
        let verified = chat_body_line(
            &theme,
            &body(ChatLineStyle::Muted, "verified: cargo test passed"),
        );
        let vlabel = verified
            .spans
            .iter()
            .find(|s| s.content.starts_with("verified"))
            .unwrap();
        assert_eq!(vlabel.style.fg, Some(theme.status_ok));
    }

    #[test]
    fn chat_body_line_leaves_prose_and_code_unsplit() {
        let theme = TuiUiState::default().theme;

        // Prose with no recognized label renders as one value span.
        let prose = chat_body_line(
            &theme,
            &body(ChatLineStyle::Plain, "Explored the codebase structure"),
        );
        assert_eq!(
            prose
                .spans
                .iter()
                .filter(|s| !s.content.trim().is_empty())
                .count(),
            1
        );

        // An unknown "Word:" prefix is not treated as a label.
        let note = chat_body_line(&theme, &body(ChatLineStyle::Plain, "Note: did something"));
        assert_eq!(
            note.spans
                .iter()
                .filter(|s| !s.content.trim().is_empty())
                .count(),
            1
        );

        // Code lines are never split, even with a colon.
        let code = chat_body_line(&theme, &body(ChatLineStyle::Code, "key: value"));
        let code_span = code
            .spans
            .iter()
            .find(|s| s.content.contains("key"))
            .unwrap();
        assert_eq!(code_span.style.fg, Some(theme.accent));
        assert_eq!(
            code.spans
                .iter()
                .filter(|s| !s.content.trim().is_empty())
                .count(),
            1
        );
    }

    #[test]
    fn commands_tab_lines_cover_every_catalog_command_once() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&commands_tab_lines("", &theme));
        // Mirrors `help_modal_command_rows_are_catalog_derived`: every catalog
        // command's usage renders, and each description renders exactly once,
        // proving the rows are catalog-derived and not duplicated.
        for spec in crate::slash_commands::catalog() {
            assert!(text.contains(spec.usage), "missing usage {}", spec.usage);
            let occurrences = text.matches(spec.description).count();
            assert_eq!(
                occurrences, 1,
                "description {:?} rendered {occurrences} times",
                spec.description
            );
        }
        assert!(text.contains("/workflow <prompt>"));
        assert!(text.contains("/queue <message>"));
        assert!(text.contains("/reload:skills"));
    }

    #[test]
    fn commands_tab_lines_filter_narrows_to_matching_usage() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&commands_tab_lines("goal", &theme));
        // Both /goal rows match the substring; /workflow does not.
        assert!(text.contains("/goal | /goal <text>"), "missing /goal row");
        assert!(text.contains("/goal clear"), "missing /goal clear row");
        assert!(
            !text.contains("/workflow <prompt>"),
            "/workflow should be filtered out"
        );
        // The echoed filter text is visible.
        assert!(
            text.contains("Filter: goal"),
            "filter line missing typed text"
        );
    }

    #[test]
    fn commands_tab_lines_empty_filter_shows_all_commands() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&commands_tab_lines("", &theme));
        for spec in crate::slash_commands::catalog() {
            assert!(text.contains(spec.usage), "missing usage {}", spec.usage);
        }
    }

    #[test]
    fn commands_tab_lines_no_match_renders_empty_indicator() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&commands_tab_lines("zzz-nope", &theme));
        assert!(
            text.contains("No commands match"),
            "missing empty-result indicator"
        );
        // No catalog usage leaks through on a no-match filter.
        for spec in crate::slash_commands::catalog() {
            assert!(
                !text.contains(spec.usage),
                "unexpected usage {} on empty result",
                spec.usage
            );
        }
    }

    #[test]
    fn commands_tab_filter_is_case_insensitive() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&commands_tab_lines("GOAL", &theme));
        assert!(
            text.contains("/goal clear"),
            "uppercase filter should match"
        );
    }

    #[test]
    fn help_filter_keys_route_only_on_commands_tab() {
        let state = state_with_input("", false);
        let commands_ui = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };
        // On the Commands tab, printable chars feed the filter; Backspace edits it.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &commands_ui,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpFilterCharacter('g'))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &commands_ui,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpFilterBackspace)
        );
        // Arrow/Tab navigation still wins over filter capture on the Commands tab.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &commands_ui,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
            ),
            Some(TuiCommand::HelpNextTab)
        );

        // On any other tab the same key does NOT route to the filter.
        let keys_ui = TuiUiState {
            help_active_tab: HelpTab::Keys,
            ..commands_ui
        };
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &keys_ui,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)
            ),
            None
        );
    }

    #[tokio::test]
    async fn help_filter_backspace_broadens_and_tab_change_resets() {
        let (sender, _receiver) = mpsc::channel(8);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            help_filter: "go".to_string(),
            ..TuiUiState::default()
        };

        // Backspace on "go" yields "g" and broadens the list.
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::HelpFilterBackspace,
        )
        .await
        .unwrap();
        assert_eq!(ui_state.help_filter, "g");

        // Switching tabs resets the filter to "".
        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::HelpNextTab)
            .await
            .unwrap();
        assert_eq!(ui_state.help_filter, "");

        // Closing the modal also clears any filter.
        ui_state.help_active_tab = HelpTab::Commands;
        ui_state.help_filter = "wf".to_string();
        execute_tui_command(&mut state, &mut ui_state, &sender, TuiCommand::ToggleHelp)
            .await
            .unwrap();
        assert!(!ui_state.help_visible);
        assert_eq!(ui_state.help_filter, "");
    }

    #[tokio::test]
    async fn help_filter_does_not_touch_composer_input() {
        let (sender, _receiver) = mpsc::channel(8);
        let mut state = state_with_input("draft prompt", false);
        let mut ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };

        for ch in ['g', 'o', 'a', 'l'] {
            execute_tui_command(
                &mut state,
                &mut ui_state,
                &sender,
                TuiCommand::HelpFilterCharacter(ch),
            )
            .await
            .unwrap();
        }
        assert_eq!(ui_state.help_filter, "goal");
        // The live composer is untouched by filtering.
        assert_eq!(state.input, "draft prompt");
    }

    #[tokio::test]
    async fn help_commands_filter_narrows_then_shows_empty_state() {
        // End-to-end: open help → Commands tab → type a matching substring →
        // only matching rows render; extend to a no-match query → empty indicator.
        let (sender, _receiver) = mpsc::channel(8);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };

        for ch in ['g', 'o', 'a', 'l'] {
            let command = key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            )
            .expect("Commands tab should capture filter characters");
            execute_tui_command(&mut state, &mut ui_state, &sender, command)
                .await
                .unwrap();
        }
        assert_eq!(ui_state.help_filter, "goal");
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);
        assert!(text.contains("/goal clear"), "matching row should render");
        assert!(
            !text.contains("/workflow <prompt>"),
            "non-matching row should be hidden"
        );
        // Filtering never leaks into the composer.
        assert_eq!(state.input, "");

        // One more character makes the query match nothing → empty indicator.
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        )
        .expect("Commands tab should capture filter characters");
        execute_tui_command(&mut state, &mut ui_state, &sender, command)
            .await
            .unwrap();
        assert_eq!(ui_state.help_filter, "goalz");
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);
        assert!(
            text.contains("No commands match"),
            "empty-result indicator should render"
        );
    }

    #[test]
    fn skills_tab_lines_list_aliases_and_disclaimer() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let ui_state = TuiUiState {
            skill_suggestions: test_skill_suggestions(),
            ..TuiUiState::default()
        };
        let text = help_tab_text(&skills_tab_lines(&ui_state, &theme));
        assert!(text.contains("project-alpha"), "missing project alias");
        assert!(text.contains("personal-beta"), "missing personal alias");
        // Source tags are surfaced.
        assert!(text.contains("Project") && text.contains("Personal"));
        // The guidance disclaimer is always present.
        assert!(text.to_lowercase().contains("guidance"));
        assert!(text.to_lowercase().contains("approvals"));
    }

    #[test]
    fn skills_tab_lines_render_empty_state_without_panic() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let ui_state = TuiUiState {
            skill_suggestions: Vec::new(),
            ..TuiUiState::default()
        };
        let text = help_tab_text(&skills_tab_lines(&ui_state, &theme));
        assert!(text.to_lowercase().contains("no skills"));
        // The disclaimer survives the empty state.
        assert!(text.to_lowercase().contains("guidance"));
    }

    #[test]
    fn getting_started_lines_render_model_examples_and_compact_agents() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let mut state = state_with_input("", false);
        state.agents = vec![
            agent_view("explorer", "Explorer", "idle", &["read"]),
            agent_view("fixer", "Fixer", "idle", &["read", "edit"]),
        ];
        let lines = getting_started_lines(&state, &theme);
        let text = help_tab_text(&lines);
        // Routing mental model: prompt -> orchestrator -> named agents.
        assert!(text.contains("orchestrator"));
        assert!(text.contains("agents"));
        // At least two copy-pasteable example prompts (marked with "> ").
        let example_count = lines
            .iter()
            .filter(|line| {
                line.spans
                    .first()
                    .is_some_and(|span| span.content.starts_with("> "))
            })
            .count();
        assert!(example_count >= 2, "expected >=2 example prompts");
        // Exactly one compact agent row per configured agent.
        assert!(text.contains("Explorer"));
        assert!(text.contains("Fixer"));
        let agent_rows = lines
            .iter()
            .filter(|line| {
                let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                joined.contains("Explorer") || joined.contains("Fixer")
            })
            .count();
        assert_eq!(agent_rows, 2, "expected one compact row per agent");
    }

    fn help_tab_text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn keys_tab_lines_contains_expected_keybindings() {
        let theme = Theme::resolve(TerminalCaps::detect());
        // No config ⇒ the default keymap renders its bindings via `format_key`.
        let text = help_tab_text(&keys_tab_lines(&default_keymap(), &theme));

        // Remappable defaults, rendered by their canonical key strings + labels.
        assert!(text.contains("ctrl+l"), "toggle-roster default key");
        assert!(text.contains("show or hide the Agent Roster"));
        assert!(text.contains("pageup"));
        assert!(text.contains("pagedown"));
        assert!(text.contains("home"));
        assert!(text.contains("end"));
        // Editing defaults from task_02/04 are present.
        assert!(text.contains("ctrl+a"));
        assert!(text.contains("ctrl+e"));
        assert!(text.contains("ctrl+k"));
        assert!(text.contains("ctrl+u"));
        assert!(text.contains("ctrl+w"));

        // Reserved / fixed keys are shown locked, distinct from remappable ones.
        assert!(text.contains("ctrl+c"));
        assert!(
            text.contains("(locked)"),
            "fixed keys carry a locked marker"
        );
        assert!(text.contains("Fixed keys (not rebindable)"));
    }

    #[test]
    fn keys_tab_reflects_a_remapped_keymap() {
        let theme = Theme::resolve(TerminalCaps::detect());
        // Rebind toggle-roster to ctrl+g; the tab should show the new key, not ctrl+l.
        let mut overrides = keybindings::KeybindingOverrides::new();
        overrides.insert(
            KeyAction::ToggleRoster,
            Some(keybindings::parse_key("ctrl+g").unwrap()),
        );
        let keymap = Keymap::resolve(&keybindings::DEFAULTS, &overrides);
        let text = help_tab_text(&keys_tab_lines(&keymap, &theme));
        assert!(text.contains("ctrl+g"), "remapped key shown");
        // The displaced default is gone from the remappable section. (`ctrl+l` does
        // not appear anywhere else in the tab.)
        assert!(!text.contains("ctrl+l"), "old default key removed");
    }

    #[test]
    fn keys_tab_renders_via_test_backend_without_panic() {
        let state = state_with_input("", false);
        let ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Keys,
            ..TuiUiState::default()
        };
        // Renders the full help modal (Keys tab) through the real render path.
        let text = render_to_text_with_ui(&state, &ui_state, 100, 32);
        assert!(text.contains("ctrl+l"));
    }

    #[test]
    fn cli_tab_lines_contains_expected_flags() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&cli_tab_lines(&theme));
        assert!(text.contains("atelier --doctor"));
        assert!(text.contains("atelier --init-config"));
        assert!(text.contains("atelier --help"));
    }

    #[test]
    fn approvals_tab_lines_explains_modes_and_roots() {
        let theme = Theme::resolve(TerminalCaps::detect());
        let text = help_tab_text(&approvals_tab_lines(&theme));
        assert!(text.contains("yolo"));
        assert!(text.contains("normal"));
        // Mentions the read/write-roots concept and capabilities.
        assert!(text.contains("read roots") && text.contains("write roots"));
        assert!(text.to_lowercase().contains("capabilities"));
    }

    #[test]
    fn approvals_tab_lines_style_uses_theme_tokens() {
        let theme = Theme::resolve(TerminalCaps::detect());
        // Every styled span draws from theme tokens (no inline Color literals);
        // the header line uses the accent token.
        let lines = approvals_tab_lines(&theme);
        let header = &lines[0];
        assert_eq!(header.spans[0].style.fg, Some(theme.accent));
    }

    #[test]
    fn help_modal_opens_on_getting_started_with_tab_strip() {
        // Opening help (default `help_active_tab`) renders the Getting Started body
        // plus a tab strip listing every tab title.
        let state = state_with_input("", false);
        let ui_state = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };
        assert_eq!(ui_state.help_active_tab, HelpTab::GettingStarted);
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);

        // Getting Started routing line is the default body.
        assert!(text.contains("orchestrator"));
        // The tab strip lists each tab title.
        assert!(text.contains("Getting Started"));
        assert!(text.contains("Commands"));
        assert!(text.contains("Keys"));
        assert!(text.contains("Skills"));
        assert!(text.contains("Approvals"));
        // The default body is Getting Started, not the Commands catalog.
        assert!(!text.contains("toggle the help overlay"));
    }

    #[test]
    fn help_modal_commands_tab_renders_catalog() {
        // Selecting the Commands tab renders the catalog-derived command rows.
        let state = state_with_input("", false);
        let ui_state = TuiUiState {
            help_visible: true,
            help_active_tab: HelpTab::Commands,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);

        assert!(text.contains("/help"));
        assert!(text.contains("toggle the help overlay"));
    }

    #[tokio::test]
    async fn help_tabs_cycle_with_arrows_and_esc_closes_from_any_tab() {
        let (sender, _receiver) = mpsc::channel(8);
        let mut state = state_with_input("", false);
        let mut ui_state = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };

        // Six Right presses cycle through all six tabs and return to Getting
        // Started — proving a full wrap. Each press is routed through the real
        // key handler, then executed.
        for _ in 0..HelpTab::ALL.len() {
            let command = key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            )
            .expect("Right should navigate while help is visible");
            execute_tui_command(&mut state, &mut ui_state, &sender, command)
                .await
                .unwrap();
        }
        assert_eq!(ui_state.help_active_tab, HelpTab::GettingStarted);
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);
        assert!(text.contains("How Atelier works"));

        // From the Skills tab, Esc closes the modal.
        ui_state.help_active_tab = HelpTab::Skills;
        let command = key_event_to_tui_command_with_ui(
            &state,
            &ui_state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .expect("Esc should close help from any tab");
        execute_tui_command(&mut state, &mut ui_state, &sender, command)
            .await
            .unwrap();
        assert!(!ui_state.help_visible);
    }
}
