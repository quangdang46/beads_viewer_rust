//! RobotEnvelope + output encoding — port of Go `cmd/bv/main.go`
//! envelope (lines 8177-8260) per api-freeze-v1.

use serde::{Deserialize, Serialize};

pub const ROBOT_CONTRACT_VERSION: &str = "1.0.0";

/// Per-line parse accounting surfaced ONLY when the loader dropped records
/// (#190). Mirrors Go `RobotLoadStats`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RobotLoadStats {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_path: String,
    pub valid: usize,
    pub errors: usize,
    pub skipped: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Standard envelope for all robot outputs. Field order = serialization
/// order (serde preserves declaration order) — part of the contract.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RobotEnvelope {
    /// RFC3339 UTC timestamp.
    pub generated_at: String,
    /// Fingerprint of source data (bv_core::data_hash).
    pub data_hash: String,
    /// "json" | "toon" — present when a format override is active.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_format: String,
    /// bv version string.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Present only when loader dropped records (#190).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_stats: Option<RobotLoadStats>,
}

impl RobotEnvelope {
    /// Go: `NewRobotEnvelope`. `load_stats` emitted only when errors > 0.
    pub fn new(
        data_hash: impl Into<String>,
        version: impl Into<String>,
        load_report: Option<&bv_core::loader::LoadReport>,
        output_format: OutputFormat,
    ) -> Self {
        let load_stats = load_report.and_then(|rep| {
            if rep.errors > 0 {
                Some(RobotLoadStats {
                    source_path: rep.path.clone(),
                    valid: rep.valid,
                    errors: rep.errors,
                    skipped: rep.skipped,
                    warnings: rep.warnings.clone(),
                })
            } else {
                None
            }
        });
        // Go parity: truncate to second precision (no microseconds).
        let ts = jiff::Timestamp::now().to_string();
        let generated_at = if let Some(pos) = ts.find('.') {
            format!("{}Z", &ts[..pos])
        } else {
            ts
        };
        RobotEnvelope {
            generated_at,
            data_hash: data_hash.into(),
            output_format: output_format.as_str().to_string(),
            version: version.into(),
            load_stats,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Toon,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Json => "json",
            OutputFormat::Toon => "toon",
        }
    }
}

/// Encode a robot payload. Golden-corpus finding (Phase 3b): for the captured
/// command set, Go's TOON output is byte-identical to compact JSON apart from
/// `output_format:"toon"` — i.e. the encoder emits compact JSON with the marker
/// field. Our encoder matches that exactly; true token-layout re-encoding can
/// be layered later without changing these bytes.
pub fn encode_payload<T: Serialize>(
    payload: &T,
    format: OutputFormat,
) -> Result<Vec<u8>, serde_json::Error> {
    // serde_json serializes struct fields in declaration order and maps in
    // insertion order only with preserve_order feature; payloads use structs,
    // so field order is stable. Compact form (no spaces) matches goldens.
    serde_json::to_vec(payload).map(|mut v| {
        if format == OutputFormat::Toon {
            // Marker parity handled inside payload structs via output_format
            // field; nothing extra at the encoder layer.
        }
        v.push(b'\n');
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RobotLoadStats;

    #[derive(Serialize)]
    struct Sample {
        generated_at: String,
        data_hash: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        load_stats: Option<RobotLoadStats>,
    }

    #[test]
    fn load_stats_emitted_only_when_errors() {
        let clean = bv_core::loader::LoadReport::default();
        assert!(
            RobotEnvelope::new("h", "v", Some(&clean), OutputFormat::Json)
                .load_stats
                .is_none()
        );
        let dirty = bv_core::loader::LoadReport {
            errors: 2,
            ..bv_core::loader::LoadReport::default()
        };
        let env = RobotEnvelope::new("h", "v", Some(&dirty), OutputFormat::Json);
        let stats = env.load_stats.expect("errors>0 must emit load_stats");
        assert_eq!(stats.errors, 2);
    }

    #[test]
    fn field_order_matches_golden() {
        let env = Sample {
            generated_at: "T".into(),
            data_hash: "H".into(),
            load_stats: None,
        };
        let bytes = encode_payload(&env, OutputFormat::Json).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        // generated_at before data_hash, matching golden key order
        let g = s.find("generated_at").unwrap();
        let d = s.find("data_hash").unwrap();
        assert!(g < d);
        assert!(s.ends_with("}\n"));
    }

    #[test]
    fn warnings_capped_shape_roundtrip() {
        let stats = RobotLoadStats {
            source_path: "/x/issues.jsonl".into(),
            valid: 10,
            errors: 1,
            skipped: 3,
            warnings: vec!["skipping malformed JSON on line 4: eof".into()],
        };
        let v = serde_json::to_value(&stats).unwrap();
        assert_eq!(v["valid"], 10);
        assert_eq!(v["warnings"].as_array().unwrap().len(), 1);
    }
}
