//! robot-diff — git snapshot comparison.
//! Compares current issue state against a previous git ref.

use bv_core::model::Issue;

/// Compare current issues against issues at a git ref.
pub fn diff_issues(current: &[Issue], previous: &[Issue], diff_ref: &str) -> DiffResult {
    let prev_map: std::collections::HashMap<String, &Issue> =
        previous.iter().map(|i| (i.id.clone(), i)).collect();
    let curr_map: std::collections::HashMap<String, &Issue> =
        current.iter().map(|i| (i.id.clone(), i)).collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for issue in current {
        if let Some(prev) = prev_map.get(&issue.id) {
            if issue.status != prev.status
                || issue.title != prev.title
                || issue.priority != prev.priority
            {
                changed.push(ChangeDetail {
                    id: issue.id.clone(),
                    status_change: if issue.status != prev.status {
                        Some(format!(
                            "{} → {}",
                            prev.status.as_str(),
                            issue.status.as_str()
                        ))
                    } else {
                        None
                    },
                    title_changed: issue.title != prev.title,
                    priority_changed: issue.priority != prev.priority,
                });
            }
        } else {
            added.push(issue.id.clone());
        }
    }

    for issue in previous {
        if !curr_map.contains_key(&issue.id) {
            removed.push(issue.id.clone());
        }
    }

    DiffResult {
        diff_ref: diff_ref.to_string(),
        added_count: added.len(),
        removed_count: removed.len(),
        changed_count: changed.len(),
        added,
        removed,
        changed,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffResult {
    pub diff_ref: String,
    pub added_count: usize,
    pub removed_count: usize,
    pub changed_count: usize,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<ChangeDetail>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangeDetail {
    pub id: String,
    pub status_change: Option<String>,
    pub title_changed: bool,
    pub priority_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bv_core::model::Status;

    #[test]
    fn diff_empty() {
        let result = diff_issues(&[], &[], "HEAD~1");
        assert_eq!(result.added_count, 0);
        assert_eq!(result.removed_count, 0);
    }

    #[test]
    fn diff_finds_added_and_removed() {
        let a = make_issue("A", Status::Open);
        let b = make_issue("B", Status::Open);
        let result = diff_issues(std::slice::from_ref(&a), std::slice::from_ref(&b), "HEAD~1");
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.removed.len(), 1);
    }

    fn make_issue(id: &str, status: Status) -> Issue {
        Issue {
            id: id.into(),
            content_hash: String::new(),
            title: format!("Issue {id}"),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status,
            priority: 2,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: Some("2026-01-01T00:00:00Z".into()),
            updated_at: Some("2026-01-01T00:00:00Z".into()),
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
}
