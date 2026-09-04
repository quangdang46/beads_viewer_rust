//! Update modal — notification when a new version is available.
//! Port of Go `pkg/ui/update_modal.go`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Render the update notification modal.
pub fn render_update_modal(f: &mut Frame, current_version: &str, latest_version: &str, area: Rect) {
    let popup_width = 50.min(area.width.saturating_sub(4));
    let popup_height = 12.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup = Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup);

    let lines = vec![
        Line::from(Span::styled(
            " Update Available ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Current: ", Style::default().fg(Color::DarkGray)),
            Span::raw(current_version.to_string()),
        ]),
        Line::from(vec![
            Span::styled("  Latest:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                latest_version.to_string(),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Run `bvr --check-update` to install",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            "  Press any key to dismiss",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Update ")
        .border_style(Style::default().fg(Color::Yellow));

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}
