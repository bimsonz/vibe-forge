pub mod app;
pub mod widgets;

use app::{AgentEntry, AgentSource, App, Focus, InputMode, NotifyLevel, ViewMode};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::commands;
use crate::config;
use crate::domain::agent::{AgentMode, AgentStatus};
use crate::domain::template::AgentTemplate;
use crate::infra::state::StateManager;
use crate::infra::tmux::TmuxController;
use crate::infra::watcher::{ForgeWatcher, WatcherEvent};
use tokio::sync::mpsc;

pub async fn run(workspace_root: PathBuf) -> anyhow::Result<()> {
    let state_manager = StateManager::new(&workspace_root);
    let state = state_manager.load().await?;
    let cfg = config::load_config(Some(&workspace_root))?;

    // Ensure vibe runs inside its tmux session so Enter/Escape window
    // switching works. If we're not already inside, bootstrap into it.
    let tmux_session = state.tmux_session_name.clone();
    let inside_vibe_tmux = if std::env::var("TMUX").is_ok() {
        TmuxController::current_session_name()
            .await
            .map(|name| name == tmux_session)
            .unwrap_or(false)
    } else {
        false
    };
    if !inside_vibe_tmux {
        return bootstrap_into_tmux(&workspace_root, &tmux_session).await;
    }

    // === Running inside the vibe tmux session ===

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(workspace_root.clone(), state, cfg, state_manager);

    // Name this window "dashboard" and bind nav keys to switch back here.
    if let Err(e) = TmuxController::rename_window("dashboard").await {
        tracing::warn!(error = %e, "failed to rename dashboard window");
    }
    if let Err(e) = TmuxController::disable_auto_rename_for(&format!("{}:dashboard", tmux_session)).await {
        tracing::warn!(error = %e, "failed to disable auto-rename");
    }
    if let Err(e) = TmuxController::set_escape_time().await {
        tracing::error!(error = %e, "failed to set escape-time");
    }
    if let Err(e) = TmuxController::enable_extended_keys().await {
        tracing::warn!(error = %e, "failed to enable extended keys");
    }
    if let Err(e) = TmuxController::setup_nav_bindings(
        &tmux_session,
        &app.config.global.dashboard_key,
        &app.config.global.overview_key,
        Some(&workspace_root),
    ).await {
        tracing::error!(error = %e, "nav binding setup failed");
        app.push_notification(
            "Hotkey setup failed — restart vibe to retry".into(),
            NotifyLevel::Error,
        );
    }
    if let Err(e) = TmuxController::hide_status_bar(&tmux_session).await {
        tracing::warn!(error = %e, "failed to hide status bar");
    }
    if let Err(e) = TmuxController::configure_scrollback(&tmux_session).await {
        tracing::warn!(error = %e, "failed to configure scrollback");
    }

    // Ensure the permanent "main" session exists (workspace root, no worktree)
    ensure_main_session(&mut app).await;

    // Start all active session windows in background so they're ready
    start_background_sessions(&mut app).await;

    // Reconcile state with tmux reality (validate pane IDs)
    app.reconcile_tmux_state().await;

    // Start file watcher for agent completion
    let (watcher_tx, mut watcher_rx) = mpsc::unbounded_channel();
    let agents_dir = workspace_root.join(".vibe").join("agents");
    let _watcher = ForgeWatcher::start(agents_dir, watcher_tx).ok();

    // Signal handler: best-effort cleanup on SIGTERM/SIGHUP so bindings don't
    // persist after an unclean shutdown. SIGKILL can't be caught — stale PID
    // locks are detected by verify_nav_bindings and re-claimed automatically.
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let shutdown = shutdown.clone();
        let tmux_session = tmux_session.clone();
        let signal_workspace_root = workspace_root.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate())
                .expect("failed to register SIGTERM handler");
            let mut sighup = signal(SignalKind::hangup())
                .expect("failed to register SIGHUP handler");

            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("SIGTERM received, cleaning up bindings");
                }
                _ = sighup.recv() => {
                    tracing::info!("SIGHUP received, cleaning up bindings");
                }
            }

            let _ = TmuxController::cleanup_nav_bindings(Some(&signal_workspace_root)).await;
            let _ = TmuxController::show_status_bar(&tmux_session).await;
            shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }

    let tick_rate = Duration::from_millis(250);
    let refresh_interval = Duration::from_secs(3);
    let mut last_tick = Instant::now();

    loop {
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
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
                    if let Err(e) = app.state_manager.save(&app.state).await {
                        tracing::error!(error = %e, "failed to save state after agent completion");
                    }

                    // Copy to clipboard
                    if app.config.global.clipboard_on_complete {
                        let text = result.raw_result.as_deref().unwrap_or(&result.summary);
                        let _ = crate::infra::clipboard::copy_text(text);
                    }

                    // OS notification
                    if app.config.global.notify_on_complete {
                        let _ = notify_rust::Notification::new()
                            .summary("Vibe: Agent completed")
                            .body(&result.summary)
                            .show();
                    }

                    app.push_notification(
                        "Agent completed — output copied to clipboard".into(),
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
                app.reconcile_tmux_state().await;

                // Deep-validate nav bindings (checks values + bind-key entries).
                // Recovers from tmux conf reloads, other instances exiting, etc.
                if !TmuxController::verify_nav_bindings(
                    &app.config.global.dashboard_key,
                    &app.config.global.overview_key,
                ).await {
                    tracing::warn!("nav bindings lost or corrupted, re-establishing");

                    // If another instance died and left a stale lock, reclaim it
                    if TmuxController::is_nav_lock_stale(&workspace_root).await {
                        tracing::info!("reclaiming stale nav binding lock");
                    }

                    if let Err(e) = TmuxController::setup_nav_bindings(
                        &tmux_session,
                        &app.config.global.dashboard_key,
                        &app.config.global.overview_key,
                        Some(&workspace_root),
                    ).await {
                        tracing::error!(error = %e, "failed to re-establish nav bindings");
                    } else {
                        app.push_notification(
                            "Keybindings restored".into(),
                            NotifyLevel::Info,
                        );
                    }
                }
            }

            // Periodic overview capture refresh (live content in tiles)
            if app.view_mode == ViewMode::SessionOverview
                && app.overview_last_capture.elapsed() >= Duration::from_millis(750)
            {
                refresh_overview_captures(&mut app).await;
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

    // Clean up tmux bindings and status bar (only if we own the PID lock)
    let _ = TmuxController::show_status_bar(&tmux_session).await;
    let _ = TmuxController::cleanup_nav_bindings(Some(&workspace_root)).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Detach from the tmux session — session windows (and their Claude Code
    // processes) survive. Next `vibe` launch will reconnect to them.
    let _ = TmuxController::detach_client().await;

    Ok(())
}

/// If vibe is not already running inside its tmux session, bootstrap into it:
/// always create a fresh "dashboard" window, launch the current binary, and attach.
/// Any previously running vibe process in the dashboard is killed — session
/// windows (Claude Code) are unaffected and continue running.
async fn bootstrap_into_tmux(
    workspace_root: &std::path::Path,
    tmux_session: &str,
) -> anyhow::Result<()> {
    TmuxController::ensure_session(tmux_session).await?;

    let dashboard_target = format!("{tmux_session}:dashboard");
    let dir = workspace_root.to_str().unwrap_or(".");
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "vibe".to_string());

    // Always kill the old dashboard window to ensure the latest binary runs.
    // Session windows (Claude Code) survive — only the dashboard is replaced.
    if TmuxController::window_exists(&dashboard_target).await {
        let _ = TmuxController::kill_window(&dashboard_target).await;
    }

    // Re-ensure session exists (in case killing the dashboard was the last window)
    TmuxController::ensure_session(tmux_session).await?;

    // Create fresh dashboard and launch the current vibe binary
    TmuxController::create_window(tmux_session, "dashboard", dir).await?;
    TmuxController::send_keys(&dashboard_target, &exe).await?;

    // Attach or switch client depending on our tmux context
    if std::env::var("TMUX").is_ok() {
        TmuxController::switch_client(tmux_session).await?;
    } else {
        TmuxController::attach(tmux_session).await?;
    }

    Ok(())
}

/// Create the permanent "main" session if it doesn't exist yet.
/// This session points at the workspace root (no worktree, no branch creation)
/// and is always pinned first in the session list.
async fn ensure_main_session(app: &mut App) {
    use crate::domain::session::{Session, SessionStatus};

    let has_main = app.state.sessions.iter().any(|s| s.is_main);
    if has_main {
        return;
    }

    let workspace = &app.state.workspace;
    let mut session = Session::new(
        "main".to_string(),
        workspace.default_branch.clone(),
        workspace.root.clone(),
        String::new(), // tmux_window filled by start_background_sessions
    );
    session.is_main = true;
    session.status = SessionStatus::Active;

    // Insert at front so it's always first
    app.state.sessions.insert(0, session);

    if let Err(e) = app.state_manager.save(&app.state).await {
        tracing::error!(error = %e, "failed to save state after creating main session");
    }
}

/// Ensure all active sessions have their tmux windows ready with Claude running.
/// This allows users to immediately interact with any session after opening dashboard.
async fn start_background_sessions(app: &mut App) {
    use crate::domain::session::SessionStatus;

    let tmux_session = app.state.tmux_session_name.clone();

    for session in app.state.sessions.clone() {
        // Skip archived sessions
        if matches!(session.status, SessionStatus::Archived) {
            continue;
        }

        let session_target = format!("{tmux_session}:{}", session.name);
        let working_dir = session.worktree_path.to_str().unwrap_or(".");

        // Only create window if it doesn't exist
        if TmuxController::window_exists(&session_target).await {
            continue;
        }

        // Create the window
        match TmuxController::create_window(&tmux_session, &session.name, working_dir).await {
            Ok(window_id) => {
                // Lock the window name so tmux doesn't auto-rename it
                let _ = TmuxController::disable_auto_rename_for(&session_target).await;

                // Build and send claude command
                let resolved_system_prompt = resolve_session_system_prompt(&session, app);

                let cmd = crate::infra::claude::interactive_command(
                    resolved_system_prompt.as_deref(),
                    &[],
                    &[],
                    None,
                    None,
                    &app.config.global.claude_extra_args,
                );

                if let Err(e) = TmuxController::send_keys(&session_target, &cmd).await {
                    tracing::warn!(
                        session = %session.name,
                        error = %e,
                        "failed to start claude in background session"
                    );
                }

                // Update session state
                if let Some(s) = app.state.find_session_by_name_mut(&session.name) {
                    s.tmux_window = window_id;
                    s.status = SessionStatus::Active;
                }
            }
            Err(e) => {
                tracing::warn!(
                    session = %session.name,
                    error = %e,
                    "failed to create background window"
                );
            }
        }
    }

    // Save updated state
    if let Err(e) = app.state_manager.save(&app.state).await {
        tracing::error!(error = %e, "failed to save state after background session startup");
    }

    // Ensure dashboard is the active window after background setup
    // (create_window may have switched the active window to a session)
    let dashboard_target = format!("{}:dashboard", tmux_session);
    let _ = TmuxController::select_window(&dashboard_target).await;
}

/// Resolve the system prompt for a session (shared helper)
fn resolve_session_system_prompt(
    session: &crate::domain::session::Session,
    app: &App,
) -> Option<String> {
    if let Some(ref sp) = session.system_prompt_override {
        Some(sp.clone())
    } else if let Some(ref tmpl_name) = session.template {
        let template_dirs = app.config.template_dirs(&app.workspace_root);
        crate::domain::template::AgentTemplate::load(tmpl_name, &template_dirs)
            .ok()
            .map(|t| t.system_prompt)
    } else {
        None
    }
}

fn draw(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    // Main layout: banner + body + status bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // banner (1 pad + 6 art + 1 pad)
            Constraint::Min(5),    // body
            Constraint::Length(1), // status bar
        ])
        .split(size);

    // Banner
    render_banner(f, main_chunks[0]);

    // Body depends on view mode
    match app.view_mode {
        ViewMode::Dashboard => {
            draw_dashboard(f, app, main_chunks[1]);
        }
        ViewMode::SessionOverview => {
            widgets::overview::render(f, app, main_chunks[1]);
        }
        ViewMode::AgentOutput => {
            widgets::output_viewer::render_fullscreen(f, app, main_chunks[1]);
        }
    }

    // Status bar
    widgets::status_bar::render(f, app, main_chunks[2]);

    // Popup overlay (rendered last, on top of everything)
    render_popup_overlay(f, app, size);
}

fn draw_dashboard(f: &mut ratatui::Frame, app: &App, area: Rect) {
    // Left panel (sessions) + right panel (detail + agents + output)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(28), // session list (fixed width)
            Constraint::Min(30),   // detail area (fills remaining)
        ])
        .split(area);

    // Session list (left)
    widgets::session_list::render(f, app, body_chunks[0]);

    // Right panel: detail + agents
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // session detail
            Constraint::Min(4),    // agent list (expands to fill)
        ])
        .split(body_chunks[1]);

    widgets::session_detail::render(f, app, right_chunks[0]);
    widgets::agent_list::render(f, app, right_chunks[1]);
}

const BANNER: [&str; 6] = [
    "██╗   ██╗██╗██████╗ ███████╗    ███████╗ ██████╗ ██████╗  ██████╗ ███████╗",
    "██║   ██║██║██╔══██╗██╔════╝    ██╔════╝██╔═══██╗██╔══██╗██╔════╝ ██╔════╝",
    "██║   ██║██║██████╔╝█████╗      █████╗  ██║   ██║██████╔╝██║  ███╗█████╗  ",
    "╚██╗ ██╔╝██║██╔══██╗██╔══╝      ██╔══╝  ██║   ██║██╔══██╗██║   ██║██╔══╝  ",
    " ╚████╔╝ ██║██████╔╝███████╗    ██║     ╚██████╔╝██║  ██║╚██████╔╝███████╗",
    "  ╚═══╝  ╚═╝╚═════╝ ╚══════╝    ╚═╝      ╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚══════╝",
];

/// Interpolate between two colors at position t (0.0 to 1.0).
fn lerp_color(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let r = (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8;
    let g = (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8;
    let b_val = (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8;
    Color::Rgb(r, g, b_val)
}

/// Compute gradient color for a horizontal position.
/// Left: hot pink (255, 0, 128) → Center: purple (128, 0, 255) → Right: cyan (0, 200, 255)
fn gradient_color(x: usize, width: usize) -> Color {
    let t = if width <= 1 {
        0.0
    } else {
        x as f32 / (width - 1) as f32
    };
    if t < 0.5 {
        lerp_color((255, 0, 128), (128, 0, 255), t * 2.0)
    } else {
        lerp_color((128, 0, 255), (0, 200, 255), (t - 0.5) * 2.0)
    }
}

fn render_banner(f: &mut ratatui::Frame, area: Rect) {
    // Calculate the character width of the banner (first line)
    let banner_char_width = BANNER[0].chars().count();
    let area_width = area.width as usize;

    // Build lines with gradient coloring, centered
    let mut lines: Vec<Line> = Vec::with_capacity(8);
    lines.push(Line::from("")); // top padding

    for art_line in &BANNER {
        let chars: Vec<char> = art_line.chars().collect();
        let pad_left = area_width.saturating_sub(chars.len()) / 2;

        let mut spans: Vec<Span> = Vec::with_capacity(chars.len() + 1);
        if pad_left > 0 {
            spans.push(Span::raw(" ".repeat(pad_left)));
        }
        for (i, ch) in chars.iter().enumerate() {
            let color = gradient_color(i, banner_char_width);
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from("")); // bottom padding

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

// ─── Popup overlay ───────────────────────────────────────────────────────────

fn render_popup_overlay(f: &mut ratatui::Frame, app: &App, area: Rect) {
    match &app.input_mode {
        InputMode::Normal => {}
        InputMode::NewSession | InputMode::SpawnAgent => {
            render_input_popup(f, app, area);
        }
        InputMode::SelectTemplate => {
            render_template_picker(f, app, area);
        }
        InputMode::ConfirmKillSession => {
            let session_name = app
                .selected_session()
                .map(|s| s.name.as_str())
                .unwrap_or("?");
            let content = Line::from(vec![
                Span::styled(
                    format!("Kill session '{session_name}'?  "),
                    Style::default().fg(Color::White),
                ),
                key_span("[Enter]"),
                Span::raw(" confirm  "),
                key_span("[Esc]"),
                Span::raw(" cancel"),
            ]);
            render_popup(f, " Confirm ", content, area);
        }
        InputMode::ConfirmKillAgent => {
            let agent_name = app
                .selected_agent()
                .map(|a| a.name.as_str())
                .unwrap_or("?");
            let content = Line::from(vec![
                Span::styled(
                    format!("Kill agent '{agent_name}'?  "),
                    Style::default().fg(Color::White),
                ),
                key_span("[Enter]"),
                Span::raw(" confirm  "),
                key_span("[Esc]"),
                Span::raw(" cancel"),
            ]);
            render_popup(f, " Confirm ", content, area);
        }
    }
}

fn render_input_popup(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let content = Line::from(vec![
        Span::styled(
            format!("{}: ", app.input_label),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&app.input_buffer, Style::default().fg(Color::White)),
        Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
        Span::styled(
            "  [Enter] confirm  [Esc] cancel",
            Style::default().fg(Color::Gray),
        ),
    ]);
    render_popup(f, &format!(" {} ", app.input_label), content, area);
}

fn render_popup(f: &mut ratatui::Frame, title: &str, content: Line<'_>, area: Rect) {
    let popup_width = 60.min(area.width.saturating_sub(4));
    let popup_height = 3;
    let popup_area = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(content).block(block);
    f.render_widget(paragraph, popup_area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn key_span(text: &str) -> Span<'_> {
    Span::styled(
        text,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

fn render_template_picker(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let entries = &app.agent_entries;

    let popup_width = 50u16.min(area.width.saturating_sub(4));
    // borders(2) + entries + separator(1) + custom(1) + footer(1)
    let inner_lines = entries.len() + 3;
    let popup_height = ((inner_lines + 2) as u16).min(area.height.saturating_sub(2));
    let popup_area = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(Span::styled(
            " Spawn Agent ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner_width = (popup_width as usize).saturating_sub(4);
    let mut lines = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let selected = i == app.selected_template;
        let marker = if selected { "\u{25b8} " } else { "  " };
        let name_padded = format!("{:<14}", entry.name);
        let style = if selected {
            Style::default()
                .fg(app.current_session_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let desc_style = if selected {
            Style::default().fg(app.current_session_color())
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(name_padded, style),
            Span::styled(entry.description.clone(), desc_style),
        ]));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(inner_width),
        Style::default().fg(Color::DarkGray),
    )));

    // Custom prompt option
    let custom_selected = app.selected_template == entries.len();
    let marker = if custom_selected { "\u{25b8} " } else { "  " };
    let style = if custom_selected {
        Style::default()
            .fg(app.current_session_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(Span::styled(
        format!("{marker}Custom prompt..."),
        style,
    )));

    // Footer
    lines.push(Line::from(vec![
        Span::styled(
            "  \u{23ce} ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("select  "),
        Span::styled(
            "Esc ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("cancel"),
    ]));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, popup_area);
}

// ─── Key handling ────────────────────────────────────────────────────────────

/// Handle a key event. Returns true if the app should quit.
async fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<bool> {
    match &app.input_mode {
        InputMode::Normal => handle_normal_key(app, code, modifiers).await,
        InputMode::NewSession => handle_input_key(app, code).await,
        InputMode::SpawnAgent => handle_input_key(app, code).await,
        InputMode::SelectTemplate => handle_select_template_key(app, code).await,
        InputMode::ConfirmKillSession => handle_confirm_kill_session(app, code).await,
        InputMode::ConfirmKillAgent => handle_confirm_kill_agent(app, code).await,
    }
}

async fn handle_normal_key(
    app: &mut App,
    code: KeyCode,
    _modifiers: KeyModifiers,
) -> anyhow::Result<bool> {
    // Full-screen agent output mode — restricted keys
    if app.view_mode == ViewMode::AgentOutput {
        return handle_agent_output_key(app, code).await;
    }

    // Session overview mode — restricted keys
    if app.view_mode == ViewMode::SessionOverview {
        return handle_overview_key(app, code).await;
    }

    match code {
        // Quit
        KeyCode::Char('q') => return Ok(true),

        // Esc: back navigation
        KeyCode::Esc => match app.focus {
            Focus::AgentList => {
                app.focus = Focus::SessionList;
            }
            Focus::SessionList => {}
        },

        // Navigation: j/k/arrows
        KeyCode::Char('j') | KeyCode::Down => match app.focus {
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
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focus {
            Focus::SessionList => {
                let count = app.visible_session_count();
                if count > 0 {
                    app.selected_session =
                        app.selected_session.checked_sub(1).unwrap_or(count - 1);
                    app.selected_agent = 0;
                    app.output_scroll = 0;
                }
            }
            Focus::AgentList => {
                let count = app.selected_session_agents().len();
                if count > 0 {
                    app.selected_agent =
                        app.selected_agent.checked_sub(1).unwrap_or(count - 1);
                    app.output_scroll = 0;
                }
            }
        },

        // Focus cycling
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::SessionList => Focus::AgentList,
                Focus::AgentList => Focus::SessionList,
            };
        }
        KeyCode::BackTab => {
            app.focus = match app.focus {
                Focus::SessionList => Focus::AgentList,
                Focus::AgentList => Focus::SessionList,
            };
        }

        // Enter: primary action
        KeyCode::Enter => match app.focus {
            Focus::SessionList => {
                do_open_session(app).await?;
            }
            Focus::AgentList => {
                if let Some(agent) = app.selected_agent() {
                    if agent.mode == AgentMode::Shell && agent.is_running() {
                        // Shell agent — has its own tmux window: "{session}~{shell_name}"
                        let session = app.selected_session().unwrap();
                        let tmux_session = app.state.tmux_session_name.clone();
                        let shell_target = format!(
                            "{}:{}~{}",
                            tmux_session, session.name, agent.name
                        );
                        let _ = TmuxController::select_window(&shell_target).await;
                    } else if agent.tmux_pane.is_some() && agent.is_running() {
                        // Interactive agent — switch to session window + select its pane
                        let session = app.selected_session().unwrap();
                        let tmux_session = app.state.tmux_session_name.clone();
                        let tmux_target =
                            format!("{}:{}", tmux_session, session.name);
                        let pane_id = agent.tmux_pane.clone();
                        let _ = TmuxController::select_window(&tmux_target).await;
                        if let Some(ref pid) = pane_id {
                            let _ = TmuxController::select_pane(pid).await;
                        }
                    } else {
                        // Headless agent with output — fullscreen view
                        app.view_mode = ViewMode::AgentOutput;
                        app.output_scroll = 0;
                    }
                }
            }
        },

        // Backspace: kill/remove (context-sensitive)
        KeyCode::Backspace => match app.focus {
            Focus::SessionList => {
                if let Some(session) = app.selected_session() {
                    if session.is_main {
                        app.push_notification(
                            "Cannot kill the main session".into(),
                            NotifyLevel::Error,
                        );
                    } else {
                        app.input_mode = InputMode::ConfirmKillSession;
                    }
                }
            }
            Focus::AgentList => {
                do_remove_or_kill_agent(app).await?;
            }
        },

        // New session
        KeyCode::Char('n') => {
            app.input_mode = InputMode::NewSession;
            app.input_buffer.clear();
            app.input_label = "New session name".to_string();
        }

        // Spawn agent (template picker)
        KeyCode::Char('s') => {
            if app.selected_session().is_some() {
                app.agent_entries = load_agent_entries(&app.workspace_root, &app.config);
                app.selected_template = 0;
                app.input_mode = InputMode::SelectTemplate;
            } else {
                app.push_notification("No session selected".into(), NotifyLevel::Error);
            }
        }

        // Copy agent output
        KeyCode::Char('c') => {
            do_copy(app);
        }

        // Refresh
        KeyCode::Char('r') => {
            app.refresh_state().await;
            app.push_notification("State refreshed".into(), NotifyLevel::Info);
        }

        // Session overview — § is the internal trigger character, sent by tmux
        // when the overview user-key (\e[33~) is pressed.
        KeyCode::Char('§') => {
            if app.visible_session_count() > 0 {
                enter_overview(app).await?;
            }
        }

        _ => {}
    }

    Ok(false)
}

/// Handle keys in full-screen agent output view
async fn handle_agent_output_key(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    match code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Esc => {
            app.view_mode = ViewMode::Dashboard;
            app.output_scroll = 0;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.output_scroll = app.output_scroll.saturating_add(1);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.output_scroll = app.output_scroll.saturating_sub(1);
        }
        KeyCode::Char('c') => {
            do_copy(app);
        }
        _ => {}
    }
    Ok(false)
}

// ─── Actions ─────────────────────────────────────────────────────────────────

/// Open the selected session by switching to its tmux window.
/// If the window doesn't exist, recreate it and start claude.
/// The TUI keeps running in its own window — use tmux keybindings
/// (e.g. Ctrl-B + window number) to switch back.
async fn do_open_session(app: &mut App) -> anyhow::Result<()> {
    let session = match app.selected_session() {
        Some(s) => s,
        None => return Ok(()),
    };

    let session_name = session.name.clone();
    let worktree_path = session.worktree_path.clone();
    let template_name = session.template.clone();
    let system_prompt_override = session.system_prompt_override.clone();
    let tmux_session = app.state.tmux_session_name.clone();

    // Ensure tmux session exists
    if let Err(e) = TmuxController::ensure_session(&tmux_session).await {
        app.push_notification(format!("tmux error: {e}"), NotifyLevel::Error);
        return Ok(());
    }

    let tmux_target = format!("{tmux_session}:{session_name}");

    // If window doesn't exist, recreate it
    if TmuxController::select_window(&tmux_target).await.is_err() {
        let working_dir = worktree_path.to_str().unwrap_or(".").to_string();

        match TmuxController::create_window(&tmux_session, &session_name, &working_dir).await {
            Ok(window_id) => {
                // Lock the window name so tmux doesn't auto-rename it
                let _ = TmuxController::disable_auto_rename_for(&tmux_target).await;

                // Build claude command
                let resolved_system_prompt = if let Some(ref sp) = system_prompt_override {
                    Some(sp.clone())
                } else if let Some(ref tmpl_name) = template_name {
                    let template_dirs = app.config.template_dirs(&app.workspace_root);
                    crate::domain::template::AgentTemplate::load(tmpl_name, &template_dirs)
                        .ok()
                        .map(|t| t.system_prompt)
                } else {
                    None
                };

                let cmd = crate::infra::claude::interactive_command(
                    resolved_system_prompt.as_deref(),
                    &[],
                    &[],
                    None,
                    None,
                    &app.config.global.claude_extra_args,
                );
                let _ = TmuxController::send_keys(&tmux_target, &cmd).await;

                // Update session in state
                if let Some(s) = app.state.find_session_by_name_mut(&session_name) {
                    s.tmux_window = window_id;
                    s.status = crate::domain::session::SessionStatus::Active;
                }
                if let Err(e) = app.state_manager.save(&app.state).await {
                    tracing::error!(error = %e, "failed to save state after opening session");
                }
            }
            Err(e) => {
                app.push_notification(
                    format!("Failed to create window: {e}"),
                    NotifyLevel::Error,
                );
                return Ok(());
            }
        }
    }

    // We're always inside the vibe tmux session (bootstrapped).
    // select_window (line 673) already switched to an existing window,
    // or create_window made the new window active. The tmux Escape binding
    // handles returning to the dashboard.
    app.push_notification(
        format!("Opened '{session_name}'"),
        NotifyLevel::Success,
    );

    Ok(())
}

/// Open a shell in its own tmux window, associated with the selected session.
/// Each shell gets a dedicated window named `{session}~shell-N` so the
/// main session's Claude Code window is never disturbed.
async fn do_open_shell(app: &mut App) -> anyhow::Result<()> {
    use crate::domain::agent::Agent;

    let session = match app.selected_session() {
        Some(s) => s,
        None => return Ok(()),
    };

    let session_name = session.name.clone();
    let session_id = session.id;
    let worktree = session.worktree_path.clone();
    let tmux_session = app.state.tmux_session_name.clone();
    let working_dir = worktree.to_str().unwrap_or(".").to_string();

    // Auto-name: shell-1, shell-2, etc. (use max existing number to avoid collisions)
    let max_shell_num = app
        .state
        .agents_for_session(session_id)
        .iter()
        .filter(|a| a.mode == AgentMode::Shell)
        .filter_map(|a| a.name.strip_prefix("shell-").and_then(|n| n.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    let shell_name = format!("shell-{}", max_shell_num + 1);

    // Each shell gets its own tmux window: "{session}~shell-N"
    let window_name = format!("{}~{}", session_name, shell_name);
    let window_target = format!("{tmux_session}:{window_name}");

    match TmuxController::create_window(&tmux_session, &window_name, &working_dir).await {
        Ok(_window_id) => {}
        Err(e) => {
            app.push_notification(format!("Failed to create shell window: {e}"), NotifyLevel::Error);
            return Ok(());
        }
    }

    // Lock the window name so tmux doesn't auto-rename it
    let _ = TmuxController::disable_auto_rename_for(&window_target).await;

    // Get the pane ID of the new window (for reconciliation/status tracking)
    let pane_id = TmuxController::first_pane_id(&window_target)
        .await
        .unwrap_or_default();

    // Create agent entry for the shell
    let output_dir = app.workspace_root.join(".vibe").join("agents");
    let mut agent = Agent::new(
        session_id,
        shell_name.clone(),
        AgentMode::Shell,
        String::new(),
        worktree,
        output_dir,
    );
    agent.status = AgentStatus::Running;
    agent.tmux_pane = Some(pane_id);

    app.state.agents.push(agent);
    if let Err(e) = app.state_manager.save(&app.state).await {
        tracing::error!(error = %e, "failed to save state after opening shell");
    }
    app.push_notification(format!("Shell '{shell_name}' opened"), NotifyLevel::Success);

    // Switch to the new shell window
    let _ = TmuxController::select_window(&window_target).await;

    Ok(())
}

/// Enter TUI-rendered overview mode.
/// Captures pane content from each session's tmux window and switches to overview view.
/// No pane movement — purely rendered inside the TUI using capture-pane snapshots.
async fn enter_overview(app: &mut App) -> anyhow::Result<()> {
    use crate::tui::app::OverviewTile;

    let tmux_session = app.state.tmux_session_name.clone();
    let mut tiles = Vec::new();

    let visible: Vec<_> = app.visible_sessions().into_iter().cloned().collect();

    for session in &visible {
        let session_target = format!("{tmux_session}:{}", session.name);

        if !TmuxController::window_exists(&session_target).await {
            continue;
        }

        let pane_id = match TmuxController::first_pane_id(&session_target).await {
            Ok(id) if !id.is_empty() => id,
            _ => continue,
        };

        let content = TmuxController::capture_pane(&pane_id, 50)
            .await
            .unwrap_or_default();

        tiles.push(OverviewTile {
            session_name: session.name.clone(),
            pane_id,
            content,
            color: app::session_color(session.id),
        });
    }

    if tiles.is_empty() {
        app.push_notification("No active session windows to show".into(), NotifyLevel::Error);
        return Ok(());
    }

    app.overview_captures = tiles;
    app.overview_selected = 0;
    app.overview_last_capture = Instant::now();
    app.view_mode = ViewMode::SessionOverview;

    Ok(())
}

/// Re-capture pane content for all overview tiles (periodic live refresh).
async fn refresh_overview_captures(app: &mut App) {
    for tile in &mut app.overview_captures {
        if let Ok(content) = TmuxController::capture_pane(&tile.pane_id, 50).await {
            tile.content = content;
        }
    }
    app.overview_last_capture = Instant::now();
}

/// Handle keys in the session overview view
async fn handle_overview_key(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    let tile_count = app.overview_captures.len();
    if tile_count == 0 {
        app.view_mode = ViewMode::Dashboard;
        return Ok(false);
    }

    // Compute columns for grid navigation (must match overview widget layout)
    let cols = crate::tui::widgets::overview::compute_columns(tile_count, 0, 0);

    match code {
        KeyCode::Char('q') => return Ok(true),

        KeyCode::Esc => {
            app.view_mode = ViewMode::Dashboard;
            app.overview_captures.clear();
        }

        KeyCode::Right | KeyCode::Char('l') => {
            app.overview_selected = (app.overview_selected + 1) % tile_count;
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.overview_selected = app
                .overview_selected
                .checked_sub(1)
                .unwrap_or(tile_count - 1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let next = app.overview_selected + cols;
            if next < tile_count {
                app.overview_selected = next;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.overview_selected >= cols {
                app.overview_selected -= cols;
            }
        }

        // Enter: switch to selected session's tmux window
        KeyCode::Enter => {
            if let Some(tile) = app.overview_captures.get(app.overview_selected) {
                let tmux_session = app.state.tmux_session_name.clone();
                let target = format!("{}:{}", tmux_session, tile.session_name);
                let _ = TmuxController::select_window(&target).await;
            }
            app.view_mode = ViewMode::Dashboard;
            app.overview_captures.clear();
        }

        _ => {}
    }

    Ok(false)
}

/// Handle backspace on an agent: if dead, remove immediately. If running, prompt to kill.
async fn do_remove_or_kill_agent(app: &mut App) -> anyhow::Result<()> {
    let agent = match app.selected_agent() {
        Some(a) => a,
        None => return Ok(()),
    };

    match &agent.status {
        // Dead agents — remove immediately, no confirmation
        AgentStatus::Completed | AgentStatus::Failed(_) | AgentStatus::Ingested => {
            let agent_id = agent.id;
            let agent_name = agent.name.clone();

            app.state.agents.retain(|a| a.id != agent_id);
            if let Err(e) = app.state_manager.save(&app.state).await {
                tracing::error!(error = %e, "failed to save state after removing agent");
            }

            // Adjust selection
            let agent_count = app.selected_session_agents().len();
            if agent_count > 0 && app.selected_agent >= agent_count {
                app.selected_agent = agent_count - 1;
            } else if agent_count == 0 {
                app.selected_agent = 0;
            }

            app.push_notification(
                format!("Removed '{agent_name}'"),
                NotifyLevel::Info,
            );
        }
        // Live agents — show kill confirmation
        AgentStatus::Running | AgentStatus::Queued => {
            app.input_mode = InputMode::ConfirmKillAgent;
        }
    }

    Ok(())
}

/// Copy the selected agent's output to clipboard
fn do_copy(app: &mut App) {
    if let Some(agent) = app.selected_agent() {
        if let Some(ref result) = agent.result {
            let text = result.raw_result.as_deref().unwrap_or(&result.summary);
            match crate::infra::clipboard::copy_text(text) {
                Ok(()) => {
                    app.push_notification(
                        "Output copied to clipboard".into(),
                        NotifyLevel::Success,
                    );
                }
                Err(e) => {
                    app.push_notification(format!("Copy failed: {e}"), NotifyLevel::Error);
                }
            }
        } else {
            app.push_notification("No output to copy".into(), NotifyLevel::Error);
        }
    } else {
        app.push_notification("No agent selected".into(), NotifyLevel::Error);
    }
}

// ─── Template loading ────────────────────────────────────────────────────────

/// Scan `.claude/agents/*.md` (Claude Code project agents) and forge built-in
/// templates, returning a unified list of spawnable agents.
fn load_agent_entries(
    workspace_root: &std::path::Path,
    config: &crate::config::MergedConfig,
) -> Vec<AgentEntry> {
    let mut entries = Vec::new();

    // 0. Shell option — always first
    entries.push(AgentEntry {
        name: "Shell".to_string(),
        description: "Open a terminal in the session worktree".to_string(),
        source: AgentSource::Shell,
    });

    // 1. Claude Code project agents: .claude/agents/*.md
    let agents_dir = workspace_root.join(".claude").join("agents");
    if let Ok(dir_entries) = std::fs::read_dir(&agents_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let desc = content
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .trim()
                            .trim_start_matches('#')
                            .trim()
                            .to_string();
                        entries.push(AgentEntry {
                            name: stem.to_string(),
                            description: desc,
                            source: AgentSource::ClaudeCode(content),
                        });
                    }
                }
            }
        }
    }

    // 2. Forge templates (workspace overrides > user global > built-ins)
    let dirs = config.template_dirs(workspace_root);
    let templates = AgentTemplate::load_all(&dirs);
    for t in templates {
        entries.push(AgentEntry {
            name: t.name,
            description: t.description,
            source: AgentSource::ForgeTemplate,
        });
    }

    entries
}

// ─── Input / Confirm handlers ────────────────────────────────────────────────

async fn handle_input_key(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    match code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.input_buffer.clear();
        }
        KeyCode::Enter => {
            let raw_input = app.input_buffer.clone();
            if raw_input.is_empty() {
                app.input_mode = InputMode::Normal;
                return Ok(false);
            }

            match app.input_mode.clone() {
                InputMode::NewSession => {
                    // Sanitize: replace spaces with hyphens, lowercase
                    let input = raw_input
                        .trim()
                        .replace(' ', "-")
                        .to_lowercase();
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
                        raw_input.clone(),
                        session_name,
                        None, // template
                        None, // system_prompt
                        false,
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

async fn handle_select_template_key(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    let total = app.agent_entries.len() + 1; // +1 for "Custom prompt..."

    match code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.agent_entries.clear();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.selected_template = (app.selected_template + 1) % total;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.selected_template = app.selected_template.checked_sub(1).unwrap_or(total - 1);
        }
        KeyCode::Enter => {
            let idx = app.selected_template;
            if idx < app.agent_entries.len() {
                // Check if this is a Shell entry
                if matches!(app.agent_entries[idx].source, AgentSource::Shell) {
                    app.input_mode = InputMode::Normal;
                    app.agent_entries.clear();
                    do_open_shell(app).await?;
                } else {
                    // Extract needed values before mutating app
                    let entry_name = app.agent_entries[idx].name.clone();
                    let entry_desc = app.agent_entries[idx].description.clone();
                    let template_name = match &app.agent_entries[idx].source {
                        AgentSource::ForgeTemplate => Some(entry_name.clone()),
                        AgentSource::ClaudeCode(_) | AgentSource::Shell => None,
                    };
                    let system_prompt = match &app.agent_entries[idx].source {
                        AgentSource::ClaudeCode(content) => Some(content.clone()),
                        AgentSource::ForgeTemplate | AgentSource::Shell => None,
                    };
                    let session_name = app.selected_session().map(|s| s.name.clone());

                    app.input_mode = InputMode::Normal;
                    app.agent_entries.clear();

                    match commands::spawn::execute(
                        &app.workspace_root,
                        entry_desc,
                        session_name,
                        template_name,
                        system_prompt,
                        false,
                        &app.config,
                    )
                    .await
                    {
                        Ok(()) => {
                            app.refresh_state().await;
                            app.push_notification(
                                format!("Agent '{entry_name}' spawned"),
                                NotifyLevel::Success,
                            );
                        }
                        Err(e) => {
                            app.push_notification(format!("Error: {e}"), NotifyLevel::Error);
                        }
                    }
                }
            } else {
                // "Custom prompt..." — switch to free text input
                app.input_mode = InputMode::SpawnAgent;
                app.input_buffer.clear();
                app.input_label = "Agent prompt".to_string();
                app.agent_entries.clear();
            }
        }
        _ => {}
    }

    Ok(false)
}

async fn handle_confirm_kill_session(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    match code {
        KeyCode::Enter => {
            if let Some(session) = app.selected_session() {
                let name = session.name.clone();
                app.input_mode = InputMode::Normal;
                match commands::kill::execute(
                    &app.workspace_root,
                    name.clone(),
                    true,  // force
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
                        app.selected_agent = 0;
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
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(false)
}

async fn handle_confirm_kill_agent(app: &mut App, code: KeyCode) -> anyhow::Result<bool> {
    match code {
        KeyCode::Enter => {
            if let Some(agent) = app.selected_agent() {
                let agent_id = agent.id;
                let agent_name = agent.name.clone();
                let agent_mode = agent.mode.clone();
                app.input_mode = InputMode::Normal;

                // Kill the agent's tmux resources
                if agent_mode == AgentMode::Shell {
                    // Shell agents have their own window — kill the whole window
                    if let Some(session) = app.selected_session() {
                        let tmux_session = app.state.tmux_session_name.clone();
                        let window_target = format!(
                            "{}:{}~{}",
                            tmux_session, session.name, agent_name
                        );
                        if let Err(e) = TmuxController::kill_window(&window_target).await {
                            tracing::warn!(window = %window_target, error = %e, "failed to kill shell window");
                        }
                    }
                } else if let Some(ref pane) = app.selected_agent().and_then(|a| a.tmux_pane.clone()) {
                    // Interactive agents use a pane inside the session window
                    if let Err(e) = TmuxController::kill_pane(pane).await {
                        tracing::warn!(pane = %pane, error = %e, "failed to kill agent pane");
                    }
                }

                // Remove the agent from state entirely
                app.state.agents.retain(|a| a.id != agent_id);
                if let Err(e) = app.state_manager.save(&app.state).await {
                    tracing::error!(error = %e, "failed to save state after killing agent");
                }

                // Adjust selection
                let agent_count = app.selected_session_agents().len();
                if agent_count > 0 && app.selected_agent >= agent_count {
                    app.selected_agent = agent_count - 1;
                } else if agent_count == 0 {
                    app.selected_agent = 0;
                }

                app.push_notification(
                    format!("Agent '{agent_name}' killed and removed"),
                    NotifyLevel::Success,
                );
            }
        }
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(false)
}
