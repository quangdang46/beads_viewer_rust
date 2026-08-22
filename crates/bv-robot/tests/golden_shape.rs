//! Golden-shape test: envelope field order and presence semantics must
//! match captured Go goldens (golden/selfrepo____robot_next.json).
use bv_robot::{encode_payload, OutputFormat, RobotLoadStats};

#[derive(serde::Serialize)]
struct NextPayload {
    generated_at: String,
    data_hash: String,
    output_format: String,
    version: String,
    actionable: bool,
    phase2_ready: bool,
}

#[test]
fn next_payload_key_order_matches_golden() {
    let p = NextPayload {
        generated_at: "T".into(),
        data_hash: "H".into(),
        output_format: "toon".into(),
        version: "v0.20.0".into(),
        actionable: true,
        phase2_ready: true,
    };
    let bytes = encode_payload(&p, OutputFormat::Toon).unwrap();
    let s = String::from_utf8(bytes).unwrap().trim_end().to_string();
    let expected_prefix = r#"{"generated_at":"T","data_hash":"H","output_format":"toon","version":"v0.20.0","actionable":true,"phase2_ready":true}"#;
    assert_eq!(s, expected_prefix);
}

#[test]
fn load_stats_present_only_on_errors() {
    // Golden contract: load_stats absent from clean loads.
    let rep = bv_core::loader::LoadReport {
        valid: 33,
        ..bv_core::loader::LoadReport::default()
    };
    let env = bv_robot::RobotEnvelope::new("h", "v0.20.0", Some(&rep), OutputFormat::Json);
    let v = serde_json::to_value(&env).unwrap();
    assert!(v.get("load_stats").is_none());
    assert_eq!(v["output_format"], "json");

    let rep = bv_core::loader::LoadReport {
        errors: 1,
        warnings: vec!["boom".into()],
        ..bv_core::loader::LoadReport::default()
    };
    let env = bv_robot::RobotEnvelope::new("h", "v0.20.0", Some(&rep), OutputFormat::Json);
    let v = serde_json::to_value(&env).unwrap();
    let ls = v["load_stats"].as_object().expect("present on errors");
    assert_eq!(ls["errors"], 1);
    assert_eq!(ls["warnings"][0], "boom");
    let _ = RobotLoadStats::default(); // type anchor
}
