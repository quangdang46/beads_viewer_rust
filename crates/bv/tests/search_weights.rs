//! `--search-weights` parity with Go v0.25.0.
//!
//! Go registers it as a pflag `flag.String` (beads_viewer/cmd/bv/main.go:1531)
//! with the modifier-requires rule `--search` (main.go:1795) and hands it to
//! `resolveSearchConfig` at main.go:2831, which drops the inherited
//! `BV_SEARCH_WEIGHTS` when the flag is non-empty (search_output.go:91-93) and
//! then runs `applySearchConfigOverrides` (search_output.go:100):
//!
//! ```go
//! if weightsFlag != "" {
//!     weights, err := search.ParseWeightsJSON(weightsFlag)
//!     if err != nil { return search.SearchConfig{}, err }
//!     cfg.Weights = weights
//!     cfg.HasWeights = true
//! }
//! }
//! ```
//!
//! `ParseWeightsJSON` (pkg/search/config.go:129) decodes into
//! `map[string]float64`, requires all six keys, rejects unknown keys, and then
//! runs `Weights.Validate` (pkg/search/weights.go:30).
//!
//! The wiring, the required/unknown-key checks, the `custom` preset name and
//! the environment-suppression rule were all already correct. The defect this
//! file pins is in the decode step: `encoding/json` treats a JSON `null` for a
//! map element as a no-op that leaves the entry at its zero value, so
//! `{"text":null,...}` decodes to `TextRelevance == 0.0`, still reaches
//! `Validate`, and passes whenever the other five components sum to 1.0.
//! Decoding into `f64` directly rejected `null` outright, turning a payload Go
//! accepts into `invalid weights JSON` and exit 1 where Go exits 0.

use std::process::Command;

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn bvr(extra: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bvr"));
    cmd.current_dir(REPO_ROOT)
        .args(["--db", ".beads/issues.jsonl"])
        .args(extra);
    let out = cmd.output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Hybrid mode is what makes the resolved weights observable: Go only fills
/// the `preset` and `weights` output fields on the hybrid path
/// (search_output.go:42-43, both `omitempty`), and `--search-weights` alone
/// does not imply hybrid the way `--search-preset` does
/// (search_output.go:118-121 only covers presets).
///
/// The query is deliberately multi-word so `AdjustWeightsForQuery`
/// (query_adjust.go:45) leaves the weights untouched. A short query floors
/// `text` at 0.55 and rescales the other five, which would hide the value that
/// was actually decoded.
fn hybrid_search(weights: &str) -> (i32, String, String) {
    bvr(&[
        "--robot-search",
        "--search",
        "robot envelope export snapshot migration",
        "--search-mode",
        "hybrid",
        &format!("--search-weights={weights}"),
    ])
}

fn json_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":");
    let start = json.find(&needle)? + needle.len();
    Some(json[start..].split([',', '}']).next().unwrap_or(""))
}

#[test]
fn json_null_for_a_weight_is_the_go_zero_value_not_a_parse_error() {
    // Go: null -> 0.0, so the remaining components must sum to 1.0 for
    // Validate to pass. Go exits 0 and logs only the low-text-weight warning.
    let (code, stdout, _) = hybrid_search(
        r#"{"text":null,"pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.6}"#,
    );
    assert_eq!(code, 0, "Go accepts a null weight; stderr was {stdout}");
    assert_eq!(
        json_field(&stdout, "preset"),
        Some("\"custom\""),
        "explicit weights report the synthetic preset `custom`: {stdout}"
    );
    // Go marshals the zero float as a bare `0`.
    assert!(
        matches!(json_field(&stdout, "text"), Some("0") | Some("0.0")),
        "the null weight decodes to Go's zero value: {stdout}"
    );
}

#[test]
fn null_that_breaks_the_sum_is_still_rejected() {
    // text=0.0 leaves 0.5, so Validate fails exactly as it does in Go.
    let (code, _, stderr) = hybrid_search(
        r#"{"text":null,"pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.1}"#,
    );
    assert_eq!(code, 1, "a null weight does not exempt the sum check");
    assert!(
        stderr.contains("weights must sum to 1.0, got 0.500"),
        "Go's Validate wording: {stderr}"
    );
}

#[test]
fn missing_unknown_key_and_sum_errors_match_go() {
    // config.go:137-139 — all six keys are required.
    let (code, _, stderr) =
        hybrid_search(r#"{"text":0.5,"pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1}"#);
    assert_eq!(code, 1);
    assert!(
        stderr.contains(r#"weights JSON missing "recency""#),
        "{stderr}"
    );

    // config.go:142-144 — `isWeightKey` rejects anything else.
    let (code, _, stderr) = hybrid_search(
        r#"{"text":0.5,"pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.1,"bogus":0.0}"#,
    );
    assert_eq!(code, 1);
    assert!(
        stderr.contains(r#"weights JSON has unknown key "bogus""#),
        "{stderr}"
    );

    // weights.go:66-68 — components must be non-negative.
    let (code, _, stderr) = hybrid_search(
        r#"{"text":-1,"pagerank":0.5,"status":0.1,"impact":0.1,"priority":0.2,"recency":0.2}"#,
    );
    assert_eq!(code, 1);
    assert!(stderr.contains("weights must be non-negative"), "{stderr}");

    // weights.go:43-45 — the sum check, with Go's tolerance of 0.001.
    let (code, _, stderr) = hybrid_search(
        r#"{"text":0.9,"pagerank":0.9,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.1}"#,
    );
    assert_eq!(code, 1);
    assert!(
        stderr.contains("weights must sum to 1.0, got 2.200"),
        "{stderr}"
    );
}

#[test]
fn a_non_number_weight_is_still_a_type_error() {
    // Accepting null must not loosen the other JSON types: Go reports
    // "json: cannot unmarshal string into Go value of type float64".
    for bad in [
        r#"{"text":"0.5","pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.1}"#,
        r#"{"text":true,"pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.1}"#,
        r#"{"text":[1],"pagerank":0.1,"status":0.1,"impact":0.1,"priority":0.1,"recency":0.1}"#,
    ] {
        let (code, _, stderr) = hybrid_search(bad);
        assert_eq!(code, 1, "expected a type error for {bad}");
        assert!(
            stderr.contains("invalid weights JSON"),
            "for {bad}: {stderr}"
        );
    }
}

#[test]
fn the_flag_overrides_an_inherited_bv_search_weights() {
    // search_output.go:91-93 — a non-empty --search-weights discards the
    // inherited BV_SEARCH_WEIGHTS rather than layering on top of it.
    let weights =
        r#"{"text":0.7,"pagerank":0.1,"status":0.05,"impact":0.05,"priority":0.05,"recency":0.05}"#;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bvr"));
    cmd.current_dir(REPO_ROOT)
        .args(["--db", ".beads/issues.jsonl"])
        .args([
            "--robot-search",
            "--search",
            "robot envelope export snapshot migration",
            "--search-mode",
            "hybrid",
            &format!("--search-weights={weights}"),
        ])
        .env(
            "BV_SEARCH_WEIGHTS",
            r#"{"text":0.1,"pagerank":0.4,"status":0.1,"impact":0.1,"priority":0.2,"recency":0.1}"#,
        );
    let out = cmd.output().expect("binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert_eq!(
        json_field(&stdout, "text"),
        Some("0.7"),
        "the flag wins over BV_SEARCH_WEIGHTS: {stdout}"
    );
}

#[test]
fn search_weights_requires_search() {
    // main.go:1795 — {modifier: "search-weights", requires: ["search"]}. The
    // invocation also passes `--robot-search`, which the same table requires
    // `search` for, and that rule is listed first (main.go:1790). Go reports the
    // first broken rule and stops, so the message names `--robot-search`; the
    // oracle's stderr is byte-identical to ours on this argv.
    let (code, _, stderr) = bvr(&[
        "--robot-search",
        r#"--search-weights={"text":1.0,"pagerank":0.0,"status":0.0,"impact":0.0,"priority":0.0,"recency":0.0}"#,
    ]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("--robot-search requires --search"),
        "{stderr}"
    );
}
