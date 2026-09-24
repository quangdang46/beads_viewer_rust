//! Snapshot-path event extraction — port of Go
//! `pkg/correlation/extractor_snapshot.go`'s `extractViaSnapshots`.
//!
//! Go reconstructs lifecycle events from per-commit JSONL *snapshot*
//! differences rather than from git's textual patch. `git log --raw --follow`
//! gives each commit's old/new blob id for the followed beads file; each unique
//! blob is read once through a long-lived `git cat-file --batch`; and the
//! per-commit record diff is computed as a **multiset** difference of the two
//! blobs' record lines.
//!
//! The multiset semantics are the observable point: a commit that only *reorders*
//! records (a tracker "sync/export" commit) has an identical multiset before and
//! after, so it produces an empty diff and **no events** — even though
//! `git log -p` renders it as a long remove/add patch. The legacy patch path
//! cannot see that difference, which is why Go routes large blobs here and why
//! `extract` dispatches on blob size (see `SNAPSHOT_BLOB_SIZE_THRESHOLD`).

use crate::extractor::{parse_diff_text, BeadEvent, CommitHeader, ExtractOptions};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// The followed file's current (HEAD) blob size at or above which `extract`
/// takes this path. Go's `snapshotBlobSizeThreshold`.
pub const SNAPSHOT_BLOB_SIZE_THRESHOLD: i64 = 64 * 1024;

/// Git's null blob id — an add or a delete, not a real parent/child blob.
const NULL_BLOB_SHA: &str = "0000000000000000000000000000000000000000";

/// Go `preferSnapshotPath` — one cheap `git cat-file -s HEAD:<file>`. An
/// unreadable size (no history yet, untracked file) also selects this path, so
/// the degenerate case degrades to "no commits" rather than a slower native
/// diff.
pub fn prefer_snapshot_path(repo: &Path, beads_rel: &str) -> bool {
    let out = Command::new("git")
        .args(["cat-file", "-s", &format!("HEAD:{beads_rel}")])
        .current_dir(repo)
        .output();
    let Ok(out) = out else { return true };
    if !out.status.success() {
        return true;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    match text.trim().parse::<i64>() {
        Ok(size) => size >= SNAPSHOT_BLOB_SIZE_THRESHOLD,
        Err(_) => true,
    }
}

/// Go `snapshotCommit` — commit metadata plus the followed file's two blob ids.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotCommit {
    header: CommitHeader,
    old_sha: String,
    new_sha: String,
}

/// Go `snapshotCommits` + `parseSnapshotLog` + `parseRawDiffLines`: one
/// metadata-only `git log --raw --no-abbrev --follow` yields, per commit, the
/// header and the first raw diff entry for the followed file.
fn snapshot_commits(
    repo: &Path,
    opts: &ExtractOptions,
    beads_rel: &str,
) -> Result<Vec<SnapshotCommit>, String> {
    let mut args: Vec<String> = vec![
        "log".into(),
        "--raw".into(),
        "--no-abbrev".into(),
        "--follow".into(),
        "--no-color".into(),
        format!("--format={}", crate::extractor::GIT_LOG_HEADER_FORMAT),
    ];
    crate::extractor::append_history_filters(&mut args, opts);
    args.push("--".to_string());
    args.push(beads_rel.to_string());

    let out = Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("spawning git log --raw: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git log --raw failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_snapshot_log(&out.stdout))
}

fn parse_snapshot_log(out: &[u8]) -> Vec<SnapshotCommit> {
    let mut commits = Vec::new();
    for chunk in commit_chunks(out) {
        let Some(nl) = chunk.iter().position(|b| *b == b'\n') else {
            continue;
        };
        let Some(header) = CommitHeader::parse(&chunk[..nl]) else {
            continue;
        };
        let mut sc = SnapshotCommit {
            header,
            old_sha: String::new(),
            new_sha: String::new(),
        };
        if parse_raw_diff_lines(&chunk[nl + 1..], &mut sc) {
            commits.push(sc);
        }
    }
    commits
}

/// Go's `commitPattern.FindAllIndex` + `forEachCommitChunk` boundary detection:
/// a commit header starts at a 40-hex-char run immediately followed by NUL.
fn commit_chunks(out: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    for i in 0..out.len().saturating_sub(40) {
        if out[i..i + 40].iter().all(u8::is_ascii_hexdigit) && out[i + 40] == 0 {
            starts.push(i);
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(idx, &start)| {
            let end = starts.get(idx + 1).copied().unwrap_or(out.len());
            &out[start..end]
        })
        .collect()
}

/// Go `parseRawDiffLines` — the first `:`-prefixed raw line, whose metadata
/// before the TAB is `:<oldmode> <newmode> <oldsha> <newsha> <status>`. A pure
/// deletion (new == null blob) carries no records and is skipped entirely.
fn parse_raw_diff_lines(payload: &[u8], sc: &mut SnapshotCommit) -> bool {
    for line in payload.split(|b| *b == b'\n') {
        if line.first() != Some(&b':') {
            continue;
        }
        let meta_end = line.iter().position(|b| *b == b'\t').unwrap_or(line.len());
        let meta = String::from_utf8_lossy(&line[..meta_end]);
        let fields: Vec<&str> = meta.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        if fields[2] != NULL_BLOB_SHA {
            sc.old_sha = fields[2].to_string();
        }
        if fields[3] != NULL_BLOB_SHA {
            sc.new_sha = fields[3].to_string();
        }
        return !sc.new_sha.is_empty();
    }
    false
}

/// One entry of a blob's record-line multiset: how many times the line occurs
/// plus a representative copy (needed to emit the synthesized diff).
#[derive(Debug, Clone, Default)]
struct RecordLineEntry {
    count: usize,
    text: Vec<u8>,
}

/// A blob's record-line multiset. Go keys this by a 64-bit hash of the line for
/// memory; the line bytes themselves are already in hand here (they are the
/// representative that gets emitted), so keying by content is both exact — no
/// hash-collision risk — and one hash instead of two.
type RecordLineSet = HashMap<Vec<u8>, RecordLineEntry>;

/// Go `scanRecordLineDescriptors` + the set assembly in
/// `buildRecordLineSnapshot`: only lines whose first byte is `{` are records,
/// and a final record without a trailing newline still counts.
fn build_record_line_set(blob: &[u8]) -> RecordLineSet {
    let mut set: RecordLineSet = HashMap::new();
    for line in blob.split(|b| *b == b'\n') {
        // split() yields a trailing empty segment for a newline-terminated blob.
        if line.is_empty() || line[0] != b'{' {
            continue;
        }
        let entry = set.entry(line.to_vec()).or_default();
        if entry.count == 0 {
            entry.text = line.to_vec();
        }
        entry.count += 1;
    }
    set
}

/// Go `synthesizeRecordDiff` — emit `-{line}` once per old occurrence the new
/// side does not account for, then `+{line}` once per new occurrence the old
/// side does not account for. `parse_diff_text` only consumes `+{`/`-{` lines
/// and sorts the affected bead IDs, so the traversal order here is not
/// observable.
fn synthesize_record_diff(old: &RecordLineSet, new: &RecordLineSet) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    for (line, oe) in old {
        let new_count = new.get(line).map_or(0, |e| e.count);
        for _ in 0..oe.count.saturating_sub(new_count) {
            buf.push(b'-');
            buf.extend_from_slice(&oe.text);
            buf.push(b'\n');
        }
    }
    for (line, ne) in new {
        let old_count = old.get(line).map_or(0, |e| e.count);
        for _ in 0..ne.count.saturating_sub(old_count) {
            buf.push(b'+');
            buf.extend_from_slice(&ne.text);
            buf.push(b'\n');
        }
    }
    buf
}

/// A long-lived `git cat-file --batch`: one object id in, one blob out, so N
/// blobs cost one process instead of N forks.
struct BlobReader {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl BlobReader {
    fn open(repo: &Path) -> Result<Self, String> {
        let mut child = Command::new("git")
            .arg("cat-file")
            .arg("--batch")
            .current_dir(repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawning git cat-file: {e}"))?;
        let stdin = child.stdin.take().ok_or("git cat-file stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("git cat-file stdout unavailable")?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::with_capacity(64 * 1024, stdout),
        })
    }

    fn read_blob(&mut self, sha: &str) -> Result<Vec<u8>, String> {
        writeln!(self.stdin, "{sha}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut header = String::new();
        self.stdout
            .read_line(&mut header)
            .map_err(|e| e.to_string())?;
        let fields: Vec<&str> = header.split_whitespace().collect();
        if fields.len() < 3 {
            // "<oid> missing" — an absent blob reads as empty, like Go's
            // readBlobs mapping it to nil.
            return Ok(Vec::new());
        }
        let size: usize = fields[2]
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let mut buf = vec![0u8; size];
        self.stdout
            .read_exact(&mut buf)
            .map_err(|e| e.to_string())?;
        // cat-file terminates each object with a newline.
        let mut nl = [0u8; 1];
        let _ = self.stdout.read_exact(&mut nl);
        Ok(buf)
    }
}

impl Drop for BlobReader {
    fn drop(&mut self) {
        // Closing stdin makes cat-file exit; reap it so no zombie survives.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Go `extractViaSnapshots` — the snapshot extraction, chronological.
///
/// Blobs are read at most once each and only the record-line sets are retained.
/// `last_use` records the final commit index that needs each blob so a set is
/// dropped as soon as no later commit references it: consecutive commits in a
/// followed chain share their boundary blob, so the live window is small even
/// when a tracker rewrites the whole file every mutation.
pub fn extract_via_snapshots(
    repo: &Path,
    opts: &ExtractOptions,
    beads_rel: &str,
) -> Result<Vec<BeadEvent>, String> {
    let commits = snapshot_commits(repo, opts, beads_rel)?;
    if commits.is_empty() {
        return Ok(Vec::new());
    }

    let mut last_use: HashMap<String, usize> = HashMap::new();
    for (i, c) in commits.iter().enumerate() {
        if !c.old_sha.is_empty() {
            last_use
                .entry(c.old_sha.clone())
                .and_modify(|last| *last = (*last).max(i))
                .or_insert(i);
        }
        if !c.new_sha.is_empty() {
            last_use
                .entry(c.new_sha.clone())
                .and_modify(|last| *last = (*last).max(i))
                .or_insert(i);
        }
    }

    let mut reader = BlobReader::open(repo)?;
    let mut live: HashMap<String, RecordLineSet> = HashMap::new();
    let mut events: Vec<BeadEvent> = Vec::new();

    // git log emits newest-first; the per-commit events are concatenated in that
    // order and the whole stream reversed at the end, matching the legacy
    // Extract contract (chronological).
    for (i, c) in commits.iter().enumerate() {
        let new_set = load_set(&mut reader, &mut live, &c.new_sha)?;
        let old_set = load_set(&mut reader, &mut live, &c.old_sha)?;
        let diff = synthesize_record_diff(&old_set, &new_set);
        if !diff.is_empty() {
            let text = String::from_utf8_lossy(&diff).to_string();
            events.extend(parse_diff_text(&text, &c.header, opts.bead_id.as_deref()));
        }
        // Evict every blob whose last referencing commit is at or before i.
        live.retain(|sha, _| last_use.get(sha).is_some_and(|last| *last > i));
    }
    events.reverse();
    Ok(events)
}

fn load_set(
    reader: &mut BlobReader,
    live: &mut HashMap<String, RecordLineSet>,
    sha: &str,
) -> Result<RecordLineSet, String> {
    if sha.is_empty() {
        // No parent blob: every record reads as an addition.
        return Ok(RecordLineSet::new());
    }
    if let Some(set) = live.get(sha) {
        return Ok(set.clone());
    }
    let blob = reader.read_blob(sha)?;
    let set = build_record_line_set(&blob);
    live.insert(sha.to_string(), set.clone());
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(lines: &[&str]) -> RecordLineSet {
        build_record_line_set(lines.join("\n").as_bytes())
    }

    fn diff_text(old: &RecordLineSet, new: &RecordLineSet) -> String {
        String::from_utf8(synthesize_record_diff(old, new)).expect("utf8")
    }

    #[test]
    fn pure_reorder_yields_no_diff() {
        // The whole point of the snapshot path: a tracker "sync/export" commit
        // that only reorders records has an identical multiset before and after.
        let old = set(&[
            r#"{"id":"A","status":"open"}"#,
            r#"{"id":"B","status":"open"}"#,
        ]);
        let new = set(&[
            r#"{"id":"B","status":"open"}"#,
            r#"{"id":"A","status":"open"}"#,
        ]);
        assert_eq!(diff_text(&old, &new), "");
    }

    #[test]
    fn changed_record_emits_removal_then_addition() {
        let old = set(&[r#"{"id":"A","status":"open"}"#]);
        let new = set(&[r#"{"id":"A","status":"closed"}"#]);
        let diff = diff_text(&old, &new);
        assert!(
            diff.contains("-{\"id\":\"A\",\"status\":\"open\"}\n"),
            "{diff}"
        );
        assert!(
            diff.contains("+{\"id\":\"A\",\"status\":\"closed\"}\n"),
            "{diff}"
        );
    }

    #[test]
    fn added_and_removed_records_are_one_sided() {
        let old = set(&[r#"{"id":"A","status":"open"}"#]);
        let new = set(&[r#"{"id":"B","status":"open"}"#]);
        let diff = diff_text(&old, &new);
        assert!(diff.contains("-{\"id\":\"A\""), "{diff}");
        assert!(!diff.contains("+{\"id\":\"A\""), "{diff}");
        assert!(diff.contains("+{\"id\":\"B\""), "{diff}");
        assert!(!diff.contains("-{\"id\":\"B\""), "{diff}");
    }

    #[test]
    fn duplicate_lines_are_counted_as_a_multiset() {
        // Two identical records, one removed: exactly one removal, not two.
        let line = r#"{"id":"A","status":"open"}"#;
        let old = build_record_line_set(format!("{line}\n{line}\n").as_bytes());
        let new = build_record_line_set(format!("{line}\n").as_bytes());
        let diff = diff_text(&old, &new);
        assert_eq!(diff.matches('\n').count(), 1);
        assert!(diff.starts_with('-'), "{diff}");
    }

    #[test]
    fn non_record_lines_are_ignored() {
        let blob = b"not a record\n{\"id\":\"A\"}\n\n";
        let set = build_record_line_set(blob);
        assert_eq!(set.len(), 1);
        assert!(set.contains_key(b"{\"id\":\"A\"}".as_slice()));
    }

    #[test]
    fn final_record_without_newline_is_counted() {
        let set = build_record_line_set(b"{\"id\":\"A\"}");
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn raw_diff_line_fills_blob_ids_and_skips_deletions() {
        let mut sc = SnapshotCommit {
            header: CommitHeader {
                sha: "s".into(),
                timestamp: "t".into(),
                author: "a".into(),
                author_email: "e".into(),
                message: "m".into(),
            },
            old_sha: String::new(),
            new_sha: String::new(),
        };
        let payload = b"\n:100644 100644 aaa bbb M\t.beads/issues.jsonl\n";
        assert!(parse_raw_diff_lines(payload, &mut sc));
        assert_eq!(sc.old_sha, "aaa");
        assert_eq!(sc.new_sha, "bbb");

        let mut del = SnapshotCommit {
            header: sc.header.clone(),
            old_sha: String::new(),
            new_sha: String::new(),
        };
        let del_payload =
            b":100644 000000 aaa 0000000000000000000000000000000000000000 D\t.beads/issues.jsonl\n";
        assert!(!parse_raw_diff_lines(del_payload, &mut del));
    }

    #[test]
    fn raw_add_leaves_old_sha_empty() {
        let mut sc = SnapshotCommit {
            header: CommitHeader {
                sha: "s".into(),
                timestamp: "t".into(),
                author: "a".into(),
                author_email: "e".into(),
                message: "m".into(),
            },
            old_sha: String::new(),
            new_sha: String::new(),
        };
        let payload =
            b":000000 100644 0000000000000000000000000000000000000000 bbb A\t.beads/issues.jsonl\n";
        assert!(parse_raw_diff_lines(payload, &mut sc));
        assert!(sc.old_sha.is_empty(), "null old blob stays empty");
        assert_eq!(sc.new_sha, "bbb");
    }

    #[test]
    fn commit_chunks_split_on_header_pattern() {
        let out = b"aaaaaaaaaabbbbbbbbbbccccccccccccdddddddddd\x00rest\npayload\nsecond-header";
        let chunks = commit_chunks(out);
        assert_eq!(chunks.len(), 1, "only the 40-hex + NUL run is a header");
    }
}
