//! Alerts panel — port of Go `pkg/ui` alerts rendering.
use bv_analysis::drift::{Alert, Severity};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub fn render_alerts(f: &mut Frame, alerts: &[Alert], cursor: usize, area: Rect) {
    let items: Vec<ListItem> = alerts
        .iter()
        .map(|a| {
            let (icon, color) = match a.severity {
                Severity::Critical => ("\u{1f534}", Color::Red),
                Severity::Warning => ("\u{26a0}\u{fe0f}", Color::Yellow),
                Severity::Info => ("\u{2139}\u{fe0f}", Color::Blue),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    &a.message,
                    Style::default().add_modifier(Modifier::BOLD).fg(color),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" \u{1f6a8} ALERTS "),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));
    let mut state = ListState::default();
    state.select(Some(cursor.min(alerts.len().saturating_sub(1))));
    f.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    #[test]
    fn alerts_module_compiles() {
        // Rendering requires a Frame; compile-check only here.
    }
}
