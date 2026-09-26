//! `--related-max-results` parity with Go v0.25.0.
//!
//! Go registers it as a pflag `flag.Int` with default 10
//! (beads_viewer/cmd/bv/main.go:1584) and hands it to the correlation engine
//! at robot_registry.go:3280-3281:
//!
//! ```go
//! if cfg.RelatedMaxResults != nil { options.MaxResults = *cfg.RelatedMaxResults }
//! ```
//!
//! `RelatedWorkOptions.MaxResults` is documented at pkg/correlation/related.go:51
//! as "Maximum results per category (0 = unlimited)" and is applied
//! independently to all four categories — `file_overlap`, `commit_overlap`,
//! `dependency_cluster` and `concurrent` — at related.go:216-218, 287-289,
//! 383-385 and 500-502, each behind the same `if opts.MaxResults > 0` guard.
//!
//! The per-category cap and the 0-means-unlimited rule were already correct.
//! What was wrong is pflag's refusal to parse: Go converts the value during
//! flag parsing, so a value `strconv.ParseInt(s, 0, 64)` cannot read is a parse
//! error, not a silent fall back to the default.

use std::process::Command;

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// A real bead from the workspace's own `.beads/issues.jsonl`, so the
/// correlation report is built and the four category fields are serialised.
/// `--db` is pinned to the same JSONL on both binaries: the Go oracle prefers
/// `beads.db` when it is fresher, while `bvr` always prefers the JSONL.
const BEAD: &str = "beads_viewer_rust-p1-model-loader-pva";

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

/// `--robot-related <bead>` with `--related-max-results=<limit>`, Go's `=VALUE`
/// spelling. Both are read by the same `flag_value` scan, so the space form
/// needs its own case below.
fn with_limit(limit: &str) -> (i32, String, String) {
    bvr(&[
        "--robot-related",
        BEAD,
        &format!("--related-max-results={limit}"),
        "--json",
    ])
}

fn payload(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).expect("stdout is one JSON object")
}

/// Go's pflag rejects a value its `strconv.ParseInt(s, 0, 64)` cannot read,
/// printing `invalid argument %q for "--related-max-results" flag: %v` and
/// exiting 1 (main.go:4542-4550). Rust used to swallow the same input and
/// silently fall back to the default of 10, so `bv --robot-related <bead>
/// --related-max-results abc` answered with a full payload and exit 0.
#[test]
fn unparsable_limit_is_pflags_parse_error_at_exit_one() {
    let (code, stdout, stderr) = with_limit("abc");
    assert_eq!(code, 1, "stdout: {stdout}");
    assert_eq!(
        stderr.trim(),
        "invalid argument \"abc\" for \"--related-max-results\" flag: \
         strconv.ParseInt: parsing \"abc\": invalid syntax"
    );
    assert!(stdout.is_empty(), "no payload is emitted: {stdout}");
}

/// The other two inputs a user actually produces: an empty value and an
/// integer that overflows int64. Go reports `strconv.ParseInt`'s own error,
/// not a Rust-specific message.
#[test]
fn empty_and_out_of_range_limits_report_go_strerror() {
    let (code, _, stderr) = with_limit("");
    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"\" for \"--related-max-results\" flag: \
         strconv.ParseInt: parsing \"\": invalid syntax"
    );

    let (code, _, stderr) = with_limit("99999999999999999999");
    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"99999999999999999999\" for \"--related-max-results\" flag: \
         strconv.ParseInt: parsing \"99999999999999999999\": value out of range"
    );
}

/// A float is not an int: `--related-max-results 1.5` is a parse error, not a
/// truncation to 1.
#[test]
fn fractional_limit_is_a_parse_error() {
    let (code, _, stderr) = with_limit("1.5");
    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"1.5\" for \"--related-max-results\" flag: \
         strconv.ParseInt: parsing \"1.5\": invalid syntax"
    );
}

/// pflag reads the value with base 0, so `0x3`, `0b11`, `0o17` and a bare
/// leading `0` (octal) are all valid and a leading `+` is allowed. A naive
/// `str::parse::<i64>()` rejects every one of them.
#[test]
fn limit_is_parsed_with_go_base_zero_semantics() {
    for raw in ["0x3", "0b11", "0o17", "010", "+5", "1_0", "3", "20"] {
        let (code, stdout, stderr) = with_limit(raw);
        assert_eq!(code, 0, "--related-max-results={raw} stderr: {stderr}");
        assert!(
            stdout.contains("total_related"),
            "--related-max-results={raw} must produce a payload: {stdout}"
        );
    }
}

/// Go's guard is `if opts.MaxResults > 0`, so 0 means UNLIMITED rather than
/// "emit nothing", and a negative value fails the same guard. Rust floors
/// negatives to 0 purely to keep the `usize` cast in range, which changes no
/// output. Neither may be rejected, and neither may serialise the categories as
/// empty arrays — Go's nil accumulator becomes JSON `null`
/// (related.go:216-218 and its three siblings).
#[test]
fn zero_and_negative_are_unlimited_not_empty() {
    for raw in ["0", "-3", "-0"] {
        let (code, stdout, stderr) = with_limit(raw);
        assert_eq!(code, 0, "--related-max-results={raw} stderr: {stderr}");
        let p = payload(&stdout);
        for category in [
            "file_overlap",
            "commit_overlap",
            "dependency_cluster",
            "concurrent",
        ] {
            assert!(
                p[category].is_null(),
                "--related-max-results={raw}: {category} must stay null, got {}",
                p[category]
            );
        }
    }
}

/// With the flag absent the limit is Go's registered default of 10
/// (main.go:1584), and the payload is the same shape 0 produces.
#[test]
fn absent_flag_uses_go_default_of_ten() {
    let (code, stdout, stderr) = bvr(&["--robot-related", BEAD, "--json"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let p = payload(&stdout);
    assert_eq!(p["total_related"], 0);
    for category in [
        "file_overlap",
        "commit_overlap",
        "dependency_cluster",
        "concurrent",
    ] {
        assert!(p[category].is_null(), "{category} must be null: {p}");
    }
}

/// Go's pflag parses every registered flag for every command, so the rejection
/// happens before cobra reaches the help flag, `--version`, or the
/// modifier-requires table — all of which `main` also short-circuits ahead of
/// the `--robot-related` handler. A handler-local check would report
/// `--related-max-results requires --robot-related` and exit 1, which is the
/// right code for the wrong reason.
#[test]
fn parse_error_precedes_help_version_and_modifier_requires() {
    let (code, stdout, stderr) = bvr(&["--related-max-results=abc", "--json"]);
    assert_eq!(code, 1, "stdout: {stdout}");
    assert!(
        stderr.starts_with("invalid argument \"abc\" for \"--related-max-results\" flag:"),
        "expected the pflag parse error, got: {stderr}"
    );

    let (code, _, stderr) = bvr(&["--version", "--related-max-results=abc"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid argument"), "stderr: {stderr}");

    let (code, _, stderr) = bvr(&["--help", "--related-max-results=abc"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("invalid argument"),
        "Go fails the parse before printing help; stderr: {stderr}"
    );

    // A primary that does not want the flag at all: still a parse error, not
    // the modifier-requires message.
    let (code, _, stderr) = bvr(&["--robot-triage", "--related-max-results=abc", "--json"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid argument"), "stderr: {stderr}");
}

/// The space form reads the next argv token as the value, exactly as pflag
/// does, so it takes the same parse path.
#[test]
fn space_separated_form_is_parsed_too() {
    let (code, _, stderr) = bvr(&[
        "--robot-related",
        BEAD,
        "--related-max-results",
        "abc",
        "--json",
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"abc\" for \"--related-max-results\" flag: \
         strconv.ParseInt: parsing \"abc\": invalid syntax"
    );

    let (code, stdout, stderr) = bvr(&[
        "--robot-related",
        BEAD,
        "--related-max-results",
        "3",
        "--json",
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("total_related"), "{stdout}");
}

/// A well-formed limit is not a modifier-requires violation once
/// `--robot-related` is present, and the categories still serialise as Go's nil
/// `null` rather than Rust's `[]`.
#[test]
fn well_formed_limit_with_its_primary_is_accepted() {
    let (code, stdout, stderr) =
        bvr(&["--robot-related", BEAD, "--related-max-results=5", "--json"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(payload(&stdout)["total_related"], 0);
}
