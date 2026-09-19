//! Semantic color roles. `ui.rs` resolves every color through the `Theme`.
use ratatui::style::Color;

use crate::domain::Column;

#[derive(Debug, Clone)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub highlight: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub border_active: Color,
    pub border_popup: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Reset,
            muted: Color::DarkGray,
            success: Color::Green,
            warning: Color::Yellow,
            danger: Color::Red,
            highlight: Color::Cyan,
            selection_bg: Color::Cyan,
            selection_fg: Color::Black,
            border_active: Color::Cyan,
            border_popup: Color::Magenta,
        }
    }
}

impl Theme {
    pub fn column_color(&self, col: Column) -> Color {
        match col {
            Column::Idea => Color::Blue,
            Column::Planned => Color::Magenta,
            Column::Now => self.warning,
            Column::Working => self.danger,
            Column::Done => self.success,
        }
    }
}
