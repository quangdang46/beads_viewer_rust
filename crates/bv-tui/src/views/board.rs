//! Kanban board view — port of Go `pkg/ui/board.go` swimlane modes.

use crate::App;
use bv_core::model::Status;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Swimlane grouping mode (cycled with `s`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwimlaneMode {
    Status,
    Priority,
    Type,
}

impl SwimlaneMode {
    pub fn next(self) -> Self {
        match self {
            SwimlaneMode::Status => SwimlaneMode::Priority,
            SwimlaneMode::Priority => SwimlaneMode::Type,
            SwimlaneMode::Type => SwimlaneMode::Status,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SwimlaneMode::Status => "Status",
            SwimlaneMode::Priority => "Priority",
            SwimlaneMode::Type => "Type",
        }
    }
}

/// Column labels for a swimlane mode, data-driven where the mode needs it.
/// `Status` is the fixed 4-column Kanban; `Priority` is fixed P0–P4;
/// `Type` is the sorted distinct `issue_type` values present in the rows
/// (single `ALL` fallback when no row declares a type).
pub fn column_labels(app: &App, mode: SwimlaneMode) -> Vec<String> {
    match mode {
        SwimlaneMode::Status => ["OPEN", "IN PROGRESS", "BLOCKED", "CLOSED"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        SwimlaneMode::Priority => (0..=4).map(|p| format!("P{p}")).collect(),
        SwimlaneMode::Type => {
            let mut types: Vec<String> = app
                .rows
                .iter()
                .map(|r| r.issue_type.clone())
                .filter(|t| !t.is_empty())
                .collect();
            types.sort();
            types.dedup();
            if types.is_empty() {
                vec!["ALL".to_string()]
            } else {
                types
            }
        }
    }
}

/// Render the board view with swimlanes. `selected` highlights the active
/// column (moved with `h`/`l`, jumped with `1`-`9`).
pub fn render_board(f: &mut Frame, app: &App, area: Rect, mode: SwimlaneMode, selected: usize) {
    let columns = column_labels(app, mode);
    let status_for = |label: &str| match label {
        "OPEN" => Status::Open,
        "IN PROGRESS" => Status::InProgress,
        "BLOCKED" => Status::Blocked,
        _ => Status::Closed,
    };

    let n = columns.len().max(1) as u16;
    let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, label) in columns.iter().enumerate() {
        let items: Vec<&crate::ListRow> = app
            .rows
            .iter()
            .filter(|r| match mode {
                SwimlaneMode::Status => r.status == status_for(label),
                SwimlaneMode::Priority => {
                    label.strip_prefix('P').and_then(|d| d.parse::<i32>().ok()) == Some(r.priority)
                }
                SwimlaneMode::Type => label == "ALL" || r.issue_type == *label,
            })
            .collect();

        let color = match mode {
            SwimlaneMode::Status => match status_for(label) {
                Status::Open => Color::Green,
                Status::InProgress => Color::Yellow,
                Status::Blocked => Color::Red,
                _ => Color::Gray,
            },
            SwimlaneMode::Priority => match label.as_str() {
                "P0" | "P1" => Color::Red,
                "P2" => Color::Yellow,
                "P3" => Color::Cyan,
                _ => Color::Gray,
            },
            SwimlaneMode::Type => Color::Cyan,
        };

        let lines: Vec<Line> = items
            .iter()
            .map(|r| {
                Line::from(vec![
                    Span::styled(format!("P{}", r.priority), Style::default().fg(Color::Red)),
                    Span::raw(" "),
                    Span::styled(&r.title, Style::default().add_modifier(Modifier::BOLD)),
                ])
            })
            .collect();

        let title = if i == selected {
            format!("▶ {label} ({}) ", items.len())
        } else {
            format!(" {label} ({}) ", items.len())
        };
        let border = if i == selected {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border);
        f.render_widget(Paragraph::new(lines).block(block), chunks[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swimlane_mode_cycles_three() {
        assert_eq!(SwimlaneMode::Status.next(), SwimlaneMode::Priority);
        assert_eq!(SwimlaneMode::Priority.next(), SwimlaneMode::Type);
        assert_eq!(SwimlaneMode::Type.next(), SwimlaneMode::Status);
    }

    #[test]
    fn labels_are_readable() {
        assert_eq!(SwimlaneMode::Status.label(), "Status");
        assert_eq!(SwimlaneMode::Priority.label(), "Priority");
        assert_eq!(SwimlaneMode::Type.label(), "Type");
    }
}
