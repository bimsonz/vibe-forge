use crate::domain::agent::AgentStatus;
use crate::tui::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::AgentList;
    let agents = app.selected_session_agents();

    let items: Vec<ListItem> = agents
        .iter()
        .map(|agent| {
            let icon = match &agent.status {
                AgentStatus::Queued => "○",
                AgentStatus::Running => "⟳",
                AgentStatus::Completed => "✓",
                AgentStatus::Failed(_) => "✗",
                AgentStatus::Ingested => "✓",
            };

            let icon_color = match &agent.status {
                AgentStatus::Queued => Color::DarkGray,
                AgentStatus::Running => Color::Yellow,
                AgentStatus::Completed => Color::Green,
                AgentStatus::Failed(_) => Color::Red,
                AgentStatus::Ingested => Color::Cyan,
            };

            let duration = agent
                .result
                .as_ref()
                .map(|r| format!(" {:.1}s", r.duration_ms as f64 / 1000.0))
                .unwrap_or_default();

            let line = Line::from(vec![
                Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
                Span::styled(&agent.name, Style::default()),
                Span::styled(
                    format!(" ({})", agent.mode),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(duration, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(
                    agent.status.to_string(),
                    Style::default().fg(icon_color),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title_style = if is_focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" Agents ({}) ", agents.len()),
            title_style,
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    if items.is_empty() {
        let paragraph = ratatui::widgets::Paragraph::new("  No agents. Press 's' to spawn one.")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(paragraph, area);
    } else {
        let mut list_state = ListState::default();
        // Always show selection so user knows which agent is selected
        list_state.select(Some(app.selected_agent));

        let highlight = if is_focused {
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        };

        let list = List::new(items).block(block).highlight_style(highlight);

        f.render_stateful_widget(list, area, &mut list_state);
    }
}
