use crate::domain::session::SessionStatus;
use crate::tui::app::{session_color, App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::SessionList;

    let sessions: Vec<ListItem> = app
        .visible_sessions()
        .iter()
        .map(|session| {
            let scolor = session_color(session.id);

            let icon = if session.is_main {
                "★"
            } else {
                match &session.status {
                    SessionStatus::Active => "●",
                    SessionStatus::Creating => "◐",
                    SessionStatus::Paused => "○",
                    SessionStatus::Completed => "✓",
                    SessionStatus::Failed(_) => "✗",
                    SessionStatus::Archived => "▪",
                }
            };

            let icon_color = match &session.status {
                SessionStatus::Failed(_) => Color::Red,
                _ => scolor,
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
                    Style::default().fg(scolor),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let focused_color = app.current_session_color();
    let title = format!("Sessions ({})", app.visible_session_count());
    let block = super::panel_block(&title, focused_color, is_focused);

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
