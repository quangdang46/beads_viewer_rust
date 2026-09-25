//! Agent brief bundle — port of the `--agent-brief` payload assembly in Go
//! `cmd/bv/main.go:3915-4007` plus its `generateJQHelpers` constant
//! (`cmd/bv/main.go:6940-7020`).
//!
//! The bundle is a directory of five files: the full triage payload, the
//! insights payload, the priority brief (the very same generator
//! [`crate::priority_brief`] produces), a static jq cheat-sheet, and a
//! `meta.json` that names all five. Only the jq sheet and the meta shape live
//! here; the CLI owns writing the directory and the two envelope-wrapped JSON
//! payloads, because those depend on the robot dispatch context that
//! `bv-export` deliberately does not depend on.
//!
//! The `meta.json` shape is Go's anonymous struct at `cmd/bv/main.go:3990-3996`:
//! an embedded `RobotEnvelope` followed by `issue_count` and `files`. Go's
//! `encoding/json` inlines embedded-struct fields at the embedded field's
//! position and then emits the rest in declaration order, so the result is
//! "envelope keys, then `issue_count`, then `files`". [`build_agent_brief_meta`]
//! reproduces that by appending to a clone of the caller's envelope object
//! rather than by rebuilding it, which keeps the caller's key order.

use serde_json::{json, Map, Value};

/// The five bundle members Go writes, in the order it writes them
/// (`cmd/bv/main.go:3921`, `:3943`, `:3964`, `:3980`, `:3988`). Go's `meta.json`
/// `files` array lists all five *including itself*
/// (`cmd/bv/main.go:3996`), so this is both the write order and the manifest.
pub const AGENT_BRIEF_FILES: [&str; 5] = [
    "triage.json",
    "insights.json",
    "brief.md",
    "helpers.md",
    "meta.json",
];

/// Prepended to `--priority-brief` output when the robot dispatch context is
/// not claim-safe — Go `cmd/bv/main.go:3901`.
pub const PRIORITY_BRIEF_PROVISIONAL_BANNER: &str = "> Readiness is provisional because source data is incomplete or stale. Restore the affected sources before claiming work.\n\n";

/// The agent-brief wording of the same banner. It is *not* the same string as
/// [`PRIORITY_BRIEF_PROVISIONAL_BANNER`]: Go spells the two independently
/// (`cmd/bv/main.go:3901` versus `:3974`) and points the agent-brief reader at
/// `source_authority` in the triage file instead of at the sources themselves.
pub const AGENT_BRIEF_PROVISIONAL_BANNER: &str =
    "> Readiness is provisional; inspect source_authority in triage.json before claiming work.\n\n";

/// Go `generateJQHelpers` (`cmd/bv/main.go:6940-7020`).
///
/// A constant: the Go function concatenates raw and interpreted string
/// literals into a fixed 1853-byte document with no interpolation. The body
/// below is that document byte-for-byte, including the trailing newline after
/// the final fence. It is reproduced by evaluating the Go return expression
/// from `cmd/bv/main.go:6941-7019` rather than by retyping it, so the fences,
/// the `$id` jq binding and the single-quoted filters are all exact.
pub fn generate_jq_helpers() -> &'static str {
    JQ_HELPERS
}

/// Go's `meta.json` anonymous struct (`cmd/bv/main.go:3990-3996`).
///
/// `envelope` is the caller-serialised `RobotEnvelope` object. A non-object
/// envelope is not representable in Go (the embedded struct is a struct, not a
/// pointer), so it is coerced to an empty object rather than rejected.
pub fn build_agent_brief_meta(envelope: &Value, issue_count: usize) -> Value {
    let mut map: Map<String, Value> = match envelope {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    map.insert("issue_count".into(), json!(issue_count));
    map.insert(
        "files".into(),
        Value::Array(
            AGENT_BRIEF_FILES
                .iter()
                .map(|f| Value::String((*f).to_string()))
                .collect(),
        ),
    );
    Value::Object(map)
}

const JQ_HELPERS: &str = r#"# jq Helper Snippets

Quick reference for extracting data from the agent brief JSON files.

## triage.json

### Top Picks
```bash
# Get top 3 recommendations
jq '.quick_ref.top_picks[:3]' triage.json

# Get IDs of top picks
jq '.quick_ref.top_picks[].id' triage.json

# Get top pick with highest unblocks
jq '.quick_ref.top_picks | max_by(.unblocks)' triage.json
```

### Recommendations
```bash
# List all recommendations with scores
jq '.recommendations[] | {id, score, action}' triage.json

# Filter high-score items (score > 0.15)
jq '.recommendations[] | select(.score > 0.15)' triage.json

# Get breakdown metrics
jq '.recommendations[] | {id, pr: .breakdown.pagerank_norm, bw: .breakdown.betweenness_norm}' triage.json
```

### Quick Wins
```bash
# List quick wins
jq '.quick_wins[] | {id, title, reason}' triage.json

# Count quick wins
jq '.quick_wins | length' triage.json
```

### Blockers
```bash
# Get actionable blockers
jq '.blockers_to_clear[] | select(.actionable)' triage.json

# Sort by unblocks count
jq '.blockers_to_clear | sort_by(-.unblocks_count)' triage.json
```

## insights.json

### Graph Metrics
```bash
# Top PageRank issues
jq '.top_pagerank | to_entries | sort_by(-.value)[:5]' insights.json

# Top betweenness centrality
jq '.top_betweenness | to_entries | sort_by(-.value)[:5]' insights.json

# Find hub issues (high in-degree)
jq '.top_in_degree | to_entries | sort_by(-.value)[:3]' insights.json
```

### Project Health
```bash
# Get velocity metrics
jq '.velocity' insights.json

# List critical issues
jq '.critical_issues' insights.json
```

## Combining Files
```bash
# Cross-reference top picks with insights
jq -s '.[0].quick_ref.top_picks[0].id as $id | .[1].top_pagerank[$id] // 0' triage.json insights.json

# Export summary to CSV
jq -r '.recommendations[] | [.id, .score, .action] | @csv' triage.json
```
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_list_matches_go_write_order() {
        // cmd/bv/main.go:3996 — the manifest lists meta.json too.
        assert_eq!(
            AGENT_BRIEF_FILES,
            [
                "triage.json",
                "insights.json",
                "brief.md",
                "helpers.md",
                "meta.json"
            ]
        );
    }

    #[test]
    fn meta_puts_envelope_keys_first() {
        // Go inlines the embedded RobotEnvelope ahead of issue_count/files.
        let envelope = json!({"generated_at": "2024-01-15T10:00:00Z", "data_hash": "abc"});
        let meta = build_agent_brief_meta(&envelope, 42);
        let keys: Vec<&str> = meta
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["generated_at", "data_hash", "issue_count", "files"]);
        assert_eq!(meta["issue_count"], json!(42));
        assert_eq!(meta["files"], json!(AGENT_BRIEF_FILES));
    }

    #[test]
    fn meta_tolerates_non_object_envelope() {
        let meta = build_agent_brief_meta(&Value::Null, 0);
        assert_eq!(meta, json!({"issue_count": 0, "files": AGENT_BRIEF_FILES}));
    }

    #[test]
    fn banners_differ_but_both_end_with_blank_line() {
        assert_ne!(
            PRIORITY_BRIEF_PROVISIONAL_BANNER,
            AGENT_BRIEF_PROVISIONAL_BANNER
        );
        for banner in [
            PRIORITY_BRIEF_PROVISIONAL_BANNER,
            AGENT_BRIEF_PROVISIONAL_BANNER,
        ] {
            assert!(banner.starts_with("> Readiness is provisional"));
            assert!(banner.ends_with("\n\n"));
        }
    }

    #[test]
    fn jq_helpers_is_the_go_document() {
        // cmd/bv/main.go:6940-7020 — a constant with no interpolation.
        let doc = generate_jq_helpers();
        assert_eq!(doc.len(), 1853, "byte length drifted from Go's constant");
        assert!(doc.starts_with("# jq Helper Snippets\n\n"));
        // The Go return expression ends with a fence and a single newline.
        assert!(doc.ends_with(
            "jq -r '.recommendations[] | [.id, .score, .action] | @csv' triage.json\n```\n"
        ));
        // Seven bash blocks, so fourteen fence lines.
        assert_eq!(doc.matches("\n```").count(), 14);
        // No CRLF crept in from a Windows checkout of the Go source.
        assert!(!doc.contains('\r'));
    }

    #[test]
    fn jq_helpers_covers_every_bundle_member() {
        let doc = generate_jq_helpers();
        for member in ["triage.json", "insights.json"] {
            assert!(doc.contains(member), "helpers.md never mentions {member}");
        }
    }
}
