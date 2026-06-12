use crate::app::chat::{
    ChatDetailRef, ChatItemKind, ChatItemView, ChatLineStyle, ChatLineView, ChatSeverity,
};
use crate::app::git::GitContext;
use crate::app::{
    AgentView, App, AppEvent, AppState, ApprovalHandle, InterruptHandle, PendingClarificationView,
    QueuedFollowUpStatus, QueuedFollowUpView,
};
use crate::config::EffectiveConfig;
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
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
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
const CLARIFICATION_SELECTED_MARKER: &str = "> ";
const CLARIFICATION_UNSELECTED_MARKER: &str = "  ";
const CLARIFICATION_RECOMMENDED_LABEL: &str = "★ recommended";
const CLARIFICATION_CUSTOM_LABEL: &str = "Custom: ";
const CLARIFICATION_HINT: &str =
    "↑/↓ select · type custom answer · Enter answer · Ctrl-C interrupt";
const QUEUE_VISIBLE_MAX: usize = 6;
const QUEUE_SELECTED_MARKER: &str = "> ";
const QUEUE_UNSELECTED_MARKER: &str = "  ";
const QUEUE_HINT: &str = "↑/↓ select · Del cancel · Ctrl-R resume (clear input to focus)";

#[derive(Clone, Debug, PartialEq, Eq)]
enum TuiCommand {
    Dispatch(AppEvent),
    DispatchAndQuit(AppEvent),
    ToggleRoster,
    ToggleHelp,
    ScrollEvents(EventScrollCommand),
    MoveInputCursor(InputCursorCommand),
    AgentDropdown(DropdownCommand),
    SkillDropdown(DropdownCommand),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClarificationCommand {
    PreviousOption,
    NextOption,
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
    clarification_option_index: usize,
    clarification_custom_answer: String,
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
            clarification_option_index: 0,
            clarification_custom_answer: String::new(),
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
// task_04 builds the model; rendering (task_05) and key handling (task_06)
// consume these fields. The allow is removed once those tasks land.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandDropdown {
    suggestions: Vec<&'static crate::slash_commands::SlashCommandSpec>,
    selected: Option<usize>,
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

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let worker = tokio::spawn(run_app_worker(app, command_receiver));

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
            clear_input(state, ui_state);
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
        TuiCommand::Clarification(command) => {
            apply_clarification_command(state, ui_state, command, command_sender).await
        }
        TuiCommand::ClarificationInputCharacter(ch) => {
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

async fn run_app_worker(
    mut app: App,
    mut command_receiver: mpsc::Receiver<AppWorkerCommand>,
) -> Result<()> {
    // Immediate startup refresh, then a change-gated poll every 5s. The poll
    // lives inside the worker's select loop, so it stops as soon as the loop
    // exits on shutdown — no separate task to abort (ADR-006).
    app.refresh_git_context().await;
    let mut git_poll = tokio::time::interval(GIT_POLL_INTERVAL);
    git_poll.tick().await; // consume the immediate first tick (startup covered it)

    loop {
        tokio::select! {
            command = command_receiver.recv() => match command {
                Some(AppWorkerCommand::Event(event)) => {
                    if let Err(error) = app.handle_event(event).await {
                        app.record_diagnostic(error.to_string())?;
                    }
                }
                Some(AppWorkerCommand::Shutdown) => {
                    app.end_session()?;
                    return Ok(());
                }
                None => break,
            },
            _ = git_poll.tick() => {
                app.refresh_git_context().await;
            }
        }
    }

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

fn clarification_key_command(
    state: &AppState,
    _ui_state: &TuiUiState,
    key: KeyEvent,
) -> Option<TuiCommand> {
    let Some(_clarification) = &state.pending_clarification else {
        return None;
    };
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
        } => Some(TuiCommand::ClarificationInputBackspace),
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
    working_directory
        .join(".multiagent")
        .join("skills-cache.json")
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
}

fn reset_agent_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.agent_selection_index = 0;
}

fn reset_skill_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.skill_selection_index = 0;
}

fn reset_command_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.command_selection_index = 0;
    ui_state.command_dropdown_dismissed = None;
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

    match command {
        ClarificationCommand::PreviousOption => {
            let option_count = clarification.options.len();
            ui_state.clarification_option_index = if ui_state.clarification_option_index == 0 {
                option_count.saturating_sub(1)
            } else {
                ui_state.clarification_option_index - 1
            };
            Ok(true)
        }
        ClarificationCommand::NextOption => {
            let option_count = clarification.options.len();
            ui_state.clarification_option_index =
                (ui_state.clarification_option_index + 1) % option_count.max(1);
            Ok(true)
        }
        ClarificationCommand::Submit => {
            let custom_answer = ui_state.clarification_custom_answer.trim().to_string();

            let (answer_text, selected_option_id, selected_option_label, answer_source) =
                if !custom_answer.is_empty() {
                    (custom_answer, None, None, "custom".to_string())
                } else if ui_state.clarification_option_index < clarification.options.len() {
                    let option = &clarification.options[ui_state.clarification_option_index];
                    (
                        option.label.clone(),
                        Some(option.id.clone()),
                        Some(option.label.clone()),
                        "recommended".to_string(),
                    )
                } else {
                    return Ok(true);
                };

            let event = AppEvent::ClarificationAnswered(crate::app::ClarificationAnswer {
                question_id: clarification.question_id.clone(),
                answer: answer_text,
                selected_option_id,
                selected_option_label,
                answer_source,
            });

            queue_app_event(command_sender, event).await?;
            ui_state.clarification_custom_answer.clear();
            ui_state.clarification_option_index = 0;
            Ok(true)
        }
    }
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
// Rendering (task_05) and key handling (task_06) call this; the allow keeps the
// model-only task_04 build clean and is removed when those tasks consume it.
#[allow(dead_code)]
fn command_dropdown(state: &AppState, ui_state: &TuiUiState) -> Option<CommandDropdown> {
    if state.pending_approval.is_some() || matches!(state.run_state, RunState::WaitingForUser) {
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
    let queue_height = queue_panel_height(state);
    let outer = if queue_height > 0 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(6),
                Constraint::Length(queue_height),
                Constraint::Length(composer_height(state)),
            ])
            .split(frame.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(6),
                Constraint::Length(composer_height(state)),
            ])
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

        let roster_items = state
            .agents
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                let availability = availability_label(&agent.availability);
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("{} ", agent.name),
                            Style::default()
                                .fg(theme.accent_for(index))
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            agent_status_label(&agent.status),
                            status_style(&theme, &agent.status),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("{}/{} ", agent.runtime, agent.model),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::styled(
                            availability,
                            availability_style(&theme, &agent.availability),
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
                ])
            })
            .collect::<Vec<_>>();
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
        render_clarification_status(frame, areas.status, &theme);
        if ui_state.help_visible {
            render_help_modal(frame, &theme);
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
    if let Some(dropdown) = agent_dropdown(state, ui_state) {
        render_agent_dropdown(frame, input_areas.input, &dropdown, &theme, &state.agents);
    } else if let Some(dropdown) = skill_dropdown(&state.input, ui_state) {
        render_skill_dropdown(frame, input_areas.input, &dropdown, &theme);
    }
    if ui_state.help_visible {
        render_help_modal(frame, &theme);
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
        vec![
            Line::from(format!(
                "Approval required for {} action {}.",
                pending.agent, pending.action_id
            )),
            Line::from(
                pending
                    .diagnostic
                    .as_deref()
                    .unwrap_or("Approve or deny the pending action."),
            ),
        ]
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
            .position(ui_state.event_scroll);
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

fn render_help_modal(frame: &mut Frame, theme: &Theme) {
    let area = centered_rect(78, 100, frame.area());
    let section_header = |label: &'static str, color| {
        Line::from(vec![
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" commands", Style::default().fg(theme.text)),
        ])
    };
    let mut lines = vec![section_header("TUI", theme.accent)];
    // Slash-command rows come from the shared catalog so help stays aligned
    // with the dropdown and unknown-command guidance (ADR-003). Non-command
    // rows (keys, scrolling, CLI flags) stay literal below.
    lines.extend(crate::slash_commands::help_command_lines().into_iter().map(Line::from));
    lines.extend([
        Line::from("Enter                submit prompt or answer approval"),
        Line::from("Ctrl-L               show or hide Agent Roster"),
        Line::from("Arrow keys           move input cursor"),
        Line::from("PageUp/PageDown     scroll Chat by page"),
        Line::from("Mouse wheel         scroll Chat by line"),
        Line::from("Home/End            jump Chat to top/latest"),
        Line::from("Ctrl-C               interrupt active run and exit"),
        Line::from("Backspace            delete input character"),
        Line::from("Text                 edit the input composer"),
        Line::from(""),
    ]);
    lines.push(section_header("CLI", theme.status_ok));
    lines.extend([
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
    ]);
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

fn composer_height(state: &AppState) -> u16 {
    let Some(clarification) = &state.pending_clarification else {
        return INPUT_COMPOSER_HEIGHT;
    };
    // borders (2) + question (1) + option rows + custom answer line (1)
    let box_rows = 2 + 1 + clarification.options.len() + 1;
    (box_rows.min(usize::from(u16::MAX)) as u16).saturating_add(WORK_INDICATOR_HEIGHT)
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

fn render_clarification_composer(
    frame: &mut Frame,
    area: Rect,
    clarification: &PendingClarificationView,
    ui_state: &TuiUiState,
) {
    let theme = ui_state.theme;
    let mut lines = vec![Line::from(Span::styled(
        clarification.question.clone(),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    ))];
    for (index, option) in clarification.options.iter().enumerate() {
        let selected = index == ui_state.clarification_option_index;
        let recommended = clarification
            .recommended_option_id
            .as_deref()
            .is_some_and(|id| id == option.id);
        let marker = if selected {
            CLARIFICATION_SELECTED_MARKER
        } else {
            CLARIFICATION_UNSELECTED_MARKER
        };
        let option_style = if selected {
            selection_style(&theme)
        } else {
            Style::default().fg(theme.text_muted)
        };
        let mut spans = vec![Span::styled(
            format!("{marker}{}", option.label),
            option_style,
        )];
        if recommended {
            spans.push(Span::styled(
                format!(" {CLARIFICATION_RECOMMENDED_LABEL}"),
                Style::default().fg(theme.accent),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(vec![
        Span::styled(
            CLARIFICATION_CUSTOM_LABEL,
            Style::default().fg(theme.accent),
        ),
        Span::raw(ui_state.clarification_custom_answer.clone()),
    ]));
    let composer = Paragraph::new(lines).block(
        Block::default()
            .title(" Clarifying question ")
            .title_style(Style::default().fg(theme.accent))
            .border_style(Style::default().fg(theme.accent))
            .borders(Borders::ALL),
    );
    frame.render_widget(composer, area);
}

fn render_clarification_status(frame: &mut Frame, status_area: Rect, theme: &Theme) {
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
            CLARIFICATION_HINT,
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
    let custom_row = 1 + 1 + clarification.options.len();
    let custom_col = 1
        + CLARIFICATION_CUSTOM_LABEL.chars().count()
        + input_char_count(&ui_state.clarification_custom_answer);
    let max_col = usize::from(area.width.saturating_sub(2));
    let max_row = usize::from(area.height.saturating_sub(2));
    frame.set_cursor_position(Position::new(
        area.x + custom_col.min(max_col) as u16,
        area.y + custom_row.min(max_row) as u16,
    ));
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
            pending_clarification: None,
            agents: Vec::new(),
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
                "/home/user/.config/.multiagent/multiagent.toml".to_string(),
                "multiagent.toml".to_string(),
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
            pending_clarification: None,
            agents: Vec::new(),
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
            pending_clarification: None,
            agents: Vec::new(),
            chat_items: Vec::new(),
            queued_follow_ups: Vec::new(),
            events: vec!["Action approval required.".to_string()],
            input: String::new(),
            git_context: None,
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Approval required for fixer action action."));
        assert!(text.contains("command requires action approval"));
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
        let ui_state = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 32);

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
        assert!(text.contains("Mouse wheel"));
        assert!(!text.contains("close this help"));
        assert!(text.contains("Ctrl-L"));
        assert!(text.contains("Arrow keys"));
        assert!(text.contains("PageUp/PageDown"));
        assert!(text.contains("Home/End"));
        assert!(text.contains("atelier --doctor"));
        assert!(text.contains("atelier --clean-sessions"));
    }

    #[test]
    fn help_modal_command_rows_are_catalog_derived() {
        let state = state_with_input("", false);
        let ui_state = TuiUiState {
            help_visible: true,
            ..TuiUiState::default()
        };
        let text = render_to_text_with_ui(&state, &ui_state, 120, 40);

        // Every fixed V1 command's usage and description renders exactly once,
        // proving the rows come from the catalog and are not duplicated.
        for spec in crate::slash_commands::catalog() {
            assert!(text.contains(spec.usage), "help missing usage {}", spec.usage);
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
        // Non-command rows survive the catalog routing.
        assert!(text.contains("Ctrl-L"));
        assert!(text.contains("Arrow keys"));
        assert!(text.contains("Mouse wheel"));
    }

    #[test]
    fn readme_skill_command_wording_matches_help_language() {
        let state = state_with_input("", false);
        let ui_state = TuiUiState {
            help_visible: true,
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
        let ui_state = TuiUiState {
            help_visible: true,
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
        (state_with_input(input, false), ui_state_with_cursor_at_end(input))
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
            pending_clarification: None,
            agents: Vec::new(),
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
        let mut ui_state = TuiUiState {
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
        for command in [
            TuiCommand::Clarification(ClarificationCommand::NextOption),
            TuiCommand::Clarification(ClarificationCommand::PreviousOption),
            TuiCommand::ClarificationInputCharacter('h'),
            TuiCommand::ClarificationInputBackspace,
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
        let selected_row = lines
            .iter()
            .position(|line| line.contains("> Feature scope"))
            .unwrap();
        let other_row = lines
            .iter()
            .position(|line| line.contains("Bug fix scope"))
            .unwrap();
        let custom_row = lines
            .iter()
            .position(|line| line.contains("Custom:"))
            .unwrap();
        assert!(question_row < selected_row);
        assert!(selected_row < other_row);
        assert!(other_row < custom_row);
        assert!(lines[selected_row].contains("★ recommended"));
        assert!(!lines[other_row].contains("> "));
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
        let mut ui_state = TuiUiState {
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
            .position(|line| line.contains("> Bug fix scope"))
            .unwrap();
        assert!(lines[recommended_row].contains("Feature scope"));
        assert!(!lines[recommended_row].contains("> "));
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
            "Custom:",
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
        let mut ui_state = TuiUiState {
            clarification_custom_answer: "abc".to_string(),
            ..Default::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();

        // composer box: y = 24 - 7 = 17; custom row = 17 + border + question + 2 options = 21
        // cursor col = border (1) + "Custom: " (8) + "abc" (3) = 12
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(12, 21));
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
        assert!(text.contains("> Clarify the target scope"));
        assert!(text.contains("★ recommended"));
        assert!(text.contains("Custom:"));
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
        let worker = tokio::spawn(run_app_worker(app, receiver));

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
}
