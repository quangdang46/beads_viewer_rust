//! Actionable items view — shows issues that need immediate attention.
//! Port of Go `pkg/ui/actionable.go`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use bv_core::model::{Issue, Status};

/// An actionable item with reason.
#[derive(Debug, Clone)]
pub struct ActionableItem {
    pub issue: Issue,
    pub reason: String,
    pub urgency: Urgency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

impl Urgency {
    pub fn color(&self) -> Color {
        match self {
            Urgency::Critical => Color::Red,
            Urgency::High => Color::Yellow,
            Urgency::Medium => Color::Cyan,
            Urgency::Low => Color::DarkGray,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Urgency::Critical => "CRIT",
            Urgency::High => "HIGH",
            Urgency::Medium => "MED",
            Urgency::Low => "LOW",
        }
    }
}

/// Build actionable items from issues (Go `computeActionable`).
pub fn compute_actionable(issues: &[Issue]) -> Vec<ActionableItem> {
    let mut items: Vec<ActionableItem> = Vec::new();

    for issue in issues {
        if issue.status.is_closed() {
            continue;
        }

        // Blocked issues are critical
        if issue.status == Status::Blocked {
            items.push(ActionableItem {
                issue: issue.clone(),
                reason: "Blocked — resolve blockers first".into(),
                urgency: Urgency::Critical,
            });
            continue;
        }

        // High priority open issues
        if issue.priority <= 1 && issue.status == Status::Open {
            items.push(ActionableItem {
                issue: issue.clone(),
                reason: format!("High priority (P{}) open", issue.priority),
                urgency: Urgency::High,
            });
            continue;
        }

        // Issues with many dependents
        let dependent_count = issues
            .iter()
            .filter(|i| {
                i.dependencies.iter().any(|d| {
                    d.depends_on_id == issue.id
                        && d.r#type == bv_core::model::DependencyType::Blocks
                })
            })
            .count();
        if dependent_count >= 3 {
            items.push(ActionableItem {
                issue: issue.clone(),
                reason: format!("Blocked by {dependent_count} issues"),
                urgency: Urgency::Medium,
            });
            continue;
        }

        // In-progress issues
        if issue.status == Status::InProgress {
            items.push(ActionableItem {
                issue: issue.clone(),
                reason: "Currently in progress".into(),
                urgency: Urgency::Low,
            });
        }
    }

    items.sort_by(|a, b| match (&a.urgency, &b.urgency) {
        (Urgency::Critical, Urgency::Critical) => std::cmp::Ordering::Equal,
        (Urgency::Critical, _) => std::cmp::Ordering::Less,
        (_, Urgency::Critical) => std::cmp::Ordering::Greater,
        (Urgency::High, Urgency::High) => std::cmp::Ordering::Equal,
        (Urgency::High, _) => std::cmp::Ordering::Less,
        (_, Urgency::High) => std::cmp::Ordering::Greater,
        (Urgency::Medium, Urgency::Medium) => std::cmp::Ordering::Equal,
        (Urgency::Medium, _) => std::cmp::Ordering::Less,
        (_, Urgency::Medium) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });

    items
}

/// Render the actionable items view.
pub fn render_actionable(f: &mut Frame, items: &[ActionableItem], selected: usize, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(" Actionable Items ({}) ", items.len()),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(
        "─".repeat(area.width.saturating_sub(2) as usize),
    ));

    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No actionable items — all clear!",
            Style::default().fg(Color::Green),
        )));
    } else {
        for (i, item) in items.iter().enumerate() {
            let is_selected = i == selected;
            let style = if is_selected {
                Style::default()
                    .fg(item.urgency.color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(item.urgency.color())
            };

            let indicator = if is_selected { "▶ " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!(
                    "{}[{}] {} - {}",
                    indicator,
                    item.urgency.label(),
                    item.issue.id,
                    truncate_str(&item.issue.title, 30)
                ),
                style,
            )));

            if is_selected {
                lines.push(Line::from(Span::styled(
                    format!("    {}", item.reason),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " j/k: navigate | Esc: close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));

    let block = Block::default().borders(Borders::ALL).title(" Actionable ");

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn truncate_str(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        s.to_string()
    } else if max_len <= 1 {
        "…".to_string()
    } else {
        let truncated: String = chars[..max_len - 1].iter().collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(id: &str, status: Status, priority: i32) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: format!("Issue {id}"),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status,
            priority,
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

    #[test]
    fn compute_actionable_finds_blocked() {
        let issues = vec![make_issue("A", Status::Blocked, 2)];
        let items = compute_actionable(&issues);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].urgency, Urgency::Critical);
    }

    #[test]
    fn compute_actionable_skips_closed() {
        let issues = vec![make_issue("A", Status::Closed, 0)];
        let items = compute_actionable(&issues);
        assert!(items.is_empty());
    }
}
