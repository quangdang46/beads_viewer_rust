//! Pure-Rust graph algorithms extracted from the upstream wasm crate.
//! No wasm-bindgen dependency — usable natively and from wasm wrappers.

pub mod algorithms;
pub mod graph;
pub mod reachability;
pub mod whatif;

pub use graph::DiGraph;

// Re-export key algorithm functions
pub use algorithms::betweenness::{betweenness, betweenness_approx};
pub use algorithms::critical_path::{
    critical_path_heights, critical_path_length, critical_path_nodes,
};
pub use algorithms::cycles::{has_cycles, tarjan_scc};
pub use algorithms::eigenvector::{eigenvector, eigenvector_default, EigenvectorConfig};
pub use algorithms::hits::{hits, hits_default, HITSConfig};
pub use algorithms::kcore::{degeneracy, kcore};
pub use algorithms::pagerank::{pagerank, pagerank_default, PageRankConfig};
pub use algorithms::slack::{slack, total_float};
pub use reachability::{reachable_from, reachable_to};

/// Initialize panic hook (native no-op; wasm wrapper handles browser hook).
pub fn init() {}
