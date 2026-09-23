//! bv-robot: robot command registry, envelope, TOON/JSON encoding.

pub mod docs;
pub mod envelope;
pub mod schema;

pub use envelope::{
    authority_hash, encode_payload, scope_hash, OutputFormat, RobotEnvelope, RobotLoadStats,
    RobotScope, RobotSourceAuthority, RobotSourceReport,
};

pub const ROBOT_CONTRACT_VERSION: &str = envelope::ROBOT_CONTRACT_VERSION;
