//! SQLite reader — port of Go `internal/datasource/sqlite.go` load path,
//! including its deliberate quirks:
//! - SELECT column list omits source_repo/created_by → those stay empty
//! - dependencies: only depends_on_id + type (created_at/by stay zero)
//! - ORDER BY updated_at DESC when the column exists
//! - labels from JSON column or separate labels-table fallback

use crate::model::{Comment, Dependency, DependencyType, Issue};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum SqliteError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = match conn.prepare(&format!("PRAGMA table_info({table})")) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map([], |r| r.get::<_, String>(1)) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.filter_map(|r| r.ok()).collect()
}

fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

/// Parse SQLite time strings: RFC3339(Nano), space-separated forms.
/// Returns normalized RFC3339Nano-style Z string for hashing.
pub fn parse_sqlite_time(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(t) = raw.parse::<jiff::Timestamp>() {
        return Some(t.to_string());
    }
    // "2006-01-02 15:04:05[-07:00]" and no-offset variants: interpret as UTC
    // then re-render, matching Go's Parse->UTC pipeline closely enough for
    // hash parity on br-produced databases (which use RFC3339 +00:00).
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(dt) = jiff::civil::DateTime::strptime(fmt, raw) {
            if let Ok(ts) = dt.to_zoned(jiff::tz::TimeZone::UTC) {
                return Some(ts.to_string());
            }
        }
    }
    None
}

/// Load issues from a beads SQLite database, mirroring the Go query shape.
pub fn load_issues_sqlite(db_path: &Path) -> Result<Vec<Issue>, SqliteError> {
    let conn = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let columns = table_columns(&conn, "issues");
    let has = |c: &str| columns.iter().any(|x| x == c);
    let expr = |c: &str, fallback: &str| {
        if has(c) {
            c.to_string()
        } else {
            fallback.to_string()
        }
    };
    let coalesce = |c: &str, fallback: &str| {
        if has(c) {
            format!("COALESCE({c}, {fallback})")
        } else {
            fallback.to_string()
        }
    };

    let where_clause = if has("tombstone") {
        "WHERE (tombstone IS NULL OR tombstone = 0)".to_string()
    } else {
        String::new()
    };
    let order_by = if has("updated_at") {
        "ORDER BY updated_at DESC"
    } else {
        ""
    };

    let query = format!(
        "SELECT id, title, {}, status, {}, {}, {}, {}, {}, {} FROM issues {} {}",
        expr("description", "NULL"),
        coalesce("priority", "3"),
        coalesce("issue_type", "'task'"),
        expr("assignee", "NULL"),
        expr("created_at", "NULL"),
        expr("updated_at", "NULL"),
        expr("labels", "NULL"),
        where_clause,
        order_by,
    );

    // Separate labels table (beads-rs schema)?
    let separate_labels = has_table(&conn, "labels");

    let mut stmt = conn.prepare(&query)?;
    let mut rows = stmt.query([])?;

    // Load ALL dependencies in one pass (Go does per-issue queries; result set
    // identical because we group by issue_id).
    let mut deps_by_issue: BTreeMap<String, Vec<Dependency>> = BTreeMap::new();
    {
        let dep_type_expr = dependency_type_expr(&conn);
        let dep_query =
            format!("SELECT issue_id, depends_on_id, {dep_type_expr} FROM dependencies");
        if let Ok(mut dstmt) = conn.prepare(&dep_query) {
            if let Ok(mut rows) = dstmt.query([]) {
                loop {
                    let row = match rows.next()? {
                        Some(r) => r,
                        None => break,
                    };
                    let on: String = row.get(1)?;
                    let ty: Option<String> = row.get(2).ok();
                    let issue_id: String = row.get(0)?;
                    deps_by_issue
                        .entry(issue_id.clone())
                        .or_default()
                        .push(Dependency {
                            issue_id: issue_id.clone(),
                            depends_on_id: on,
                            depends_on_legacy: String::new(),
                            target_id_legacy: String::new(),
                            r#type: DependencyType::parse(ty.as_deref().unwrap_or("")),
                            // Go quirk: created_at/created_by NOT selected — zero values.
                            created_at: None,
                            created_by: String::new(),
                        });
                }
            }
        }
    }

    let mut comments_by_issue: BTreeMap<String, Vec<Comment>> = BTreeMap::new();
    if has_table(&conn, "comments") {
        let cquery =
            "SELECT issue_id, id, author, text, created_at FROM comments ORDER BY created_at";
        if let Ok(mut cstmt) = conn.prepare(cquery) {
            if let Ok(mut rows) = cstmt.query([]) {
                loop {
                    let row = match rows.next()? {
                        Some(r) => r,
                        None => break,
                    };
                    let issue_id: String = row.get(0)?;
                    comments_by_issue
                        .entry(issue_id)
                        .or_default()
                        .push(Comment {
                            id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                            issue_id: String::new(),
                            author: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                            text: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                            created_at: row.get::<_, Option<String>>(4).ok().flatten(),
                        });
                }
            }
        }
    }

    let mut issues = Vec::new();
    loop {
        let row = match rows.next()? {
            Some(r) => r,
            None => break,
        };
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let description: Option<String> = row.get(2)?;
        let status_raw: String = row.get(3)?;
        let priority: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(3);
        let issue_type: String = row
            .get::<_, Option<String>>(5)?
            .unwrap_or_else(|| "task".to_string());
        let assignee: Option<String> = row.get(6)?;
        let created_at: Option<String> = row
            .get::<_, Option<String>>(7)?
            .and_then(|s| parse_sqlite_time(&s));
        let updated_at: Option<String> = row
            .get::<_, Option<String>>(8)?
            .and_then(|s| parse_sqlite_time(&s));
        let labels_json: Option<String> = row.get(9)?;

        let mut labels = labels_json
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "null")
            .map(parse_json_string_array)
            .unwrap_or_default();

        if separate_labels && labels.is_empty() {
            labels = load_labels_from_table(&conn, &id);
        }

        let status = crate::model::Status::parse(&status_raw).unwrap_or(crate::model::Status::Open);

        issues.push(Issue {
            id: id.clone(),
            content_hash: String::new(),
            title,
            description: description.unwrap_or_default(),
            design: String::new(), // not selected (Go parity)
            acceptance_criteria: String::new(),
            notes: String::new(),
            status,
            priority: priority as i32,
            issue_type,
            assignee: assignee.unwrap_or_default(),
            estimated_minutes: None, // not selected (Go parity)
            created_at,
            updated_at,
            due_date: None,
            closed_at: None, // not selected (Go parity)
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels,
            dependencies: deps_by_issue.remove(&id).unwrap_or_default(),
            comments: comments_by_issue.remove(&id).unwrap_or_default(),
            source_repo: String::new(), // not selected (Go parity)
        });
    }
    Ok(issues)
}

fn dependency_type_expr(conn: &Connection) -> &'static str {
    let cols = table_columns(conn, "dependencies");
    if cols.iter().any(|c| c == "type") {
        "type"
    } else {
        "'blocks'"
    }
}

fn load_labels_from_table(conn: &Connection, issue_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT label FROM labels WHERE issue_id = ?1") {
        if let Ok(mut rows) = stmt.query([issue_id]) {
            while let Ok(Some(row)) = rows.next() {
                if let Ok(l) = row.get::<_, String>(0) {
                    out.push(l);
                }
            }
        }
    }
    out
}

fn parse_json_string_array(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}
