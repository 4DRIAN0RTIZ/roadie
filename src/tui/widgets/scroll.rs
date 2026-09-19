/// Vertical scroll offset shared by scrollable popups.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Scroll(u16);

impl Scroll {
    pub fn offset(self) -> u16 {
        self.0
    }

    /// Moves the offset by `delta` rows, clamped at 0.
    pub fn by(&mut self, delta: i32) {
        self.0 = (self.0 as i32 + delta).clamp(0, u16::MAX as i32) as u16;
    }

    pub fn reset(&mut self) {
        self.0 = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_clamps_at_zero() {
        let mut s = Scroll::default();
        s.by(-3);
        assert_eq!(s.offset(), 0);
        s.by(5);
        s.by(-2);
        assert_eq!(s.offset(), 3);
        s.reset();
        assert_eq!(s.offset(), 0);
    }
}
