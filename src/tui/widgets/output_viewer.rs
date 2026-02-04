use crate::tui::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::OutputViewer;

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Output ")
        .borders(Borders::ALL)
        .border_style(border_style);

    // Show the selected agent's output, or the most recently completed agent's output
    let output_text = app
        .selected_agent()
        .and_then(|a| a.result.as_ref())
        .map(|r| r.raw_result.as_deref().unwrap_or(&r.summary))
        .or_else(|| {
            // Fall back to most recently completed agent in this session
            let agents = app.selected_session_agents();
            agents
                .iter()
                .filter_map(|a| a.result.as_ref())
                .last()
                .map(|r| r.raw_result.as_deref().unwrap_or(&r.summary))
        })
        .unwrap_or("No output yet. Agent results will appear here.");

    let paragraph = Paragraph::new(output_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.output_scroll, 0));

    f.render_widget(paragraph, area);
}
