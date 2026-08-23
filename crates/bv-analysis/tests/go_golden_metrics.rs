//! Differential tests against Go golden metric files.
use bv_graph_core::DiGraph;

fn load_expected(name: &str) -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/go_expected/"
    );
    let raw = std::fs::read_to_string(format!("{path}{name}_metrics.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn get_f64(v: &serde_json::Value, key: &str, id: &str) -> f64 {
    v[key][id].as_f64().unwrap_or(0.0)
}

fn chain(n: usize) -> DiGraph {
    let mut g = DiGraph::with_capacity(n, n - 1);
    for i in 0..n {
        g.add_node(&format!("n{i}"));
    }
    for i in 0..n.saturating_sub(1) {
        g.add_edge(i, i + 1);
    }
    g
}

#[test]
fn chain_10_pagerank_matches_go() {
    let go = load_expected("chain_10");
    let g = chain(10);
    let pr = bv_graph_core::pagerank_default(&g);
    assert_eq!(g.len(), go["node_count"].as_u64().unwrap() as usize);
    for (i, &score) in pr.iter().enumerate() {
        let id = format!("n{i}");
        let expected = get_f64(&go, "pagerank", &id);
        assert!(
            (score - expected).abs() < 0.01,
            "pagerank[{id}] rust={score:.6} go={expected:.6}"
        );
    }
}

#[test]
fn chain_10_density_matches_go() {
    let go = load_expected("chain_10");
    let g = chain(10);
    let n = g.len() as f64;
    let density = if n <= 1.0 {
        0.0
    } else {
        g.edge_count() as f64 / (n * (n - 1.0))
    };
    assert!((density - go["density"].as_f64().unwrap()).abs() < 0.001);
}

#[test]
fn star_10_pagerank_matches_go() {
    let go = load_expected("star_10");
    // Star: center n0 → n1..n9
    let mut g = DiGraph::with_capacity(10, 9);
    for i in 0..10 {
        g.add_node(&format!("n{i}"));
    }
    for i in 1..10 {
        g.add_edge(i, 0);
    }
    let pr = bv_graph_core::pagerank_default(&g);
    assert_eq!(g.len(), go["node_count"].as_u64().unwrap() as usize);
    for (i, &score) in pr.iter().enumerate() {
        let id = format!("n{i}");
        let expected = get_f64(&go, "pagerank", &id);
        assert!(
            (score - expected).abs() < 0.01,
            "pagerank[{id}] rust={score:.6} go={expected:.6}"
        );
    }
}

#[test]
fn cycle_5_density_matches_go() {
    let go = load_expected("cycle_5");
    let mut g = DiGraph::with_capacity(5, 5);
    for i in 0..5 {
        g.add_node(&format!("n{i}"));
    }
    for i in 0..5 {
        g.add_edge(i, (i + 1) % 5);
    }
    let n = g.len() as f64;
    let density = if n <= 1.0 {
        0.0
    } else {
        g.edge_count() as f64 / (n * (n - 1.0))
    };
    assert!((density - go["density"].as_f64().unwrap()).abs() < 0.001);
}

#[test]
fn diamond_5_edge_count_matches_go() {
    let go = load_expected("diamond_5");
    let mut g = DiGraph::new();
    for i in 0..5 {
        g.add_node(&format!("n{i}"));
    }
    // Diamond shape: n0→n1,n0→n2,n1→n3,n2→n3,n3→n4
    g.add_edge(0, 1);
    g.add_edge(0, 2);
    g.add_edge(1, 3);
    g.add_edge(2, 3);
    g.add_edge(3, 4);
    assert_eq!(g.edge_count(), go["edge_count"].as_u64().unwrap() as usize);
}
