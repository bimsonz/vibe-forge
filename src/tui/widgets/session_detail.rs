use crate::tui::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let session = app.selected_session();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    if let Some(session) = session {
        let block = block.title(format!(" {} ", session.name));

        let lines = vec![
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&session.branch, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    session.status.to_string(),
                    Style::default()
                        .fg(match &session.status {
                            crate::domain::session::SessionStatus::Active => Color::Green,
                            crate::domain::session::SessionStatus::Failed(_) => Color::Red,
                            crate::domain::session::SessionStatus::Completed => Color::Cyan,
                            _ => Color::Yellow,
                        })
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Worktree: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    session
                        .worktree_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    Style::default(),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Template: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    session.template.as_deref().unwrap_or("none"),
                    Style::default(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Agents: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", app.selected_session_agents().len()),
                    Style::default(),
                ),
            ]),
        ];

        let paragraph = Paragraph::new(lines).block(block);
        f.render_widget(paragraph, area);
    } else {
        let paragraph = Paragraph::new("No session selected")
            .block(block.title(" Session "))
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(paragraph, area);
    }
}
