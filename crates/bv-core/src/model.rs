//! Frozen data model for beads issues (api-freeze-v1).
//!
//! Ported 1:1 from upstream `pkg/model/types.go` @ 9ace029.
//! Field names, JSON tags, and semantics are part of the compatibility
//! contract — changes here require a plan edit + CHANGELOG "API change"
//! entry (additive-only during parity).

use serde::{Deserialize, Serialize};

/// Issue status. Exactly the 10 values Go recognizes (`model.Status.IsValid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "blocked")]
    Blocked,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "pinned")]
    Pinned,
    #[serde(rename = "hooked")]
    Hooked,
    #[serde(rename = "review")]
    Review,
    #[serde(rename = "closed")]
    Closed,
    #[serde(rename = "tombstone")]
    Tombstone,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::InProgress => "in_progress",
            Status::Blocked => "blocked",
            Status::Deferred => "deferred",
            Status::Draft => "draft",
            Status::Pinned => "pinned",
            Status::Hooked => "hooked",
            Status::Review => "review",
            Status::Closed => "closed",
            Status::Tombstone => "tombstone",
        }
    }

    /// Go: `IsClosed()` — only exactly `closed`.
    pub fn is_closed(self) -> bool {
        self == Status::Closed
    }

    /// Go: `IsOpen()` — open OR in_progress.
    pub fn is_open(self) -> bool {
        matches!(self, Status::Open | Status::InProgress)
    }

    /// Go: `IsTombstone()`.
    pub fn is_tombstone(self) -> bool {
        self == Status::Tombstone
    }
}

/// Dependency type. Legacy compat: an EMPTY type string is blocking,
/// same as `blocks` (Go: `DependencyType.IsBlocking`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DependencyType {
    #[default]
    Blocks,
    Related,
    ParentChild,
    DiscoveredFrom,
}

impl DependencyType {
    pub fn as_str(self) -> &'static str {
        match self {
            DependencyType::Blocks => "blocks",
            DependencyType::Related => "related",
            DependencyType::ParentChild => "parent-child",
            DependencyType::DiscoveredFrom => "discovered-from",
        }
    }

    /// Go: `IsBlocking` — only `blocks` blocks among recognized values.
    /// (The legacy EMPTY string maps to `Blocks` in `parse`, so it blocks too.)
    pub fn is_blocking(self) -> bool {
        self == DependencyType::Blocks
    }

    /// Parse from raw JSONL string; "" maps to Blocks (legacy default).
    pub fn parse(raw: &str) -> Self {
        match raw {
            "" | "blocks" => DependencyType::Blocks,
            "related" => DependencyType::Related,
            "parent-child" => DependencyType::ParentChild,
            "discovered-from" => DependencyType::DiscoveredFrom,
            // Unknown types fall back to Blocks to preserve Go's IsBlocking()
            // behavior on unrecognized strings? No — Go IsValid() rejects them
            // at validation; loader keeps them but they don't block. We model
            // unknowns as Related (non-blocking) and record validity separately.
            _ => DependencyType::Related,
        }
    }

    /// True when the raw string is one of the four recognized values or empty.
    pub fn raw_is_valid(raw: &str) -> bool {
        matches!(
            raw,
            "" | "blocks" | "related" | "parent-child" | "discovered-from"
        )
    }
}

impl Serialize for DependencyType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DependencyType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(DependencyType::parse(&s))
    }
}

/// A relationship between issues.
///
/// Deser accepts `depends_on_id` (canonical), plus legacy `depends_on`
/// and `target_id` fallbacks — mirrors Go `Dependency.UnmarshalJSON`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default)]
    pub issue_id: String,
    #[serde(default, rename = "depends_on_id")]
    pub depends_on_id: String,
    /// Legacy field name; folded into `depends_on_id` when canonical absent.
    #[serde(default, rename = "depends_on")]
    pub depends_on_legacy: String,
    /// Legacy field name; folded into `depends_on_id` when others absent.
    #[serde(default, rename = "target_id")]
    pub target_id_legacy: String,
    #[serde(default)]
    pub r#type: DependencyType,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub created_by: String,
}

impl Dependency {
    /// Effective depends-on after legacy-field folding (Go UnmarshalJSON).
    pub fn effective_depends_on(&self) -> &str {
        if !self.depends_on_id.is_empty() {
            &self.depends_on_id
        } else if !self.depends_on_legacy.is_empty() {
            &self.depends_on_legacy
        } else {
            &self.target_id_legacy
        }
    }
}

/// A comment on an issue. `id` tolerates JSON numbers (legacy integer IDs)
/// by stringifying the raw literal verbatim (#145 semantics).
#[derive(Debug, Clone, Serialize)]
pub struct Comment {
    pub id: String,
    #[serde(rename = "issueId", alias = "issue_id")]
    pub issue_id: String,
    pub author: String,
    pub text: String,
    pub created_at: Option<String>,
}

impl<'de> Deserialize<'de> for Comment {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default, rename = "id")]
            id: Option<serde_json::Value>,
            #[serde(default, alias = "issueId")]
            issue_id: String,
            #[serde(default)]
            author: String,
            #[serde(default)]
            text: String,
            #[serde(default)]
            created_at: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let id = match raw.id {
            None | Some(serde_json::Value::Null) => String::new(),
            Some(serde_json::Value::String(s)) => s,
            Some(other) => other.to_string().trim().to_string(),
        };
        Ok(Comment {
            id,
            issue_id: raw.issue_id,
            author: raw.author,
            text: raw.text,
            created_at: raw.created_at,
        })
    }
}

/// A trackable work item — 25 fields matching Go `model.Issue` exactly.
///
/// Timestamps stay `Option<String>` at this layer so we can preserve the
/// exact RFC3339(Nano) textual form for data_hash byte-parity; typed
/// parsing happens in the loader with jiff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    #[serde(skip)]
    pub content_hash: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub design: String,
    #[serde(
        default,
        rename = "acceptance_criteria",
        skip_serializing_if = "String::is_empty"
    )]
    pub acceptance_criteria: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub status: Status,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, rename = "issue_type")]
    pub issue_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub assignee: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_minutes: Option<i64>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub compaction_level: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_at_commit: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub original_size: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Dependency>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_repo: String,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

impl Issue {
    /// Go: `Issue.Validate()` — ID+Title non-empty; timestamps must not be
    /// inverted. Returns human-readable error strings mirroring Go messages.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id.is_empty() {
            return Err(ValidationError::MissingField("id"));
        }
        if self.title.is_empty() {
            return Err(ValidationError::MissingField("title"));
        }
        // updated_at < created_at rejection (loader feeds these to load_stats)
        if let (Some(c), Some(u)) = (&self.created_at, &self.updated_at) {
            if let (Ok(c), Ok(u)) = (c.parse::<jiff::Timestamp>(), u.parse::<jiff::Timestamp>()) {
                if u < c {
                    return Err(ValidationError::InvertedTimestamps);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid status: {0}")]
    InvalidStatus(String),
    #[error("invalid dependency type")]
    InvalidDependencyType,
    #[error("updated_at cannot be before created_at")]
    InvertedTimestamps,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrips_all_ten_values() {
        for raw in [
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
            let s: Status = serde_json::from_value(serde_json::json!(raw)).unwrap();
            assert_eq!(s.as_str(), raw);
        }
    }

    #[test]
    fn dependency_folds_legacy_field_names() {
        // canonical
        let d: Dependency =
            serde_json::from_value(serde_json::json!({"issue_id":"A","depends_on_id":"B"}))
                .unwrap();
        assert_eq!(d.effective_depends_on(), "B");
        // legacy depends_on
        let d: Dependency =
            serde_json::from_value(serde_json::json!({"issue_id":"A","depends_on":"C"})).unwrap();
        assert_eq!(d.effective_depends_on(), "C");
        // legacy target_id
        let d: Dependency =
            serde_json::from_value(serde_json::json!({"issue_id":"A","target_id":"D"})).unwrap();
        assert_eq!(d.effective_depends_on(), "D");
        // canonical wins over legacy
        let d: Dependency = serde_json::from_value(
            serde_json::json!({"issue_id":"A","depends_on_id":"B","target_id":"D"}),
        )
        .unwrap();
        assert_eq!(d.effective_depends_on(), "B");
    }

    #[test]
    fn comment_id_tolerates_numeric_legacy_ids() {
        let c: Comment =
            serde_json::from_value(serde_json::json!({"id": 42, "text": "hi"})).unwrap();
        assert_eq!(c.id, "42");
        let c: Comment =
            serde_json::from_value(serde_json::json!({"id": "uuid-abc", "text": "hi"})).unwrap();
        assert_eq!(c.id, "uuid-abc");
        let c: Comment = serde_json::from_value(serde_json::json!({"text": "no id"})).unwrap();
        assert_eq!(c.id, "");
    }

    #[test]
    fn issue_omits_empty_optionals_like_go() {
        let issue = Issue {
            id: "X-1".into(),
            content_hash: String::new(),
            title: "Test".into(),
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
        };
        let json = serde_json::to_value(&issue).unwrap();
        let obj = json.as_object().unwrap();
        // omitempty parity
        assert!(!obj.contains_key("design"));
        assert!(!obj.contains_key("labels"));
        assert!(!obj.contains_key("dependencies"));
        assert!(!obj.contains_key("compaction_level")); // zero omitted
        assert!(obj.contains_key("priority")); // no omitempty in Go
        assert_eq!(obj.get("acceptance_criteria"), None);
    }

    #[test]
    fn validate_rejects_inverted_timestamps() {
        let mut issue = minimal_issue();
        issue.created_at = Some("2026-01-05T00:00:00Z".into());
        issue.updated_at = Some("2026-01-01T00:00:00Z".into());
        assert_eq!(issue.validate(), Err(ValidationError::InvertedTimestamps));
    }

    fn minimal_issue() -> Issue {
        Issue {
            id: "X-1".into(),
            content_hash: String::new(),
            title: "T".into(),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status: Status::Open,
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
}
