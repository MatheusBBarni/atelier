use crate::app::chat::{
    ChatDetailRef, ChatItemKind, ChatItemView, ChatLineStyle, ChatLineView, ChatSeverity,
};
use crate::app::git::GitContext;
use crate::app::{
    AgentView, App, AppEvent, AppState, ApprovalHandle, InterruptHandle, PendingClarificationView,
    QueuedFollowUpStatus, QueuedFollowUpView,
};
use crate::config::EffectiveConfig;
use crate::file_index::{FileEntry, FileIndex, FileSuggestion};
use crate::orchestrator::RunState;
use crate::skills::{
    self, SkillSourceTag, SkillSuggestion, SKILL_DISCOVERY_MAX_DEPTH, SKILL_FILE_NAME,
    SKILL_SUGGESTION_CACHE_SCHEMA_VERSION,
};
use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEvent, MouseEventKind,
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
    ReloadSkills,
    InputCharacter(char),
    InputBackspace,
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
    Shutdown,
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
        }
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

    // No loading interstitial: the main UI (with the branded welcome item)
    // renders on the first frame, and skill scanning happens behind it inside
    // `run_loop` so startup stays under the first-frame budget (ADR-005).
    let mut ui_state = TuiUiState::with_skill_suggestions(working_directory, Vec::new());
    ui_state.theme = theme;
    ui_state.hide_banner = hide_banner;
    let result = run_loop(
        &mut terminal,
        state_receiver,
        command_sender.clone(),
        interrupt_handle,
        approval_handle,
        file_index_receiver,
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

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state_receiver: watch::Receiver<AppState>,
    command_sender: mpsc::Sender<AppWorkerCommand>,
    interrupt_handle: InterruptHandle,
    approval_handle: ApprovalHandle,
    mut file_index_receiver: watch::Receiver<Vec<FileEntry>>,
    mut ui_state: TuiUiState,
) -> Result<()> {
    let mut state = state_receiver.borrow_and_update().clone();
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
        clamp_input_cursor(&mut ui_state, &state.input);
        terminal.draw(|frame| render(frame, &state, &mut ui_state))?;

        if event::poll(Duration::from_millis(50))? {
            let command = match event::read()? {
                Event::Key(key) => key_event_to_tui_command_with_ui(&state, &ui_state, key),
                Event::Mouse(mouse) => mouse_event_to_tui_command(&ui_state, mouse),
                _ => None,
            };
            if let Some(command) = command {
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
            move_input_cursor(ui_state, &state.input, command);
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
        TuiCommand::Dispatch(event) => {
            if matches_help_command(&event) {
                ui_state.help_visible = !ui_state.help_visible;
                clear_input(state, ui_state);
                return Ok(true);
            }
            let clears_input = matches!(
                event,
                AppEvent::PromptSubmitted(_) | AppEvent::ApprovalAnswered(_)
            );
            if let AppEvent::ApprovalAnswered(approved) = &event {
                if let (Some(approval_handle), Some(pending)) =
                    (approval_handle, state.pending_approval.as_ref())
                {
                    approval_handle.answer(*approved);
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
    matches!(event, AppEvent::PromptSubmitted(prompt) if prompt.trim() == "/help")
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

    loop {
        tokio::select! {
            command = command_receiver.recv() => match command {
                Some(AppWorkerCommand::Event(event)) => {
                    if let Err(error) = app.handle_event(event).await {
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
    if ui_state.help_visible {
        match key {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => Some(TuiCommand::ToggleHelp),
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested)),
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
    } else if state.pending_clarification.is_some() {
        clarification_key_command(state, ui_state, key).or(match key {
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested)),
            _ => None,
        })
    } else if state.pending_approval.is_some() {
        key_event_to_tui_command(state, key)
    } else if agent_dropdown(state, ui_state).is_some() {
        agent_dropdown_key_command(key).or_else(|| key_event_to_tui_command(state, key))
    } else if skill_dropdown(&state.input, ui_state).is_some() {
        skill_dropdown_key_command(key).or_else(|| key_event_to_tui_command(state, key))
    } else if let Some(dropdown) = file_mention_dropdown(state, ui_state) {
        file_mention_dropdown_key_command(&dropdown, key)
            .or_else(|| key_event_to_tui_command(state, key))
    } else if let Some(dropdown) = command_dropdown(state, ui_state) {
        command_dropdown_key_command(&dropdown, key)
            .or_else(|| key_event_to_tui_command(state, key))
    } else if queue_control_active(state, ui_state) {
        queue_control_key_command(state, ui_state, key)
            .or_else(|| key_event_to_tui_command(state, key))
    } else {
        key_event_to_tui_command(state, key)
    }
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

fn key_event_to_tui_command(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    match key {
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested)),
        KeyEvent {
            code: KeyCode::Char('l'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(TuiCommand::ToggleRoster),
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
            code: KeyCode::PageUp,
            ..
        } => Some(TuiCommand::ScrollEvents(EventScrollCommand::PageUp)),
        KeyEvent {
            code: KeyCode::PageDown,
            ..
        } => Some(TuiCommand::ScrollEvents(EventScrollCommand::PageDown)),
        KeyEvent {
            code: KeyCode::Home,
            ..
        } => Some(TuiCommand::ScrollEvents(EventScrollCommand::Top)),
        KeyEvent {
            code: KeyCode::End, ..
        } => Some(TuiCommand::ScrollEvents(EventScrollCommand::Bottom)),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.input.trim() == "/help" => Some(TuiCommand::ToggleHelp),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.input.trim() == RELOAD_SKILLS_COMMAND => Some(TuiCommand::ReloadSkills),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } if state.pending_approval.is_some() => Some(TuiCommand::Dispatch(
            AppEvent::ApprovalAnswered(approval_input_is_yes(&state.input)),
        )),
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
            state.input.clone(),
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

fn approval_input_is_yes(input: &str) -> bool {
    matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "approve" | "approved"
    )
}

fn clear_input(state: &mut AppState, ui_state: &mut TuiUiState) {
    state.input.clear();
    ui_state.input_cursor = 0;
    ui_state.input_preferred_col = None;
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
    clear_command_dropdown_dismissal(ui_state);
    clear_file_mention_dropdown_dismissal(ui_state);
    reset_dropdown_selections(ui_state);
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

        let roster_items = agent_roster_items(&state.agents, RosterRowStyle::Full, &theme);
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
    render_input_status(frame, input_areas.status, ui_state, work_active);
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
        let mut lines = Vec::new();
        // First-approval explainer: a single muted line shown at most once per
        // user (ADR-004), gated by the persisted latch via `AppState`. Additive —
        // the approval prompt below is unchanged.
        if state.show_first_approval_explainer {
            lines.push(Line::styled(
                crate::app::chat::FIRST_APPROVAL_EXPLAINER,
                Style::default().fg(theme.text_muted),
            ));
        }
        lines.push(Line::from(format!(
            "Approval required for {} action {}.",
            pending.agent, pending.action_id
        )));
        lines.push(Line::from(
            pending
                .diagnostic
                .as_deref()
                .unwrap_or("Approve or deny the pending action."),
        ));
        lines
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
            // Resolve the agent's accent by its roster position so the dropdown
            // matches the roster/chat coloring for the same agent.
            let accent = agents
                .iter()
                .position(|agent| agent.id == suggestion.id)
                .map(|roster_index| theme.accent_for(roster_index))
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
/// color (ADR-006). `None` for non-attributed kinds or unmatched agents, so the
/// caller falls back to severity styling. Consistent with the roster/dropdown
/// because all three resolve through `theme.accent_for(roster_index)`.
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
        ChatItemKind::Diagnostic => "diagnostic",
        ChatItemKind::SkillContext => "skills",
        ChatItemKind::AgentResult => "agent",
        ChatItemKind::RunSummary => "run",
        ChatItemKind::Welcome => "welcome",
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
        HelpTab::Keys => keys_tab_lines(theme),
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

/// Keybinding rows for the Keys help tab. Relocated verbatim from the pre-tab
/// `render_help_modal` literals; pure builder consumed by the tabbed render
/// (task 06). No `AppState`/`TuiUiState` reads.
fn keys_tab_lines(_theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from("Enter                submit prompt or answer approval"),
        Line::from("Ctrl-L               show or hide Agent Roster"),
        Line::from("Arrow keys           move input cursor"),
        Line::from("PageUp/PageDown     scroll Chat by page"),
        Line::from("Mouse wheel         scroll Chat by line"),
        Line::from("Home/End            jump Chat to top/latest"),
        Line::from("Ctrl-C               interrupt active run and exit"),
        Line::from("Backspace            delete input character"),
        Line::from("Text                 edit the input composer"),
    ]
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
/// Agent colors cycle via `theme.accent_for(index)`; all other styling reuses the
/// shared `status_style` / `availability_style` helpers so there is one data path.
fn agent_roster_items(
    agents: &[AgentView],
    style: RosterRowStyle,
    theme: &Theme,
) -> Vec<ListItem<'static>> {
    agents
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let name_style = Style::default()
                .fg(theme.accent_for(index))
                .add_modifier(Modifier::BOLD);
            match style {
                RosterRowStyle::Full => ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{} ", agent.name), name_style),
                        Span::styled(
                            agent_status_label(&agent.status).to_string(),
                            status_style(theme, &agent.status),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("{}/{} ", agent.runtime, agent.model),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::styled(
                            availability_label(&agent.availability),
                            availability_style(theme, &agent.availability),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("effort:{} ", agent.effort),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::styled(
                            if agent.thinking {
                                "thinking:on"
                            } else {
                                "thinking:off"
                            },
                            if agent.thinking {
                                Style::default().fg(theme.accent)
                            } else {
                                Style::default().fg(theme.text_dim)
                            },
                        ),
                    ]),
                ]),
                RosterRowStyle::Compact => ListItem::new(agent_compact_line(index, agent, theme)),
            }
        })
        .collect()
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
    let hint_width = WORK_HINT.chars().count();
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
            WORK_HINT,
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
    let Some(clarification) = &state.pending_clarification else {
        return INPUT_COMPOSER_HEIGHT;
    };
    let inner_width = clarification_inner_width(area);
    let layout = clarification_layout(clarification, ui_state, &ui_state.theme);
    let content_rows = wrapped_event_line_count(&layout.lines, inner_width);
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
        PendingClarificationView,
    };
    use crate::config::{load_effective_config, ConfigLoadOptions};
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

    #[test]
    fn agent_roster_items_full_renders_three_lines_per_agent() {
        let theme = TuiUiState::default().theme;
        let agents = vec![
            agent_view("fixer", "Fixer", "running", &["read"]),
            agent_view("planner", "Planner", "idle", &[]),
        ];
        let items = agent_roster_items(&agents, RosterRowStyle::Full, &theme);
        assert_eq!(items.len(), 2);
        for item in &items {
            assert_eq!(item.height(), 3);
        }
    }

    #[test]
    fn agent_roster_items_compact_renders_one_line_per_agent() {
        let theme = TuiUiState::default().theme;
        let agents = vec![agent_view("fixer", "Fixer", "running", &["read"])];
        let items = agent_roster_items(&agents, RosterRowStyle::Compact, &theme);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].height(), 1);
        let text = roster_items_to_text(items, 80, 3);
        assert!(text.contains("Fixer"), "compact row should show the name");
        assert!(
            text.contains("fake/default"),
            "compact row should show runtime/model"
        );
    }

    #[test]
    fn agent_roster_items_renders_down_availability_label() {
        let theme = TuiUiState::default().theme;
        let agents = vec![unavailable_agent_view()];

        let compact = roster_items_to_text(
            agent_roster_items(&agents, RosterRowStyle::Compact, &theme),
            80,
            3,
        );
        assert!(compact.contains("Fixer"));
        assert!(compact.contains("codex/default"));
        assert!(
            compact.contains("down"),
            "compact unavailable agent should show the `down` label"
        );

        let full = roster_items_to_text(
            agent_roster_items(&agents, RosterRowStyle::Full, &theme),
            80,
            6,
        );
        assert!(
            full.contains("down"),
            "full unavailable agent should show the `down` label"
        );
    }

    #[test]
    fn ctrl_l_roster_render_shows_each_agent_after_extraction() {
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
        };
        let ui_state = TuiUiState {
            roster_visible: true,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 24);
        assert!(text.contains("Agent Roster"));
        // First agent: name, runtime/model, availability label.
        assert!(text.contains("Fixer"));
        assert!(text.contains("codex/default"));
        // Second agent (unavailable): name, runtime/model, `down` label.
        assert!(text.contains("claude/default"));
        assert!(text.contains("down"));
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
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: String::new(),
            git_context: None,
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
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Fixer"));
        assert!(text.contains("codex/default"));
        assert!(text.contains("effort:high"));
        assert!(text.contains("down"));
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
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["You: build a feature".to_string()],
            input: String::new(),
            git_context: None,
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
            }),
            show_first_approval_explainer: false,
            pending_clarification: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["Action approval required.".to_string()],
            input: String::new(),
            git_context: None,
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Approval required for fixer action action."));
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
            }),
            show_first_approval_explainer,
            pending_clarification: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            // Empty so the pending-approval fallback render path is exercised.
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["Action approval required.".to_string()],
            input: String::new(),
            git_context: None,
        }
    }

    #[test]
    fn renders_first_approval_explainer_when_flag_set() {
        let state = fallback_approval_state(true);
        // Wide enough that the single explainer line is not wrapped by the
        // paragraph renderer, so the full text is contiguous in the output.
        let text = render_to_text(&state, 200, 24);
        // The explainer line shows alongside the (unchanged) approval prompt.
        assert!(text.contains(crate::app::chat::FIRST_APPROVAL_EXPLAINER));
        assert!(text.contains("Approval required for fixer action action."));
    }

    #[test]
    fn suppresses_first_approval_explainer_when_flag_unset() {
        let state = fallback_approval_state(false);
        let text = render_to_text(&state, 100, 24);
        assert!(!text.contains(crate::app::chat::FIRST_APPROVAL_EXPLAINER));
        assert!(text.contains("Approval required for fixer action action."));
    }

    #[test]
    fn enter_key_submits_current_input_as_app_event() {
        let state = state_with_input("build this", false);

        assert_eq!(
            key_event_to_tui_command(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::Dispatch(AppEvent::PromptSubmitted(
                "build this".to_string()
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
            TuiCommand::Dispatch(AppEvent::PromptSubmitted("slow prompt".to_string())),
        )
        .await
        .unwrap();

        assert!(keep_running);
        assert!(state.input.is_empty());
        assert_eq!(ui_state.input_cursor, 0);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(prompt)) if prompt == "slow prompt"
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
        assert!(keys_text.contains("Mouse wheel"));
        assert!(keys_text.contains("Ctrl-L"));
        assert!(keys_text.contains("Arrow keys"));
        assert!(keys_text.contains("PageUp/PageDown"));
        assert!(keys_text.contains("Home/End"));

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
        assert!(keys_text.contains("Ctrl-L"));
        assert!(keys_text.contains("Arrow keys"));
        assert!(keys_text.contains("Mouse wheel"));
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
                "hello world".to_string()
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
                "/goal ship v2".to_string()
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
                "/goal ship v2".to_string()
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
                "/config".to_string()
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
                "/agent:fixer inspect docs".to_string()
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
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(true)))
        );
        assert_eq!(
            key_event_to_tui_command(&no_state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(false)))
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
            TuiCommand::Dispatch(AppEvent::ApprovalAnswered(true)),
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
        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Some(TuiCommand::DispatchAndQuit(AppEvent::RunInterruptRequested))
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

        assert_eq!(
            key_event_to_tui_command(
                &state,
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
            }),
            show_first_approval_explainer: false,
            pending_clarification: None,
            agents: Vec::new(),
            roster_rows: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: Vec::new(),
            input: input.to_string(),
            git_context: None,
        }
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
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(true)))
        );
        assert_eq!(
            key_event_to_tui_command_with_ui(&no_state, &ui_state, key(KeyCode::Enter)),
            Some(TuiCommand::Dispatch(AppEvent::ApprovalAnswered(false)))
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

        assert!(text.contains("Approval required for fixer action action."));
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

        // Ctrl-R only resumes paused items; on a pending item it falls through.
        assert_eq!(
            key_event_to_tui_command_with_ui(
                &state,
                &ui_state,
                key_with_modifiers(KeyCode::Char('r'), KeyModifiers::CONTROL)
            ),
            None
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
        let state = state_with_agent_roster("draft prompt");
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
            branch: "feat/one".to_string(),
        });
        let before = render_to_text(&state, 80, 24);
        assert!(before.contains("feat/one"));

        state.git_context = Some(GitContext {
            repo_name: "atelier".to_string(),
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
    fn roster_names_carry_same_accents_as_chat() {
        let theme = TuiUiState::default().theme;
        let mut state = state_with_input("", false);
        state.agents = vec![
            agent_view("explorer", "Explorer", "idle", &[]),
            agent_view("fixer", "Fixer", "idle", &[]),
        ];
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui_state = TuiUiState::default();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(
            title_cell_fg(buffer, "Explorer").unwrap(),
            theme.accent_for(0)
        );
        assert_eq!(title_cell_fg(buffer, "Fixer").unwrap(), theme.accent_for(1));
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
        let theme = TuiUiState::default().theme;
        // Roster order: explorer (0), fixer (1), archived/disabled (2, filtered).
        let state = state_with_agent_roster("/agent:");
        let mut ui_state = TuiUiState {
            roster_visible: false,
            input_cursor: input_char_count("/agent:"),
            ..TuiUiState::default()
        };
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(
            title_cell_fg(buffer, "explorer").unwrap(),
            theme.accent_for(0)
        );
        assert_eq!(title_cell_fg(buffer, "fixer").unwrap(), theme.accent_for(1));
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
        let text = help_tab_text(&keys_tab_lines(&theme));
        assert!(text.contains("Ctrl-L"));
        assert!(text.contains("PageUp/PageDown"));
        assert!(text.contains("Home/End"));
        // Verbatim relocation: the full keybinding set is present.
        assert!(text.contains("Enter"));
        assert!(text.contains("Backspace"));
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
