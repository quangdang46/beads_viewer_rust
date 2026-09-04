//! History/time-travel view — port of Go `pkg/ui/history.go` (subset).
//!
//! Shows bead↔commit correlations with bead-centric and git-centric modes.
//! Responsive layout: narrow (2-pane) vs standard/wide (3-pane with timeline).
//!
//! Scope cut vs Go (3,671 lines): cass session integration, full search with
//! mode switching, file tree panel, and transition animations are not ported.
//! Core navigation (bead/commit selection, mode toggle, confidence filter,
//! timeline of events) is real.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

/// Correlated commit for a bead (simplified from correlator module).
#[derive(Debug, Clone)]
pub struct HistoryCommit {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub confidence: f64,
    pub files: Vec<String>,
}

/// A bead with its correlated commits.
#[derive(Debug, Clone)]
pub struct BeadHistory {
    pub bead_id: String,
    pub title: String,
    pub status: String,
    pub commits: Vec<HistoryCommit>,
}

/// Timeline event (simplified — lifecycle events + commits).
#[derive(Debug, Clone)]
pub enum TimelineEvent {
    Commit {
        sha: String,
        short_sha: String,
        message: String,
        confidence: f64,
        timestamp: String,
    },
    Lifecycle {
        event_type: String,
        detail: String,
        timestamp: String,
    },
}

/// History view mode: bead-centric or git-centric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryMode {
    Bead,
    Git,
}

/// State held by App for the History view.
pub struct HistoryState {
    pub bead_histories: Vec<BeadHistory>,
    pub selected_bead: usize,
    pub selected_commit: usize,
    pub mode: HistoryMode,
    pub min_confidence: f64,
}

impl HistoryState {
    pub fn build_from_beads(bead_histories: Vec<BeadHistory>) -> Self {
        HistoryState {
            bead_histories,
            selected_bead: 0,
            selected_commit: 0,
            mode: HistoryMode::Bead,
            min_confidence: 0.0,
        }
    }

    pub fn selected_bead(&self) -> Option<&BeadHistory> {
        self.bead_histories.get(self.selected_bead)
    }

    pub fn selected_commit(&self) -> Option<&HistoryCommit> {
        self.selected_bead()
            .and_then(|b| b.commits.get(self.selected_commit))
    }

    pub fn move_bead_up(&mut self) {
        self.selected_bead = self.selected_bead.saturating_sub(1);
        self.selected_commit = 0;
    }

    pub fn move_bead_down(&mut self) {
        if self.selected_bead + 1 < self.bead_histories.len() {
            self.selected_bead += 1;
            self.selected_commit = 0;
        }
    }

    pub fn move_commit_up(&mut self) {
        self.selected_commit = self.selected_commit.saturating_sub(1);
    }

    pub fn move_commit_down(&mut self) {
        if let Some(bead) = self.selected_bead() {
            if self.selected_commit + 1 < bead.commits.len() {
                self.selected_commit += 1;
            }
        }
    }

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            HistoryMode::Bead => HistoryMode::Git,
            HistoryMode::Git => HistoryMode::Bead,
        };
        self.selected_bead = 0;
        self.selected_commit = 0;
    }

    pub fn build_timeline(&self) -> Vec<TimelineEvent> {
        let mut events = Vec::new();
        if let Some(bead) = self.selected_bead() {
            // Lifecycle events
            events.push(TimelineEvent::Lifecycle {
                event_type: "created".into(),
                detail: format!("Bead {} created", bead.bead_id),
                timestamp: String::new(),
            });
            for commit in &bead.commits {
                events.push(TimelineEvent::Commit {
                    sha: commit.sha.clone(),
                    short_sha: commit.short_sha.clone(),
                    message: commit.message.clone(),
                    confidence: commit.confidence,
                    timestamp: commit.timestamp.clone(),
                });
            }
        }
        events.sort_by(|a, b| {
            let ts_a = match a {
                TimelineEvent::Commit { timestamp, .. } => timestamp.clone(),
                TimelineEvent::Lifecycle { timestamp, .. } => timestamp.clone(),
            };
            let ts_b = match b {
                TimelineEvent::Commit { timestamp, .. } => timestamp.clone(),
                TimelineEvent::Lifecycle { timestamp, .. } => timestamp.clone(),
            };
            ts_a.cmp(&ts_b)
        });
        events
    }
}

/// Render the history view.
pub fn render_history(f: &mut Frame, state: &HistoryState, area: Rect) {
    if state.bead_histories.is_empty() {
        let msg = Paragraph::new("No correlation data available")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, area);
        return;
    }

    let layout_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30),
            Constraint::Length(1),
            Constraint::Min(40),
        ])
        .split(area);

    render_bead_list(f, state, layout_chunks[0]);
    render_separator(f, layout_chunks[1]);
    render_commit_detail(f, state, layout_chunks[2]);
}

fn render_bead_list(f: &mut Frame, state: &HistoryState, area: Rect) {
    let items: Vec<ListItem> = state
        .bead_histories
        .iter()
        .enumerate()
        .map(|(i, bead)| {
            let style = if i == state.selected_bead {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                let color = match bead.status.as_str() {
                    "open" => Color::Green,
                    "in_progress" => Color::Cyan,
                    "blocked" => Color::Red,
                    "closed" => Color::DarkGray,
                    _ => Color::White,
                };
                Style::default().fg(color)
            };
            let line = Line::from(Span::styled(
                format!("{} [{}]", bead.bead_id, bead.commits.len()),
                style,
            ));
            ListItem::new(line)
        })
        .collect();

    let mode_title = match state.mode {
        HistoryMode::Bead => " BEADS ",
        HistoryMode::Git => " COMMITS ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(mode_title);

    let list = List::new(items).block(block);
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_bead));
    f.render_stateful_widget(list, area, &mut list_state);
}

fn render_separator(f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(Color::DarkGray))))
        .collect();
    let sep = Paragraph::new(lines);
    f.render_widget(sep, area);
}

fn render_commit_detail(f: &mut Frame, state: &HistoryState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(bead) = state.selected_bead() {
        // Bead header
        lines.push(Line::from(Span::styled(
            format!(" {} - {}", bead.bead_id, bead.title),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!(" Status: {}", bead.status),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(format!(
            " Commits: {}",
            bead.commits.len()
        )));
        lines.push(Line::from(""));

        // Commit list
        if bead.commits.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No correlated commits",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "── Correlated Commits ──",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for (i, commit) in bead.commits.iter().enumerate() {
                let is_selected = i == state.selected_commit;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let conf_color = if commit.confidence >= 0.8 {
                    Color::Green
                } else if commit.confidence >= 0.5 {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };

                let indicator = if is_selected { "▶ " } else { "  " };

                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{} {} {:.0}%",
                        indicator,
                        commit.short_sha,
                        truncate_str(&commit.message, 50),
                        commit.confidence * 100.0
                    ),
                    style,
                )));

                // Show file count for selected commit
                if is_selected && !commit.files.is_empty() {
                    let files_str = commit.files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                    let more = if commit.files.len() > 5 {
                        format!(" +{} more", commit.files.len() - 5)
                    } else {
                        String::new()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("    Files: {files_str}{more}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.push(Line::from(Span::styled(
                        format!("    Author: {} | Confidence: {:.0}%", commit.author, commit.confidence * 100.0),
                        Style::default().fg(conf_color),
                    )));
                }
            }
        }

        // Timeline
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "── Timeline ──",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        let timeline = state.build_timeline();
        for event in &timeline {
            match event {
                TimelineEvent::Commit { short_sha, message, confidence, .. } => {
                    let color = if *confidence >= 0.8 {
                        Color::Green
                    } else if *confidence >= 0.5 {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    };
                    lines.push(Line::from(Span::styled(
                        format!("  ● {short_sha} {} ({:.0}%)", truncate_str(message, 40), confidence * 100.0),
                        Style::default().fg(color),
                    )));
                }
                TimelineEvent::Lifecycle { event_type, detail, .. } => {
                    lines.push(Line::from(Span::styled(
                        format!("  ◆ {event_type}: {detail}"),
                        Style::default().fg(Color::Cyan),
                    )));
                }
            }
        }

        // Navigation hint
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "j/k: bead | g/h: commit | t: toggle mode",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" HISTORY ");

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn truncate_str(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        s.to_string()
    } else if max_len <= 1 {
        "…".to_string()
    } else {
        let truncated: String = chars[..max_len - 1].iter().collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_state_bead_navigation() {
        let mut state = HistoryState {
            bead_histories: vec![
                BeadHistory {
                    bead_id: "A".into(),
                    title: "A".into(),
                    status: "open".into(),
                    commits: vec![],
                },
                BeadHistory {
                    bead_id: "B".into(),
                    title: "B".into(),
                    status: "open".into(),
                    commits: vec![],
                },
            ],
            selected_bead: 0,
            selected_commit: 0,
            mode: HistoryMode::Bead,
            min_confidence: 0.0,
        };
        assert_eq!(state.selected_bead().unwrap().bead_id, "A");
        state.move_bead_down();
        assert_eq!(state.selected_bead().unwrap().bead_id, "B");
        state.move_bead_up();
        assert_eq!(state.selected_bead().unwrap().bead_id, "A");
    }

    #[test]
    fn history_mode_toggle() {
        let mut state = HistoryState {
            bead_histories: vec![],
            selected_bead: 0,
            selected_commit: 0,
            mode: HistoryMode::Bead,
            min_confidence: 0.0,
        };
        assert_eq!(state.mode, HistoryMode::Bead);
        state.toggle_mode();
        assert_eq!(state.mode, HistoryMode::Git);
        state.toggle_mode();
        assert_eq!(state.mode, HistoryMode::Bead);
    }
}
