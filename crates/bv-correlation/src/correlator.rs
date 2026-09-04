//! Bead↔commit correlator — assembles the primitives in `explicit.rs`,
//! `temporal.rs`, and `scorer.rs` into an actual per-bead correlation
//! report, the way Go's `pkg/correlation/correlator.go` (882 lines) does.
//!
//! Documented scope cut (see plan doc §11): this is a simplified but real
//! pipeline, not a line-for-line port of `correlator.go`. Differences:
//! - Walks full `git log --name-status` once (all commits touching the
//!   repo), rather than Go's two-strategy walk (explicit-ID-filtered `-G`
//!   search + separate temporal-window commit walk merged via
//!   `mergeCorrelatedCommits`). Functionally similar output, fewer git
//!   invocations, no incremental/cached-artifact layer (`historyArtifact`,
//!   `GenerateReportCached`).
//! - Temporal signal requires an author match against the issue's
//!   `assignee` field (Go's `extractTemporalCandidates` uses author
//!   activity windows without that requirement, weighted by concurrent
//!   active-bead count) — simpler, more conservative (fewer false
//!   positives, more false negatives for temporal-only correlation).
//! - No `--as-of` time-travel support yet.

use crate::explicit::{calculate_confidence, find_mentions, IdPatterns};
use crate::temporal::{temporal_confidence, TemporalWindow};
use bv_core::model::Issue;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub timestamp: String,
    pub author: String,
    pub author_email: String,
    pub message: String,
    pub files: Vec<String>,
}

/// Walk the full git log once: sha, timestamp, author, message, files
/// touched. Go's git-log format string convention (`%x00` separators)
/// mirrors `extractor.rs` for consistency.
pub fn walk_commits(repo: &Path, limit: usize) -> Result<Vec<CommitInfo>, String> {
    let mut args: Vec<String> = vec![
        "log".into(),
        "--name-only".into(),
        "--format=%x01%H%x00%aI%x00%an%x00%ae%x00%s".into(),
    ];
    if limit > 0 {
        args.push(format!("-n{limit}"));
    }
    let out = Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("spawning git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git log failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(parse_commits(&text))
}

fn parse_commits(text: &str) -> Vec<CommitInfo> {
    let mut out = Vec::new();
    for block in text.split('\x01').skip(1) {
        let mut lines = block.splitn(2, '\n');
        let Some(header) = lines.next() else { continue };
        let fields: Vec<&str> = header.split('\x00').collect();
        if fields.len() < 5 {
            continue;
        }
        let files: Vec<String> = lines
            .next()
            .unwrap_or("")
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        out.push(CommitInfo {
            sha: fields[0].to_string(),
            timestamp: fields[1].to_string(),
            author: fields[2].to_string(),
            author_email: fields[3].to_string(),
            message: fields[4].to_string(),
            files,
        });
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CorrelatedCommit {
    pub sha: String,
    pub bead_id: String,
    pub confidence: f64,
    pub methods: Vec<&'static str>,
    pub reason: String,
    pub timestamp: String,
    pub author: String,
    pub files: Vec<String>,
}

/// Correlate every commit against every issue. Returns commits grouped by
/// bead ID, each entry carrying the combined confidence across whichever
/// signals fired (explicit-ID mention and/or same-author temporal window).
pub fn correlate(issues: &[Issue], commits: &[CommitInfo]) -> BTreeMap<String, Vec<CorrelatedCommit>> {
    let patterns = IdPatterns::default();
    let by_id: BTreeMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut report: BTreeMap<String, Vec<CorrelatedCommit>> = BTreeMap::new();

    for commit in commits {
        // 1. Explicit-ID signal: scan the commit message once.
        let mentions = find_mentions(&commit.message, &patterns);
        let mentioned_ids: std::collections::BTreeSet<&str> =
            mentions.iter().map(|m| m.bead_id.as_str()).collect();
        for bead_id in &mentioned_ids {
            let Some(issue) = by_id.get(bead_id) else { continue };
            let kind = mentions.iter().find(|m| m.bead_id == *bead_id).map(|m| m.kind);
            let confidence = calculate_confidence(kind, mentions.len());
            report.entry(issue.id.clone()).or_default().push(CorrelatedCommit {
                sha: commit.sha.clone(),
                bead_id: issue.id.clone(),
                confidence,
                methods: vec!["explicit_id"],
                reason: format!("commit message references {bead_id}"),
                timestamp: commit.timestamp.clone(),
                author: commit.author.clone(),
                files: commit.files.clone(),
            });
        }

        // 2. Temporal signal: same author committing within the issue's
        // active window (created_at..closed_at, or created_at..now for
        // still-open issues), skipped for issues already matched above.
        let Ok(commit_ts) = commit.timestamp.parse::<jiff::Timestamp>() else { continue };
        for issue in issues {
            if mentioned_ids.contains(issue.id.as_str()) {
                continue;
            }
            if issue.assignee.is_empty() || issue.assignee != commit.author && issue.assignee != commit.author_email {
                continue;
            }
            let Some(created) = issue.created_at.as_deref().and_then(|s| s.parse::<jiff::Timestamp>().ok()) else {
                continue;
            };
            let end = issue
                .closed_at
                .as_deref()
                .and_then(|s| s.parse::<jiff::Timestamp>().ok())
                .unwrap_or(commit_ts.max(created));
            if commit_ts < created || commit_ts > end {
                continue;
            }
            let window_secs = (end - created).total(jiff::Unit::Second).unwrap_or(0.0).max(0.0);
            let title_words: Vec<String> = issue
                .title
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .map(|w| w.to_lowercase())
                .collect();
            let paths_match_hints = commit.files.iter().any(|f| {
                let lower = f.to_lowercase();
                title_words.iter().any(|w| lower.contains(w.as_str()))
            });
            let confidence = temporal_confidence(&TemporalWindow {
                active_beads: 1,
                window_duration: Duration::from_secs_f64(window_secs),
                paths_match_hints,
            });
            report.entry(issue.id.clone()).or_default().push(CorrelatedCommit {
                sha: commit.sha.clone(),
                bead_id: issue.id.clone(),
                confidence,
                methods: vec!["temporal_author"],
                reason: format!("same author active during {}'s open window", issue.id),
                timestamp: commit.timestamp.clone(),
                author: commit.author.clone(),
                files: commit.files.clone(),
            });
        }
    }

    // By construction each (bead, commit) pair is pushed at most once above
    // (the temporal pass explicitly skips beads already matched by an
    // explicit mention in that same commit — see `mentioned_ids.contains`),
    // so there is nothing to merge here; `combine_confidence` /
    // `Method` are re-exported from `scorer` for callers that want to
    // combine a bead's per-commit confidences into one overall score (used
    // by `robot-explain-correlation`), not needed for per-commit dedup.
    for commits in report.values_mut() {
        commits.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use bv_core::model::Status;

    fn issue(id: &str, title: &str, assignee: &str, created: &str, closed: Option<&str>) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: title.to_string(),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status: if closed.is_some() { Status::Closed } else { Status::Open },
            priority: 2,
            issue_type: "task".into(),
            assignee: assignee.to_string(),
            estimated_minutes: None,
            created_at: Some(created.to_string()),
            updated_at: Some(created.to_string()),
            due_date: None,
            closed_at: closed.map(|s| s.to_string()),
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: vec![],
            dependencies: vec![],
            comments: vec![],
            source_repo: String::new(),
        }
    }

    fn commit(sha: &str, ts: &str, author: &str, msg: &str, files: &[&str]) -> CommitInfo {
        CommitInfo {
            sha: sha.to_string(),
            timestamp: ts.to_string(),
            author: author.to_string(),
            author_email: String::new(),
            message: msg.to_string(),
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn explicit_mention_correlates_regardless_of_author() {
        let issues = vec![issue("PROJ-1", "fix login bug", "", "2026-01-01T00:00:00Z", None)];
        let commits = vec![commit("sha1", "2026-01-02T00:00:00Z", "anyone", "fixes PROJ-1", &["a.rs"])];
        let report = correlate(&issues, &commits);
        let hits = report.get("PROJ-1").expect("correlated");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].methods.contains(&"explicit_id"));
        assert!(hits[0].confidence > 0.7);
    }

    #[test]
    fn temporal_signal_requires_assignee_match() {
        let issues = vec![issue("PROJ-2", "refactor db layer", "alice", "2026-01-01T00:00:00Z", Some("2026-01-03T00:00:00Z"))];
        let commits = vec![
            commit("sha2", "2026-01-02T00:00:00Z", "alice", "misc cleanup", &["db.rs"]),
            commit("sha3", "2026-01-02T00:00:00Z", "bob", "misc cleanup", &["db.rs"]),
        ];
        let report = correlate(&issues, &commits);
        let hits = report.get("PROJ-2").expect("correlated");
        assert_eq!(hits.len(), 1, "only alice's commit should correlate");
        assert!(hits[0].methods.contains(&"temporal_author"));
    }

    #[test]
    fn commit_outside_window_does_not_correlate() {
        let issues = vec![issue("PROJ-3", "task", "alice", "2026-01-01T00:00:00Z", Some("2026-01-02T00:00:00Z"))];
        let commits = vec![commit("sha4", "2026-06-01T00:00:00Z", "alice", "unrelated", &["x.rs"])];
        let report = correlate(&issues, &commits);
        assert!(!report.contains_key("PROJ-3"));
    }

    #[test]
    fn no_signals_yields_no_entry() {
        let issues = vec![issue("PROJ-4", "task", "alice", "2026-01-01T00:00:00Z", None)];
        let commits = vec![commit("sha5", "2026-01-02T00:00:00Z", "bob", "unrelated work", &["y.rs"])];
        let report = correlate(&issues, &commits);
        assert!(!report.contains_key("PROJ-4"));
    }
}
