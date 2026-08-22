//! bv-robot: robot command registry, envelope, TOON/JSON encoding.

pub mod envelope;

pub use envelope::{encode_payload, OutputFormat, RobotEnvelope, RobotLoadStats};

pub const ROBOT_CONTRACT_VERSION: &str = envelope::ROBOT_CONTRACT_VERSION;
