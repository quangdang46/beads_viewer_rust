//! `argv::normalize_flag_spelling` — Go's `rewriteSingleDashLongFlags`
//! (main.go:533-553) decides whether `-foo` is a long flag with
//! `flags.Lookup(name) == nil`, never with a test on the shape of the name.
//!
//! The name-shape test that this replaces read any multi-character
//! alphanumeric token as a flag, so a *value* that happened to start with `-`
//! was rewritten along with it. `--relations-threshold -1e400` reached Rust as
//! `--relations-threshold --1e400` and was rejected with
//!
//! ```text
//! invalid argument "--1e400" for "--relations-threshold" flag:
//!     strconv.ParseFloat: parsing "--1e400": invalid syntax
//! ```
//!
//! where Go reports the failure against the value
//!
//! ```text
//! invalid argument "-1e400" for "--relations-threshold" flag:
//!     strconv.ParseFloat: parsing "-1e400": value out of range
//! ```
//!
//! and `--relations-threshold -Inf`, which Go accepts and floors to the 0.5
//! default (`pkg/correlation/file_index.go:484-486`), failed outright. Every
//! value-taking flag that can hold a negative number was affected, so this file
//! drives the space-separated spelling — the one that goes through the
//! rewriter — rather than `--flag=value`.

use std::process::Command;

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bvr"))
        .current_dir(REPO_ROOT)
        .args(args)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn relations(threshold: &str) -> (i32, String, String) {
    run(&[
        "--db",
        ".beads/issues.jsonl",
        "--robot-file-relations",
        "README.md",
        "--relations-threshold",
        threshold,
    ])
}

/// A value that is not a flag must survive normalization, so the error Go's
/// value parser produces is the one that surfaces — naming the value, not a
/// flag spelling derived from it.
#[test]
fn unparseable_negative_value_is_reported_against_the_value() {
    for (value, reason) in [
        ("-1e400", "value out of range"),
        ("-NaN", "invalid syntax"),
        ("-1e-4000x", "invalid syntax"),
    ] {
        let (code, stdout, stderr) = relations(value);
        assert_eq!(code, 1, "{value}: stdout {stdout}");
        assert_eq!(
            stderr,
            format!(
                "invalid argument \"{value}\" for \"--relations-threshold\" flag: \
                 strconv.ParseFloat: parsing \"{value}\": {reason}\n"
            ),
            "Go's message must name the value {value}, not a flag derived from it"
        );
    }
}

/// Go's `strconv.ParseFloat` accepts the `inf` spellings and
/// `GetRelatedFiles` floors anything `<= 0` back to the 0.5 default, so these
/// are successful runs whose `threshold` is exactly the default.
#[test]
fn negative_infinities_floor_to_the_default_threshold() {
    for value in ["-Inf", "-inf", "-1e-400", "-2", "-0.5", "-0"] {
        let (code, stdout, stderr) = relations(value);
        assert_eq!(code, 0, "{value}: stderr {stderr}");
        let payload: serde_json::Value =
            serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{value}: {e}\n{stdout}"));
        assert_eq!(
            payload["threshold"], 0.5,
            "{value} is <= 0 and floors to the 0.5 default"
        );
    }
}

/// The rewriter classifies each token independently, so a single-dash *flag*
/// followed by a negative value has to work in one invocation: the flag is
/// still recognised and doubled, the value is still left alone.
#[test]
fn single_dash_flag_alongside_a_negative_value() {
    let (code, _, stderr) = run(&[
        "--db",
        ".beads/issues.jsonl",
        "-robot-file-relations",
        "README.md",
        "--relations-threshold",
        "-1e400",
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        stderr,
        "invalid argument \"-1e400\" for \"--relations-threshold\" flag: \
         strconv.ParseFloat: parsing \"-1e400\": value out of range\n"
    );
}

/// A non-flag value that is not a number either — the same class of token, one
/// step further from a flag name.
#[test]
fn flag_shaped_value_is_not_rewritten() {
    let (code, _, stderr) = relations("-1e400x");
    assert_eq!(code, 1);
    assert!(
        stderr.contains("parsing \"-1e400x\": invalid syntax"),
        "the value must be the thing that failed: {stderr}"
    );
    assert!(
        !stderr.contains("--1e400x"),
        "a value must never be turned into a flag: {stderr}"
    );
}
