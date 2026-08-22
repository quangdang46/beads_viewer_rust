//! Mermaid + markdown export — port of Go `pkg/export/mermaid_generator.go`
//! and `markdown.go` sanitization/class rules.

use bv_core::model::{Issue, Status};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Go: `sanitizeMermaidID` — keep letters/digits/-/_ else drop; empty → "node".
pub fn sanitize_mermaid_id(id: &str) -> String {
    let result: String = id
        .chars()
        .filter(|r| r.is_alphanumeric() || *r == '-' || *r == '_')
        .collect();
    if result.is_empty() {
        "node".to_string()
    } else {
        result
    }
}

/// Go: `sanitizeMermaidText` — escape/replace problematic chars, truncate 40.
pub fn sanitize_mermaid_text(text: &str) -> String {
    let replaced = text
        .replace('"', "'")
        .replace('[', "(")
        .replace(']', ")")
        .replace('{', "(")
        .replace('}', ")")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "/")
        .replace('`', "'")
        .replace('\n', " ")
        .replace('\r', "");
    let cleaned: String = replaced.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    let runes: Vec<char> = trimmed.chars().collect();
    if runes.len() > 40 {
        let head: String = runes[..37].iter().collect();
        format!("{head}...")
    } else {
        trimmed.to_string()
    }
}

/// FNV-1a 32-bit hash (Go fnv.New32a) for collision suffixes.
fn fnv1a32(data: &[u8]) -> u32 {
    const OFFSET: u32 = 0x811C_9DC5;
    const PRIME: u32 = 0x0100_0193;
    let mut h = OFFSET;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(PRIME);
    }
    h
}

fn status_class(status: Status) -> &'static str {
    match status {
        Status::Closed | Status::Tombstone => "closed",
        Status::InProgress => "inprogress",
        Status::Blocked => "blocked",
        _ => "open",
    }
}

/// Build the mermaid `graph TD` diagram. Deterministic: issues sorted by ID,
/// FNV-32a suffix on sanitized-ID collisions.
pub fn generate_mermaid(issues: &[Issue]) -> String {
    let mut sb = String::new();
    sb.push_str("graph TD\n\n");
    sb.push_str("    classDef open fill:#50FA7B,stroke:#333,color:#000\n");
    sb.push_str("    classDef inprogress fill:#8BE9FD,stroke:#333,color:#000\n");
    sb.push_str("    classDef blocked fill:#FF5555,stroke:#333,color:#000\n");
    sb.push_str("    classDef closed fill:#6272A4,stroke:#333,color:#fff\n");
    sb.push('\n');

    // Sort by ID for determinism.
    let mut sorted: Vec<&Issue> = issues.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    // Collision-free safe IDs.
    let mut safe_map: BTreeMap<String, String> = BTreeMap::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let get_safe = |orig: &str,
                    safe_map: &mut BTreeMap<String, String>,
                    used: &mut std::collections::HashSet<String>|
     -> String {
        if let Some(safe) = safe_map.get(orig) {
            return safe.clone();
        }
        let base = sanitize_mermaid_id(orig);
        let base = if base.is_empty() {
            "node".to_string()
        } else {
            base
        };
        let mut safe = base.clone();
        if used.contains(&safe) {
            let h = fnv1a32(orig.as_bytes());
            safe = format!("{base}_{h:x}");
        }
        used.insert(safe.clone());
        safe_map.insert(orig.to_string(), safe.clone());
        safe
    };

    for i in &sorted {
        get_safe(&i.id, &mut safe_map, &mut used);
    }

    let mut has_links = false;

    // Nodes.
    for i in &sorted {
        let safe_id = &safe_map[&i.id];
        let safe_title = sanitize_mermaid_text(&i.title);
        let safe_label_id = sanitize_mermaid_text(&i.id);
        let _ = writeln!(sb, "    {safe_id}[\"{safe_label_id}<br/>{safe_title}\"]");

        let class = status_class(i.status);
        let _ = writeln!(sb, "    {safe_id}:::{class}");
    }

    sb.push('\n');

    // Edges: blocking ==> thick; related -.-> dashed.
    for i in &sorted {
        let from = match safe_map.get(&i.id) {
            Some(x) => x,
            None => continue,
        };
        let mut deps = i.dependencies.clone();
        deps.sort_by(|a, b| a.effective_depends_on().cmp(b.effective_depends_on()));
        for dep in &deps {
            let target = dep.effective_depends_on().to_string();
            let Some(to) = safe_map.get(&target) else {
                continue;
            };
            has_links = true;
            if dep.r#type.is_blocking() {
                let _ = writeln!(sb, "    {from} ==> {to}");
            } else {
                let _ = writeln!(sb, "    {from} -.-> {to}");
            }
        }
    }

    if !has_links {
        sb.push_str("\n    %% No dependencies found\n");
    }
    sb
}

/// Full markdown report (Go GenerateMarkdown subset): title header,
/// summary table, per-issue sections with metadata, mermaid diagram.
pub fn generate_markdown(issues: &[Issue], title: &str) -> String {
    let mut sb = String::new();
    let _ = writeln!(sb, "# {title}\n");
    let now = jiff::Timestamp::now();
    let _ = writeln!(sb, "_Generated {}_\n", now);

    // Summary table.
    let open = issues
        .iter()
        .filter(|i| matches!(i.status, Status::Open))
        .count();
    let in_progress = issues
        .iter()
        .filter(|i| matches!(i.status, Status::InProgress))
        .count();
    let blocked = issues
        .iter()
        .filter(|i| matches!(i.status, Status::Blocked))
        .count();
    let closed = issues.iter().filter(|i| i.status.is_closed()).count();

    let _ = writeln!(
        sb,
        "| Metric | Count |\n|---|---|\n| Total | {} |\n| Open | {open} |\n| In Progress | {in_progress} |\n| Blocked | {blocked} |\n| Closed | {closed} |\n",
        issues.len()
    );

    // TOC anchors.
    sb.push_str("## Table of Contents\n\n");
    let mut sorted: Vec<&Issue> = issues.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for i in &sorted {
        let slug = slugify(&i.id);
        let _ = writeln!(sb, "- [{}](#{})", i.id, slug);
    }

    sb.push_str("\n## Dependency Graph\n\n```mermaid\n");
    sb.push_str(&generate_mermaid(issues));
    sb.push_str("```\n\n## Issues\n");

    for i in &sorted {
        let slug = slugify(&i.id);
        let _ = write!(sb, "\n### {} {}\n\n", i.id, sanitize_mermaid_text(&i.title));
        let _ = writeln!(
            sb,
            "| Field | Value |\n|---|---|\n| Status | {} |\n| Priority | P{} |\n| Type | {} |{}",
            i.status.as_str(),
            i.priority,
            i.issue_type,
            if i.assignee.is_empty() {
                String::new()
            } else {
                format!("\n| Assignee | {} |", i.assignee)
            }
        );
        if !i.description.is_empty() {
            sb.push_str("\n#### Description\n\n");
            sb.push_str(&i.description);
            sb.push('\n');
        }
        let _ = writeln!(sb, "\n[Back to top](#table-of-contents)");
        let _ = slug;
    }
    sb
}

fn slugify(id: &str) -> String {
    let lower = id.to_lowercase();
    let mut out = String::new();
    let mut last_dash = true;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bv_core::model::{Dependency, DependencyType};

    fn issue(id: &str, title: &str, status: Status) -> Issue {
        Issue {
            id: id.into(),
            content_hash: String::new(),
            title: title.into(),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status,
            priority: 2,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: None,
            updated_at: None,
            due_date: None,
            closed_at: None,
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

    #[test]
    fn sanitize_id_keeps_safe_chars_only() {
        assert_eq!(sanitize_mermaid_id("bv-123"), "bv-123");
        assert_eq!(sanitize_mermaid_id("a b/c"), "abc");
        assert_eq!(sanitize_mermaid_id("!!!"), "node");
    }

    #[test]
    fn sanitize_text_escapes_and_truncates() {
        assert_eq!(sanitize_mermaid_text("[a] <b>"), "(a) &lt;b&gt;");
        assert_eq!(sanitize_mermaid_text("a|b"), "a/b");
        let long = "x".repeat(50);
        let s = sanitize_mermaid_text(&long);
        assert_eq!(s.chars().count(), 40);
        assert!(s.ends_with("..."));
    }

    #[test]
    fn mermaid_contains_classdefs_and_nodes() {
        let issues = vec![
            issue("A-1", "First", Status::Open),
            issue("B-2", "Second", Status::Closed),
        ];
        let m = generate_mermaid(&issues);
        assert!(m.starts_with("graph TD"));
        assert!(m.contains("classDef open fill:#50FA7B"));
        assert!(m.contains("A_1") || m.contains("A-1"));
        assert!(m.contains(":::closed"));
    }

    #[test]
    fn blocking_edge_uses_thick_arrow() {
        let mut a = issue("A-1", "Dependent", Status::Open);
        a.dependencies.push(Dependency {
            issue_id: "A-1".into(),
            depends_on_id: "B-2".into(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: DependencyType::Blocks,
            created_at: None,
            created_by: String::new(),
        });
        let b = issue("B-2", "Dependency", Status::Open);
        let m = generate_mermaid(&[b, a]);
        assert!(m.contains("==>"), "blocking edge uses ==>");
    }

    #[test]
    fn related_edge_uses_dashed() {
        let mut a = issue("A-1", "Related", Status::Open);
        a.dependencies.push(Dependency {
            issue_id: "A-1".into(),
            depends_on_id: "B-2".into(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: DependencyType::parse("related"),
            created_at: None,
            created_by: String::new(),
        });
        let b = issue("B-2", "Other", Status::Open);
        let m = generate_mermaid(&[a, b]);
        assert!(m.contains("-.->"), "related edge uses -.->");
    }

    #[test]
    fn collision_gets_fnv_suffix() {
        // Two IDs that sanitize to the same safe form.
        let issues = vec![
            issue("a b", "One", Status::Open),
            issue("ab!", "Two", Status::Open),
        ];
        let m = generate_mermaid(&issues);
        assert!(m.contains("_") && (m.contains("ab_") || m.contains("a_b")));
    }

    #[test]
    fn markdown_has_summary_and_toc() {
        let issues = vec![issue("X-1", "Test", Status::Open)];
        let md = generate_markdown(&issues, "My Report");
        assert!(md.starts_with("# My Report"));
        assert!(md.contains("| Total | 1 |"));
        assert!(md.contains("## Table of Contents"));
        assert!(md.contains("```mermaid"));
    }
}
