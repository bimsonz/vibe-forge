use crate::domain::agent::AgentStatus;
use crate::tui::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::OutputViewer;

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
        .title(Span::styled(" Output ", title_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    // Show the selected agent's output, or context about what's happening
    let output_text = if let Some(agent) = app.selected_agent() {
        if let Some(ref result) = agent.result {
            // Agent has output — show it
            result
                .raw_result
                .as_deref()
                .unwrap_or(&result.summary)
                .to_string()
        } else {
            // No output yet — show prompt and status
            match &agent.status {
                AgentStatus::Running => {
                    format!(
                        "Agent '{}' is running...\n\nPrompt: {}",
                        agent.name,
                        truncate(&agent.prompt, 500)
                    )
                }
                AgentStatus::Queued => {
                    format!(
                        "Agent '{}' is queued.\n\nPrompt: {}",
                        agent.name,
                        truncate(&agent.prompt, 500)
                    )
                }
                AgentStatus::Failed(msg) => {
                    format!("Agent '{}' failed: {msg}", agent.name)
                }
                _ => "No output yet.".to_string(),
            }
        }
    } else {
        // No agent selected — try fallback to most recently completed
        let agents = app.selected_session_agents();
        agents
            .iter()
            .filter_map(|a| a.result.as_ref())
            .last()
            .map(|r| {
                r.raw_result
                    .as_deref()
                    .unwrap_or(&r.summary)
                    .to_string()
            })
            .unwrap_or_else(|| "No output yet. Select an agent or press 's' to spawn one.".into())
    };

    let paragraph = Paragraph::new(output_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.output_scroll, 0));

    f.render_widget(paragraph, area);
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
