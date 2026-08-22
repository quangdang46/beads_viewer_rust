//! Byte-exact port of Go `analysis.ComputeDataHash` (pkg/analysis/cache.go:142).
//!
//! Contract: the Rust output MUST equal the Go output for identical issue
//! sets. Verified against golden fixtures captured from commit 9ace029.

use crate::model::Issue;
use sha2::{Digest, Sha256};

/// Normalize an RFC3339 timestamp string to UTC RFC3339Nano form, matching
/// Go `t.UTC().Format(time.RFC3339Nano)`:
/// - nanosecond precision, trailing zeros REMOVED (Go RFC3339Nano layout)
/// - "Z" suffix for UTC
pub fn normalize_rfc3339_nano(raw: &str) -> Option<String> {
    let ts = raw.parse::<jiff::Timestamp>().ok()?;
    // jiff formats with as many subsecond digits as needed (Go RFC3339Nano
    // trims trailing zeros too) — but jiff prints "Z" for UTC by default.
    Some(ts.to_string())
}

struct HashIssue<'a> {
    id: &'a str,
    title: &'a str,
    description: &'a str,
    notes: &'a str,
    design: &'a str,
    acceptance_criteria: &'a str,
    assignee: &'a str,
    source_repo: &'a str,
    external_ref: Option<&'a str>,
    status: &'a str,
    issue_type: &'a str,
    priority: i32,
    estimated_minutes: Option<i64>,
    created_at: Option<String>,
    updated_at: Option<String>,
    closed_at: Option<String>,
    labels: Vec<String>,
    deps: Vec<DepKey>,
    comments: Vec<CommentKey>,
}

#[derive(PartialEq)]
struct DepKey {
    depends_on: String,
    dep_type: String,
    created_at: String,
    created_by: String,
}

#[derive(PartialEq)]
struct CommentKey {
    id: String,
    author: String,
    text: String,
    created_at: String,
}

impl<'a> HashIssue<'a> {
    fn from_issue(issue: &'a Issue) -> Option<Self> {
        let mut labels = issue.labels.clone();
        labels.sort();

        let mut deps: Vec<DepKey> = issue
            .dependencies
            .iter()
            .map(|d| DepKey {
                depends_on: d.effective_depends_on().to_string(),
                dep_type: match d.r#type {
                    crate::model::DependencyType::Blocks => "blocks".into(),
                    crate::model::DependencyType::Related => "related".into(),
                    crate::model::DependencyType::ParentChild => "parent-child".into(),
                    crate::model::DependencyType::DiscoveredFrom => "discovered-from".into(),
                },
                created_at: d
                    .created_at
                    .as_deref()
                    .and_then(normalize_rfc3339_nano)
                    .unwrap_or_default(),
                created_by: d.created_by.clone(),
            })
            .collect();
        deps.sort_by(|a, b| {
            a.depends_on
                .cmp(&b.depends_on)
                .then(a.dep_type.cmp(&b.dep_type))
                .then(a.created_at.cmp(&b.created_at))
                .then(a.created_by.cmp(&b.created_by))
        });

        let mut comments: Vec<CommentKey> = issue
            .comments
            .iter()
            .map(|c| CommentKey {
                id: c.id.clone(),
                author: c.author.clone(),
                text: c.text.clone(),
                created_at: c
                    .created_at
                    .as_deref()
                    .and_then(normalize_rfc3339_nano)
                    .unwrap_or_default(),
            })
            .collect();
        comments.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then(a.created_at.cmp(&b.created_at))
                .then(a.author.cmp(&b.author))
                .then(a.text.cmp(&b.text))
        });

        Some(HashIssue {
            id: &issue.id,
            title: &issue.title,
            description: &issue.description,
            notes: &issue.notes,
            design: &issue.design,
            acceptance_criteria: &issue.acceptance_criteria,
            assignee: &issue.assignee,
            source_repo: &issue.source_repo,
            external_ref: issue.external_ref.as_deref(),
            status: issue.status.as_str(),
            issue_type: &issue.issue_type,
            priority: issue.priority,
            estimated_minutes: issue.estimated_minutes,
            created_at: issue.created_at.as_deref().and_then(normalize_rfc3339_nano),
            updated_at: issue.updated_at.as_deref().and_then(normalize_rfc3339_nano),
            closed_at: issue.closed_at.as_deref().and_then(normalize_rfc3339_nano),
            labels,
            deps,
            comments,
        })
    }
}

fn write_field(h: &mut Sha256, field: &[u8]) {
    h.update(field);
    h.update([0u8]);
}

/// Compute the 16-hex-char data hash. Empty input -> "empty".
pub fn compute_data_hash(issues: &[Issue]) -> String {
    if issues.is_empty() {
        return "empty".to_string();
    }

    let mut sorted: Vec<&Issue> = issues.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    let mut h = Sha256::new();
    for issue in sorted {
        let hi = match HashIssue::from_issue(issue) {
            Some(x) => x,
            None => continue,
        };
        write_field(&mut h, hi.id.as_bytes());
        write_field(&mut h, hi.title.as_bytes());
        write_field(&mut h, hi.description.as_bytes());
        write_field(&mut h, hi.notes.as_bytes());
        write_field(&mut h, hi.design.as_bytes());
        write_field(&mut h, hi.acceptance_criteria.as_bytes());
        write_field(&mut h, hi.assignee.as_bytes());
        write_field(&mut h, hi.source_repo.as_bytes());
        if let Some(ext) = hi.external_ref {
            h.update(ext.as_bytes());
        }
        h.update([0u8]);

        write_field(&mut h, hi.status.as_bytes());
        write_field(&mut h, hi.issue_type.as_bytes());

        write_field(&mut h, hi.priority.to_string().as_bytes());
        if let Some(est) = hi.estimated_minutes {
            h.update(est.to_string().as_bytes());
        }
        h.update([0u8]);
        if let Some(c) = &hi.created_at {
            h.update(c.as_bytes());
        }
        h.update([0u8]);
        if let Some(u) = &hi.updated_at {
            h.update(u.as_bytes());
        }
        h.update([0u8]);
        if let Some(cl) = &hi.closed_at {
            h.update(cl.as_bytes());
        }
        h.update([0u8]);

        for lbl in &hi.labels {
            h.update(lbl.as_bytes());
            h.update([0u8]);
        }
        h.update([0u8]); // end-of-labels separator

        for d in &hi.deps {
            write_field(&mut h, d.depends_on.as_bytes());
            write_field(&mut h, d.dep_type.as_bytes());
            write_field(&mut h, d.created_at.as_bytes());
            write_field(&mut h, d.created_by.as_bytes());
        }
        h.update([0u8]); // end-of-deps separator

        for c in &hi.comments {
            write_field(&mut h, c.id.as_bytes());
            write_field(&mut h, c.author.as_bytes());
            write_field(&mut h, c.text.as_bytes());
            write_field(&mut h, c.created_at.as_bytes());
        }
        // NOTE: Go has NO comments-end separator (verified byte-stream vs
        // instrumented upstream). Comments flow straight into {1}.

        h.update([1u8]); // issue separator
    }

    let digest = h.finalize();
    hex_encode_16(&digest)
}

fn hex_encode_16(digest: &[u8]) -> String {
    let mut out = String::with_capacity(16);
    for b in digest.iter().take(8) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Dependency, DependencyType, Issue, Status};

    fn issue(id: &str) -> Issue {
        Issue {
            id: id.into(),
            content_hash: String::new(),
            title: format!("Title {}", id),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status: Status::Open,
            priority: 2,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: Some("2026-01-01T00:00:00Z".into()),
            updated_at: Some("2026-01-01T01:00:00Z".into()),
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
    fn empty_input_yields_empty_sentinel() {
        assert_eq!(compute_data_hash(&[]), "empty");
    }

    #[test]
    fn order_independent() {
        let a = issue("A");
        let b = issue("B");
        let h1 = compute_data_hash(&[a.clone(), b.clone()]);
        let h2 = compute_data_hash(&[b, a]);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn content_change_changes_hash() {
        let mut a = issue("A");
        let h1 = compute_data_hash(std::slice::from_ref(&a));
        a.title = "Changed".into();
        let h2 = compute_data_hash(std::slice::from_ref(&a));
        assert_ne!(h1, h2);
    }

    #[test]
    fn dependency_affects_hash() {
        let mut a = issue("A");
        let h1 = compute_data_hash(std::slice::from_ref(&a));
        a.dependencies.push(Dependency {
            issue_id: "A".into(),
            depends_on_id: "B".into(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: DependencyType::Blocks,
            created_at: Some("2026-01-01T00:00:00Z".into()),
            created_by: "test".into(),
        });
        let h2 = compute_data_hash(std::slice::from_ref(&a));
        assert_ne!(h1, h2);
    }
}
