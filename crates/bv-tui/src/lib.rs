//! bv-tui: terminal UI for beads — ratatui-based, Elm-inspired event loop.
//! Phase 6 TUI-M1 slice: core journey (open→load→list→detail→filter→quit).

use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::io;

/// Issue status (re-exported for view rendering).
use bv_core::model::Status;

/// Filter mode (Go: o/c/r/a keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    All,
    Open,
    Closed,
    Ready,
}

impl FilterMode {
    pub fn label(self) -> &'static str {
        match self {
            FilterMode::All => "all",
            FilterMode::Open => "open",
            FilterMode::Closed => "closed",
            FilterMode::Ready => "ready",
        }
    }
}

/// Sort mode (5 modes cycling with `s`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Default,
    CreatedAsc,
    CreatedDesc,
    Priority,
    Updated,
}

impl SortMode {
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Default => "Default",
            SortMode::CreatedAsc => "Created \u{2191}",
            SortMode::CreatedDesc => "Created \u{2193}",
            SortMode::Priority => "Priority",
            SortMode::Updated => "Updated",
        }
    }

    pub fn next(self) -> Self {
        match self {
            SortMode::Default => SortMode::CreatedAsc,
            SortMode::CreatedAsc => SortMode::CreatedDesc,
            SortMode::CreatedDesc => SortMode::Priority,
            SortMode::Priority => SortMode::Updated,
            SortMode::Updated => SortMode::Default,
        }
    }
}

/// A display row in the list (one per visible issue).
pub struct ListRow {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub priority: i32,
}

/// Application state (Elm model).
pub struct App {
    pub rows: Vec<ListRow>,
    pub filtered_indices: Vec<usize>,
    pub cursor: usize,
    pub filter_mode: FilterMode,
    pub sort_mode: SortMode,
    pub show_detail: bool,
    pub quit_requested: bool,
    pub quit_confirmed: bool,
    pub width: u16,
    pub height: u16,
    /// True when terminal width > 100 (split view threshold).
    pub split_view: bool,
    pub status_msg: String,
    /// Current active view (list is default, toggled by b/E/g/i/etc).
    pub current_view: ViewMode,
}

/// Which view is currently displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Board,
    Tree,
    Graph,
    Insights,
}

impl App {
    pub fn new(issues: Vec<bv_core::model::Issue>) -> Self {
        let rows: Vec<ListRow> = issues
            .iter()
            .map(|i| ListRow {
                id: i.id.clone(),
                title: i.title.clone(),
                status: i.status,
                priority: i.priority,
            })
            .collect();
        let mut app = App {
            rows,
            filtered_indices: Vec::new(),
            cursor: 0,
            filter_mode: FilterMode::All,
            sort_mode: SortMode::Default,
            show_detail: false,
            quit_requested: false,
            quit_confirmed: false,
            width: 120,
            height: 40,
            split_view: true,
            status_msg: String::new(),
            current_view: ViewMode::List,
        };
        app.apply_filter();
        app
    }

    pub fn apply_filter(&mut self) {
        self.filtered_indices = (0..self.rows.len())
            .filter(|&i| {
                let r = &self.rows[i];
                match self.filter_mode {
                    FilterMode::All => true,
                    FilterMode::Open => matches!(r.status, Status::Open | Status::InProgress),
                    FilterMode::Closed => r.status.is_closed(),
                    FilterMode::Ready => matches!(r.status, Status::Open),
                }
            })
            .collect();
        // Apply sort
        if self.sort_mode == SortMode::Default {
            self.filtered_indices.sort_by_key(|&i| {
                (
                    self.rows[i].priority,
                    std::cmp::Reverse(self.rows[i].id.clone()),
                )
            });
        }
        if self.cursor >= self.filtered_indices.len() {
            self.cursor = self.filtered_indices.len().saturating_sub(1);
        }
    }

    pub fn cycle_filter(&mut self) {
        self.filter_mode = match self.filter_mode {
            FilterMode::All => FilterMode::Open,
            FilterMode::Open => FilterMode::Closed,
            FilterMode::Closed => FilterMode::Ready,
            FilterMode::Ready => FilterMode::All,
        };
        self.apply_filter();
    }

    pub fn selected(&self) -> Option<&ListRow> {
        self.filtered_indices
            .get(self.cursor)
            .map(|&i| &self.rows[i])
    }

    /// Handle a key event; returns true if the event was consumed.
    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => {
                self.quit_requested = true;
                true
            }
            KeyCode::Esc => {
                if self.quit_confirmed {
                    self.quit_confirmed = false;
                    true
                } else {
                    self.quit_requested = true;
                    true
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.cursor + 1 < self.filtered_indices.len() {
                    self.cursor += 1;
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                true
            }
            KeyCode::Char('o') => {
                self.filter_mode = FilterMode::Open;
                self.apply_filter();
                true
            }
            KeyCode::Char('c') => {
                self.filter_mode = FilterMode::Closed;
                self.apply_filter();
                true
            }
            KeyCode::Char('r') => {
                self.filter_mode = FilterMode::Ready;
                self.apply_filter();
                true
            }
            KeyCode::Char('b') => {
                self.current_view = if self.current_view == ViewMode::Board {
                    ViewMode::List
                } else {
                    ViewMode::Board
                };
                true
            }
            KeyCode::Char('i') => {
                self.current_view = if self.current_view == ViewMode::Insights {
                    ViewMode::List
                } else {
                    ViewMode::Insights
                };
                true
            }
            KeyCode::Char('E') => {
                self.current_view = if self.current_view == ViewMode::Tree {
                    ViewMode::List
                } else {
                    ViewMode::Tree
                };
                true
            }
            KeyCode::Char('a') => {
                self.filter_mode = FilterMode::All;
                self.apply_filter();
                true
            }
            KeyCode::Char('s') => {
                self.sort_mode = self.sort_mode.next();
                self.apply_filter();
                true
            }
            KeyCode::Enter => {
                self.show_detail = !self.show_detail;
                true
            }
            _ => false,
        }
    }
}

/// Render the UI frame.
pub fn render(f: &mut Frame, app: &App) {
    match app.current_view {
        ViewMode::Tree => {
            let lines = crate::views::tree::render_tree_lines(&[]);
            let block = ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(" TREE VIEW ");
            f.render_widget(
                ratatui::widgets::Paragraph::new(lines).block(block),
                f.area(),
            );
            render_status_bar(f, app);
            return;
        }
        ViewMode::Insights => {
            crate::views::insights::render_insights(
                f,
                &Default::default(),
                &Default::default(),
                &Default::default(),
                &Default::default(),
            );
            render_status_bar(f, app);
            return;
        }
        ViewMode::Board => {
            crate::views::board::render_board(
                f,
                app,
                f.area(),
                crate::views::board::SwimlaneMode::Status,
            );
            render_status_bar(f, app);
            return;
        }
        _ => {}
    }

    let chunks = if app.split_view && app.width > 100 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(f.area())
    };

    render_list(f, app, chunks[0]);
    if app.split_view && app.width > 100 && chunks.len() > 1 {
        render_detail(f, app, chunks[1]);
    }

    render_status_bar(f, app);
}

fn status_color(s: Status) -> Color {
    match s {
        Status::Open => Color::Green,
        Status::InProgress => Color::Yellow,
        Status::Blocked => Color::Red,
        Status::Closed | Status::Tombstone => Color::DarkGray,
        _ => Color::Cyan,
    }
}

fn render_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(vis_idx, &row_idx)| {
            let row = &app.rows[row_idx];
            let selected = vis_idx == app.cursor;
            let prefix = if selected { "> " } else { "  " };
            let color = status_color(row.status);
            ListItem::new(Line::from(vec![
                Span::raw(prefix),
                Span::styled(format!("{:<24}", row.id), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("P{}", row.priority),
                    Style::default().fg(if row.priority <= 1 {
                        Color::Red
                    } else {
                        Color::Gray
                    }),
                ),
                Span::raw(" "),
                Span::styled(&row.title, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(row.status.as_str().to_string(), Style::default().fg(color)),
            ]))
        })
        .collect();

    let title = format!(
        " ISSUES ({}) [{}] ",
        app.filtered_indices.len(),
        app.sort_mode.label()
    );
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">");
    let mut state = ListState::default();
    state.select(Some(app.cursor));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_detail(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let content = match app.selected() {
        Some(row) => vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&row.id),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    row.status.as_str(),
                    Style::default().fg(status_color(row.status)),
                ),
            ]),
            Line::from(vec![
                Span::styled("Priority: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("P{}", row.priority)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                &row.title,
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ],
        None => vec![Line::from("No issue selected")],
    };
    let para =
        Paragraph::new(content).block(Block::default().borders(Borders::ALL).title(" DETAIL "));
    f.render_widget(para, area);
}

fn render_status_bar(f: &mut Frame, app: &App) {
    let area = ratatui::layout::Rect {
        x: 0,
        y: f.area().height - 1,
        width: f.area().width,
        height: 1,
    };
    let filter_label = app.filter_mode.label();
    let msg = if app.status_msg.is_empty() {
        format!(
            " {} issues | filter:{} | sort:{} | q:quit",
            app.filtered_indices.len(),
            filter_label,
            app.sort_mode.label()
        )
    } else {
        format!(" {}", app.status_msg)
    };
    let bar = Paragraph::new(Line::from(vec![Span::styled(
        msg,
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(bar, area);
}

/// Run the TUI event loop. Returns when user quits.
pub fn run_tui(app: &mut App) -> io::Result<()> {
    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    loop {
        terminal.draw(|f| render(f, app))?;
        if app.quit_requested && app.quit_confirmed {
            break;
        }
        if let CEvent::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key.code);
            }
        }
    }

    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app(n: usize) -> App {
        let issues: Vec<bv_core::model::Issue> = (0..n)
            .map(|i| bv_core::model::Issue {
                id: format!("T-{i}"),
                content_hash: String::new(),
                title: format!("Issue {i}"),
                description: String::new(),
                design: String::new(),
                acceptance_criteria: String::new(),
                notes: String::new(),
                status: if i % 3 == 0 {
                    Status::Closed
                } else {
                    Status::Open
                },
                priority: (i % 4) as i32,
                issue_type: "task".into(),
                assignee: String::new(),
                estimated_minutes: None,
                created_at: None,
                updated_at: None,
                due_date: None,
                closed_at: None,
                external_ref: None,
                compaction_level: 0,
                compacted_at: None,
                compacted_at_commit: None,
                original_size: 0,
                labels: vec![],
                dependencies: vec![],
                comments: vec![],
                source_repo: String::new(),
            })
            .collect();
        App::new(issues)
    }

    #[test]
    fn filter_open_shows_only_non_closed() {
        let mut app = make_app(9); // every 3rd closed → 6 non-closed
        app.filter_mode = FilterMode::Open;
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 6);
    }

    #[test]
    fn j_k_navigation_bounds() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.cursor, 1);
        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char('j')); // clamped at last
        assert_eq!(app.cursor, 2);
        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn sort_mode_cycles_through_five() {
        let mut app = make_app(3);
        let start = app.sort_mode;
        app.handle_key(KeyCode::Char('s'));
        assert_ne!(start, app.sort_mode);
    }

    #[test]
    fn quit_sets_flag() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('q'));
        assert!(app.quit_requested);
    }
}

pub mod chrome;
pub mod views;
pub mod worker;
