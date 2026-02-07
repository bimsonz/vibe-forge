use crate::tui::app::{App, Focus, ViewMode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let running = app.state.running_agents().len();
    let mut right_indicators = String::new();
    if app.state.workspace.is_multi_repo() {
        right_indicators.push_str(&format!(
            "  [{} repos]",
            app.state.workspace.repos.len()
        ));
    }
    if running > 0 {
        right_indicators.push_str(&format!("  {running} agent(s) running"));
    }

    // Show latest notification if recent (< 5 seconds)
    let notification = app.notifications.back().and_then(|n| {
        if n.created_at.elapsed().as_secs() < 5 {
            Some(&n.message)
        } else {
            None
        }
    });

    let dash_key_label = format!("[{}]", app.config.global.dashboard_key_display());
    let overview_key_label = format!("[{}]", app.config.global.overview_key_display());

    let line = if let Some(msg) = notification {
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(msg, Style::default().fg(Color::Yellow)),
        ])
    } else if app.view_mode == ViewMode::SessionOverview {
        // TUI-rendered tiled overview with live capture previews
        let mut spans = vec![];
        spans.push(key_span("[arrows]"));
        spans.push(Span::raw("select "));
        spans.push(key_span("[⏎]"));
        spans.push(Span::raw("open "));
        spans.push(key_span(&dash_key_label));
        spans.push(Span::raw("back "));
        spans.push(key_span("[q]"));
        spans.push(Span::raw("uit"));
        spans.push(Span::styled(
            right_indicators,
            Style::default().fg(Color::Yellow),
        ));
        Line::from(spans)
    } else if app.view_mode == ViewMode::AgentOutput {
        // Full-screen agent output mode
        let mut spans = vec![];
        spans.push(key_span("[j/k]"));
        spans.push(Span::raw("scroll "));
        spans.push(key_span("[c]"));
        spans.push(Span::raw("opy "));
        spans.push(key_span("[Esc]"));
        spans.push(Span::raw("back "));
        spans.push(key_span("[q]"));
        spans.push(Span::raw("uit "));
        spans.push(Span::styled(
            right_indicators,
            Style::default().fg(Color::Yellow),
        ));
        Line::from(spans)
    } else {
        // Dashboard overview — session list shortcuts
        let mut spans = vec![];

        match app.focus {
            Focus::SessionList => {
                spans.push(key_span("[n]"));
                spans.push(Span::raw("ew "));
                spans.push(key_span("[⏎]"));
                spans.push(Span::raw("open "));
                spans.push(key_span(&overview_key_label));
                spans.push(Span::raw("overview "));
                spans.push(key_span("[⌫]"));
                spans.push(Span::raw("kill "));
                spans.push(key_span("[q]"));
                spans.push(Span::raw("uit "));
                spans.push(Span::styled(
                    "Prefix+d/o",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Focus::AgentList => {
                spans.push(key_span("[s]"));
                spans.push(Span::raw("pawn "));
                spans.push(key_span("[⏎]"));
                spans.push(Span::raw("view "));
                spans.push(key_span("[⌫]"));
                spans.push(Span::raw("remove "));
                spans.push(key_span("[Esc]"));
                spans.push(Span::raw("back "));
                spans.push(key_span("[q]"));
                spans.push(Span::raw("uit "));
            }
        }

        spans.push(Span::styled(
            right_indicators,
            Style::default().fg(Color::Yellow),
        ));

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
