use crate::domain::session::SessionStatus;
use crate::tui::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::SessionList;

    let sessions: Vec<ListItem> = app
        .state
        .sessions
        .iter()
        .filter(|s| !matches!(s.status, SessionStatus::Archived))
        .map(|session| {
            let icon = match &session.status {
                SessionStatus::Active => "●",
                SessionStatus::Creating => "◐",
                SessionStatus::Paused => "○",
                SessionStatus::Completed => "✓",
                SessionStatus::Failed(_) => "✗",
                SessionStatus::Archived => "▪",
            };

            let icon_color = match &session.status {
                SessionStatus::Active => Color::Green,
                SessionStatus::Creating => Color::Yellow,
                SessionStatus::Completed => Color::Cyan,
                SessionStatus::Failed(_) => Color::Red,
                _ => Color::DarkGray,
            };

            let agent_count = app.state.agents_for_session(session.id).len();
            let agent_suffix = if agent_count > 0 {
                format!(" [{agent_count}]")
            } else {
                String::new()
            };

            let line = Line::from(vec![
                Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
                Span::styled(
                    format!("{}{}", session.name, agent_suffix),
                    Style::default(),
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

    let title = format!(" Sessions ({}) ", app.visible_session_count());
    let block = Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let mut list_state = ListState::default();
    list_state.select(Some(app.selected_session));

    let highlight = if is_focused {
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    };

    let list = List::new(sessions)
        .block(block)
        .highlight_style(highlight);

    f.render_stateful_widget(list, area, &mut list_state);
}
