//! Drift detection — port of Go `pkg/drift` (Calculator + Result + exit
//! codes) and `pkg/baseline` snapshot format v1.

use bv_core::model::Issue;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Info,
    Critical,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlertType {
    #[default]
    NewCycle,
    DensityGrowth,
    NodeCountChange,
    EdgeCountChange,
    BlockedIncrease,
    ActionableChange,
    PagerankChange,
    StaleIssue,
    BlockingCascade,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Alert {
    #[serde(rename = "type")]
    pub alert_type: AlertType,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "baseline_value")]
    pub baseline_val: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "current_value")]
    pub current_val: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unblocks_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downstream_priority_sum: Option<i64>,
}

/// Baseline stats snapshot (Go baseline.json v1 subset used by checks).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BaselineStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub density: f64,
    pub open: usize,
    pub closed: usize,
    pub blocked: usize,
    pub cycle_count: usize,
    pub actionable: usize,
    /// top PageRank per issue id (top-N stored at capture time)
    #[serde(default)]
    pub pagerank: BTreeMap<String, f64>,
}

/// Per-label staleness threshold overrides (Go `LabelConfig`, bv-167).
#[derive(Debug, Clone, Default)]
pub struct LabelConfig {
    pub stale_warning_days: Option<u32>,
    pub stale_critical_days: Option<u32>,
    pub in_progress_stale_multiplier: Option<f64>,
}

/// Drift configuration thresholds (Go DefaultConfig).
#[derive(Debug, Clone)]
pub struct DriftConfig {
    pub density_warning_pct: f64,
    pub density_info_pct: f64,
    pub node_growth_info_pct: f64,
    pub edge_growth_info_pct: f64,
    pub blocked_increase_threshold: i64,
    pub actionable_decrease_warning_pct: f64,
    pub actionable_increase_info_pct: f64,
    pub pagerank_change_warning_pct: f64,
    /// Days since last update before a stale warning is emitted.
    pub stale_warning_days: u32,
    /// Days since last update before a stale critical alert is emitted.
    pub stale_critical_days: u32,
    /// Multiplier applied to staleness thresholds for in-progress items.
    /// <1 tightens thresholds (items age faster).
    pub in_progress_stale_multiplier: f64,
    /// Minimum unblocks count for an info-level BlockingCascade alert.
    pub blocking_cascade_info_threshold: i64,
    /// Minimum unblocks count for a warning-level BlockingCascade alert.
    pub blocking_cascade_warning_threshold: i64,
    /// Alert types that are disabled and should not generate alerts (bv-167).
    pub disabled_alerts: Vec<String>,
    /// Per-label staleness overrides (bv-167).
    pub label_overrides: BTreeMap<String, LabelConfig>,
}

impl Default for DriftConfig {
    fn default() -> Self {
        DriftConfig {
            density_warning_pct: 50.0,
            density_info_pct: 20.0,
            node_growth_info_pct: 25.0,
            edge_growth_info_pct: 25.0,
            blocked_increase_threshold: 5,
            actionable_decrease_warning_pct: 30.0,
            actionable_increase_info_pct: 20.0,
            pagerank_change_warning_pct: 50.0,
            stale_warning_days: 14,
            stale_critical_days: 30,
            in_progress_stale_multiplier: 0.5,
            blocking_cascade_info_threshold: 3,
            blocking_cascade_warning_threshold: 5,
            disabled_alerts: Vec::new(),
            label_overrides: BTreeMap::new(),
        }
    }
}

impl DriftConfig {
    /// Returns true if the given alert type is in the disabled list (bv-167).
    pub fn is_alert_disabled(&self, alert_type: &str) -> bool {
        self.disabled_alerts.iter().any(|d| d == alert_type)
    }

    /// Resolve staleness thresholds for an issue based on its labels (bv-167).
    /// If multiple labels have overrides, the tightest (smallest) non-zero
    /// thresholds among them are used.  Unset values inherit the global default.
    pub fn get_staleness_thresholds(&self, labels: &[String]) -> (u32, u32, f64) {
        let applicable: Vec<&LabelConfig> = labels
            .iter()
            .filter_map(|l| self.label_overrides.get(l))
            .collect();

        if applicable.is_empty() {
            return (
                self.stale_warning_days,
                self.stale_critical_days,
                self.in_progress_stale_multiplier,
            );
        }

        let resolve = |lc: &LabelConfig| -> (u32, u32, f64) {
            let w = lc.stale_warning_days.unwrap_or(self.stale_warning_days);
            let c = lc.stale_critical_days.unwrap_or(self.stale_critical_days);
            let m = lc
                .in_progress_stale_multiplier
                .unwrap_or(self.in_progress_stale_multiplier);
            (w, c, m)
        };

        let (mut warn, mut crit, mut mult) = resolve(applicable[0]);
        for lc in &applicable[1..] {
            let (w, c, m) = resolve(lc);
            if w < warn {
                warn = w;
            }
            if c < crit {
                crit = c;
            }
            if m < mult {
                mult = m;
            }
        }
        (warn, crit, mult)
    }
}

/// Complete drift analysis result (Go `Result`).
#[derive(Debug, Default, Serialize)]
pub struct DriftResult {
    pub has_drift: bool,
    pub alerts: Vec<Alert>,
    pub critical_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
}

impl DriftResult {
    /// CI contract: exit 0=OK, 1=critical, 2=warning.
    pub fn exit_code(&self) -> u8 {
        if self.critical_count > 0 {
            1
        } else if self.warning_count > 0 {
            2
        } else {
            0
        }
    }

    fn push(&mut self, alert: Alert) {
        self.has_drift = true;
        match alert.severity {
            Severity::Critical => self.critical_count += 1,
            Severity::Warning => self.warning_count += 1,
            Severity::Info => self.info_count += 1,
        }
        self.alerts.push(alert);
    }
}

fn pct_change(baseline: f64, current: f64) -> Option<f64> {
    if baseline == 0.0 {
        return None;
    }
    Some(((current - baseline) / baseline) * 100.0)
}

/// Parse an ISO-8601 timestamp string to seconds since Unix epoch.
/// Returns `None` on parse failure.
fn parse_ts_secs(s: &str) -> Option<f64> {
    s.parse::<jiff::Timestamp>()
        .ok()
        .map(|ts| ts.as_millisecond() as f64 / 1000.0)
}

/// Emit StaleIssue alerts for open/in_progress issues whose last update is
/// beyond the configured staleness thresholds.  Mirrors Go
/// `Calculator.checkStaleness`.
fn check_staleness(result: &mut DriftResult, cfg: &DriftConfig, issues: &[Issue]) {
    if cfg.is_alert_disabled("stale_issue") || issues.is_empty() {
        return;
    }
    let now_secs = jiff::Timestamp::now().as_millisecond() as f64 / 1000.0;

    for issue in issues {
        // Skip closed and deferred statuses.
        if issue.status.is_closed() || issue.status == bv_core::model::Status::Deferred {
            continue;
        }

        // Last activity = max(updated_at, created_at).
        let last_active_secs = issue
            .updated_at
            .as_deref()
            .or(issue.created_at.as_deref())
            .and_then(parse_ts_secs)
            .unwrap_or(0.0);
        if last_active_secs == 0.0 {
            continue;
        }

        let (warn_days, crit_days, in_progress_mult) = cfg.get_staleness_thresholds(&issue.labels);

        let mut warn_secs = warn_days as f64 * 86400.0;
        let mut crit_secs = crit_days as f64 * 86400.0;

        // Tighten thresholds for in-progress items (Go: in_progress_stale_multiplier).
        if issue.status == bv_core::model::Status::InProgress && in_progress_mult > 0.0 {
            warn_secs *= in_progress_mult;
            crit_secs *= in_progress_mult;
        }

        let inactive_secs = now_secs - last_active_secs;
        if inactive_secs < 0.0 {
            continue;
        }
        let inactive_days = inactive_secs / 86400.0;

        let severity = if inactive_secs >= crit_secs {
            Severity::Critical
        } else if inactive_secs >= warn_secs {
            Severity::Warning
        } else {
            continue;
        };

        let status_str = match issue.status {
            bv_core::model::Status::Open => "open",
            bv_core::model::Status::InProgress => "in_progress",
            bv_core::model::Status::Closed => "closed",
            bv_core::model::Status::Deferred => "deferred",
            _ => "other",
        };

        result.push(Alert {
            alert_type: AlertType::StaleIssue,
            severity,
            message: format!("Issue {} inactive for {:.0} days", issue.id, inactive_days),
            baseline_val: None,
            current_val: Some(inactive_days),
            delta: None,
            details: vec![
                format!("status={status_str}"),
                format!("last_update={}", issue.updated_at.as_deref().unwrap_or("")),
            ],
            issue_id: issue.id.clone(),
            label: String::new(),
            detected_at: None,
            unblocks_count: None,
            downstream_priority_sum: None,
        });
    }
}

/// Compute the set of issue IDs that `issue_id` would unblock if completed.
///
/// An issue is considered "unblocked" by completing `issue_id` when:
/// 1. It depends on `issue_id` via a blocking dependency type.
/// 2. It is not already closed-like.
/// 3. It has no other open blockers besides `issue_id`.
///
/// This is a simplified port of Go `Analyzer.computeUnblocks` (plan.go:85).
fn compute_unblocks(issues: &[Issue], issue_id: &str) -> Vec<String> {
    let by_id: BTreeMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();

    let Some(blocker) = by_id.get(issue_id) else {
        return Vec::new();
    };

    // Find all non-closed issues that directly depend on this blocker.
    let dependents: Vec<&Issue> = issues
        .iter()
        .filter(|i| {
            !i.status.is_closed()
                && i.id != issue_id
                && i.dependencies
                    .iter()
                    .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == blocker.id)
        })
        .collect();

    let mut unblocks = Vec::new();
    for dep_issue in dependents {
        // Check if this dependent has other open blockers besides `issue_id`.
        let still_blocked = dep_issue.dependencies.iter().any(|d| {
            if !d.r#type.is_blocking() {
                return false;
            }
            let target = d.effective_depends_on();
            if target == issue_id {
                return false; // The one we are completing.
            }
            // Check if that other blocker is still open.
            by_id
                .get(target)
                .is_some_and(|other| !other.status.is_closed())
        });
        if !still_blocked {
            unblocks.push(dep_issue.id.clone());
        }
    }
    unblocks.sort();
    unblocks
}

/// Emit BlockingCascade alerts for issues whose completion would unblock
/// many downstream dependents.  Mirrors Go `Calculator.checkBlockingCascade`.
fn check_blocking_cascade(result: &mut DriftResult, cfg: &DriftConfig, issues: &[Issue]) {
    if cfg.is_alert_disabled("blocking_cascade") || issues.is_empty() {
        return;
    }
    let info_thresh = cfg.blocking_cascade_info_threshold;
    let warn_thresh = cfg.blocking_cascade_warning_threshold;
    if info_thresh <= 0 && warn_thresh <= 0 {
        return;
    }

    // Only check actionable (non-closed, non-deferred) issues.
    for issue in issues {
        if issue.status.is_closed() || issue.status == bv_core::model::Status::Deferred {
            continue;
        }
        let unblocked = compute_unblocks(issues, &issue.id);
        let count = unblocked.len() as i64;
        if count == 0 {
            continue;
        }

        let severity = if warn_thresh > 0 && count >= warn_thresh {
            Severity::Warning
        } else if info_thresh > 0 && count < info_thresh {
            continue;
        } else {
            Severity::Info
        };

        // Downstream priority sum for urgency scoring (bv-165).
        // Lower priority values = higher importance (P0=critical, P4=backlog).
        let issue_map: BTreeMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();
        let priority_sum: i64 = unblocked
            .iter()
            .filter_map(|id| issue_map.get(id.as_str()))
            .map(|i| i.priority as i64)
            .sum();

        result.push(Alert {
            alert_type: AlertType::BlockingCascade,
            severity,
            message: format!(
                "Completing {} unblocks {} downstream item(s)",
                issue.id, count
            ),
            baseline_val: None,
            current_val: Some(count as f64),
            delta: None,
            details: unblocked,
            issue_id: issue.id.clone(),
            label: String::new(),
            detected_at: None,
            unblocks_count: Some(count),
            downstream_priority_sum: Some(priority_sum),
        });
    }
}

/// Run all drift checks between two snapshots.
///
/// `issues` is the current issue list for issue-level alerts (staleness,
/// blocking cascade).  Pass `&[]` to skip issue-level checks.
pub fn calculate(
    baseline: &BaselineStats,
    current: &BaselineStats,
    cfg: &DriftConfig,
    new_cycles: &[Vec<String>],
    issues: &[Issue],
) -> DriftResult {
    let mut r = DriftResult::default();

    // Cycles: any NEW cycle is critical.
    if !new_cycles.is_empty() {
        let names: Vec<String> = new_cycles.iter().map(|c| c.join(" -> ")).collect();
        r.push(Alert {
            alert_type: AlertType::NewCycle,
            severity: Severity::Critical,
            message: format!("New dependency cycles introduced: {}", names.join("; ")),
            ..Default::default()
        });
    }

    // Density growth.
    if baseline.density > 0.0 {
        if let Some(pct) = pct_change(baseline.density, current.density) {
            let sev = if pct >= cfg.density_warning_pct {
                Some(Severity::Warning)
            } else if pct >= cfg.density_info_pct {
                Some(Severity::Info)
            } else {
                None
            };
            if let Some(sev) = sev {
                r.push(Alert {
                    alert_type: AlertType::DensityGrowth,
                    severity: sev,
                    message: format!("Graph density increased by {pct:.1}%"),
                    baseline_val: Some(baseline.density),
                    current_val: Some(current.density),
                    delta: Some(current.density - baseline.density),
                    ..Default::default()
                });
            }
        }
    }

    // Node count change (info at threshold).
    if let Some(pct) = pct_change(baseline.node_count as f64, current.node_count as f64) {
        if pct.abs() >= cfg.node_growth_info_pct {
            r.push(Alert {
                alert_type: AlertType::NodeCountChange,
                severity: Severity::Info,
                message: format!(
                    "Node count changed from {} to {} ({pct:+.1}%)",
                    baseline.node_count, current.node_count
                ),
                baseline_val: Some(baseline.node_count as f64),
                current_val: Some(current.node_count as f64),
                delta: Some((current.node_count as i64 - baseline.node_count as i64) as f64),
                ..Default::default()
            });
        }
    }

    // Edge count change (info at threshold).
    if let Some(pct) = pct_change(baseline.edge_count as f64, current.edge_count as f64) {
        if pct.abs() >= cfg.edge_growth_info_pct {
            r.push(Alert {
                alert_type: AlertType::EdgeCountChange,
                severity: Severity::Info,
                message: format!(
                    "Edge count changed from {} to {} ({pct:+.1}%)",
                    baseline.edge_count, current.edge_count
                ),
                baseline_val: Some(baseline.edge_count as f64),
                current_val: Some(current.edge_count as f64),
                delta: Some((current.edge_count as i64 - baseline.edge_count as i64) as f64),
                ..Default::default()
            });
        }
    }

    // Blocked increase (warning at +N).
    let blocked_delta = current.blocked as i64 - baseline.blocked as i64;
    if blocked_delta >= cfg.blocked_increase_threshold {
        r.push(Alert {
            alert_type: AlertType::BlockedIncrease,
            severity: Severity::Warning,
            message: format!(
                "Blocked issues increased from {} to {} (+{blocked_delta})",
                baseline.blocked, current.blocked
            ),
            baseline_val: Some(baseline.blocked as f64),
            current_val: Some(current.blocked as f64),
            delta: Some(blocked_delta as f64),
            ..Default::default()
        });
    }

    // Actionable decrease (warning at -N%) / increase (info at +N%).
    if baseline.actionable > 0 {
        if let Some(pct) = pct_change(baseline.actionable as f64, current.actionable as f64) {
            if pct <= -cfg.actionable_decrease_warning_pct {
                r.push(Alert {
                    alert_type: AlertType::ActionableChange,
                    severity: Severity::Warning,
                    message: format!(
                        "Actionable issues decreased from {} to {} ({pct:.1}%)",
                        baseline.actionable, current.actionable
                    ),
                    baseline_val: Some(baseline.actionable as f64),
                    current_val: Some(current.actionable as f64),
                    delta: Some((current.actionable as i64 - baseline.actionable as i64) as f64),
                    ..Default::default()
                });
            } else if pct >= cfg.actionable_increase_info_pct {
                r.push(Alert {
                    alert_type: AlertType::ActionableChange,
                    severity: Severity::Info,
                    message: format!(
                        "Actionable issues increased from {} to {} (+{pct:.1}%)",
                        baseline.actionable, current.actionable
                    ),
                    baseline_val: Some(baseline.actionable as f64),
                    current_val: Some(current.actionable as f64),
                    delta: Some((current.actionable as i64 - baseline.actionable as i64) as f64),
                    ..Default::default()
                });
            }
        }
    }

    // PageRank shifts on shared issues (warning at threshold).
    for (id, bl_pr) in &baseline.pagerank {
        if let Some(cur_pr) = current.pagerank.get(id) {
            if let Some(pct) = pct_change(*bl_pr, *cur_pr) {
                if pct.abs() >= cfg.pagerank_change_warning_pct {
                    r.push(Alert {
                        alert_type: AlertType::PagerankChange,
                        severity: Severity::Warning,
                        message: format!("PageRank of {id} changed by {pct:+.1}%"),
                        baseline_val: Some(*bl_pr),
                        current_val: Some(*cur_pr),
                        delta: Some(cur_pr - bl_pr),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // Staleness: check open/in_progress issues against per-label thresholds.
    check_staleness(&mut r, cfg, issues);

    // Blocking cascade: BFS downstream through blocked issues.
    check_blocking_cascade(&mut r, cfg, issues);

    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        nodes: usize,
        edges: usize,
        density: f64,
        blocked: usize,
        actionable: usize,
    ) -> BaselineStats {
        BaselineStats {
            node_count: nodes,
            edge_count: edges,
            density,
            open: nodes,
            closed: 0,
            blocked,
            cycle_count: 0,
            actionable,
            pagerank: BTreeMap::new(),
        }
    }

    #[test]
    fn no_drift_on_identical_snapshots() {
        let s = snap(100, 120, 0.05, 3, 40);
        let r = calculate(&s, &s, &DriftConfig::default(), &[], &[]);
        assert!(!r.has_drift);
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn new_cycle_is_critical_exit_one() {
        let s = snap(10, 10, 0.1, 0, 5);
        let r = calculate(
            &s,
            &s,
            &DriftConfig::default(),
            &[vec!["A".into(), "B".into(), "A".into()]],
            &[],
        );
        assert!(r.has_drift);
        assert_eq!(r.critical_count, 1);
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn density_growth_warning_at_fifty_pct() {
        let base = snap(100, 120, 0.04, 0, 40);
        let cur = snap(100, 130, 0.064, 0, 40); // +60%
        let r = calculate(&base, &cur, &DriftConfig::default(), &[], &[]);
        assert!(r
            .alerts
            .iter()
            .any(|a| a.alert_type == AlertType::DensityGrowth && a.severity == Severity::Warning));
        assert_eq!(r.exit_code(), 2);
    }

    #[test]
    fn density_growth_info_at_twenty_pct() {
        let base = snap(100, 120, 0.05, 0, 40);
        let cur = snap(100, 125, 0.0625, 0, 40); // +25% -> info band
        let r = calculate(&base, &cur, &DriftConfig::default(), &[], &[]);
        assert!(r
            .alerts
            .iter()
            .any(|a| a.alert_type == AlertType::DensityGrowth && a.severity == Severity::Info));
        assert_eq!(r.exit_code(), 0); // info doesn't set exit code
    }

    #[test]
    fn blocked_increase_warning_at_plus_five() {
        let base = snap(100, 120, 0.05, 2, 40);
        let cur = snap(100, 120, 0.05, 8, 34); // +6 blocked; actionable -15% (<30 no warn)
        let r = calculate(&base, &cur, &DriftConfig::default(), &[], &[]);
        assert!(r
            .alerts
            .iter()
            .any(|a| a.alert_type == AlertType::BlockedIncrease));
        assert_eq!(r.exit_code(), 2);
    }

    #[test]
    fn actionable_drop_warning_at_minus_thirty_pct() {
        let base = snap(100, 120, 0.05, 0, 40);
        let cur = snap(100, 120, 0.05, 0, 25); // -37.5%
        let r = calculate(&base, &cur, &DriftConfig::default(), &[], &[]);
        assert!(r.alerts.iter().any(
            |a| a.alert_type == AlertType::ActionableChange && a.severity == Severity::Warning
        ));
    }

    #[test]
    fn pagerank_shift_warning() {
        let mut base = snap(10, 10, 0.1, 0, 5);
        base.pagerank.insert("X-1".into(), 0.10);
        let mut cur = snap(10, 10, 0.1, 0, 5);
        cur.pagerank.insert("X-1".into(), 0.20); // +100%
        let r = calculate(&base, &cur, &DriftConfig::default(), &[], &[]);
        assert!(r
            .alerts
            .iter()
            .any(|a| a.alert_type == AlertType::PagerankChange));
    }

    // -- StaleIssue tests ---------------------------------------------------

    fn stale_issue(id: &str, status: bv_core::model::Status, updated: &str) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: format!("issue {id}"),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status,
            priority: 2,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: Some("2026-01-01T00:00:00Z".into()),
            updated_at: Some(updated.into()),
            due_date: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: vec![],
            dependencies: vec![],
            comments: vec![],
            source_repo: String::new(),
        }
    }

    #[test]
    fn stale_open_issue_triggers_warning_after_fourteen_days() {
        let s = snap(10, 10, 0.1, 0, 5);
        // 20 days ago: exceeds 14-day warning but not 30-day critical.
        let issue = stale_issue("X-1", bv_core::model::Status::Open, "2026-08-16T00:00:00Z");
        let r = calculate(&s, &s, &DriftConfig::default(), &[], &[issue]);
        assert!(
            r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::StaleIssue && a.severity == Severity::Warning),
            "expected stale warning for 20-day inactive issue"
        );
    }

    #[test]
    fn stale_open_issue_triggers_critical_after_thirty_days() {
        let s = snap(10, 10, 0.1, 0, 5);
        // 40 days ago: exceeds 30-day critical.
        let issue = stale_issue("X-2", bv_core::model::Status::Open, "2026-07-27T00:00:00Z");
        let r = calculate(&s, &s, &DriftConfig::default(), &[], &[issue]);
        assert!(
            r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::StaleIssue && a.severity == Severity::Critical),
            "expected stale critical for 40-day inactive issue"
        );
    }

    #[test]
    fn in_progress_stale_uses_halved_thresholds() {
        let s = snap(10, 10, 0.1, 0, 5);
        // 8 days ago: below 14-day warning, but in_progress multiplier 0.5
        // makes effective warn = 7 days -> should trigger.
        let issue = stale_issue(
            "X-3",
            bv_core::model::Status::InProgress,
            "2026-08-28T00:00:00Z",
        );
        let r = calculate(&s, &s, &DriftConfig::default(), &[], &[issue]);
        assert!(
            r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::StaleIssue),
            "expected stale alert for in_progress issue with halved thresholds"
        );
    }

    #[test]
    fn stale_issue_skipped_when_disabled() {
        let s = snap(10, 10, 0.1, 0, 5);
        let cfg = DriftConfig {
            disabled_alerts: vec!["stale_issue".into()],
            ..Default::default()
        };
        let issue = stale_issue("X-4", bv_core::model::Status::Open, "2026-07-01T00:00:00Z");
        let r = calculate(&s, &s, &cfg, &[], &[issue]);
        assert!(
            !r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::StaleIssue),
            "stale_issue disabled -- no alert expected"
        );
    }

    #[test]
    fn closed_issue_not_flagged_stale() {
        let s = snap(10, 10, 0.1, 0, 5);
        let issue = stale_issue(
            "X-5",
            bv_core::model::Status::Closed,
            "2026-07-01T00:00:00Z",
        );
        let r = calculate(&s, &s, &DriftConfig::default(), &[], &[issue]);
        assert!(
            !r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::StaleIssue),
            "closed issues must not trigger stale alerts"
        );
    }

    // -- BlockingCascade tests ----------------------------------------------

    fn casc_issue(id: &str, status: bv_core::model::Status, priority: i32) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: format!("issue {id}"),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status,
            priority,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: None,
            updated_at: None,
            due_date: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: vec![],
            dependencies: vec![],
            comments: vec![],
            source_repo: String::new(),
        }
    }

    fn casc_dep(from: &str, to: &str) -> bv_core::model::Dependency {
        bv_core::model::Dependency {
            issue_id: from.to_string(),
            depends_on_id: to.to_string(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: bv_core::model::DependencyType::Blocks,
            created_at: None,
            created_by: String::new(),
        }
    }

    #[test]
    fn blocking_cascade_info_at_three_unblocks() {
        let s = snap(10, 10, 0.1, 0, 5);
        // Blocker X-1 is depended on by X-2, X-3, X-4 (3 dependents).
        let mut dependent2 = casc_issue("X-2", bv_core::model::Status::Open, 2);
        dependent2.dependencies.push(casc_dep("X-2", "X-1"));
        let mut dependent3 = casc_issue("X-3", bv_core::model::Status::Open, 3);
        dependent3.dependencies.push(casc_dep("X-3", "X-1"));
        let mut dependent4 = casc_issue("X-4", bv_core::model::Status::Open, 4);
        dependent4.dependencies.push(casc_dep("X-4", "X-1"));
        let blocker = casc_issue("X-1", bv_core::model::Status::Open, 1);
        let issues = vec![blocker, dependent2, dependent3, dependent4];

        let r = calculate(&s, &s, &DriftConfig::default(), &[], &issues);
        let cascade_alerts: Vec<_> = r
            .alerts
            .iter()
            .filter(|a| a.alert_type == AlertType::BlockingCascade)
            .collect();
        assert_eq!(
            cascade_alerts.len(),
            1,
            "expected one cascade alert for X-1"
        );
        assert_eq!(cascade_alerts[0].severity, Severity::Info);
        assert_eq!(cascade_alerts[0].unblocks_count, Some(3));
        assert_eq!(cascade_alerts[0].issue_id, "X-1");
    }

    #[test]
    fn blocking_cascade_warning_at_five_unblocks() {
        let s = snap(10, 10, 0.1, 0, 5);
        let blocker = casc_issue("X-1", bv_core::model::Status::Open, 1);
        let mut issues = vec![blocker];
        for i in 2..=6 {
            let mut dep = casc_issue(&format!("X-{i}"), bv_core::model::Status::Open, 2);
            dep.dependencies.push(casc_dep(&format!("X-{i}"), "X-1"));
            issues.push(dep);
        }
        let r = calculate(&s, &s, &DriftConfig::default(), &[], &issues);
        let cascade = r
            .alerts
            .iter()
            .find(|a| a.alert_type == AlertType::BlockingCascade);
        assert!(cascade.is_some(), "expected cascade alert");
        assert_eq!(cascade.unwrap().severity, Severity::Warning);
        assert_eq!(cascade.unwrap().unblocks_count, Some(5));
    }

    #[test]
    fn blocking_cascade_skipped_when_disabled() {
        let s = snap(10, 10, 0.1, 0, 5);
        let cfg = DriftConfig {
            disabled_alerts: vec!["blocking_cascade".into()],
            ..Default::default()
        };
        let blocker = casc_issue("X-1", bv_core::model::Status::Open, 1);
        let mut dep = casc_issue("X-2", bv_core::model::Status::Open, 2);
        dep.dependencies.push(casc_dep("X-2", "X-1"));
        let mut dep2 = casc_issue("X-3", bv_core::model::Status::Open, 3);
        dep2.dependencies.push(casc_dep("X-3", "X-1"));
        let mut dep3 = casc_issue("X-4", bv_core::model::Status::Open, 4);
        dep3.dependencies.push(casc_dep("X-4", "X-1"));
        let issues = vec![blocker, dep, dep2, dep3];

        let r = calculate(&s, &s, &cfg, &[], &issues);
        assert!(
            !r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::BlockingCascade),
            "blocking_cascade disabled -- no alert expected"
        );
    }

    #[test]
    fn closed_dependent_not_counted_in_cascade() {
        let s = snap(10, 10, 0.1, 0, 5);
        let blocker = casc_issue("X-1", bv_core::model::Status::Open, 1);
        let mut dep_open = casc_issue("X-2", bv_core::model::Status::Open, 2);
        dep_open.dependencies.push(casc_dep("X-2", "X-1"));
        let mut dep_closed = casc_issue("X-3", bv_core::model::Status::Closed, 3);
        dep_closed.dependencies.push(casc_dep("X-3", "X-1"));
        let issues = vec![blocker, dep_open, dep_closed];

        let r = calculate(&s, &s, &DriftConfig::default(), &[], &issues);
        // Only X-2 is open, so unblocks = 1, below info threshold (3).
        assert!(
            !r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::BlockingCascade),
            "closed dependents must not be counted"
        );
    }

    #[test]
    fn already_blocked_dependent_not_counted_in_cascade() {
        let s = snap(10, 10, 0.1, 0, 5);
        // X-1 and X-BOTH are blockers; X-2 depends on both.
        let blocker1 = casc_issue("X-1", bv_core::model::Status::Open, 1);
        let blocker2 = casc_issue("X-BOTH", bv_core::model::Status::Open, 1);
        let mut dep = casc_issue("X-2", bv_core::model::Status::Open, 2);
        dep.dependencies.push(casc_dep("X-2", "X-1"));
        dep.dependencies.push(casc_dep("X-2", "X-BOTH"));
        let issues = vec![blocker1, blocker2, dep];

        let r = calculate(&s, &s, &DriftConfig::default(), &[], &issues);
        // Completing X-1 still leaves X-2 blocked by X-BOTH, so unblocks = 0.
        assert!(
            !r.alerts
                .iter()
                .any(|a| a.alert_type == AlertType::BlockingCascade && a.issue_id == "X-1"),
            "X-2 is still blocked by X-BOTH -- X-1 must not appear"
        );
    }
}
