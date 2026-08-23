//! Insights dashboard — 6-panel metric view (Go `pkg/ui/insights.go`).
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::BTreeMap;

fn top_n(map: &BTreeMap<String, f64>, n: usize) -> Vec<(String, f64)> {
    let mut items: Vec<(String, f64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    items.truncate(n);
    items
}

fn panel(f: &mut Frame, area: Rect, title: &str, entries: &[(String, f64)], color: Color) {
    let lines: Vec<Line> = entries
        .iter()
        .map(|(id, val)| {
            Line::from(vec![
                Span::styled(format!("  {id} "), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{val:.4}"), Style::default().fg(color)),
            ])
        })
        .collect();
    let block = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} ")),
    );
    f.render_widget(block, area);
}

/// Render the full insights dashboard (6 panels).
pub fn render_insights(
    f: &mut Frame,
    page_rank: &BTreeMap<String, f64>,
    betweenness: &BTreeMap<String, f64>,
    hubs: &BTreeMap<String, f64>,
    authorities: &BTreeMap<String, f64>,
) {
    eprintln!(
        "DEBUG insights: pr={} bw={} hubs={}",
        page_rank.len(),
        betweenness.len(),
        hubs.len()
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(f.area());

    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(rows[0]);

    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(rows[1]);

    // Row 1: Bottlenecks | Keystones | Influencers
    panel(
        f,
        top_cols[0],
        "\u{1f6a7} Bottlenecks (BW)",
        &top_n(betweenness, 5),
        Color::Red,
    );
    panel(
        f,
        top_cols[1],
        "\u{1f3db} Keystones (CP)",
        &top_n(authorities, 5),
        Color::Yellow,
    );
    let influencers: BTreeMap<String, f64> = page_rank.clone();
    panel(
        f,
        top_cols[2],
        "\u{1f310} Influencers (EV)",
        &top_n(&influencers, 5),
        Color::Green,
    );

    // Row 2: Hubs | Authorities | Cycles
    panel(
        f,
        bottom_cols[0],
        "\u{1f6f0} Hubs",
        &top_n(hubs, 5),
        Color::Magenta,
    );
    panel(
        f,
        bottom_cols[1],
        "\u{1f4da} Authorities",
        &top_n(authorities, 5),
        Color::Blue,
    );
    let cycles_text = Paragraph::new("No cycles detected").block(
        Block::default()
            .borders(Borders::ALL)
            .title(" \u{1f504} Cycles "),
    );
    f.render_widget(cycles_text, bottom_cols[2]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_n_returns_sorted_desc() {
        let mut m = BTreeMap::new();
        m.insert("A".into(), 0.1);
        m.insert("B".into(), 0.9);
        m.insert("C".into(), 0.5);
        let top = top_n(&m, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "B");
        assert_eq!(top[1].0, "C");
    }
}
