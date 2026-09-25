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
    topo::topological_sort_gonum,
};
use bv_graph_core::DiGraph;
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
    /// Graph density = edges / (nodes * (nodes - 1)). Used for density-aware
    /// tier decisions matching Go `ConfigForSize` (pkg/analysis/config.go:86).
    pub density: f64,
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
            density: 0.0,
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

    /// Whether cycles should be skipped entirely (Go: XL always skips).
    pub fn skip_cycles(&self, nodes: usize) -> bool {
        nodes >= self.xl_threshold
    }

    /// Whether HITS should be skipped for this graph (Go: skip when XL + dense).
    pub fn skip_hits(&self, nodes: usize) -> bool {
        // XL (>2000 nodes) AND density >= 0.001 -> skip HITS.
        nodes >= self.xl_threshold && self.density >= 0.001
    }

    /// Whether betweenness should use approximate mode (Go: approx when dense).
    /// Returns `(use_approx, skip)` where skip means don't compute at all.
    pub fn betweenness_mode(&self, nodes: usize) -> (bool, bool) {
        if nodes >= self.xl_threshold {
            // XL: always approximate (Go parity).
            (true, false)
        } else if nodes >= self.medium_threshold {
            // Large (500-2000): approximate if density >= 0.01, skip otherwise.
            if self.density < 0.01 {
                (true, false)
            } else {
                (false, true)
            }
        } else {
            // Small/Medium: always exact.
            (false, false)
        }
    }

    /// Go `RecommendSampleSize` (pkg/analysis/betweenness_approx.go:489).
    /// `edge_count` is reserved in Go for future density-aware heuristics and
    /// is deliberately unused; the parameter is kept so call sites read the
    /// same as Go's.
    pub fn recommend_sample_size(&self, node_count: usize, _edge_count: usize) -> usize {
        if node_count == 0 {
            return 0;
        }
        if node_count < 100 {
            node_count
        } else if node_count < 500 {
            // 20% sample, floored at 50.
            std::cmp::max(node_count / 5, 50)
        } else if node_count < 2000 {
            100
        } else {
            200
        }
    }
}

/// Go `AnalysisConfig` as it appears in robot output
/// (pkg/analysis/config.go:12). Field order is part of the compatibility
/// contract: Go serializes the struct positionally, and the differential
/// harness compares canonicalized JSON, so a reordered field is a drift.
///
/// The two execution-state fields Go marks `json:"-"` (`DisableCache`,
/// `RunToCompletion`) are not emitted and so are absent here too.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AnalysisConfigReport {
    #[serde(rename = "ComputeBetweenness")]
    pub compute_betweenness: bool,
    /// Nanoseconds — Go marshals `time.Duration` as an int64 count of ns.
    #[serde(rename = "BetweennessTimeout")]
    pub betweenness_timeout_ns: i64,
    #[serde(rename = "BetweennessSkipReason")]
    pub betweenness_skip_reason: &'static str,
    /// "exact" | "approximate" | "skip" (Go `BetweennessMode`).
    #[serde(rename = "BetweennessMode")]
    pub betweenness_mode: &'static str,
    #[serde(rename = "BetweennessSampleSize")]
    pub betweenness_sample_size: usize,
    #[serde(rename = "BetweennessIsApproximate")]
    pub betweenness_is_approximate: bool,

    #[serde(rename = "ComputePageRank")]
    pub compute_page_rank: bool,
    #[serde(rename = "PageRankTimeout")]
    pub page_rank_timeout_ns: i64,
    #[serde(rename = "PageRankSkipReason")]
    pub page_rank_skip_reason: &'static str,

    #[serde(rename = "ComputeHITS")]
    pub compute_hits: bool,
    #[serde(rename = "HITSTimeout")]
    pub hits_timeout_ns: i64,
    #[serde(rename = "HITSSkipReason")]
    pub hits_skip_reason: &'static str,

    #[serde(rename = "ComputeCycles")]
    pub compute_cycles: bool,
    #[serde(rename = "CyclesTimeout")]
    pub cycles_timeout_ns: i64,
    #[serde(rename = "MaxCyclesToStore")]
    pub max_cycles_to_store: usize,
    #[serde(rename = "CyclesSkipReason")]
    pub cycles_skip_reason: &'static str,

    #[serde(rename = "ComputeEigenvector")]
    pub compute_eigenvector: bool,
    #[serde(rename = "ComputeCriticalPath")]
    pub compute_critical_path: bool,
    #[serde(rename = "ComputeKCore")]
    pub compute_kcore: bool,
    #[serde(rename = "ComputeArticulation")]
    pub compute_articulation: bool,
    #[serde(rename = "ComputeSlack")]
    pub compute_slack: bool,
}

const NS_PER_SEC: i64 = 1_000_000_000;
const NS_PER_MS: i64 = 1_000_000;

/// Go `ConfigForSize` (pkg/analysis/config.go:98) — byte port.
///
/// Tiers, all keyed on node count:
/// - `< 100`   small: exact betweenness, 2s budgets, 1000 cycles stored
/// - `< 500`   medium: exact betweenness, 500ms budgets, 100 cycles
/// - `< 2000`  large: approximate betweenness when density < 0.01 (500ms
///   budget, sampled), otherwise betweenness is skipped entirely
/// - `>= 2000` XL: approximate betweenness always; cycles are skipped; HITS
///   only survives on very sparse graphs (density < 0.001)
///
/// `node_count` is the issue count, not the number of nodes carrying edges —
/// the tier boundary is drawn against the full set.
pub fn config_for_size(node_count: usize, edge_count: usize, density: f64) -> AnalysisConfigReport {
    let sample = |n: usize| {
        if n == 0 {
            0
        } else if n < 100 {
            n
        } else if n < 500 {
            std::cmp::max(n / 5, 50)
        } else if n < 2000 {
            100
        } else {
            200
        }
    };
    let _ = edge_count; // reserved in Go too

    match node_count {
        n if n < 100 => AnalysisConfigReport {
            compute_betweenness: true,
            betweenness_timeout_ns: 2 * NS_PER_SEC,
            betweenness_skip_reason: "",
            betweenness_mode: "exact",
            betweenness_sample_size: 0,
            betweenness_is_approximate: false,
            compute_page_rank: true,
            page_rank_timeout_ns: 2 * NS_PER_SEC,
            page_rank_skip_reason: "",
            compute_hits: true,
            hits_timeout_ns: 2 * NS_PER_SEC,
            hits_skip_reason: "",
            compute_cycles: true,
            cycles_timeout_ns: 2 * NS_PER_SEC,
            max_cycles_to_store: 1000,
            cycles_skip_reason: "",
            compute_eigenvector: true,
            compute_critical_path: true,
            compute_kcore: true,
            compute_articulation: true,
            compute_slack: true,
        },
        n if n < 500 => AnalysisConfigReport {
            compute_betweenness: true,
            betweenness_timeout_ns: 500 * NS_PER_MS,
            betweenness_skip_reason: "",
            betweenness_mode: "exact",
            betweenness_sample_size: 0,
            betweenness_is_approximate: false,
            compute_page_rank: true,
            page_rank_timeout_ns: 500 * NS_PER_MS,
            page_rank_skip_reason: "",
            compute_hits: true,
            hits_timeout_ns: 500 * NS_PER_MS,
            hits_skip_reason: "",
            compute_cycles: true,
            cycles_timeout_ns: 500 * NS_PER_MS,
            max_cycles_to_store: 100,
            cycles_skip_reason: "",
            compute_eigenvector: true,
            compute_critical_path: true,
            compute_kcore: true,
            compute_articulation: true,
            compute_slack: true,
        },
        n if n < 2000 => {
            // Large: approximate betweenness for sparse graphs, skip for dense.
            let (bw, bw_timeout, bw_reason, bw_mode, bw_sample) = if density < 0.01 {
                (true, 500 * NS_PER_MS, "", "approximate", sample(node_count))
            } else {
                (false, 0, "graph too dense (density > 0.01)", "skip", 0)
            };
            AnalysisConfigReport {
                compute_betweenness: bw,
                betweenness_timeout_ns: bw_timeout,
                betweenness_skip_reason: bw_reason,
                betweenness_mode: bw_mode,
                betweenness_sample_size: bw_sample,
                betweenness_is_approximate: false,
                compute_page_rank: true,
                page_rank_timeout_ns: 300 * NS_PER_MS,
                page_rank_skip_reason: "",
                compute_hits: true,
                hits_timeout_ns: 300 * NS_PER_MS,
                hits_skip_reason: "",
                compute_cycles: true,
                cycles_timeout_ns: 300 * NS_PER_MS,
                max_cycles_to_store: 50,
                cycles_skip_reason: "",
                compute_eigenvector: true,
                compute_critical_path: true,
                compute_kcore: true,
                compute_articulation: true,
                compute_slack: true,
            }
        }
        _ => {
            // XL: HITS only on very sparse graphs; cycles never run.
            let (hits, hits_timeout, hits_reason) = if density < 0.001 {
                (true, 200 * NS_PER_MS, "")
            } else {
                (false, 0, "graph too large and dense")
            };
            AnalysisConfigReport {
                compute_betweenness: true,
                betweenness_timeout_ns: 500 * NS_PER_MS,
                betweenness_skip_reason: "",
                betweenness_mode: "approximate",
                betweenness_sample_size: sample(node_count),
                betweenness_is_approximate: false,
                compute_page_rank: true,
                page_rank_timeout_ns: 200 * NS_PER_MS,
                page_rank_skip_reason: "",
                compute_hits: hits,
                hits_timeout_ns: hits_timeout,
                hits_skip_reason: hits_reason,
                compute_cycles: false,
                cycles_timeout_ns: 0,
                max_cycles_to_store: 10,
                cycles_skip_reason: "graph too large (>2000 nodes)",
                compute_eigenvector: true,
                compute_critical_path: true,
                compute_kcore: true,
                compute_articulation: true,
                compute_slack: true,
            }
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
    /// Topological order, gonum `topo.Sort` walked backwards like Go
    /// (dependencies first). `None` when gonum reports a cyclic graph, which
    /// is how Go leaves the field nil (serialized as JSON `null`).
    pub topological_order: Option<Vec<String>>,
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
    // Go sorts the issue ids before assigning dense node indices
    // (graph.go:1570-1585), and HITS sums over those indices, so the node
    // order changes the floating-point result. Build in sorted-id order to
    // match.
    let mut sorted: Vec<&bv_core::model::Issue> = issues.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    let mut g = DiGraph::with_capacity(sorted.len(), sorted.len() * 2);
    for i in &sorted {
        g.add_node(&i.id);
    }
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(sorted.len() * 2);
    for i in &sorted {
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
                // Go keeps self-loops (gonum SetEdge accepts them); they
                // participate in out-degree and cycle detection there.
                edges.push((from, to));
            }
        }
    }
    // Go's `buildCachedAdjacency` sorts every node's neighbour list before the
    // Brandes walk (betweenness_approx.go:59, :70), and `network.Betweenness`
    // consumes the same gonum graph. Successor order is not cosmetic: the BFS
    // stack order follows it, and the delta accumulation is floating point, so
    // an unsorted list changes the per-node scores. Measured on
    // large_cyclic_600, 473 of 600 nodes differ between insertion order and
    // target-index order.
    //
    // Node indices are already id-sorted (see above), so sorting by target
    // index is exactly Go's `sort.Ints(neighbors)`.
    edges.sort_by_key(|&(from, to)| (from, to));
    for (from, to) in edges {
        g.add_edge(from, to);
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
    // Go: `sorted, err := topo.Sort(a.g)` then walks the slice backwards
    // (graph.go:1950-1954). gonum's topo.Sort is Tarjan-SCC based, so a Kahn
    // order here is valid but ordered differently; the insights field has to
    // reproduce gonum exactly. On a cyclic graph gonum returns an Unorderable
    // error and Go leaves the field nil, so `None` stays `None` here.
    let topological_order = topological_sort_gonum(g).map(|order| {
        order
            .iter()
            .rev()
            .map(|&idx| g.node_id(idx).unwrap_or_default().to_string())
            .collect()
    });
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
        match run_with_timeout(budget.timeout_for(n), move || {
            let _t = crate::metrics::time(&crate::metrics::TIMING_PAGERANK_COMPUTE);
            pagerank_default(&gc)
        }) {
            Ok(pr) => {
                status.page_rank = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                out.page_rank = Some(idx_to_score_map(&g, pr));
            }
            Err(()) => status.page_rank = StatusEntry::timeout(0.0),
        }

        // Betweenness: density-aware mode selection (Go ConfigForSize parity).
        let t0 = Instant::now();
        let sample = recommend_sample_size(n);
        let (bw_approx, bw_skip) = budget.betweenness_mode(n);
        if bw_skip {
            status.betweenness = StatusEntry::skipped("graph too dense (density > 0.01)");
        } else if !bw_approx {
            let gc = std::sync::Arc::clone(&g);
            match run_with_timeout(budget.timeout_for(n), move || {
                let _t = crate::metrics::time(&crate::metrics::TIMING_BETWEENNESS_COMPUTE);
                betweenness(&gc)
            }) {
                Ok(bw) => {
                    status.betweenness = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                    out.betweenness = Some(idx_to_score_map(&g, bw));
                }
                Err(()) => status.betweenness = StatusEntry::timeout(0.0),
            }
        } else {
            let gc = std::sync::Arc::clone(&g);
            match run_with_timeout(budget.timeout_for(n), move || {
                let _t = crate::metrics::time(&crate::metrics::TIMING_BETWEENNESS_COMPUTE);
                betweenness_approx(&gc, sample, Some(1))
            }) {
                Ok(bw) => {
                    // Go: state stays "computed" with reason "approximate"
                    // (betweennessReason) plus the sample size.
                    let mut e = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                    e.reason = "approximate".into();
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
        // Skip for XL dense graphs (Go ConfigForSize: density >= 0.001).
        if budget.skip_hits(n) {
            status.hits = StatusEntry::skipped("graph too large and dense");
        } else {
            let t0 = Instant::now();
            let gc = std::sync::Arc::clone(&g);
            match run_with_timeout(budget.timeout_for(n), move || {
                let _t = crate::metrics::time(&crate::metrics::TIMING_HITS_COMPUTE);
                hits_default(&gc)
            }) {
                Ok(h) => {
                    status.hits = StatusEntry::computed(t0.elapsed().as_secs_f64() * 1000.0);
                    out.hubs = Some(idx_to_score_map(&g, h.hubs));
                    out.authorities = Some(idx_to_score_map(&g, h.authorities));
                }
                Err(()) => status.hits = StatusEntry::timeout(0.0),
            }
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

        // Cycles: skip entirely for XL graphs (Go ConfigForSize: >2000 nodes).
        // Tarjan pre-check then enumerate capped by tier.
        if budget.skip_cycles(n) {
            status.cycles = StatusEntry::skipped("graph too large (>2000 nodes)");
        } else {
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
        } // end else !skip_cycles
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

    fn issue_with_blocking_deps(id: &str, depends_on: &[&str]) -> bv_core::model::Issue {
        use bv_core::model::{Dependency, DependencyType, Issue, Status};
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: id.to_string(),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status: Status::Open,
            priority: 2,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: None,
            updated_at: None,
            due_date: None,
            defer_until: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: vec![],
            dependencies: depends_on
                .iter()
                .map(|target| Dependency {
                    issue_id: id.to_string(),
                    depends_on_id: (*target).to_string(),
                    depends_on_legacy: String::new(),
                    target_id_legacy: String::new(),
                    r#type: DependencyType::Blocks,
                    created_at: None,
                    created_by: String::new(),
                })
                .collect(),
            comments: vec![],
            source_repo: String::new(),
        }
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
        let order = p1.topological_order.as_deref().unwrap();
        assert_eq!(order.len(), 12);
        assert_eq!(order[0], "FIX-12");
    }

    #[test]
    fn phase1_topological_order_matches_gonum() {
        // Diamond over sorted ids: a->b, a->c, b->d, c->d. gonum's topo.Sort
        // returns [a, c, b, d] and Go walks that slice backwards
        // (graph.go:1952-1954), so the reported order is d, b, c, a. Feeding
        // the Kahn order [a, b, c, d] through the same backwards walk would
        // report d, c, b, a instead — the tie is what differs.
        let issues = vec![
            issue_with_blocking_deps("a", &["b", "c"]),
            issue_with_blocking_deps("b", &["d"]),
            issue_with_blocking_deps("c", &["d"]),
            issue_with_blocking_deps("d", &[]),
        ];
        let g = build_graph(&issues);
        let p1 = analyze_phase1(&g);
        assert_eq!(
            p1.topological_order,
            Some(["d", "b", "c", "a"].map(String::from).to_vec())
        );
    }

    #[test]
    fn phase1_topological_order_empty_when_cyclic() {
        // gonum returns Unorderable for a cyclic graph and Go leaves
        // TopologicalOrder nil, i.e. JSON null (graph.go:1951-1954).
        let issues = vec![
            issue_with_blocking_deps("a", &["b"]),
            issue_with_blocking_deps("b", &["a"]),
        ];
        let g = build_graph(&issues);
        let p1 = analyze_phase1(&g);
        assert_eq!(p1.topological_order, None);
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

#[cfg(test)]
mod config_for_size_tests {
    use super::*;
    use serde_json::Value;

    /// The three tiers that appear in the frozen corpus, pinned to the exact
    /// `analysis_config` objects Go emitted for them. Read from
    /// `golden/{medium_tree,large_cyclic_600,xl_2500}____robot_insights.json`.
    fn golden(name: &str) -> Value {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("repo root");
        let txt = std::fs::read_to_string(
            repo.join("golden")
                .join(format!("{name}____robot_insights.json")),
        )
        .expect("golden readable");
        serde_json::from_str::<Value>(&txt).expect("golden parses")["analysis_config"].clone()
    }

    fn assert_matches_golden(node_count: usize, edge_count: usize, density: f64, name: &str) {
        let got = serde_json::to_value(config_for_size(node_count, edge_count, density))
            .expect("serializes");
        let want = golden(name);
        for (k, wv) in want.as_object().expect("golden object") {
            let gv = got
                .get(k)
                .unwrap_or_else(|| panic!("{name}: missing field {k}"));
            assert_eq!(gv, wv, "{name}: field {k}");
        }
        // No field beyond Go's set may appear — extra fields are a parity break.
        for k in got.as_object().expect("serialized object").keys() {
            assert!(
                want.get(k).is_some(),
                "{name}: Rust emitted extra field {k} that Go does not"
            );
        }
    }

    #[test]
    fn medium_tier_matches_go() {
        // medium_tree: 121 issues -> 100..499 tier.
        assert_matches_golden(121, 120, 0.0, "medium_tree");
    }

    #[test]
    fn large_tier_matches_go() {
        // large_cyclic_600: 600 issues, sparse -> approximate, sample 100.
        assert_matches_golden(600, 900, 0.0025, "large_cyclic_600");
    }

    #[test]
    fn xl_tier_matches_go() {
        // xl_2500: 2500 issues -> approximate sample 200, cycles skipped.
        assert_matches_golden(2500, 5000, 0.0008, "xl_2500");
    }

    #[test]
    fn small_tier_is_exact_with_two_second_budgets() {
        let c = config_for_size(12, 11, 0.0);
        assert_eq!(c.betweenness_mode, "exact");
        assert_eq!(c.betweenness_timeout_ns, 2_000_000_000);
        assert_eq!(c.max_cycles_to_store, 1000);
        assert!(c.compute_cycles && c.compute_hits);
    }

    #[test]
    fn large_dense_graph_skips_betweenness() {
        let c = config_for_size(800, 6000, 0.02);
        assert!(!c.compute_betweenness);
        assert_eq!(c.betweenness_mode, "skip");
        assert_eq!(c.betweenness_timeout_ns, 0);
        assert_eq!(
            c.betweenness_skip_reason,
            "graph too dense (density > 0.01)"
        );
    }

    #[test]
    fn xl_dense_graph_skips_hits_but_keeps_betweenness() {
        let c = config_for_size(3000, 9000, 0.01);
        assert!(c.compute_betweenness);
        assert_eq!(c.betweenness_mode, "approximate");
        assert!(!c.compute_hits);
        assert_eq!(c.hits_skip_reason, "graph too large and dense");
        assert_eq!(c.hits_timeout_ns, 0);
    }

    #[test]
    fn recommend_sample_size_matches_go_tiers() {
        let b = AnalysisBudget::default();
        assert_eq!(b.recommend_sample_size(0, 0), 0);
        assert_eq!(b.recommend_sample_size(12, 11), 12);
        assert_eq!(b.recommend_sample_size(121, 120), 50);
        assert_eq!(b.recommend_sample_size(600, 900), 100);
        assert_eq!(b.recommend_sample_size(2500, 5000), 200);
        // 1000/5 = 200 beats the floor of 50.
        assert_eq!(b.recommend_sample_size(400, 300), 80);
    }

    #[test]
    fn tier_boundaries_are_exact() {
        assert_eq!(config_for_size(99, 0, 0.0).max_cycles_to_store, 1000);
        assert_eq!(config_for_size(100, 0, 0.0).max_cycles_to_store, 100);
        assert_eq!(config_for_size(499, 0, 0.0).max_cycles_to_store, 100);
        assert_eq!(config_for_size(500, 0, 0.0).max_cycles_to_store, 50);
        assert_eq!(config_for_size(1999, 0, 0.0).max_cycles_to_store, 50);
        assert_eq!(config_for_size(2000, 0, 0.0).max_cycles_to_store, 10);
        // Cycles flip off exactly at the XL boundary.
        assert!(config_for_size(1999, 0, 0.0).compute_cycles);
        assert!(!config_for_size(2000, 0, 0.0).compute_cycles);
    }
}
