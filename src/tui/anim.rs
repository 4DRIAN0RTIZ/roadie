//! Pure animation helpers (fade-in). Time is always an input.
use std::time::Duration;

use ratatui::style::Color;

/// Length of one card's fade-in.
pub const FADE: Duration = Duration::from_millis(700);
/// Extra start delay per card arriving in the same drain, and its cap.
pub const STAGGER_STEP: Duration = Duration::from_millis(40);
pub const STAGGER_MAX: Duration = Duration::from_millis(300);
/// Assumed terminal colors when the theme uses `Color::Reset`.
const ASSUMED_BG: (u8, u8, u8) = (20, 20, 24);
const ASSUMED_FG: (u8, u8, u8) = (220, 220, 220);

/// Smoothstep on `t` clamped to 0..=1: soft start and soft end.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Start delay for the `order`-th card (0-based) arriving in one drain.
pub fn stagger(order: usize) -> Duration {
    STAGGER_STEP
        .checked_mul(order.min(u32::MAX as usize) as u32)
        .unwrap_or(STAGGER_MAX)
        .min(STAGGER_MAX)
}

/// Linear fade progress (0..=1) after `elapsed`.
pub fn progress(elapsed: Duration) -> f32 {
    (elapsed.as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
}

/// True for `COLORTERM=truecolor` or `24bit`.
pub fn truecolor_from(colorterm: Option<&str>) -> bool {
    matches!(colorterm, Some(v) if v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"))
}

/// RGB for a color; `Reset` maps to the assumed background/foreground.
fn rgb(c: Color, reset: (u8, u8, u8)) -> Option<(u8, u8, u8)> {
    Some(match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Reset => reset,
        Color::Black => (0, 0, 0),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::Gray => (229, 229, 229),
        Color::DarkGray => (102, 102, 102),
        Color::White => (255, 255, 255),
        _ => return None,
    })
}

/// Fades `target` in from `background`: `eased` 0 is the background, 1 is
/// exactly `target` (unchanged, so named palette colors are kept at rest).
/// Colors that cannot be resolved to RGB are returned as-is.
pub fn fade_toward(background: Color, target: Color, eased: f32) -> Color {
    if eased >= 1.0 {
        return target;
    }
    let (Some(a), Some(b)) = (rgb(background, ASSUMED_BG), rgb(target, ASSUMED_FG)) else {
        return target;
    };
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * eased.max(0.0)).round() as u8;
    Color::Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ease_in_out_is_soft_at_both_ends() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert_eq!(ease_in_out(2.0), 1.0);
        assert_eq!(ease_in_out(-1.0), 0.0);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-6);
        assert!(ease_in_out(0.1) < 0.1); // slow start
        assert!(ease_in_out(0.9) > 0.9); // slow end
        assert!(ease_in_out(0.3) < ease_in_out(0.6));
    }

    #[test]
    fn stagger_grows_per_card_and_caps() {
        assert_eq!(stagger(0), ms(0));
        assert_eq!(stagger(1), ms(40));
        assert_eq!(stagger(5), ms(200));
        assert_eq!(stagger(7), ms(280));
        assert_eq!(stagger(8), STAGGER_MAX);
        assert_eq!(stagger(1000), STAGGER_MAX);
        assert_eq!(stagger(usize::MAX), STAGGER_MAX);
    }

    #[test]
    fn progress_is_linear_and_clamped() {
        assert_eq!(progress(ms(0)), 0.0);
        assert!((progress(ms(350)) - 0.5).abs() < 1e-6);
        assert_eq!(progress(ms(700)), 1.0);
        assert_eq!(progress(ms(5000)), 1.0);
    }

    #[test]
    fn truecolor_detection() {
        assert!(truecolor_from(Some("truecolor")));
        assert!(truecolor_from(Some("24bit")));
        assert!(truecolor_from(Some("TrueColor")));
        assert!(!truecolor_from(Some("256color")));
        assert!(!truecolor_from(None));
    }

    #[test]
    fn fade_interpolates_rgb() {
        let bg = Color::Rgb(0, 0, 0);
        let fg = Color::Rgb(200, 100, 50);
        assert_eq!(fade_toward(bg, fg, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(fade_toward(bg, fg, 0.5), Color::Rgb(100, 50, 25));
        assert_eq!(fade_toward(bg, fg, 1.0), fg);
    }

    #[test]
    fn fade_resolves_named_and_reset_and_keeps_unknown() {
        // At rest the exact palette color is kept.
        assert_eq!(fade_toward(Color::Reset, Color::Red, 1.0), Color::Red);
        // Mid-fade named colors become RGB, starting from the assumed background.
        assert_eq!(
            fade_toward(Color::Reset, Color::Reset, 0.0),
            Color::Rgb(20, 20, 24)
        );
        assert!(matches!(
            fade_toward(Color::Reset, Color::Red, 0.5),
            Color::Rgb(..)
        ));
        // Indexed colors cannot be interpolated.
        assert_eq!(
            fade_toward(Color::Reset, Color::Indexed(9), 0.2),
            Color::Indexed(9)
        );
    }
}
