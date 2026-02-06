pub mod agent_list;
pub mod output_viewer;
pub mod overview;
pub mod session_detail;
pub mod session_list;
pub mod status_bar;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders};

/// Build a styled Block for a panel with consistent focus behavior.
/// Focused: Rounded border in session color, bold title with ▸ prefix.
/// Unfocused: Plain border in gray, gray title.
pub fn panel_block(title: &str, color: Color, is_focused: bool) -> Block<'static> {
    if is_focused {
        Block::default()
            .title(Span::styled(
                format!(" \u{25b8} {} ", title),
                Style::default()
                    .fg(color)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(color))
    } else {
        Block::default()
            .title(Span::styled(
                format!(" {} ", title),
                Style::default().fg(Color::Gray),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(Color::Gray))
    }
}
