//! Footer activity state machine for background repo loading. Time is
//! injected so every transition is deterministic (same idea as hteam-cli's
//! `set_status` + `expire_status(STATUS_TTL)`).
use std::time::{Duration, Instant};

/// How long the "done" line stays before the footer returns to the key hints.
pub const DONE_TTL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Busy,
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    Fetching {
        done: usize,
        total: usize,
        errors: usize,
    },
    Done {
        errors: usize,
        total: usize,
        at: Instant,
    },
}

impl Activity {
    /// Starts (or restarts) a round over `total` repos.
    pub fn start(total: usize) -> Self {
        if total == 0 {
            Activity::Idle
        } else {
            Activity::Fetching {
                done: 0,
                total,
                errors: 0,
            }
        }
    }

    /// Records one finished repo; switches to `Done` after the last one.
    pub fn complete(self, ok: bool, now: Instant) -> Self {
        match self {
            Activity::Fetching {
                done,
                total,
                errors,
            } => {
                let (done, errors) = (done + 1, errors + usize::from(!ok));
                if done >= total {
                    Activity::Done {
                        errors,
                        total,
                        at: now,
                    }
                } else {
                    Activity::Fetching {
                        done,
                        total,
                        errors,
                    }
                }
            }
            other => other,
        }
    }

    /// Drops an expired `Done` line.
    pub fn expire(self, now: Instant) -> Self {
        match self {
            Activity::Done { at, .. } if now.saturating_duration_since(at) >= DONE_TTL => {
                Activity::Idle
            }
            other => other,
        }
    }

    /// True while something is shown that changes over time (fast redraw tick).
    pub fn is_active(self) -> bool {
        self != Activity::Idle
    }

    /// Footer text and tone, if any.
    pub fn line(self) -> Option<(String, Tone)> {
        match self {
            Activity::Idle => None,
            Activity::Fetching { done, total, .. } => {
                Some((format!(" fetching {done}/{total} repos..."), Tone::Busy))
            }
            Activity::Done {
                errors: 0, total, ..
            } => Some((format!(" done, {total} repos loaded"), Tone::Ok)),
            Activity::Done { errors, .. } => {
                let s = if errors == 1 { "" } else { "s" };
                Some((format!(" done, {errors} error{s}"), Tone::Error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn fetching_progress_then_done_then_cleared() {
        let t0 = Instant::now();
        let mut a = Activity::start(3);
        assert_eq!(
            a.line().unwrap(),
            (" fetching 0/3 repos...".into(), Tone::Busy)
        );
        a = a.complete(true, t0);
        assert_eq!(a.line().unwrap().0, " fetching 1/3 repos...");
        a = a.complete(true, t0).complete(true, t0 + s(1));
        assert_eq!(
            a.line().unwrap(),
            (" done, 3 repos loaded".into(), Tone::Ok)
        );
        assert!(a.is_active());
        // Stays for 3s after the last card completed, then clears.
        assert_eq!(a.expire(t0 + s(1) + Duration::from_millis(2999)), a);
        let cleared = a.expire(t0 + s(1) + s(3));
        assert_eq!(cleared, Activity::Idle);
        assert!(!cleared.is_active());
        assert_eq!(cleared.line(), None);
    }

    #[test]
    fn fetching_never_expires() {
        let t0 = Instant::now();
        let a = Activity::start(2);
        assert_eq!(a.expire(t0 + s(60)), a);
    }

    #[test]
    fn errors_are_counted_and_still_clear() {
        let t0 = Instant::now();
        let a = Activity::start(3)
            .complete(false, t0)
            .complete(true, t0)
            .complete(false, t0);
        assert_eq!(a.line().unwrap(), (" done, 2 errors".into(), Tone::Error));
        assert_eq!(a.expire(t0 + s(3)), Activity::Idle);
        let one = Activity::start(1).complete(false, t0);
        assert_eq!(one.line().unwrap().0, " done, 1 error");
    }

    #[test]
    fn restart_resets_and_empty_round_is_idle() {
        let t0 = Instant::now();
        let done = Activity::start(1).complete(true, t0);
        assert!(matches!(done, Activity::Done { .. }));
        assert_eq!(
            Activity::start(4).line().unwrap().0,
            " fetching 0/4 repos..."
        );
        assert_eq!(Activity::start(0), Activity::Idle);
    }

    #[test]
    fn extra_completions_are_ignored_outside_a_round() {
        let t0 = Instant::now();
        assert_eq!(Activity::Idle.complete(true, t0), Activity::Idle);
        let done = Activity::start(1).complete(true, t0);
        assert_eq!(done.complete(false, t0 + s(1)), done);
    }
}
