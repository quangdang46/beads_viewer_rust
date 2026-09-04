//! Agent prompt modal — shows agent-specific prompts and actions.
//! Port of Go `pkg/ui/agent_prompt_modal.go`.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// An agent prompt action.
#[derive(Debug, Clone)]
pub struct AgentPrompt {
    pub label: String,
    pub command: String,
    pub description: String,
}

/// Get default agent prompts.
pub fn default_agent_prompts() -> Vec<AgentPrompt> {
    vec![
        AgentPrompt {
            label: "Triage".into(),
            command: "bvr --robot-triage".into(),
            description: "Run unified triage analysis".into(),
        },
        AgentPrompt {
            label: "Next Pick".into(),
            command: "bvr --robot-next".into(),
            description: "Get the single top recommendation".into(),
        },
        AgentPrompt {
            label: "Insights".into(),
            command: "bvr --robot-insights".into(),
            description: "View graph metrics and top-N lists".into(),
        },
        AgentPrompt {
            label: "Plan".into(),
            command: "bvr --robot-plan".into(),
            description: "Dependency-respecting execution plan".into(),
        },
        AgentPrompt {
            label: "Search".into(),
            command: "bvr --robot-search --search".into(),
            description: "Semantic search across issues".into(),
        },
        AgentPrompt {
            label: "Graph".into(),
            command: "bvr --robot-graph".into(),
            description: "Dependency graph as JSON/DOT/Mermaid".into(),
        },
        AgentPrompt {
            label: "History".into(),
            command: "bvr --robot-history".into(),
            description: "Bead-commit correlation from git log".into(),
        },
        AgentPrompt {
            label: "Orphans".into(),
            command: "bvr --robot-orphans".into(),
            description: "Detect orphan commits not linked to beads".into(),
        },
    ]
}

/// Render the agent prompt modal.
pub fn render_agent_prompt(f: &mut Frame, prompts: &[AgentPrompt], selected: usize, area: Rect) {
    let popup_width = 55.min(area.width.saturating_sub(4));
    let popup_height = (prompts.len() as u16 + 6).min(area.height.saturating_sub(2));
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
        " Agent Commands ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (i, prompt) in prompts.iter().enumerate() {
        let style = if i == selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let indicator = if i == selected { "▶ " } else { "  " };
        lines.push(Line::from(Span::styled(
            format!("{}{} - {}", indicator, prompt.label, prompt.description),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " j/k: navigate | Enter: copy command | Esc: close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Agent Prompts ")
        .border_style(Style::default().fg(Color::Cyan));

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}
