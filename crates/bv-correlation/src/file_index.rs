//! File→bead reverse index — port of Go `pkg/correlation/file_index.go`.
//!
//! This is the public, full-fidelity port of Go's `FileLookup` and
//! `CoChangeMatrix`. `orphan.rs` previously carried a private, reduced copy of
//! the exact-match arm; that copy is gone and it now uses [`FileLookup`] from
//! here, so there is exactly one implementation of the index in this crate.
//!
//! Two Go behaviours are load-bearing and easy to lose in translation, so they
//! are called out at their definitions:
//! - [`normalize_path`] strips exactly one leading `./` and one trailing `/`
//!   (`strings.TrimPrefix`/`TrimSuffix`, not a repeat-strip), so `"a//"`
//!   normalizes to `"a/"`.
//! - [`CoChangeMatrix::get_related_files`] maps a non-positive threshold back
//!   to Go's `0.5` default rather than treating it as "include everything".

use crate::history::{self, HistoryReport};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Go `BeadReference` — links a bead to a file via commits.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BeadReference {
    pub bead_id: String,
    pub title: String,
    /// open/in_progress/closed.
    pub status: String,
    /// Which commits linked this bead to this file.
    pub commit_shas: Vec<String>,
    /// Most recent commit timestamp (RFC3339, as stored in the report).
    pub last_touch: String,
    /// Sum of insertions + deletions across commits.
    pub total_changes: i64,
}

/// Go `FileIndexStats` — aggregate statistics about the file index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct FileIndexStats {
    /// Number of unique files.
    pub total_files: usize,
    /// Sum of all bead references.
    pub total_bead_links: usize,
    /// Files touched by more than one bead.
    pub files_with_multiple_beads: usize,
}

/// Go `FileBeadLookupResult` — the result of looking up beads for a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileBeadLookupResult {
    pub file_path: String,
    /// Currently open beads.
    pub open_beads: Vec<BeadReference>,
    /// Recently closed beads.
    pub closed_beads: Vec<BeadReference>,
    pub total_beads: usize,
}

/// Go `FileBeadIndex` — O(1) lookup from file path to the beads that touched it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FileBeadIndex {
    /// Normalized file path → beads that modified it.
    pub file_to_beads: BTreeMap<String, Vec<BeadReference>>,
    pub stats: FileIndexStats,
}

/// Go `FileLookup` — file-to-bead lookup, plus the co-change matrix it carries.
#[derive(Debug, Clone, Default)]
pub struct FileLookup {
    index: FileBeadIndex,
    /// BeadID → (title, status), refreshed at lookup time because a status can
    /// change after the index was built.
    beads: BTreeMap<String, (String, String)>,
    co_change: CoChangeMatrix,
}

/// Go `normalizePath`.
///
/// Strips exactly one leading `./` and exactly one trailing `/`. Go uses
/// `strings.TrimPrefix`/`strings.TrimSuffix`, so this is deliberately a single
/// strip rather than `trim_end_matches`.
pub fn normalize_path(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let no_prefix = slashed.strip_prefix("./").unwrap_or(&slashed);
    no_prefix.strip_suffix('/').unwrap_or(no_prefix).to_string()
}

/// Go `classifyBeadStatus` — `(bucket, skip)`.
///
/// `skip` marks a tombstone, which the index drops at build time so every
/// lookup surface (hotspots, per-file lookups, related work) agrees on the same
/// bead set (Go issue #184).
pub fn classify_bead_status(status: &str) -> (&'static str, bool) {
    match status.trim().to_lowercase().as_str() {
        "tombstone" => ("", true),
        "closed" => ("closed", false),
        _ => ("open", false),
    }
}

/// Go `normalizeStatus` (related.go) / the same normalization in orphan.go.
pub fn normalize_status(status: &str) -> String {
    status.trim().to_lowercase()
}

/// Go `isClosedHistoryStatus` (orphan.go:190).
pub fn is_closed_history_status(status: &str) -> bool {
    let normalized = normalize_status(status);
    normalized == "closed" || normalized == "tombstone"
}

/// Go `sortBeadRefs` — most recently touched first, bead ID breaking ties.
fn sort_bead_refs(refs: &mut [BeadReference]) {
    refs.sort_by(|a, b| {
        ts_cmp_str(&b.last_touch, &a.last_touch).then_with(|| a.bead_id.cmp(&b.bead_id))
    });
}

/// String-ordered timestamp comparison. RFC3339 instants compare correctly once
/// parsed; unparseable values fall back to lexical order so the sort stays
/// total.
fn ts_cmp_str(a: &str, b: &str) -> std::cmp::Ordering {
    match (history::parse_ts(a), history::parse_ts(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
}

/// Go `appendUnique` (orphan.go:612).
fn append_unique(slice: &mut Vec<String>, s: &str) {
    if !slice.iter().any(|existing| existing == s) {
        slice.push(s.to_string());
    }
}

/// Go `accumulateBeadReference` (file_index.go:815) — merge a second sighting of
/// the same bead into the map built by the prefix/glob arms.
fn accumulate_bead_reference(
    refs: &mut BTreeMap<String, BeadReference>,
    mut reference: BeadReference,
) {
    let Some(existing) = refs.get_mut(&reference.bead_id) else {
        refs.insert(reference.bead_id.clone(), reference);
        return;
    };
    if !reference.title.is_empty() {
        existing.title = std::mem::take(&mut reference.title);
    }
    if !reference.status.is_empty() {
        existing.status = std::mem::take(&mut reference.status);
    }
    let shas = std::mem::take(&mut reference.commit_shas);
    for sha in &shas {
        append_unique(&mut existing.commit_shas, sha);
    }
    if ts_cmp_str(&reference.last_touch, &existing.last_touch) == std::cmp::Ordering::Greater {
        existing.last_touch = std::mem::take(&mut reference.last_touch);
    }
    existing.total_changes += reference.total_changes;
}

/// Go `beadReferencesFromMap` (file_index.go:839).
fn bead_references_from_map(refs: BTreeMap<String, BeadReference>) -> Vec<BeadReference> {
    refs.into_values()
        .map(|mut reference| {
            reference.commit_shas.sort();
            reference
        })
        .collect()
}

impl FileLookup {
    /// Go `NewFileLookup` — build the file index and the co-change matrix from a
    /// history report.
    pub fn new(report: &HistoryReport) -> Self {
        Self {
            index: build_file_index(report),
            beads: report
                .histories
                .iter()
                .map(|(id, h)| (id.clone(), (h.title.clone(), h.status.clone())))
                .collect(),
            co_change: CoChangeMatrix::build(report),
        }
    }

    /// Go `LookupByFile` — beads that touched `path`. `path` may be an exact
    /// file or a directory prefix.
    pub fn lookup_by_file(&self, path: &str) -> FileBeadLookupResult {
        let normalized_path = normalize_path(path);
        let mut result = FileBeadLookupResult {
            file_path: path.to_string(),
            open_beads: Vec::new(),
            closed_beads: Vec::new(),
            total_beads: 0,
        };

        // Exact match first.
        if let Some(refs) = self.index.file_to_beads.get(&normalized_path) {
            for reference in refs {
                let reference = self.refresh(reference);
                let (bucket, skip) = classify_bead_status(&reference.status);
                if skip {
                    continue;
                }
                if bucket == "closed" {
                    result.closed_beads.push(reference);
                } else {
                    result.open_beads.push(reference);
                }
            }
            sort_bead_refs(&mut result.open_beads);
            sort_bead_refs(&mut result.closed_beads);
            result.total_beads = result.open_beads.len() + result.closed_beads.len();
            return result;
        }

        // Prefix match, for directory lookups. `normalize_path` has already
        // folded backslashes to forward slashes, so only "/" needs checking.
        let mut open_refs: BTreeMap<String, BeadReference> = BTreeMap::new();
        let mut closed_refs: BTreeMap<String, BeadReference> = BTreeMap::new();
        let prefix = format!("{normalized_path}/");
        for (file_path, refs) in &self.index.file_to_beads {
            if !file_path.starts_with(&prefix) {
                continue;
            }
            for reference in refs {
                let reference = self.refresh(reference);
                let (bucket, skip) = classify_bead_status(&reference.status);
                if skip {
                    continue;
                }
                if bucket == "closed" {
                    accumulate_bead_reference(&mut closed_refs, reference);
                } else {
                    accumulate_bead_reference(&mut open_refs, reference);
                }
            }
        }

        result.open_beads = bead_references_from_map(open_refs);
        result.closed_beads = bead_references_from_map(closed_refs);
        sort_bead_refs(&mut result.open_beads);
        sort_bead_refs(&mut result.closed_beads);
        result.total_beads = result.open_beads.len() + result.closed_beads.len();
        result
    }

    /// Go `LookupByFileGlob` — beads for files matching a glob pattern.
    pub fn lookup_by_file_glob(&self, pattern: &str) -> FileBeadLookupResult {
        let mut open_refs: BTreeMap<String, BeadReference> = BTreeMap::new();
        let mut closed_refs: BTreeMap<String, BeadReference> = BTreeMap::new();

        for (file_path, refs) in &self.index.file_to_beads {
            if !glob_match(pattern, file_path) {
                continue;
            }
            for reference in refs {
                let reference = self.refresh(reference);
                let (bucket, skip) = classify_bead_status(&reference.status);
                if skip {
                    continue;
                }
                if bucket == "closed" {
                    accumulate_bead_reference(&mut closed_refs, reference);
                } else {
                    accumulate_bead_reference(&mut open_refs, reference);
                }
            }
        }

        let mut result = FileBeadLookupResult {
            file_path: pattern.to_string(),
            open_beads: bead_references_from_map(open_refs),
            closed_beads: bead_references_from_map(closed_refs),
            total_beads: 0,
        };
        sort_bead_refs(&mut result.open_beads);
        sort_bead_refs(&mut result.closed_beads);
        result.total_beads = result.open_beads.len() + result.closed_beads.len();
        result
    }

    /// Go `GetAllFiles` — every file in the index, sorted by path.
    pub fn all_files(&self) -> Vec<String> {
        self.index.file_to_beads.keys().cloned().collect()
    }

    /// Go `GetStats`.
    pub fn stats(&self) -> FileIndexStats {
        self.index.stats
    }

    /// Go `GetRelatedFiles` — files that frequently co-change with `file_path`.
    /// `threshold` is the minimum correlation (defaulting to 0.5 when
    /// non-positive); `limit` caps the result (defaulting to 10).
    pub fn get_related_files(
        &self,
        file_path: &str,
        threshold: f64,
        limit: usize,
    ) -> CoChangeResult {
        self.co_change
            .get_related_files(file_path, threshold, limit)
    }

    /// Go `GetCoChangeMatrix` — the underlying matrix for advanced queries.
    pub fn co_change_matrix(&self) -> &CoChangeMatrix {
        &self.co_change
    }

    /// Re-read a bead's title/status from the report, which may have moved on
    /// since the index was built (Go's inline `fl.beads[ref.BeadID]` refresh).
    fn refresh(&self, reference: &BeadReference) -> BeadReference {
        let Some((title, status)) = self.beads.get(&reference.bead_id) else {
            return reference.clone();
        };
        BeadReference {
            title: title.clone(),
            status: status.clone(),
            ..reference.clone()
        }
    }
}

/// Go `BuildFileIndex` — map every file a correlated commit touched to the beads
/// behind that commit.
///
/// Beads whose status classifies as skip (tombstone) are excluded at build time
/// so every consumer of this index agrees on the same bead set (Go #184).
pub fn build_file_index(report: &HistoryReport) -> FileBeadIndex {
    // file → beadID → reference, for deduplication.
    let mut file_bead_map: HashMap<String, BTreeMap<String, BeadReference>> = HashMap::new();

    for (bead_id, history) in &report.histories {
        if classify_bead_status(&history.status).1 {
            continue;
        }
        for commit in history.commits.iter().flatten() {
            for file in &commit.files {
                let entry = file_bead_map
                    .entry(normalize_path(&file.path))
                    .or_default()
                    .entry(bead_id.clone())
                    .or_insert_with(|| BeadReference {
                        bead_id: bead_id.clone(),
                        title: history.title.clone(),
                        status: history.status.clone(),
                        last_touch: commit.timestamp.clone(),
                        ..Default::default()
                    });
                append_unique(&mut entry.commit_shas, &commit.short_sha);
                if ts_cmp_str(&commit.timestamp, &entry.last_touch) == std::cmp::Ordering::Greater {
                    entry.last_touch = commit.timestamp.clone();
                }
                entry.total_changes += file.insertions + file.deletions;
            }
        }
    }

    let mut file_to_beads = BTreeMap::new();
    let mut total_links = 0usize;
    let mut multiple_beads_count = 0usize;

    for (file_path, bead_map) in file_bead_map {
        let mut refs: Vec<BeadReference> = bead_map.into_values().collect();
        for reference in &mut refs {
            reference.commit_shas.sort();
        }
        // Most recent first, bead ID breaking ties.
        sort_bead_refs(&mut refs);
        total_links += refs.len();
        if refs.len() > 1 {
            multiple_beads_count += 1;
        }
        file_to_beads.insert(file_path, refs);
    }

    let stats = FileIndexStats {
        total_files: file_to_beads.len(),
        total_bead_links: total_links,
        files_with_multiple_beads: multiple_beads_count,
    };
    FileBeadIndex {
        file_to_beads,
        stats,
    }
}

/// Go `CoChangeEntry` — a file that frequently co-changes with another file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoChangeEntry {
    /// The related file.
    pub file_path: String,
    /// Commits where both files changed.
    pub co_change_count: usize,
    /// Total commits touching the source file.
    pub total_commits: usize,
    /// `co_change_count / total_commits`, in 0.0-1.0.
    pub correlation: f64,
    /// Up to 3 sample full commit SHAs, sorted.
    pub sample_commits: Vec<String>,
}

/// Go `CoChangeResult`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoChangeResult {
    /// The queried file.
    pub file_path: String,
    /// Total commits touching this file.
    pub total_commits: usize,
    /// Files that co-change, sorted by correlation descending.
    pub related_files: Vec<CoChangeEntry>,
    /// The effective minimum correlation threshold used.
    pub threshold: f64,
}

/// Go `CoChangeMatrix` — which files frequently change together.
#[derive(Debug, Clone, Default)]
pub struct CoChangeMatrix {
    /// file → related file → count of commits where both changed.
    matrix: BTreeMap<String, BTreeMap<String, usize>>,
    /// file → total commits touching that file.
    file_commit_counts: BTreeMap<String, usize>,
    /// Full commit SHA → files changed in that commit, for sample selection.
    commit_files: BTreeMap<String, Vec<String>>,
}

impl CoChangeMatrix {
    /// Go `GetRelatedFiles` — files that co-change with `file_path`.
    ///
    /// A non-positive `threshold` maps back to Go's `0.5` default; a zero
    /// `limit` maps to 10.
    pub fn get_related_files(
        &self,
        file_path: &str,
        threshold: f64,
        limit: usize,
    ) -> CoChangeResult {
        let threshold = if threshold <= 0.0 { 0.5 } else { threshold };
        let limit = if limit == 0 { 10 } else { limit };
        let normalized_path = normalize_path(file_path);

        let total_commits = self
            .file_commit_counts
            .get(&normalized_path)
            .copied()
            .unwrap_or(0);
        let mut result = CoChangeResult {
            file_path: file_path.to_string(),
            total_commits,
            related_files: Vec::new(),
            threshold,
        };
        if total_commits == 0 {
            // File not found in history.
            return result;
        }
        let Some(related) = self.matrix.get(&normalized_path) else {
            // No co-changes found.
            return result;
        };

        let mut entries: Vec<CoChangeEntry> = related
            .iter()
            .filter_map(|(related_file, &count)| {
                let correlation = count as f64 / total_commits as f64;
                if correlation < threshold {
                    return None;
                }
                Some(CoChangeEntry {
                    file_path: related_file.clone(),
                    co_change_count: count,
                    total_commits,
                    correlation,
                    sample_commits: self.sample_commits(&normalized_path, related_file),
                })
            })
            .collect();

        // Correlation descending, path breaking ties. The float comparison is
        // exact: every value here is count/total for one shared denominator.
        entries.sort_by(|a, b| {
            b.correlation
                .partial_cmp(&a.correlation)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.file_path.cmp(&b.file_path))
        });
        entries.truncate(limit);
        result.related_files = entries;
        result
    }

    /// Total commits touching `file_path` (normalized).
    pub fn total_commits(&self, file_path: &str) -> usize {
        self.file_commit_counts
            .get(&normalize_path(file_path))
            .copied()
            .unwrap_or(0)
    }

    /// Commits where both files changed. Collected then sorted — taking the
    /// first three straight from a map made robot output vary between runs.
    fn sample_commits(&self, normalized_path: &str, related_file: &str) -> Vec<String> {
        let mut sample_commits: Vec<String> = self
            .commit_files
            .iter()
            .filter(|(_, files)| {
                files.iter().any(|f| f == normalized_path)
                    && files.iter().any(|f| f == related_file)
            })
            .map(|(sha, _)| sha.clone())
            .collect();
        sample_commits.sort();
        sample_commits.truncate(3);
        sample_commits
    }
}

/// Go `BuildCoChangeMatrix` — analyze which files frequently change together in
/// the same commits.
///
/// Note the deliberate asymmetry with [`build_file_index`]: Go filters
/// tombstones there (file_index.go:70-72) but not here (file_index.go:430-432),
/// so a tombstoned bead's commits still count toward co-change. This port keeps
/// that difference rather than "fixing" it, because co-change is a property of
/// commit history rather than of current bead membership.
pub fn build_co_change_matrix(report: &HistoryReport) -> CoChangeMatrix {
    CoChangeMatrix::build(report)
}

impl CoChangeMatrix {
    fn build(report: &HistoryReport) -> Self {
        let mut matrix = CoChangeMatrix::default();
        // A commit can appear in several bead histories; count it once.
        let mut processed_commits: HashSet<&str> = HashSet::new();

        for history in report.histories.values() {
            for commit in history.commits.iter().flatten() {
                if !processed_commits.insert(commit.sha.as_str()) {
                    continue;
                }

                // Normalize, dedupe, then sort this commit's file list.
                let mut files: Vec<String> = Vec::new();
                let mut seen_files: HashSet<String> = HashSet::new();
                for file in &commit.files {
                    let normalized = normalize_path(&file.path);
                    if normalized.is_empty() {
                        continue;
                    }
                    if !seen_files.insert(normalized.clone()) {
                        continue;
                    }
                    files.push(normalized);
                }
                files.sort();

                // Keyed by the full SHA: seven-character prefixes are not unique
                // and can otherwise overwrite one another in long histories.
                matrix
                    .commit_files
                    .insert(commit.sha.clone(), files.clone());

                for file in &files {
                    *matrix.file_commit_counts.entry(file.clone()).or_insert(0) += 1;
                }

                // All ordered pairs; the self-relationship is skipped.
                for file_a in &files {
                    let row = matrix.matrix.entry(file_a.clone()).or_default();
                    for file_b in &files {
                        if file_a == file_b {
                            continue;
                        }
                        *row.entry(file_b.clone()).or_insert(0) += 1;
                    }
                }
            }
        }

        matrix
    }
}

/// Go `filepath.Match` — shell-style glob over `/`-separated path segments.
///
/// `*` does not cross `/` and `?` matches exactly one non-`/` character, as in
/// Go. Returns false for a malformed pattern, mirroring Go's `(false, err)` arm
/// at file_index.go:260.
fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    glob_match_inner(&pattern, &name)
}

fn glob_match_inner(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => {
            // `*` matches any run of non-separator characters.
            for skip in 0..=name.len() {
                if skip > 0 && name[skip - 1] == '/' {
                    break;
                }
                if glob_match_inner(&pattern[1..], &name[skip..]) {
                    return true;
                }
            }
            false
        }
        Some('?') => {
            !name.is_empty() && name[0] != '/' && glob_match_inner(&pattern[1..], &name[1..])
        }
        Some('[') => {
            match match_class(pattern, name) {
                Some((consumed, matched)) => {
                    matched && glob_match_inner(&pattern[consumed..], &name[1..])
                }
                // Unterminated class: Go treats the '[' as a literal.
                None => {
                    !name.is_empty()
                        && name[0] == '['
                        && glob_match_inner(&pattern[1..], &name[1..])
                }
            }
        }
        Some(c) => !name.is_empty() && name[0] == *c && glob_match_inner(&pattern[1..], &name[1..]),
    }
}

/// Match a `[...]` class. Returns the pattern length consumed and whether the
/// first name character matched, or `None` when the class is unterminated.
fn match_class(pattern: &[char], name: &[char]) -> Option<(usize, bool)> {
    let mut i = 1usize; // past '['
    let negated = matches!(pattern.get(i), Some('^') | Some('!'));
    if negated {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    // A `]` immediately after the (optional) `^` is a literal.
    while i < pattern.len() {
        if pattern[i] == ']' && !first {
            i += 1;
            return Some((i, matched != negated));
        }
        first = false;
        let lo = pattern[i];
        if pattern.get(i + 1) == Some(&'-') && pattern.get(i + 2).is_some_and(|c| *c != ']') {
            let hi = pattern[i + 2];
            if !name.is_empty() && name[0] >= lo && name[0] <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if !name.is_empty() && name[0] == lo {
                matched = true;
            }
            i += 1;
        }
    }
    None // unterminated class
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryReport;
    use crate::history::{
        BeadHistory, BeadMilestones, FileChange, HistoryCommit, HistoryStats, HistoryWindow,
    };
    use std::collections::BTreeMap;

    fn commit(sha: &str, ts: &str, files: &[(&str, i64, i64)]) -> HistoryCommit {
        HistoryCommit {
            bead_id: String::new(),
            sha: sha.to_string(),
            short_sha: sha.chars().take(7).collect(),
            message: String::new(),
            author: String::new(),
            author_email: String::new(),
            timestamp: ts.to_string(),
            files: files
                .iter()
                .map(|(path, ins, del)| FileChange {
                    path: path.to_string(),
                    action: "M".into(),
                    insertions: *ins,
                    deletions: *del,
                })
                .collect(),
            method: "explicit_id",
            methods: vec!["explicit_id".into()],
            confidence: 1.0,
            reason: String::new(),
            confirmed: false,
        }
    }

    fn history(
        bead_id: &str,
        title: &str,
        status: &str,
        commits: Vec<HistoryCommit>,
    ) -> BeadHistory {
        BeadHistory {
            bead_id: bead_id.into(),
            title: title.into(),
            status: status.into(),
            events: Vec::new(),
            milestones: BeadMilestones::default(),
            commits: Some(commits),
            cycle_time: None,
            last_author: String::new(),
        }
    }

    fn report(histories: BTreeMap<String, BeadHistory>) -> HistoryReport {
        HistoryReport {
            generated_at: "2026-01-01T00:00:00Z".into(),
            data_hash: String::new(),
            git_range: String::new(),
            latest_commit_sha: String::new(),
            window: HistoryWindow {
                revision: String::new(),
                limit: 0,
                since: None,
                until: None,
                commits: 0,
            },
            stats: HistoryStats {
                total_beads: 0,
                beads_with_commits: 0,
                total_commits: 0,
                unique_authors: 0,
                avg_commits_per_bead: 0.0,
                avg_cycle_time_days: None,
                method_distribution: BTreeMap::new(),
                strategies: None,
                feedback_applied: None,
            },
            commit_index: crate::history::build_commit_index(&histories),
            histories,
            causal_history: None,
        }
    }

    #[test]
    fn normalize_path_strips_one_prefix_and_one_suffix() {
        // Go uses TrimPrefix/TrimSuffix, so a doubled slash leaves one behind.
        assert_eq!(normalize_path("./a/b.rs"), "a/b.rs");
        assert_eq!(normalize_path(".\\a\\b.rs"), "a/b.rs");
        assert_eq!(normalize_path("a/b/"), "a/b");
        assert_eq!(normalize_path("a//"), "a/");
        // Only one leading "./" is removed, even when doubled up.
        assert_eq!(normalize_path("././/a"), ".//a");
    }

    #[test]
    fn classify_bead_status_matches_go() {
        assert_eq!(classify_bead_status("tombstone"), ("", true));
        assert_eq!(classify_bead_status("  TOMBSTONE "), ("", true));
        assert_eq!(classify_bead_status("closed"), ("closed", false));
        assert_eq!(classify_bead_status("open"), ("open", false));
        assert_eq!(classify_bead_status("in_progress"), ("open", false));
        assert_eq!(classify_bead_status(""), ("open", false));
    }

    /// bd-1 (closed) touches a.rs in two commits and b.rs in one; bd-2 (open)
    /// touches b.rs. SHAs are 8 chars so their 7-char prefixes stay distinct.
    fn sample_report() -> HistoryReport {
        let mut histories = BTreeMap::new();
        histories.insert(
            "bd-1".into(),
            history(
                "bd-1",
                "first",
                "closed",
                vec![
                    commit(
                        "aaaaaa11",
                        "2026-01-01T00:00:00Z",
                        &[("src/a.rs", 10, 2), ("src/b.rs", 5, 1)],
                    ),
                    commit("aaaaaa22", "2026-01-02T00:00:00Z", &[("src/a.rs", 3, 0)]),
                ],
            ),
        );
        histories.insert(
            "bd-2".into(),
            history(
                "bd-2",
                "second",
                "open",
                vec![commit(
                    "bbbbbb11",
                    "2026-01-03T00:00:00Z",
                    &[("src/b.rs", 7, 3)],
                )],
            ),
        );
        report(histories)
    }

    /// `sample_report` plus a tombstoned bead that also touched a.rs, for
    /// testing the two different tombstone policies.
    fn tombstoned_report() -> HistoryReport {
        let mut histories = BTreeMap::new();
        histories.insert(
            "bd-1".into(),
            history(
                "bd-1",
                "first",
                "closed",
                vec![commit(
                    "aaaaaa11",
                    "2026-01-01T00:00:00Z",
                    &[("src/a.rs", 10, 2)],
                )],
            ),
        );
        histories.insert(
            "bd-2".into(),
            history(
                "bd-2",
                "gone",
                "tombstone",
                vec![commit(
                    "cccccc11",
                    "2026-01-04T00:00:00Z",
                    &[("src/a.rs", 1, 1)],
                )],
            ),
        );
        report(histories)
    }

    #[test]
    fn build_file_index_accumulates_changes_and_keeps_distinct_shas() {
        let index = build_file_index(&sample_report());
        assert_eq!(index.stats.total_files, 2);
        assert_eq!(index.stats.total_bead_links, 3, "a.rs×1 + b.rs×2");
        assert_eq!(
            index.stats.files_with_multiple_beads, 1,
            "only b.rs has two beads"
        );
        let a_refs = &index.file_to_beads["src/a.rs"];
        assert_eq!(a_refs.len(), 1);
        assert_eq!(a_refs[0].bead_id, "bd-1");
        // Short SHAs, deduped and sorted.
        assert_eq!(a_refs[0].commit_shas, vec!["aaaaaa1", "aaaaaa2"]);
        assert_eq!(a_refs[0].total_changes, 10 + 2 + 3);
        assert_eq!(a_refs[0].last_touch, "2026-01-02T00:00:00Z");
    }

    #[test]
    fn build_file_index_excludes_tombstoned_beads() {
        let index = build_file_index(&tombstoned_report());
        let a_refs = &index.file_to_beads["src/a.rs"];
        assert_eq!(a_refs.len(), 1, "tombstoned bd-2 must not reach the index");
        assert_eq!(a_refs[0].bead_id, "bd-1");
    }

    #[test]
    fn co_change_matrix_keeps_tombstoned_beads() {
        // The mirror image of the test above: Go filters tombstones when
        // building the file index but not when building the co-change matrix,
        // so bd-2's commit still counts toward a.rs.
        let lookup = FileLookup::new(&tombstoned_report());
        assert_eq!(lookup.co_change_matrix().total_commits("src/a.rs"), 2);
        assert_eq!(
            build_file_index(&tombstoned_report())
                .stats
                .total_bead_links,
            1
        );
    }

    #[test]
    fn lookup_by_file_splits_open_and_closed_and_counts_total() {
        let lookup = FileLookup::new(&sample_report());
        let result = lookup.lookup_by_file("src/b.rs");
        assert_eq!(result.file_path, "src/b.rs");
        assert_eq!(result.total_beads, 2);
        assert_eq!(
            result
                .open_beads
                .iter()
                .map(|r| r.bead_id.as_str())
                .collect::<Vec<_>>(),
            vec!["bd-2"]
        );
        assert_eq!(
            result
                .closed_beads
                .iter()
                .map(|r| r.bead_id.as_str())
                .collect::<Vec<_>>(),
            vec!["bd-1"]
        );
    }

    #[test]
    fn lookup_by_file_prefix_merges_across_a_directory() {
        let lookup = FileLookup::new(&sample_report());
        // The exact arm misses, so the prefix arm runs and accumulates bd-1
        // across both of its files instead of returning it twice.
        let result = lookup.lookup_by_file("src");
        assert_eq!(result.total_beads, 2);
        let closed = &result.closed_beads[0];
        assert_eq!(closed.bead_id, "bd-1");
        assert_eq!(closed.total_changes, 10 + 2 + 5 + 1 + 3);
        assert_eq!(closed.commit_shas, vec!["aaaaaa1", "aaaaaa2"]);
    }

    #[test]
    fn lookup_by_file_unknown_path_is_empty() {
        let lookup = FileLookup::new(&sample_report());
        let result = lookup.lookup_by_file("nope.rs");
        assert_eq!(result.total_beads, 0);
        assert!(result.open_beads.is_empty());
        assert!(result.closed_beads.is_empty());
    }

    #[test]
    fn co_change_matrix_counts_a_commit_once() {
        let lookup = FileLookup::new(&sample_report());
        let matrix = lookup.co_change_matrix();
        // a.rs is in aaaaaa11 and aaaaaa22; b.rs in aaaaaa11 and bbbbbb11.
        assert_eq!(matrix.total_commits("src/a.rs"), 2);
        assert_eq!(matrix.total_commits("src/b.rs"), 2);
    }

    #[test]
    fn related_files_default_threshold_is_half_not_zero() {
        let lookup = FileLookup::new(&sample_report());
        // threshold 0.0 maps back to Go's 0.5 default: b.rs co-changed with
        // a.rs in 1 of the 2 commits touching a.rs, so it is exactly at 0.5.
        // A threshold of 0.0 must NOT be read as "include everything".
        let at_default = lookup.get_related_files("src/a.rs", 0.0, 10);
        assert_eq!(at_default.threshold, 0.5);
        assert_eq!(at_default.total_commits, 2);
        assert_eq!(at_default.related_files.len(), 1);
        assert_eq!(at_default.related_files[0].file_path, "src/b.rs");
        assert_eq!(at_default.related_files[0].co_change_count, 1);
        assert_eq!(at_default.related_files[0].correlation, 0.5);
        assert_eq!(at_default.related_files[0].sample_commits, vec!["aaaaaa11"]);
    }

    #[test]
    fn related_files_threshold_above_correlation_filters_everything() {
        let lookup = FileLookup::new(&sample_report());
        let strict = lookup.get_related_files("src/a.rs", 0.99, 10);
        assert!(strict.related_files.is_empty());
        assert_eq!(strict.threshold, 0.99);
        // The queried file itself is still reported.
        assert_eq!(strict.total_commits, 2);
    }

    #[test]
    fn related_files_limit_truncates() {
        let lookup = FileLookup::new(&sample_report());
        let capped = lookup.get_related_files("src/a.rs", 0.1, 0);
        assert_eq!(
            capped.related_files.len(),
            1,
            "only b.rs co-changes with a.rs"
        );
    }

    #[test]
    fn related_files_unknown_file_reports_zero_commits() {
        let lookup = FileLookup::new(&sample_report());
        let result = lookup.get_related_files("nope.rs", 0.5, 10);
        assert_eq!(result.total_commits, 0);
        assert!(result.related_files.is_empty());
    }

    #[test]
    fn glob_star_does_not_cross_separators() {
        assert!(glob_match("src/*.rs", "src/a.rs"));
        assert!(!glob_match("src/*.rs", "src/nested/a.rs"));
        assert!(glob_match("src/*", "src/nested"));
        assert!(glob_match("*.rs", "a.rs"));
        assert!(glob_match("[ab].rs", "b.rs"));
        assert!(!glob_match("[!ab].rs", "b.rs"));
        assert!(glob_match("[a-c].rs", "b.rs"));
    }
}
