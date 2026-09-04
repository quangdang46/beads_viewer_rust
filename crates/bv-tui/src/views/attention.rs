//! Attention view — labels ranked by attention score
//! (Go `pkg/ui/attention.go`, 226 lines).
//!
//! Backing data comes from `bv_analysis::label_health::LabelAttentionResult`
//! (computed in `robot-label-attention`). This module is purely rendering.

use bv_analysis::label_health::LabelAttentionScore;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Render the attention view: ranked list of labels by attention score
/// with visual bars and key details.
pub fn render_attention(f: &mut Frame, labels: &[LabelAttentionScore], cursor: usize, area: Rect) {
    if labels.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "No label attention data available",
            Style::default().fg(Color::DarkGray),
        )))
        .block(Block::default().borders(Borders::ALL).title(" ATTENTION "));
        f.render_widget(msg, area);
        return;
    }

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage(60),
            ratatui::layout::Constraint::Percentage(40),
        ])
        .split(area);

    // Left panel: ranked list.
    let max_score = labels
        .iter()
        .map(|l| l.attention_score)
        .fold(0.0f64, f64::max)
        .max(1.0);

    let mut items: Vec<Line> = Vec::new();
    for (i, lbl) in labels.iter().enumerate() {
        let bar_width = ((lbl.attention_score / max_score) * 20.0) as usize;
        let bar: String = "\u{2588}".repeat(bar_width);
        let is_selected = i == cursor;
        let style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if lbl.attention_score > max_score * 0.5 {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        let rank_style = Style::default().fg(Color::DarkGray);
        items.push(Line::from(vec![
            Span::styled(format!("{:>2}. ", lbl.rank), rank_style),
            Span::styled(format!("{:<20}", lbl.label), style),
            Span::styled(bar, Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(" {:.2}", lbl.attention_score),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let list = Paragraph::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" ATTENTION ({}) ", labels.len())),
    );
    f.render_widget(list, chunks[0]);

    // Right panel: details for selected label.
    let sel = cursor.min(labels.len().saturating_sub(1));
    let lbl = &labels[sel];
    let mut detail_lines: Vec<Line> = Vec::new();
    detail_lines.push(Line::from(Span::styled(
        format!("Label: {}", lbl.label),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    detail_lines.push(Line::from(""));

    detail_lines.push(Line::from(format!("Rank: {}/{}", lbl.rank, labels.len())));
    detail_lines.push(Line::from(format!("Score: {:.4}", lbl.attention_score)));
    detail_lines.push(Line::from(format!(
        "Normalized: {:.2}",
        lbl.normalized_score
    )));
    detail_lines.push(Line::from(""));

    detail_lines.push(Line::from(Span::styled(
        "Components:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    detail_lines.push(Line::from(format!(
        "  PageRank sum: {:.4}",
        lbl.pagerank_sum
    )));
    detail_lines.push(Line::from(format!(
        "  Staleness:    {:.2}",
        lbl.staleness_factor
    )));
    detail_lines.push(Line::from(format!(
        "  Block impact: {:.0}",
        lbl.block_impact
    )));
    detail_lines.push(Line::from(format!(
        "  Velocity:     {:.2}",
        lbl.velocity_factor
    )));
    detail_lines.push(Line::from(""));

    detail_lines.push(Line::from(format!("Open:     {}", lbl.open_count)));
    detail_lines.push(Line::from(format!("Blocked:  {}", lbl.blocked_count)));
    detail_lines.push(Line::from(format!("Stale:    {}", lbl.stale_count)));

    let detail = Paragraph::new(detail_lines)
        .block(Block::default().borders(Borders::ALL).title(" DETAIL "))
        .wrap(Wrap { trim: false });
    f.render_widget(detail, chunks[1]);
}
