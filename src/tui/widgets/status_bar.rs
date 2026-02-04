use crate::tui::app::{App, InputMode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    match &app.input_mode {
        InputMode::Normal => render_normal(f, app, area),
        InputMode::NewSession | InputMode::SpawnAgent => render_input(f, app, area),
        InputMode::ConfirmKill => render_confirm(f, app, area),
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
        Line::from(vec![
            Span::styled(
                " [n]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("ew "),
            Span::styled(
                "[s]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("pawn "),
            Span::styled(
                "[k]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("ill "),
            Span::styled(
                "[a]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("ttach "),
            Span::styled(
                "[c]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("opy "),
            Span::styled(
                "[q]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("uit"),
            Span::styled(running_indicator, Style::default().fg(Color::Yellow)),
        ])
    };

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
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

fn render_confirm(f: &mut Frame, app: &App, area: Rect) {
    let session_name = app
        .selected_session()
        .map(|s| s.name.as_str())
        .unwrap_or("?");

    let line = Line::from(vec![
        Span::styled(
            format!(" Kill session '{session_name}'? "),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled("[y]", Style::default().fg(Color::Cyan)),
        Span::raw("es "),
        Span::styled("[n]", Style::default().fg(Color::Cyan)),
        Span::raw("o"),
    ]);

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}
