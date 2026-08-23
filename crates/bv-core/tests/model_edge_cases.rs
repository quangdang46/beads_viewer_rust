//! Model edge case tests ported from Go pkg/model tests.

use bv_core::model::{Comment, Dependency, DependencyType, Issue, Status};

// === Go: DependencyType_IsBlocking — empty string blocks ===
#[test]
fn empty_dep_type_is_blocking() {
    let dep = Dependency {
        issue_id: "A".into(),
        depends_on_id: "B".into(),
        depends_on_legacy: String::new(),
        target_id_legacy: String::new(),
        r#type: DependencyType::parse(""),
        created_at: None,
        created_by: String::new(),
    };
    assert!(dep.r#type.is_blocking());
}

#[test]
fn blocks_type_is_blocking() {
    let dt = DependencyType::parse("blocks");
    assert!(dt.is_blocking());
}

#[test]
fn related_type_not_blocking() {
    let dt = DependencyType::parse("related");
    assert!(!dt.is_blocking());
}

#[test]
fn parent_child_type_not_blocking() {
    let dt = DependencyType::parse("parent-child");
    assert!(!dt.is_blocking());
}

// === Go: Status_IsValid / IsClosed / IsOpen ===
#[test]
fn all_statuses_valid() {
    for s in [
        "open",
        "in_progress",
        "blocked",
        "deferred",
        "draft",
        "pinned",
        "hooked",
        "review",
        "closed",
        "tombstone",
    ] {
        assert!(Status::parse(s).is_some(), "status {s} should be valid");
    }
}

#[test]
fn invalid_status_returns_none() {
    assert!(Status::parse("banana").is_none());
    assert!(Status::parse("").is_none());
}

#[test]
fn is_open_only_for_open_and_in_progress() {
    assert!(Status::Open.is_open());
    assert!(Status::InProgress.is_open());
    assert!(!Status::Blocked.is_open());
    assert!(!Status::Closed.is_open());
}

// === Go: Issue_Validate ===
fn make_issue() -> Issue {
    Issue {
        id: "TEST-1".into(),
        content_hash: String::new(),
        title: "Test issue".into(),
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
        updated_at: Some("2026-01-02T00:00:00Z".into()),
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
fn validate_passes_for_valid_issue() {
    assert!(make_issue().validate().is_ok());
}

#[test]
fn validate_fails_empty_id() {
    let mut i = make_issue();
    i.id = String::new();
    assert!(i.validate().is_err());
}

#[test]
fn validate_fails_empty_title() {
    let mut i = make_issue();
    i.title = String::new();
    assert!(i.validate().is_err());
}

// === Go: Comment_UnmarshalJSON — number and null IDs ===
#[test]
fn comment_id_null_becomes_empty_string() {
    let c: Comment = serde_json::from_value(serde_json::json!({"id": null, "text": "x"})).unwrap();
    assert_eq!(c.id, "");
}

#[test]
fn comment_missing_id_becomes_empty_string() {
    let c: Comment = serde_json::from_value(serde_json::json!({"text": "no id"})).unwrap();
    assert_eq!(c.id, "");
}

// === Go: omitempty serialization parity ===
#[test]
fn zero_priority_still_serialized() {
    // Go: priority has no omitempty → always present
    let mut i = make_issue();
    i.priority = 0;
    let json = serde_json::to_value(&i).unwrap();
    assert!(json.get("priority").is_some());
    assert_eq!(json["priority"], 0);
}

#[test]
fn empty_labels_omitted() {
    let i = make_issue();
    let json = serde_json::to_value(&i).unwrap();
    assert!(json.get("labels").is_none()); // omitempty
}

#[test]
fn compaction_level_zero_omitted() {
    let i = make_issue();
    let json = serde_json::to_value(&i).unwrap();
    assert!(json.get("compaction_level").is_none()); // omitempty + is_zero
}
