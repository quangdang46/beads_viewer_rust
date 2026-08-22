//! Confidence scoring for bead↔commit correlations — port of Go
//! `pkg/correlation/scorer.go` (MethodRanges, CombineConfidence, levels).

use serde::Serialize;

/// Correlation method (Go `CorrelationMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    CoCommitted,
    ExplicitId,
    TemporalAuthor,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::CoCommitted => "co_committed",
            Method::ExplicitId => "explicit_id",
            Method::TemporalAuthor => "temporal_author",
        }
    }

    /// Go `MethodRanges`: valid confidence interval per method.
    pub fn range(self) -> (f64, f64) {
        match self {
            Method::CoCommitted => (0.85, 0.99),
            Method::ExplicitId => (0.70, 0.99),
            Method::TemporalAuthor => (0.20, 0.85),
        }
    }
}

/// Go: confidence level buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    VeryHigh,
    High,
    Moderate,
    Low,
}

impl ConfidenceLevel {
    pub fn from_confidence(c: f64) -> Self {
        if c >= 0.90 {
            ConfidenceLevel::VeryHigh
        } else if c >= 0.75 {
            ConfidenceLevel::High
        } else if c >= 0.50 {
            ConfidenceLevel::Moderate
        } else {
            ConfidenceLevel::Low
        }
    }
}

/// Clamp a raw confidence into the method's valid range.
pub fn clamp_to_method(method: Method, confidence: f64) -> f64 {
    let (min, max) = method.range();
    confidence.clamp(min, max)
}

/// Go: `CombineConfidence` — highest-confidence signal is the base; each
/// additional independent signal adds headroom × 0.1 × its score, capped 0.99.
pub fn combine_confidence(signals: &[(Method, f64)]) -> f64 {
    if signals.is_empty() {
        return 0.0;
    }
    // Base = max by confidence.
    let mut best_idx = 0;
    for (i, &(_, c)) in signals.iter().enumerate() {
        if c > signals[best_idx].1 {
            best_idx = i;
        }
    }
    let base = clamp_to_method(signals[best_idx].0, signals[best_idx].1);
    let (_base_min, base_max) = signals[best_idx].0.range();
    let headroom = base_max - base;

    let mut combined = base;
    for (i, &(method, score)) in signals.iter().enumerate() {
        if i == best_idx {
            continue;
        }
        let clamped = clamp_to_method(method, score);
        // normalized position within the extra method's own range
        let (min, max) = method.range();
        let span = max - min;
        let pos = if span > 0.0 {
            (clamped - min) / span
        } else {
            0.0
        };
        combined += headroom * 0.1 * pos;
    }
    combined.min(0.99)
}

/// Signal weights for multi-signal correlation ranking
/// (Go scorer.go: co_commit=50 message=40 timing=25 author=15 file≤15 proximity=7).
pub const SIGNAL_WEIGHT_CO_COMMIT: i32 = 50;
pub const SIGNAL_WEIGHT_MESSAGE_MATCH: i32 = 40;
pub const SIGNAL_WEIGHT_TIMING: i32 = 25;
pub const SIGNAL_WEIGHT_AUTHOR_MATCH: i32 = 15;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_match_go() {
        assert_eq!(Method::CoCommitted.range(), (0.85, 0.99));
        assert_eq!(Method::ExplicitId.range(), (0.70, 0.99));
        assert_eq!(Method::TemporalAuthor.range(), (0.20, 0.85));
    }

    #[test]
    fn combine_single_signal_is_clamped_base() {
        let s = vec![(Method::ExplicitId, 0.95)];
        let c = combine_confidence(&s);
        assert!((c - 0.95).abs() < 1e-9);
        // out-of-range input gets clamped into [0.70, 0.99]
        let s2 = vec![(Method::ExplicitId, 0.50)];
        assert!((combine_confidence(&s2) - 0.70).abs() < 1e-9);
    }

    #[test]
    fn combining_adds_headroom_not_exceeds_cap() {
        let strong = vec![
            (Method::CoCommitted, 0.95),
            (Method::ExplicitId, 0.90),
            (Method::TemporalAuthor, 0.60),
        ];
        let c = combine_confidence(&strong);
        assert!(c > 0.95, "multi-signal boosts above base");
        assert!(c <= 0.99);
    }

    #[test]
    fn empty_signals_zero() {
        assert_eq!(combine_confidence(&[]), 0.0);
    }

    #[test]
    fn levels_bucket_correctly() {
        assert_eq!(
            ConfidenceLevel::from_confidence(0.95),
            ConfidenceLevel::VeryHigh
        );
        assert_eq!(
            ConfidenceLevel::from_confidence(0.80),
            ConfidenceLevel::High
        );
        assert_eq!(
            ConfidenceLevel::from_confidence(0.55),
            ConfidenceLevel::Moderate
        );
        assert_eq!(ConfidenceLevel::from_confidence(0.25), ConfidenceLevel::Low);
    }
}
