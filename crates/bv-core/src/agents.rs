//! AGENTS.md integration — port of Go `pkg/agents`.
//!
//! Handles detection, content injection, and preference storage for
//! automatically adding beads_viewer usage instructions to agent
//! configuration files (AGENTS.md, CLAUDE.md, etc.).

pub mod detect;
pub mod file;
pub mod prefs;

/// Current blurb version. Increment when making breaking changes.
/// v4: rename Go `bv` references to Rust `bvr` + fix repo links
/// (quangdang46/beads_viewer_rust) so the injected instructions match this binary.
pub const BLURB_VERSION: i32 = 4;
pub const BLURB_START_MARKER: &str = "<!-- bv-agent-instructions-v4 -->";
pub const BLURB_END_MARKER: &str = "<!-- end-bv-agent-instructions -->";

pub const SUPPORTED_AGENT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "agents.md", "claude.md"];

/// The full v4 agent blurb content.
pub const AGENT_BLURB: &str = r#"<!-- bv-agent-instructions-v4 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`) for issue tracking and [beads_viewer_rust](https://github.com/quangdang46/beads_viewer_rust) (`bvr`) for graph-aware triage. Issues are stored in `.beads/` and tracked in git. Current `br` workspaces normally export `.beads/issues.jsonl`; older `bd`/legacy workspaces may use `.beads/beads.jsonl`. `bvr` auto-discovers the supported JSONL files, so agents should use `br`/`bvr` commands instead of hard-coding a single filename.

### Using bvr as an AI sidecar

bvr is a graph-aware triage engine for Beads projects. Instead of parsing .beads/issues.jsonl / .beads/beads.jsonl directly or hallucinating graph traversal, use robot flags for deterministic, dependency-aware outputs with precomputed metrics (PageRank, betweenness, critical path, cycles, HITS, eigenvector, k-core).

**Scope boundary:** bvr handles *what to work on* (triage, priority, planning). `br` handles creating, modifying, and closing beads.

**CRITICAL: Use ONLY --robot-* flags. Bare bvr launches an interactive TUI that blocks your session.**

#### The Workflow: Start With Triage

**`bvr --robot-triage` is your single entry point.** It returns everything you need in one call:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

```bash
bvr --robot-triage        # THE MEGA-COMMAND: start here
bvr --robot-next          # Minimal: just the single top pick + claim command
```

Before claiming, verify current state with `br show <id> --json` or `br ready --json`. `recommendations` can include graph-important blocked or assigned work; only `quick_ref.top_picks` and non-empty `claim_command` fields represent claimable work.

#### Other bvr Commands

| Command | Purpose |
|---------|---------|
| `bvr --robot-insights` | Deep graph analysis: PageRank, betweenness, HITS, k-core, critical path |
| `bvr --robot-plan` | Dependency-respecting execution plan with parallel tracks |
| `bvr --robot-priority` | Priority misalignment detection |
| `bvr --robot-alerts` | Stale issues, blocking cascades |
| `bvr --robot-suggest` | Smart suggestions: duplicates, missing dependencies, labels |
| `bvr --robot-graph` | Dependency graph export (JSON/DOT/Mermaid) |
| `bvr --robot-search <query>` | Semantic search over issue titles/descriptions |
| `bvr --robot-history` | Bead-to-commit correlation from git history |
| `bvr --robot-label-health` | Per-label health metrics |
| `bvr --robot-schema` | JSON Schema definitions for all robot outputs |

#### br Quick Reference

```bash
br list                          # List all beads
br ready                         # Actionable beads (no open blockers)
br show <id>                     # View bead details
br update <id> --status in_progress  # Claim work
br update <id> --status closed   # Complete work
br dep add <id> <target>         # Add dependency
br sync --flush-only             # Sync changes to issues.jsonl
```
<!-- end-bv-agent-instructions -->
"#;

/// Get the preferred agent file path for a new file.
/// Returns AGENTS.md in the project root.
pub fn get_preferred_agent_file_path(work_dir: &std::path::Path) -> std::path::PathBuf {
    work_dir.join("AGENTS.md")
}

/// Check if content contains a blurb (current, older versioned, or legacy).
/// Any `bv-agent-instructions-vN` marker counts — version mismatch is handled
/// separately by `get_blurb_version` + `needs_upgrade`, so old blurbs still
/// trigger the upgrade path instead of a duplicate append.
pub fn contains_any_blurb(content: &str) -> bool {
    contains_legacy_blurb(content)
        || content.contains(BLURB_START_MARKER)
        || content.contains("bv-agent-instructions-v")
}

/// Check if content contains a legacy blurb (pre-v4).
/// Requires ALL patterns: the v3 blurb has "### Using bv as an AI sidecar"
/// but NOT "bv already computes the hard parts" — that's the key
/// differentiator that makes the check reliable. Any v3 blurb (with `bv`
/// commands instead of `bvr`) is now treated as outdated: `needs_upgrade`
/// fires on version mismatch, and `update_blurb` strips the old section by
/// its markers.
pub fn contains_legacy_blurb(content: &str) -> bool {
    let patterns = [
        "### Using bv as an AI sidecar",
        "--robot-insights",
        "--robot-plan",
        "bv already computes the hard parts",
    ];
    let matches = patterns.iter().filter(|p| content.contains(*p)).count();
    matches == patterns.len()
}

/// Get the blurb version from content. Returns 0 if none found.
/// The marker format is `<!-- bv-agent-instructions-vN -->` — extract the
/// number after `v`.
pub fn get_blurb_version(content: &str) -> i32 {
    // Match pattern: bv-agent-instructions-vN
    if let Some(pos) = content.find("bv-agent-instructions-v") {
        let rest = &content[pos + "bv-agent-instructions-v".len()..];
        if let Some(end) = rest.find("-->") {
            let version_str = &rest[..end].trim();
            return version_str.parse().unwrap_or(0);
        }
    }
    0
}

/// Replace legacy blurb with current version.
/// Strips ANY versioned marker section (`bv-agent-instructions-vN`), not just
/// the current one, so older blurbs (e.g. v3) upgrade cleanly to v4.
pub fn update_blurb(content: &str) -> String {
    // Remove everything that looks like a blurb — any versioned markers plus
    // legacy content. The legacy blurb contains patterns not in v3+:
    // "bv already computes the hard parts" is the key differentiator.
    let mut cleaned = content.to_string();

    // Remove any marker-wrapped section (any version, not just current).
    if let Some(start) = cleaned.find("bv-agent-instructions-v") {
        // Rewind to the opening "<!--" of the start marker.
        let section_start = cleaned[..start].rfind("<!--").unwrap_or(start);
        if let Some(end) = cleaned[start..].find(BLURB_END_MARKER) {
            let end_pos = start + end + BLURB_END_MARKER.len();
            let before = cleaned[..section_start].trim_end().to_string();
            let after = if end_pos < cleaned.len() {
                cleaned[end_pos..].trim_start().to_string()
            } else {
                String::new()
            };
            cleaned = if after.is_empty() {
                before
            } else {
                format!(
                    "{before}

{after}"
                )
            };
        }
    }

    // Remove any remaining legacy blurb content
    // (the "## Beads Workflow Integration" section before the markers)
    if let Some(legacy_start) = cleaned.find("## Beads Workflow Integration") {
        let mut legacy_end = cleaned.len();
        let search_from = legacy_start + 25;
        for line in cleaned[search_from..].lines() {
            if line.starts_with("## ") && !line.starts_with("### ") {
                if let Some(pos) = cleaned[search_from..].find(line) {
                    legacy_end = search_from + pos;
                }
                break;
            }
        }
        let before = cleaned[..legacy_start].trim_end().to_string();
        let after = if legacy_end < cleaned.len() {
            cleaned[legacy_end..].trim_start().to_string()
        } else {
            String::new()
        };
        cleaned = if after.is_empty() {
            before
        } else {
            format!(
                "{before}

{after}"
            )
        };
    }

    append_blurb(&cleaned)
}

/// Remove blurb from content (both legacy and current markers).
/// Order: remove marker-wrapped content first, then any legacy remnant.
pub fn remove_blurb(content: &str) -> String {
    // Step 1: Remove marker-wrapped content.
    if let Some(start) = content.find(BLURB_START_MARKER) {
        if let Some(end) = content[start..].find(BLURB_END_MARKER) {
            let end_pos = start + end + BLURB_END_MARKER.len();
            let before = content[..start].trim_end().to_string();
            let after = if end_pos < content.len() {
                content[end_pos..].trim_start().to_string()
            } else {
                String::new()
            };
            let mut result = before;
            if !after.is_empty() {
                result.push_str("\n\n");
                result.push_str(&after);
            }
            return result;
        }
    }

    // Step 2: Remove legacy blurb that's NOT between markers.
    if let Some(legacy_start) = content.find("## Beads Workflow Integration") {
        let mut legacy_end = content.len();
        let search_from = legacy_start + 25;
        for line in content[search_from..].lines() {
            if line.starts_with("## ") && !line.starts_with("### ") {
                if let Some(pos) = content[search_from..].find(line) {
                    legacy_end = search_from + pos;
                }
                break;
            }
        }
        let before = content[..legacy_start].trim_end().to_string();
        let after = if legacy_end < content.len() {
            content[legacy_end..].trim_start().to_string()
        } else {
            String::new()
        };
        if after.is_empty() {
            return before;
        }
        return format!("{before}\n\n{after}");
    }

    content.to_string()
}

/// Append blurb to content.
pub fn append_blurb(content: &str) -> String {
    let mut result = content.trim_end().to_string();
    result.push_str("\n\n");
    result.push_str(AGENT_BLURB);
    result.push('\n');
    result
}

/// Verify blurb is present in content.
pub fn verify_blurb_present(content: &str) -> bool {
    content.contains(BLURB_START_MARKER) && content.contains(BLURB_END_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blurb_markers_present() {
        assert!(AGENT_BLURB.contains(BLURB_START_MARKER));
        assert!(AGENT_BLURB.contains(BLURB_END_MARKER));
    }

    #[test]
    fn version_is_4() {
        assert_eq!(BLURB_VERSION, 4);
    }

    #[test]
    fn append_and_verify() {
        let content = "# My Project\n\nSome text here.";
        let result = append_blurb(content);
        assert!(verify_blurb_present(&result));
        assert!(result.starts_with("# My Project"));
    }

    #[test]
    fn remove_blurb_cleans_up() {
        let content = "# Project\n\n";
        let with_blurb = append_blurb(content);
        let cleaned = remove_blurb(&with_blurb);
        assert!(!cleaned.contains(BLURB_START_MARKER));
        assert!(cleaned.contains("# Project"));
    }

    #[test]
    fn get_blurb_version_extracts_number() {
        let content = format!("{BLURB_START_MARKER}\nsome content\n{BLURB_END_MARKER}");
        assert_eq!(get_blurb_version(&content), BLURB_VERSION);
    }

    #[test]
    fn v3_blurb_needs_upgrade_to_v4() {
        // The old v3 content (Go `bv` commands) must be detected as outdated
        // so the TUI offers an upgrade and `update_blurb` replaces it.
        let v3 = "<!-- bv-agent-instructions-v3 -->\nbv --robot-triage\n<!-- end-bv-agent-instructions -->";
        assert!(contains_any_blurb(v3));
        assert_eq!(get_blurb_version(v3), 3);
        assert!(get_blurb_version(v3) < BLURB_VERSION);
        let upgraded = update_blurb(v3);
        assert!(verify_blurb_present(&upgraded));
        assert!(upgraded.contains("bvr --robot-triage"));
        assert!(!upgraded.contains("bv-agent-instructions-v3"));
    }

    #[test]
    fn supported_files_includes_common() {
        assert!(SUPPORTED_AGENT_FILES.contains(&"AGENTS.md"));
        assert!(SUPPORTED_AGENT_FILES.contains(&"CLAUDE.md"));
    }
}
