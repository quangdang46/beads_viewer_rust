//! Go-semantics parity checks for the imported algorithms (Phase 2a).
use bv_analysis::algorithms::pagerank::{pagerank, pagerank_default, PageRankConfig};
use bv_analysis::DiGraph;

/// FIX-1..FIX-12 chain: FIX-k depends on FIX-(k-1); edge k -> k-1.
fn small_chain() -> DiGraph {
    let mut g = DiGraph::with_capacity(12, 11);
    for i in 1..=12 {
        g.add_node(&format!("FIX-{i}"));
    }
    for i in 2..=12 {
        g.add_edge(i - 1, i);
    }
    g
}

#[test]
fn pagerank_default_uses_go_max_iterations() {
    assert_eq!(
        PageRankConfig::default().max_iterations,
        1000,
        "must match Go computePageRank maxIterations (graph.go:2795)"
    );
    assert_eq!(PageRankConfig::default().tolerance, 1e-6);
}

#[test]
fn pagerank_scores_sum_to_one_and_are_deterministic() {
    let g = small_chain();
    let scores = pagerank_default(&g);
    assert_eq!(scores.len(), 12);
    let sum: f64 = scores.iter().sum();
    assert!((sum - 1.0).abs() < 1e-6, "scores sum to {sum}");
    // determinism: same input twice -> identical output
    let again = pagerank_default(&g);
    assert_eq!(scores, again);
}

#[test]
fn pagerank_explicit_config_matches_default() {
    let g = small_chain();
    let cfg = PageRankConfig {
        damping: 0.85,
        tolerance: 1e-6,
        max_iterations: 1000,
    };
    let explicit = pagerank(&g, &cfg);
    let def = pagerank_default(&g);
    assert_eq!(
        explicit, def,
        "default must equal Go-aligned explicit config"
    );
}
