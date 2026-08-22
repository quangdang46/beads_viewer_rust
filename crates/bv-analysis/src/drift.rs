//! Drift detection — port of Go `pkg/drift` (Calculator + Result + exit
//! codes) and `pkg/baseline` snapshot format v1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertType {
    NewCycle,
    DensityGrowth,
    NodeCountChange,
    EdgeCountChange,
    BlockedIncrease,
    ActionableChange,
    PagerankChange,
}

#[derive(Debug, Clone, Serialize)]
pub struct Alert {
    #[serde(rename = "type")]
    pub alert_type: AlertType,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_val: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_val: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<f64>,
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
        }
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

/// Run all drift checks between two snapshots.
pub fn calculate(
    baseline: &BaselineStats,
    current: &BaselineStats,
    cfg: &DriftConfig,
    new_cycles: &[Vec<String>],
) -> DriftResult {
    let mut r = DriftResult::default();

    // Cycles: any NEW cycle is critical.
    if !new_cycles.is_empty() {
        let names: Vec<String> = new_cycles.iter().map(|c| c.join(" -> ")).collect();
        r.push(Alert {
            alert_type: AlertType::NewCycle,
            severity: Severity::Critical,
            message: format!("New dependency cycles introduced: {}", names.join("; ")),
            baseline_val: None,
            current_val: None,
            delta: None,
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
                    });
                }
            }
        }
    }

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
        let r = calculate(&s, &s, &DriftConfig::default(), &[]);
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
        );
        assert!(r.has_drift);
        assert_eq!(r.critical_count, 1);
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn density_growth_warning_at_fifty_pct() {
        let base = snap(100, 120, 0.04, 0, 40);
        let cur = snap(100, 130, 0.064, 0, 40); // +60%
        let r = calculate(&base, &cur, &DriftConfig::default(), &[]);
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
        let r = calculate(&base, &cur, &DriftConfig::default(), &[]);
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
        let r = calculate(&base, &cur, &DriftConfig::default(), &[]);
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
        let r = calculate(&base, &cur, &DriftConfig::default(), &[]);
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
        let r = calculate(&base, &cur, &DriftConfig::default(), &[]);
        assert!(r
            .alerts
            .iter()
            .any(|a| a.alert_type == AlertType::PagerankChange));
    }
}
