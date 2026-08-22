//! bv-core: frozen data model, tolerant JSONL loader, datasource discovery.
//! Contracts here are api-freeze-v1 — see AGENTS.md and plan §3.1.

pub mod model;

pub use model::{Comment, Dependency, DependencyType, Issue, Status, ValidationError};

pub mod data_hash;
pub mod discovery;
pub mod loader;
pub mod sqlite;
