use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear},
    Frame,
};

/// Carves a centered `percent_x` x `percent_y` rectangle out of `area`.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/// Clears `area`, draws a themed bordered block, returns the inner rect.
pub fn draw_frame(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<String>,
    border: Color,
    background: Color,
    foreground: Color,
) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .style(Style::default().bg(background).fg(foreground))
        .border_style(Style::default().fg(border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}
