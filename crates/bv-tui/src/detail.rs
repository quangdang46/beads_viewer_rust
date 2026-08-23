//! Detail pane — port of Go `updateViewportContent` (model.go:7415-7600).
//! Renders full issue information in plain text matching Go's markdown output.

use bv_core::model::Issue;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

fn type_icon_md(issue_type: &str) -> &'static str {
    match issue_type {
        "bug" => "🐛",
        "feature" => "✨",
        "task" => "📋",
        "epic" => "🚀",
        "chore" => "🧹",
        _ => "•",
    }
}

fn priority_icon(p: i32) -> &'static str {
    match p {
        0 => "🔴 P0",
        1 => "🟠 P1",
        2 => "🔵 P2",
        3 => "⚪ P3",
        _ => "⚫ P4",
    }
}

/// Build the full detail text lines for an issue.
pub fn build_detail_lines(issue: &Issue, graph_scores: Option<&GraphScores>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let _dim = Style::default().fg(Color::DarkGray);
    let _section = bold.fg(Color::Cyan);

    // Title with type icon
    let icon = type_icon_md(&issue.issue_type);
    lines.push(Line::from(Span::styled(
        format!("{icon} {title}", title = issue.title),
        bold,
    )));
    lines.push(Line::from(""));

    // Meta table
    let status_upper = issue.status.as_str().to_uppercase();
    let prio = priority_icon(issue.priority);
    let assignee = if issue.assignee.is_empty() {
        "-"
    } else {
        &issue.assignee
    };
    let created = issue
        .created_at
        .as_deref()
        .and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .map(|t| t.strftime("%Y-%m-%d").to_string())
        .unwrap_or_default();

    lines.push(line_kv("ID", &issue.id));
    lines.push(line_kv_colored("Status", &status_upper, Color::Green));
    lines.push(line_kv("Priority", prio));
    lines.push(line_kv("Assignee", assignee));
    lines.push(line_kv("Created", &created));

    // Labels
    if !issue.labels.is_empty() {
        lines.push(line_kv("Labels", &issue.labels.join(", ")));
    }

    lines.push(Line::from(""));

    // Graph Analysis section
    if let Some(gs) = graph_scores {
        lines.push(section_line("📊 Graph Analysis"));
        lines.push(indent_line(format!(
            "Impact Depth: {:.0} (downstream chain length)",
            gs.critical_path
        )));
        lines.push(indent_line(format!(
            "Centrality: PR {:.4} • BW {:.4} • EV {:.4}",
            gs.pagerank, gs.betweenness, gs.eigenvector
        )));
        lines.push(indent_line(format!(
            "Flow Role: Hub {:.4} • Authority {:.4}",
            gs.hubs, gs.authorities
        )));
        lines.push(Line::from(""));
    }

    // Dependencies
    if !issue.dependencies.is_empty() {
        lines.push(section_line("🔗 Dependencies"));
        for dep in &issue.dependencies {
            let arrow = if dep.r#type.is_blocking() {
                "→ blocked by"
            } else {
                "→ related to"
            };
            lines.push(indent_line(format!(
                "{arrow} {}",
                dep.effective_depends_on()
            )));
        }
        lines.push(Line::from(""));
    }

    // Description
    if !issue.description.is_empty() {
        lines.push(section_line("📝 Description"));
        for l in issue.description.lines() {
            lines.push(Line::from(Span::raw(format!("  {l}"))));
        }
        lines.push(Line::from(""));
    }

    // Design Notes
    if !issue.design.is_empty() {
        lines.push(section_line("🎨 Design Notes"));
        for l in issue.design.lines() {
            lines.push(Line::from(Span::raw(format!("  {l}"))));
        }
        lines.push(Line::from(""));
    }

    // Acceptance Criteria
    if !issue.acceptance_criteria.is_empty() {
        lines.push(section_line("✅ Acceptance Criteria"));
        for l in issue.acceptance_criteria.lines() {
            lines.push(Line::from(Span::raw(format!("  {l}"))));
        }
        lines.push(Line::from(""));
    }

    // Notes
    if !issue.notes.is_empty() {
        lines.push(section_line("📌 Notes"));
        for l in issue.notes.lines() {
            lines.push(Line::from(Span::raw(format!("  {l}"))));
        }
        lines.push(Line::from(""));
    }

    // Comments
    if !issue.comments.is_empty() {
        lines.push(section_line(&format!(
            "💬 Comments ({})",
            issue.comments.len()
        )));
        for c in &issue.comments {
            let author = if c.author.is_empty() {
                "unknown"
            } else {
                &c.author
            };
            lines.push(indent_line(format!("{author}: {}", c.text)));
        }
        lines.push(Line::from(""));
    }

    lines
}

/// Graph scores for the selected issue.
pub struct GraphScores {
    pub pagerank: f64,
    pub betweenness: f64,
    pub eigenvector: f64,
    pub hubs: f64,
    pub authorities: f64,
    pub critical_path: f64,
}

fn line_kv(key: &str, val: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key}: "), Style::default().fg(Color::DarkGray)),
        Span::raw(val.to_string()),
    ])
}

fn line_kv_colored(key: &str, val: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key}: "), Style::default().fg(Color::DarkGray)),
        Span::styled(
            val.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn section_line(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn indent_line(text: String) -> Line<'static> {
    Line::from(Span::raw(format!("  {text}")))
}

/// Wrapper for ListRow from lib.rs
pub fn build_detail_lines_from_row(
    row: &crate::ListRow,
    graph_scores: &Option<GraphScores>,
) -> Vec<Line<'static>> {
    // Convert ListRow fields into a pseudo-Issue for build_detail_lines
    let issue = Issue {
        id: row.id.clone(),
        content_hash: String::new(),
        title: row.title.clone(),
        description: row.description.clone(),
        design: String::new(),
        acceptance_criteria: String::new(),
        notes: row.notes.clone(),
        status: row.status,
        priority: row.priority,
        issue_type: row.issue_type.clone(),
        assignee: row.assignee.clone(),
        estimated_minutes: None,
        created_at: row.created_at.clone(),
        updated_at: None,
        due_date: None,
        closed_at: None,
        external_ref: None,
        compaction_level: 0,
        compacted_at: None,
        compacted_at_commit: None,
        original_size: 0,
        labels: row.labels.clone(),
        dependencies: vec![],
        comments: vec![],
        source_repo: String::new(),
    };
    build_detail_lines(&issue, graph_scores.as_ref())
}
