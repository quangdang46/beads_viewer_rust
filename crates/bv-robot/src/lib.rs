//! bv-robot: robot command registry, envelope, TOON/JSON encoding.

pub mod envelope;

pub use envelope::{encode_payload, OutputFormat, RobotEnvelope, RobotLoadStats};
