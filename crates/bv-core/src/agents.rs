//! AGENTS.md integration — port of Go `pkg/agents`.
//!
//! Handles detection, content injection, and preference storage for
//! automatically adding beads_viewer usage instructions to agent
//! configuration files (AGENTS.md, CLAUDE.md, etc.).

pub mod detect;
pub mod file;
pub mod prefs;

/// Current blurb version. Increment when making breaking changes.
pub const BLURB_VERSION: i32 = 3;
pub const BLURB_START_MARKER: &str = "<!-- bv-agent-instructions-v3 -->";
pub const BLURB_END_MARKER: &str = "<!-- end-bv-agent-instructions -->";

pub const SUPPORTED_AGENT_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "agents.md",
    "claude.md",
];

/// The full v3 agent blurb content.
pub const AGENT_BLURB: &str = r#"<!-- bv-agent-instructions-v3 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`) for issue tracking and [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) (`bv`) for graph-aware triage. Issues are stored in `.beads/` and tracked in git. Current `br` workspaces normally export `.beads/issues.jsonl`; older `bd`/legacy workspaces may use `.beads/beads.jsonl`. `bv` auto-discovers the supported JSONL files, so agents should use `br`/`bv` commands instead of hard-coding a single filename.

### Using bv as an AI sidecar

bv is a graph-aware triage engine for Beads projects. Instead of parsing .beads/issues.jsonl / .beads/beads.jsonl directly or hallucinating graph traversal, use robot flags for deterministic, dependency-aware outputs with precomputed metrics (PageRank, betweenness, critical path, cycles, HITS, eigenvector, k-core).

**Scope boundary:** bv handles *what to work on* (triage, priority, planning). `br` handles creating, modifying, and closing beads.

**CRITICAL: Use ONLY --robot-* flags. Bare bv launches an interactive TUI that blocks your session.**

#### The Workflow: Start With Triage

**`bv --robot-triage` is your single entry point.** It returns everything you need in one call:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

```bash
bv --robot-triage        # THE MEGA-COMMAND: start here
bv --robot-next          # Minimal: just the single top pick + claim command
```

Before claiming, verify current state with `br show <id> --json` or `br ready --json`. `recommendations` can include graph-important blocked or assigned work; only `quick_ref.top_picks` and non-empty `claim_command` fields represent claimable work.

#### Other bv Commands

| Command | Purpose |
|---------|---------|
| `bv --robot-insights` | Deep graph analysis: PageRank, betweenness, HITS, k-core, critical path |
| `bv --robot-plan` | Dependency-respecting execution plan with parallel tracks |
| `bv --robot-priority` | Priority misalignment detection |
| `bv --robot-alerts` | Stale issues, blocking cascades |
| `bv --robot-suggest` | Smart suggestions: duplicates, missing dependencies, labels |
| `bv --robot-graph` | Dependency graph export (JSON/DOT/Mermaid) |
| `bv --robot-search <query>` | Semantic search over issue titles/descriptions |
| `bv --robot-history` | Bead-to-commit correlation from git history |
| `bv --robot-label-health` | Per-label health metrics |
| `bv --robot-schema` | JSON Schema definitions for all robot outputs |

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

/// Check if content contains a blurb (current or legacy).
pub fn contains_any_blurb(content: &str) -> bool {
    contains_legacy_blurb(content) || content.contains(BLURB_START_MARKER)
}

/// Check if content contains a legacy blurb (pre-v1).
/// Go requires ALL legacy patterns (the v3 blurb has "### Using bv as an
/// AI sidecar" but NOT "bv already computes the hard parts" — that's the
/// key differentiator that makes the check reliable).
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
pub fn update_blurb(content: &str) -> String {
    // Remove everything that looks like a blurb — both legacy and current
    // markers. The legacy blurb contains patterns not in v3:
    // "bv already computes the hard parts" is the key differentiator.
    let mut cleaned = content.to_string();

    // Remove current marker-wrapped section
    if let Some(start) = cleaned.find(BLURB_START_MARKER) {
        if let Some(end) = cleaned[start..].find(BLURB_END_MARKER) {
            let end_pos = start + end + BLURB_END_MARKER.len();
            let before = cleaned[..start].trim_end().to_string();
            let after = if end_pos < cleaned.len() {
                cleaned[end_pos..].trim_start().to_string()
            } else {
                String::new()
            };
            cleaned = if after.is_empty() {
                before
            } else {
                format!("{before}

{after}")
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
            format!("{before}

{after}")
        };
    }

    append_blurb(&cleaned)
}

/// Remove blurb from content (both legacy and current markers).
/// Order: remove legacy first, then marker-wrapped content.
pub fn remove_blurb(content: &str) -> String {
    let mut result = content.to_string();

    // 1. Remove legacy blurb (section between "## Beads Workflow Integration"
    //    and next ## heading, OR content containing robot- patterns).
    if let Some(legacy_start) = result.find("## Beads Workflow Integration") {
        let mut legacy_end = result.len();
        let search_from = legacy_start + 25;
        for line in result[search_from..].lines() {
            if line.starts_with("## ") && !line.starts_with("### ") {
                if let Some(pos) = result[search_from..].find(line) {
                    legacy_end = search_from + pos;
                }
                break;
            }
        }
        let before = result[..legacy_start].trim_end().to_string();
        let after = if legacy_end < result.len() {
            result[legacy_end..].trim_start().to_string()
        } else {
            String::new()
        };
        result = if after.is_empty() {
            before
        } else {
            format!("{before}\n\n{after}")
        };
    }

    // 2. Remove marker-wrapped content.
    if let Some(start) = result.find(BLURB_START_MARKER) {
        if let Some(end) = result[start..].find(BLURB_END_MARKER) {
            let end_pos = start + end + BLURB_END_MARKER.len();
            let mut cleaned = result[..start].trim_end().to_string();
            if end_pos < result.len() {
                let rest = result[end_pos..].trim_start();
                if !rest.is_empty() {
                    cleaned.push_str("\n\n");
                    cleaned.push_str(rest);
                }
            }
            return cleaned;
        }
    }

    result
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
    fn version_is_3() {
        assert_eq!(BLURB_VERSION, 3);
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
        assert_eq!(get_blurb_version(&content), 3);
    }

    #[test]
    fn supported_files_includes_common() {
        assert!(SUPPORTED_AGENT_FILES.contains(&"AGENTS.md"));
        assert!(SUPPORTED_AGENT_FILES.contains(&"CLAUDE.md"));
    }
}
