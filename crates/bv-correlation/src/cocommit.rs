//! Co-commit extraction — port of Go `pkg/correlation/cocommit.go`'s
//! confidence/reason logic (690 lines total; the bulk is batched git
//! plumbing for large-repo performance, not semantics — see scope-cut
//! note below).
//!
//! For each bead lifecycle event (a commit that touched the beads JSONL —
//! `extractor::extract`), the files changed *in that same commit* (besides
//! the beads file itself) are "co-committed" with the bead: strong direct
//! evidence of what code that commit's bead-status-change corresponds to.
//!
//! Documented scope cut: Go does its own batched `git log --no-walk`
//! plumbing (`primeBatch`) purely as a performance optimization for large
//! repos. This reuses `correlator::walk_commits`'s already-collected file
//! lists (one `git log --name-only` for the whole repo) instead of
//! re-fetching per event — same output, one process instead of N.

use crate::correlator::CommitInfo;
use crate::extractor::BeadEvent;
use serde::Serialize;
use std::collections::BTreeMap;

const EXCLUDED_PREFIXES: &[&str] = &[
    ".beads/", ".bv/", ".git/", "node_modules/", "vendor/", "__pycache__/", ".venv/", "venv/", "dist/", "build/", ".next/",
];

fn is_excluded(path: &str) -> bool {
    EXCLUDED_PREFIXES.iter().any(|p| path.starts_with(p))
}

fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("test") || lower.contains("spec") || lower.ends_with("_test.go") || lower.ends_with(".test.ts")
}

fn contains_bead_id(text: &str, bead_id: &str) -> bool {
    !bead_id.is_empty() && text.to_lowercase().contains(&bead_id.to_lowercase())
}

fn calculate_confidence(event: &BeadEvent, files: &[String]) -> f64 {
    let mut confidence = 0.95f64;
    if contains_bead_id(&event.commit_msg, &event.bead_id) {
        confidence += 0.04;
    }
    if files.len() > 20 {
        confidence -= 0.10;
    }
    if !files.is_empty() && files.iter().all(|f| is_test_file(f)) {
        confidence -= 0.05;
    }
    confidence.clamp(0.0, 1.0)
}

fn generate_reason(files: &[String], confidence: f64) -> String {
    if files.is_empty() {
        return "no co-committed files (beads-file-only commit)".to_string();
    }
    format!("{} file(s) co-committed with bead status change (confidence {:.2})", files.len(), confidence)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CoCommitResult {
    pub sha: String,
    pub bead_id: String,
    pub confidence: f64,
    pub reason: String,
    pub files: Vec<String>,
}

/// Extract co-committed files for every lifecycle event, grouped by bead
/// ID. Commits that only touched the beads JSONL (no other files, after
/// excluding vendored/build paths) are still returned with an empty
/// `files` list and a low-information `reason` — matching Go, which
/// records the event either way.
pub fn extract_all_cocommits(events: &[BeadEvent], commits: &[CommitInfo]) -> BTreeMap<String, Vec<CoCommitResult>> {
    let by_sha: BTreeMap<&str, &CommitInfo> = commits.iter().map(|c| (c.sha.as_str(), c)).collect();
    let mut out: BTreeMap<String, Vec<CoCommitResult>> = BTreeMap::new();

    for event in events {
        let Some(commit) = by_sha.get(event.commit_sha.as_str()) else { continue };
        let files: Vec<String> = commit.files.iter().filter(|f| !is_excluded(f)).cloned().collect();
        let confidence = calculate_confidence(event, &files);
        let reason = generate_reason(&files, confidence);
        out.entry(event.bead_id.clone()).or_default().push(CoCommitResult {
            sha: event.commit_sha.clone(),
            bead_id: event.bead_id.clone(),
            confidence,
            reason,
            files,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractor::EventType;

    fn event(bead_id: &str, sha: &str, msg: &str) -> BeadEvent {
        BeadEvent {
            bead_id: bead_id.to_string(),
            event_type: EventType::Modified,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            commit_sha: sha.to_string(),
            commit_msg: msg.to_string(),
            author: "alice".to_string(),
            author_email: String::new(),
        }
    }

    fn commit(sha: &str, files: &[&str]) -> CommitInfo {
        CommitInfo {
            sha: sha.to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            author: "alice".to_string(),
            author_email: String::new(),
            message: String::new(),
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn excludes_beads_and_vendored_paths() {
        let events = vec![event("A-1", "sha1", "update A-1")];
        let commits = vec![commit("sha1", &[".beads/issues.jsonl", "vendor/lib.go", "src/main.rs"])];
        let result = extract_all_cocommits(&events, &commits);
        let hits = &result["A-1"];
        assert_eq!(hits[0].files, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn bead_id_in_message_boosts_confidence() {
        let events = vec![
            event("A-2", "sha2", "unrelated message"),
            event("A-3", "sha3", "fixes A-3"),
        ];
        let commits = vec![commit("sha2", &["x.rs"]), commit("sha3", &["y.rs"])];
        let result = extract_all_cocommits(&events, &commits);
        assert!(result["A-3"][0].confidence > result["A-2"][0].confidence);
    }

    #[test]
    fn shotgun_commit_penalized() {
        let files: Vec<&str> = (0..25).map(|_| "f.rs").collect();
        let events = vec![event("A-4", "sha4", "big change")];
        let commits = vec![commit("sha4", &files)];
        let result = extract_all_cocommits(&events, &commits);
        assert!(result["A-4"][0].confidence < 0.95);
    }

    #[test]
    fn unknown_sha_is_skipped() {
        let events = vec![event("A-5", "missing-sha", "msg")];
        let result = extract_all_cocommits(&events, &[]);
        assert!(!result.contains_key("A-5"));
    }
}
