//! Temporal causality analysis for one bead — port of Go
//! `pkg/correlation/causality.go` (`BuildCausalityChainAt` + insights),
//! backing `robot-causality`.
//!
//! Documented scope cut (see plan doc §11): Go's chain also includes
//! explicit blocked/unblocked events (derived from status-field snapshots
//! in the beads JSONL diff) and computes blocked-duration/critical-path
//! insights from them. `BeadEvent` (this crate's `extractor.rs`) only
//! distinguishes created/claimed/closed/reopened/modified — it doesn't
//! carry the specific "became blocked" transition — so blocked-period
//! detection, `blocked_percentage`, and `critical_path` are not computed
//! here. Everything else (chronological event chain, commit events,
//! gap/duration analysis, summary, recommendations) is real.

use crate::extractor::{BeadEvent, EventType};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CausalEvent {
    pub id: usize,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub timestamp: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Seconds until the next event, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_next_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CausalChain {
    pub bead_id: String,
    pub events: Vec<CausalEvent>,
    pub edge_count: usize,
    pub start_time: String,
    pub end_time: String,
    pub total_duration_secs: f64,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CausalInsights {
    pub commit_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_time_between_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_gap_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_gap_desc: Option<String>,
    pub summary: String,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CausalityResult {
    pub chain: CausalChain,
    pub insights: CausalInsights,
}

fn event_label(t: EventType) -> &'static str {
    match t {
        EventType::Created => "created",
        EventType::Claimed => "claimed",
        EventType::Closed => "closed",
        EventType::Reopened => "reopened",
        EventType::Modified => "modified",
    }
}

/// Build the causal chain + insights for one bead from its lifecycle
/// events (already filtered/sorted by caller is not required — this sorts
/// internally). Returns `None` if there are no events for this bead.
pub fn build_causality_chain(bead_id: &str, events: &[BeadEvent]) -> Option<CausalityResult> {
    let mut mine: Vec<&BeadEvent> = events.iter().filter(|e| e.bead_id == bead_id).collect();
    if mine.is_empty() {
        return None;
    }
    mine.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    let mut causal_events: Vec<CausalEvent> = Vec::with_capacity(mine.len());
    let mut gaps: Vec<f64> = Vec::new();
    let mut longest_gap: Option<(f64, usize, usize)> = None;

    for (i, e) in mine.iter().enumerate() {
        causal_events.push(CausalEvent {
            id: i,
            event_type: event_label(e.event_type),
            timestamp: e.timestamp.clone(),
            description: format!("{} → {}", bead_id, event_label(e.event_type)),
            commit_sha: Some(e.commit_sha.clone()),
            duration_next_secs: None,
        });
    }

    // Fill duration_next / gap tracking with real parsed timestamps.
    for i in 0..causal_events.len().saturating_sub(1) {
        let (Ok(t1), Ok(t2)) = (
            mine[i].timestamp.parse::<jiff::Timestamp>(),
            mine[i + 1].timestamp.parse::<jiff::Timestamp>(),
        ) else {
            continue;
        };
        let secs = (t2 - t1).total(jiff::Unit::Second).unwrap_or(0.0).max(0.0);
        causal_events[i].duration_next_secs = Some(secs);
        gaps.push(secs);
        if longest_gap.map(|(g, ..)| secs > g).unwrap_or(true) {
            longest_gap = Some((secs, i, i + 1));
        }
    }

    let start_time = mine[0].timestamp.clone();
    let end_time = mine[mine.len() - 1].timestamp.clone();
    let total_duration_secs = mine[0]
        .timestamp
        .parse::<jiff::Timestamp>()
        .ok()
        .zip(mine[mine.len() - 1].timestamp.parse::<jiff::Timestamp>().ok())
        .map(|(a, b)| (b - a).total(jiff::Unit::Second).unwrap_or(0.0).max(0.0))
        .unwrap_or(0.0);
    let is_complete = mine.iter().any(|e| e.event_type == EventType::Closed);
    let commit_count = mine.len();
    let edge_count = causal_events.len().saturating_sub(1);

    let avg_time_between_secs = if !gaps.is_empty() {
        Some(gaps.iter().sum::<f64>() / gaps.len() as f64)
    } else {
        None
    };
    let (longest_gap_secs, longest_gap_desc) = match longest_gap {
        Some((secs, i, j)) => (
            Some(secs),
            Some(format!(
                "{:.1}h between {} and {}",
                secs / 3600.0,
                event_label(mine[i].event_type),
                event_label(mine[j].event_type)
            )),
        ),
        None => (None, None),
    };

    let summary = format!(
        "{bead_id}: {} events over {:.1}h ({})",
        commit_count,
        total_duration_secs / 3600.0,
        if is_complete { "complete" } else { "still open" }
    );

    let mut recommendations = Vec::new();
    if let Some(secs) = longest_gap_secs {
        if secs > 7.0 * 86400.0 {
            recommendations.push(format!(
                "Longest gap was {:.1} days — consider breaking this bead into smaller pieces next time.",
                secs / 86400.0
            ));
        }
    }
    if !is_complete {
        recommendations.push("Bead is still open — no full-cycle insight available yet.".to_string());
    }

    Some(CausalityResult {
        chain: CausalChain {
            bead_id: bead_id.to_string(),
            events: causal_events,
            edge_count,
            start_time,
            end_time,
            total_duration_secs,
            is_complete,
        },
        insights: CausalInsights {
            commit_count,
            avg_time_between_secs,
            longest_gap_secs,
            longest_gap_desc,
            summary,
            recommendations,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(bead_id: &str, ty: EventType, ts: &str, sha: &str) -> BeadEvent {
        BeadEvent {
            bead_id: bead_id.to_string(),
            event_type: ty,
            timestamp: ts.to_string(),
            commit_sha: sha.to_string(),
            commit_msg: String::new(),
            author: "alice".to_string(),
            author_email: String::new(),
        }
    }

    #[test]
    fn no_events_returns_none() {
        assert!(build_causality_chain("MISSING", &[]).is_none());
    }

    #[test]
    fn simple_chain_computes_duration_and_completeness() {
        let events = vec![
            event("A-1", EventType::Created, "2026-01-01T00:00:00Z", "sha1"),
            event("A-1", EventType::Claimed, "2026-01-01T02:00:00Z", "sha2"),
            event("A-1", EventType::Closed, "2026-01-01T06:00:00Z", "sha3"),
        ];
        let result = build_causality_chain("A-1", &events).unwrap();
        assert!(result.chain.is_complete);
        assert_eq!(result.chain.events.len(), 3);
        assert_eq!(result.chain.edge_count, 2);
        assert!((result.chain.total_duration_secs - 6.0 * 3600.0).abs() < 1.0);
        assert_eq!(result.insights.commit_count, 3);
    }

    #[test]
    fn open_bead_is_not_complete_and_flags_it() {
        let events = vec![event("A-2", EventType::Created, "2026-01-01T00:00:00Z", "sha1")];
        let result = build_causality_chain("A-2", &events).unwrap();
        assert!(!result.chain.is_complete);
        assert!(result.insights.recommendations.iter().any(|r| r.contains("still open")));
    }

    #[test]
    fn events_for_other_beads_are_excluded() {
        let events = vec![
            event("A-3", EventType::Created, "2026-01-01T00:00:00Z", "sha1"),
            event("A-4", EventType::Created, "2026-01-01T00:00:00Z", "sha2"),
        ];
        let result = build_causality_chain("A-3", &events).unwrap();
        assert_eq!(result.chain.events.len(), 1);
    }

    #[test]
    fn longest_gap_is_identified() {
        let events = vec![
            event("A-5", EventType::Created, "2026-01-01T00:00:00Z", "sha1"),
            event("A-5", EventType::Claimed, "2026-01-01T00:10:00Z", "sha2"),
            event("A-5", EventType::Closed, "2026-01-10T00:00:00Z", "sha3"),
        ];
        let result = build_causality_chain("A-5", &events).unwrap();
        assert!(result.insights.longest_gap_secs.unwrap() > 8.0 * 86400.0);
    }
}
