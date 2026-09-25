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
/// File-overlap weight is `min(5 * files, 15)` (Go scorer.go:415).
pub const SIGNAL_WEIGHT_FILE_OVERLAP_PER_FILE: i32 = 5;
pub const SIGNAL_WEIGHT_FILE_OVERLAP_CAP: i32 = 15;
/// Proximity (Go scorer.go:424).
pub const SIGNAL_WEIGHT_PROXIMITY: i32 = 7;
/// A proximity signal is emitted only above this fraction of the primary
/// method's maximum confidence (Go scorer.go:422).
pub const PROXIMITY_CONFIDENCE_FRACTION: f64 = 0.9;

/// Go `CorrelationSignal` — one factor contributing to a correlation's
/// confidence. Field order matches Go's declaration (types.go:244-248).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CorrelationSignal {
    /// `message_match` | `timing` | `file_overlap` | `author_match` |
    /// `proximity` | `co_commit`. Go's tag is `json:"type"`; the field is
    /// renamed here so the name is not a Rust keyword.
    #[serde(rename = "type")]
    pub signal_type: String,
    /// Contribution to overall confidence (0-100).
    pub weight: i32,
    /// Human/agent readable explanation.
    pub detail: String,
}

/// Go `CorrelationExplanation` — the bare body of
/// `--robot-explain-correlation <sha>:<bead>`. Go encodes this struct on its
/// own, with no robot envelope around it (robot_registry.go:2871-2887), so
/// every field here is serialized and the field order is Go's declaration
/// order (types.go:263-278).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CorrelationExplanation {
    pub commit_sha: String,
    pub bead_id: String,
    /// 0.0 to 1.0.
    pub confidence: f64,
    /// 0 to 100, for display.
    pub confidence_pct: i64,
    /// "very high" | "high" | "moderate" | "low" | "very low".
    pub level: String,
    /// Primary correlation method.
    pub method: String,
    /// All contributing signals.
    pub signals: Vec<CorrelationSignal>,
    /// Sum of signal weights.
    pub total_weight: i32,
    /// One-line summary.
    pub summary: String,
    /// Suggested action.
    pub recommendation: String,
    /// The stored confirm/reject/ignore decision for this pair, when one
    /// exists. The explanation always describes the raw strategy score so a
    /// rejected pair can still be explained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<crate::feedback::CorrelationFeedback>,
}

/// Go `ConfidenceLevel` — the human-readable bucket string.
///
/// The existing [`ConfidenceLevel`] enum has no `VeryLow` arm and folds it into
/// `Low`, so it cannot render Go's five buckets; this returns the string the
/// oracle emits.
pub fn confidence_level_str(confidence: f64) -> &'static str {
    if confidence >= 0.9 {
        "very high"
    } else if confidence >= 0.75 {
        "high"
    } else if confidence >= 0.5 {
        "moderate"
    } else if confidence >= 0.3 {
        "low"
    } else {
        "very low"
    }
}

/// Go `Scorer.ExtractSignals` — every method that matched the commit
/// (falling back to the primary `method`) contributes its signal, then the
/// file and proximity signals are appended.
pub fn extract_signals(commit: &crate::history::HistoryCommit) -> Vec<CorrelationSignal> {
    let mut signals: Vec<CorrelationSignal> = Vec::new();

    for method in commit.all_methods() {
        match method {
            "co_committed" => signals.push(CorrelationSignal {
                signal_type: "co_commit".to_string(),
                weight: SIGNAL_WEIGHT_CO_COMMIT,
                detail: "Commit modified both code and beads file together (direct causation)"
                    .to_string(),
            }),
            "explicit_id" => signals.push(CorrelationSignal {
                signal_type: "message_match".to_string(),
                weight: SIGNAL_WEIGHT_MESSAGE_MATCH,
                detail: "Commit message contains bead ID reference".to_string(),
            }),
            "temporal_author" => {
                signals.push(CorrelationSignal {
                    signal_type: "timing".to_string(),
                    weight: SIGNAL_WEIGHT_TIMING,
                    detail: "Commit within bead's active time window".to_string(),
                });
                signals.push(CorrelationSignal {
                    signal_type: "author_match".to_string(),
                    weight: SIGNAL_WEIGHT_AUTHOR_MATCH,
                    detail: format!("By assignee: {}", commit.author),
                });
            }
            _ => {}
        }
    }

    if !commit.files.is_empty() {
        let file_weight = (commit.files.len() as i32 * SIGNAL_WEIGHT_FILE_OVERLAP_PER_FILE)
            .min(SIGNAL_WEIGHT_FILE_OVERLAP_CAP);
        signals.push(CorrelationSignal {
            signal_type: "file_overlap".to_string(),
            weight: file_weight,
            detail: format!("{} file(s) in commit scope", commit.files.len()),
        });
    }

    // If confidence is higher than the base method suggests, there may be a
    // proximity signal. Go only consults the table for a method it knows.
    if let Some((_, max)) = method_range(commit.method) {
        if commit.confidence > max * PROXIMITY_CONFIDENCE_FRACTION {
            signals.push(CorrelationSignal {
                signal_type: "proximity".to_string(),
                weight: SIGNAL_WEIGHT_PROXIMITY,
                detail: "Adjacent to other confirmed linked commits".to_string(),
            });
        }
    }

    signals
}

/// Go `MethodRanges` lookup: `(min, max)` for a known method, `None` for one
/// the table does not carry.
fn method_range(method: &str) -> Option<(f64, f64)> {
    match method {
        "co_committed" => Some((0.85, 0.99)),
        "explicit_id" => Some((0.70, 0.99)),
        "temporal_author" => Some((0.20, 0.85)),
        _ => None,
    }
}

/// Go `Scorer.buildSummary` — one line naming every method that matched.
fn build_summary(commit: &crate::history::HistoryCommit, signal_count: usize) -> String {
    let mut descs: Vec<&str> = Vec::new();
    for method in commit.all_methods() {
        match method {
            "co_committed" => descs.push("Co-committed with bead update"),
            "explicit_id" => descs.push("Explicitly references bead ID"),
            "temporal_author" => descs.push("Temporal+author correlation"),
            _ => {}
        }
    }
    let mut method_desc = descs.join(" + ");
    if commit.confirmed {
        method_desc.push_str(", confirmed by feedback");
    }
    format!(
        "{} ({:.0}% confidence, {} signals)",
        method_desc,
        commit.confidence * 100.0,
        signal_count
    )
}

/// Go `Scorer.buildRecommendation`. The second argument is unused in Go too.
fn build_recommendation(confidence: f64) -> &'static str {
    if confidence >= 0.85 {
        "High confidence - likely correct, no action needed"
    } else if confidence >= 0.65 {
        "Moderate confidence - review if accuracy matters"
    } else if confidence >= 0.45 {
        "Low confidence - manual verification recommended"
    } else {
        "Very low confidence - consider rejecting if incorrect"
    }
}

/// Go `Scorer.BuildExplanation` — the full breakdown for one (commit, bead)
/// correlation.
pub fn build_explanation(
    commit: &crate::history::HistoryCommit,
    bead_id: &str,
) -> CorrelationExplanation {
    let signals = extract_signals(commit);
    let total_weight = signals.iter().map(|s| s.weight).sum();
    CorrelationExplanation {
        commit_sha: commit.sha.clone(),
        bead_id: bead_id.to_string(),
        confidence: commit.confidence,
        confidence_pct: (commit.confidence * 100.0) as i64,
        level: confidence_level_str(commit.confidence).to_string(),
        method: commit.method.to_string(),
        summary: build_summary(commit, signals.len()),
        recommendation: build_recommendation(commit.confidence).to_string(),
        signals,
        total_weight,
        feedback: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryCommit;

    #[test]
    fn ranges_match_go() {
        assert_eq!(Method::CoCommitted.range(), (0.85, 0.99));
        assert_eq!(Method::ExplicitId.range(), (0.70, 0.99));
        assert_eq!(Method::TemporalAuthor.range(), (0.20, 0.85));
    }

    /// Minimal `HistoryCommit` for the explanation tests.
    fn commit(method: &'static str, methods: &[&str], conf: f64, files: usize) -> HistoryCommit {
        HistoryCommit {
            bead_id: "B-1".to_string(),
            sha: "9c721684712f572b9e9988eb7b0b3dd897d971b2".to_string(),
            short_sha: "9c72168".to_string(),
            message: "chore: bump".to_string(),
            author: "Ada".to_string(),
            author_email: "ada@example.com".to_string(),
            timestamp: "2026-08-23T03:50:38+07:00".to_string(),
            files: (0..files)
                .map(|i| crate::history::FileChange {
                    path: format!("src/f{i}.rs"),
                    action: "M".to_string(),
                    insertions: 1,
                    deletions: 1,
                })
                .collect(),
            method,
            methods: methods.iter().map(|s| s.to_string()).collect(),
            confidence: conf,
            reason: "matched".to_string(),
            confirmed: false,
        }
    }

    /// The exact pair from `beads_viewer_rust-api-freeze-b73` at 9c72168, whose
    /// explanation is frozen in the report this port was written against.
    #[test]
    fn build_explanation_reproduces_the_oracle_output() {
        let e = build_explanation(
            &commit("co_committed", &["co_committed"], 0.99, 6),
            "beads_viewer_rust-api-freeze-b73",
        );
        assert_eq!(e.commit_sha, "9c721684712f572b9e9988eb7b0b3dd897d971b2");
        assert_eq!(e.bead_id, "beads_viewer_rust-api-freeze-b73");
        assert_eq!(e.confidence, 0.99);
        assert_eq!(e.confidence_pct, 99);
        assert_eq!(e.level, "very high");
        assert_eq!(e.method, "co_committed");
        assert_eq!(e.total_weight, 72);
        assert_eq!(
            e.summary,
            "Co-committed with bead update (99% confidence, 3 signals)"
        );
        assert_eq!(
            e.recommendation,
            "High confidence - likely correct, no action needed"
        );
        let types: Vec<&str> = e.signals.iter().map(|s| s.signal_type.as_str()).collect();
        assert_eq!(types, ["co_commit", "file_overlap", "proximity"]);
        // File overlap is min(5 * files, 15): six files still cap at 15.
        assert_eq!(e.signals[1].weight, 15);
        assert_eq!(e.signals[1].detail, "6 file(s) in commit scope");
        assert_eq!(e.signals[2].weight, 7);
        assert!(
            e.feedback.is_none(),
            "the store supplies feedback, not the scorer"
        );
    }

    /// Go's `AllMethods` falls back to the primary `method` when `Methods` is
    /// empty (types.go:123-131) — without that fallback a single-method commit
    /// would lose its only signal.
    ///
    /// explicit_id tops out at 0.99, so the proximity gate `confidence > 0.891`
    /// (scorer.go:422) does not fire at 0.80.
    #[test]
    fn signals_fall_back_to_the_primary_method() {
        let e = build_explanation(&commit("explicit_id", &[], 0.80, 0), "B-1");
        let types: Vec<&str> = e.signals.iter().map(|s| s.signal_type.as_str()).collect();
        assert_eq!(types, ["message_match"]);
        assert_eq!(e.total_weight, 40);
        // Go's format string is a literal "%d signals" — no singular case.
        assert_eq!(
            e.summary,
            "Explicitly references bead ID (80% confidence, 1 signals)"
        );
    }

    /// Every matched method contributes its own signal (scorer.go:373-410), and
    /// `temporal_author` contributes two: timing and author_match.
    ///
    /// All three methods cap at 0.99, so 0.5 is well under the 0.891 proximity
    /// gate and no proximity signal is appended.
    #[test]
    fn every_matched_method_contributes_its_signal() {
        let e = build_explanation(
            &commit(
                "co_committed",
                &["co_committed", "explicit_id", "temporal_author"],
                0.5,
                0,
            ),
            "B-1",
        );
        let types: Vec<&str> = e.signals.iter().map(|s| s.signal_type.as_str()).collect();
        assert_eq!(
            types,
            ["co_commit", "message_match", "timing", "author_match"]
        );
        assert_eq!(e.signals[3].detail, "By assignee: Ada");
        // 50 + 40 + 25 + 15
        assert_eq!(e.total_weight, 130);
        assert_eq!(
            e.summary,
            "Co-committed with bead update + Explicitly references bead ID + \
             Temporal+author correlation (50% confidence, 4 signals)"
        );
    }

    /// The proximity gate is per-primary-method: `temporal_author` tops out at
    /// 0.85, so its threshold is 0.765 and a 0.80 commit clears it even though
    /// the same confidence would not clear explicit_id's 0.891.
    #[test]
    fn proximity_gate_uses_the_primary_methods_own_maximum() {
        let e = build_explanation(
            &commit("temporal_author", &["temporal_author"], 0.80, 0),
            "B-1",
        );
        let types: Vec<&str> = e.signals.iter().map(|s| s.signal_type.as_str()).collect();
        assert_eq!(types, ["timing", "author_match", "proximity"]);
        assert_eq!(e.total_weight, 47);
    }

    /// Go only consults `MethodRanges` for a method the table knows, so an
    /// unknown primary method gets no proximity signal (scorer.go:421-428).
    #[test]
    fn unknown_method_gets_no_proximity_signal() {
        let e = build_explanation(
            &commit("brand_new_strategy", &["brand_new_strategy"], 0.99, 0),
            "B-1",
        );
        assert!(e.signals.is_empty(), "{:?}", e.signals);
        assert_eq!(e.total_weight, 0);
        assert_eq!(e.level, "very high");
    }

    /// Go's five confidence buckets (scorer.go:330-344).
    #[test]
    fn confidence_levels_cover_go_buckets() {
        assert_eq!(confidence_level_str(0.95), "very high");
        assert_eq!(confidence_level_str(0.90), "very high");
        assert_eq!(confidence_level_str(0.89), "high");
        assert_eq!(confidence_level_str(0.75), "high");
        assert_eq!(confidence_level_str(0.74), "moderate");
        assert_eq!(confidence_level_str(0.50), "moderate");
        assert_eq!(confidence_level_str(0.49), "low");
        assert_eq!(confidence_level_str(0.30), "low");
        assert_eq!(confidence_level_str(0.29), "very low");
        assert_eq!(confidence_level_str(0.0), "very low");
    }

    /// Go's recommendation thresholds (scorer.go:461-472).
    #[test]
    fn recommendations_match_go_thresholds() {
        assert_eq!(
            build_recommendation(0.85),
            "High confidence - likely correct, no action needed"
        );
        assert_eq!(
            build_recommendation(0.84),
            "Moderate confidence - review if accuracy matters"
        );
        assert_eq!(
            build_recommendation(0.64),
            "Low confidence - manual verification recommended"
        );
        assert_eq!(
            build_recommendation(0.44),
            "Very low confidence - consider rejecting if incorrect"
        );
    }

    /// A confirmed commit gets the ", confirmed by feedback" suffix in the
    /// summary (scorer.go:450-452).
    #[test]
    fn confirmed_commit_says_so_in_the_summary() {
        let mut c = commit("co_committed", &["co_committed"], 0.99, 0);
        c.confirmed = true;
        let e = build_explanation(&c, "B-1");
        assert_eq!(
            e.summary,
            "Co-committed with bead update, confirmed by feedback (99% confidence, 2 signals)"
        );
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
