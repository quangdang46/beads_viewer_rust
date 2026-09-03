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

/// Go: `Scorer.CombineConfidence` — sorts raw confidences descending, uses
/// the highest as the base, then folds in each remaining signal with a
/// diminishing boost (`headroom * 0.1 * score`, headroom recomputed against
/// the *running* base after each fold), capped at 0.99. No per-method
/// clamping happens inside the combiner — `ValidateConfidence`/
/// `clamp_to_method` is a separate, single-signal check in Go.
pub fn combine_confidence(signals: &[(Method, f64)]) -> f64 {
    if signals.is_empty() {
        return 0.0;
    }
    if signals.len() == 1 {
        return signals[0].1;
    }
    let mut scores: Vec<f64> = signals.iter().map(|&(_, c)| c).collect();
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut base = scores[0];
    for &score in &scores[1..] {
        let headroom = 1.0 - base;
        base += headroom * 0.1 * score;
    }
    base.min(0.99)
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
    fn combine_single_signal_passes_through_unclamped() {
        // Go: `len(signals) == 1` returns the raw confidence, no clamping —
        // clamping is a separate concern (`ValidateConfidence`).
        let s = vec![(Method::ExplicitId, 0.95)];
        let c = combine_confidence(&s);
        assert!((c - 0.95).abs() < 1e-9);
        let s2 = vec![(Method::ExplicitId, 0.50)];
        assert!((combine_confidence(&s2) - 0.50).abs() < 1e-9);
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
