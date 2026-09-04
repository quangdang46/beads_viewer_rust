//! Sprint JSONL loader — reads `.beads/sprints.jsonl` (Go
//! `pkg/loader/sprint.go:LoadSprints`).

use crate::model::{BurndownPoint, Forecast, Sprint};
use std::path::Path;

const SPRINTS_FILE: &str = "sprints.jsonl";

/// Load all sprints from `<repo>/.beads/sprints.jsonl`.
/// Returns an empty vec if the file doesn't exist (Go returns nil, not error).
pub fn load_sprints(repo_path: &Path) -> Result<Vec<Sprint>, String> {
    let beads_dir = repo_path.join(".beads");
    let sprints_path = beads_dir.join(SPRINTS_FILE);
    if !sprints_path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&sprints_path)
        .map_err(|e| format!("reading {}: {e}", sprints_path.display()))?;
    let mut sprints = Vec::new();
    for (line_num, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Sprint>(line) {
            Ok(sprint) => sprints.push(sprint),
            Err(e) => {
                eprintln!(
                    "Warning: skipping malformed sprint at line {}: {e}",
                    line_num + 1
                );
            }
        }
    }
    Ok(sprints)
}

/// Seconds between two timestamps, as f64.
fn secs_between(a: jiff::Timestamp, b: jiff::Timestamp) -> f64 {
    let diff = a - b;
    diff.total(jiff::Unit::Second).unwrap_or(0.0)
}

/// Calculate a simple burndown for a sprint: remaining open/bead-ids-not-closed
/// count per day from start_date to now (or end_date).
/// Returns (points, total_issues). Go's `calculateBurndownAt` is substantially
/// more complex (tracks scope changes per day); this is a documented simplified
/// version.
pub fn calculate_burndown(
    sprint: &Sprint,
    issues: &[crate::model::Issue],
    now: jiff::Timestamp,
) -> (Vec<BurndownPoint>, usize) {
    let Some(start) = sprint
        .start_date
        .as_deref()
        .and_then(|s| s.parse::<jiff::Timestamp>().ok())
    else {
        return (Vec::new(), 0);
    };
    let end = sprint
        .end_date
        .as_deref()
        .and_then(|s| s.parse::<jiff::Timestamp>().ok())
        .unwrap_or(now);

    let sprint_issues: Vec<&crate::model::Issue> = issues
        .iter()
        .filter(|i| sprint.bead_ids.iter().any(|bid| bid == &i.id))
        .collect();
    let total = sprint_issues.len();
    if total == 0 {
        return (Vec::new(), 0);
    }

    let mut points = Vec::new();
    let mut day = start;
    let cutoff = if end < now { end } else { now };
    while day <= cutoff {
        let remaining = sprint_issues
            .iter()
            .filter(|i| !i.status.is_closed())
            .count() as i64;
        points.push(BurndownPoint {
            date: day.to_string(),
            remaining,
            ideal: None,
        });
        day += jiff::SignedDuration::from_secs(86400);
    }

    // Ideal line: linear from total to 0 over the sprint duration.
    let sprint_days = (secs_between(end, start) / 86400.0).max(1.0);
    for pt in &mut points {
        let day_ts = pt.date.parse::<jiff::Timestamp>().unwrap_or(start);
        let day_offset = secs_between(day_ts, start) / 86400.0;
        pt.ideal = Some(total as f64 * (1.0 - day_offset / sprint_days));
    }

    (points, total)
}

/// Simple forecast: estimate ETA based on sprint velocity target and
/// remaining open issues. Falls back to "30 days from now" if no
/// velocity_target is set. Go's `robot-forecast` is substantially more
/// complex (factors in graph metrics, historical velocity); this is a
/// documented simplified version.
pub fn estimate_forecast(
    sprint: &Sprint,
    issues: &[crate::model::Issue],
    now: jiff::Timestamp,
) -> Option<Forecast> {
    let sprint_issues: Vec<&crate::model::Issue> = issues
        .iter()
        .filter(|i| sprint.bead_ids.iter().any(|bid| bid == &i.id))
        .collect();
    let remaining = sprint_issues
        .iter()
        .filter(|i| !i.status.is_closed())
        .count();
    if remaining == 0 {
        return None;
    }

    let velocity = sprint.velocity_target.unwrap_or(1.0);
    let days_needed = (remaining as f64 / velocity).ceil() as i64;
    let eta = now + jiff::SignedDuration::from_secs(days_needed * 86400);

    let mut factors = vec![format!("{remaining} open issues remaining")];
    if let Some(target) = sprint.velocity_target {
        factors.push(format!("velocity target: {target:.1} issues/day"));
    }

    Some(Forecast {
        bead_id: sprint.id.clone(),
        eta_date: eta.to_string(),
        confidence: 0.5,
        factors,
        created_at: Some(now.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Issue, Status};

    fn issue(id: &str, status: Status) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: id.to_string(),
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

    fn sprint(id: &str, bead_ids: &[&str], velocity: Option<f64>) -> Sprint {
        Sprint {
            id: id.to_string(),
            name: format!("Sprint {id}"),
            start_date: Some("2026-01-01T00:00:00Z".into()),
            end_date: Some("2026-01-14T00:00:00Z".into()),
            bead_ids: bead_ids.iter().map(|s| s.to_string()).collect(),
            velocity_target: velocity,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn load_sprints_returns_empty_when_file_missing() {
        let dir = std::env::temp_dir().join(format!("bvr-sprint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let result = load_sprints(&dir).unwrap();
        assert!(result.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn calculate_burndown_counts_remaining_per_day() {
        let s = sprint("s1", &["A", "B", "C"], Some(1.0));
        let issues = vec![
            issue("A", Status::Open),
            issue("B", Status::Closed),
            issue("C", Status::Open),
            issue("D", Status::Open), // not in sprint
        ];
        let now = "2026-01-03T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
        let (points, total) = calculate_burndown(&s, &issues, now);
        assert_eq!(total, 3); // only A, B, C are in the sprint
        assert!(points.len() >= 2);
        // Day 0 should have 2 remaining (A and C open)
        assert_eq!(points[0].remaining, 2);
    }

    #[test]
    fn estimate_forecast_returns_none_when_all_closed() {
        let s = sprint("s1", &["A"], Some(1.0));
        let issues = vec![issue("A", Status::Closed)];
        let now = "2026-01-05T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
        assert!(estimate_forecast(&s, &issues, now).is_none());
    }

    #[test]
    fn estimate_forecast_uses_velocity_target() {
        let s = sprint("s1", &["A", "B", "C"], Some(2.0));
        let issues = vec![
            issue("A", Status::Open),
            issue("B", Status::Open),
            issue("C", Status::Closed),
        ];
        let now = "2026-01-05T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
        let f = estimate_forecast(&s, &issues, now).unwrap();
        assert_eq!(f.bead_id, "s1");
        assert!(f.confidence > 0.0);
        assert!(f.factors.iter().any(|f| f.contains("velocity")));
    }
}
