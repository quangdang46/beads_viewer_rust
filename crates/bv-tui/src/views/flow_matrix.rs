//! Flow-Matrix view — cross-label dependency flow dashboard
//! (Go `pkg/ui/flow_matrix.go`, 905 lines).
//!
//! Backing data comes from `bv_analysis::label_health::CrossLabelFlow`
//! (computed in `robot-label-flow`). This module is purely rendering.

use bv_analysis::label_health::CrossLabelFlow;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Render the flow-matrix view: label list on the left with outgoing-dep
/// bar charts, detail panel on the right for the selected label.
pub fn render_flow_matrix(f: &mut Frame, flow: &CrossLabelFlow, cursor: usize, area: Rect) {
    if flow.labels.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "No cross-label dependencies found",
            Style::default().fg(Color::DarkGray),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" FLOW MATRIX "),
        );
        f.render_widget(msg, area);
        return;
    }

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage(55),
            ratatui::layout::Constraint::Percentage(45),
        ])
        .split(area);

    // Left panel: labels with outgoing dependency counts as bars.
    let max_out = flow
        .flow_matrix
        .iter()
        .map(|row| row.iter().sum::<i64>())
        .max()
        .unwrap_or(1)
        .max(1);

    let mut items: Vec<Line> = Vec::new();
    for (i, label) in flow.labels.iter().enumerate() {
        let out: i64 = flow
            .flow_matrix
            .get(i)
            .map(|row| row.iter().sum())
            .unwrap_or(0);
        let bar_width = ((out as f64 / max_out as f64) * 20.0) as usize;
        let bar: String = "\u{2588}".repeat(bar_width);
        let is_selected = i == cursor;
        let style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(Line::from(vec![
            Span::styled(format!("{label:<20}"), style),
            Span::styled(bar, Style::default().fg(Color::Cyan)),
            Span::styled(format!(" {out}"), Style::default().fg(Color::DarkGray)),
        ]));
    }

    let list = Paragraph::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" LABELS ({}) ", flow.labels.len())),
    );
    f.render_widget(list, chunks[0]);

    // Right panel: details for the selected label.
    let sel = cursor.min(flow.labels.len().saturating_sub(1));
    let sel_label = &flow.labels[sel];
    let mut detail_lines: Vec<Line> = Vec::new();
    detail_lines.push(Line::from(Span::styled(
        format!("Label: {sel_label}"),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    detail_lines.push(Line::from(""));

    // Bottleneck status.
    let is_bottleneck = flow.bottleneck_labels.contains(sel_label);
    detail_lines.push(Line::from(vec![
        Span::raw("Bottleneck: "),
        if is_bottleneck {
            Span::styled(
                "YES",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("no", Style::default().fg(Color::DarkGray))
        },
    ]));

    // Top incoming.
    let mut incoming: Vec<(String, i64)> = Vec::new();
    for (j, other) in flow.labels.iter().enumerate() {
        if j == sel {
            continue;
        }
        let count = flow
            .flow_matrix
            .get(j)
            .and_then(|row| row.get(sel).copied())
            .unwrap_or(0);
        if count > 0 {
            incoming.push((other.clone(), count));
        }
    }
    incoming.sort_by_key(|item| std::cmp::Reverse(item.1));
    if !incoming.is_empty() {
        detail_lines.push(Line::from(Span::styled(
            "Blocked by:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for (lbl, cnt) in incoming.iter().take(5) {
            detail_lines.push(Line::from(format!("  {lbl} ({cnt})")));
        }
    }

    // Top outgoing.
    let mut outgoing: Vec<(String, i64)> = Vec::new();
    for (j, other) in flow.labels.iter().enumerate() {
        if j == sel {
            continue;
        }
        let count = flow
            .flow_matrix
            .get(sel)
            .and_then(|row| row.get(j).copied())
            .unwrap_or(0);
        if count > 0 {
            outgoing.push((other.clone(), count));
        }
    }
    outgoing.sort_by_key(|item| std::cmp::Reverse(item.1));
    if !outgoing.is_empty() {
        detail_lines.push(Line::from(Span::styled(
            "Blocks:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for (lbl, cnt) in outgoing.iter().take(5) {
            detail_lines.push(Line::from(format!("  {lbl} ({cnt})")));
        }
    }

    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled(
        format!("Total cross-label deps: {}", flow.total_cross_label_deps),
        Style::default().fg(Color::DarkGray),
    )));

    let detail = Paragraph::new(detail_lines)
        .block(Block::default().borders(Borders::ALL).title(" DETAIL "))
        .wrap(Wrap { trim: false });
    f.render_widget(detail, chunks[1]);
}
