//! Composite impact scoring — port of Go `pkg/analysis/priority.go`
//! ComputeImpactScoresFromStats + component functions.

use bv_core::model::{Issue, Status};
use bv_graph_core::DiGraph;
use serde::Serialize;
use std::collections::BTreeMap;

pub const URGENCY_LABELS: [&str; 5] = ["urgent", "critical", "blocker", "hotfix", "asap"];
/// Cap for critical-path-depth normalization.
pub const MAX_CRITICAL_PATH_DEPTH: f64 = 10.0;
/// Half-life for urgency decay.
pub const URGENCY_DECAY_DAYS: f64 = 7.0;

fn normalize(v: f64, max: f64) -> f64 {
    if max == 0.0 {
        0.0
    } else {
        v / max
    }
}

/// Go: `computeStaleness` — 30-day cap, unknown = 0.5.
pub fn compute_staleness(updated_at: Option<&str>, now: &jiff::Timestamp) -> f64 {
    let Some(raw) = updated_at else {
        return 0.5;
    };
    match raw.parse::<jiff::Timestamp>() {
        Ok(t) => {
            let secs = (*now - t).total(jiff::Unit::Second).unwrap_or(0.0);
            let days = secs / 86400.0;
            (days / 30.0).clamp(0.0, 1.0)
        }
        Err(_) => 0.5,
    }
}

/// Go: `computePriorityBoost` — P0=1.0 .. P4+=0.
pub fn compute_priority_boost(priority: i32) -> f64 {
    match priority {
        0 => 1.0,
        1 => 0.75,
        2 => 0.5,
        3 => 0.25,
        _ => 0.0,
    }
}

/// Go: `computeTimeToImpact` — depth .7 + time-efficiency .3.
pub fn compute_time_to_impact(
    critical_path_depth: f64,
    estimated_minutes: Option<i64>,
    median_minutes: i64,
) -> f64 {
    let effective = match estimated_minutes {
        Some(m) if m > 0 => m as f64,
        _ => median_minutes as f64,
    };
    let depth_norm = (critical_path_depth / MAX_CRITICAL_PATH_DEPTH).min(1.0);
    const MAX_MINUTES: f64 = 480.0;
    let time_factor = (1.0 - (effective / MAX_MINUTES)).clamp(0.0, 1.0);
    depth_norm * 0.7 + time_factor * 0.3
}

/// Go: `computeUrgency` — label weights + exponential decay (half-life 7d).
pub fn compute_urgency(labels: &[String], created_at: Option<&str>, now: &jiff::Timestamp) -> f64 {
    let mut score = 0.0f64;
    'outer: for label in labels {
        let lower = label.to_lowercase();
        for urgent in URGENCY_LABELS {
            if lower.contains(urgent) {
                score += match urgent {
                    "critical" | "blocker" => 1.0,
                    "urgent" | "hotfix" => 0.8,
                    "asap" => 0.6,
                    _ => 0.0,
                };
                break 'outer;
            }
        }
    }
    if let Some(raw) = created_at {
        if let Ok(created) = raw.parse::<jiff::Timestamp>() {
            let secs = (*now - created).total(jiff::Unit::Second).unwrap_or(0.0);
            let days = secs / 86400.0;
            if days > 0.0 {
                // 0.5 * (1 - e^(-days/halfLife))
                score += 0.5 * (1.0 - (-(days / URGENCY_DECAY_DAYS)).exp());
            }
        }
    }
    score.min(1.0)
}

/// Risk signals composite — port of Go `pkg/analysis/risk.go` weights.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RiskSignals {
    pub fan_variance: f64,
    pub activity_churn: f64,
    pub cross_repo_risk: f64,
    pub status_risk: f64,
    pub composite_risk: f64,
}

/// Simplified risk computation using graph-local signals.
/// Full churn/cross-repo ports land with correlation integration; the
/// weights and composition match Go DefaultRiskWeights exactly.
pub fn compute_risk_signals(issue: &Issue, g: &DiGraph, idx: usize) -> RiskSignals {
    let fan_out = g.out_degree(idx) as f64;
    let fan_in = g.in_degree(idx) as f64;
    // Fan variance proxy: imbalance between in/out degree normalized.
    let total = fan_in + fan_out;
    let fan_variance = if total > 0.0 {
        (fan_out - fan_in).abs() / total
    } else {
        0.0
    };
    // Activity churn proxy: staleness-driven (no comment history in core).
    let churn = compute_staleness(issue.updated_at.as_deref(), &jiff::Timestamp::now()) * 0.5;
    let cross_repo = if issue.source_repo.is_empty() {
        0.0
    } else {
        0.2
    };
    let status_risk = match issue.status {
        Status::Blocked => 0.8,
        Status::InProgress => 0.4,
        _ => 0.0,
    };
    let composite = fan_variance * 0.30 + churn * 0.30 + cross_repo * 0.20 + status_risk * 0.20;
    RiskSignals {
        fan_variance,
        activity_churn: churn,
        cross_repo_risk: cross_repo,
        status_risk,
        composite_risk: composite.min(1.0),
    }
}

/// Per-issue impact result matching golden `recommendations[]` breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct IssueImpact {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub status: String,
    pub priority: i32,
    pub labels: Vec<String>,
    pub score: f64,
    pub breakdown: Breakdown,
}

/// Golden field names + order.
#[derive(Debug, Clone, Serialize)]
pub struct Breakdown {
    pub pagerank: f64,
    pub betweenness: f64,
    pub blocker_ratio: f64,
    pub staleness: f64,
    pub priority_boost: f64,
    pub time_to_impact: f64,
    pub urgency: f64,
    pub risk: f64,
}

/// Inputs gathered from Phase-1/2 stats for one scoring pass.
pub struct ImpactInputs<'a> {
    pub issues: &'a [Issue],
    pub pagerank: &'a BTreeMap<String, f64>,
    pub betweenness: &'a BTreeMap<String, f64>,
    /// critical path heights keyed by id.
    pub critical_path: &'a BTreeMap<String, f64>,
    pub g: &'a DiGraph,
    pub now: jiff::Timestamp,
}

/// Score all open issues, ranked by score desc then ID asc (Go tie-break).
pub fn compute_impact_scores(inputs: &ImpactInputs) -> Vec<IssueImpact> {
    // Max values for normalization
    let max_pr = inputs.pagerank.values().copied().fold(0.0, f64::max);
    let max_bw = inputs.betweenness.values().copied().fold(0.0, f64::max);
    // Blocker counts = in-degree over blocking edges; max across nodes.
    let max_blockers = (0..inputs.g.len())
        .map(|i| inputs.g.in_degree(i))
        .max()
        .unwrap_or(0);

    let mut results: Vec<IssueImpact> = Vec::new();
    for issue in inputs.issues {
        // Skip closed/tombstone (Go parity).
        if matches!(issue.status, Status::Closed | Status::Tombstone) {
            continue;
        }
        let pr_norm = normalize(
            inputs.pagerank.get(&issue.id).copied().unwrap_or(0.0),
            max_pr,
        );
        let bw_norm = normalize(
            inputs.betweenness.get(&issue.id).copied().unwrap_or(0.0),
            max_bw,
        );
        let idx = inputs.g.node_idx(&issue.id).unwrap_or(usize::MAX);
        let blockers = if idx == usize::MAX {
            0
        } else {
            inputs.g.in_degree(idx)
        };
        let blocker_norm = normalize(blockers as f64, max_blockers as f64);
        let staleness_norm = compute_staleness(issue.updated_at.as_deref(), &inputs.now);
        let prio_norm = compute_priority_boost(issue.priority);
        let depth = inputs.critical_path.get(&issue.id).copied().unwrap_or(0.0);
        let tti_norm = compute_time_to_impact(depth, issue.estimated_minutes, 60);
        let urgency_norm = compute_urgency(&issue.labels, issue.created_at.as_deref(), &inputs.now);
        let risk = compute_risk_signals(issue, inputs.g, idx);

        let b = Breakdown {
            pagerank: pr_norm * super::scoring::WEIGHT_PAGE_RANK,
            betweenness: bw_norm * super::scoring::WEIGHT_BETWEENNESS,
            blocker_ratio: blocker_norm * super::scoring::WEIGHT_BLOCKER_RATIO,
            staleness: staleness_norm * super::scoring::WEIGHT_STALENESS,
            priority_boost: prio_norm * super::scoring::WEIGHT_PRIORITY_BOOST,
            time_to_impact: tti_norm * super::scoring::WEIGHT_TIME_TO_IMPACT,
            urgency: urgency_norm * super::scoring::WEIGHT_URGENCY,
            risk: risk.composite_risk * super::scoring::WEIGHT_RISK,
        };
        let score = b.pagerank
            + b.betweenness
            + b.blocker_ratio
            + b.staleness
            + b.priority_boost
            + b.time_to_impact
            + b.urgency
            + b.risk;

        results.push(IssueImpact {
            id: issue.id.clone(),
            title: issue.title.clone(),
            issue_type: issue.issue_type.clone(),
            status: issue.status.as_str().to_string(),
            priority: issue.priority,
            labels: issue.labels.clone(),
            score,
            breakdown: b,
        });
    }
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_caps_at_thirty_days() {
        let now = jiff::Timestamp::now();
        let old = (now - jiff::Span::new().hours(60 * 24)).to_string();
        assert_eq!(compute_staleness(Some(&old), &now), 1.0);
        let fresh = (now - jiff::Span::new().hours(12)).to_string();
        let s = compute_staleness(Some(&fresh), &now);
        assert!((s - 0.5 / 30.0).abs() < 0.01);
        assert_eq!(compute_staleness(None, &now), 0.5);
    }

    #[test]
    fn priority_boost_matches_go_table() {
        assert_eq!(compute_priority_boost(0), 1.0);
        assert_eq!(compute_priority_boost(1), 0.75);
        assert_eq!(compute_priority_boost(2), 0.5);
        assert_eq!(compute_priority_boost(3), 0.25);
        assert_eq!(compute_priority_boost(4), 0.0);
    }

    #[test]
    fn urgency_label_weights_and_decay_cap() {
        let now = jiff::Timestamp::now();
        let crit = vec!["critical".to_string()];
        let fresh_created = (now - jiff::Span::new().hours(1)).to_string();
        let u = compute_urgency(&crit, Some(&fresh_created), &now);
        assert!(u >= 1.0); // label alone hits cap
        let no_labels: Vec<String> = vec![];
        let ancient = (now - jiff::Span::new().hours(365 * 24)).to_string();
        let u2 = compute_urgency(&no_labels, Some(&ancient), &now);
        // decay approaches but never exceeds 0.5
        assert!(u2 < 0.51 && u2 > 0.45);
    }

    #[test]
    fn time_to_impact_depth_weighted_over_time() {
        // deep chain + no estimate(median 60m): depth 10 -> 1.0*0.7 + (1-60/480)*0.3 ≈ 0.86
        let s = compute_time_to_impact(10.0, None, 60);
        assert!((s - (1.0 * 0.7 + (1.0 - 60.0 / 480.0) * 0.3)).abs() < 1e-9);
    }
}
