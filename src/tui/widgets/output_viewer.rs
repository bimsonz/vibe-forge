use crate::domain::agent::AgentStatus;
use crate::tui::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render_fullscreen(f: &mut Frame, app: &App, area: Rect) {
    let scolor = app.current_session_color();

    let agent_title = if let Some(agent) = app.selected_agent() {
        format!("Agent: {} [{}]", agent.name, agent.status)
    } else {
        "Agent Output".to_string()
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" \u{25b8} {} ", agent_title),
            Style::default()
                .fg(scolor)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(scolor));

    let output_text = build_output_text(app);

    let paragraph = Paragraph::new(output_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.output_scroll, 0));

    f.render_widget(paragraph, area);
}

fn build_output_text(app: &App) -> String {
    if let Some(agent) = app.selected_agent() {
        if let Some(ref result) = agent.result {
            result
                .raw_result
                .as_deref()
                .unwrap_or(&result.summary)
                .to_string()
        } else {
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
            .unwrap_or_else(|| {
                "No output yet. Select an agent or press 's' to spawn one.".into()
            })
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
