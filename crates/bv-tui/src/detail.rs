//! Detail pane — renders full issue information matching Go bv's viewport.

use bv_core::model::Issue;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub struct GraphScores {
    pub pagerank: f64,
    pub betweenness: f64,
    pub eigenvector: f64,
    pub hubs: f64,
    pub authorities: f64,
    pub critical_path: f64,
}

fn type_icon_md(t: &str) -> &'static str {
    match t {
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

fn status_icon(s: &str) -> &'static str {
    match s {
        "open" => "🟢",
        "in_progress" => "🔵",
        "blocked" => "🔴",
        "closed" => "⚫",
        _ => "⚪",
    }
}

fn dep_type_icon(t: &str) -> &'static str {
    match t {
        "blocks" => "⛔",
        "related" => "🔗",
        "parent-child" => "📦",
        "discovered-from" => "🔍",
        _ => "•",
    }
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

fn indent(text: &str) -> Line<'static> {
    Line::from(Span::raw(format!("  {text}")))
}

/// Build full detail lines for an issue (matching Go updateViewportContent).
pub fn build_detail_lines(
    issue: &Issue,
    graph_scores: Option<&GraphScores>,
    all_issues: Option<&std::collections::HashMap<String, Issue>>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    // Title with type icon
    let icon = type_icon_md(&issue.issue_type);
    lines.push(Line::from(Span::styled(
        format!("{icon} {}", issue.title),
        bold,
    )));
    lines.push(Line::from(""));

    // Meta
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

    if !issue.labels.is_empty() {
        lines.push(line_kv("Labels", &issue.labels.join(", ")));
    }
    lines.push(Line::from(""));

    // Graph Analysis
    if let Some(gs) = graph_scores {
        lines.push(section_line("📊 Graph Analysis"));
        lines.push(indent(&format!(
            "Impact Depth: {:.0} (downstream chain length)",
            gs.critical_path
        )));
        lines.push(indent(&format!(
            "Centrality: PR {:.4} \u{2022} BW {:.4} \u{2022} EV {:.4}",
            gs.pagerank, gs.betweenness, gs.eigenvector
        )));
        lines.push(indent(&format!(
            "Flow Role: Hub {:.4} \u{2022} Authority {:.4}",
            gs.hubs, gs.authorities
        )));
        lines.push(Line::from(""));
    }

    // Dependencies with icons
    if !issue.dependencies.is_empty() {
        lines.push(section_line("🔗 Dependencies"));
        for dep in &issue.dependencies {
            let target_id = dep.effective_depends_on().to_string();
            let dt = dep_type_icon(dep.r#type.as_str());
            let arrow = if dep.r#type.is_blocking() {
                "→ blocked by"
            } else {
                "→ related to"
            };

            // Look up target for status icon + title
            if let Some(all) = all_issues {
                if let Some(target) = all.get(&target_id) {
                    let _s = status_icon(target.status.as_str());
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::raw(dt),
                        Span::raw(" "),
                        Span::styled(arrow.to_string(), dim),
                        Span::raw(" "),
                        Span::styled(target_id.clone(), Style::default().fg(Color::Cyan)),
                        Span::raw(" "),
                        Span::styled(target.title.clone(), Style::default().fg(Color::White)),
                        Span::styled(format!(" ({})", target.status.as_str()), dim),
                    ]));
                    continue;
                }
            }
            lines.push(indent(&format!("{arrow} {target_id}")));
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
            lines.push(indent(&format!("{author}: {}", c.text)));
        }
        lines.push(Line::from(""));
    }

    lines
}
