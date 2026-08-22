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

/// Render the board view with swimlanes.
pub fn render_board(f: &mut Frame, app: &App, area: Rect, mode: SwimlaneMode) {
    let columns = match mode {
        SwimlaneMode::Status => vec![
            ("OPEN", Status::Open),
            ("IN PROGRESS", Status::InProgress),
            ("BLOCKED", Status::Blocked),
            ("CLOSED", Status::Closed),
        ],
        _ => vec![
            ("ALL", Status::Open), // priority/type modes use different grouping
        ],
    };

    let n = columns.len() as u16;
    let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, (label, status)) in columns.iter().enumerate() {
        let items: Vec<&crate::ListRow> = app
            .rows
            .iter()
            .filter(|r| match mode {
                SwimlaneMode::Status => r.status == *status,
                _ => true,
            })
            .collect();

        let color = match status {
            Status::Open => Color::Green,
            Status::InProgress => Color::Yellow,
            Status::Blocked => Color::Red,
            _ => Color::Gray,
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

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {label} ({}) ", items.len()))
            .border_style(Style::default().fg(color));
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
