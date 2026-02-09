use crate::tui::app::{App, OverviewTile};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let tiles = &app.overview_captures;
    if tiles.is_empty() {
        let msg = Paragraph::new("No active sessions")
            .style(Style::default().fg(Color::Gray));
        f.render_widget(msg, area);
        return;
    }

    let count = tiles.len();
    let cols = compute_columns(count, area.width, area.height);
    let rows = (count + cols - 1) / cols;

    let row_constraints: Vec<Constraint> = (0..rows)
        .map(|_| Constraint::Ratio(1, rows as u32))
        .collect();
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    let mut tile_idx = 0;
    for (row_num, row_area) in row_areas.iter().enumerate() {
        let tiles_in_row = if row_num == rows - 1 {
            count - tile_idx
        } else {
            cols
        };

        let col_constraints: Vec<Constraint> = (0..tiles_in_row)
            .map(|_| Constraint::Ratio(1, tiles_in_row as u32))
            .collect();
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*row_area);

        for col_area in col_areas.iter() {
            if tile_idx >= count {
                break;
            }
            render_tile(f, &tiles[tile_idx], tile_idx == app.overview_selected, *col_area);
            tile_idx += 1;
        }
    }
}

fn render_tile(f: &mut Frame, tile: &OverviewTile, selected: bool, area: Rect) {
    let needs_attention = tile.needs_attention && !selected;

    // 500ms on/off flash cycle for attention indicators
    let flash_on = if needs_attention {
        (std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            / 500)
            % 2
            == 0
    } else {
        false
    };

    let border_color = if selected {
        tile.color
    } else if flash_on {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let border_type = if selected {
        BorderType::Double
    } else if needs_attention {
        BorderType::Thick
    } else {
        BorderType::Plain
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", tile.session_name),
            Style::default()
                .fg(if selected { tile.color } else { Color::Gray })
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ))
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color));

    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;

    let lines: Vec<Line> = tile
        .content
        .lines()
        .rev()
        .take(inner_height)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|l| {
            // Truncate by character count (not byte count) to avoid
            // splitting multi-byte UTF-8 characters like ▜ or █.
            let truncated: String = l.chars().take(inner_width).collect();
            Line::from(Span::styled(
                truncated,
                Style::default().fg(Color::White),
            ))
        })
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

/// Determine optimal column count for the grid.
/// Layout: 1→1x1, 2→2x1, 3→2+1, 4→2x2, 5→3+2, 6→3x2, etc.
pub fn compute_columns(count: usize, _width: u16, _height: u16) -> usize {
    if count <= 1 {
        1
    } else {
        (count as f32).sqrt().ceil() as usize
    }
}
