//! Velocity comparison view — compares sprint velocity over time.
//! Port of Go `pkg/ui/velocity_comparison.go`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// A single velocity data point.
#[derive(Debug, Clone)]
pub struct VelocityPoint {
    pub sprint_name: String,
    pub completed: usize,
    pub planned: usize,
    pub velocity: f64,
}

/// Render the velocity comparison view.
pub fn render_velocity_comparison(f: &mut Frame, points: &[VelocityPoint], area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        " Sprint Velocity Comparison ",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if points.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No velocity data available",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Header
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<20} {:>8} {:>8} {:>8}",
                "Sprint", "Planned", "Done", "Velocity"
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from("─".repeat(42)));

        // Data rows
        for point in points {
            let velocity_color = if point.velocity >= 0.8 {
                Color::Green
            } else if point.velocity >= 0.5 {
                Color::Yellow
            } else {
                Color::Red
            };

            lines.push(Line::from(vec![
                Span::raw(format!("  {:<20}", point.sprint_name)),
                Span::styled(
                    format!("{:>8}", point.planned),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:>8}", point.completed),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("{:>7.0}%", point.velocity * 100.0),
                    Style::default().fg(velocity_color),
                ),
            ]));

            // Mini bar
            let bar_width: usize = 30;
            let filled = (point.velocity * bar_width as f64).round() as usize;
            let empty = bar_width.saturating_sub(filled);
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("█".repeat(filled), Style::default().fg(Color::Green)),
                Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    // Summary
    if !points.is_empty() {
        let avg_velocity: f64 =
            points.iter().map(|p| p.velocity).sum::<f64>() / points.len() as f64;
        let total_planned: usize = points.iter().map(|p| p.planned).sum();
        let total_done: usize = points.iter().map(|p| p.completed).sum();

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Summary:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "  Average velocity: {:.0}%",
            avg_velocity * 100.0
        )));
        lines.push(Line::from(format!(
            "  Total planned: {} | Total done: {}",
            total_planned, total_done
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " j/k: navigate | Esc: close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Velocity Comparison ");

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}
