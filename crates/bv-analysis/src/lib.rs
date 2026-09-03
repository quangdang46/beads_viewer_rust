//! bv-analysis: two-phase analyzer, cache, scoring, drift.
//! Graph algorithms come from bv-graph-core (extracted from upstream wasm crate).

pub mod analyzer;
pub mod cache;
pub mod drift;
pub mod impact;
pub mod label_health;
pub mod scoring;
pub mod triage;

pub use bv_graph_core::algorithms;
pub use bv_graph_core::DiGraph;

// Re-export analyzer types for downstream crates
pub use analyzer::build_graph;
pub use analyzer::{
    analyze_phase1, analyze_phase2_blocking, AnalysisBudget, GraphAnalysisPhase2, MetricStatus,
    Phase1Stats, StatusEntry,
};
pub use impact::{compute_impact_scores, ImpactInputs};
pub use triage::{build_triage, compute_blocked_set, TriageOutput};
