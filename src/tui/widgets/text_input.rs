use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::{CursorMove, Input, TextArea};

/// Multiline text editor backed by `ratatui-textarea`.
#[derive(Debug, Clone)]
pub struct TextInput {
    editor: TextArea<'static>,
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new("")
    }
}

impl TextInput {
    pub fn new(text: impl AsRef<str>) -> Self {
        let mut editor = TextArea::from(text.as_ref().split('\n'));
        editor.move_cursor(CursorMove::Jump(u16::MAX, u16::MAX));
        Self { editor }
    }

    pub fn as_str(&self) -> String {
        self.editor.lines().join("\n")
    }

    pub fn input(&mut self, key: KeyEvent) {
        self.editor.input(Input::from(Event::Key(key)));
    }

    pub fn push(&mut self, ch: char) {
        self.input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    pub fn push_str(&mut self, text: &str) {
        self.paste(text);
    }

    pub fn backspace(&mut self) {
        self.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }

    pub fn paste(&mut self, text: &str) {
        self.editor.insert_str(text);
    }

    pub fn set(&mut self, text: impl AsRef<str>) {
        *self = Self::new(text);
    }

    pub fn trim(&self) -> String {
        self.as_str().trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    #[test]
    fn edits_at_the_cursor_without_losing_unicode_or_pasted_text() {
        let mut text = TextInput::new("hi🌍");
        text.input(key(KeyCode::Left));
        text.input(key(KeyCode::Char(' ')));
        text.input(key(KeyCode::Delete));
        text.paste("there\n世界");
        assert_eq!(text.as_str(), "hi there\n世界");
    }

    #[test]
    fn supports_line_navigation_and_home_end() {
        let mut text = TextInput::new("one\ntwo");
        text.input(key(KeyCode::Up));
        text.input(key(KeyCode::Home));
        text.input(key(KeyCode::Char('>')));
        text.input(key(KeyCode::End));
        text.input(key(KeyCode::Char('<')));
        assert_eq!(text.as_str(), ">one<\ntwo");
    }
}
