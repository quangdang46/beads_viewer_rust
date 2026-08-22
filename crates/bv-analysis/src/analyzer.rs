//! Two-phase analyzer — port of Go `analysis.Analyzer` orchestration
//! (graph.go:65-200 status/config shapes, phase semantics).
//!
//! Phase 1 (sync): degree maps, topo order, density — always immediate.
//! Phase 2: per-metric work on std::threads with per-metric timeouts;
//! panic -> timeout status; MetricStatus per metric.

use crate::algorithms::{
    betweenness::{betweenness, betweenness_approx},
    critical_path::{critical_path_heights, critical_path_nodes},
    cycles::{enumerate_cycles, tarjan_scc},
    hits::hits_default,
    kcore::kcore,
    pagerank::pagerank_default,
    topo::topological_sort,
};
use crate::graph::DiGraph;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Per-metric computation outcome (Go `statusEntry`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StatusEntry {
    /// computed|approx|timeout|skipped
    pub state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub sample: usize,
    /// Milliseconds, omitted when zero (Go MarshalJSON omitempty).
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub ms: f64,
}

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}
fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

impl StatusEntry {
    pub fn computed(ms: f64) -> Self {
        StatusEntry {
            state: "computed".into(),
            ms,
            ..Default::default()
        }
    }
    pub fn skipped(reason: &str) -> Self {
        StatusEntry {
            state: "skipped".into(),
            reason: reason.into(),
            ..Default::default()
        }
    }
    pub fn timeout(ms: f64) -> Self {
        StatusEntry {
            state: "timeout".into(),
            ms,
            ..Default::default()
        }
    }
}

/// Per-metric status map keyed exactly as the Go JSON:
/// PageRank/Betweenness/Eigenvector/HITS/Critical/Cycles/KCore/Articulation/Slack.
/// BTreeMap keeps key order stable in serialized output.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MetricStatus {
    pub page_rank: StatusEntry,
    pub betweenness: StatusEntry,
    pub eigenvector: StatusEntry,
    pub hits: StatusEntry,
    pub critical: StatusEntry,
    pub cycles: StatusEntry,
    pub kcore: StatusEntry,
    pub articulation: StatusEntry,
    pub slack: StatusEntry,
}

impl MetricStatus {
    /// Serialize with Go's exact JSON keys.
    pub fn to_json_map(&self) -> serde_json::Value {
        serde_json::json!({
            "PageRank": self.page_rank,
            "Betweenness": self.betweenness,
            "Eigenvector": self.eigenvector,
            "HITS": self.hits,
            "Critical": self.critical,
            "Cycles": self.cycles,
            "KCore": self.kcore,
            "Articulation": self.articulation,
            "Slack": self.slack,
        })
    }
}

/// Per-metric Phase 2 timeout budgets (Go `ConfigForSize`).
#[derive(Debug, Clone, Copy)]
pub struct AnalysisBudget {
    pub small_threshold: usize,
    pub medium_threshold: usize,
    pub xl_threshold: usize,
    /// Per-metric override from BV_PHASE2_TIMEOUT_S (seconds).
    pub override_secs: Option<u64>,
    pub skip_phase2: bool,
}

impl Default for AnalysisBudget {
    fn default() -> Self {
        AnalysisBudget {
            small_threshold: 100,
            medium_threshold: 500,
            xl_threshold: 2000,
            override_secs: None,
            skip_phase2: false,
        }
    }
}

impl AnalysisBudget {
    /// Timeout for a metric at a given node count (Go ConfigForSize tiers).
    pub fn timeout_for(&self, nodes: usize) -> Duration {
        if let Some(s) = self.override_secs {
            return Duration::from_secs(s);
        }
        match nodes {
            n if n < self.small_threshold => Duration::from_secs(2),
            n if n < self.medium_threshold => Duration::from_millis(500),
            n if n < self.xl_threshold => Duration::from_millis(300),
            _ => Duration::from_millis(200),
        }
    }

    pub fn max_cycles(&self, nodes: usize) -> usize {
        match nodes {
            n if n < self.small_threshold => 1000,
            n if n < self.medium_threshold => 100,
            n if n < self.xl_threshold => 50,
            _ => 10,
        }
    }
}

/// Phase 1 results — always available immediately.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Phase1Stats {
    /// out-degree per node (dependencies this issue has).
    pub out_degree: BTreeMap<String, usize>,
    /// in-degree per node (issues depending on this one).
    pub in_degree: BTreeMap<String, usize>,
    /// Topological order (Kahn with sorted frontier — gonum determinism).
    pub topological_order: Vec<String>,
    pub density: f64,
    pub node_count: usize,
    pub edge_count: usize,
}

/// Full analysis result. Phase 2 metrics are Option until computed.
#[derive(Debug, Clone, Default)]
pub struct GraphAnalysis {
    pub phase1: Phase1Stats,
    pub status: MetricStatus,

    pub page_rank: Option<BTreeMap<String, f64>>,
    pub betweenness: Option<BTreeMap<String, f64>>,
    pub eigenvector: Option<BTreeMap<String, f64>>,
    pub hubs: Option<BTreeMap<String, f64>>,
    pub authorities: Option<BTreeMap<String, f64>>,
    pub critical_path_score: Option<BTreeMap<String, f64>>,
    pub core_number: Option<BTreeMap<String, u32>>,
    pub articulation: Option<Vec<String>>,
    pub slack: Option<BTreeMap<String, f64>>,
    pub cycles: Option<Vec<Vec<String>>>,
}

/// Build the blocking-only DiGraph from loaded issues.
/// Edge direction matches wasm crate convention: issue -> its dependency
/// (u depends on v => edge u -> v). Empty type counts as blocks (legacy).
pub fn build_graph(issues: &[bv_core::model::Issue]) -> DiGraph {
    let mut g = DiGraph::with_capacity(issues.len(), issues.len() * 2);
    for i in issues {
        g.add_node(&i.id);
    }
    for i in issues {
        let from = match g.node_idx(&i.id) {
            Some(x) => x,
            None => continue,
        };
        for dep in &i.dependencies {
            // Only blocking types gate the graph (related/parent-child don't).
            if !dep.r#type.is_blocking() {
                continue;
            }
            let target = dep.effective_depends_on().to_string();
            if let Some(to) = g.node_idx(&target) {
                if to != from {
                    g.add_edge(from, to);
                }
            }
        }
    }
    g
}

/// Phase 1: degrees + topo + density (sync, cheap).
pub fn analyze_phase1(g: &DiGraph) -> Phase1Stats {
    let n = g.len();
    let mut out_degree = BTreeMap::new();
    let mut in_degree = BTreeMap::new();
    for idx in 0..n {
        let id = g.node_id(idx).unwrap_or_default().to_string();
        out_degree.insert(id.clone(), g.out_degree(idx));
        in_degree.insert(id, g.in_degree(idx));
    }
    let topo = topological_sort(g); // Kahn sorted-frontier; None when cyclic
    let topological_order: Vec<String> = topo
        .unwrap_or_else(|| (0..n).collect())
        .into_iter()
        .map(|idx| g.node_id(idx).unwrap_or_default().to_string())
        .collect();
    Phase1Stats {
        out_degree,
        in_degree,
        topological_order,
        density: g.density(),
        node_count: n,
        edge_count: g.edge_count(),
    }
}

fn idx_to_score_map(g: &DiGraph, scores: Vec<f64>) -> BTreeMap<String, f64> {
    scores
        .into_iter()
        .enumerate()
        .map(|(i, s)| (g.node_id(i).unwrap_or_default().to_string(), s))
        .collect()
}

/// Run one Phase-2 metric on a thread with timeout; panic or overrun -> Err.
fn run_with_timeout<T: Send + 'static>(
    budget: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ()> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Panic inside f is caught by converting the join result.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let _ = tx.send(result);
    });
    match rx.recv_timeout(budget) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Err(()), // panic -> timeout status (Go parity)
        Err(_) => Err(()),     // recv_timeout elapsed
    }
}

/// Execute Phase 2 synchronously-with-timeouts (call from a worker thread of
/// your own; this function blocks until all metrics settle or time out).
/// Mirrors Go's goroutine-per-metric fan-out but joins before returning.
/// Takes `Arc<DiGraph>` so metric closures own a handle ('static bound).
pub fn analyze_phase2_blocking(
    g: std::sync::Arc<DiGraph>,
    budget: &AnalysisBudget,
) -> (MetricStatus, GraphAnalysisPhase2) {
    let mut status = MetricStatus::default();
    let mut out = GraphAnalysisPhase2::default();
    let n = g.len();

    if budget.skip_phase2 {
        let reason = "BV_SKIP_PHASE2 set";
        status.page_rank = StatusEntry::skipped(reason);
        status.betweenness = StatusEntry::skipped(reason);
        status.eigenvector = StatusEntry::skipped(reason);
        status.hits = StatusEntry::skipped(reason);
        status.critical = StatusEntry::skipped(reason);
        status.cycles = StatusEntry::skipped(reason);
        // k-core/articulation/slack REMAIN enabled (Go ApplyEnvOverrides).
    } else {
        // PageRank
        let t0 = Instant::now();
        let gc = std::sync::Arc::clone(&g);
        match run_with_timeout(budget.timeout_for(n), move || pagerank_default(&gc)) {
            Ok(pr) => {
                status.page_rank = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                out.page_rank = Some(idx_to_score_map(&g, pr));
            }
            Err(()) => status.page_rank = StatusEntry::timeout(0.0),
        }

        // Betweenness: exact below XL threshold; approx w/ sample above.
        let t0 = Instant::now();
        let sample = recommend_sample_size(n);
        if n < budget.xl_threshold {
            let gc = std::sync::Arc::clone(&g);
            match run_with_timeout(budget.timeout_for(n), move || betweenness(&gc)) {
                Ok(bw) => {
                    status.betweenness = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                    out.betweenness = Some(idx_to_score_map(&g, bw));
                }
                Err(()) => status.betweenness = StatusEntry::timeout(0.0),
            }
        } else {
            let gc = std::sync::Arc::clone(&g);
            match run_with_timeout(budget.timeout_for(n), move || {
                betweenness_approx(&gc, sample, Some(1))
            }) {
                Ok(bw) => {
                    let mut e = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                    e.state = "approx".into();
                    e.sample = sample;
                    status.betweenness = e;
                    out.betweenness = Some(idx_to_score_map(&g, bw));
                }
                Err(()) => status.betweenness = StatusEntry::timeout(0.0),
            }
        }

        // Eigenvector (fixed 50 iterations — Go parity).
        let t0 = Instant::now();
        let gc = std::sync::Arc::clone(&g);
        match run_with_timeout(budget.timeout_for(n), move || {
            crate::algorithms::eigenvector::eigenvector_default(&gc)
        }) {
            Ok(ev) => {
                status.eigenvector = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                out.eigenvector = Some(idx_to_score_map(&g, ev));
            }
            Err(()) => status.eigenvector = StatusEntry::timeout(0.0),
        }

        // HITS (tol 1e-3 — Go network.HITS(g, 1e-3)).
        let t0 = Instant::now();
        let gc = std::sync::Arc::clone(&g);
        match run_with_timeout(budget.timeout_for(n), move || hits_default(&gc)) {
            Ok(h) => {
                status.hits = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                out.hubs = Some(idx_to_score_map(&g, h.hubs));
                out.authorities = Some(idx_to_score_map(&g, h.authorities));
            }
            Err(()) => status.hits = StatusEntry::timeout(0.0),
        }

        // Critical path heights DP.
        let t0 = Instant::now();
        let gc = std::sync::Arc::clone(&g);
        match run_with_timeout(budget.timeout_for(n), move || critical_path_heights(&gc)) {
            Ok(heights) => {
                status.critical = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                let mut m = BTreeMap::new();
                for (i, v) in heights.into_iter().enumerate() {
                    m.insert(g.node_id(i).unwrap_or_default().to_string(), v);
                }
                out.critical_path_score = Some(m);
            }
            Err(()) => status.critical = StatusEntry::timeout(0.0),
        }

        // Cycles: Tarjan pre-check then enumerate capped by tier.
        let t0 = Instant::now();
        let cap = budget.max_cycles(n);
        let scc = tarjan_scc(&g);
        if !scc.has_cycles {
            status.cycles = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
            out.cycles = Some(Vec::new());
        } else {
            let gc = std::sync::Arc::clone(&g);
            match run_with_timeout(budget.timeout_for(n), move || enumerate_cycles(&gc, cap)) {
                Ok(cycles) => {
                    status.cycles = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                    let named: Vec<Vec<String>> = cycles
                        .into_iter()
                        .map(|c| {
                            c.into_iter()
                                .map(|i| g.node_id(i).unwrap_or_default().to_string())
                                .collect()
                        })
                        .collect();
                    out.cycles = Some(named);
                }
                Err(()) => status.cycles = StatusEntry::timeout(0.0),
            }
        }
    }

    // k-core / articulation / slack: always-on even under BV_SKIP_PHASE2.
    let t0 = Instant::now();
    let gc = std::sync::Arc::clone(&g);
    match run_with_timeout(budget.timeout_for(n), move || kcore(&gc)) {
        Ok(cores) => {
            status.kcore = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
            let mut m = BTreeMap::new();
            for (i, v) in cores.into_iter().enumerate() {
                m.insert(g.node_id(i).unwrap_or_default().to_string(), v);
            }
            out.core_number = Some(m);
        }
        Err(()) => status.kcore = StatusEntry::timeout(0.0),
    }

    let t0 = Instant::now();
    let gc = std::sync::Arc::clone(&g);
    match run_with_timeout(budget.timeout_for(n), move || {
        crate::algorithms::articulation::articulation_points(&gc)
    }) {
        Ok(points) => {
            status.articulation = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
            out.articulation = Some(
                points
                    .into_iter()
                    .map(|i| g.node_id(i).unwrap_or_default().to_string())
                    .collect(),
            );
        }
        Err(()) => status.articulation = StatusEntry::timeout(0.0),
    }

    let t0 = Instant::now();
    let gc = std::sync::Arc::clone(&g);
    match run_with_timeout(budget.timeout_for(n), move || {
        crate::algorithms::slack::slack(&gc)
    }) {
        Ok(slacks) => {
            status.slack = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
            out.slack = Some(idx_to_score_map(&g, slacks));
        }
        Err(()) => status.slack = StatusEntry::timeout(0.0),
    }

    (status, out)
}

/// Phase 2 metric payloads.
#[derive(Debug, Clone, Default)]
pub struct GraphAnalysisPhase2 {
    pub page_rank: Option<BTreeMap<String, f64>>,
    pub betweenness: Option<BTreeMap<String, f64>>,
    pub eigenvector: Option<BTreeMap<String, f64>>,
    pub hubs: Option<BTreeMap<String, f64>>,
    pub authorities: Option<BTreeMap<String, f64>>,
    pub critical_path_score: Option<BTreeMap<String, f64>>,
    pub core_number: Option<BTreeMap<String, u32>>,
    pub articulation: Option<Vec<String>>,
    pub slack: Option<BTreeMap<String, f64>>,
    pub cycles: Option<Vec<Vec<String>>>,
}

/// Go: `RecommendSampleSize` tiers.
pub fn recommend_sample_size(nodes: usize) -> usize {
    match nodes {
        0..=99 => nodes,
        100..=499 => (nodes / 5).max(50),
        500..=1999 => 100,
        _ => 200,
    }
}

/// Critical path node list convenience (max height nodes).
pub fn critical_path(g: &DiGraph) -> Vec<usize> {
    critical_path_nodes(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(n: usize) -> DiGraph {
        let mut g = DiGraph::with_capacity(n, n.saturating_sub(1));
        for i in 1..=n {
            g.add_node(&format!("FIX-{i}"));
        }
        // Node indices are 0-based; FIX-k has index k-1.
        // Edge index j -> j+1 = dependency chain FIX-(j+1) depends on FIX-(j+...
        // matching wasm convention: u depends on v => edge u -> v.
        for j in 0..n - 1 {
            g.add_edge(j, j + 1);
        }
        g
    }

    #[test]
    fn phase1_degrees_and_density() {
        let g = chain(12);
        let p1 = analyze_phase1(&g);
        assert_eq!(p1.node_count, 12);
        assert_eq!(p1.edge_count, 11);
        assert_eq!(p1.out_degree["FIX-1"], 1); // FIX-1 -> FIX-2
        assert_eq!(p1.in_degree["FIX-1"], 0); // first node unblocked
        assert_eq!(p1.in_degree["FIX-12"], 1); // edge FIX-11 -> FIX-12
        assert_eq!(p1.topological_order.len(), 12);
        assert_eq!(p1.topological_order[0], "FIX-1");
    }

    #[test]
    fn phase2_computes_all_metrics_small_graph() {
        let g = chain(12);
        let (status, out) =
            analyze_phase2_blocking(std::sync::Arc::new(g), &AnalysisBudget::default());
        assert_eq!(status.page_rank.state, "computed", "{:?}", status.page_rank);
        assert_eq!(status.betweenness.state, "computed");
        assert_eq!(status.kcore.state, "computed");
        let pr = out.page_rank.expect("pagerank present");
        assert_eq!(pr.len(), 12);
        let sum: f64 = pr.values().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        // acyclic graph -> zero cycles but computed
        assert_eq!(out.cycles, Some(Vec::new()));
    }

    #[test]
    fn skip_phase2_keeps_kcore_family_enabled() {
        let g = chain(12);
        let budget = AnalysisBudget {
            skip_phase2: true,
            ..Default::default()
        };
        let (status, out) = analyze_phase2_blocking(std::sync::Arc::new(g), &budget);
        assert_eq!(status.page_rank.state, "skipped");
        assert_eq!(status.hits.state, "skipped");
        assert_eq!(status.kcore.state, "computed", "k-core stays enabled");
        assert_eq!(status.slack.state, "computed");
        assert!(out.core_number.is_some());
        assert!(out.page_rank.is_none());
    }

    #[test]
    fn timeout_budget_tiers_match_go_configforsize() {
        let b = AnalysisBudget::default();
        assert_eq!(b.timeout_for(50), Duration::from_secs(2));
        assert_eq!(b.timeout_for(300), Duration::from_millis(500));
        assert_eq!(b.timeout_for(900), Duration::from_millis(300));
        assert_eq!(b.timeout_for(5000), Duration::from_millis(200));
        assert_eq!(b.max_cycles(50), 1000);
        assert_eq!(b.max_cycles(300), 100);
        assert_eq!(b.max_cycles(900), 50);
        assert_eq!(b.max_cycles(5000), 10);
    }

    #[test]
    fn recommend_sample_size_tiers() {
        assert_eq!(recommend_sample_size(50), 50);
        assert_eq!(recommend_sample_size(300), 60);
        assert_eq!(recommend_sample_size(150), 50); // max(50, 30)
        assert_eq!(recommend_sample_size(1000), 100);
        assert_eq!(recommend_sample_size(5000), 200);
    }

    #[test]
    fn status_entry_json_matches_go_shape() {
        let e = StatusEntry {
            state: "computed".into(),
            ms: 5.5,
            ..Default::default()
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["state"], "computed");
        assert_eq!(v["ms"], 5.5);
        assert!(v.get("reason").is_none()); // omitempty
        let skipped = StatusEntry::skipped("too dense");
        let v = serde_json::to_value(&skipped).unwrap();
        assert_eq!(v["reason"], "too dense");
        assert!(v.get("ms").is_none());
    }

    #[test]
    fn metric_status_json_uses_go_keys_in_order() {
        let ms = MetricStatus::default();
        let v = ms.to_json_map();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec![
                "PageRank",
                "Betweenness",
                "Eigenvector",
                "HITS",
                "Critical",
                "Cycles",
                "KCore",
                "Articulation",
                "Slack"
            ]
        );
    }
}
