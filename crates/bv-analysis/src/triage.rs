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
///
/// Go parity (`br ready`/`br blocked`): blocking is inherited through
/// parent-child links — a child of a (transitively) blocked parent is
/// blocked even when it carries no direct `blocks` edge itself. Routes
/// through [`crate::blocker_chain::open_blockers`] (direct blocking edges
/// + transitive parent-blocked propagation) so every consumer agrees.
pub fn compute_blocked_set(issues: &[Issue]) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let by_id: HashMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut blocked = HashSet::new();
    for i in issues {
        // Skip self-loops: Go's graph drops them (gonum SimpleGraph rejects
        // self-edges), so a self-edge must not count as its own blocker.
        let direct = i.dependencies.iter().any(|dep| {
            if !dep.r#type.is_blocking() {
                return false;
            }
            let target = dep.effective_depends_on();
            target != i.id
                && by_id
                    .get(target)
                    .is_some_and(|b| b.status.is_open() && b.id != i.id)
        });
        if direct {
            blocked.insert(i.id.clone());
            continue;
        }
        // Ancestor-epic inheritance (#2): a child of a (transitively)
        // blocked parent is blocked even with no direct `blocks` edge.
        // Go's getOpenBlockersInternal surfaces the open parent only when
        // the parent is itself transitively blocked — never for a
        // standalone open parent — which is exactly what open_blockers
        // computes; here we only accept the parent-propagated part.
        let inherited = crate::blocker_chain::open_blockers(&by_id, &i.id)
            .into_iter()
            .any(|b| {
                i.dependencies.iter().any(|d| {
                    d.r#type == bv_core::model::DependencyType::ParentChild
                        && d.effective_depends_on() == b.as_str()
                })
            });
        if inherited {
            blocked.insert(i.id.clone());
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
    let mut week_buckets: BTreeMap<i64, usize> = BTreeMap::new();

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
        // Bucket by the Monday of the closure's ISO week.
        // Go (triage.go:299-304) keys `weekBuckets` by `isoWeekStart(year,
        // week)` — a time.Time Monday — and emits it as an RFC3339 date, not
        // an ISO week label. Keying by (year, week) and formatting as
        // "YYYY-Www" produced "2026-W34" where Go emits "2026-08-17T00:00:00Z".
        if let Some(ts_str) = issue.closed_at.as_deref().or(issue.updated_at.as_deref()) {
            if let Some((y, m, d)) = parse_ymd(ts_str) {
                let (iso_year, iso_week) = ymd_to_iso_week(y, m, d);
                if let Some(monday) = iso_week_start(iso_year, iso_week) {
                    *week_buckets.entry(monday).or_insert(0) += 1;
                }
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
    // Go (triage.go:314-321) starts at `truncateToMonday(now)` and steps back
    // 7 days at a time, emitting each cursor as an RFC3339 timestamp.
    let mut weekly: Vec<serde_json::Value> = Vec::new();
    if let Some((ny, nm, nd)) = parse_ymd(&now.to_string()) {
        let (cur_y, cur_w) = ymd_to_iso_week(ny, nm, nd);
        if let Some(mut cursor) = iso_week_start(cur_y, cur_w) {
            for _ in 0..8 {
                let count = week_buckets.get(&cursor).copied().unwrap_or(0);
                let ts = jiff::Timestamp::from_second(cursor * 86400)
                    .map(|t| format!("{}T00:00:00Z", t.strftime("%Y-%m-%d")))
                    .unwrap_or_default();
                weekly.push(serde_json::json!({
                    "week_start": ts,
                    "closed": count,
                }));
                cursor -= 7;
            }
        }
    }

    Some(serde_json::json!({
        "closed_last_7_days": closed_last_7,
        "closed_last_30_days": closed_last_30,
        "avg_days_to_close": avg_days,
        "weekly": weekly,
        "estimated": estimated,
    }))
}

/// Days since the Unix epoch for the Monday that starts the given ISO week.
/// Go `isoWeekStart` (triage.go) reconstructs the Monday from the ISO
/// year/week pair; the four-day rule puts the reference in that same week.
fn iso_week_start(iso_year: i32, iso_week: u32) -> Option<i64> {
    // Jan 4 is always in ISO week 1.
    let jan4 = days_from_civil(iso_year, 1, 4);
    // Weekday of Jan 4, 0=Monday.
    let jan4_dow = (jan4 + 3).rem_euclid(7);
    let week1_monday = jan4 - jan4_dow;
    Some(week1_monday + (iso_week as i64 - 1) * 7)
}

/// Days since 1970-01-01 for a proleptic-Gregorian y/m/d (Howard Hinnant's
/// `days_from_civil`), matching Go's `time.Date(...).Unix()` / 86400.
fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
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

    // 1. Compute the unblocks map (Go `buildUnblocksMap`, triage.go:797).
    //    Reverse map: blocker_id -> the issues it would unblock. Only an issue
    //    blocked by EXACTLY ONE open blocker counts, and only when completing
    //    that blocker makes the issue actionable. This is deliberately not
    //    plain in-degree: Go only credits a blocker for issues it alone holds up.
    let mut unblocks_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for issue in issues {
        if matches!(issue.status, Status::Closed | Status::Tombstone) {
            continue;
        }
        unblocks_map.entry(issue.id.clone()).or_default();
        let open_blockers: Vec<&str> = issue
            .dependencies
            .iter()
            .filter(|d| d.r#type.is_blocking())
            .map(|d| d.effective_depends_on())
            .filter(|bid| !bid.is_empty())
            .filter(|bid| {
                issues.iter().any(|i| {
                    i.id == *bid && !matches!(i.status, Status::Closed | Status::Tombstone)
                })
            })
            .collect();
        if open_blockers.len() == 1 {
            unblocks_map
                .entry(open_blockers[0].to_string())
                .or_default()
                .push(issue.id.clone());
        }
    }
    for list in unblocks_map.values_mut() {
        list.sort();
    }
    let max_unblocks = unblocks_map.values().map(|v| v.len()).max().unwrap_or(0);

    /// Go `getBlockerDepthRecursive` (triage.go:1440-1470): the length of the
    /// longest chain of open blockers above this issue. Returns -1 on a cycle, and
    /// memoizes the result so a wide graph stays linear.
    fn blocker_depth_recursive(
        issue_id: &str,
        issues: &[Issue],
        visited: &mut std::collections::BTreeSet<String>,
        memo: &mut BTreeMap<String, i64>,
    ) -> i64 {
        if let Some(val) = memo.get(issue_id) {
            return *val;
        }
        if !visited.insert(issue_id.to_string()) {
            return -1; // cycle
        }
        let open_blockers: Vec<String> = issues
            .iter()
            .find(|i| i.id == issue_id)
            .map(|i| {
                i.dependencies
                    .iter()
                    .filter(|d| d.r#type.is_blocking())
                    .map(|d| d.effective_depends_on().to_string())
                    .filter(|bid| {
                        !bid.is_empty() && issues.iter().any(|o| o.id == *bid && o.status.is_open())
                    })
                    .collect()
            })
            .unwrap_or_default();
        if open_blockers.is_empty() {
            visited.remove(issue_id);
            memo.insert(issue_id.to_string(), 0);
            return 0;
        }
        let mut max_chain = 0i64;
        for blocker_id in &open_blockers {
            let depth = blocker_depth_recursive(blocker_id, issues, visited, memo);
            if depth == -1 {
                visited.remove(issue_id);
                return -1;
            }
            max_chain = max_chain.max(depth);
        }
        visited.remove(issue_id);
        memo.insert(issue_id.to_string(), max_chain + 1);
        max_chain + 1
    }

    // 2. Compute blocker depths. Go `getBlockerDepthRecursive` (triage.go:1440)
    //    walks the whole open-blocker CHAIN and returns its maximum length, not
    //    the number of direct blockers. Counting direct open blockers gave every
    //    mid-chain issue a depth of 1, so the quick-win boost (gated at
    //    depth <= 2) fired far too often and every score past the first link
    //    came out too high.
    let mut blocker_depths: BTreeMap<String, usize> = BTreeMap::new();
    for issue in issues {
        if !issue.status.is_open() {
            continue;
        }
        let mut visited: std::collections::BTreeSet<String> = Default::default();
        let mut memo: BTreeMap<String, i64> = BTreeMap::new();
        let depth = blocker_depth_recursive(&issue.id, issues, &mut visited, &mut memo);
        if depth >= 0 {
            blocker_depths.insert(issue.id.clone(), depth as usize);
        }
    }

    // Go builds the recommendation's graph context here: `UnblocksIDs` from the
    // unblocks map (triage.go:947) and `BlockedBy` from the context's open
    // blockers, set only when non-empty (triage.go:949). Both are omitted
    // when empty, so the field order is reasons, unblocks_ids, blocked_by.
    let issue_index: std::collections::HashMap<&str, &Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();
    for rec in recommendations.iter_mut() {
        rec.unblocks_ids = unblocks_map.get(&rec.id).cloned().unwrap_or_default();
        rec.blocked_by = crate::blocker_chain::open_blockers(&issue_index, &rec.id);
        // Go's claimable gate requires zero open blockers
        // (isClaimableRecommendation, triage.go:1191). The impact scorer
        // stamps a preliminary verdict before it can see the dependency graph,
        // so refine it here where blocked_by is known.
        if !rec.blocked_by.is_empty() {
            rec.claimable = false;
        }
        let blocker_depth = *blocker_depths.get(&rec.id).unwrap_or(&0);
        let unblocks = rec.unblocks_ids.len();
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
        // Go marks a quick win by `QuickWinBoost > 0.05` (triage.go:1759) and
        // rewrites the action hint for a genuinely startable bead
        // (triage.go:1605-1622). The boost is only known here, so the hint is
        // stamped in this layer rather than where the base action is derived.
        if quickwin_boost > 0.05 {
            // Go emits the quick-win reason immediately BEFORE the
            // claim-status reason (triage.go:1605, then 1625) and rewrites
            // the action hint from the same predicate, but only for a bead
            // that is genuinely startable (no open blockers).
            const QUICK_WIN_REASON: &str = "⚡ Low effort, high impact - good starting point";
            if let Some(pos) = rec
                .reasons
                .iter()
                .position(|r| r.starts_with("✅ Currently unclaimed"))
            {
                rec.reasons.insert(pos, QUICK_WIN_REASON.to_string());
            } else {
                rec.reasons.push(QUICK_WIN_REASON.to_string());
            }
            if rec.blocked_by.is_empty() {
                rec.action = "Quick win - start here for fast progress".to_string();
            }
        }
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
