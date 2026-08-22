//! bv-analysis: two-phase analyzer, cache, scoring, drift.
//! Graph algorithms come from bv-graph-core (extracted from upstream wasm crate).

pub mod analyzer;
pub mod cache;
pub mod drift;
pub mod impact;
pub mod scoring;
pub mod triage;

pub use bv_graph_core::DiGraph;

// Re-export algorithm modules for convenience
pub use bv_graph_core::algorithms;
