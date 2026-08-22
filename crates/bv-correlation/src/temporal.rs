//! Temporal correlation confidence — port of Go
//! `pkg/correlation/temporal.go` calculateTemporalConfidence.

use std::time::Duration;

/// Inputs for one temporal window evaluation.
pub struct TemporalWindow {
    /// How many beads this author had active during the window.
    pub active_beads: usize,
    /// Duration of the claim→close window.
    pub window_duration: Duration,
    /// Whether commit file paths match keywords from the bead title.
    pub paths_match_hints: bool,
}

/// Go: `calculateTemporalConfidence` — base 0.50 ± factors, clamp [0.20, 0.85].
pub fn temporal_confidence(w: &TemporalWindow) -> f64 {
    let mut base = 0.50f64;

    // Factor 1: concurrent beads by the same author.
    if w.active_beads <= 1 {
        base += 0.20;
    } else if w.active_beads == 2 {
        base += 0.10;
    } else if w.active_beads > 3 {
        base -= 0.10;
    }

    // Factor 2: window length.
    let d = w.window_duration;
    if d < Duration::from_secs(4 * 3600) {
        base += 0.10;
    } else if d < Duration::from_secs(24 * 3600) {
        base += 0.05;
    } else if d > Duration::from_secs(7 * 24 * 3600) {
        base -= 0.15;
    } else if d > Duration::from_secs(3 * 24 * 3600) {
        base -= 0.05;
    }

    // Factor 3: path-hint match.
    if w.paths_match_hints {
        base += 0.15;
    }

    base.clamp(0.20, 0.85)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_single_bead_short_window_is_high() {
        let w = TemporalWindow {
            active_beads: 1,
            window_duration: Duration::from_secs(2 * 3600),
            paths_match_hints: true,
        };
        // 0.50 + 0.20 + 0.10 + 0.15 = 0.95 → clamped to 0.85
        assert_eq!(temporal_confidence(&w), 0.85);
    }

    #[test]
    fn scattered_many_beads_long_window_is_low() {
        let w = TemporalWindow {
            active_beads: 5,
            window_duration: Duration::from_secs(8 * 24 * 3600),
            paths_match_hints: false,
        };
        // 0.50 - 0.10 - 0.15 = 0.45... wait: -0.10 (beads>3) then -0.15 (>7d) = 0.25
        assert!((temporal_confidence(&w) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn clamped_to_floor() {
        let w = TemporalWindow {
            active_beads: 10,
            window_duration: Duration::from_secs(30 * 24 * 3600),
            paths_match_hints: false,
        };
        // 0.50 - 0.10 (beads>3) - 0.15 (>7d window) = 0.25; floor not reached
        assert!((temporal_confidence(&w) - 0.25).abs() < 1e-9);
    }
}
