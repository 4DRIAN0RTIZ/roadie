//! Generic ratatui building blocks shared across popups.
mod popup;
mod scroll;
mod text_input;

pub use popup::{centered_rect, draw_frame};
pub use scroll::Scroll;
pub use text_input::TextInput;
