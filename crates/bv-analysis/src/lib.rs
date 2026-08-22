//! bv-analysis: graph algorithms (imported verbatim from the upstream wasm
//! crate), two-phase analyzer, cache, scoring. See plan §4.3.
//!
//! PARITY RULE: algorithm modules are included by #[path] from
//! crates/bv-graph-wasm/src/algorithms — the ONLY change is that crate::graph
//! resolves to our native DiGraph shim (same layout/API, no wasm-bindgen).

pub mod algorithms;
pub mod analyzer;
pub mod graph;
#[path = "../../bv-graph-wasm/src/reachability.rs"]
pub mod reachability;

pub use graph::DiGraph;
