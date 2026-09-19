/// Plain text buffer with push/backspace; `\n` is allowed for multiline use.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TextInput(String);

impl TextInput {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn push(&mut self, ch: char) {
        self.0.push(ch);
    }

    pub fn push_str(&mut self, s: &str) {
        self.0.push_str(s);
    }

    pub fn backspace(&mut self) {
        self.0.pop();
    }

    pub fn set(&mut self, text: impl Into<String>) {
        self.0 = text.into();
    }

    pub fn trim(&self) -> &str {
        self.0.trim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_backspace_and_multiline() {
        let mut t = TextInput::default();
        t.push('a');
        t.push('\n');
        t.push_str("bc");
        assert_eq!(t.as_str(), "a\nbc");
        t.backspace();
        assert_eq!(t.as_str(), "a\nb");
        t.set("  x ");
        assert_eq!(t.trim(), "x");
    }
}
