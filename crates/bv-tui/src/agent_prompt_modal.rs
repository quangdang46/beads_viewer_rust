//! Agent blurb prompt modal — port of Go `pkg/ui/agent_prompt_modal.go`.
//!
//! Asks whether to inject the bv blurb into the detected AGENTS.md/CLAUDE.md.
//! State lives here; key handling lives in `App::handle_agent_prompt_key`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// The three options, matching Go's option order.
pub const AGENT_PROMPT_OPTIONS: [&str; 3] = ["Yes, add it", "No thanks", "Don't ask again"];

/// Pending blurb prompt state (Go `AgentPromptModal`).
#[derive(Debug, Clone)]
pub struct AgentPromptModal {
    pub file_path: String,
    pub file_type: String,
    pub needs_upgrade: bool,
    pub selected: usize,
    /// Project dir the detection ran in — prefs are keyed per project, so the
    /// modal carries it instead of re-reading the ambient cwd on each key.
    pub project_dir: String,
}

impl AgentPromptModal {
    pub fn move_left(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.selected + 1 < AGENT_PROMPT_OPTIONS.len() {
            self.selected += 1;
        }
    }
}

/// Render the blurb prompt modal.
pub fn render_agent_prompt(f: &mut Frame, modal: &AgentPromptModal, area: Rect) {
    let popup_width = 62.min(area.width.saturating_sub(4));
    let popup_height = 11.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup = Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup);

    let action = if modal.needs_upgrade { "update" } else { "add" };
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        " Agent Instructions ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        " {} found — {} bv instructions?",
        modal.file_type, action
    )));
    lines.push(Line::from(Span::styled(
        modal.file_path.clone(),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    let mut option_spans: Vec<Span> = Vec::new();
    for (i, opt) in AGENT_PROMPT_OPTIONS.iter().enumerate() {
        let style = if i == modal.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let marker = if i == modal.selected { "▶" } else { " " };
        option_spans.push(Span::styled(format!("{marker}{opt}  "), style));
    }
    lines.push(Line::from(option_spans));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ←/→: select | y/n/d: quick | Enter: confirm | Esc: skip",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Agent Prompt ")
        .border_style(Style::default().fg(Color::Cyan));

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}
