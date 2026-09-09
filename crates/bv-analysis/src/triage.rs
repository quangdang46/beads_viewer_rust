//! Triage payload assembly — port of Go triage output shapes:
//! quick_ref (#165 strict semantics), project_health, commands.

use crate::impact::{compute_impact_scores, ImpactInputs};
use bv_core::model::{Issue, Status};
use bv_graph_core::DiGraph;
use serde::Serialize;
use std::collections::BTreeMap;

// === Triage scoring weights (Go triage.go:1203-1208) — DO NOT ALTER ===
/// Base score weight applied to the impact score.
pub const TRIAGE_BASE_WEIGHT: f64 = 0.70;
/// Maximum boost for unblock count.
pub const TRIAGE_UNBLOCK_BOOST: f64 = 0.15;
/// Maximum boost for quick-win potential.
pub const TRIAGE_QUICKWIN_BOOST: f64 = 0.15;
/// Minimum unblocks required for full unblock boost (normalization floor).
pub const TRIAGE_UNBLOCK_THRESHOLD: usize = 5;
/// Maximum blocker depth eligible for quick-win boost.
pub const TRIAGE_QUICKWIN_MAX_DEPTH: usize = 2;

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

/// Per-row triage decoration for the TUI list (Go `IssueItem` triage fields:
/// `IsQuickWin`, `IsBlocker`, `UnblocksCount` — populated in Go from
/// `triageResult.QuickWins` / `BlockersToClear` / `unblocksMap`).
#[derive(Debug, Clone, Default)]
pub struct RowTriage {
    pub is_quick_win: bool,
    pub is_blocker: bool,
    pub unblocks_count: usize,
}

/// Compute per-row triage decorations for every issue, in one pass.
///
/// Semantics (Go `buildUnblocksMap` + `buildQuickWins` + `buildBlockersToClear`):
/// - `unblocks_count[id]` = number of non-closed-like issues B for which `id`
///   is the ONLY remaining open blocker (direct blocking edge or
///   parent-propagated), and B would be actionable once `id` completes.
/// - `is_blocker` = appears in the blockers-to-clear set: any non-closed-like
///   issue with >= 1 unblock, sorted by unblock count desc, id asc. The TUI
///   keeps the full set (not Go's display `limit`) since it's a per-row flag,
///   not a top-N list.
/// - `is_quick_win` = claimable (open, non-epic, unassigned, zero open
///   blockers — Go's `isClaimableRecommendation` minus scheduler-deferral and
///   not-ready-labels, neither of which the TUI models) AND in the top 5 of
///   the quick-win ranking (`log2(unblocks+1)*0.4 + simplicity*0.4 +
///   priority_bonus*0.2`, score desc, id asc; Go `buildQuickWins` with
///   `BlokerRatioNorm` approximated by the raw blocker ratio — the normalized
///   value isn't available without the full impact-scoring pipeline).
///
/// Shared by the TUI row renderer and (future) CLI callers so the two never
/// drift apart — `crates/bv/src/main.rs`'s inline `quick_wins` /
/// `blockers_to_clear` builders are the golden-covered legacy copies and are
/// intentionally left untouched.
pub fn compute_row_triage(issues: &[Issue]) -> std::collections::HashMap<String, RowTriage> {
    use std::collections::{HashMap, HashSet};
    fn closed_like(s: Status) -> bool {
        matches!(s, Status::Closed | Status::Tombstone)
    }

    let by_id: HashMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();

    // Open blockers per issue (direct blocking edges + parent propagation),
    // via the shared blocker_chain helper (Go `getOpenBlockersInternal`).
    let open_blockers_of =
        |id: &str| -> Vec<String> { crate::blocker_chain::open_blockers(&by_id, id) };

    // Go `buildUnblocksMap`: B counts toward A iff A is B's ONLY open blocker
    // and B is actionable-after-completing-A (approximated here as:
    // non-closed-like and non-deferred — scheduler deferral has no Rust model
    // beyond `Status::Deferred`).
    let mut unblocks: HashMap<String, Vec<String>> = HashMap::new();
    for issue in issues {
        unblocks.entry(issue.id.clone()).or_default();
        if closed_like(issue.status) || issue.status == Status::Deferred {
            continue;
        }
        let blockers = open_blockers_of(&issue.id);
        if blockers.len() == 1 {
            unblocks
                .entry(blockers[0].clone())
                .or_default()
                .push(issue.id.clone());
        }
    }
    for list in unblocks.values_mut() {
        list.sort();
    }

    // Go `buildQuickWins` ranking over the claimable subset.
    let blocker_ratio_of = |id: &str| -> f64 {
        let Some(issue) = by_id.get(id) else {
            return 0.0;
        };
        let total = issues.len().max(1) as f64;
        let blocked_by_count = issues
            .iter()
            .filter(|o| {
                !closed_like(o.status)
                    && o.dependencies
                        .iter()
                        .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == id)
            })
            .count() as f64;
        let _ = issue;
        blocked_by_count / total
    };
    let mut qw_candidates: Vec<(f64, &str)> = Vec::new();
    for issue in issues {
        // Claimable subset (Go `isClaimableRecommendation`, TUI-applicable
        // parts): open, non-epic, unassigned, zero open blockers.
        if issue.status != Status::Open {
            continue;
        }
        if issue.issue_type == "epic" {
            continue;
        }
        if !issue.assignee.is_empty() {
            continue;
        }
        if !open_blockers_of(&issue.id).is_empty() {
            continue;
        }
        let unblocks_count = unblocks.get(&issue.id).map(Vec::len).unwrap_or(0);
        let unblock_impact = ((unblocks_count as f64) + 1.0).log2();
        let ratio = blocker_ratio_of(&issue.id);
        let simplicity = if ratio < 0.2 {
            1.0
        } else if ratio < 0.4 {
            0.5
        } else {
            0.0
        };
        let priority_bonus = if issue.priority <= 1 { 0.5 } else { 0.0 };
        let qw_score = unblock_impact * 0.4 + simplicity * 0.4 + priority_bonus * 0.2;
        qw_candidates.push((qw_score, issue.id.as_str()));
    }
    qw_candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(b.1))
    });
    let quick_win_set: HashSet<&str> = qw_candidates.iter().take(5).map(|(_, id)| *id).collect();

    let mut out: HashMap<String, RowTriage> = HashMap::new();
    for issue in issues {
        let unblocks_count = unblocks.get(&issue.id).map(Vec::len).unwrap_or(0);
        let is_blocker = !closed_like(issue.status) && unblocks_count > 0;
        out.insert(
            issue.id.clone(),
            RowTriage {
                is_quick_win: quick_win_set.contains(issue.id.as_str()),
                is_blocker,
                unblocks_count,
            },
        );
    }
    out
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
    /// Go TriageConfig parity: PageRank computed, Betweenness computed with
    /// reason "approximate" (sample recorded only when the approximator
    /// actually sampled), every other Phase-2 metric skipped.
    pub metric_status: crate::analyzer::MetricStatus,
}

/// Parse YYYY-MM-DDTHH:MM:SS from an RFC3339 timestamp string.
fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let y: i32 = s[0..4].parse().ok()?;
    let m: u32 = s[5..7].parse().ok()?;
    let d: u32 = s[8..10].parse().ok()?;
    Some((y, m, d))
}

/// Convert year/month/day to ISO year + ISO week number (Monday-start).
/// Uses Julian Day Number arithmetic for ISO week computation.
fn ymd_to_iso_week(y: i32, m: u32, d: u32) -> (i32, u32) {
    fn julian_day(y: i32, m: i32, d: i32) -> i64 {
        let a = (14 - m) / 12;
        let yy = y + 4800 - a;
        let mm = m + 12 * a - 3;
        (d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045) as i64
    }
    let jd = julian_day(y, m as i32, d as i32);
    let dow = (jd % 7 + 7) % 7; // 0=Monday
    let day_of_year = jd - julian_day(y, 1, 1) + 1;
    let week_num = ((day_of_year - dow + 10) / 7) as u32;

    if week_num == 0 {
        // Belongs to previous year's last week.
        let prev_y = y - 1;
        let prev_jan1_dow = (julian_day(prev_y, 1, 1) % 7 + 7) % 7;
        let is_leap = prev_y % 4 == 0 && (prev_y % 100 != 0 || prev_y % 400 == 0);
        let total_days = 365 + i64::from(is_leap);
        let weeks_in_prev = ((total_days - prev_jan1_dow + 6) / 7) as u32;
        (prev_y, weeks_in_prev)
    } else if week_num > 52 {
        // Check if it belongs to next year's week 1.
        let dec31_dow = (julian_day(y, 12, 31) % 7 + 7) % 7;
        if dow >= 4 - dec31_dow {
            (y + 1, 1)
        } else {
            (y, week_num)
        }
    } else {
        (y, week_num)
    }
}

/// Velocity snapshot — port of Go `ComputeProjectVelocity` (triage.go:215).
/// Computes closure velocity from issue timestamps, including weekly buckets.
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

    // Weekly buckets: key = (iso_year, iso_week), value = count.
    // Go uses 8 weeks, Monday-start ISO weeks.
    use std::collections::BTreeMap;
    let mut week_buckets: BTreeMap<(i32, u32), usize> = BTreeMap::new();

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
        // Bucket by ISO week (year, week_of_year) from timestamp string.
        if let Some(ts_str) = issue.closed_at.as_deref().or(issue.updated_at.as_deref()) {
            if let Some((y, m, d)) = parse_ymd(ts_str) {
                let (iso_year, iso_week) = ymd_to_iso_week(y, m, d);
                *week_buckets.entry((iso_year, iso_week)).or_insert(0) += 1;
            }
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

    // Build weekly array (newest first, 8 weeks). Go parity: VelocityWeek.
    // Compute the most recent Monday and iterate backwards.
    let mut weekly: Vec<serde_json::Value> = Vec::new();
    let now_str = now.to_string();
    if let Some((ny, nm, nd)) = parse_ymd(&now_str) {
        let (cur_y, cur_w) = ymd_to_iso_week(ny, nm, nd);
        for offset in 0..8i32 {
            // Subtract offset weeks from current ISO week.
            let mut wy = cur_y;
            let mut ww = cur_w as i32 - offset;
            while ww <= 0 {
                wy -= 1;
                let (_, prev_w52) = ymd_to_iso_week(wy + 1, 1, 4);
                ww += prev_w52 as i32;
            }
            let count = week_buckets.get(&(wy, ww as u32)).copied().unwrap_or(0);
            weekly.push(serde_json::json!({
                "week_start": format!("{wy}-W{ww:02}"),
                "closed": count,
            }));
        }
    }

    Some(serde_json::json!({
        "closed_last_7_days": closed_last_7,
        "closed_last_30_days": closed_last_30,
        "avg_days_to_close": (avg_days * 100.0).round() / 100.0,
        "weekly": weekly,
        "estimated": estimated,
    }))
}

pub fn build_triage(issues: &[Issue], g: &DiGraph, now: jiff::Timestamp) -> TriageOutput {
    // Go TriageConfig (fast config) parity: PageRank exact, Betweenness
    // approximate with sample 50 (falling back to exact inside the
    // approximator when sample >= n), all other Phase-2 metrics skipped.
    const TRIAGE_BETWEENNESS_SAMPLE: usize = 50;

    let n = g.len();
    let t0 = std::time::Instant::now();
    let pagerank = crate::algorithms::pagerank::pagerank_default(g);
    let pr_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let (betweenness, actual_sample) = if TRIAGE_BETWEENNESS_SAMPLE >= n {
        // Go ApproxBetweenness falls back to exact when sample >= n; the
        // config-mode reason stays "approximate" but no sample is reported.
        (crate::algorithms::betweenness::betweenness(g), 0)
    } else {
        (
            crate::algorithms::betweenness::betweenness_approx(
                g,
                TRIAGE_BETWEENNESS_SAMPLE,
                Some(1),
            ),
            TRIAGE_BETWEENNESS_SAMPLE,
        )
    };
    let bw_ms = t1.elapsed().as_secs_f64() * 1000.0;

    // Go TriageConfig skips ComputeCriticalPath: no time-to-impact component.
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

    let skipped = crate::analyzer::StatusEntry::skipped("disabled by triage fast config");
    let metric_status = crate::analyzer::MetricStatus {
        page_rank: crate::analyzer::StatusEntry::computed(pr_ms),
        betweenness: {
            let mut e = crate::analyzer::StatusEntry::computed(bw_ms);
            e.reason = "approximate".into();
            e.sample = actual_sample;
            e
        },
        eigenvector: skipped.clone(),
        hits: skipped.clone(),
        critical: skipped.clone(),
        cycles: skipped.clone(),
        kcore: skipped.clone(),
        articulation: skipped.clone(),
        slack: skipped.clone(),
    };

    let inputs = ImpactInputs {
        issues,
        pagerank: &pr,
        betweenness: &bw,
        critical_path: None, // Go TriageConfig skips ComputeCriticalPath
        g,
        now,
    };
    let mut recommendations = compute_impact_scores(&inputs);

    // === Go triage scoring (triage.go:1267-1311) ===
    // triageScore = baseScore * 0.70 + unblockBoost + quickwinBoost
    // This transforms raw impact scores into triage-prioritized scores.

    // 1. Compute unblock counts: how many open issues each issue unblocks.
    let mut unblock_counts: BTreeMap<String, usize> = BTreeMap::new();
    for issue in issues {
        if !issue.status.is_open() {
            continue;
        }
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let blocker_id = dep.effective_depends_on();
            if !blocker_id.is_empty() {
                *unblock_counts.entry(blocker_id.to_string()).or_insert(0) += 1;
            }
        }
    }
    let max_unblocks = unblock_counts.values().copied().max().unwrap_or(0);

    // 2. Compute blocker depths: how many open blocking deps each issue has.
    let mut blocker_depths: BTreeMap<String, usize> = BTreeMap::new();
    for issue in issues {
        if !issue.status.is_open() {
            continue;
        }
        let depth = issue
            .dependencies
            .iter()
            .filter(|d| d.r#type.is_blocking())
            .filter(|d| {
                let bid = d.effective_depends_on();
                if bid.is_empty() {
                    return false;
                }
                // Count only open blockers
                issues.iter().any(|i| i.id == bid && i.status.is_open())
            })
            .count();
        blocker_depths.insert(issue.id.clone(), depth);
    }

    // 3. Apply triage scoring to each recommendation.
    for rec in &mut recommendations {
        let unblocks = *unblock_counts.get(&rec.id).unwrap_or(&0);
        let blocker_depth = *blocker_depths.get(&rec.id).unwrap_or(&0);
        let base_score = rec.score;

        // Unblock boost: normalized unblocks * weight
        let unblock_boost = if unblocks > 0 {
            let norm = unblocks as f64 / (max_unblocks.max(TRIAGE_UNBLOCK_THRESHOLD) as f64);
            norm.min(1.0) * TRIAGE_UNBLOCK_BOOST
        } else {
            0.0
        };

        // Quick-win boost: depth-based factor * base score * weight
        let quickwin_boost = if rec.status != Status::InProgress.as_str()
            && blocker_depth <= TRIAGE_QUICKWIN_MAX_DEPTH
        {
            let depth_factor =
                1.0 - blocker_depth as f64 / (TRIAGE_QUICKWIN_MAX_DEPTH as f64 + 1.0);
            (depth_factor * base_score * TRIAGE_QUICKWIN_BOOST).min(TRIAGE_QUICKWIN_BOOST)
        } else {
            0.0
        };

        rec.score = base_score * TRIAGE_BASE_WEIGHT + unblock_boost + quickwin_boost;
    }

    // Re-sort by triage score descending, ID ascending (Go tie-break).
    recommendations.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });

    let blocked_set = compute_blocked_set(issues);
    let (counts, quick_ref) = compute_counts(issues, &blocked_set);
    let velocity = compute_project_velocity(issues, now);
    TriageOutput {
        recommendations,
        counts,
        quick_ref,
        velocity,
        metric_status,
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
