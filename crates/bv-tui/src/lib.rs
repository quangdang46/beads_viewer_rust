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
use std::collections::BTreeMap;
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
    pub issue_type: String,
    pub labels: Vec<String>,
    pub created_at: Option<String>,
    pub description: String,
    pub notes: String,
    pub assignee: String,
}

/// Application state (Elm model).
/// Stores computed graph metrics for rendering views.
#[derive(Default, Clone)]
pub struct GraphMetrics {
    pub pagerank: BTreeMap<String, f64>,
    pub betweenness: BTreeMap<String, f64>,
    pub eigenvector: BTreeMap<String, f64>,
    pub hubs: BTreeMap<String, f64>,
    pub authorities: BTreeMap<String, f64>,
}

pub struct App {
    pub rows: Vec<ListRow>,
    pub filtered_indices: Vec<usize>,
    pub cursor: usize,
    pub filter_mode: FilterMode,
    pub sort_mode: SortMode,
    pub show_detail: bool,
    pub quit_requested: bool,
    pub width: u16,
    pub height: u16,
    /// True when terminal width > 100 (split view threshold).
    pub split_view: bool,
    pub status_msg: String,
    /// Current active view (list is default, toggled by b/E/g/i/etc).
    pub current_view: ViewMode,
    /// Search mode active (/ pressed).
    pub searching: bool,
    /// Current search query.
    pub search_query: String,
    pub show_sidebar: bool,
    /// Which panel has focus: false = list, true = detail
    pub focus_detail: bool,
    /// Scroll offset for the detail pane
    pub detail_scroll: u16,
    /// Graph metric maps from analysis (for insights rendering)
    pub graph_metrics: Option<GraphMetrics>,
}

/// Which view is currently displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Board,
    Tree,
    Graph,
    Insights,
    Alerts,
    TimeTravel,
    Tutorial,
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
                issue_type: i.issue_type.clone(),
                labels: i.labels.clone(),
                created_at: i.created_at.clone(),
                description: i.description.clone(),
                notes: i.notes.clone(),
                assignee: i.assignee.clone(),
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
            width: 120,
            height: 40,
            split_view: true,
            status_msg: String::new(),
            current_view: ViewMode::List,
            searching: false,
            search_query: String::new(),
            show_sidebar: false,
            focus_detail: false,
            detail_scroll: 0,
            graph_metrics: None,
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

    fn handle_search_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                self.searching = false;
                self.search_query.clear();
                self.apply_filter();
                true
            }
            KeyCode::Enter => {
                self.searching = false;
                self.apply_filter();
                true
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.apply_search();
                true
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.apply_search();
                true
            }
            _ => true,
        }
    }

    fn apply_search(&mut self) {
        let q = self.search_query.to_lowercase();
        if q.is_empty() {
            self.filtered_indices = (0..self.rows.len()).collect();
        } else {
            self.filtered_indices = (0..self.rows.len())
                .filter(|&i| {
                    let r = &self.rows[i];
                    r.id.to_lowercase().contains(&q) || r.title.to_lowercase().contains(&q)
                })
                .collect();
        }
        self.cursor = 0;
    }

    /// Handle mouse events (wheel scroll + click select).
    pub fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> bool {
        use crossterm::event::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if self.cursor + 1 < self.filtered_indices.len() {
                    self.cursor += 1;
                }
                true
            }
            MouseEventKind::ScrollUp => {
                self.cursor = self.cursor.saturating_sub(1);
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let header_lines = 2;
                if mouse.row > header_lines {
                    // Click in left 40% → select list item
                    if mouse.column < (self.width as f64 * 0.4) as u16 {
                        let idx = (mouse.row - header_lines - 1) as usize;
                        if idx < self.filtered_indices.len() {
                            self.cursor = idx;
                            self.focus_detail = false;
                        }
                    } else {
                        // Click on right panel → focus detail
                        self.focus_detail = true;
                    }
                }
                true
            }
            _ => false,
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
        if self.searching {
            return self.handle_search_key(code);
        }
        match code {
            KeyCode::Tab => {
                self.focus_detail = !self.focus_detail;
                true
            }
            KeyCode::Char('q') => {
                if self.focus_detail {
                    self.focus_detail = false;
                } else {
                    self.quit_requested = true;
                }
                true
            }
            KeyCode::Esc => {
                if self.searching {
                    self.searching = false;
                    self.search_query.clear();
                    self.apply_filter();
                } else if self.focus_detail {
                    self.focus_detail = false;
                } else if self.show_detail {
                    self.show_detail = false;
                } else {
                    self.quit_requested = true;
                }
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.focus_detail {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                } else if self.cursor + 1 < self.filtered_indices.len() {
                    self.cursor += 1;
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus_detail {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                }
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
            KeyCode::Char('/') => {
                self.searching = true;
                self.search_query.clear();
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
            KeyCode::Char('`') => {
                self.current_view = if self.current_view == ViewMode::Tutorial {
                    ViewMode::List
                } else {
                    ViewMode::Tutorial
                };
                true
            }
            KeyCode::Char(';') => {
                self.show_sidebar = !self.show_sidebar;
                true
            }
            KeyCode::Char('t') => {
                self.current_view = if self.current_view == ViewMode::TimeTravel {
                    ViewMode::List
                } else {
                    ViewMode::TimeTravel
                };
                true
            }
            KeyCode::Char('!') => {
                self.current_view = if self.current_view == ViewMode::Alerts {
                    ViewMode::List
                } else {
                    ViewMode::Alerts
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

fn type_icon(issue_type: &str) -> (&'static str, Color) {
    match issue_type {
        "bug" => ("🐛", Color::Red),
        "feature" => ("✨", Color::Green),
        "task" => ("📋", Color::Blue),
        "epic" => ("🚀", Color::Magenta),
        "chore" => ("🧹", Color::Gray),
        _ => ("•", Color::DarkGray),
    }
}

fn prio_badge_style(priority: i32) -> (Color, Color) {
    match priority {
        0 => (Color::White, Color::Red),
        1 => (Color::White, Color::LightRed),
        2 => (Color::Black, Color::Yellow),
        3 => (Color::White, Color::Blue),
        _ => (Color::Gray, Color::DarkGray),
    }
}

fn status_badge(status: &str) -> (&'static str, Color) {
    match status {
        "open" => ("OPEN", Color::Green),
        "in_progress" => ("PROG", Color::Yellow),
        "blocked" => ("BLKD", Color::Red),
        "deferred" => ("DEFR", Color::Cyan),
        "draft" => ("DRFT", Color::Cyan),
        "pinned" => ("PIN", Color::Magenta),
        "hooked" => ("HOOK", Color::Cyan),
        "review" => ("REVW", Color::Blue),
        "closed" => ("DONE", Color::DarkGray),
        "tombstone" => ("TOMB", Color::DarkGray),
        _ => ("????", Color::Gray),
    }
}

fn age_str(created_at: &Option<String>) -> String {
    if let Some(ca) = created_at {
        if let Ok(t) = ca.parse::<jiff::Timestamp>() {
            let now = jiff::Timestamp::now();
            let days = (now - t).total(jiff::Unit::Second).unwrap_or(0.0) / 86400.0;
            if days < 1.0 {
                return "today".into();
            }
            if days < 30.0 {
                return format!("{days:.0}d");
            }
            if days < 365.0 {
                return format!("{:.0}w", days / 7.0);
            }
            return format!("{:.0}y", days / 365.0);
        }
    }
    String::new()
}

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
            let empty = BTreeMap::new();
            let (pr, bw, hub, auth) = if let Some(ref gm) = app.graph_metrics {
                (&gm.pagerank, &gm.betweenness, &gm.hubs, &gm.authorities)
            } else {
                (&empty, &empty, &empty, &empty)
            };
            crate::views::insights::render_insights(f, pr, bw, hub, auth);
            render_status_bar(f, app);
            return;
        }
        ViewMode::Alerts => {
            crate::views::alerts::render_alerts(f, &[], 0, f.area());
            render_status_bar(f, app);
            return;
        }
        ViewMode::Tutorial => {
            let help = crate::chrome::default_help_entries();
            let lines: Vec<Line> = vec![
                Line::from(Span::styled(
                    "bvr Tutorial",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Navigation:"),
                Line::from(Span::styled("  j/k     Move down/up", Style::default())),
                Line::from(Span::styled(
                    "  g/G     Jump to top/bottom",
                    Style::default(),
                )),
                Line::from(""),
                Line::from("Views:"),
            ]
            .into_iter()
            .chain(
                help.iter()
                    .map(|(k, d)| Line::from(Span::raw(format!("  {k:<12} {d}")))),
            )
            .collect();
            let para = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" \u{1f4d6} TUTORIAL — Press ` to close "),
            );
            f.render_widget(para, f.area());
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

    if app.show_sidebar {
        render_sidebar(f, app);
    }

    render_status_bar(f, app);
}

fn render_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let inner_width = area.width.saturating_sub(2) as usize;

    // Header row matching Go: "  TYPE PRI STATUS      ID                     TITLE"
    let header = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
        "  TYPE PRI STATUS      ID                     TITLE",
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    )));
    let header_area = ratatui::layout::Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    f.render_widget(header, header_area);

    // List items
    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(vis_idx, &row_idx)| {
            let row = &app.rows[row_idx];
            let selected = vis_idx == app.cursor;
            let (icon, icon_color) = type_icon(&row.issue_type);

            // Priority badge
            let (pfg, pbg) = prio_badge_style(row.priority);
            let prio_label = format!("P{}", row.priority);

            // Status badge
            let status_str = row.status.as_str();
            let (slabel, scolor) = status_badge(status_str);

            // Age
            let age = age_str(&row.created_at);

            let mut spans = vec![];

            // Selection indicator
            if selected {
                spans.push(Span::styled(
                    "▸ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw("  "));
            }

            // Type icon
            spans.push(Span::styled(icon, Style::default().fg(icon_color)));
            spans.push(Span::raw(" "));

            // Priority badge
            spans.push(Span::styled(
                format!("{:<3}", prio_label),
                Style::default()
                    .fg(pfg)
                    .bg(pbg)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(" "));

            // Status badge
            spans.push(Span::styled(
                format!("{:<4}", slabel),
                Style::default().fg(scolor),
            ));
            spans.push(Span::raw(" "));

            // ID
            spans.push(Span::styled(
                format!("{:<20}", row.id),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(" "));

            // Title (truncated to fit)
            let title_width = inner_width.saturating_sub(45);
            let title = if row.title.len() > title_width {
                format!("{}…", &row.title[..title_width.saturating_sub(1)])
            } else {
                format!("{:<width$}", row.title, width = title_width)
            };
            spans.push(Span::styled(
                title,
                Style::default().add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ));

            // Age (right-aligned)
            if !age.is_empty() {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(age, Style::default().fg(Color::DarkGray)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = format!(
        " ISSUES ({}) [{}] ",
        app.filtered_indices.len(),
        app.sort_mode.label()
    );
    let list_border_color = if !app.focus_detail {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(list_border_color)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">");

    // Offset by 1 for the header row
    let list_area = ratatui::layout::Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height.saturating_sub(1),
    };

    let mut state = ListState::default();
    state.select(Some(app.cursor));
    f.render_stateful_widget(list, list_area, &mut state);
}

fn render_detail(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let content: Vec<Line> = match app.selected() {
        Some(row) => {
            // Build graph scores if available
            // Look up actual graph metrics for this issue
            let graph_scores = app.graph_metrics.as_ref().and_then(|gm| {
                gm.pagerank
                    .get(&row.id)
                    .map(|&pr| crate::detail::GraphScores {
                        pagerank: pr,
                        betweenness: gm.betweenness.get(&row.id).copied().unwrap_or(0.0),
                        eigenvector: gm.eigenvector.get(&row.id).copied().unwrap_or(0.0),
                        hubs: gm.hubs.get(&row.id).copied().unwrap_or(0.0),
                        authorities: gm.authorities.get(&row.id).copied().unwrap_or(0.0),
                        critical_path: 0.0,
                    })
            });
            crate::detail::build_detail_lines_from_row(row, &graph_scores)
        }
        None => vec![
            Line::from(""),
            Line::from(Span::styled(
                "No issue selected",
                Style::default().fg(Color::DarkGray),
            )),
        ],
    };
    let focused_border = if app.focus_detail {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = if app.focus_detail {
        " DETAIL ◄ FOCUSED "
    } else {
        " DETAIL "
    };
    let para = ratatui::widgets::Paragraph::new(content)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((app.detail_scroll, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(focused_border)),
        );
    f.render_widget(para, area);
}

fn render_sidebar(f: &mut Frame, _app: &App) {
    let entries = crate::chrome::default_help_entries();
    let area = ratatui::layout::Rect {
        x: f.area().width.saturating_sub(34),
        y: 0,
        width: 34.min(f.area().width),
        height: f.area().height,
    };
    crate::chrome::render_sidebar(f, area, &[("Navigation", entries)]);
}

fn render_status_bar(f: &mut Frame, app: &App) {
    if app.searching {
        let area = ratatui::layout::Rect {
            x: 0,
            y: f.area().height - 1,
            width: f.area().width,
            height: 1,
        };
        let bar = Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::styled(&app.search_query, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]));
        f.render_widget(bar, area);
        return;
    }
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
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    loop {
        terminal.draw(|f| render(f, app))?;
        if app.quit_requested {
            break;
        }
        match event::read()? {
            CEvent::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }
                    app.handle_key(key.code);
                }
            }
            CEvent::Mouse(mouse) => {
                app.handle_mouse(mouse);
            }
            _ => {}
        }
    }

    crossterm::execute!(
        io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
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
pub mod detail;
pub mod views;
pub mod worker;
