//! Bead-event extraction from git history — port of Go
//! `pkg/correlation/extractor.go` legacy `git log -p --unified=0 --follow`
//! path. Event classification and status transitions match Go exactly.

use serde::Serialize;
use std::collections::BTreeMap;
use std::process::Command;

/// Lifecycle event types (Go EventType).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Created,
    Claimed,
    Closed,
    Reopened,
    Modified,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            EventType::Created => "created",
            EventType::Claimed => "claimed",
            EventType::Closed => "closed",
            EventType::Reopened => "reopened",
            EventType::Modified => "modified",
        }
    }
}

/// One bead lifecycle event (Go `BeadEvent`).
#[derive(Debug, Clone, Serialize)]
pub struct BeadEvent {
    pub bead_id: String,
    #[serde(rename = "event_type")]
    pub event_type: EventType,
    /// RFC3339 timestamp of the commit.
    pub timestamp: String,
    pub commit_sha: String,
    #[serde(rename = "commit_message")]
    pub commit_msg: String,
    pub author: String,
    pub author_email: String,
}

/// Minimal bead state snapshot parsed from +/- diff lines (Go beadSnapshot).
#[derive(Debug, Clone)]
struct BeadSnapshot {
    id: String,
    status: String,
}

struct CommitInfo {
    sha: String,
    timestamp: String,
    author: String,
    author_email: String,
    message: String,
}

/// Options controlling extraction (Go `ExtractOptions` subset).
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Only commits touching this bead ID (git -G filter).
    pub bead_id: Option<String>,
    /// Max commits to walk; 0 = unlimited.
    pub limit: usize,
    /// Path to the beads JSONL relative to repo root. Defaults to the
    /// first existing preferred name.
    pub beads_file: Option<String>,
}

pub const DEFAULT_BEADS_FILES: [&str; 3] = ["issues.jsonl", "beads.jsonl", "beads.base.jsonl"];

fn resolve_beads_path(repo: &Path, requested: Option<&str>) -> String {
    if let Some(p) = requested {
        return p.to_string();
    }
    for name in DEFAULT_BEADS_FILES {
        let candidate = repo.join(".beads").join(name);
        if candidate.exists() {
            // git log path is repo-relative
            return format!(".beads/{name}");
        }
    }
    ".beads/issues.jsonl".to_string()
}

use std::path::Path;

/// Extract bead lifecycle events from the repository's git history.
pub fn extract(repo: &Path, opts: &ExtractOptions) -> Result<Vec<BeadEvent>, String> {
    let beads_rel = resolve_beads_path(repo, opts.beads_file.as_deref());

    let mut args: Vec<String> = vec![
        "log".into(),
        "-p".into(),
        "--unified=0".into(),
        "--follow".into(),
        "--format=%H%x00%aI%x00%an%x00%ae%x00%s".into(),
    ];
    if opts.limit > 0 {
        args.push(format!("-n{}", opts.limit));
    }
    if let Some(id) = &opts.bead_id {
        args.push(format!("-G\"id\":\\s*\"{id}\""));
    }
    args.push("--".into());
    args.push(beads_rel.clone());

    let out = Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("spawning git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(parse_log_output(&text, opts.bead_id.as_deref()))
}

/// Parse combined `git log -p` output into events. Public for differential
/// testing against fixture logs.
pub fn parse_log_output(output: &str, filter_bead_id: Option<&str>) -> Vec<BeadEvent> {
    let mut all_events = Vec::new();

    // Split into commit sections by the header line pattern:
    // <40-hex>\x00<rfc3339>\x00<name>\x00<email>\x00<subject>
    let mut current: Option<(CommitInfo, String)> = None;

    for line in output.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        // Header detection: 40 hex chars followed by NUL
        let bytes = line.as_bytes();
        let is_header =
            bytes.len() > 41 && bytes[..40].iter().all(|b| b.is_ascii_hexdigit()) && bytes[40] == 0;

        if std::env::var("BV_DEBUG_PARSE").is_ok() {
            eprintln!("LINE: {line:.60?} is_header={is_header}");
        }
        #[cfg(test)]
        eprintln!("line={:.50?} header={}", line, is_header);
        if is_header {
            if let Some((info, diff)) = current.take() {
                all_events.extend(parse_diff_section(&diff, &info, filter_bead_id));
            }
            let parts: Vec<&str> = line.split('\u{0}').collect();
            if parts.len() >= 5 {
                current = Some((
                    CommitInfo {
                        sha: parts[0].to_string(),
                        timestamp: parts[1].to_string(),
                        author: parts[2].to_string(),
                        author_email: parts[3].to_string(),
                        message: parts[4].to_string(),
                    },
                    String::new(),
                ));
            }
        } else if current.is_some() {
            if let Some((_, diff)) = current.as_mut() {
                diff.push_str(line);
                diff.push('\n');
            }
        }
    }
    if let Some((info, diff)) = current.take() {
        all_events.extend(parse_diff_section(&diff, &info, filter_bead_id));
    }

    all_events
}

fn ignorable_metadata_line(line: &str) -> bool {
    match line.as_bytes().first() {
        None => true,
        Some(b'@') | Some(b'd') | Some(b'i') | Some(b'n') => true,
        // "diff --git", "index", "new file mode" all covered by first byte
        _ => false,
    }
}

fn parse_bead_json(json_str: &str) -> Option<BeadSnapshot> {
    #[derive(serde::Deserialize)]
    struct Partial {
        #[serde(default)]
        id: String,
        #[serde(default)]
        status: String,
    }
    let p: Partial = serde_json::from_str(json_str).ok()?;
    if p.id.is_empty() {
        return None;
    }
    Some(BeadSnapshot {
        id: p.id,
        status: p.status,
    })
}

/// Go: `determineStatusEvent`.
fn determine_status_event(old_status: &str, new_status: &str) -> EventType {
    let old_s = old_status.trim().to_lowercase();
    let new_s = new_status.trim().to_lowercase();
    let was_closed = old_s == "closed" || old_s == "tombstone";
    match new_s.as_str() {
        "in_progress" => {
            if was_closed {
                EventType::Reopened
            } else {
                EventType::Claimed
            }
        }
        "closed" | "tombstone" => EventType::Closed,
        "open" => {
            if was_closed {
                EventType::Reopened
            } else {
                EventType::Modified
            }
        }
        _ => EventType::Modified,
    }
}

/// Go: `parseDiff` — collect +/- bead snapshots then emit sorted-ID events.
fn parse_diff_section(
    diff: &str,
    info: &CommitInfo,
    filter_bead_id: Option<&str>,
) -> Vec<BeadEvent> {
    // BTreeMap keeps deterministic sorted iteration (Go sorts explicitly).
    let mut old_beads: BTreeMap<String, BeadSnapshot> = BTreeMap::new();
    let mut new_beads: BTreeMap<String, BeadSnapshot> = BTreeMap::new();

    for line in diff.split('\n') {
        if ignorable_metadata_line(line) {
            continue;
        }
        if let Some(json) = line.strip_prefix('-') {
            if let Some(snap) = parse_bead_json(json) {
                if filter_bead_id.is_none_or(|f| snap.id == f) {
                    old_beads.insert(snap.id.clone(), snap);
                }
            }
        } else if let Some(json) = line.strip_prefix('+') {
            if let Some(snap) = parse_bead_json(json) {
                if filter_bead_id.is_none_or(|f| snap.id == f) {
                    new_beads.insert(snap.id.clone(), snap);
                }
            }
        }
    }

    let mut events = Vec::new();
    for (bead_id, new_snap) in &new_beads {
        match old_beads.get(bead_id) {
            None => {
                events.push(BeadEvent {
                    bead_id: bead_id.clone(),
                    event_type: EventType::Created,
                    timestamp: info.timestamp.clone(),
                    commit_sha: info.sha.clone(),
                    commit_msg: info.message.clone(),
                    author: info.author.clone(),
                    author_email: info.author_email.clone(),
                });
            }
            Some(old_snap) => {
                let event_type = if old_snap.status != new_snap.status {
                    determine_status_event(&old_snap.status, &new_snap.status)
                } else {
                    EventType::Modified
                };
                events.push(BeadEvent {
                    bead_id: bead_id.clone(),
                    event_type,
                    timestamp: info.timestamp.clone(),
                    commit_sha: info.sha.clone(),
                    commit_msg: info.message.clone(),
                    author: info.author.clone(),
                    author_email: info.author_email.clone(),
                });
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOG: &str = concat!(
        "abc123def4567890abc123def4567890abc12344\x00",
        "2026-08-22T10:00:00Z\x00",
        "Tran Quang Dang\x00",
        "dev@example.com\x00",
        "beads: close phase1a\n"
    );

    #[test]
    fn parses_created_event_from_added_line() {
        let log = format!(
            "{SAMPLE_LOG}{}",
            "+{\"id\":\"X-1\",\"title\":\"New\",\"status\":\"open\"}\n"
        );
        let events = parse_log_output(&log, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].bead_id, "X-1");
        assert_eq!(events[0].event_type, EventType::Created);
        assert_eq!(
            events[0].commit_sha,
            "abc123def4567890abc123def4567890abc12344"
        );
    }

    #[test]
    fn status_transition_open_to_in_progress_is_claimed() {
        let log = format!(
            "{SAMPLE_LOG}{}{}",
            "-{\"id\":\"X-1\",\"title\":\"T\",\"status\":\"open\"}\n",
            "+{\"id\":\"X-1\",\"title\":\"T\",\"status\":\"in_progress\"}\n"
        );
        let events = parse_log_output(&log, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Claimed);
    }

    #[test]
    fn closed_to_open_is_reopened() {
        let log = format!(
            "{SAMPLE_LOG}{}{}",
            "-{\"id\":\"X-1\",\"title\":\"T\",\"status\":\"closed\"}\n",
            "+{\"id\":\"X-1\",\"title\":\"T\",\"status\":\"open\"}\n"
        );
        let events = parse_log_output(&log, None);
        assert_eq!(events[0].event_type, EventType::Reopened);
    }

    #[test]
    fn same_status_change_is_modified() {
        let log = format!(
            "{SAMPLE_LOG}{}{}",
            "-{\"id\":\"X-1\",\"title\":\"Old title\",\"status\":\"open\"}\n",
            "+{\"id\":\"X-1\",\"title\":\"New title\",\"status\":\"open\"}\n"
        );
        let events = parse_log_output(&log, None);
        assert_eq!(events[0].event_type, EventType::Modified);
    }

    #[test]
    fn metadata_lines_ignored() {
        let log = format!(
            "{SAMPLE_LOG}{}{}{}{}",
            "diff --git a/.beads/issues.jsonl b/.beads/issues.jsonl\n",
            "index abc..def 100644\n",
            "@@ -1 +1 @@\n",
            "+{\"id\":\"Y-9\",\"title\":\"N\",\"status\":\"open\"}\n"
        );
        let events = parse_log_output(&log, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].bead_id, "Y-9");
    }

    #[test]
    fn filter_restricts_to_bead() {
        let log = format!(
            "{SAMPLE_LOG}{}{}",
            "+{\"id\":\"A-1\",\"title\":\"A\",\"status\":\"open\"}\n",
            "+{\"id\":\"B-2\",\"title\":\"B\",\"status\":\"open\"}\n"
        );
        let events = parse_log_output(&log, Some("B-2"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].bead_id, "B-2");
    }

    #[test]
    fn deletion_only_produces_no_event() {
        // Go parity: hadOld && !hasNew is not tracked as an event type.
        let log = format!(
            "{SAMPLE_LOG}{}",
            "-{\"id\":\"GONE\",\"title\":\"x\",\"status\":\"open\"}\n"
        );
        let events = parse_log_output(&log, None);
        assert!(events.is_empty());
    }
}
