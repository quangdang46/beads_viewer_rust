//! Algorithm modules included VERBATIM from crates/bv-graph-wasm/src/algorithms
//! via #[path]. Do not edit here — edit the source of truth in the wasm copy
//! (or, post-Phase-9, the extracted bv-graph-core).

#[path = "../../../bv-graph-wasm/src/algorithms/pagerank.rs"]
pub mod pagerank;

#[path = "../../../bv-graph-wasm/src/algorithms/betweenness.rs"]
pub mod betweenness;

#[path = "../../../bv-graph-wasm/src/algorithms/cycles.rs"]
pub mod cycles;

#[path = "../../../bv-graph-wasm/src/algorithms/critical_path.rs"]
pub mod critical_path;

#[path = "../../../bv-graph-wasm/src/algorithms/topo.rs"]
pub mod topo;

#[path = "../../../bv-graph-wasm/src/algorithms/subgraph.rs"]
pub mod subgraph;

#[path = "../../../bv-graph-wasm/src/whatif.rs"]
pub mod whatif;
