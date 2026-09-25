//! Priority brief writer — port of Go `pkg/export/markdown.go:721-978`
//! (`PriorityBriefConfig`, `DefaultPriorityBriefConfig`,
//! `GeneratePriorityBriefFromTriageJSON`, `barChart`, `truncateString`,
//! `getTypeIcon`) at the frozen parity commit `18afafa`.
//!
//! The brief is a small agent-facing digest of a triage result: a summary
//! table, the top recommendations with three inline bar gauges, the quick wins
//! and the blockers worth clearing, closed by a metric legend. Go builds it by
//! unmarshalling the same `--robot-triage` JSON the CLI already produces, so
//! this module takes the marshalled bytes rather than a typed triage value and
//! picks only the fields the document actually prints.
//!
//! Section order, table headers, the `P%d` / `%.2f` field shapes, the two
//! trailing spaces on the `*Generated:*` line and the `…` ellipsis are
//! transcribed from the Go writer. `crates/bv-export/tests/priority_brief.rs`
//! pins the output byte-for-byte so a paraphrase cannot creep back in.

use serde::Deserialize;
use std::fmt::Write as _;

use crate::markdown::{wall_clock, GO_ZERO_DATETIME};

/// Go `PriorityBriefConfig` (`pkg/export/markdown.go:722-730`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityBriefConfig {
    /// Go: `MaxRecommendations` — default 5.
    pub max_recommendations: usize,
    /// Go: `MaxQuickWins` — default 3.
    pub max_quick_wins: usize,
    /// Go: `MaxBlockers` — default 3.
    pub max_blockers: usize,
    /// Go: `IncludeWhatIf` — default true. The generator never reads it (Go
    /// keeps the field for callers that post-process the document); it is
    /// carried so the struct round-trips with the Go one.
    pub include_what_if: bool,
    /// Go: `IncludeLegend` — default true. Unlike `include_what_if` this one
    /// gates real output: false drops the trailing legend section.
    pub include_legend: bool,
    /// Go: `DataHash` — optional provenance line. Empty means "print nothing".
    pub data_hash: String,
}

impl Default for PriorityBriefConfig {
    /// Go `DefaultPriorityBriefConfig` (`pkg/export/markdown.go:732-740`).
    fn default() -> Self {
        Self {
            max_recommendations: 5,
            max_quick_wins: 3,
            max_blockers: 3,
            include_what_if: true,
            include_legend: true,
            data_hash: String::new(),
        }
    }
}

/// Go's anonymous unmarshal target (`pkg/export/markdown.go:747-800`).
///
/// Go also declares `Meta.Phase2Ready` and `QuickRef.TopPicks` there; the
/// document never reads either, so they are omitted here rather than carried
/// as dead fields. `serde` ignores unknown keys, so the real triage payload
/// still parses unchanged.
#[derive(Debug, Deserialize)]
struct TriageBrief {
    #[serde(default)]
    meta: Meta,
    #[serde(default)]
    quick_ref: QuickRef,
    #[serde(default)]
    recommendations: Vec<Recommendation>,
    #[serde(default)]
    quick_wins: Vec<QuickWin>,
    #[serde(default)]
    blockers_to_clear: Vec<Blocker>,
}

#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    version: String,
    #[serde(default)]
    generated_at: String,
    #[serde(default)]
    issue_count: i64,
}

#[derive(Debug, Default, Deserialize)]
struct QuickRef {
    #[serde(default)]
    open_count: i64,
    #[serde(default)]
    actionable_count: i64,
    #[serde(default)]
    blocked_count: i64,
    #[serde(default)]
    in_progress_count: i64,
}

#[derive(Debug, Deserialize)]
struct Recommendation {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    priority: i64,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    reasons: Vec<String>,
    #[serde(default)]
    breakdown: Breakdown,
}

#[derive(Debug, Default, Deserialize)]
struct Breakdown {
    #[serde(default)]
    pagerank_norm: f64,
    #[serde(default)]
    betweenness_norm: f64,
    #[serde(default)]
    time_to_impact_norm: f64,
}

#[derive(Debug, Deserialize)]
struct QuickWin {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct Blocker {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    unblocks_count: i64,
    #[serde(default)]
    actionable: bool,
}

/// Go returns the bare unmarshal failure wrapped at
/// `pkg/export/markdown.go:802` (`failed to parse triage JSON: %w`). The Go
/// inner text comes from `encoding/json`; only the wrapper is contractual, so
/// that is what this reproduces verbatim.
#[derive(Debug, thiserror::Error)]
pub enum PriorityBriefError {
    #[error("failed to parse triage JSON: {0}")]
    Parse(#[from] serde_json::Error),
    /// Go's `encoding/json` reports an unparseable `generated_at` as
    /// `parsing time "…" as "2006-01-02T15:04:05Z07:00": cannot parse "…" as "2006"`,
    /// which is part of the same wrapped error as a syntax failure.
    #[error("failed to parse triage JSON: parsing time \"{raw}\" as \"2006-01-02T15:04:05Z07:00\": cannot parse \"{raw}\" as \"2006\"")]
    GeneratedAt { raw: String },
}

/// Go `GeneratePriorityBriefFromTriageJSON`
/// (`pkg/export/markdown.go:744-926`).
pub fn generate_priority_brief_from_triage_json(
    triage_json: &[u8],
    config: &PriorityBriefConfig,
) -> Result<String, PriorityBriefError> {
    let triage: TriageBrief = serde_json::from_slice(triage_json)?;

    // Go unmarshals `generated_at` into a `time.Time`, so a present-but-
    // unparseable value is a hard parse error rather than a blank header.
    if !triage.meta.generated_at.is_empty() && wall_clock(&triage.meta.generated_at).is_none() {
        return Err(PriorityBriefError::GeneratedAt {
            raw: triage.meta.generated_at.clone(),
        });
    }

    let mut sb = String::new();

    // Header — pkg/export/markdown.go:808-814. The two trailing spaces on the
    // `*Generated:*` line are a Markdown hard break and are load-bearing.
    let _ = write!(sb, "# 📊 Priority Brief\n\n");
    let generated = wall_clock(&triage.meta.generated_at)
        .map_or(GO_ZERO_DATETIME.to_string(), |t| {
            t.strftime("%Y-%m-%d %H:%M").to_string()
        });
    let _ = writeln!(sb, "*Generated: {generated}*  ");
    let _ = write!(
        sb,
        "*Version: {} | Issues: {}*\n\n",
        triage.meta.version, triage.meta.issue_count
    );

    // Data hash — pkg/export/markdown.go:816-818.
    if !config.data_hash.is_empty() {
        let _ = write!(sb, "**Hash:** `{}`\n\n", config.data_hash);
    }

    // Summary stats — pkg/export/markdown.go:820-829.
    sb.push_str("## 📈 Summary\n\n");
    sb.push_str("| Open | In Progress | Blocked | Actionable |\n");
    sb.push_str("|:----:|:-----------:|:-------:|:----------:|\n");
    let _ = writeln!(
        sb,
        "| {} | {} | {} | {} |",
        triage.quick_ref.open_count,
        triage.quick_ref.in_progress_count,
        triage.quick_ref.blocked_count,
        triage.quick_ref.actionable_count
    );
    sb.push('\n');

    sb.push_str("---\n\n");

    // Top recommendations — pkg/export/markdown.go:835-866.
    sb.push_str("## 🎯 Top Recommendations\n\n");
    if triage.recommendations.is_empty() {
        sb.push_str("*No recommendations available.*\n\n");
    } else {
        sb.push_str("| # | Issue | Type | P | Score | PR | BW | TI | Top Reason |\n");
        sb.push_str("|:-:|-------|:----:|:-:|:-----:|:--:|:--:|:--:|------------|\n");
        let limit = config.max_recommendations.min(triage.recommendations.len());
        for (i, rec) in triage.recommendations.iter().take(limit).enumerate() {
            let reason = rec
                .reasons
                .first()
                .map_or_else(|| "-".to_string(), |r| truncate_string(r, 30));
            let _ = writeln!(
                sb,
                "| {} | **{}** {} | {} | P{} | {:.2} | {} | {} | {} | {} |",
                i + 1,
                rec.id,
                truncate_string(&rec.title, 25),
                get_type_icon(&rec.r#type),
                rec.priority,
                rec.score,
                bar_chart(rec.breakdown.pagerank_norm),
                bar_chart(rec.breakdown.betweenness_norm),
                bar_chart(rec.breakdown.time_to_impact_norm),
                reason,
            );
        }
        sb.push('\n');
    }

    // Quick wins — pkg/export/markdown.go:868-886.
    sb.push_str("## ⚡ Quick Wins\n\n");
    if triage.quick_wins.is_empty() {
        sb.push_str("*No quick wins identified.*\n\n");
    } else {
        sb.push_str("| Issue | Reason |\n");
        sb.push_str("|-------|--------|\n");
        let limit = config.max_quick_wins.min(triage.quick_wins.len());
        for qw in triage.quick_wins.iter().take(limit) {
            let _ = writeln!(
                sb,
                "| **{}** {} | {} |",
                qw.id,
                truncate_string(&qw.title, 30),
                truncate_string(&qw.reason, 40),
            );
        }
        sb.push('\n');
    }

    // Blockers — pkg/export/markdown.go:888-907.
    sb.push_str("## 🚧 Blockers to Clear\n\n");
    if triage.blockers_to_clear.is_empty() {
        sb.push_str("*No critical blockers.*\n\n");
    } else {
        sb.push_str("| Issue | Unblocks | Ready? |\n");
        sb.push_str("|-------|:--------:|:------:|\n");
        let limit = config.max_blockers.min(triage.blockers_to_clear.len());
        for b in triage.blockers_to_clear.iter().take(limit) {
            let ready = if b.actionable { "✅" } else { "❌" };
            let _ = writeln!(
                sb,
                "| **{}** {} | {} | {} |",
                b.id,
                truncate_string(&b.title, 30),
                b.unblocks_count,
                ready,
            );
        }
        sb.push('\n');
    }

    // Legend — pkg/export/markdown.go:909-922.
    if config.include_legend {
        sb.push_str("---\n\n");
        sb.push_str("## 📖 Legend\n\n");
        sb.push_str("| Symbol | Meaning |\n");
        sb.push_str("|:------:|:--------|\n");
        sb.push_str("| **PR** | PageRank - dependency importance |\n");
        sb.push_str("| **BW** | Betweenness - critical path frequency |\n");
        sb.push_str("| **TI** | Time-to-Impact - urgency factor |\n");
        sb.push_str("| █░░░ | Low (0-25%) |\n");
        sb.push_str("| ██░░ | Medium (25-50%) |\n");
        sb.push_str("| ███░ | High (50-75%) |\n");
        sb.push_str("| ████ | Very High (75-100%) |\n");
    }

    Ok(sb)
}

/// Go `barChart` (`pkg/export/markdown.go:931-953`): a four-cell fill gauge for
/// a 0-1 value. Out-of-range values clamp before bucketing, and Go's
/// `int(value * 4)` truncates toward zero — so 0.25 lands in the second bucket,
/// not the first.
fn bar_chart(value: f64) -> &'static str {
    let value = value.clamp(0.0, 1.0);
    match (value * 4.0) as i64 {
        0 => "░░░░",
        1 => "█░░░",
        2 => "██░░",
        3 => "███░",
        _ => "████",
    }
}

/// Go `truncateString` (`pkg/export/markdown.go:955-965`): truncate to
/// `max_len` *runes* so multi-byte titles never split a character, and append a
/// single `…`. Note this is not the same helper `pkg/export/graph_snapshot.go`
/// uses (`truncate`, three dots) — they are separate functions in Go and stay
/// separate here.
fn truncate_string(s: &str, max_len: usize) -> String {
    let runes: Vec<char> = s.chars().collect();
    if runes.len() <= max_len {
        return s.to_string();
    }
    if max_len < 3 {
        return runes[..max_len].iter().collect();
    }
    let mut out: String = runes[..max_len - 1].iter().collect();
    out.push('…');
    out
}

/// Go `getTypeIcon` (`pkg/export/markdown.go:967-978`).
fn get_type_icon(issue_type: &str) -> &'static str {
    match issue_type {
        "bug" => "🐛",
        "feature" => "✨",
        "task" => "📋",
        "epic" => "🚀",
        "chore" => "🧹",
        _ => "•",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_go() {
        // pkg/export/markdown.go:732-740
        let c = PriorityBriefConfig::default();
        assert_eq!(c.max_recommendations, 5);
        assert_eq!(c.max_quick_wins, 3);
        assert_eq!(c.max_blockers, 3);
        assert!(c.include_what_if);
        assert!(c.include_legend);
        assert!(c.data_hash.is_empty());
    }

    #[test]
    fn bar_chart_buckets() {
        // pkg/export/markdown.go:931-953
        assert_eq!(bar_chart(-1.0), "░░░░");
        assert_eq!(bar_chart(0.0), "░░░░");
        assert_eq!(bar_chart(0.24), "░░░░");
        assert_eq!(bar_chart(0.25), "█░░░");
        assert_eq!(bar_chart(0.5), "██░░");
        assert_eq!(bar_chart(0.75), "███░");
        assert_eq!(bar_chart(1.0), "████");
        assert_eq!(bar_chart(2.0), "████");
    }

    #[test]
    fn truncate_string_is_rune_based() {
        // pkg/export/markdown.go:955-965
        assert_eq!(truncate_string("short", 30), "short");
        assert_eq!(truncate_string("abcdefgh", 5), "abcd…");
        assert_eq!(truncate_string("abcdefgh", 2), "ab");
        // Exactly max_len runes is not truncated.
        assert_eq!(truncate_string("abcde", 5), "abcde");
        // Multi-byte: 3 emoji are 3 runes, not 12 bytes.
        assert_eq!(truncate_string("🐛🐛🐛", 3), "🐛🐛🐛");
        assert_eq!(truncate_string("🐛🐛🐛🐛", 3), "🐛🐛…");
    }

    #[test]
    fn type_icons_match_go() {
        // pkg/export/markdown.go:967-978
        assert_eq!(get_type_icon("bug"), "🐛");
        assert_eq!(get_type_icon("feature"), "✨");
        assert_eq!(get_type_icon("task"), "📋");
        assert_eq!(get_type_icon("epic"), "🚀");
        assert_eq!(get_type_icon("chore"), "🧹");
        assert_eq!(get_type_icon("unknown-kind"), "•");
        assert_eq!(get_type_icon(""), "•");
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        // pkg/export/markdown.go:802
        let err = generate_priority_brief_from_triage_json(b"{", &PriorityBriefConfig::default())
            .unwrap_err();
        assert!(err.to_string().starts_with("failed to parse triage JSON:"));
    }

    #[test]
    fn unparseable_generated_at_is_an_error() {
        let json = br#"{"meta":{"generated_at":"not-a-time"}}"#;
        let err = generate_priority_brief_from_triage_json(json, &PriorityBriefConfig::default())
            .unwrap_err();
        assert!(err.to_string().contains("parsing time \"not-a-time\""));
    }

    #[test]
    fn absent_meta_renders_go_zero_time() {
        // pkg/export/markdown.go:809 with a zero `time.Time`.
        let out = generate_priority_brief_from_triage_json(b"{}", &PriorityBriefConfig::default())
            .unwrap();
        assert!(out.starts_with("# 📊 Priority Brief\n\n*Generated: 0001-01-01 00:00*  \n"));
    }
}
