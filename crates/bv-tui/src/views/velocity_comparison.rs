//! Velocity comparison view — compares velocity over time.
//!
//! # STATUS: BLOCKED ON `bv-analysis`. The view below is NOT the Go view.
//!
//! Go's `pkg/ui/velocity_comparison.go` is a **per-LABEL** table:
//! `label | W-4 | W-3 | W-2 | W-1 | Avg | Trend | Spark`, rows sorted by
//! 4-week moving average descending then label ascending, with a normalized
//! Unicode sparkline, a trend word plus symbol, and a cursor that scrolls
//! (`ensureVisible` keeps it inside `height-4` rows; a data refresh that
//! shrinks or empties the label set resets both cursor and scroll offset).
//!
//! The view in this file today is **per-SPRINT** (`Sprint | Planned | Done |
//! Velocity%`) fed from `App::velocity_points` (lib.rs). It shares no
//! column, no row and no data source with the Go view.
//!
//! The upstream computation does not exist in Rust: `bv-analysis` has no
//! `label_health` module (`grep -r HistoricalVelocity crates/` → 0 hits).
//! Porting the view without it would render zeros.
//!
//! ## What must be added to `crates/bv-analysis/src/label_health.rs` first
//!
//! ```text
//! pub struct WeeklySnapshot {          // Go label_health.go:63-70
//!     pub week_start:  Timestamp,      //   start of the week (Monday)
//!     pub week_end:    Timestamp,      //   end of the week (Sunday)
//!     pub closed:      usize,          //   issues closed this week
//!     pub weeks_ago:   usize,          //   0 = current week
//!     pub issue_ids:   Vec<String>,
//!     pub cumulative:  usize,
//! }
//!
//! pub struct HistoricalVelocity {      // Go label_health.go:48-60
//!     pub label:             String,
//!     pub weekly_velocity:   Vec<WeeklySnapshot>,  // newest -> oldest
//!     pub weeks_analyzed:    usize,
//!     pub moving_avg_4_week: f64,
//!     pub moving_avg_8_week: f64,
//!     pub peak_week:         usize,
//!     pub peak_velocity:     usize,
//!     pub trough_week:       usize,
//!     pub trough_velocity:   usize,
//!     pub variance:          f64,
//!     pub consistency_score: usize,     // 0-100
//! }
//!
//! // Go label_health.go:2109. num_weeks <= 0 defaults to 8.
//! pub fn compute_historical_velocity(
//!     issues: &[Issue], label: &str, num_weeks: usize, now: Timestamp,
//! ) -> HistoricalVelocity;
//!
//! // Go label_health.go:2255 — iterates ExtractLabels(issues).
//! pub fn compute_all_historical_velocity(
//!     issues: &[Issue], num_weeks: usize, now: Timestamp,
//! ) -> HashMap<String, HistoricalVelocity>;
//!
//! // Go label_health.go:2268-2303. Thresholds are exact:
//! //   n < 4                    -> "insufficient_data"
//! //   older_sum == 0, recent >0 -> "accelerating"
//! //   older_sum == 0, recent==0 -> "stable"
//! //   ratio = recent/older; > 1.3 -> "accelerating", < 0.7 -> "decelerating"
//! //   variance > peak*0.5      -> "erratic"
//! //   otherwise                -> "stable"
//! // `half_point = n / 2`; recentSum = WeeklyVelocity[0..half],
//! // olderSum = WeeklyVelocity[half..n]  (slice is newest-first).
//! pub fn velocity_trend(hv: &HistoricalVelocity) -> &'static str;
//! ```
//!
//! Once those land, the port is: a `VelocityComparisonModel` with
//! `data: Vec<VelocityRow>`, `cursor: usize`, `scroll_offset: usize` and
//! `set_data` / `move_up` / `move_down` / `ensure_visible` /
//! `selected_label` / `data_count` (Go `velocity_comparison.go:42-110`,
//! `:284-296`), `build_sparkline` verbatim from `:113-131`, and `View()`
//! from `:184-335`.
//!
//! Priority note: Go itself never calls `SetData`/`View`/`MoveUp`/`MoveDown`
//! on this model — it is constructed at `model.go:1736`, stored at
//! `:770`/`:1923`, and otherwise dead. Confirm with the owner before
//! spending effort on the port.

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
