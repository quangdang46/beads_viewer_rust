//! The value-position sweep behind `argv::normalize_flag_spelling`.
//!
//! Go decides whether `-foo` is a long flag by consulting the live flag set
//! (`flags.Lookup(name) == nil`, main.go:548) — never by the shape of the name.
//! The name-shape test that this replaced read any multi-character
//! alphanumeric token as a flag, so a *value* beginning with `-` was rewritten
//! along with it and handed to the value parser as a flag spelling:
//! `--db -1e400` became `--db --1e400` and `bvr` went looking for a database
//! at `./--1e400`.
//!
//! `single_dash_flag_rewrite.rs` pins one flag against the Go oracle's exact
//! message. This file covers the other direction — breadth rather than depth —
//! by walking every value-taking flag in the registry and asserting none of them
//! ever sees a value it was handed as a flag. `flags::tests::
//! registry_covers_every_go_command_line_flag` pins the name set itself; the two
//! together mean the rewriter cannot drift away from Go's `flag.CommandLine`
//! without one of them going red.
//!
//! `bv` is a binary-only crate (no `lib.rs`), so the table is spelled out here
//! rather than imported from `flags::ROBOT_PRIMARIES`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The sweep hands a path-shaped value to every flag, and several of them
/// (`--db`, `--repo`, `--workspace`) *act* on a path — the flag creates the
/// directory it is given. Running in the repo root therefore litters it with
/// `-1e400/`, which the selfrepo golden cases then read back as a
/// datasource-discovery change. Every invocation gets its own scratch
/// directory instead, and `--db` is absolute for the same reason.
fn scratch_dir(flag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bv-rewrite-sweep-{}-{flag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Some flags legitimately start something that does not return — `--preview-
/// pages` serves until interrupted, `--watch-export` loops on a file event.
/// The rewrite under test happens long before any of that, so the sweep gives
/// every invocation a deadline and inspects whatever it managed to print. std
/// has no process timeout, so the child is killed from a watchdog thread.
const DEADLINE: Duration = Duration::from_secs(5);

/// Both streams of one invocation, captured and truncated to the deadline.
fn run(flag: &str) -> String {
    let cwd = scratch_dir(flag);
    let db = PathBuf::from(REPO_ROOT).join(".beads").join("issues.jsonl");
    let mut child = Command::new(env!("CARGO_BIN_EXE_bvr"))
        .current_dir(&cwd)
        .args([
            "--db",
            &db.to_string_lossy(),
            &format!("--{flag}"),
            "-1e400",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("--{flag}: {e}"));

    let (tx, rx) = mpsc::channel();
    let spawn_reader = |pipe: Box<dyn Read + Send>| {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut pipe = pipe;
            let mut buf = String::new();
            let _ = pipe.read_to_string(&mut buf);
            tx.send(buf).ok();
        })
    };
    let mut streams = Vec::new();
    if let Some(out) = child.stdout.take() {
        streams.push(spawn_reader(Box::new(out)));
    }
    if let Some(err) = child.stderr.take() {
        streams.push(spawn_reader(Box::new(err)));
    }

    // The watchdog owns the child so it can both reap it and cut it short.
    // Polling rather than a single sleep keeps the sweep fast: the deadline
    // only costs anything for a flag that actually blocks.
    let watchdog = thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < DEADLINE {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => return,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    });

    drop(tx);
    for h in streams {
        h.join().expect("stream reader thread");
    }
    let mut both = String::new();
    for chunk in rx.iter() {
        both.push_str(&chunk);
    }
    watchdog.join().expect("watchdog thread");
    both
}

/// Every `flag.CommandLine` entry Go registers through a non-`Bool`
/// constructor (`flag.String`/`StringP`/`StringArray`/`Int`/`Float64`/`Var`),
/// in `main.go:1458-1583`. These are the flags whose *next argument* is a
/// value, so they are the ones a leading-dash value can be swallowed by.
const VALUE_FLAGS: &[&str] = &[
    "agent-brief",
    "agents",
    "alert-label",
    "alert-type",
    "as-of",
    "attention-limit",
    "bead-history",
    "capacity-label",
    "correlation-by",
    "correlation-reason",
    "cpu-profile",
    "db",
    "debug-height",
    "debug-render",
    "debug-width",
    "diff-since",
    "export",
    "export-format",
    "export-graph",
    "export-md",
    "export-pages",
    "export-template",
    "feedback-accept",
    "feedback-ignore",
    "file-beads-limit",
    "forecast-agents",
    "forecast-label",
    "forecast-sprint",
    "format",
    "graph-depth",
    "graph-format",
    "graph-preset",
    "graph-root",
    "graph-title",
    "history-limit",
    "history-since",
    "hotspots-limit",
    "id-pattern",
    "label",
    "min-confidence",
    "network-depth",
    "orphans-min-score",
    "pages-title",
    "preview-pages",
    "priority-brief",
    "recipe",
    "related-max-results",
    "related-min-relevance",
    "relations-limit",
    "relations-threshold",
    "repo",
    "robot-blocker-chain",
    "robot-burndown",
    "robot-by-assignee",
    "robot-by-label",
    "robot-causality",
    "robot-confirm-correlation",
    "robot-docs",
    "robot-explain-correlation",
    "robot-file-beads",
    "robot-file-relations",
    "robot-forecast",
    "robot-history-timeout-ms",
    "robot-impact",
    "robot-impact-network",
    "robot-max-results",
    "robot-min-confidence",
    "robot-not-ready-labels",
    "robot-reject-correlation",
    "robot-related",
    "robot-sprint-show",
    "save-baseline",
    "schema-command",
    "script-format",
    "script-limit",
    "search",
    "search-limit",
    "search-min-score",
    "search-mode",
    "search-preset",
    "search-weights",
    "severity",
    "suggest-bead",
    "suggest-confidence",
    "suggest-type",
    "theme",
    "workspace",
];

/// A value that begins with `-` must reach the flag's value parser as a value.
/// The marker is chosen so that a rewrite is unmissable: if `-1e400` ever
/// becomes the token `--1e400`, it is no longer a number and the flag that
/// consumes it either fails on the doubled dashes or treats the whole thing as
/// a path. Neither may be reported in terms of `--1e400`.
#[test]
fn no_value_taking_flag_receives_a_rewritten_value() {
    let mut failures = Vec::new();
    for flag in VALUE_FLAGS {
        let both = run(flag);
        if both.contains("--1e400") {
            let offending = both
                .lines()
                .find(|l| l.contains("--1e400"))
                .unwrap_or_default();
            failures.push(format!("--{flag} -1e400 -> {offending}"));
        }
    }
    assert!(
        failures.is_empty(),
        "the single-dash rewriter turned a value into a flag:\n  {}",
        failures.join("\n  ")
    );
}
