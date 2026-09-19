//! Visual-only effects registry for the board (entrance, flashes, selection
//! blend, count flashes, popup fade). State changes never wait on it: this
//! only records start times; the renderer asks for a level at a given time.
//! Everything takes `now` as input so it is deterministic in tests.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::anim;

/// Delay between neighbouring columns on board entrance.
pub const COLUMN_STAGGER: Duration = Duration::from_millis(80);
/// Cap on the total entrance delay of any card (column + row stagger).
pub const BOARD_DELAY_MAX: Duration = Duration::from_millis(500);
pub const FLASH: Duration = Duration::from_millis(500);
pub const SELECT: Duration = Duration::from_millis(120);
pub const COUNT_FLASH: Duration = Duration::from_millis(500);
pub const POPUP_FADE: Duration = Duration::from_millis(150);

/// Entrance delay of the card at (`col`, `row`): column stagger plus the
/// per-card stagger, capped.
pub fn board_delay(col: usize, row: usize) -> Duration {
    let cols = COLUMN_STAGGER
        .checked_mul(col.min(u32::MAX as usize) as u32)
        .unwrap_or(BOARD_DELAY_MAX);
    (cols + anim::stagger(row)).min(BOARD_DELAY_MAX)
}

/// Eased progress 0..=1 of an effect that started at `start` and lasts
/// `dur`; 0 before it starts (delays), 1 once finished.
pub fn level(start: Instant, dur: Duration, now: Instant) -> f32 {
    let t = now.saturating_duration_since(start).as_secs_f32() / dur.as_secs_f32();
    anim::ease_in_out(t)
}

fn finished(start: Instant, dur: Duration, now: Instant) -> bool {
    now.saturating_duration_since(start) >= dur && now >= start
}

#[derive(Debug, Default)]
pub struct Effects {
    /// Effects only run on truecolor terminals; otherwise every level is 1.
    pub enabled: bool,
    board: Option<Instant>,
    flashes: HashMap<u64, Instant>,
    select: Option<(u64, Instant)>,
    counts: HashMap<usize, Instant>,
    popup: Option<Instant>,
}

impl Effects {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }

    pub fn start_board(&mut self, now: Instant) {
        if self.enabled {
            self.board = Some(now);
        }
    }

    /// 1.0 when the board is not entering (or effects are off).
    pub fn board_level(&self, col: usize, row: usize, now: Instant) -> f32 {
        match self.board {
            Some(t) if self.enabled => level(t + board_delay(col, row), anim::FADE, now),
            _ => 1.0,
        }
    }

    pub fn flash(&mut self, number: u64, now: Instant) {
        if self.enabled {
            self.flashes.insert(number, now);
        }
    }

    /// 0 = just flashed, 1 = back to normal.
    pub fn flash_level(&self, number: u64, now: Instant) -> f32 {
        self.flashes
            .get(&number)
            .map_or(1.0, |t| level(*t, FLASH, now))
    }

    /// Starts the selection blend on `number`. Restarts cleanly (single slot,
    /// never queued); re-selecting the same card keeps its running blend.
    pub fn select(&mut self, number: u64, now: Instant) {
        if self.enabled && self.select.map(|s| s.0) != Some(number) {
            self.select = Some((number, now));
        }
    }

    pub fn select_level(&self, number: u64, now: Instant) -> f32 {
        match self.select {
            Some((n, t)) if n == number => level(t, SELECT, now),
            _ => 1.0,
        }
    }

    pub fn count_flash(&mut self, col: usize, now: Instant) {
        if self.enabled {
            self.counts.insert(col, now);
        }
    }

    pub fn count_level(&self, col: usize, now: Instant) -> f32 {
        self.counts
            .get(&col)
            .map_or(1.0, |t| level(*t, COUNT_FLASH, now))
    }

    pub fn start_popup(&mut self, now: Instant) {
        if self.enabled {
            self.popup = Some(now);
        }
    }

    pub fn popup_level(&self, now: Instant) -> f32 {
        match self.popup {
            Some(t) if self.enabled => level(t, POPUP_FADE, now),
            _ => 1.0,
        }
    }

    /// Drops finished entries so the registry stays small.
    pub fn prune(&mut self, now: Instant) {
        self.flashes.retain(|_, t| !finished(*t, FLASH, now));
        self.counts.retain(|_, t| !finished(*t, COUNT_FLASH, now));
        if self
            .board
            .is_some_and(|t| finished(t, anim::FADE + BOARD_DELAY_MAX, now))
        {
            self.board = None;
        }
        if self.select.is_some_and(|(_, t)| finished(t, SELECT, now)) {
            self.select = None;
        }
        if self.popup.is_some_and(|t| finished(t, POPUP_FADE, now)) {
            self.popup = None;
        }
    }

    /// True while anything still animates (drives the fast redraw tick).
    pub fn is_active(&self, now: Instant) -> bool {
        self.enabled
            && (self
                .board
                .is_some_and(|t| !finished(t, anim::FADE + BOARD_DELAY_MAX, now))
                || self.flashes.values().any(|t| !finished(*t, FLASH, now))
                || self
                    .counts
                    .values()
                    .any(|t| !finished(*t, COUNT_FLASH, now))
                || self.select.is_some_and(|(_, t)| !finished(t, SELECT, now))
                || self.popup.is_some_and(|t| !finished(t, POPUP_FADE, now)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn board_delay_staggers_columns_then_rows_and_caps() {
        assert_eq!(board_delay(0, 0), ms(0));
        assert_eq!(board_delay(1, 0), ms(80));
        assert_eq!(board_delay(0, 2), ms(80));
        assert_eq!(board_delay(2, 3), ms(160 + 120));
        assert_eq!(board_delay(4, 8), BOARD_DELAY_MAX);
        assert_eq!(board_delay(usize::MAX, usize::MAX), BOARD_DELAY_MAX);
        // left-to-right and top-to-bottom ordering
        assert!(board_delay(1, 0) > board_delay(0, 0));
        assert!(board_delay(0, 1) > board_delay(0, 0));
    }

    #[test]
    fn level_waits_for_delay_then_eases_to_one() {
        let t0 = Instant::now();
        assert_eq!(level(t0 + ms(100), ms(200), t0), 0.0);
        assert_eq!(level(t0, ms(200), t0), 0.0);
        assert!((level(t0, ms(200), t0 + ms(100)) - 0.5).abs() < 1e-6);
        assert_eq!(level(t0, ms(200), t0 + ms(200)), 1.0);
        assert_eq!(level(t0, ms(200), t0 + ms(900)), 1.0);
    }

    #[test]
    fn disabled_effects_are_inert() {
        let t0 = Instant::now();
        let mut fx = Effects::new(false);
        fx.start_board(t0);
        fx.flash(1, t0);
        fx.select(1, t0);
        fx.count_flash(0, t0);
        fx.start_popup(t0);
        assert_eq!(fx.board_level(0, 0, t0), 1.0);
        assert_eq!(fx.flash_level(1, t0), 1.0);
        assert_eq!(fx.select_level(1, t0), 1.0);
        assert_eq!(fx.count_level(0, t0), 1.0);
        assert_eq!(fx.popup_level(t0), 1.0);
        assert!(!fx.is_active(t0));
    }

    #[test]
    fn board_entrance_runs_left_to_right_then_goes_idle() {
        let t0 = Instant::now();
        let mut fx = Effects::new(true);
        assert!(!fx.is_active(t0));
        fx.start_board(t0);
        assert_eq!(fx.board_level(0, 0, t0), 0.0);
        let mid = t0 + ms(300);
        assert!(fx.board_level(0, 0, mid) > fx.board_level(3, 0, mid));
        assert!(fx.board_level(0, 0, mid) > fx.board_level(0, 4, mid));
        let end = t0 + anim::FADE + BOARD_DELAY_MAX;
        assert!(fx.is_active(end - ms(1)));
        assert_eq!(fx.board_level(4, 9, end), 1.0);
        assert!(!fx.is_active(end));
        fx.prune(end);
        assert_eq!(fx.board_level(0, 0, end), 1.0);
    }

    #[test]
    fn flash_fades_back_and_is_pruned() {
        let t0 = Instant::now();
        let mut fx = Effects::new(true);
        fx.flash(7, t0);
        assert_eq!(fx.flash_level(7, t0), 0.0);
        assert!(fx.flash_level(7, t0 + ms(250)) > 0.0);
        assert_eq!(fx.flash_level(8, t0), 1.0); // other cards unaffected
        assert!(fx.is_active(t0 + ms(499)));
        assert!(!fx.is_active(t0 + FLASH));
        fx.prune(t0 + FLASH);
        assert!(fx.flashes.is_empty());
    }

    #[test]
    fn selection_blend_restarts_never_queues() {
        let t0 = Instant::now();
        let mut fx = Effects::new(true);
        fx.select(1, t0);
        fx.select(2, t0 + ms(30)); // fast key repeat: restart on the new card
        assert_eq!(fx.select_level(1, t0 + ms(30)), 1.0); // old card: no blend
        assert_eq!(fx.select_level(2, t0 + ms(30)), 0.0);
        fx.select(2, t0 + ms(60)); // same card: keeps its blend
        assert!(fx.select_level(2, t0 + ms(90)) > 0.0);
        assert!(!fx.is_active(t0 + ms(30) + SELECT));
    }

    #[test]
    fn counts_and_popup_animate_independently() {
        let t0 = Instant::now();
        let mut fx = Effects::new(true);
        fx.count_flash(2, t0);
        fx.start_popup(t0);
        assert_eq!(fx.count_level(2, t0), 0.0);
        assert_eq!(fx.count_level(1, t0), 1.0);
        assert_eq!(fx.popup_level(t0), 0.0);
        assert_eq!(fx.popup_level(t0 + POPUP_FADE), 1.0);
        assert!(fx.is_active(t0 + POPUP_FADE)); // count flash still running
        assert!(!fx.is_active(t0 + COUNT_FLASH));
    }
}
