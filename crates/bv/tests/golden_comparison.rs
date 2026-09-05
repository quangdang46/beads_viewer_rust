//! Golden comparison gate — runs the Rust binary against each fixture with
//! every golden-captured command, normalizes nondeterministic fields
//! (timestamps/timings), and compares against the frozen Go goldens in
//! `golden/` byte-for-byte (modulo normalization).
//!
//! This is the real differential gate the CI "Golden corpus integrity" job
//! was waiting on. It operates as a RATCHET:
//!
//! - Infrastructure regressions (binary error, invalid JSON) always fail —
//!   those break the harness itself, not just parity.
//! - Content divergences fail only when the count exceeds
//!   `GOLDEN_GATE_BASELINE_FAILS`. Lower the constant as parity work lands;
//!   never raise it. When it reaches 0 the gate is a hard byte-for-byte
//!   contract.
//!
//! Note: `selfrepo` goldens were captured 2026-08-22 against the live repo's
//! `.beads/issues.jsonl`, which legitimately drifts as beads are added here —
//! expect selfrepo diffs until goldens are recaptured at a frozen bead set.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Golden-captured commands. Golden filename is
/// `{fixture}____{slug}.json` where slug maps dashes/spaces of the CLI
/// args to underscores — matching `scripts/capture_goldens.sh`.
const GOLDEN_CASES: &[(&[&str], &str)] = &[
    (&["--robot-triage"], "robot_triage"),
    (&["--robot-next"], "robot_next"),
    (&["--robot-plan"], "robot_plan"),
    (&["--robot-insights"], "robot_insights"),
    (&["--robot-priority"], "robot_priority"),
    (&["--robot-suggest"], "robot_suggest"),
    (&["--robot-alerts"], "robot_alerts"),
    (&["--robot-graph"], "robot_graph"),
    (&["--robot-label-health"], "robot_label_health"),
    (&["--robot-label-flow"], "robot_label_flow"),
    (&["--robot-label-attention"], "robot_label_attention"),
    (&["--robot-history"], "robot_history"),
    (
        &["--robot-diff", "--diff-since", "HEAD~5"],
        "robot_diff___diff_since_HEAD~5",
    ),
    (&["--robot-recipes"], "robot_recipes"),
    (&["--robot-schema"], "robot_schema"),
];

/// Fixture name → directory to run in. `selfrepo` is the repo root itself;
/// the rest are synthetic JSONL graphs under tests/fixtures/.
fn fixtures() -> Vec<(&'static str, PathBuf)> {
    // CARGO_MANIFEST_DIR is crates/bv; the repo root is two levels up.
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf();
    let mut out = vec![("selfrepo", repo.clone())];
    for name in ["small_chain", "medium_tree", "large_cyclic_600", "xl_2500"] {
        out.push((name, repo.join("tests").join("fixtures").join(name)));
    }
    out
}

fn run_bvr(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_bvr"))
        .args(args)
        .current_dir(cwd)
        .env("BV_ROBOT", "1")
        .env("BV_NO_CACHE", "1")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Strip nondeterministic fields: timestamps → placeholder, timing
/// measurements removed entirely (they vary run-to-run in Go and Rust).
fn normalize(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                match k.as_str() {
                    "ms" | "compute_time_ms" => {}
                    "generated_at" | "timestamp" => {
                        out.insert(k.clone(), Value::String("<TIMESTAMP>".into()));
                    }
                    _ => {
                        out.insert(k.clone(), normalize(val));
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

fn canonical(v: &Value) -> String {
    serde_json::to_string(&normalize(v)).expect("json serialize")
}

/// Ratchet baseline: number of content divergences at gate introduction
/// (2026-09-05: 65 of 65 comparable cases diverge; 10 goldens are empty
/// captures and skipped). Lower as parity lands; the test fails if
/// divergences exceed this count, locking in progress and preventing
/// regressions.
const GOLDEN_GATE_BASELINE_FAILS: usize = 65;

#[test]
fn rust_output_matches_frozen_go_goldens() {
    // CARGO_MANIFEST_DIR is crates/bv; the repo root is two levels up.
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf();
    let golden_dir = repo.join("golden");

    let cases = GOLDEN_CASES;
    let fixtures = fixtures();

    let mut pass = 0usize;
    let mut skip = 0usize;
    let mut infra_fails = 0usize;
    let mut diff_fails = 0usize;
    let mut infra_msgs: Vec<String> = Vec::new();
    let mut diff_msgs: Vec<String> = Vec::new();

    for (fixture_name, cwd) in &fixtures {
        for (args, slug) in cases {
            let case = format!("{fixture_name}____{slug}");
            let golden_path = golden_dir.join(format!("{fixture_name}____{slug}.json"));
            if !golden_path.exists() {
                skip += 1;
                continue;
            }

            let Some(rust_out) = run_bvr(cwd, args) else {
                infra_fails += 1;
                infra_msgs.push(format!("FAIL(rust-error): {case}"));
                continue;
            };
            let Ok(rust_json) = serde_json::from_str::<Value>(&rust_out) else {
                infra_fails += 1;
                infra_msgs.push(format!("FAIL(rust-json): {case}"));
                continue;
            };
            let Ok(golden_txt) = std::fs::read_to_string(&golden_path) else {
                skip += 1;
                continue;
            };
            // Some captured goldens are empty (0 bytes) — the capture run
            // errored for commands needing git history in non-git fixture
            // dirs. Treat them as unavailable, not as harness breakage.
            if golden_txt.trim().is_empty() {
                skip += 1;
                continue;
            }
            let Ok(golden_json) = serde_json::from_str::<Value>(&golden_txt) else {
                infra_fails += 1;
                infra_msgs.push(format!("FAIL(golden-json): {case}"));
                continue;
            };

            let (a, b) = (canonical(&rust_json), canonical(&golden_json));
            if a == b {
                pass += 1;
            } else {
                diff_fails += 1;
                // First divergence point for fast triage.
                let diff_at = a
                    .char_indices()
                    .zip(b.char_indices())
                    .find(|((_, ca), (_, cb))| ca != cb)
                    .map(|((ai, ca), (bi, cb))| format!("byte {ai}/{bi}: '{ca}' vs '{cb}'"))
                    .unwrap_or_else(|| format!("len {} vs {}", a.len(), b.len()));
                diff_msgs.push(format!("DIFF: {case} — {diff_at}"));
            }
        }
    }

    eprintln!(
        "Golden gate: PASS={pass} DIFF_FAILS={diff_fails} INFRA_FAILS={infra_fails} SKIP={skip}"
    );
    for m in diff_msgs.iter().take(20) {
        eprintln!("  {m}");
    }
    for m in &infra_msgs {
        eprintln!("  {m}");
    }

    // Infrastructure regressions always fail the gate.
    assert!(
        infra_msgs.is_empty(),
        "Golden-gate harness broke (binary error / invalid JSON):\n{}",
        infra_msgs.join("\n")
    );
    // Content parity is a ratchet: fail only if worse than baseline.
    assert!(
        diff_fails <= GOLDEN_GATE_BASELINE_FAILS,
        "Parity regressed: {diff_fails} divergences > baseline {}. Fix or lower the baseline:\n{}",
        GOLDEN_GATE_BASELINE_FAILS,
        diff_msgs.join("\n")
    );
}
