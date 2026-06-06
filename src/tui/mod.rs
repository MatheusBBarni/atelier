use crate::app::chat::{
    ChatDetailRef, ChatItemKind, ChatItemView, ChatLineStyle, ChatLineView, ChatSeverity,
};
use crate::app::{App, AppEvent, AppState, InterruptHandle};
use crate::config::EffectiveConfig;
use crate::orchestrator::RunState;
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
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Position, Rect};
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

const USER_EVENT_BG: Color = Color::Rgb(18, 52, 71);
const INPUT_COMPOSER_HEIGHT: u16 = 5;
const INPUT_BOX_HEIGHT: u16 = 4;
const INPUT_PROMPT: &str = "> ";
const INPUT_PROMPT_WIDTH: usize = 2;
const AGENT_PREFIX: &str = "/agent:";
const SKILL_PREFIX: &str = "/skill:";
const DROPDOWN_MAX_ITEMS: usize = 6;
const SKILL_DISCOVERY_MAX_DEPTH: usize = 4;
const WORK_HINT: &str = "/help";
const WORK_INDICATOR_HEIGHT: u16 = 1;
const WORK_LABEL: &str = "Working";
const MOUSE_SCROLL_LINES: usize = 3;
const WORK_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

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
    input_cursor: usize,
    input_preferred_col: Option<usize>,
    input_width: usize,
    agent_selection_index: usize,
    skill_suggestions: Vec<SkillSuggestion>,
    skill_selection_index: usize,
    work_spinner_frame: usize,
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
            input_cursor: 0,
            input_preferred_col: None,
            input_width: 1,
            agent_selection_index: 0,
            skill_suggestions: Vec::new(),
            skill_selection_index: 0,
            work_spinner_frame: 0,
        }
    }
}

impl TuiUiState {
    fn with_skill_suggestions(skill_suggestions: Vec<SkillSuggestion>) -> Self {
        Self {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SkillSuggestion {
    id: String,
    tag: SkillSourceTag,
    origin: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SkillDropdown {
    token: PromptToken,
    suggestions: Vec<SkillSuggestion>,
    selected: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum SkillSourceTag {
    Project,
    Personal,
}

impl SkillSourceTag {
    fn label(self) -> &'static str {
        match self {
            SkillSourceTag::Project => "Project",
            SkillSourceTag::Personal => "Personal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SkillRoot {
    path: PathBuf,
    tag: SkillSourceTag,
    origin: &'static str,
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
    let mut app = App::new_with_debug(config, debug_enabled).await?;
    let (state_sender, state_receiver) = watch::channel(app.state().clone());
    app.attach_state_sender(state_sender);
    let interrupt_handle = app.interrupt_handle();
    let (command_sender, command_receiver) = mpsc::channel(1024);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let worker = tokio::spawn(run_app_worker(app, command_receiver));

    let result = match terminal.draw(render_skill_loading) {
        Ok(_) => {
            let skill_suggestions = load_skill_suggestions(&working_directory);
            run_loop(
                &mut terminal,
                state_receiver,
                command_sender.clone(),
                interrupt_handle,
                skill_suggestions,
            )
            .await
        }
        Err(error) => Err(error).context("failed to render skill loading state"),
    };

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
    skill_suggestions: Vec<SkillSuggestion>,
) -> Result<()> {
    let mut state = state_receiver.borrow_and_update().clone();
    let mut ui_state = TuiUiState::with_skill_suggestions(skill_suggestions);
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
                if !execute_tui_command_with_interrupt(
                    &mut state,
                    &mut ui_state,
                    &command_sender,
                    Some(&interrupt_handle),
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
    execute_tui_command_with_interrupt(state, ui_state, command_sender, None, command).await
}

async fn execute_tui_command_with_interrupt(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command_sender: &mpsc::Sender<AppWorkerCommand>,
    interrupt_handle: Option<&InterruptHandle>,
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

async fn run_app_worker(
    mut app: App,
    mut command_receiver: mpsc::Receiver<AppWorkerCommand>,
) -> Result<()> {
    while let Some(command) = command_receiver.recv().await {
        match command {
            AppWorkerCommand::Event(event) => {
                if let Err(error) = app.handle_event(event).await {
                    app.record_diagnostic(error.to_string())?;
                }
            }
            AppWorkerCommand::Shutdown => {
                app.end_session()?;
                return Ok(());
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
    } else if agent_dropdown(state, ui_state).is_some() {
        agent_dropdown_key_command(key).or_else(|| key_event_to_tui_command(state, key))
    } else if skill_dropdown(&state.input, ui_state).is_some() {
        skill_dropdown_key_command(key).or_else(|| key_event_to_tui_command(state, key))
    } else {
        key_event_to_tui_command(state, key)
    }
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
    let roots = skill_roots(working_directory);
    let fingerprint = skill_file_fingerprints(&roots);
    if let Some(suggestions) = read_cached_skill_suggestions(working_directory, &fingerprint) {
        return suggestions;
    }

    let suggestions = discover_skill_suggestions_from_roots(&roots);
    let _ = write_skill_suggestion_cache(working_directory, &fingerprint, &suggestions);
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
    if cache.schema_version == 1 && cache.fingerprint == fingerprint {
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
        schema_version: 1,
        fingerprint: fingerprint.to_vec(),
        suggestions: suggestions.to_vec(),
    };
    fs::write(&path, serde_json::to_vec_pretty(&cache)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn skill_roots(working_directory: &Path) -> Vec<SkillRoot> {
    let mut roots = vec![
        SkillRoot {
            path: working_directory.join(".agents/skills"),
            tag: SkillSourceTag::Project,
            origin: ".agents/skills",
        },
        SkillRoot {
            path: working_directory.join(".claude/skills"),
            tag: SkillSourceTag::Project,
            origin: ".claude/skills",
        },
    ];
    if let Some(home) = dirs::home_dir() {
        roots.extend([
            SkillRoot {
                path: home.join(".agents/skills"),
                tag: SkillSourceTag::Personal,
                origin: "~/.agents/skills",
            },
            SkillRoot {
                path: home.join(".claude/skills"),
                tag: SkillSourceTag::Personal,
                origin: "~/.claude/skills",
            },
        ]);
    }
    roots
}

fn skill_file_fingerprints(roots: &[SkillRoot]) -> Vec<SkillFileFingerprint> {
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
    let skill_file = directory.join("SKILL.md");
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

fn discover_skill_suggestions_from_roots(roots: &[SkillRoot]) -> Vec<SkillSuggestion> {
    let mut suggestions = roots
        .iter()
        .flat_map(discover_skill_suggestions_from_root)
        .collect::<Vec<_>>();
    suggestions.sort_by(|left, right| {
        (
            skill_tag_rank(left.tag),
            left.id.as_str(),
            left.origin.as_str(),
        )
            .cmp(&(
                skill_tag_rank(right.tag),
                right.id.as_str(),
                right.origin.as_str(),
            ))
    });
    suggestions
}

fn discover_skill_suggestions_from_root(root: &SkillRoot) -> Vec<SkillSuggestion> {
    let mut suggestions = Vec::new();
    collect_skill_suggestions(&root.path, root, 0, &mut suggestions);
    suggestions
}

fn collect_skill_suggestions(
    directory: &Path,
    root: &SkillRoot,
    depth: usize,
    suggestions: &mut Vec<SkillSuggestion>,
) {
    if depth > SKILL_DISCOVERY_MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(suggestion) = skill_suggestion_from_dir(&path, root) {
            suggestions.push(suggestion);
        }
        collect_skill_suggestions(&path, root, depth + 1, suggestions);
    }
}

fn skill_suggestion_from_dir(path: &Path, root: &SkillRoot) -> Option<SkillSuggestion> {
    if !path.is_dir() {
        return None;
    }
    let skill_file = path.join("SKILL.md");
    if !skill_file.is_file() {
        return None;
    }
    let id = skill_name_from_file(&skill_file).or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .filter(|name| is_valid_skill_id(name))
    })?;
    Some(SkillSuggestion {
        id,
        tag: root.tag,
        origin: root.origin.to_string(),
    })
}

fn skill_name_from_file(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let mut lines = contents.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }
    for line in lines.take(80) {
        let trimmed = line.trim();
        if trimmed == "---" {
            return None;
        }
        if let Some(value) = trimmed.strip_prefix("name:") {
            return clean_skill_name(value);
        }
    }
    None
}

fn clean_skill_name(value: &str) -> Option<String> {
    let value = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    is_valid_skill_id(&value).then_some(value)
}

fn is_valid_skill_id(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_whitespace)
}

fn skill_tag_rank(tag: SkillSourceTag) -> u8 {
    match tag {
        SkillSourceTag::Project => 0,
        SkillSourceTag::Personal => 1,
    }
}

fn reset_dropdown_selections(ui_state: &mut TuiUiState) {
    reset_agent_dropdown_selection(ui_state);
    reset_skill_dropdown_selection(ui_state);
}

fn reset_agent_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.agent_selection_index = 0;
}

fn reset_skill_dropdown_selection(ui_state: &mut TuiUiState) {
    ui_state.skill_selection_index = 0;
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

fn apply_skill_suggestion(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    token: &PromptToken,
    suggestion: &SkillSuggestion,
) {
    let start = byte_index_for_char(&state.input, token.value_start);
    let end = byte_index_for_char(&state.input, token.value_end);
    state.input.replace_range(start..end, &suggestion.id);

    let inserted_len = input_char_count(&suggestion.id);
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

fn active_prompt_token(input: &str, cursor: usize, prefix: &str) -> Option<PromptToken> {
    if !input.starts_with(prefix) {
        return None;
    }
    let prefix_len = input_char_count(prefix);
    if cursor < prefix_len {
        return None;
    }

    let value_len = input[prefix.len()..]
        .chars()
        .take_while(|ch| !ch.is_whitespace())
        .count();
    let value_start = prefix_len;
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
        .filter(|skill| query.is_empty() || skill.id.to_ascii_lowercase().contains(&query))
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

fn render_skill_loading(frame: &mut Frame) {
    let area = frame.area();
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Loading skills",
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("...", Style::default().fg(Color::White)),
        ]),
        Line::from("Scanning project and personal skill folders"),
    ];
    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray))
        .block(
            Block::default()
                .title(" Atelier ")
                .title_style(Style::default().fg(Color::Yellow))
                .border_style(Style::default().fg(Color::Yellow))
                .borders(Borders::ALL),
        );
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render(frame: &mut Frame, state: &AppState, ui_state: &mut TuiUiState) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(INPUT_COMPOSER_HEIGHT),
        ])
        .split(frame.area());
    let event_area = if ui_state.roster_visible {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(28), Constraint::Percentage(72)])
            .split(outer[0]);

        let roster_items = state
            .agents
            .iter()
            .map(|agent| {
                let availability = availability_label(&agent.availability);
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("{} ", agent.name),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            agent_status_label(&agent.status),
                            status_style(&agent.status),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("{}/{} ", agent.runtime, agent.model),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::styled(availability, availability_style(&agent.availability)),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("effort:{} ", agent.effort),
                            Style::default().fg(Color::LightBlue),
                        ),
                        Span::styled(
                            if agent.thinking {
                                "thinking:on"
                            } else {
                                "thinking:off"
                            },
                            if agent.thinking {
                                Style::default().fg(Color::LightMagenta)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                    ]),
                ])
            })
            .collect::<Vec<_>>();
        let roster = List::new(roster_items).block(
            Block::default()
                .title(" Agent Roster ")
                .title_style(Style::default().fg(Color::Cyan))
                .border_style(Style::default().fg(Color::DarkGray))
                .borders(Borders::ALL),
        );
        frame.render_widget(roster, main[0]);
        main[1]
    } else {
        outer[0]
    };

    render_chat(frame, event_area, state, ui_state);

    let work_active = work_indicator_active(state);
    let input_areas = input_areas(outer[1]);
    let input_layout = input_layout(input_areas.input, &state.input, ui_state.input_cursor);
    ui_state.input_width = input_layout.width;
    let input = Paragraph::new(wrapped_input_lines(&state.input, input_layout.width))
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .border_style(Style::default().fg(Color::Yellow))
                .borders(Borders::ALL),
        )
        .scroll((input_layout.scroll.min(usize::from(u16::MAX)) as u16, 0));
    frame.render_widget(input, input_areas.input);
    render_input_status(frame, input_areas.status, ui_state, work_active);
    if let Some(dropdown) = agent_dropdown(state, ui_state) {
        render_agent_dropdown(frame, input_areas.input, &dropdown);
    } else if let Some(dropdown) = skill_dropdown(&state.input, ui_state) {
        render_skill_dropdown(frame, input_areas.input, &dropdown);
    }
    if ui_state.help_visible {
        render_help_modal(frame);
    } else {
        set_input_cursor(frame, input_areas.input, input_layout);
    }
}

fn render_chat(frame: &mut Frame, event_area: Rect, state: &AppState, ui_state: &mut TuiUiState) {
    let event_lines = if !state.chat_items.is_empty() {
        chat_item_lines(&state.chat_items)
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
            .map(|event| legacy_chat_line(event))
            .collect::<Vec<_>>()
    };
    let block = Block::default()
        .title(" Chat ")
        .title_style(Style::default().fg(Color::Green))
        .border_style(Style::default().fg(Color::DarkGray))
        .borders(Borders::ALL);
    let inner_area = block.inner(event_area);
    let paragraph_width = inner_area.width.saturating_sub(1).max(1);
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
        .style(Style::default().fg(Color::White))
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

fn render_agent_dropdown(frame: &mut Frame, input_area: Rect, dropdown: &AgentDropdown) {
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
        .map(|(index, suggestion)| agent_dropdown_item(suggestion, index == selected))
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
            .title_style(Style::default().fg(Color::Yellow))
            .border_style(Style::default().fg(Color::Yellow))
            .borders(Borders::ALL),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(list, area);
}

fn agent_dropdown_item(suggestion: &AgentSuggestion, selected: bool) -> ListItem<'static> {
    let marker_style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let id_style = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let line = Line::from(vec![
        Span::styled(if selected { "> " } else { "  " }, marker_style),
        Span::styled(suggestion.id.clone(), id_style),
        Span::raw("  "),
        Span::styled(suggestion.name.clone(), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(suggestion.detail.clone(), Style::default().fg(Color::Gray)),
    ]);
    let item = ListItem::new(line);
    if selected {
        item.style(Style::default().bg(Color::DarkGray))
    } else {
        item
    }
}

fn render_skill_dropdown(frame: &mut Frame, input_area: Rect, dropdown: &SkillDropdown) {
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
        .map(|(index, suggestion)| skill_dropdown_item(suggestion, index == selected))
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
            .title_style(Style::default().fg(Color::Yellow))
            .border_style(Style::default().fg(Color::Yellow))
            .borders(Borders::ALL),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(list, area);
}

fn skill_dropdown_item(suggestion: &SkillSuggestion, selected: bool) -> ListItem<'static> {
    let marker_style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let id_style = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let tag_style = Style::default()
        .fg(Color::Black)
        .bg(match suggestion.tag {
            SkillSourceTag::Project => Color::LightGreen,
            SkillSourceTag::Personal => Color::LightBlue,
        })
        .add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled(if selected { "> " } else { "  " }, marker_style),
        Span::styled(suggestion.id.clone(), id_style),
        Span::raw("  "),
        Span::styled(format!(" {} ", suggestion.tag.label()), tag_style),
        Span::raw("  "),
        Span::styled(suggestion.origin.clone(), Style::default().fg(Color::Gray)),
    ]);
    let item = ListItem::new(line);
    if selected {
        item.style(Style::default().bg(Color::DarkGray))
    } else {
        item
    }
}

fn chat_item_lines(items: &[ChatItemView]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for item in items {
        if item.kind == ChatItemKind::UserPrompt {
            lines.extend(user_prompt_lines(item));
            lines.push(Line::from(""));
            continue;
        }
        lines.push(chat_item_header_line(item));
        if let Some(summary) = item
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
        {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(summary.to_string(), Style::default().fg(Color::Gray)),
            ]));
        }
        for body in &item.body {
            lines.push(chat_body_line(body));
        }
        if !item.details.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  details: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    item.details
                        .iter()
                        .map(detail_label)
                        .collect::<Vec<_>>()
                        .join(", "),
                    Style::default().fg(Color::DarkGray),
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

fn user_prompt_lines(item: &ChatItemView) -> Vec<Line<'static>> {
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
                            .fg(Color::Black)
                            .bg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ", Style::default().bg(USER_EVENT_BG)),
                    Span::styled(text, Style::default().fg(Color::White).bg(USER_EVENT_BG)),
                    Span::styled(" ", Style::default().bg(USER_EVENT_BG)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        continuation_prefix.clone(),
                        Style::default().bg(USER_EVENT_BG),
                    ),
                    Span::styled(text, Style::default().fg(Color::White).bg(USER_EVENT_BG)),
                    Span::styled(" ", Style::default().bg(USER_EVENT_BG)),
                ])
            }
        })
        .collect()
}

fn chat_item_header_line(item: &ChatItemView) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {} ", item.status.label()),
            severity_badge_style(&item.severity),
        ),
        Span::raw(" "),
        Span::styled(
            item.title.clone(),
            severity_title_style(&item.severity).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", chat_kind_label(&item.kind)),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn chat_body_line(line: &ChatLineView) -> Line<'static> {
    let (prefix, style) = match line.style {
        ChatLineStyle::Plain => ("  ", Style::default().fg(Color::White)),
        ChatLineStyle::Muted => ("  ", Style::default().fg(Color::Gray)),
        ChatLineStyle::Code => ("  ", Style::default().fg(Color::LightBlue)),
        ChatLineStyle::DiffAdd => ("  ", Style::default().fg(Color::Green)),
        ChatLineStyle::DiffRemove => ("  ", Style::default().fg(Color::Red)),
        ChatLineStyle::DiffContext => ("  ", Style::default().fg(Color::DarkGray)),
        ChatLineStyle::Warning => ("  ", Style::default().fg(Color::Yellow)),
        ChatLineStyle::Error => ("  ", Style::default().fg(Color::Red)),
    };
    Line::from(vec![
        Span::styled(prefix, Style::default()),
        Span::styled(line.text.clone(), style),
    ])
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
        ChatItemKind::Diagnostic => "diagnostic",
        ChatItemKind::AgentResult => "agent",
        ChatItemKind::RunSummary => "run",
    }
}

fn severity_badge_style(severity: &ChatSeverity) -> Style {
    match severity {
        ChatSeverity::Info => Style::default().fg(Color::Black).bg(Color::Blue),
        ChatSeverity::Success => Style::default().fg(Color::Black).bg(Color::Green),
        ChatSeverity::Warning => Style::default().fg(Color::Black).bg(Color::Yellow),
        ChatSeverity::Error => Style::default().fg(Color::White).bg(Color::Red),
    }
}

fn severity_title_style(severity: &ChatSeverity) -> Style {
    match severity {
        ChatSeverity::Info => Style::default().fg(Color::White),
        ChatSeverity::Success => Style::default().fg(Color::LightGreen),
        ChatSeverity::Warning => Style::default().fg(Color::Yellow),
        ChatSeverity::Error => Style::default().fg(Color::Red),
    }
}

fn wrapped_event_line_count(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn render_help_modal(frame: &mut Frame) {
    let area = centered_rect(78, 100, frame.area());
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "TUI",
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" commands", Style::default().fg(Color::White)),
        ]),
        Line::from("/help + Enter        toggle this help"),
        Line::from("/agent:<agent_name>  select enabled agent with Up/Down + Enter"),
        Line::from("/skill:<skill_name>  prefix prompt with skill name"),
        Line::from("/goal <text> | /goal | /goal clear   manage session goal"),
        Line::from("/subtask <agent> <task>              run bounded child task"),
        Line::from("/config              show config files, preset, warnings"),
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
        Line::from(vec![
            Span::styled(
                "CLI",
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" commands", Style::default().fg(Color::White)),
        ]),
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
    ];
    let help = Paragraph::new(lines)
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .block(
            Block::default()
                .title(" Help ")
                .title(Line::from(" Esc ").right_aligned())
                .title_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
                .border_style(Style::default().fg(Color::Yellow))
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

fn legacy_chat_line(event: &str) -> Line<'_> {
    if let Some(message) = event.strip_prefix("You: ") {
        return Line::from(vec![
            Span::styled(
                " You ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(USER_EVENT_BG)),
            Span::styled(message, Style::default().fg(Color::White).bg(USER_EVENT_BG)),
            Span::styled(" ", Style::default().bg(USER_EVENT_BG)),
        ]);
    }

    if event.contains("failed") || event.contains("Failed") {
        return Line::from(Span::styled(event, Style::default().fg(Color::Red)));
    }

    Line::from(event)
}

fn status_style(status: &str) -> Style {
    match status {
        "running" | "streaming" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "waiting_action" | "waiting_approval" | "waiting_for_user" | "cancelling" => {
            Style::default().fg(Color::Yellow)
        }
        "interrupted" => Style::default().fg(Color::Red),
        "disabled" => Style::default().fg(Color::DarkGray),
        _ => Style::default().fg(Color::Gray),
    }
}

fn agent_status_label(status: &str) -> &str {
    match status {
        "streaming" => "running",
        _ => status,
    }
}

fn availability_style(availability: &Option<crate::runtime::RuntimeAvailability>) -> Style {
    match availability
        .as_ref()
        .map(|availability| &availability.status)
    {
        Some(crate::runtime::RuntimeAvailabilityStatus::Available) => {
            Style::default().fg(Color::Green)
        }
        Some(crate::runtime::RuntimeAvailabilityStatus::Unavailable) => {
            Style::default().fg(Color::Red)
        }
        Some(crate::runtime::RuntimeAvailabilityStatus::Unknown) | None => {
            Style::default().fg(Color::Yellow)
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
}

fn input_areas(composer_area: Rect) -> InputAreas {
    if composer_area.height <= WORK_INDICATOR_HEIGHT {
        return InputAreas {
            input: composer_area,
            status: Rect::ZERO,
        };
    }

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(INPUT_BOX_HEIGHT),
            Constraint::Length(WORK_INDICATOR_HEIGHT),
            Constraint::Min(0),
        ])
        .split(composer_area);

    InputAreas {
        input: areas[0],
        status: areas[1],
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
    let line_area = Rect {
        x: status_area.x + 1,
        y: status_area.y,
        width: status_area.width.saturating_sub(2),
        height: 1,
    };
    let line_width = usize::from(line_area.width);
    let left_width = if work_active {
        1 + 1 + WORK_LABEL.chars().count()
    } else {
        0
    };
    let hint_width = WORK_HINT.chars().count();
    let mut spans = Vec::new();
    if work_active {
        let spinner = WORK_SPINNER_FRAMES[ui_state.work_spinner_frame % WORK_SPINNER_FRAMES.len()];
        ui_state.work_spinner_frame = ui_state.work_spinner_frame.wrapping_add(1);
        spans.extend([
            Span::styled(spinner, Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled(
                WORK_LABEL,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    } else {
        ui_state.work_spinner_frame = 0;
    }
    if line_width >= left_width.saturating_add(hint_width) {
        spans.push(Span::raw(
            " ".repeat(line_width.saturating_sub(left_width + hint_width)),
        ));
        spans.push(Span::styled(
            WORK_HINT,
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Clear, status_area);
    frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
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

fn wrapped_input_lines(input: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    if input.is_empty() {
        return vec![prompted_input_line("", true)];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0usize;
    for ch in input.chars() {
        line.push(ch);
        line_len += 1;
        if line_len == width {
            lines.push(prompted_input_line(&line, lines.is_empty()));
            line.clear();
            line_len = 0;
        }
    }
    if !line.is_empty() || input.chars().count().is_multiple_of(width) {
        lines.push(prompted_input_line(&line, lines.is_empty()));
    }
    lines
}

fn prompted_input_line(input: &str, first_line: bool) -> Line<'static> {
    let prefix = if first_line { INPUT_PROMPT } else { "  " };
    Line::from(vec![
        Span::styled(prefix, Style::default().fg(Color::Cyan)),
        Span::raw(input.to_string()),
    ])
}

fn set_input_cursor(frame: &mut Frame, input_area: Rect, input_layout: InputLayout) {
    frame.set_cursor_position(Position::new(
        input_area.x + 1 + input_layout.cursor_col,
        input_area.y + 1 + input_layout.cursor_row,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::chat::ChatProjection;
    use crate::app::{AgentView, ConfigStatusView, LiveStepStatus, LiveStepView, LiveStreamView};
    use crate::history::HistoryEvent;
    use crate::orchestrator::RunState;
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
            pending_approval: None,
            agents: Vec::new(),
            chat_items: Vec::new(),
            events: Vec::new(),
            input: String::new(),
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Agent Roster"));
        assert!(text.contains("Chat"));
        assert!(text.contains(">"));
        assert!(!text.contains("Input Composer"));
        assert!(text.contains("No chat yet."));
    }

    #[test]
    fn renders_skill_loading_state() {
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render_skill_loading).unwrap();

        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Loading skills"));
        assert!(text.contains("Scanning project and personal skill folders"));
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
            pending_approval: None,
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
            events: vec![
                "Run started.".to_string(),
                "Fixer step started.".to_string(),
            ],
            input: "follow up".to_string(),
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
        state.live_step = Some(LiveStepView {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            agent: "fixer".to_string(),
            status: LiveStepStatus::Streaming,
            streams: vec![LiveStreamView {
                stream: "stdout".to_string(),
                content: "compiling target".to_string(),
                sequence_end: 1,
                final_delta: false,
            }],
        });
        state.events = vec!["Fixer step started.".to_string()];
        let mut projection = ChatProjection::new();
        projection.apply_live_step(state.live_step.as_ref());
        state.chat_items = projection.items().to_vec();

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("fixer is running"));
        assert!(text.contains("[stdout:live:#1] compiling target"));
        assert!(!text.contains("Fixer step started."));
    }

    #[test]
    fn renders_live_step_running_state_before_stream_content() {
        let mut state = state_with_input("", false);
        state.live_step = Some(LiveStepView {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            agent: "fixer".to_string(),
            status: LiveStepStatus::Running,
            streams: Vec::new(),
        });
        let mut projection = ChatProjection::new();
        projection.apply_live_step(state.live_step.as_ref());
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
            pending_approval: None,
            agents: Vec::new(),
            chat_items: Vec::new(),
            events: vec!["You: build a feature".to_string()],
            input: String::new(),
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
            step_id: None,
            timestamp: "2026-06-05T00:00:00.000Z".to_string(),
            kind: "prompt_submitted".to_string(),
            payload: json!({ "prompt": "build a feature" }),
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
            .assert_cursor_position(Position::new(7, 20));
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
            .assert_cursor_position(Position::new(13, 9));
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
            pending_approval: Some(crate::app::PendingApprovalView {
                run_id: "run".to_string(),
                step_id: "step".to_string(),
                action_id: "action".to_string(),
                agent: "fixer".to_string(),
                summary: "Action requires action approval.".to_string(),
                diagnostic: Some("command requires action approval: cargo install x".to_string()),
            }),
            agents: Vec::new(),
            chat_items: Vec::new(),
            events: vec!["Action approval required.".to_string()],
            input: String::new(),
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
        assert!(text.contains("/help + Enter"));
        assert!(text.contains("/agent:<agent_name>"));
        assert!(text.contains("/skill:<skill_name>"));
        assert!(text.contains("enabled agent"));
        assert!(text.contains("/goal <text>"));
        assert!(text.contains("/goal clear"));
        assert!(text.contains("/subtask <agent>"));
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

    #[test]
    fn discovers_project_and_personal_skills_from_agent_and_claude_roots() {
        let dir = tempdir().unwrap();
        let project_agents = dir.path().join(".agents/skills");
        let project_claude = dir.path().join(".claude/skills");
        let personal_agents = dir.path().join("home/.agents/skills");
        let personal_claude = dir.path().join("home/.claude/skills");
        write_skill(&project_agents, "project-agent", "frontmatter-project");
        write_skill(
            &project_agents.join(".system"),
            "project-system",
            "nested-project",
        );
        write_skill_without_name(&project_claude, "project-claude");
        write_skill(&personal_agents, "personal-agent", "frontmatter-personal");
        write_skill(&personal_claude, "personal-claude", "personal-claude");

        let roots = vec![
            SkillRoot {
                path: project_agents,
                tag: SkillSourceTag::Project,
                origin: ".agents/skills",
            },
            SkillRoot {
                path: project_claude,
                tag: SkillSourceTag::Project,
                origin: ".claude/skills",
            },
            SkillRoot {
                path: personal_agents,
                tag: SkillSourceTag::Personal,
                origin: "~/.agents/skills",
            },
            SkillRoot {
                path: personal_claude,
                tag: SkillSourceTag::Personal,
                origin: "~/.claude/skills",
            },
        ];

        let suggestions = discover_skill_suggestions_from_roots(&roots);

        assert_eq!(
            suggestions,
            vec![
                SkillSuggestion {
                    id: "frontmatter-project".to_string(),
                    tag: SkillSourceTag::Project,
                    origin: ".agents/skills".to_string(),
                },
                SkillSuggestion {
                    id: "nested-project".to_string(),
                    tag: SkillSourceTag::Project,
                    origin: ".agents/skills".to_string(),
                },
                SkillSuggestion {
                    id: "project-claude".to_string(),
                    tag: SkillSourceTag::Project,
                    origin: ".claude/skills".to_string(),
                },
                SkillSuggestion {
                    id: "frontmatter-personal".to_string(),
                    tag: SkillSourceTag::Personal,
                    origin: "~/.agents/skills".to_string(),
                },
                SkillSuggestion {
                    id: "personal-claude".to_string(),
                    tag: SkillSourceTag::Personal,
                    origin: "~/.claude/skills".to_string(),
                },
            ]
        );
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
        let line = legacy_chat_line("You: build a feature");

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
            .assert_cursor_position(Position::new(13, 8));
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
            pending_approval: None,
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
            events: vec!["Run started.".to_string()],
            input: String::new(),
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
            pending_approval: pending_approval.then(|| crate::app::PendingApprovalView {
                run_id: "run".to_string(),
                step_id: "step".to_string(),
                action_id: "action".to_string(),
                agent: "fixer".to_string(),
                summary: "Action requires approval.".to_string(),
                diagnostic: None,
            }),
            agents: Vec::new(),
            chat_items: Vec::new(),
            events: Vec::new(),
            input: input.to_string(),
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
            SkillSuggestion {
                id: "project-alpha".to_string(),
                tag: SkillSourceTag::Project,
                origin: ".agents/skills".to_string(),
            },
            SkillSuggestion {
                id: "personal-beta".to_string(),
                tag: SkillSourceTag::Personal,
                origin: "~/.agents/skills".to_string(),
            },
        ]
    }

    fn write_skill(root: &Path, directory: &str, name: &str) {
        let skill_dir = root.join(directory);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test skill\n---\n"),
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

    fn default_config_status() -> ConfigStatusView {
        ConfigStatusView {
            summary: "Config: sources=1 preset=none warnings=0".to_string(),
            sources: vec!["built-in defaults".to_string()],
            preset: None,
            warnings: Vec::new(),
        }
    }
}
