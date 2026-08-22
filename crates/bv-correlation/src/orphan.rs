//! Orphan-commit detection — port of Go `pkg/correlation/orphan.go`:
//! message-pattern suspicion scoring with exact weights.

use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

/// One message pattern + its contribution weight (Go orphanMessagePatterns).
struct MessagePattern {
    re: Regex,
    weight: i32,
}

static MESSAGE_PATTERNS: LazyLock<Vec<MessagePattern>> = LazyLock::new(|| {
    vec![
        MessagePattern {
            re: Regex::new(r"\b(fix|fixes|fixed)\b").unwrap(),
            weight: 10,
        },
        MessagePattern {
            re: Regex::new(r"\b(close|closes|closed)\b").unwrap(),
            weight: 10,
        },
        MessagePattern {
            re: Regex::new(r"\b(resolve|resolves|resolved)\b").unwrap(),
            weight: 10,
        },
        MessagePattern {
            re: Regex::new(r"\b(implement|implements|implemented)\b").unwrap(),
            weight: 8,
        },
        MessagePattern {
            re: Regex::new(r"\b(add|adds|added)\b").unwrap(),
            weight: 5,
        },
        MessagePattern {
            re: Regex::new(r"#\d+").unwrap(),
            weight: 15,
        },
        // lowercase JIRA-style (message is lowercased before matching)
        MessagePattern {
            re: Regex::new(r"\b[a-z]{2,5}-\d+\b").unwrap(),
            weight: 20,
        },
        MessagePattern {
            re: Regex::new(r"\bbv-[a-z0-9]+\b").unwrap(),
            weight: 25,
        },
        MessagePattern {
            re: Regex::new(r"\bbeads?[-_]?\d+\b").unwrap(),
            weight: 25,
        },
    ]
});

fn message_patterns() -> &'static [MessagePattern] {
    &MESSAGE_PATTERNS
}

/// Go `orphanBeadIDPattern`: bv-XXXX.. case-insensitive ID extraction.
pub fn extract_bv_ids(message_lower: &str) -> Vec<String> {
    let re = Regex::new(r"(?i)\bbv-([a-z0-9]{4,8})\b").unwrap();
    re.captures_iter(message_lower)
        .map(|c| format!("bv-{}", c[1].to_lowercase()))
        .collect()
}

/// Go `checkMessage`: total pattern weight capped at 35.
pub fn message_suspicion(message: &str) -> (i32, Vec<String>) {
    let msg = message.to_lowercase();
    let mut total = 0i32;
    let mut details = Vec::new();
    for p in message_patterns() {
        if let Some(m) = p.re.find(&msg) {
            total += p.weight;
            details.push(m.as_str().to_string());
        }
    }
    let cap = total.min(35);
    (cap, details)
}

/// Suspicion signal weights for non-message factors (Go checkTiming/checkFiles).
pub const TIMING_MATCH_WEIGHT: i32 = 30;
pub const FILE_OVERLAP_BASE_WEIGHT: i32 = 25;
pub const MENTIONED_BEAD_SCORE: i32 = 35;
pub const AUTHOR_NEARBY_WEIGHT: i32 = 15;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_keyword_scores_ten() {
        let (score, details) = message_suspicion("fix the login bug");
        assert_eq!(score, 10);
        assert_eq!(details, vec!["fix"]);
    }

    #[test]
    fn bv_pattern_scores_twenty_five() {
        let (score, _) = message_suspicion("update bv-abc123 handling");
        // "bv-abc123" matches bv-pattern(25); also lowercase JIRA-style? no —
        // bv- has digits+letters; [a-z]{2,5}-\d+ needs dash-digits. Only bv hit.
        assert!(score >= 25);
    }

    #[test]
    fn multiple_patterns_accumulate_capped_at_35() {
        let (score, _) = message_suspicion("fix and close #123");
        // fix=10 + close=10 + #123=15 = 35 → capped at 35
        assert_eq!(score, 35);
    }

    #[test]
    fn plain_message_scores_zero() {
        let (score, details) = message_suspicion("refactor internal helpers");
        assert_eq!(score, 0);
        assert!(details.is_empty());
    }

    #[test]
    fn extracts_bv_ids_case_insensitive() {
        let ids = extract_bv_ids("resolves BV-ab12cd34 quickly");
        assert_eq!(ids, vec!["bv-ab12cd34"]);
    }
}

/// One orphan-commit candidate with computed suspicion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OrphanCandidate {
    pub sha: String,
    #[serde(rename = "short_sha")]
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub suspicion_score: i32,
    pub signals: Vec<String>,
}

impl OrphanCandidate {
    pub fn into_json(self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Scan `git log` (excluding beads-file-only commits) for orphan candidates:
/// commits whose message scores >= min_score but which never touched the
/// beads JSONL in the same commit (i.e., no bead update co-committed).
pub fn scan_orphan_candidates(
    repo: &Path,
    correlated_events: &[super::extractor::BeadEvent],
    min_score: i32,
) -> Vec<OrphanCandidate> {
    // SHAs that already updated the beads file = correlated commits.
    let correlated: std::collections::HashSet<&str> = correlated_events
        .iter()
        .map(|e| e.commit_sha.as_str())
        .collect();

    let out = Command::new("git")
        .args(["log", "--no-merges", "--format=%H%x00%aI%x00%an%x00%s"])
        .current_dir(repo)
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();

    let mut candidates = Vec::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\u{0}').collect();
        if parts.len() < 4 {
            continue;
        }
        let sha = parts[0];
        if correlated.contains(sha) {
            continue; // already linked to a bead
        }
        let timestamp = parts[1].to_string();
        let author = parts[2].to_string();
        let message = parts[3].to_string();

        let (score, signals) = message_suspicion(&message);
        if score >= min_score {
            candidates.push(OrphanCandidate {
                short_sha: sha.chars().take(7).collect(),
                sha: sha.to_string(),
                message,
                author,
                timestamp,
                suspicion_score: score,
                signals,
            });
        }
    }
    // Highest suspicion first.
    candidates.sort_by_key(|c| std::cmp::Reverse(c.suspicion_score));
    candidates
}
