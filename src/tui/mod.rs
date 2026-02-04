pub mod app;
pub mod widgets;

use app::{App, Focus, InputMode, NotifyLevel};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::commands;
use crate::config;
use crate::domain::agent::AgentStatus;
use crate::infra::state::StateManager;
use crate::infra::watcher::{ForgeWatcher, WatcherEvent};
use tokio::sync::mpsc;

pub async fn run(workspace_root: PathBuf) -> anyhow::Result<()> {
    let state_manager = StateManager::new(&workspace_root);
    let state = state_manager.load().await?;
    let cfg = config::load_config(Some(&workspace_root))?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(workspace_root.clone(), state, cfg, state_manager);

    // Start file watcher for agent completion
    let (watcher_tx, mut watcher_rx) = mpsc::unbounded_channel();
    let agents_dir = workspace_root.join(".forge").join("agents");
    let _watcher = ForgeWatcher::start(agents_dir, watcher_tx).ok();

    let tick_rate = Duration::from_millis(250);
    let refresh_interval = Duration::from_secs(3);
    let mut last_tick = Instant::now();

    loop {
        // Draw
        terminal.draw(|f| draw(f, &app))?;

        // Poll for events
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if handle_key(&mut app, key.code, key.modifiers).await? {
                    break;
                }
            }
        }

        // Check for watcher events (non-blocking)
        while let Ok(event) = watcher_rx.try_recv() {
            match event {
                WatcherEvent::AgentCompleted { agent_id, result } => {
                    // Update agent in state
                    if let Some(agent) = app.state.find_agent_by_id_mut(agent_id) {
                        agent.status = AgentStatus::Completed;
                        agent.completed_at = Some(chrono::Utc::now());
                        agent.result = Some(result.clone());
                    }
                    app.state_manager.save(&app.state).await.ok();

                    // Copy to clipboard
                    if app.config.global.clipboard_on_complete {
                        let text = result.raw_result.as_deref().unwrap_or(&result.summary);
                        let _ = crate::infra::clipboard::copy_text(text);
                    }

                    // OS notification
                    if app.config.global.notify_on_complete {
                        let _ = notify_rust::Notification::new()
                            .summary("Forge: Agent completed")
                            .body(&result.summary)
                            .show();
                    }

                    app.push_notification(
                        format!("Agent completed — output copied to clipboard"),
                        NotifyLevel::Success,
                    );
                }
                WatcherEvent::AgentOutputWritten { .. } => {
                    app.refresh_state().await;
                }
            }
        }

        // Tick
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();

            // Periodic state refresh
            if app.last_refresh.elapsed() >= refresh_interval {
                app.refresh_state().await;
            }

            // Expire old notifications
            while app
                .notifications
                .front()
                .is_some_and(|n| n.created_at.elapsed().as_secs() > 10)
            {
                app.notifications.pop_front();
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn draw(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    // Main layout: header + body + status bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(5),   // body
            Constraint::Length(1), // status bar
        ])
        .split(size);

    // Header
    render_header(f, app, main_chunks[0]);

    // Body: left panel (sessions) + right panel (detail + agents + output)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // session list
            Constraint::Percentage(75), // detail area
        ])
        .split(main_chunks[1]);

    // Session list (left)
    widgets::session_list::render(f, app, body_chunks[0]);

    // Right panel: detail + agents + output
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // session detail
            Constraint::Length(8),  // agent list
            Constraint::Min(4),    // output viewer
        ])
        .split(body_chunks[1]);

    widgets::session_detail::render(f, app, right_chunks[0]);
    widgets::agent_list::render(f, app, right_chunks[1]);
    widgets::output_viewer::render(f, app, right_chunks[2]);

    // Status bar
    widgets::status_bar::render(f, app, main_chunks[2]);
}

fn render_header(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let active = app.state.active_sessions().len();
    let running = app.state.running_agents().len();

    let line = Line::from(vec![
        Span::styled(
            " forge",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(": {} ", app.state.workspace.name),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("({active} sessions, {running} agents)"),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}

/// Handle a key event. Returns true if the app should quit.
async fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<bool> {
    match &app.input_mode {
        InputMode::Normal => handle_normal_key(app, code, modifiers).await,
        InputMode::NewSession => handle_input_key(app, code).await,
        InputMode::SpawnAgent => handle_input_key(app, code).await,
        InputMode::ConfirmKill => handle_confirm_key(app, code).await,
    }
}

async fn handle_normal_key(
    app: &mut App,
    code: KeyCode,
    _modifiers: KeyModifiers,
) -> anyhow::Result<bool> {
    match code {
        // Quit
        KeyCode::Char('q') => return Ok(true),

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            match app.focus {
                Focus::SessionList => {
                    let count = app.visible_session_count();
                    if count > 0 {
                        app.selected_session = (app.selected_session + 1) % count;
                        app.selected_agent = 0;
                        app.output_scroll = 0;
                    }
                }
                Focus::AgentList => {
                    let count = app.selected_session_agents().len();
                    if count > 0 {
                        app.selected_agent = (app.selected_agent + 1) % count;
                        app.output_scroll = 0;
                    }
                }
                Focus::OutputViewer => {
                    app.output_scroll = app.output_scroll.saturating_add(1);
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            match app.focus {
                Focus::SessionList => {
                    let count = app.visible_session_count();
                    if count > 0 {
                        app.selected_session = app.selected_session.checked_sub(1).unwrap_or(count - 1);
                        app.selected_agent = 0;
                        app.output_scroll = 0;
                    }
                }
                Focus::AgentList => {
                    let count = app.selected_session_agents().len();
                    if count > 0 {
                        app.selected_agent = app.selected_agent.checked_sub(1).unwrap_or(count - 1);
                        app.output_scroll = 0;
                    }
                }
                Focus::OutputViewer => {
                    app.output_scroll = app.output_scroll.saturating_sub(1);
                }
            }
        }

        // Focus cycling
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::SessionList => Focus::AgentList,
                Focus::AgentList => Focus::OutputViewer,
                Focus::OutputViewer => Focus::SessionList,
            };
        }
        KeyCode::BackTab => {
            app.focus = match app.focus {
                Focus::SessionList => Focus::OutputViewer,
                Focus::AgentList => Focus::SessionList,
                Focus::OutputViewer => Focus::AgentList,
            };
        }

        // New session
        KeyCode::Char('n') => {
            app.input_mode = InputMode::NewSession;
            app.input_buffer.clear();
            app.input_label = "New session name".to_string();
        }

        // Spawn agent
        KeyCode::Char('s') => {
            if app.selected_session().is_some() {
                app.input_mode = InputMode::SpawnAgent;
                app.input_buffer.clear();
                app.input_label = "Agent prompt".to_string();
            } else {
                app.push_notification("No session selected".into(), NotifyLevel::Error);
            }
        }

        // Kill session
        KeyCode::Char('K') => {
            if app.selected_session().is_some() {
                app.input_mode = InputMode::ConfirmKill;
            }
        }

        // Attach to session
        KeyCode::Char('a') => {
            if let Some(session) = app.selected_session() {
                let session_name = session.name.clone();
                let tmux_session = app.state.tmux_session_name.clone();

                // Restore terminal before attaching
                disable_raw_mode()?;
                execute!(io::stdout(), LeaveAlternateScreen)?;

                let tmux_target = format!("{tmux_session}:{session_name}");
                let _ = crate::infra::tmux::TmuxController::select_window(&tmux_target).await;
                let _ = crate::infra::tmux::TmuxController::attach(&tmux_session).await;

                // Re-setup terminal after detach
                enable_raw_mode()?;
                execute!(io::stdout(), EnterAlternateScreen)?;
                app.refresh_state().await;
            }
        }

        // Copy agent output
        KeyCode::Char('c') => {
            if let Some(agent) = app.selected_agent() {
                if let Some(ref result) = agent.result {
                    let text = result.raw_result.as_deref().unwrap_or(&result.summary);
                    match crate::infra::clipboard::copy_text(text) {
                        Ok(()) => {
                            app.push_notification("Output copied to clipboard".into(), NotifyLevel::Success);
                        }
                        Err(e) => {
                            app.push_notification(format!("Copy failed: {e}"), NotifyLevel::Error);
                        }
                    }
                } else {
                    app.push_notification("No output to copy".into(), NotifyLevel::Error);
                }
            }
        }

        // Refresh
        KeyCode::Char('r') => {
            app.refresh_state().await;
            app.push_notification("State refreshed".into(), NotifyLevel::Info);
        }

        _ => {}
    }

    Ok(false)
}

async fn handle_input_key(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    match code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.input_buffer.clear();
        }
        KeyCode::Enter => {
            let input = app.input_buffer.clone();
            if input.is_empty() {
                app.input_mode = InputMode::Normal;
                return Ok(false);
            }

            match app.input_mode.clone() {
                InputMode::NewSession => {
                    app.input_mode = InputMode::Normal;
                    match commands::new::execute(
                        &app.workspace_root,
                        input.clone(),
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        &app.config,
                    )
                    .await
                    {
                        Ok(()) => {
                            app.refresh_state().await;
                            app.push_notification(
                                format!("Session '{input}' created"),
                                NotifyLevel::Success,
                            );
                        }
                        Err(e) => {
                            app.push_notification(format!("Error: {e}"), NotifyLevel::Error);
                        }
                    }
                }
                InputMode::SpawnAgent => {
                    app.input_mode = InputMode::Normal;
                    let session_name = app.selected_session().map(|s| s.name.clone());
                    match commands::spawn::execute(
                        &app.workspace_root,
                        input.clone(),
                        session_name,
                        None, // template
                        false, // interactive
                        &app.config,
                    )
                    .await
                    {
                        Ok(()) => {
                            app.refresh_state().await;
                            app.push_notification("Agent spawned".into(), NotifyLevel::Success);
                        }
                        Err(e) => {
                            app.push_notification(format!("Error: {e}"), NotifyLevel::Error);
                        }
                    }
                }
                _ => {}
            }
            app.input_buffer.clear();
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
        }
        _ => {}
    }
    Ok(false)
}

async fn handle_confirm_key(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(session) = app.selected_session() {
                let name = session.name.clone();
                app.input_mode = InputMode::Normal;
                match commands::kill::execute(
                    &app.workspace_root,
                    name.clone(),
                    true, // force
                    false, // don't delete branch
                    &app.config,
                )
                .await
                {
                    Ok(()) => {
                        app.refresh_state().await;
                        if app.selected_session >= app.visible_session_count().saturating_sub(1) {
                            app.selected_session = app.visible_session_count().saturating_sub(1);
                        }
                        app.push_notification(
                            format!("Session '{name}' killed"),
                            NotifyLevel::Success,
                        );
                    }
                    Err(e) => {
                        app.push_notification(format!("Error: {e}"), NotifyLevel::Error);
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(false)
}
