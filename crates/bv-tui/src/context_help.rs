//! Context-sensitive help overlay — shows relevant shortcuts for the current view.
//! Port of Go `pkg/ui/context_help.go`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::keybindings::{build_default_registry, Focus};

/// Render the context-sensitive help overlay for the current focus.
pub fn render_context_help(f: &mut Frame, focus: Focus, area: Rect) {
    let reg = build_default_registry();
    let bindings = reg.bindings_for(focus);

    let popup_width = 50.min(area.width.saturating_sub(4));
    let popup_height = (bindings.len() as u16 + 6).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup = Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(" Help — {:?}", focus),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

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
        lines.push(Line::from(format!(
            "  {:<14} {}",
            binding.key, binding.desc
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Press any key to close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}
