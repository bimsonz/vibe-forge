use crate::domain::agent::{AgentMode, AgentStatus};
use crate::tui::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::AgentList;
    let agents = app.selected_session_agents();
    let scolor = app.current_session_color();

    let items: Vec<ListItem> = agents
        .iter()
        .map(|agent| {
            let (icon, icon_color) = if agent.mode == AgentMode::Shell {
                (
                    "$",
                    if agent.is_running() {
                        Color::Green
                    } else {
                        Color::DarkGray
                    },
                )
            } else {
                let ic = match &agent.status {
                    AgentStatus::Queued => "○",
                    AgentStatus::Running => "⟳",
                    AgentStatus::Completed => "✓",
                    AgentStatus::Failed(_) => "✗",
                    AgentStatus::Ingested => "✓",
                };
                let ic_color = match &agent.status {
                    AgentStatus::Queued => Color::DarkGray,
                    AgentStatus::Running => Color::Yellow,
                    AgentStatus::Completed => scolor,
                    AgentStatus::Failed(_) => Color::Red,
                    AgentStatus::Ingested => scolor,
                };
                (ic, ic_color)
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
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(duration, Style::default().fg(Color::Gray)),
                Span::raw("  "),
                Span::styled(
                    agent.status.to_string(),
                    Style::default().fg(icon_color),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let title = format!("Agents ({})", agents.len());
    let block = super::panel_block(&title, scolor, is_focused);

    if items.is_empty() {
        let paragraph = ratatui::widgets::Paragraph::new("  No agents. Press 's' to spawn one.")
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
    } else {
        let mut list_state = ListState::default();
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
