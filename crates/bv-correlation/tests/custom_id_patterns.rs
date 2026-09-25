//! The `--id-pattern` registry as the CLI sees it (Go `SetCustomIDPatterns` /
//! `CustomIDPatterns`, `pkg/correlation/explicit.go:41-60`).
//!
//! This lives in an integration test on purpose: the registry is a
//! process-global, so the CLI writes to it once at startup while every
//! message-based ID matcher reads it. Exercising it here means the mutation
//! cannot leak into the unit-test fixtures in the library, which share a
//! process with each other but not with this binary.

use bv_correlation::{custom_id_patterns, set_custom_id_patterns};

/// One test, because the registry is per-process and the runner is parallel:
/// splitting these steps across tests would have them race on the same global.
#[test]
fn registry_round_trips_in_registration_order() {
    assert!(
        custom_id_patterns().is_empty(),
        "unset until the CLI compiles --id-pattern"
    );

    let first = regex::Regex::new(r"\bzzq-[a-z0-9]{5}\b").unwrap();
    let second = regex::Regex::new(r"(?i)ticket\s+(zzt-[a-z]{3})\b").unwrap();
    set_custom_id_patterns(vec![first.clone(), second.clone()]);

    let got = custom_id_patterns();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].as_str(), first.as_str());
    assert_eq!(got[1].as_str(), second.as_str());

    // Go's `SetCustomIDPatterns` replaces rather than appends, and an empty
    // slice clears the list -- the shape a CLI run with no --id-pattern sees.
    set_custom_id_patterns(Vec::new());
    assert!(custom_id_patterns().is_empty());
}
