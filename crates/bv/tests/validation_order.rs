//! Go v0.25.0's four validation stages run strictly sequentially, each with its
//! own `os.Exit(1)` (cmd/bv/main.go:1890-1906):
//!
//! ```text
//! 1890  validateModifierFlags        -> exit 1
//! 1894  validateEnumFlags            -> exit 1
//! 1898  validateExclusivePrimaryCommands -> exit 1
//! 1903  *watchExport && *asOf        -> exit 1
//! ```
//!
//! Rust merged stages 1, 3 and 4 into one `violations` vector and printed all
//! of them together, so a run that Go rejects at stage 1 also printed the
//! stage-4 `--watch-export`/`--as-of` message. An agent reading stderr saw a
//! second error that Go never emits, and had to work out which one was the real
//! rejection. The exclusive-primary stage is affected identically: two
//! conflicting primaries plus one dangling modifier printed three errors where
//! Go printed one.
//!
//! Each case below pins one boundary between adjacent stages.

use std::process::{Command, Output};

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bvr"))
        .current_dir(REPO_ROOT)
        .args(args)
        .args(["--db", ".beads/issues.jsonl"])
        .output()
        .expect("binary runs")
}

fn stderr_of(args: &[&str]) -> String {
    let out = run(args);
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit 1 for {args:?}, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Stage 1 fires alone: `--watch-export` has no `--export-pages`, so Go exits at
/// main.go:1890 and never reaches the as-of check at :1903.
#[test]
fn modifier_requires_short_circuits_the_as_of_guard() {
    let err = stderr_of(&["--watch-export", "--as-of", "HEAD"]);
    assert!(
        err.contains("--watch-export requires --export-pages"),
        "{err}"
    );
    assert!(
        !err.contains("cannot be combined with --as-of"),
        "stage 1 must short-circuit before stage 4, but Rust also printed the \
         as-of message:\n{err}"
    );
}

/// The same boundary for `--no-live-reload`, which is also in MODIFIER_REQUIRES.
#[test]
fn modifier_requires_short_circuits_for_no_live_reload() {
    let err = stderr_of(&["--no-live-reload", "--as-of", "HEAD"]);
    assert!(
        err.contains("--no-live-reload requires --preview-pages"),
        "{err}"
    );
}

/// Stage 4 alone: `--export-pages` satisfies the modifier, so stage 1 passes and
/// the as-of conflict is the rejection. This is the message the short-circuit
/// test above must NOT have swallowed.
#[test]
fn as_of_guard_still_fires_when_modifier_is_satisfied() {
    let err = stderr_of(&[
        "--export-pages",
        "./out",
        "--watch-export",
        "--as-of",
        "HEAD",
    ]);
    assert!(
        err.contains("--watch-export cannot be combined with --as-of"),
        "{err}"
    );
    assert!(
        !err.contains("requires --export-pages"),
        "stage 1 passes here, so its message must not appear:\n{err}"
    );
}

/// Stage 1 still reports exactly one violation. Go's `validateModifierFlags`
/// builds the message inside its loop and `return`s on the first broken rule
/// (cmd/bv/main.go:230-232), so with two dangling modifiers the second is never
/// reached — Rust collected all of them and printed a line Go does not.
///
/// `--no-live-reload` is rule 55 and `--watch-export` is rule 56 in Go's
/// `modifierRules` slice (main.go:1839-1840), so the reported one is the former.
#[test]
fn modifier_stage_reports_only_the_first_violation() {
    let err = stderr_of(&["--watch-export", "--no-live-reload"]);
    assert!(
        err.contains("--no-live-reload requires --preview-pages"),
        "{err}"
    );
    assert!(
        !err.contains("--watch-export requires"),
        "Go returns on the first broken rule (main.go:230-232), and --no-live-reload \\\n         is the earlier rule, so the --watch-export message must not appear:\n{err}"
    );
}
