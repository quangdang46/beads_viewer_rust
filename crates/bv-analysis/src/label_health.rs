//! Label health / cross-label flow / attention scoring — port of Go
//! `pkg/analysis/label_health.go` (subset backing `robot-label-health`,
//! `robot-label-flow`, `robot-label-attention`).
//!
//! Deliberate scope cut vs Go: the deep blockage-cascade tree
//! (`ComputeBlockageCascade`), per-label subgraph PageRank/critical-path
//! (`ComputeLabelSubgraph`/`ComputeLabelPageRank`/`ComputeLabelCriticalPath`),
//! and multi-week historical velocity trends are not ported here — those
//! back other, still-undispatched commands. Where `computeLabelAttention`
//! needs a per-label PageRank sum, we sum the already-computed *global*
//! PageRank over the label's issues rather than re-running PageRank on an
//! extracted subgraph; this is a documented approximation, not a silent
//! stub — attention ranking still reflects real graph centrality.

use bv_core::model::{Issue, Status};
use serde::Serialize;
use std::collections::BTreeMap;

fn is_closed_like(s: Status) -> bool {
    matches!(s, Status::Closed | Status::Tombstone)
}

fn has_label(issue: &Issue, label: &str) -> bool {
    issue.labels.iter().any(|l| l == label)
}

fn parse_ts(raw: &Option<String>) -> Option<jiff::Timestamp> {
    raw.as_deref()
        .and_then(|s| s.parse::<jiff::Timestamp>().ok())
}

fn clamp_score(v: i64) -> i64 {
    v.clamp(0, 100)
}

// ---------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------

pub const DEFAULT_STALE_THRESHOLD_DAYS: i64 = 14;
pub const HEALTHY_THRESHOLD: i64 = 70;
pub const WARNING_THRESHOLD: i64 = 40;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelHealthConfig {
    pub stale_threshold_days: i64,
    pub velocity_weight: f64,
    pub freshness_weight: f64,
    pub flow_weight: f64,
    pub criticality_weight: f64,
    pub min_issues_for_health: i64,
    pub include_closed_in_flow: bool,
}

impl Default for LabelHealthConfig {
    fn default() -> Self {
        LabelHealthConfig {
            stale_threshold_days: DEFAULT_STALE_THRESHOLD_DAYS,
            velocity_weight: 0.25,
            freshness_weight: 0.25,
            flow_weight: 0.25,
            criticality_weight: 0.25,
            min_issues_for_health: 1,
            include_closed_in_flow: false,
        }
    }
}

pub fn health_level_from_score(score: i64) -> &'static str {
    if score >= HEALTHY_THRESHOLD {
        "healthy"
    } else if score >= WARNING_THRESHOLD {
        "warning"
    } else {
        "critical"
    }
}

fn composite_health(
    velocity: i64,
    freshness: i64,
    flow: i64,
    criticality: i64,
    cfg: &LabelHealthConfig,
) -> i64 {
    let weighted = velocity as f64 * cfg.velocity_weight
        + freshness as f64 * cfg.freshness_weight
        + flow as f64 * cfg.flow_weight
        + criticality as f64 * cfg.criticality_weight;
    clamp_score((weighted + 0.5) as i64)
}

// ---------------------------------------------------------------------
// Label extraction
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct LabelExtractionResult {
    pub labels: Vec<String>,
    pub label_count: usize,
    pub issue_count: usize,
    pub unlabeled_count: usize,
    pub top_labels: Vec<String>,
}

pub fn extract_labels(issues: &[Issue]) -> LabelExtractionResult {
    let mut result = LabelExtractionResult {
        issue_count: issues.len(),
        ..Default::default()
    };
    if issues.is_empty() {
        return result;
    }
    let mut set = std::collections::BTreeSet::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for issue in issues {
        if issue.labels.is_empty() {
            result.unlabeled_count += 1;
        }
        for label in &issue.labels {
            if label.is_empty() {
                continue;
            }
            set.insert(label.clone());
            *counts.entry(label.clone()).or_insert(0) += 1;
        }
    }
    result.labels = set.into_iter().collect();
    result.label_count = result.labels.len();
    let mut top: Vec<(String, usize)> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    result.top_labels = top.into_iter().map(|(l, _)| l).collect();
    result
}

// ---------------------------------------------------------------------
// Velocity / freshness
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct VelocityMetrics {
    pub closed_last_7_days: i64,
    pub closed_last_30_days: i64,
    pub avg_days_to_close: f64,
    pub trend_direction: String,
    pub trend_percent: f64,
    pub velocity_score: i64,
}

pub fn compute_velocity_metrics(issues: &[Issue], now: jiff::Timestamp) -> VelocityMetrics {
    let day_secs = 86400.0;
    let week_ago = now - jiff::SignedDuration::from_secs(7 * 86400);
    let month_ago = now - jiff::SignedDuration::from_secs(30 * 86400);
    let prev_week_start = now - jiff::SignedDuration::from_secs(14 * 86400);

    let (mut closed7, mut closed30, mut prev_week, mut current_week) = (0i64, 0i64, 0i64, 0i64);
    let mut total_close_days = 0.0;
    let mut close_samples = 0i64;

    for iss in issues {
        if !is_closed_like(iss.status) {
            continue;
        }
        let Some(closed_at) = parse_ts(&iss.closed_at) else {
            continue;
        };
        if closed_at > week_ago {
            closed7 += 1;
        }
        if closed_at > month_ago {
            closed30 += 1;
        }
        if closed_at > prev_week_start && closed_at < week_ago {
            prev_week += 1;
        } else if closed_at > week_ago {
            current_week += 1;
        }
        if let Some(created_at) = parse_ts(&iss.created_at) {
            let secs = (closed_at - created_at)
                .total(jiff::Unit::Second)
                .unwrap_or(0.0);
            total_close_days += secs / day_secs;
            close_samples += 1;
        }
    }

    let avg_days = if close_samples > 0 {
        total_close_days / close_samples as f64
    } else {
        0.0
    };

    let (mut trend_dir, mut trend_percent) = ("stable".to_string(), 0.0);
    if prev_week > 0 {
        trend_percent = ((current_week - prev_week) as f64 / prev_week as f64) * 100.0;
        if trend_percent > 10.0 {
            trend_dir = "improving".into();
        } else if trend_percent < -10.0 {
            trend_dir = "declining".into();
        }
    } else if current_week > 0 {
        trend_dir = "improving".into();
        trend_percent = 100.0;
    }

    let mut velocity_score = if closed30 > 0 {
        (closed30 as f64 * 10.0).min(100.0) as i64
    } else {
        0
    };
    if trend_dir == "improving" && velocity_score < 100 {
        velocity_score = clamp_score(velocity_score + 10);
    }

    VelocityMetrics {
        closed_last_7_days: closed7,
        closed_last_30_days: closed30,
        avg_days_to_close: avg_days,
        trend_direction: trend_dir,
        trend_percent,
        velocity_score,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FreshnessMetrics {
    pub most_recent_update: Option<String>,
    pub oldest_open_issue: Option<String>,
    pub avg_days_since_update: f64,
    pub stale_count: i64,
    pub stale_threshold_days: i64,
    pub freshness_score: i64,
}

pub fn compute_freshness_metrics(
    issues: &[Issue],
    now: jiff::Timestamp,
    stale_days: i64,
) -> FreshnessMetrics {
    let stale_days = if stale_days <= 0 {
        DEFAULT_STALE_THRESHOLD_DAYS
    } else {
        stale_days
    };
    let mut most_recent: Option<jiff::Timestamp> = None;
    let mut most_recent_raw: Option<String> = None;
    let mut oldest_open: Option<jiff::Timestamp> = None;
    let mut oldest_open_raw: Option<String> = None;
    let mut total_staleness = 0.0;
    let mut count = 0i64;
    let mut stale_count = 0i64;
    let threshold = stale_days as f64;

    for iss in issues {
        if let Some(updated) = parse_ts(&iss.updated_at) {
            if most_recent.is_none_or(|m| updated > m) {
                most_recent = Some(updated);
                most_recent_raw = iss.updated_at.clone();
            }
            let days = (now - updated).total(jiff::Unit::Second).unwrap_or(0.0) / 86400.0;
            total_staleness += days;
            count += 1;
            if days >= threshold {
                stale_count += 1;
            }
        }
        if !is_closed_like(iss.status) {
            if let Some(created) = parse_ts(&iss.created_at) {
                if oldest_open.is_none_or(|o| created < o) {
                    oldest_open = Some(created);
                    oldest_open_raw = iss.created_at.clone();
                }
            }
        }
    }

    let avg_staleness = if count > 0 {
        total_staleness / count as f64
    } else {
        0.0
    };
    let freshness_score = (100.0 - (avg_staleness / (threshold * 2.0)) * 100.0).max(0.0) as i64;

    FreshnessMetrics {
        most_recent_update: most_recent_raw,
        oldest_open_issue: oldest_open_raw,
        avg_days_since_update: avg_staleness,
        stale_count,
        stale_threshold_days: stale_days,
        freshness_score: clamp_score(freshness_score),
    }
}

// ---------------------------------------------------------------------
// Cross-label flow
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BlockingPair {
    pub blocker_id: String,
    pub blocked_id: String,
    pub blocker_label: String,
    pub blocked_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelDependency {
    pub from_label: String,
    pub to_label: String,
    pub issue_count: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issue_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocking_pairs: Vec<BlockingPair>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CrossLabelFlow {
    pub labels: Vec<String>,
    pub flow_matrix: Vec<Vec<i64>>,
    pub dependencies: Vec<LabelDependency>,
    pub critical_paths: Vec<serde_json::Value>,
    pub bottleneck_labels: Vec<String>,
    pub total_cross_label_deps: i64,
}

pub fn compute_cross_label_flow(issues: &[Issue], cfg: &LabelHealthConfig) -> CrossLabelFlow {
    let extraction = extract_labels(issues);
    let label_list = extraction.labels;
    let n = label_list.len();
    let index: BTreeMap<&str, usize> = label_list
        .iter()
        .enumerate()
        .map(|(i, l)| (l.as_str(), i))
        .collect();
    let mut matrix = vec![vec![0i64; n]; n];

    let issue_map: std::collections::HashMap<&str, &Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();

    let mut dep_map: BTreeMap<(String, String), LabelDependency> = BTreeMap::new();
    let mut total_deps = 0i64;

    for blocked in issues {
        if !cfg.include_closed_in_flow && is_closed_like(blocked.status) {
            continue;
        }
        for dep in &blocked.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let Some(blocker) = issue_map.get(dep.depends_on_id.as_str()) else {
                continue;
            };
            if !cfg.include_closed_in_flow && is_closed_like(blocker.status) {
                continue;
            }
            for from in &blocker.labels {
                for to in &blocked.labels {
                    if from.is_empty() || to.is_empty() || from == to {
                        continue;
                    }
                    let (Some(&i_from), Some(&i_to)) =
                        (index.get(from.as_str()), index.get(to.as_str()))
                    else {
                        continue;
                    };
                    matrix[i_from][i_to] += 1;
                    total_deps += 1;
                    let key = (from.clone(), to.clone());
                    let entry = dep_map.entry(key).or_insert_with(|| LabelDependency {
                        from_label: from.clone(),
                        to_label: to.clone(),
                        issue_count: 0,
                        issue_ids: Vec::new(),
                        blocking_pairs: Vec::new(),
                    });
                    entry.issue_count += 1;
                    entry.issue_ids.push(blocked.id.clone());
                    entry.blocking_pairs.push(BlockingPair {
                        blocker_id: blocker.id.clone(),
                        blocked_id: blocked.id.clone(),
                        blocker_label: from.clone(),
                        blocked_label: to.clone(),
                    });
                }
            }
        }
    }

    let mut deps: Vec<LabelDependency> = dep_map.into_values().collect();
    deps.sort_by(|a, b| {
        a.from_label
            .cmp(&b.from_label)
            .then_with(|| a.to_label.cmp(&b.to_label))
            .then_with(|| b.issue_count.cmp(&a.issue_count))
    });

    let mut out_counts: BTreeMap<&str, i64> = BTreeMap::new();
    let mut max_out = 0i64;
    for (i, row) in matrix.iter().enumerate() {
        let sum: i64 = row.iter().sum();
        out_counts.insert(label_list[i].as_str(), sum);
        if sum > max_out {
            max_out = sum;
        }
    }
    let mut bottlenecks: Vec<String> = out_counts
        .iter()
        .filter(|(_, &c)| c == max_out && c > 0)
        .map(|(l, _)| l.to_string())
        .collect();
    bottlenecks.sort();

    CrossLabelFlow {
        labels: label_list,
        flow_matrix: matrix,
        dependencies: deps,
        critical_paths: Vec::new(),
        bottleneck_labels: bottlenecks,
        total_cross_label_deps: total_deps,
    }
}

// ---------------------------------------------------------------------
// Per-label health
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FlowMetrics {
    pub incoming_deps: i64,
    pub outgoing_deps: i64,
    pub incoming_labels: Vec<String>,
    pub outgoing_labels: Vec<String>,
    pub blocked_by_external: i64,
    pub blocking_external: i64,
    pub flow_score: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CriticalityMetrics {
    pub avg_pagerank: f64,
    pub avg_betweenness: f64,
    pub max_betweenness: f64,
    pub critical_path_count: i64,
    pub bottleneck_count: i64,
    pub criticality_score: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelHealth {
    pub label: String,
    pub issue_count: i64,
    pub open_count: i64,
    pub closed_count: i64,
    #[serde(rename = "blocked_count")]
    pub blocked: i64,
    pub health: i64,
    pub health_level: &'static str,
    pub velocity: VelocityMetrics,
    pub freshness: FreshnessMetrics,
    pub flow: FlowMetrics,
    pub criticality: CriticalityMetrics,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

/// Precomputed graph centrality, shared across all labels for efficiency
/// (Go: `*GraphStats` passed into `ComputeAllLabelHealth`/`ComputeLabelHealthForLabel`).
pub struct GraphStats {
    pub pagerank: BTreeMap<String, f64>,
    pub betweenness: BTreeMap<String, f64>,
    pub critical_path: BTreeMap<String, f64>,
}

pub fn compute_graph_stats(issues: &[Issue]) -> GraphStats {
    let g = crate::build_graph(issues);
    let pr = bv_graph_core::algorithms::pagerank::pagerank_default(&g);
    let bw = bv_graph_core::algorithms::betweenness::betweenness(&g);
    let cp = bv_graph_core::algorithms::critical_path::critical_path_heights(&g);
    let mut pagerank = BTreeMap::new();
    let mut betweenness = BTreeMap::new();
    let mut critical_path = BTreeMap::new();
    for i in 0..g.len() {
        let id = g.node_id(i).unwrap_or_default().to_string();
        pagerank.insert(id.clone(), pr.get(i).copied().unwrap_or(0.0));
        betweenness.insert(id.clone(), bw.get(i).copied().unwrap_or(0.0));
        critical_path.insert(id, cp.get(i).copied().unwrap_or(0.0));
    }
    GraphStats {
        pagerank,
        betweenness,
        critical_path,
    }
}

fn labels_for_issue<'a>(issues: &'a [Issue], id: &str) -> Vec<&'a str> {
    issues
        .iter()
        .find(|i| i.id == id)
        .map(|i| i.labels.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default()
}

pub fn compute_label_health_for_label(
    label: &str,
    issues: &[Issue],
    cfg: &LabelHealthConfig,
    now: jiff::Timestamp,
    stats: &GraphStats,
) -> LabelHealth {
    let labeled: Vec<&Issue> = issues.iter().filter(|i| has_label(i, label)).collect();
    let issue_ids: Vec<String> = labeled.iter().map(|i| i.id.clone()).collect();
    let issue_count = labeled.len() as i64;

    if issue_count == 0 {
        return LabelHealth {
            label: label.to_string(),
            issue_count: 0,
            open_count: 0,
            closed_count: 0,
            blocked: 0,
            health: 0,
            health_level: "critical",
            velocity: compute_velocity_metrics(&[], now),
            freshness: compute_freshness_metrics(&[], now, cfg.stale_threshold_days),
            flow: FlowMetrics {
                incoming_deps: 0,
                outgoing_deps: 0,
                incoming_labels: vec![],
                outgoing_labels: vec![],
                blocked_by_external: 0,
                blocking_external: 0,
                flow_score: 100,
            },
            criticality: CriticalityMetrics {
                avg_pagerank: 0.0,
                avg_betweenness: 0.0,
                max_betweenness: 0.0,
                critical_path_count: 0,
                bottleneck_count: 0,
                criticality_score: 50,
            },
            issues: vec![],
        };
    }

    let labeled_owned: Vec<Issue> = labeled.iter().map(|i| (*i).clone()).collect();
    let (mut open_count, mut closed_count, mut blocked) = (0i64, 0i64, 0i64);
    for iss in &labeled {
        match iss.status {
            Status::Closed | Status::Tombstone => closed_count += 1,
            Status::Blocked => blocked += 1,
            _ => open_count += 1,
        }
    }

    let velocity = compute_velocity_metrics(&labeled_owned, now);
    let freshness = compute_freshness_metrics(&labeled_owned, now, cfg.stale_threshold_days);

    let labeled_set: std::collections::HashSet<&str> =
        labeled.iter().map(|i| i.id.as_str()).collect();
    let mut seen_in = std::collections::BTreeSet::new();
    let mut seen_out = std::collections::BTreeSet::new();
    let (mut incoming_deps, mut outgoing_deps) = (0i64, 0i64);
    let (mut blocked_by_external, mut blocking_external) = (0i64, 0i64);

    for iss in &labeled {
        let mut has_external_blocker = false;
        for dep in &iss.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            for bl in labels_for_issue(issues, &dep.depends_on_id) {
                if bl != label {
                    incoming_deps += 1;
                    seen_in.insert(bl.to_string());
                    has_external_blocker = true;
                }
            }
        }
        if has_external_blocker {
            blocked_by_external += 1;
        }
    }
    for other in issues {
        if labeled_set.contains(other.id.as_str()) {
            continue;
        }
        let mut counted = false;
        for dep in &other.dependencies {
            if !dep.r#type.is_blocking() || !labeled_set.contains(dep.depends_on_id.as_str()) {
                continue;
            }
            for ol in &other.labels {
                if ol != label {
                    outgoing_deps += 1;
                    seen_out.insert(ol.clone());
                }
            }
            if !counted {
                blocking_external += 1;
                counted = true;
            }
        }
    }
    let flow = FlowMetrics {
        incoming_deps,
        outgoing_deps,
        incoming_labels: seen_in.into_iter().collect(),
        outgoing_labels: seen_out.into_iter().collect(),
        blocked_by_external,
        blocking_external,
        flow_score: clamp_score(100 - incoming_deps * 5),
    };

    let max_pr = stats.pagerank.values().copied().fold(0.0, f64::max);
    let max_bw = stats.betweenness.values().copied().fold(0.0, f64::max);
    let (mut pr_sum, mut bw_sum, mut max_bw_label) = (0.0, 0.0, 0.0);
    let (mut crit_count, mut bottleneck_count) = (0i64, 0i64);
    for iss in &labeled {
        let pr = stats.pagerank.get(&iss.id).copied().unwrap_or(0.0);
        let bw = stats.betweenness.get(&iss.id).copied().unwrap_or(0.0);
        pr_sum += pr;
        bw_sum += bw;
        if bw > max_bw_label {
            max_bw_label = bw;
        }
        if stats.critical_path.get(&iss.id).copied().unwrap_or(0.0) > 0.0 {
            crit_count += 1;
        }
        if bw > 0.0 {
            bottleneck_count += 1;
        }
    }
    let avg_pr = pr_sum / issue_count as f64;
    let avg_bw = bw_sum / issue_count as f64;
    let mut crit_score = 0i64;
    if max_pr > 0.0 {
        crit_score += ((avg_pr / max_pr) * 50.0) as i64;
    }
    if max_bw > 0.0 {
        crit_score += ((max_bw_label / max_bw) * 50.0) as i64;
    }
    let criticality = CriticalityMetrics {
        avg_pagerank: avg_pr,
        avg_betweenness: avg_bw,
        max_betweenness: max_bw_label,
        critical_path_count: crit_count,
        bottleneck_count,
        criticality_score: clamp_score(crit_score),
    };

    let health = composite_health(
        velocity.velocity_score,
        freshness.freshness_score,
        flow.flow_score,
        criticality.criticality_score,
        cfg,
    );

    LabelHealth {
        label: label.to_string(),
        issue_count,
        open_count,
        closed_count,
        blocked,
        health,
        health_level: health_level_from_score(health),
        velocity,
        freshness,
        flow,
        criticality,
        issues: issue_ids,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelSummary {
    pub label: String,
    pub issue_count: i64,
    pub open_count: i64,
    pub health: i64,
    pub health_level: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_issue: Option<String>,
    pub needs_attention: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelAnalysisResult {
    pub generated_at: String,
    pub total_labels: i64,
    pub healthy_count: i64,
    pub warning_count: i64,
    pub critical_count: i64,
    pub labels: Vec<LabelHealth>,
    pub summaries: Vec<LabelSummary>,
    pub attention_needed: Vec<String>,
}

pub fn compute_all_label_health(
    issues: &[Issue],
    cfg: &LabelHealthConfig,
    now: jiff::Timestamp,
) -> LabelAnalysisResult {
    let extraction = extract_labels(issues);
    let stats = compute_graph_stats(issues);
    let mut result = LabelAnalysisResult {
        generated_at: now.to_string(),
        total_labels: extraction.label_count as i64,
        healthy_count: 0,
        warning_count: 0,
        critical_count: 0,
        labels: Vec::new(),
        summaries: Vec::new(),
        attention_needed: Vec::new(),
    };

    for label in &extraction.labels {
        let health = compute_label_health_for_label(label, issues, cfg, now, &stats);
        let summary = LabelSummary {
            label: label.clone(),
            issue_count: health.issue_count,
            open_count: health.open_count,
            health: health.health,
            health_level: health.health_level,
            top_issue: health.issues.first().cloned(),
            needs_attention: health.health < HEALTHY_THRESHOLD,
        };
        match health.health_level {
            "healthy" => result.healthy_count += 1,
            "warning" => {
                result.warning_count += 1;
                result.attention_needed.push(label.clone());
            }
            _ => {
                result.critical_count += 1;
                result.attention_needed.push(label.clone());
            }
        }
        result.labels.push(health);
        result.summaries.push(summary);
    }

    result
        .summaries
        .sort_by(|a, b| b.health.cmp(&a.health).then_with(|| a.label.cmp(&b.label)));
    result
}

// ---------------------------------------------------------------------
// Attention scoring
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelAttentionScore {
    pub label: String,
    pub attention_score: f64,
    pub normalized_score: f64,
    pub rank: i64,
    pub pagerank_sum: f64,
    pub staleness_factor: f64,
    pub block_impact: f64,
    pub velocity_factor: f64,
    pub open_count: i64,
    pub blocked_count: i64,
    pub stale_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LabelAttentionResult {
    pub generated_at: String,
    pub labels: Vec<LabelAttentionScore>,
    pub top_attention: Vec<String>,
    pub low_attention: Vec<String>,
    pub max_score: f64,
    pub min_score: f64,
    pub total_labels: i64,
}

fn compute_label_attention(
    label: &str,
    issues: &[Issue],
    cfg: &LabelHealthConfig,
    now: jiff::Timestamp,
    stats: &GraphStats,
) -> LabelAttentionScore {
    let labeled: Vec<&Issue> = issues.iter().filter(|i| has_label(i, label)).collect();
    let mut score = LabelAttentionScore {
        label: label.to_string(),
        attention_score: 0.0,
        normalized_score: 0.0,
        rank: 0,
        pagerank_sum: 0.0,
        staleness_factor: 0.0,
        block_impact: 0.0,
        velocity_factor: 0.0,
        open_count: 0,
        blocked_count: 0,
        stale_count: 0,
    };
    if labeled.is_empty() {
        return score;
    }
    for iss in &labeled {
        if !is_closed_like(iss.status) {
            score.open_count += 1;
        }
        // Documented approximation: sum global PageRank over this label's
        // issues rather than re-running PageRank on an extracted subgraph
        // (see module doc comment).
        score.pagerank_sum += stats.pagerank.get(&iss.id).copied().unwrap_or(0.0);
    }

    let labeled_owned: Vec<Issue> = labeled.iter().map(|i| (*i).clone()).collect();
    let freshness = compute_freshness_metrics(&labeled_owned, now, cfg.stale_threshold_days);
    score.stale_count = freshness.stale_count;
    score.staleness_factor = if score.open_count > 0 {
        1.0 + score.stale_count as f64 / score.open_count as f64
    } else {
        1.0
    };

    let mut block_impact = 0i64;
    for iss in &labeled {
        for other in issues {
            if other.id == iss.id {
                continue;
            }
            for dep in &other.dependencies {
                if dep.depends_on_id == iss.id && dep.r#type.is_blocking() {
                    block_impact += 1;
                }
            }
        }
    }
    score.block_impact = block_impact as f64;
    score.blocked_count = block_impact;

    let velocity = compute_velocity_metrics(&labeled_owned, now);
    score.velocity_factor = velocity.closed_last_30_days as f64 + 1.0;

    let numerator = score.pagerank_sum * score.staleness_factor * (1.0 + score.block_impact);
    score.attention_score = numerator / score.velocity_factor;
    score
}

pub fn compute_label_attention_scores(
    issues: &[Issue],
    cfg: &LabelHealthConfig,
    now: jiff::Timestamp,
) -> LabelAttentionResult {
    let mut result = LabelAttentionResult {
        generated_at: now.to_string(),
        labels: Vec::new(),
        top_attention: Vec::new(),
        low_attention: Vec::new(),
        max_score: 0.0,
        min_score: 0.0,
        total_labels: 0,
    };
    let extraction = extract_labels(issues);
    if extraction.label_count == 0 {
        return result;
    }
    let stats = compute_graph_stats(issues);

    let mut scores: Vec<LabelAttentionScore> = extraction
        .labels
        .iter()
        .map(|label| compute_label_attention(label, issues, cfg, now, &stats))
        .collect();

    let (mut max_score, mut min_score) = (0.0, 0.0);
    for (i, s) in scores.iter().enumerate() {
        if i == 0 {
            max_score = s.attention_score;
            min_score = s.attention_score;
        } else {
            if s.attention_score > max_score {
                max_score = s.attention_score;
            }
            if s.attention_score < min_score {
                min_score = s.attention_score;
            }
        }
    }
    result.max_score = max_score;
    result.min_score = min_score;
    let range = max_score - min_score;
    for s in &mut scores {
        s.normalized_score = if range > 0.0 {
            (s.attention_score - min_score) / range
        } else {
            0.5
        };
    }

    scores.sort_by(|a, b| {
        const EPS: f64 = 1e-6;
        let diff = a.attention_score - b.attention_score;
        if diff.abs() > EPS {
            b.attention_score
                .partial_cmp(&a.attention_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.label.cmp(&b.label)
        }
    });
    for (i, s) in scores.iter_mut().enumerate() {
        s.rank = i as i64 + 1;
    }

    let top_n = scores.len().min(3);
    result.top_attention = scores[..top_n].iter().map(|s| s.label.clone()).collect();
    let low_start = (scores.len().saturating_sub(top_n)).max(top_n);
    result.low_attention = scores[low_start..]
        .iter()
        .map(|s| s.label.clone())
        .collect();

    result.total_labels = scores.len() as i64;
    result.labels = scores;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use bv_core::model::{Dependency, DependencyType, Issue};

    fn issue(id: &str, status: Status, labels: &[&str]) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: id.to_string(),
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
            updated_at: Some("2026-01-01T00:00:00Z".into()),
            due_date: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            dependencies: vec![],
            comments: vec![],
            source_repo: String::new(),
        }
    }

    fn blocks(issue_id: &str, depends_on_id: &str) -> Dependency {
        Dependency {
            issue_id: issue_id.to_string(),
            depends_on_id: depends_on_id.to_string(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: DependencyType::Blocks,
            created_at: None,
            created_by: String::new(),
        }
    }

    #[test]
    fn extract_labels_counts_and_sorts() {
        let issues = vec![
            issue("A-1", Status::Open, &["backend", "urgent"]),
            issue("A-2", Status::Open, &["backend"]),
            issue("A-3", Status::Open, &[]),
        ];
        let r = extract_labels(&issues);
        assert_eq!(r.labels, vec!["backend".to_string(), "urgent".to_string()]);
        assert_eq!(r.unlabeled_count, 1);
        assert_eq!(r.top_labels[0], "backend"); // 2 issues > urgent's 1
    }

    #[test]
    fn cross_label_flow_counts_blocking_pairs_across_labels() {
        let mut blocker = issue("A-1", Status::Open, &["backend"]);
        let mut blocked = issue("A-2", Status::Open, &["frontend"]);
        blocked.dependencies.push(blocks("A-2", "A-1"));
        blocker.dependencies.clear();
        let issues = vec![blocker, blocked];
        let cfg = LabelHealthConfig::default();
        let flow = compute_cross_label_flow(&issues, &cfg);
        assert_eq!(flow.total_cross_label_deps, 1);
        assert_eq!(flow.dependencies.len(), 1);
        assert_eq!(flow.dependencies[0].from_label, "backend");
        assert_eq!(flow.dependencies[0].to_label, "frontend");
        assert_eq!(flow.bottleneck_labels, vec!["backend".to_string()]);
    }

    #[test]
    fn cross_label_flow_ignores_same_label_and_non_blocking() {
        let mut blocker = issue("A-1", Status::Open, &["backend"]);
        let mut blocked = issue("A-2", Status::Open, &["backend"]); // same label
        blocked.dependencies.push(blocks("A-2", "A-1"));
        blocker.dependencies.clear();
        let issues = vec![blocker, blocked];
        let flow = compute_cross_label_flow(&issues, &LabelHealthConfig::default());
        assert_eq!(
            flow.total_cross_label_deps, 0,
            "same-label deps must not count as cross-label"
        );
    }

    #[test]
    fn label_health_empty_label_is_critical_zero() {
        let issues = vec![issue("A-1", Status::Open, &["backend"])];
        let stats = compute_graph_stats(&issues);
        let h = compute_label_health_for_label(
            "nonexistent",
            &issues,
            &LabelHealthConfig::default(),
            jiff::Timestamp::now(),
            &stats,
        );
        assert_eq!(h.issue_count, 0);
        assert_eq!(h.health, 0);
        assert_eq!(h.health_level, "critical");
    }

    #[test]
    fn all_label_health_covers_every_extracted_label() {
        let issues = vec![
            issue("A-1", Status::Open, &["backend"]),
            issue("A-2", Status::Blocked, &["frontend"]),
        ];
        let result = compute_all_label_health(
            &issues,
            &LabelHealthConfig::default(),
            jiff::Timestamp::now(),
        );
        assert_eq!(result.total_labels, 2);
        assert_eq!(result.labels.len(), 2);
        assert_eq!(result.summaries.len(), 2);
        // healthy + warning + critical must partition all labels
        assert_eq!(
            result.healthy_count + result.warning_count + result.critical_count,
            2
        );
    }

    #[test]
    fn attention_scores_rank_by_score_desc_with_label_tiebreak() {
        let issues = vec![
            issue("A-1", Status::Open, &["quiet"]),
            issue("A-2", Status::Open, &["busy"]),
        ];
        let result = compute_label_attention_scores(
            &issues,
            &LabelHealthConfig::default(),
            jiff::Timestamp::now(),
        );
        assert_eq!(result.total_labels, 2);
        assert_eq!(result.labels.len(), 2);
        // ranks are contiguous starting at 1
        let mut ranks: Vec<i64> = result.labels.iter().map(|l| l.rank).collect();
        ranks.sort();
        assert_eq!(ranks, vec![1, 2]);
    }

    #[test]
    fn empty_issue_set_yields_empty_attention_result() {
        let result = compute_label_attention_scores(
            &[],
            &LabelHealthConfig::default(),
            jiff::Timestamp::now(),
        );
        assert_eq!(result.total_labels, 0);
        assert!(result.labels.is_empty());
    }
}
