//! Shortcuts sidebar — shows keyboard shortcuts for the current view.
//! Port of Go `pkg/ui/shortcuts_sidebar.go`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::keybindings::{build_default_registry, Focus};

/// Render the shortcuts sidebar for the current focus.
pub fn render_shortcuts_sidebar(f: &mut Frame, focus: Focus, area: Rect) {
    let reg = build_default_registry();
    let bindings = reg.bindings_for(focus);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        " Shortcuts ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(
        "─".repeat(area.width.saturating_sub(2) as usize),
    ));

    let mut current_cat = String::new();
    for binding in bindings {
        if binding.category != current_cat {
            current_cat = binding.category.clone();
            lines.push(Line::from(Span::styled(
                format!(" {current_cat}:"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<12}", binding.key),
                Style::default().fg(Color::Green),
            ),
            Span::raw(binding.desc.clone()),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}
