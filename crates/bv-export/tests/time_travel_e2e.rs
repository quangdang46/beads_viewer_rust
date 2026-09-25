//! End-to-end test for the time-travel timeline, against Go's own output.
//!
//! The test builds a small git repository with a known three-commit history,
//! runs the real `bv-correlation` extractor over it, and feeds the result to
//! [`generate_history_for_export`]. The expected document was produced by
//! running Go's `bv --export-pages` over the *same* repository at parity
//! commit `18afafa` and reading `data/history.json`, so this pins the whole
//! path — event classification, ancestry ordering, list sorting and the JSON
//! shape — against the reference rather than against a reading of it.
//!
//! Commit SHAs are normalised out of the comparison. They are content-derived
//! and therefore stable only for a byte-identical repository, and line-ending
//! handling (`core.autocrlf`) is a per-machine git setting; everything the
//! timeline actually decides — order, grouping, messages, dates — is compared
//! literally.

use bv_correlation::history::{build_history_report, BeadInfo, HistoryOptions};
use bv_export::time_travel::generate_history_for_export;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const BEADS_FILE: &str = ".beads/issues.jsonl";

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn git_at(repo: &Path, args: &[&str], date: &str) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_beads(repo: &Path, body: &str) {
    std::fs::write(repo.join(BEADS_FILE), body).expect("write issues.jsonl");
}

fn commit(repo: &Path, date: &str, message: &str) {
    git_at(repo, &["add", "-A"], date);
    git_at(repo, &["commit", "-q", "-m", message], date);
}

/// The three-commit history the expected document was captured from: bd-1 is
/// added, then closed, then bd-2 is added.
fn build_fixture_repo(tag: &str) -> PathBuf {
    let repo = std::env::temp_dir().join(format!("bvr-time-travel-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(repo.join(".beads")).expect("create .beads");

    // Pin the repository's own git configuration so the fixture does not
    // inherit whatever the host has set. `-c` flags cover the invocation;
    // `config` covers the on-disk state the fixture leaves behind.
    git(&repo, &["init", "-q", "-b", "main", "."]);
    for (key, value) in [
        ("user.email", "fixture@example.com"),
        ("user.name", "Fixture"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&repo, &["config", key, value]);
    }
    std::fs::write(repo.join(".gitattributes"), "* -text\n").expect("write .gitattributes");

    write_beads(
        &repo,
        "{\"id\":\"bd-1\",\"title\":\"First issue\",\"status\":\"open\",\"priority\":1,\
         \"issue_type\":\"task\",\"created_at\":\"2024-01-01T00:00:00Z\",\
         \"updated_at\":\"2024-01-01T00:00:00Z\"}\n",
    );
    commit(&repo, "2024-01-01T10:00:00+00:00", "add bd-1");

    write_beads(
        &repo,
        "{\"id\":\"bd-1\",\"title\":\"First issue\",\"status\":\"closed\",\"priority\":1,\
         \"issue_type\":\"task\",\"created_at\":\"2024-01-01T00:00:00Z\",\
         \"updated_at\":\"2024-01-02T00:00:00Z\",\"closed_at\":\"2024-01-02T00:00:00Z\"}\n",
    );
    commit(&repo, "2024-01-02T10:00:00+00:00", "close bd-1");

    write_beads(
        &repo,
        "{\"id\":\"bd-1\",\"title\":\"First issue\",\"status\":\"closed\",\"priority\":1,\
         \"issue_type\":\"task\",\"created_at\":\"2024-01-01T00:00:00Z\",\
         \"updated_at\":\"2024-01-02T00:00:00Z\",\"closed_at\":\"2024-01-02T00:00:00Z\"}\n\
         {\"id\":\"bd-2\",\"title\":\"Second issue\",\"status\":\"open\",\"priority\":2,\
         \"issue_type\":\"bug\",\"created_at\":\"2024-01-03T00:00:00Z\",\
         \"updated_at\":\"2024-01-03T00:00:00Z\"}\n",
    );
    commit(&repo, "2024-01-03T10:00:00+00:00", "add bd-2");

    repo
}

/// Strip the fields that cannot be stable across machines: the commit SHAs
/// (content-derived) and the generation timestamp.
fn normalize(doc: &Value) -> Value {
    let mut doc = doc.clone();
    if let Some(commits) = doc.get_mut("commits").and_then(Value::as_array_mut) {
        for (i, commit) in commits.iter_mut().enumerate() {
            if let Some(sha) = commit.get("sha").and_then(Value::as_str) {
                assert_eq!(sha.len(), 40, "expected a full object name");
                commit["sha"] = Value::String(format!("<sha-{i}>"));
            }
        }
    }
    doc["generated_at"] = Value::String("<generated_at>".to_string());
    doc
}

#[test]
fn timeline_matches_go_export_pages_output() {
    let repo = build_fixture_repo("e2e");
    let beads = [
        BeadInfo {
            id: "bd-1".to_string(),
            title: "First issue".to_string(),
            status: "closed".to_string(),
        },
        BeadInfo {
            id: "bd-2".to_string(),
            title: "Second issue".to_string(),
            status: "open".to_string(),
        },
    ];
    let report = build_history_report(
        &repo,
        &beads,
        &HistoryOptions {
            // Go `cmd/bv/main.go:7084` — the same 500 the ancestry walk uses.
            limit: 500,
            ..HistoryOptions::default()
        },
        Some(BEADS_FILE),
        "2024-01-15T10:00:00Z".to_string(),
    )
    .expect("history report");

    let history = generate_history_for_export(&repo, BEADS_FILE, &report, "2024-01-15T10:00:00Z")
        .expect("timeline");
    let got = normalize(&serde_json::to_value(&history).unwrap());

    // Captured from Go `bv --export-pages` over the identical repository at
    // 18afafa, with the three SHAs replaced by their positions.
    let expected: Value = serde_json::from_str(
        r#"{
          "generated_at": "<generated_at>",
          "commits": [
            { "sha": "<sha-0>", "date": "2024-01-01T10:00:00Z", "message": "add bd-1",
              "beads_added": ["bd-1"] },
            { "sha": "<sha-1>", "date": "2024-01-02T10:00:00Z", "message": "close bd-1",
              "beads_closed": ["bd-1"] },
            { "sha": "<sha-2>", "date": "2024-01-03T10:00:00Z", "message": "add bd-2",
              "beads_added": ["bd-2"] }
          ]
        }"#,
    )
    .unwrap();

    assert_eq!(got, expected);
}

#[test]
fn a_commit_missing_from_the_ancestry_walk_is_an_error() {
    // cmd/bv/main.go:7170-7172 — silently dropping it would desynchronise
    // the scrubber, so the export fails instead.
    let repo = build_fixture_repo("missing");
    let beads = [BeadInfo {
        id: "bd-1".to_string(),
        title: "First issue".to_string(),
        status: "closed".to_string(),
    }];
    let mut report = build_history_report(
        &repo,
        &beads,
        &HistoryOptions {
            limit: 500,
            ..HistoryOptions::default()
        },
        Some(BEADS_FILE),
        "2024-01-15T10:00:00Z".to_string(),
    )
    .expect("history report");

    // Point an event at a commit the `git log` walk cannot see.
    for history in report.histories.values_mut() {
        for event in &mut history.events {
            if !event.commit_sha.is_empty() {
                event.commit_sha = "0".repeat(40);
                break;
            }
        }
    }

    let err = generate_history_for_export(&repo, BEADS_FILE, &report, "2024-01-15T10:00:00Z")
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "timeline commit {} missing from source history",
            "0".repeat(40)
        )
    );
}
