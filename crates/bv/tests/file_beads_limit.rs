//! `--file-beads-limit` parity with Go v0.25.0.
//!
//! Go registers it as a pflag `flag.Int` with default 20
//! (beads_viewer/cmd/bv/main.go:1564) and consumes it in
//! `handleRobotFileBeads` (robot_registry.go:3140-3148):
//!
//! ```go
//! closedLimit := 20
//! if cfg.FileBeadsLimit != nil { closedLimit = *cfg.FileBeadsLimit }
//! if closedLimit < 0 { closedLimit = 0 }
//! if len(result.ClosedBeads) > closedLimit { result.ClosedBeads = result.ClosedBeads[:closedLimit] }
//! ```
//!
//! Two behaviours matter and both were wrong before this file existed:
//! the negative floor, and pflag's refusal to parse a non-integer value.

use std::process::Command;

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// `--robot-file-beads <path>` in the workspace, so the correlation report is
/// built from real history and `closed_beads` is non-empty.
fn run_with_limit(limit: &str) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bvr"))
        .current_dir(REPO_ROOT)
        .args([
            "--robot-file-beads",
            "crates/bv/src/main.rs",
            &format!("--file-beads-limit={limit}"),
        ])
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn closed_beads(stdout: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(stdout).expect("stdout is one JSON object")
        ["closed_beads"]
        .as_array()
        .expect("closed_beads is an array")
        .clone()
}

/// Go's pflag rejects a value its `strconv.ParseInt(s, 0, 64)` cannot read,
/// printing `invalid argument %q for "--file-beads-limit" flag: %v` and exiting
/// 1 (main.go:4548). Rust used to swallow the same input and silently fall back
/// to the default of 20, so `bv --robot-file-beads README.md --file-beads-limit abc`
/// answered with 20 beads instead of failing.
#[test]
fn unparsable_limit_is_pflags_parse_error_at_exit_one() {
    let (code, stdout, stderr) = run_with_limit("abc");
    assert_eq!(code, 1, "stdout: {stdout}");
    assert_eq!(
        stderr.trim(),
        "invalid argument \"abc\" for \"--file-beads-limit\" flag: \
         strconv.ParseInt: parsing \"abc\": invalid syntax"
    );
    assert!(stdout.is_empty(), "no payload is emitted: {stdout}");
}

/// The same parse error for the two other inputs a user actually produces: an
/// empty value and an integer that overflows int64. Go reports
/// `strconv.ParseInt`'s own error, not a Rust-specific message.
#[test]
fn empty_and_out_of_range_limits_report_go_strerror() {
    let (code, _, stderr) = run_with_limit("");
    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"\" for \"--file-beads-limit\" flag: \
         strconv.ParseInt: parsing \"\": invalid syntax"
    );

    let (code, _, stderr) = run_with_limit("99999999999999999999");
    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"99999999999999999999\" for \"--file-beads-limit\" flag: \
         strconv.ParseInt: parsing \"99999999999999999999\": value out of range"
    );
}

/// pflag reads the value with `strconv.ParseInt(s, 0, 64)`, so `0x3` and `0b11`
/// are both 3 and a leading `+` is allowed. A naive decimal `str::parse::<i64>()`
/// rejects all three, and the silent-fallback-to-20 behaviour this file replaces
/// returned the untruncated 6-bead list for every one of them.
#[test]
fn limit_is_parsed_with_go_base_zero_semantics() {
    for (raw, want) in [("0x3", 3usize), ("0b11", 3), ("+3", 3), ("3", 3)] {
        let (code, stdout, stderr) = run_with_limit(raw);
        assert_eq!(code, 0, "--file-beads-limit={raw} stderr: {stderr}");
        assert_eq!(
            closed_beads(&stdout).len(),
            want,
            "--file-beads-limit={raw} must be read by strconv.ParseInt(s, 0, 64)"
        );
    }
}

/// Go floors a negative limit to 0 (`if closedLimit < 0 { closedLimit = 0 }`)
/// and then still runs `if len(...) > closedLimit`, so a negative value emits an
/// EMPTY ARRAY, not the whole list and not a slice panic. Rust's `.max(0)`
/// before the length comparison is what makes that safe.
#[test]
fn negative_limit_is_floored_to_zero_and_emits_empty_array() {
    let (code, stdout, stderr) = run_with_limit("-3");
    assert_eq!(code, 0, "stderr: {stderr}");
    let payload: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON object");
    assert_eq!(
        payload["closed_beads"],
        serde_json::json!([]),
        "a floored limit must serialise as [], not null"
    );
    // `total_beads` is read off the lookup result BEFORE the truncation
    // (robot_registry.go:3149-3153), so the total still counts the beads the
    // payload drops.
    assert!(
        payload["total_beads"].as_i64().unwrap_or(0) > 0,
        "total_beads is pre-truncation: {payload}"
    );
}

/// A positive limit below the closed-bead count truncates, and `total_beads`
/// stays at the untruncated number.
#[test]
fn positive_limit_truncates_but_leaves_total_beads_intact() {
    let (code, stdout, stderr) = run_with_limit("1");
    assert_eq!(code, 0, "stderr: {stderr}");
    let payload: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON object");
    assert_eq!(payload["closed_beads"].as_array().map(Vec::len), Some(1));
    let total = payload["total_beads"]
        .as_i64()
        .expect("total_beads is an int");
    assert!(total > 1, "total_beads counts pre-truncation: {payload}");
}
