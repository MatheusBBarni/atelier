use crate::app::{App, AppEvent, AppState};
use crate::config::EffectiveConfig;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum TuiCommand {
    Dispatch(AppEvent),
    DispatchAndQuit(AppEvent),
    ToggleRoster,
    ToggleHelp,
    ScrollEvents(EventScrollCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EventScrollCommand {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
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
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let worker = tokio::spawn(run_app_worker(app, command_receiver));

    let result = run_loop(&mut terminal, state_receiver, command_sender.clone()).await;

    let cleanup_result = (|| -> Result<()> {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
        terminal.draw(|frame| render(frame, &state, &mut ui_state))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if let Some(command) = key_event_to_tui_command_with_ui(&state, &ui_state, key) {
                    if !execute_tui_command(&mut state, &mut ui_state, &command_sender, command)
                        .await?
                    {
                        break;
                    }
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
            state.input.clear();
            Ok(true)
        }
        TuiCommand::ScrollEvents(command) => {
            scroll_events(ui_state, command);
            Ok(true)
        }
        TuiCommand::Dispatch(AppEvent::InputCharacter(ch)) => {
            state.input.push(ch);
            Ok(true)
        }
        TuiCommand::Dispatch(AppEvent::InputBackspace) => {
            state.input.pop();
            Ok(true)
        }
        TuiCommand::Dispatch(event) => {
            if matches_help_command(&event) {
                ui_state.help_visible = !ui_state.help_visible;
                state.input.clear();
                return Ok(true);
            }
            let clears_input = matches!(
                event,
                AppEvent::PromptSubmitted(_) | AppEvent::ApprovalAnswered(_)
            );
            queue_app_event(command_sender, event).await?;
            if clears_input {
                state.input.clear();
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
        } => Some(TuiCommand::ScrollEvents(EventScrollCommand::LineUp)),
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => Some(TuiCommand::ScrollEvents(EventScrollCommand::LineDown)),
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
        } => Some(TuiCommand::Dispatch(AppEvent::InputBackspace)),
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            Some(TuiCommand::Dispatch(AppEvent::InputCharacter(ch)))
        }
        _ => None,
    }
}

fn approval_input_is_yes(input: &str) -> bool {
    matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "approve" | "approved"
    )
}

fn scroll_events(ui_state: &mut TuiUiState, command: EventScrollCommand) {
    let max_scroll = event_max_scroll(ui_state);
    let page = ui_state.event_viewport_lines.saturating_sub(1).max(1);
    match command {
        EventScrollCommand::LineUp => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_sub(1);
            ui_state.event_follow = false;
        }
        EventScrollCommand::LineDown => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_add(1).min(max_scroll);
            ui_state.event_follow = ui_state.event_scroll == max_scroll;
        }
        EventScrollCommand::PageUp => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_sub(page);
            ui_state.event_follow = false;
        }
        EventScrollCommand::PageDown => {
            ui_state.event_scroll = ui_state.event_scroll.saturating_add(page).min(max_scroll);
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
        .constraints([Constraint::Min(6), Constraint::Length(3)])
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

    let input = Paragraph::new(state.input.as_str())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .title(" Input Composer ")
                .title_style(Style::default().fg(Color::Yellow))
                .border_style(Style::default().fg(Color::Yellow))
                .borders(Borders::ALL),
        );
    frame.render_widget(input, outer[1]);
    if ui_state.help_visible {
        render_help_modal(frame);
    } else {
        set_input_cursor(frame, outer[1], state);
    }
}

fn render_event_stream(
    frame: &mut Frame,
    event_area: Rect,
    state: &AppState,
    ui_state: &mut TuiUiState,
) {
    let event_lines = if let Some(pending) = &state.pending_approval {
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
    let area = centered_rect(78, 72, frame.area());
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
        Line::from("Esc                  close this help"),
        Line::from("Enter                submit prompt or answer approval"),
        Line::from("Ctrl-L               show or hide Agent Roster"),
        Line::from("Up/Down             scroll Event Stream one line"),
        Line::from("PageUp/PageDown     scroll Event Stream by page"),
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

fn set_input_cursor(frame: &mut Frame, input_area: ratatui::layout::Rect, state: &AppState) {
    let inner_width = input_area.width.saturating_sub(2);
    let cursor_offset = state.input.chars().count().min(usize::from(inner_width)) as u16;
    frame.set_cursor_position(Position::new(
        input_area.x + 1 + cursor_offset,
        input_area.y + 1,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AgentView;
    use crate::orchestrator::RunState;
    use crate::runtime::{RuntimeAvailability, RuntimeAvailabilityStatus};
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_empty_tui_surfaces() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Idle,
            active_run_id: None,
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
    fn renders_user_prompt_events_with_message_text() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::Running,
            active_run_id: Some("run".to_string()),
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
        let mut ui_state = TuiUiState::default();

        terminal
            .draw(|frame| render(frame, &state, &mut ui_state))
            .unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(5, 22));
    }

    #[test]
    fn renders_pending_approval_prompt() {
        let state = AppState {
            session_id: "session".to_string(),
            run_state: RunState::WaitingForUser,
            active_run_id: Some("run".to_string()),
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
        let mut ui_state = TuiUiState::default();

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
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AppWorkerCommand::Event(AppEvent::PromptSubmitted(prompt)) if prompt == "slow prompt"
        ));
    }

    #[tokio::test]
    async fn help_command_toggles_modal_without_app_event() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut state = state_with_input("/help", false);
        let mut ui_state = TuiUiState::default();

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
        assert!(text.contains("Esc"));
        assert!(text.contains("close this help"));
        assert!(text.contains("Ctrl-L"));
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
            TuiCommand::Dispatch(AppEvent::InputCharacter('x')),
        )
        .await
        .unwrap();
        execute_tui_command(
            &mut state,
            &mut ui_state,
            &sender,
            TuiCommand::Dispatch(AppEvent::InputBackspace),
        )
        .await
        .unwrap();

        assert!(state.input.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn worker_state_sync_preserves_local_input() {
        let mut local_state = state_with_input("draft prompt", false);
        let worker_state = AppState {
            events: vec!["Run started.".to_string()],
            ..state_with_input("", false)
        };
        let (sender, mut receiver) = watch::channel(state_with_input("", false));

        sender.send(worker_state).unwrap();
        sync_worker_state(&mut local_state, &mut receiver);

        assert_eq!(local_state.input, "draft prompt");
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
    fn edit_keys_become_input_app_events() {
        let state = state_with_input("abc", false);

        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::InputCharacter('x')))
        );
        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::InputBackspace))
        );
    }

    #[test]
    fn ctrl_c_is_the_only_exit_key() {
        let state = state_with_input("", false);

        assert_eq!(
            key_event_to_tui_command(
                &state,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
            ),
            Some(TuiCommand::Dispatch(AppEvent::InputCharacter('q')))
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

    fn state_with_input(input: &str, pending_approval: bool) -> AppState {
        AppState {
            session_id: "session".to_string(),
            run_state: if pending_approval {
                RunState::WaitingForUser
            } else {
                RunState::Idle
            },
            active_run_id: pending_approval.then(|| "run".to_string()),
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
}
