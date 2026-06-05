use crate::app::{App, AppEvent, AppState};
use crate::config::EffectiveConfig;
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
use std::io::{self, IsTerminal};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

const USER_EVENT_BG: Color = Color::Rgb(18, 52, 71);
const INPUT_COMPOSER_HEIGHT: u16 = 5;
const MOUSE_SCROLL_LINES: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
enum TuiCommand {
    Dispatch(AppEvent),
    DispatchAndQuit(AppEvent),
    ToggleRoster,
    ToggleHelp,
    ScrollEvents(EventScrollCommand),
    MoveInputCursor(InputCursorCommand),
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
        }
    }
}

pub async fn run_tui(config: EffectiveConfig, debug_enabled: bool) -> Result<()> {
    if !io::stdout().is_terminal() {
        println!("multiagent TUI requires an interactive terminal. Use --doctor or --print-config for non-interactive checks.");
        return Ok(());
    }

    let mut app = App::new_with_debug(config, debug_enabled).await?;
    let (state_sender, state_receiver) = watch::channel(app.state().clone());
    app.attach_state_sender(state_sender);
    let (command_sender, command_receiver) = mpsc::channel(1024);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let worker = tokio::spawn(run_app_worker(app, command_receiver));

    let result = run_loop(&mut terminal, state_receiver, command_sender.clone()).await;

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
) -> Result<()> {
    let mut state = state_receiver.borrow_and_update().clone();
    let mut ui_state = TuiUiState::default();
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
                if !execute_tui_command(&mut state, &mut ui_state, &command_sender, command).await?
                {
                    break;
                }
            }
        }
    }
    Ok(())
}

async fn execute_tui_command(
    state: &mut AppState,
    ui_state: &mut TuiUiState,
    command_sender: &mpsc::Sender<AppWorkerCommand>,
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
    if !ui_state.help_visible {
        return key_event_to_tui_command(state, key);
    }

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
                        Span::styled(agent.status.as_str(), status_style(&agent.status)),
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

    render_event_stream(frame, event_area, state, ui_state);

    let input_layout = input_layout(outer[1], &state.input, ui_state.input_cursor);
    ui_state.input_width = input_layout.width;
    let input_title = format!(" Input Composer | {} ", state.config_status.summary);
    let input = Paragraph::new(wrapped_input_lines(&state.input, input_layout.width))
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .title(input_title)
                .title_style(Style::default().fg(Color::Yellow))
                .border_style(Style::default().fg(Color::Yellow))
                .borders(Borders::ALL),
        )
        .scroll((input_layout.scroll.min(usize::from(u16::MAX)) as u16, 0));
    frame.render_widget(input, outer[1]);
    if ui_state.help_visible {
        render_help_modal(frame);
    } else {
        set_input_cursor(frame, outer[1], input_layout);
    }
}

fn render_event_stream(
    frame: &mut Frame,
    event_area: Rect,
    state: &AppState,
    ui_state: &mut TuiUiState,
) {
    let mut event_lines = if let Some(pending) = &state.pending_approval {
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
        vec![Line::from("No events yet.")]
    } else {
        state
            .events
            .iter()
            .map(|event| event_stream_line(event))
            .collect::<Vec<_>>()
    };
    if let Some(live_step) = &state.live_step {
        let mut live_lines = vec![Line::from(vec![
            Span::styled("Active step ", Style::default().fg(Color::LightGreen)),
            Span::styled(
                format!("{} ", live_step.agent),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("run:{} step:{}", live_step.run_id, live_step.step_id),
                Style::default().fg(Color::DarkGray),
            ),
        ])];
        if live_step.streams.is_empty() {
            live_lines.push(Line::from("  waiting for runtime output"));
        } else {
            for stream in &live_step.streams {
                let marker = if stream.final_delta { "final" } else { "live" };
                live_lines.push(Line::from(format!(
                    "  [{}:{}] {}",
                    stream.stream, marker, stream.content
                )));
            }
        }
        live_lines.push(Line::from(""));
        live_lines.extend(event_lines);
        event_lines = live_lines;
    }
    let block = Block::default()
        .title(" Event Stream ")
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
        Line::from("/goal <text> | /goal | /goal clear   manage session goal"),
        Line::from("/subtask <agent> <task>              run bounded child task"),
        Line::from("/config              show config files, preset, warnings"),
        Line::from("Esc                  close this help"),
        Line::from("Enter                submit prompt or answer approval"),
        Line::from("Ctrl-L               show or hide Agent Roster"),
        Line::from("Arrow keys           move input cursor"),
        Line::from("PageUp/PageDown     scroll Event Stream by page"),
        Line::from("Mouse wheel         scroll Event Stream by line"),
        Line::from("Home/End            jump Event Stream to top/latest"),
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
        Line::from("multiagent                         open the TUI"),
        Line::from("multiagent --cwd <path>            run from a workspace"),
        Line::from("multiagent --config <path>         use a config file"),
        Line::from("multiagent --doctor [--json]       check runtimes and history"),
        Line::from("multiagent --print-config          print merged config"),
        Line::from("multiagent --init-config           create config files"),
        Line::from("multiagent --codemap init|changes|update manage repo maps"),
        Line::from("multiagent --clean-sessions [--yes] delete local history"),
        Line::from("multiagent --debug                 write debug events"),
        Line::from("multiagent --help                  print CLI help"),
    ];
    let help = Paragraph::new(lines)
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .block(
            Block::default()
                .title(" Help ")
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

fn event_stream_line(event: &str) -> Line<'_> {
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
        "running" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "waiting_approval" | "waiting_for_user" => Style::default().fg(Color::Yellow),
        "interrupted" => Style::default().fg(Color::Red),
        "disabled" => Style::default().fg(Color::DarkGray),
        _ => Style::default().fg(Color::Gray),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputLayout {
    width: usize,
    cursor_col: u16,
    cursor_row: u16,
    scroll: usize,
}

fn input_layout(input_area: Rect, input: &str, cursor: usize) -> InputLayout {
    let width = usize::from(input_area.width.saturating_sub(2).max(1));
    let visible_rows = usize::from(input_area.height.saturating_sub(2).max(1));
    let cursor_cells = cursor.min(input_char_count(input));
    let cursor_line = cursor_cells / width;
    let cursor_col = cursor_cells % width;
    let scroll = cursor_line.saturating_sub(visible_rows.saturating_sub(1));
    let visible_cursor_row = cursor_line.saturating_sub(scroll);
    InputLayout {
        width,
        cursor_col: cursor_col.min(width.saturating_sub(1)) as u16,
        cursor_row: visible_cursor_row.min(visible_rows.saturating_sub(1)) as u16,
        scroll,
    }
}

fn wrapped_input_lines(input: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    if input.is_empty() {
        return vec![Line::from("")];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0usize;
    for ch in input.chars() {
        line.push(ch);
        line_len += 1;
        if line_len == width {
            lines.push(Line::from(std::mem::take(&mut line)));
            line_len = 0;
        }
    }
    if !line.is_empty() || input.chars().count().is_multiple_of(width) {
        lines.push(Line::from(line));
    }
    lines
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
    use crate::app::{AgentView, ConfigStatusView, LiveStepView, LiveStreamView};
    use crate::orchestrator::RunState;
    use crate::runtime::{RuntimeAvailability, RuntimeAvailabilityStatus};
    use ratatui::backend::TestBackend;

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
            events: Vec::new(),
            input: String::new(),
        };
        let text = render_to_text(&state, 100, 24);
        assert!(text.contains("Agent Roster"));
        assert!(text.contains("Event Stream"));
        assert!(text.contains("Input Composer"));
        assert!(text.contains("No events yet."));
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
    fn renders_live_step_stream_detail_above_events() {
        let mut state = state_with_input("", false);
        state.live_step = Some(LiveStepView {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            agent: "fixer".to_string(),
            streams: vec![LiveStreamView {
                stream: "stdout".to_string(),
                content: "compiling target".to_string(),
                final_delta: false,
            }],
        });
        state.events = vec!["Fixer step started.".to_string()];

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("Active step"));
        assert!(text.contains("fixer"));
        assert!(text.contains("[stdout:live] compiling target"));
        assert!(text.contains("Fixer step started."));
    }

    #[test]
    fn renders_config_status_footer_at_80x24_and_120x40() {
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

        assert!(small.contains("Config: sources=2 preset=research warnings=1"));
        assert!(large.contains("Config: sources=2 preset=research warnings=1"));
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
            events: vec!["You: build a feature".to_string()],
            input: String::new(),
        };

        let text = render_to_text(&state, 100, 24);

        assert!(text.contains("You"));
        assert!(text.contains("build a feature"));
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
            .assert_cursor_position(Position::new(5, 20));
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
        assert!(text.contains("abcdefghijklmnopqrstuv"));
        assert!(text.contains("wxyz1234"));
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(9, 9));
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
        assert!(text.contains("/help + Enter"));
        assert!(text.contains("/goal <text>"));
        assert!(text.contains("/goal clear"));
        assert!(text.contains("/subtask <agent>"));
        assert!(text.contains("/config"));
        assert!(text.contains("Esc"));
        assert!(text.contains("Mouse wheel"));
        assert!(text.contains("close this help"));
        assert!(text.contains("Ctrl-L"));
        assert!(text.contains("Arrow keys"));
        assert!(text.contains("PageUp/PageDown"));
        assert!(text.contains("Home/End"));
        assert!(text.contains("multiagent --doctor"));
        assert!(text.contains("multiagent --clean-sessions"));
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
        let line = event_stream_line("You: build a feature");

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
        assert_eq!(ui_state.input_width, 22);

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

        assert_eq!(state.input, "abcdefgXhijklmnopqrstuvwxyz1234");
        assert_eq!(ui_state.input_cursor, 8);
        assert!(receiver.try_recv().is_err());

        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(9, 8));
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
        assert!(text.contains("Event Stream"));
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

    fn default_config_status() -> ConfigStatusView {
        ConfigStatusView {
            summary: "Config: sources=1 preset=none warnings=0".to_string(),
            sources: vec!["built-in defaults".to_string()],
            preset: None,
            warnings: Vec::new(),
        }
    }
}
