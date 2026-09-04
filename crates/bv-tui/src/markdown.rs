//! Markdown rendering for TUI detail pane.
//! Port of Go `pkg/ui/markdown.go` — simplified terminal markdown renderer.
//! Converts markdown text to ratatui Lines with basic styling.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render markdown text into ratatui Lines for terminal display.
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim_end();

        // Headings
        if let Some(rest) = trimmed.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                format!("  {rest}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                format!(" {rest}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            lines.push(Line::from(Span::styled(
                "─".repeat(40),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Code blocks (``` fences)
        if trimmed.starts_with("```") {
            lines.push(Line::from(Span::styled(
                "  ┌─ code ─┐",
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            lines.push(Line::from(Span::styled(
                format!("  │ {rest}"),
                Style::default().fg(Color::Cyan),
            )));
            continue;
        }

        // Bullet list
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let content = &trimmed[2..];
            lines.push(Line::from(vec![Span::raw("  • "), render_inline(content)]));
            continue;
        }

        // Numbered list
        if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
            if let Some(rest) = rest.strip_prefix(". ") {
                lines.push(Line::from(vec![Span::raw("  "), render_inline(rest)]));
                continue;
            }
        }

        // Regular paragraph
        if !trimmed.is_empty() {
            lines.push(Line::from(render_inline(trimmed)));
        } else {
            lines.push(Line::from(""));
        }
    }
    lines
}

/// Render inline markdown (bold, italic, code, links).
fn render_inline(text: &str) -> Span<'static> {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    // Bold: **text**
                    if let Some(end) = text[1..].find("**") {
                        result.push_str(&text[1..=end]);
                        // Skip past the closing **
                        for _ in 0..end {
                            chars.next();
                        }
                    } else {
                        result.push('*');
                        result.push('*');
                    }
                } else if chars.peek() == Some(&'*') {
                    chars.next();
                    result.push_str("**");
                } else {
                    result.push('*');
                }
            }
            '`' => {
                // Inline code
                if let Some(end) = text[1..].find('`') {
                    result.push_str(&text[1..end]);
                    for _ in 0..end {
                        chars.next();
                    }
                } else {
                    result.push('`');
                }
            }
            _ => result.push(c),
        }
    }

    // Apply basic styling based on content
    if result.starts_with("**") && result.ends_with("**") {
        Span::styled(
            result.trim_matches('*').to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )
    } else if result.contains('`') {
        Span::styled(
            result.trim_matches('`').to_string(),
            Style::default().fg(Color::Green),
        )
    } else {
        Span::raw(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_heading() {
        let lines = render_markdown("# Title");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn render_bullet() {
        let lines = render_markdown("- item one\n- item two");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_empty() {
        let lines = render_markdown("");
        assert!(lines.is_empty());
    }
}
