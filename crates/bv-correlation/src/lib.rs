//! bv-correlation: bead↔commit correlation engine.
//! Phase 4a slice: event extraction from git log (legacy patch path).

pub mod extractor;
pub mod extractor_snapshot;

pub use extractor::{extract, parse_log_output, BeadEvent, EventType, ExtractOptions};

pub mod explicit;
pub mod feedback;
pub mod scorer;
pub mod temporal;

/// The `--id-pattern` registry (Go `SetCustomIDPatterns` / `CustomIDPatterns`,
/// `pkg/correlation/explicit.go:41-60`). The CLI compiles every `--id-pattern`
/// occurrence and calls [`set_custom_id_patterns`] once at startup, before any
/// dispatch, so every message-based ID matcher sees the same list.
pub use explicit::{custom_id_patterns, set_custom_id_patterns};

pub mod causality;
pub mod cocommit;
pub mod correlator;
pub mod file_index;
pub mod history;
pub mod network;
pub mod orphan;
pub mod readiness;
pub mod related;
