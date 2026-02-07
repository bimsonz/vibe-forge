use crate::tui::app::{session_color, App};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let session = app.selected_session();

    if let Some(session) = session {
        let scolor = session_color(session.id);

        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", session.name),
                Style::default().fg(scolor).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(scolor));

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(Color::Gray)),
                Span::styled(&session.branch, Style::default().fg(scolor)),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
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
                Span::styled("Worktree: ", Style::default().fg(Color::Gray)),
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
        ];

        // Show per-repo worktree paths for multi-repo sessions
        if !session.repo_worktrees.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Repos: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{}", session.repo_worktrees.len()),
                    Style::default(),
                ),
            ]));
            for (repo_name, _) in &session.repo_worktrees {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(repo_name, Style::default().fg(scolor)),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Template: ", Style::default().fg(Color::Gray)),
            Span::styled(
                session.template.as_deref().unwrap_or("none"),
                Style::default(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Agents: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", app.selected_session_agents().len()),
                Style::default(),
            ),
        ]));

        let paragraph = Paragraph::new(lines).block(block);
        f.render_widget(paragraph, area);
    } else {
        let block = Block::default()
            .title(Span::styled(" Session ", Style::default().fg(Color::Gray)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray));
        let paragraph = Paragraph::new("No session selected")
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
    }
}
