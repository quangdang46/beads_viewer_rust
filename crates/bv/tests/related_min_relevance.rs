//! `--related-min-relevance` parity with Go v0.25.0.
//!
//! Go registers it as a custom pflag value, not a scalar type
//! (beads_viewer/cmd/bv/main.go:1578-1583):
//!
//! ```go
//! relatedMinRelevanceFlag, flagErr := newPercentOrFraction("related-min-relevance", 20)
//! flag.Var(relatedMinRelevanceFlag, "related-min-relevance", "Minimum relevance score for related work (...)")
//! ```
//!
//! `percentOrFraction.Set` (cmd/bv/flag_types.go:66-95) takes EITHER an int
//! 0-100 (percent) OR a float 0.0-1.0 (fraction) and canonicalises to int
//! percent, so "50" and "0.5" are the same threshold. A `.` anywhere is what
//! selects the float path — that is why "1e2" is an int, not 100.
//!
//! Every rejection is a pflag PARSE error: cobra routes it through
//! `FlagErrorFunc` → `enrichFlagParseError`, which passes a bare `Set` message
//! through untouched, and main.go:4548-4550 prints pflag's
//! `invalid argument %q for %q flag: %v` wrapper and exits 1.

use std::path::PathBuf;
use std::process::Command;

/// Two open issues with identical activity windows, which makes
/// `--robot-related R-1` report R-2 under `concurrent` at relevance 80.
/// `git init` is required: `handleRobotRelated` refuses a non-repository
/// before it ever reads the flag. `tag` keeps the parallel tests off each
/// other's directory.
fn fixture_repo(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("bvr_related_min_rel_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".beads")).unwrap();
    std::fs::write(
        dir.join(".beads").join("issues.jsonl"),
        concat!(
            r#"{"id":"R-1","title":"Root one","status":"open","priority":1,"issue_type":"task","#,
            r#""labels":["rel"],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","dependencies":[]}"#,
            "\n",
            r#"{"id":"R-2","title":"Root two","status":"open","priority":2,"issue_type":"task","#,
            r#""labels":["rel"],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","dependencies":[]}"#,
            "\n",
        ),
    )
    .unwrap();
    for args in [
        &["init", "-q", "."][..],
        &["add", "-A"][..],
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ][..],
    ] {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {args:?} failed in the fixture repo");
    }
    dir
}

fn run(dir: &PathBuf, extra: &[String]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bvr"))
        .current_dir(dir)
        .args(["--robot-related", "R-1"])
        .args(extra)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// The flag as the user types it, in the space form Go's pflag rewrites.
fn with(value: &str) -> Vec<String> {
    vec!["--related-min-relevance".to_string(), value.to_string()]
}

/// Go's `Set` rejects an unparsable value, and pflag reports it verbatim.
/// Rust printed the bare `Set` message and exited 2, so both the text and the
/// exit code were wrong.
#[test]
fn unparsable_value_is_a_pflag_parse_error_at_exit_one() {
    let dir = fixture_repo("parse_error");
    let (code, stdout, stderr) = run(&dir, &with("abc"));
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(code, 1, "stdout: {stdout}");
    assert_eq!(
        stderr.trim(),
        "invalid argument \"abc\" for \"--related-min-relevance\" flag: \
         --related-min-relevance: \"abc\" is not an integer \
         (expected int 0-100 percent OR float 0.0-1.0 fraction)"
    );
    assert!(stdout.is_empty(), "no payload is emitted: {stdout}");
}

/// One case per `return` in `Set` (flag_types.go:68-92), so a future edit that
/// keeps the common "abc" message while losing one of the other four still
/// fails. Every one of them carries pflag's wrapper and exits 1.
#[test]
fn every_set_rejection_branch_reproduces_go_message_and_exit_one() {
    let dir = fixture_repo("branches");
    let cases = [
        ("", "empty value (expected int 0-100 or float 0.0-1.0)"),
        (
            "20abc",
            "\"20abc\" is not an integer \
             (expected int 0-100 percent OR float 0.0-1.0 fraction)",
        ),
        (
            "0x10",
            "\"0x10\" is not an integer \
             (expected int 0-100 percent OR float 0.0-1.0 fraction)",
        ),
        (
            "0.5.5",
            "\"0.5.5\" is not a number \
             (expected int 0-100 percent OR float 0.0-1.0 fraction)",
        ),
        (
            "1.5",
            "float 1.5 out of range (expected 0.0-1.0 fraction; for percent use int 0-100)",
        ),
        (
            "101",
            "int 101 out of range (expected 0-100 percent; for fraction use float 0.0-1.0)",
        ),
    ];
    for (value, tail) in cases {
        let (code, stdout, stderr) = run(&dir, &with(value));
        assert_eq!(
            code, 1,
            "--related-min-relevance {value:?} stdout: {stdout}"
        );
        assert_eq!(
            stderr.trim(),
            format!(
                "invalid argument {value:?} for \"--related-min-relevance\" flag: \
                 --related-min-relevance: {tail}"
            ),
            "--related-min-relevance {value:?}"
        );
        assert!(stdout.is_empty(), "no payload for {value:?}: {stdout}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Go trims with `s = strings.TrimSpace(s)` BEFORE formatting, so `Set` quotes
/// the trimmed value while pflag's outer `%q` keeps the raw argv token. Rust
/// quoted the raw token in both places, so `"  abc  "` misreported what the user
/// actually typed as the offending number.
#[test]
fn outer_wrapper_quotes_the_raw_token_while_set_quotes_the_trimmed_one() {
    let dir = fixture_repo("trimmed");
    let (code, _, stderr) = run(&dir, &with("  abc  "));
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(code, 1);
    assert_eq!(
        stderr.trim(),
        "invalid argument \"  abc  \" for \"--related-min-relevance\" flag: \
         --related-min-relevance: \"abc\" is not an integer \
         (expected int 0-100 percent OR float 0.0-1.0 fraction)"
    );
}

/// The float path calls `strconv.ParseFloat(s, 64)`, which accepts digit
/// underscores and hex float literals and reports overflow as an error. Rust
/// used `str::parse::<f64>()`, which rejected the first two and turned the
/// third into `inf` — so all three landed on the wrong message.
#[test]
fn float_branch_uses_strconv_parsefloat() {
    let dir = fixture_repo("parsefloat");
    let cases = [
        (
            "1_0.5",
            "float 10.5 out of range (expected 0.0-1.0 fraction; for percent use int 0-100)",
        ),
        (
            "0x1.8p1",
            "float 3 out of range (expected 0.0-1.0 fraction; for percent use int 0-100)",
        ),
        // ParseFloat's range error is `err != nil`, so `Set` reports it with
        // the FLOAT branch's "is not a number" wording, not as a range error.
        (
            "1.7976931348623159e309",
            "\"1.7976931348623159e309\" is not a number \
             (expected int 0-100 percent OR float 0.0-1.0 fraction)",
        ),
    ];
    for (value, tail) in cases {
        let (code, _, stderr) = run(&dir, &with(value));
        assert_eq!(code, 1, "--related-min-relevance {value:?}");
        assert_eq!(
            stderr.trim(),
            format!(
                "invalid argument {value:?} for \"--related-min-relevance\" flag: \
                 --related-min-relevance: {tail}"
            ),
            "--related-min-relevance {value:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The out-of-range message renders the float with `%g` (flag_types.go:80).
/// fmt switches to `%e` when the decimal exponent is `< -4` or `>= 6` and pads
/// it to two digits; Rust's `{}` never does, so `1.0e10` printed as
/// `10000000000`. `1.e5` is the other side of that boundary and must stay in
/// fixed notation.
#[test]
fn out_of_range_float_uses_go_g_verb() {
    let dir = fixture_repo("gverb");
    let cases = [
        // exp >= 6 → %e, exponent padded to two digits
        ("1.0e10", "1e+10"),
        ("1.0e6", "1e+06"),
        // exp == 5 → still %f
        ("1.0e5", "100000"),
        ("1.e5", "100000"),
        // exp == -4 → still %f; exp == -5 → %e
        ("-1.0e-4", "-0.0001"),
        ("-1.0e-5", "-1e-05"),
        ("-0.5", "-0.5"),
        ("1.0000000000000002", "1.0000000000000002"),
    ];
    for (value, rendered) in cases {
        let (code, _, stderr) = run(&dir, &with(value));
        assert_eq!(code, 1, "--related-min-relevance {value:?}");
        assert!(
            stderr
                .trim()
                .contains(&format!("float {rendered} out of range")),
            "--related-min-relevance {value:?} rendered with Rust's Display, not Go's %g: {stderr}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The payload with `generated_at` removed. Two runs of the same command agree
/// on every field except the second-resolution timestamp, which would otherwise
/// make the equivalence assertions below fail whenever a run straddles a tick.
fn payload_without_timestamp(stdout: &str) -> serde_json::Value {
    let mut payload: serde_json::Value = serde_json::from_str(stdout).expect("one JSON object");
    payload
        .as_object_mut()
        .expect("envelope is an object")
        .remove("generated_at");
    payload
}

/// The whole point of `percentOrFraction`: one threshold, two spellings. Both
/// must reach the same integer percent, so the two payloads are identical.
#[test]
fn fraction_and_percent_spellings_canonicalize_identically() {
    let dir = fixture_repo("units");
    for (fraction, percent) in [("0.5", "50"), ("0.1", "10"), ("0.79", "79"), ("1.0", "100")] {
        let (code, by_fraction, stderr) = run(&dir, &with(fraction));
        assert_eq!(
            code, 0,
            "--related-min-relevance {fraction} stderr: {stderr}"
        );
        let (code, by_percent, stderr) = run(&dir, &with(percent));
        assert_eq!(
            code, 0,
            "--related-min-relevance {percent} stderr: {stderr}"
        );
        assert_eq!(
            payload_without_timestamp(&by_fraction),
            payload_without_timestamp(&by_percent),
            "--related-min-relevance {fraction} and {percent} must be the same threshold"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Omitting the flag must equal Go's registered default of 20 (main.go:1578).
#[test]
fn omitted_flag_matches_the_go_default_of_twenty() {
    let dir = fixture_repo("default");
    let (code, default, stderr) = run(&dir, &[]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let (code, explicit, stderr) = run(&dir, &with("20"));
    assert_eq!(code, 0, "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        payload_without_timestamp(&default),
        payload_without_timestamp(&explicit),
        "the default is 20 percent, not 0.2 or 50"
    );
}

/// The parsed integer is the filter itself, not a parsed-and-ignored value.
/// The fixture's only relation scores 80, and the bound is inclusive
/// (`relevance >= minRelevance`), so 80 and 0.8 keep it while 81 and 0.81 do
/// not. This is the assertion that would break if the canonicalised integer
/// stopped reaching `opts.MinRelevance`.
#[test]
fn parsed_percent_reaches_the_relevance_filter() {
    let dir = fixture_repo("filter");
    for (value, want_related) in [
        ("0", 1),
        ("50", 1),
        ("80", 1),
        ("0.8", 1),
        ("81", 0),
        ("0.81", 0),
        ("100", 0),
    ] {
        let (code, stdout, stderr) = run(&dir, &with(value));
        assert_eq!(code, 0, "--related-min-relevance {value} stderr: {stderr}");
        let payload: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON object");
        assert_eq!(
            payload["total_related"].as_u64(),
            Some(want_related),
            "--related-min-relevance {value} should yield {want_related} related: {payload}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
