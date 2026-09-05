//! Triage payload assembly — port of Go triage output shapes:
//! quick_ref (#165 strict semantics), project_health, commands.

use crate::impact::{compute_impact_scores, ImpactInputs};
use bv_core::model::{Issue, Status};
use bv_graph_core::DiGraph;
use serde::Serialize;
use std::collections::BTreeMap;

/// Strict count semantics from #165.
#[derive(Debug, Default, Serialize)]
pub struct QuickRef {
    /// status exactly "open"
    pub open_count: usize,
    /// non-closed with zero open blocking dependencies
    pub actionable_count: usize,
    /// status exactly "blocked"
    pub blocked_count: usize,
    pub in_progress_count: usize,
    /// every non-closed issue
    pub not_closed_count: usize,
    /// non-closed blocked by open dependencies
    pub not_actionable_count: usize,
}

impl QuickRef {
    /// Partition invariant: not_closed == actionable + not_actionable.
    pub fn validate_invariant(&self) -> bool {
        self.not_closed_count == self.actionable_count + self.not_actionable_count
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ProjectCounts {
    pub total: usize,
    pub open: usize,
    pub closed: usize,
    pub blocked: usize,
    pub actionable: usize,
    pub not_closed: usize,
    pub dependency_blocked: usize,
    /// Go golden key `dependency_blocked` alias for quick_ref.not_actionable.
    pub not_actionable_count: usize,
    pub by_status: BTreeMap<String, usize>,
    pub by_type: BTreeMap<String, usize>,
    #[serde(rename = "by_priority")]
    pub by_priority: BTreeMap<String, usize>,
}

pub fn compute_counts(
    issues: &[Issue],
    blocked_set: &std::collections::HashSet<String>,
) -> (ProjectCounts, QuickRef) {
    let mut c = ProjectCounts::default();
    let mut qr = QuickRef::default();
    c.total = issues.len();
    for i in issues {
        *c.by_status
            .entry(i.status.as_str().to_string())
            .or_default() += 1;
        *c.by_type.entry(i.issue_type.clone()).or_default() += 1;
        *c.by_priority.entry(i.priority.to_string()).or_default() += 1;

        if matches!(i.status, Status::Closed | Status::Tombstone) {
            c.closed += 1;
            continue;
        }
        // non-closed from here on
        c.not_closed += 1;
        match i.status {
            Status::Open => {
                c.open += 1;
                qr.open_count += 1;
            }
            Status::Blocked => {
                c.blocked += 1;
                qr.blocked_count += 1;
            }
            _ => {}
        }
        let blocked_here = blocked_set.contains(&i.id);
        if blocked_here {
            c.dependency_blocked += 1;
        }
        // actionable = open-like with no open blockers (Go definition)
        if i.status.is_open() && !blocked_here {
            c.actionable += 1;
        }
    }
    qr.open_count = c.open;
    qr.blocked_count = c.blocked;
    qr.actionable_count = c.actionable;
    qr.not_closed_count = c.not_closed;
    qr.not_actionable_count = qr.not_closed_count - qr.actionable_count;
    c.not_actionable_count = qr.not_actionable_count;
    (c, qr)
}

/// Compute the set of issue IDs that have >=1 open blocker.
pub fn compute_blocked_set(issues: &[Issue]) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    let mut open_ids: HashSet<&str> = HashSet::new();
    for i in issues {
        if i.status.is_open() {
            open_ids.insert(&i.id);
        }
    }
    let mut blocked = HashSet::new();
    for i in issues {
        for dep in &i.dependencies {
            if dep.r#type.is_blocking() {
                let target = dep.effective_depends_on().to_string();
                if open_ids.contains(target.as_str()) && target != i.id {
                    blocked.insert(i.id.clone());
                    break;
                }
            }
        }
    }
    blocked
}

/// Assemble the full triage recommendation list (ranked) + quick_ref +
/// project_health counts. Command templates built by caller with real IDs.
pub struct TriageOutput {
    pub recommendations: Vec<crate::impact::IssueImpact>,
    pub counts: ProjectCounts,
    pub quick_ref: QuickRef,
    pub velocity: Option<serde_json::Value>,
}

/// Velocity snapshot — port of Go `ComputeProjectVelocity` (triage.go:215).
/// Computes closure velocity from issue timestamps.
pub fn compute_project_velocity(
    issues: &[Issue],
    now: jiff::Timestamp,
) -> Option<serde_json::Value> {
    let week_ago = now - jiff::SignedDuration::from_secs(7 * 86400);
    let month_ago = now - jiff::SignedDuration::from_secs(30 * 86400);

    let mut closed_last_7 = 0usize;
    let mut closed_last_30 = 0usize;
    let mut total_close_secs = 0.0f64;
    let mut close_samples = 0usize;
    let mut estimated = false;

    for issue in issues {
        if !matches!(issue.status, Status::Closed | Status::Tombstone) {
            continue;
        }
        // Determine closure time (Go parity: closed_at > updated_at > now).
        let closed_at = issue
            .closed_at
            .as_deref()
            .and_then(|s| s.parse::<jiff::Timestamp>().ok())
            .or_else(|| {
                estimated = true;
                issue
                    .updated_at
                    .as_deref()
                    .and_then(|s| s.parse::<jiff::Timestamp>().ok())
            })
            .unwrap_or(now);

        if closed_at >= week_ago {
            closed_last_7 += 1;
        }
        if closed_at >= month_ago {
            closed_last_30 += 1;
        }
        // Average time-to-close.
        if let Some(created) = issue
            .created_at
            .as_deref()
            .and_then(|s| s.parse::<jiff::Timestamp>().ok())
        {
            let dur = closed_at - created;
            total_close_secs += dur.total(jiff::Unit::Second).unwrap_or(0.0);
            close_samples += 1;
        }
    }

    let avg_days = if close_samples > 0 {
        total_close_secs / 86400.0 / close_samples as f64
    } else {
        0.0
    };

    Some(serde_json::json!({
        "closed_last_7_days": closed_last_7,
        "closed_last_30_days": closed_last_30,
        "avg_days_to_close": (avg_days * 100.0).round() / 100.0,
        "estimated": estimated,
    }))
}

pub fn build_triage(issues: &[Issue], g: &DiGraph, now: jiff::Timestamp) -> TriageOutput {
    let pagerank = crate::algorithms::pagerank::pagerank_default(g);
    let betweenness = crate::algorithms::betweenness::betweenness(g);
    let heights = crate::algorithms::critical_path::critical_path_heights(g);
    let pr: BTreeMap<String, f64> = pagerank
        .into_iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), v))
        .collect();
    let bw: BTreeMap<String, f64> = betweenness
        .into_iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), v))
        .collect();
    let cp: BTreeMap<String, f64> = heights
        .into_iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), v))
        .collect();

    let inputs = ImpactInputs {
        issues,
        pagerank: &pr,
        betweenness: &bw,
        critical_path: &cp,
        g,
        now,
    };
    let recommendations = compute_impact_scores(&inputs);
    let blocked_set = compute_blocked_set(issues);
    let (counts, quick_ref) = compute_counts(issues, &blocked_set);
    let velocity = compute_project_velocity(issues, now);
    TriageOutput {
        recommendations,
        counts,
        quick_ref,
        velocity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_issues(name: &str) -> Vec<Issue> {
        let path = format!(
            "{}/../../tests/fixtures/{}/.beads/issues.jsonl",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        let raw = std::fs::read_to_string(path).unwrap();
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<Issue>(l).unwrap())
            .filter(|i| i.validate().is_ok())
            .collect()
    }

    #[test]
    fn small_chain_quick_ref_matches_golden() {
        // Golden: open=12 actionable=1 blocked=0 in_progress=0 not_closed=12
        //         not_actionable=11
        let issues = fixture_issues("small_chain");
        let g = crate::analyzer::build_graph(&issues);
        let out = build_triage(&issues, &g, jiff::Timestamp::now());
        assert_eq!(out.quick_ref.open_count, 12);
        assert_eq!(out.quick_ref.actionable_count, 1);
        assert_eq!(out.quick_ref.blocked_count, 0);
        assert_eq!(out.quick_ref.not_closed_count, 12);
        assert_eq!(out.quick_ref.not_actionable_count, 11);
        assert!(out.quick_ref.validate_invariant());
        // counts block matches golden too
        assert_eq!(out.counts.total, 12);
        assert_eq!(out.counts.dependency_blocked, 11);
    }

    #[test]
    fn recommendations_ranked_desc_with_id_tiebreak() {
        let issues = fixture_issues("medium_tree");
        let g = crate::analyzer::build_graph(&issues);
        let out = build_triage(&issues, &g, jiff::Timestamp::now());
        for w in out.recommendations.windows(2) {
            let ok = w[0].score > w[1].score || (w[0].score == w[1].score && w[0].id <= w[1].id);
            assert!(
                ok,
                "{:?} vs {:?}",
                (&w[0].id, w[0].score),
                (&w[1].id, w[1].score)
            );
        }
    }
}
