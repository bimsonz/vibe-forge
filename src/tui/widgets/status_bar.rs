use crate::tui::app::{App, Focus, InputMode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    match &app.input_mode {
        InputMode::Normal => render_normal(f, app, area),
        InputMode::NewSession | InputMode::SpawnAgent => render_input(f, app, area),
        InputMode::ConfirmKillSession => render_confirm_session(f, app, area),
        InputMode::ConfirmKillAgent => render_confirm_agent(f, app, area),
    }
}

fn render_normal(f: &mut Frame, app: &App, area: Rect) {
    let running = app.state.running_agents().len();
    let running_indicator = if running > 0 {
        format!("  {running} agent(s) running")
    } else {
        String::new()
    };

    // Show latest notification if recent (< 5 seconds)
    let notification = app.notifications.back().and_then(|n| {
        if n.created_at.elapsed().as_secs() < 5 {
            Some(&n.message)
        } else {
            None
        }
    });

    let line = if let Some(msg) = notification {
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(msg, Style::default().fg(Color::Yellow)),
        ])
    } else {
        // Context-sensitive shortcuts based on focused panel
        let mut spans = vec![];

        // Always available
        spans.push(key_span("[n]"));
        spans.push(Span::raw("ew "));
        spans.push(key_span("[s]"));
        spans.push(Span::raw("pawn "));

        // Kill: show what it will act on
        match app.focus {
            Focus::SessionList => {
                spans.push(key_span("[K/x]"));
                spans.push(Span::raw("ill session "));
            }
            Focus::AgentList => {
                spans.push(key_span("[K/x]"));
                spans.push(Span::raw("ill agent "));
            }
            Focus::OutputViewer => {
                spans.push(key_span("[j/k]"));
                spans.push(Span::raw("scroll "));
            }
        }

        spans.push(key_span("[a/⏎]"));
        spans.push(Span::raw("ttach "));
        spans.push(key_span("[c]"));
        spans.push(Span::raw("opy "));
        spans.push(key_span("[r]"));
        spans.push(Span::raw("efresh "));
        spans.push(key_span("[q]"));
        spans.push(Span::raw("uit"));

        // Focus indicator
        let focus_label = match app.focus {
            Focus::SessionList => " │ Sessions",
            Focus::AgentList => " │ Agents",
            Focus::OutputViewer => " │ Output",
        };
        spans.push(Span::styled(
            focus_label,
            Style::default().fg(Color::DarkGray),
        ));

        spans.push(Span::styled(running_indicator, Style::default().fg(Color::Yellow)));

        Line::from(spans)
    };

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}

fn key_span(text: &str) -> Span<'_> {
    Span::styled(
        text,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            format!(" {}: ", app.input_label),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&app.input_buffer, Style::default()),
        Span::styled("█", Style::default().fg(Color::Cyan)),
        Span::styled(
            "  (Enter to confirm, Esc to cancel)",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}

fn render_confirm_session(f: &mut Frame, app: &App, area: Rect) {
    let session_name = app
        .selected_session()
        .map(|s| s.name.as_str())
        .unwrap_or("?");

    let line = Line::from(vec![
        Span::styled(
            format!(" Kill session '{session_name}'? "),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        key_span("[y]"),
        Span::raw("es "),
        key_span("[n]"),
        Span::raw("o"),
    ]);

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}

fn render_confirm_agent(f: &mut Frame, app: &App, area: Rect) {
    let agent_name = app
        .selected_agent()
        .map(|a| a.name.as_str())
        .unwrap_or("?");

    let line = Line::from(vec![
        Span::styled(
            format!(" Kill agent '{agent_name}'? "),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        key_span("[y]"),
        Span::raw("es "),
        key_span("[n]"),
        Span::raw("o"),
    ]);

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}
