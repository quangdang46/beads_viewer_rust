//! bv-correlation: bead↔commit correlation engine.
//! Phase 4a slice: event extraction from git log (legacy patch path).

pub mod extractor;

pub use extractor::{extract, parse_log_output, BeadEvent, EventType, ExtractOptions};

pub mod explicit;
pub mod feedback;
pub mod scorer;
pub mod temporal;

pub mod causality;
pub mod cocommit;
pub mod correlator;
pub mod network;
pub mod orphan;
