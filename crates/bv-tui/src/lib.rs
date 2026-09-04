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
    /// Full issue data keyed by ID (for detail pane rendering)
    pub issue_map: std::collections::HashMap<String, bv_core::model::Issue>,
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
    pub show_help: bool,
    pub label_filter: Option<String>,
    /// Scroll offset for the detail pane
    pub detail_scroll: u16,
    /// Graph metric maps from analysis (for insights rendering)
    pub graph_metrics: Option<GraphMetrics>,
    /// Active drift alerts (critical, warning, total) for footer badge
    pub alerts_critical: usize,
    pub alerts_warning: usize,
    pub alerts_total: usize,
    /// The actual alert messages backing the counts above, for the Alerts view.
    pub alerts: Vec<bv_analysis::drift::Alert>,
    /// Collapsed node ids in the Tree view (all nodes start expanded).
    pub tree_collapsed: std::collections::HashSet<String>,
    /// Cursor position within the Alerts view list.
    pub alerts_cursor: usize,
    /// Cross-label flow data for the FlowMatrix view.
    pub flow: Option<bv_analysis::label_health::CrossLabelFlow>,
    pub flow_cursor: usize,
    /// Attention scores for the Attention view.
    pub attention_labels: Vec<bv_analysis::label_health::LabelAttentionScore>,
    pub attention_cursor: usize,
    /// Graph view: selected issue index (sorted IDs).
    pub graph_cursor: usize,
    /// Graph view: scroll offset in the node list panel.
    pub graph_scroll: usize,
    /// Precomputed graph data for the Graph view.
    pub graph_data: Option<crate::views::graph::GraphData>,
    /// History/time-travel view state.
    pub history: Option<crate::views::history::HistoryState>,
    /// Label picker state.
    pub label_picker: Option<crate::views::pickers::LabelPicker>,
    /// Sprint dashboard state (loaded from .beads/sprints.jsonl).
    pub sprint: Option<crate::views::sprint::SprintState>,
    /// When the snapshot was loaded (freshness badge, Go bv-h305)
    pub loaded_at: std::time::Instant,
    /// PID of another live instance holding .beads/.bv.lock (Go bv-vrvn)
    pub instance_pid: Option<u32>,
    /// Large/huge dataset warning text (Go bv-9thm)
    pub dataset_warning: Option<String>,
    /// cass session count for selected bead (Go bv-y836)
    pub session_count: usize,
    pub update_tag: Option<String>,
    /// Workspace mode: loaded repo names (Go workspaceMode)
    pub workspace_repos: Option<Vec<String>>,
    /// Active repo filter in workspace mode (None = all repos)
    pub active_repo: Option<String>,
    /// cass CLI availability cache
    cass_available: bool,
    cass_cache: std::collections::HashMap<String, usize>,
}

/// Which view is currently displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Board,
    Tree,
    Graph,
    FlowMatrix,
    Attention,
    Insights,
    Alerts,
    TimeTravel,
    Sprint,
    Tutorial,
}

/// Large/huge dataset warning (Go largeDatasetWarning, bv-9thm).
fn dataset_warning_for(total: usize) -> Option<String> {
    let compact = |n: usize| {
        if n >= 1_000_000 {
            format!("{}m", n / 1_000_000)
        } else if n >= 1_000 {
            format!("{}k", n / 1_000)
        } else {
            format!("{n}")
        }
    };
    if total >= 20_000 {
        Some(format!("\u{26a0} huge {} issues", compact(total)))
    } else if total >= 5_000 {
        Some(format!("\u{26a0} large {} issues", compact(total)))
    } else {
        None
    }
}

/// Check whether the cass CLI is on PATH (Go cass.Detector).
fn cass_installed() -> bool {
    std::env::var("PATH")
        .map(|paths| {
            paths
                .split(':')
                .any(|dir| std::path::Path::new(dir).join("cass").is_file())
        })
        .unwrap_or(false)
}

/// Acquire the instance lock in .beads/.bv.lock (Go instance.NewLock, bv-vrvn).
/// Returns PID of another live instance if one holds the lock.
/// Best-effort cross-platform liveness check for a PID.
/// Unix: `kill -0 <pid>` (signal 0 — no-op, only checks permission/existence).
/// Windows: `kill` doesn't exist as a binary, so shell out to `tasklist` and
/// check whether the PID shows up in its filtered output.
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/NH", "/FO", "CSV", "/FI", &format!("PID eq {pid}")])
        .output()
        .map(|out| {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn is_process_alive(_pid: u32) -> bool {
    // Unknown platform: assume dead so stale locks get reclaimed rather than
    // wedging the TUI forever.
    false
}

pub fn acquire_instance_lock(beads_dir: &std::path::Path) -> Option<u32> {
    let lock_path = beads_dir.join(".bv.lock");
    let my_pid = std::process::id();
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(mut f) => {
            use std::io::Write;
            let _ = writeln!(f, "{{\"pid\":{my_pid}}}");
            None // we are the first instance
        }
        Err(_) => {
            // Lock exists — read holder PID and check if alive.
            let holder = std::fs::read_to_string(&lock_path).ok().and_then(|s| {
                s.trim()
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .split(':')
                    .nth(1)
                    .and_then(|p| p.trim().parse::<u32>().ok())
            });
            // If we already hold the lock (re-entrant acquire, or a stale write
            // from a previous run of this same process), it's trivially "alive".
            let alive = holder
                .map(|pid| pid == my_pid || is_process_alive(pid))
                .unwrap_or(false);
            if alive {
                holder
            } else {
                // Stale lock — take over.
                if let Ok(mut f) = std::fs::File::create(&lock_path) {
                    use std::io::Write;
                    let _ = writeln!(f, "{{\"pid\":{my_pid}}}");
                }
                None
            }
        }
    }
}

/// Remove the instance lock if we hold it.
pub fn release_instance_lock(beads_dir: &std::path::Path) {
    let lock_path = beads_dir.join(".bv.lock");
    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        let content = content.trim();
        let mine = content
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split(':')
            .nth(1)
            .and_then(|p| p.trim().parse::<u32>().ok())
            == Some(std::process::id());
        if mine {
            let _ = std::fs::remove_file(&lock_path);
        }
    }
}

impl App {
    pub fn new(issues: Vec<bv_core::model::Issue>) -> Self {
        let issue_map: std::collections::HashMap<String, bv_core::model::Issue> =
            issues.iter().map(|i| (i.id.clone(), i.clone())).collect();
        // Compute proactive alerts (Go computeAlerts): cycles-driven drift
        let g_alerts = bv_analysis::build_graph(&issues);
        let has_cycle = bv_graph_core::algorithms::cycles::has_cycles(&g_alerts);
        let mut alerts: Vec<bv_analysis::drift::Alert> = Vec::new();
        if has_cycle {
            alerts.push(bv_analysis::drift::Alert {
                alert_type: bv_analysis::drift::AlertType::NewCycle,
                severity: bv_analysis::drift::Severity::Critical,
                message: "Dependency cycle detected in the issue graph".into(),
                baseline_val: None,
                current_val: None,
                delta: None,
            });
        }
        // Staleness check: open issues untouched for a long time (Go drift engine check)
        let now = jiff::Timestamp::now();
        for i in &issues {
            if matches!(
                i.status,
                bv_core::model::Status::Open
                    | bv_core::model::Status::InProgress
                    | bv_core::model::Status::Blocked
            ) {
                if let Some(updated) = &i.updated_at {
                    if let Ok(t) = updated.parse::<jiff::Timestamp>() {
                        let age = now.since(t).map(|d| d.get_days()).unwrap_or(0);
                        if age > 30 {
                            alerts.push(bv_analysis::drift::Alert {
                                alert_type: bv_analysis::drift::AlertType::BlockedIncrease,
                                severity: bv_analysis::drift::Severity::Warning,
                                message: format!(
                                    "{} has been stale for {age} days ({:?})",
                                    i.id, i.status
                                ),
                                baseline_val: None,
                                current_val: Some(age as f64),
                                delta: None,
                            });
                        }
                    }
                }
            }
        }
        let a_crit = alerts
            .iter()
            .filter(|a| a.severity == bv_analysis::drift::Severity::Critical)
            .count();
        let a_warn = alerts
            .iter()
            .filter(|a| a.severity == bv_analysis::drift::Severity::Warning)
            .count();
        let alerts_total = alerts.len();
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
            issue_map,
            alerts_critical: a_crit,
            alerts_warning: a_warn,
            alerts_total,
            alerts,
            tree_collapsed: std::collections::HashSet::new(),
            alerts_cursor: 0,
            flow: None,
            flow_cursor: 0,
            attention_labels: Vec::new(),
            attention_cursor: 0,
            graph_cursor: 0,
            graph_scroll: 0,
            graph_data: None,
            history: None,
            label_picker: None,
            sprint: None,
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
            show_help: false,
            label_filter: None,
            detail_scroll: 0,
            graph_metrics: None,
            loaded_at: std::time::Instant::now(),
            instance_pid: None,
            dataset_warning: dataset_warning_for(issues.len()),
            session_count: 0,
            update_tag: None,
            workspace_repos: None,
            active_repo: None,
            cass_available: cass_installed(),
            cass_cache: std::collections::HashMap::new(),
        };
        // Pre-compute flow-matrix and attention data for the TUI views
        // (backed by the same data the robot-label-flow / robot-label-attention
        // commands use, so there's no separate code path — just computed once
        // at startup instead of on demand).
        let cfg = bv_analysis::label_health::LabelHealthConfig::default();
        let now = jiff::Timestamp::now();
        let flow = bv_analysis::label_health::compute_cross_label_flow(&issues, &cfg);
        app.flow = Some(flow);
        let attention =
            bv_analysis::label_health::compute_label_attention_scores(&issues, &cfg, now);
        app.attention_labels = attention.labels;
        // Build graph data for the Graph view (blocker/dependent maps).
        app.graph_data = Some(crate::views::graph::GraphData::build(
            app.issue_map.values().cloned().collect(),
            app.graph_metrics.clone(),
        ));
        // Initialize history view (empty for now — populated when user presses t).
        app.history = Some(crate::views::history::HistoryState::build_from_beads(
            vec![],
        ));
        app.apply_filter();
        app
    }
    pub fn apply_filter(&mut self) {
        let label_filter = self.label_filter.clone();
        let active_repo = self.active_repo.clone();
        self.filtered_indices = (0..self.rows.len())
            .filter(|&i| {
                let r = &self.rows[i];
                let mode_ok = match self.filter_mode {
                    FilterMode::All => true,
                    FilterMode::Open => matches!(r.status, Status::Open | Status::InProgress),
                    FilterMode::Closed => r.status.is_closed(),
                    FilterMode::Ready => matches!(r.status, Status::Open),
                };
                mode_ok
                    && label_filter
                        .as_ref()
                        .is_none_or(|label| r.labels.iter().any(|l| l == label))
                    && active_repo.as_ref().is_none_or(|repo| {
                        self.issue_map
                            .get(&r.id)
                            .map(|i| &i.source_repo == repo)
                            .unwrap_or(false)
                    })
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
    /// Update cass session count for the selected bead (Go getCassSessionCount).
    fn update_session_count(&mut self) {
        self.session_count = 0;
        if !self.cass_available {
            return;
        }
        let Some(row) = self.selected() else { return };
        let id = row.id.clone();
        if let Some(n) = self.cass_cache.get(&id) {
            self.session_count = *n;
            return;
        }
        if let Ok(out) = std::process::Command::new("cass")
            .args(["search", &id, "--robot", "--limit", "10"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        {
            let count = serde_json::from_slice::<serde_json::Value>(&out.stdout)
                .ok()
                .and_then(|v| v.get("results").and_then(|r| r.as_array()).map(|a| a.len()))
                .unwrap_or(0);
            self.cass_cache.insert(id, count);
            self.session_count = count;
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
        if self.show_help {
            self.show_help = false;
            return true;
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
                } else if self.current_view == ViewMode::FlowMatrix {
                    let max = self.flow.as_ref().map(|f| f.labels.len()).unwrap_or(0);
                    if self.flow_cursor + 1 < max {
                        self.flow_cursor += 1;
                    }
                } else if self.current_view == ViewMode::Attention {
                    if self.attention_cursor + 1 < self.attention_labels.len() {
                        self.attention_cursor += 1;
                    }
                } else if self.current_view == ViewMode::Graph {
                    let max = self
                        .graph_data
                        .as_ref()
                        .map(|g| g.sorted_ids.len())
                        .unwrap_or(0);
                    if max > 0 && self.graph_cursor + 1 < max {
                        self.graph_cursor += 1;
                        // Auto-scroll the node list panel
                        let visible = self.height.saturating_sub(4) as usize;
                        if self.graph_cursor >= self.graph_scroll + visible {
                            self.graph_scroll = self.graph_cursor.saturating_sub(visible - 1);
                        }
                    } else if self.current_view == ViewMode::TimeTravel {
                        if let Some(ref mut h) = self.history {
                            h.move_bead_down();
                        }
                    }
                } else if self.current_view == ViewMode::Alerts {
                    if self.alerts_cursor + 1 < self.alerts.len() {
                        self.alerts_cursor += 1;
                    }
                } else if self.cursor + 1 < self.filtered_indices.len() {
                    self.cursor += 1;
                    self.update_session_count();
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus_detail {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                } else if self.current_view == ViewMode::FlowMatrix {
                    self.flow_cursor = self.flow_cursor.saturating_sub(1);
                } else if self.current_view == ViewMode::Attention {
                    self.attention_cursor = self.attention_cursor.saturating_sub(1);
                } else if self.current_view == ViewMode::Graph {
                    self.graph_cursor = self.graph_cursor.saturating_sub(1);
                    // Auto-scroll up if cursor goes above visible area
                    if self.graph_cursor < self.graph_scroll {
                        self.graph_scroll = self.graph_cursor;
                    }
                } else if self.current_view == ViewMode::Alerts {
                    self.alerts_cursor = self.alerts_cursor.saturating_sub(1);
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                    self.update_session_count();
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
            KeyCode::Char('f') => {
                self.current_view = if self.current_view == ViewMode::FlowMatrix {
                    ViewMode::List
                } else {
                    ViewMode::FlowMatrix
                };
                true
            }
            KeyCode::Char('A') => {
                self.current_view = if self.current_view == ViewMode::Attention {
                    ViewMode::List
                } else {
                    ViewMode::Attention
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
            KeyCode::Char('G') => {
                self.current_view = if self.current_view == ViewMode::Graph {
                    ViewMode::List
                } else {
                    // Build graph data when entering graph view
                    if self.graph_data.is_none() {
                        let issues: Vec<bv_core::model::Issue> =
                            self.issue_map.values().cloned().collect();
                        let metrics = self.graph_metrics.clone();
                        self.graph_data =
                            Some(crate::views::graph::GraphData::build(issues, metrics));
                    }
                    ViewMode::Graph
                };
                true
            }
            KeyCode::Char('P') => {
                self.current_view = if self.current_view == ViewMode::Sprint {
                    ViewMode::List
                } else {
                    ViewMode::Sprint
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
            KeyCode::Char('x') => {
                self.export_markdown();
                true
            }
            KeyCode::Char('C') => {
                self.copy_issue_to_clipboard();
                true
            }
            KeyCode::Char('O') => {
                self.open_in_editor();
                true
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                true
            }
            KeyCode::Char('S') => {
                self.sort_mode = SortMode::Priority;
                self.apply_filter();
                self.status_msg = "Sorted by triage score".to_string();
                true
            }
            KeyCode::Char('L') => {
                self.cycle_label_filter();
                true
            }
            KeyCode::Char('w') if self.workspace_repos.is_some() => {
                self.cycle_repo_filter();
                true
            }
            _ => false,
        }
    }

    /// Cycle active repo filter in workspace mode (Go repo picker, simplified).
    fn cycle_repo_filter(&mut self) {
        let Some(repos) = &self.workspace_repos else {
            return;
        };
        if repos.is_empty() {
            return;
        }
        self.active_repo = match &self.active_repo {
            None => Some(repos[0].clone()),
            Some(current) => match repos.iter().position(|r| r == current) {
                Some(i) if i + 1 < repos.len() => Some(repos[i + 1].clone()),
                _ => None,
            },
        };
        self.status_msg = match &self.active_repo {
            Some(r) => format!("Repos: {r}"),
            None => "Repos: all".to_string(),
        };
        self.apply_filter();
    }

    /// Handle a Ctrl-modified key event; returns true if consumed.
    pub fn handle_ctrl_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.reload_from_disk();
                true
            }
            _ => false,
        }
    }

    /// Export all issues to beads_report_<project>_<date>.md (Go exportToMarkdown).
    fn export_markdown(&mut self) {
        let issues: Vec<bv_core::model::Issue> = self.issue_map.values().cloned().collect();

        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "beads".to_string());
        let sanitized: String = project
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        let filename = format!("beads_report_{sanitized}_{today}.md");

        let md = bv_export::mermaid::generate_markdown(&issues, "Beads Report");
        match std::fs::write(&filename, md) {
            Ok(_) => {
                self.status_msg = format!("Exported {} issues to {filename}", issues.len());
            }
            Err(e) => {
                self.status_msg = format!("Export failed: {e}");
            }
        }
    }

    /// Copy selected issue details to clipboard (Go copyIssueToClipboard).
    fn copy_issue_to_clipboard(&mut self) {
        let Some(row) = self.selected() else { return };
        let text = format!(
            "{}: {}\nStatus: {} | Priority: P{} | Type: {}\n{}",
            row.id,
            row.title,
            row.status.as_str(),
            row.priority,
            row.issue_type,
            row.description,
        );
        let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
            ("pbcopy", &[])
        } else if std::env::var("WAYLAND_DISPLAY").is_ok() {
            ("wl-copy", &[])
        } else {
            ("xclip", &["-selection", "clipboard"])
        };
        match std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
                self.status_msg = format!("Copied {} to clipboard", row.id);
            }
            Err(e) => {
                self.status_msg = format!("Clipboard failed: {e}");
            }
        }
    }

    /// Open selected issue in $EDITOR (Go "O" edit).
    fn open_in_editor(&mut self) {
        let Some(row) = self.selected() else { return };
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        let path = std::env::temp_dir().join(format!("bead_{}.md", row.id));
        let body = format!(
            "# {}: {}\n\n## Description\n\n{}\n\n## Notes\n\n{}\n",
            row.id, row.title, row.description, row.notes
        );
        if std::fs::write(&path, &body).is_ok() {
            let _ = std::process::Command::new(&editor).arg(&path).status();
            self.status_msg = format!("Opened {} in {editor}", row.id);
        }
    }

    /// Cycle label filter through available labels (Go label picker, simplified).
    fn cycle_label_filter(&mut self) {
        let mut labels: Vec<String> = self
            .rows
            .iter()
            .flat_map(|r| r.labels.iter().cloned())
            .collect();
        labels.sort();
        labels.dedup();
        if labels.is_empty() {
            self.status_msg = "No labels".to_string();
            return;
        }
        match &self.label_filter {
            None => {
                self.label_filter = Some(labels[0].clone());
                self.status_msg = format!("Filter: label={}", labels[0]);
            }
            Some(current) => match labels.iter().position(|l| l == current) {
                Some(i) if i + 1 < labels.len() => {
                    self.label_filter = Some(labels[i + 1].clone());
                    self.status_msg = format!("Filter: label={}", labels[i + 1]);
                }
                _ => {
                    self.label_filter = None;
                    self.status_msg = "Filter: all labels".to_string();
                }
            },
        }
        self.apply_filter();
    }

    /// Reload issues from disk (Go Ctrl+R refresh).
    pub fn reload_from_disk(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_default();
        match bv_core::discovery::load_issues_from_repo(&cwd) {
            Ok((issues, _)) => {
                let fresh = App::new(issues);
                *self = fresh;
                self.status_msg = "Refreshed".to_string();
            }
            Err(e) => {
                self.status_msg = format!("Refresh failed: {e}");
            }
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
            let issues: Vec<bv_core::model::Issue> = app.issue_map.values().cloned().collect();
            let nodes = crate::views::tree::build_tree_nodes(&issues, &app.tree_collapsed);
            let lines = crate::views::tree::render_tree_lines(&nodes);
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
            crate::views::alerts::render_alerts(f, &app.alerts, app.alerts_cursor, f.area());
            render_status_bar(f, app);
            return;
        }
        ViewMode::FlowMatrix => {
            if let Some(ref flow) = app.flow {
                crate::views::flow_matrix::render_flow_matrix(f, flow, app.flow_cursor, f.area());
            } else {
                let msg = ratatui::widgets::Paragraph::new("No flow data available");
                f.render_widget(msg, f.area());
            }
            render_status_bar(f, app);
            return;
        }
        ViewMode::Attention => {
            crate::views::attention::render_attention(
                f,
                &app.attention_labels,
                app.attention_cursor,
                f.area(),
            );
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
        ViewMode::Graph => {
            if let Some(ref graph) = app.graph_data {
                crate::views::graph::render_graph(
                    f,
                    graph,
                    app.graph_cursor,
                    app.graph_scroll,
                    f.area(),
                );
            } else {
                // Build graph data on-the-fly if not cached
                let issues: Vec<bv_core::model::Issue> = app.issue_map.values().cloned().collect();
                let graph =
                    crate::views::graph::GraphData::build(issues, app.graph_metrics.clone());
                crate::views::graph::render_graph(
                    f,
                    &graph,
                    app.graph_cursor,
                    app.graph_scroll,
                    f.area(),
                );
            }
            render_status_bar(f, app);
            return;
        }
        ViewMode::Sprint => {
            if let Some(ref sprint_state) = app.sprint {
                let issues: Vec<bv_core::model::Issue> = app.issue_map.values().cloned().collect();
                crate::views::sprint::render_sprint(f, sprint_state, &issues, f.area());
            } else {
                let msg = ratatui::widgets::Paragraph::new("No sprint data available");
                f.render_widget(msg, f.area());
            }
            render_status_bar(f, app);
            return;
        }
        ViewMode::TimeTravel => {
            if let Some(ref history) = app.history {
                crate::views::history::render_history(f, history, f.area());
            } else {
                let msg = ratatui::widgets::Paragraph::new("No history data available");
                f.render_widget(msg, f.area());
            }
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

    // Help overlay (Go "?" help)
    if app.show_help {
        let help_lines = vec![
            Line::from(Span::styled(
                " Keyboard Shortcuts ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  j/k, \u{2191}\u{2193}     Navigate list / scroll detail"),
            Line::from("  \u{23ce}          Toggle detail pane"),
            Line::from("  tab         Focus detail pane"),
            Line::from("  esc         Back / quit"),
            Line::from("  /           Search"),
            Line::from("  a/o/c/r     Filter: all/open/closed/ready"),
            Line::from("  s           Cycle sort mode"),
            Line::from("  S           Triage sort (priority)"),
            Line::from("  L           Cycle label filter"),
            Line::from("  b           Toggle board view"),
            Line::from("  i           Toggle insights view"),
            Line::from("  t           Toggle time-travel view"),
            Line::from("  G           Toggle graph view"),
            Line::from("  f           Toggle flow-matrix view"),
            Line::from("  A           Toggle attention view"),
            Line::from("  E           Toggle tree view"),
            Line::from("  !           Toggle alerts view"),
            Line::from("  `           Toggle tutorial"),
            Line::from("  ;           Toggle sidebar"),
            Line::from("  x           Export markdown report"),
            Line::from("  C           Copy issue to clipboard"),
            Line::from("  O           Open issue in $EDITOR"),
            Line::from("  Ctrl+R      Refresh from disk"),
            Line::from("  q           Quit"),
            Line::from(""),
            Line::from(Span::styled(
                " Press any key to close ",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let w = 50.min(app.width.saturating_sub(4));
        let h = help_lines.len() as u16 + 2;
        let x = (app.width.saturating_sub(w)) / 2;
        let y = (app.height.saturating_sub(h)) / 2;
        let popup = ratatui::layout::Rect {
            x,
            y,
            width: w,
            height: h,
        };
        f.render_widget(ratatui::widgets::Clear, popup);
        f.render_widget(
            ratatui::widgets::Paragraph::new(help_lines).block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
            popup,
        );
    }
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

            // Use full issue data from issue_map for detail rendering
            if let Some(full_issue) = app.issue_map.get(&row.id) {
                crate::detail::build_detail_lines(
                    full_issue,
                    graph_scores.as_ref(),
                    Some(&app.issue_map),
                )
            } else {
                vec![Line::from(""), Line::from("Issue not found in map")]
            }
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
    let area = ratatui::layout::Rect {
        x: 0,
        y: f.area().height - 1,
        width: f.area().width,
        height: 1,
    };

    // Search mode
    if app.searching {
        let bar = Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::styled(&app.search_query, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]));
        f.render_widget(bar, area);
        return;
    }

    let mut spans: Vec<Span> = Vec::new();

    // Filter badge (colored bg like Go)
    let (filter_icon, filter_txt) = match app.filter_mode {
        FilterMode::All => ("📋", "ALL"),
        FilterMode::Open => ("📂", "OPEN"),
        FilterMode::Closed => ("✅", "CLOSED"),
        FilterMode::Ready => ("🚀", "READY"),
    };
    spans.push(Span::styled(
        format!(" {filter_icon} {filter_txt} "),
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    ));

    // Label filter badge (Go searchBadge area)
    if let Some(label) = &app.label_filter {
        spans.push(Span::styled(
            format!(" #{label} "),
            Style::default().bg(Color::DarkGray).fg(Color::Cyan),
        ));
    }

    // Sort badge (only when not default)
    if app.sort_mode != SortMode::Default {
        spans.push(Span::styled(
            format!(" ↕ {} ", app.sort_mode.label()),
            Style::default().bg(Color::DarkGray).fg(Color::Cyan),
        ));
    }

    // Stats section with colored indicators
    let open_count = app
        .rows
        .iter()
        .filter(|r| matches!(r.status, Status::Open))
        .count();
    let ready_count = app.filtered_indices.len();
    let blocked_count = app
        .rows
        .iter()
        .filter(|r| matches!(r.status, Status::Blocked))
        .count();
    let closed_count = app.rows.iter().filter(|r| r.status.is_closed()).count();

    spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        format!("○{open_count} "),
        Style::default().fg(Color::Green),
    ));
    spans.push(Span::styled(
        format!("◉{ready_count} "),
        Style::default().fg(Color::Cyan),
    ));
    spans.push(Span::styled(
        format!("◈{blocked_count} "),
        Style::default().fg(Color::Yellow),
    ));
    spans.push(Span::styled(
        format!("●{closed_count}"),
        Style::default().fg(Color::DarkGray),
    ));

    // Alerts badge (Go alertsSection, bv-168)
    if app.alerts_total > 0 {
        let (bg, fg) = if app.alerts_critical > 0 {
            (Color::Red, Color::White)
        } else {
            (Color::DarkGray, Color::Yellow)
        };
        spans.push(Span::styled(
            format!(" \u{26a0} {} alerts (!) ", app.alerts_total),
            Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
        ));
    }

    // Freshness badge (Go bv-h305: warn 30s, stale 2min since snapshot)
    {
        let elapsed = app.loaded_at.elapsed();
        let stale_s = std::env::var("BV_FRESHNESS_STALE_S")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120);
        let warn_s = std::env::var("BV_FRESHNESS_WARN_S")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);
        let secs = elapsed.as_secs();
        let fmt_age = |s: u64| {
            if s < 60 {
                "<1m ago".to_string()
            } else if s < 3600 {
                format!("{}m ago", s / 60)
            } else {
                format!("{}h ago", s / 3600)
            }
        };
        if secs >= stale_s {
            spans.push(Span::styled(
                format!(" \u{26a0} STALE: {} ", fmt_age(secs)),
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if secs >= warn_s {
            spans.push(Span::styled(
                format!(" \u{26a0} {} ", fmt_age(secs)),
                Style::default().bg(Color::DarkGray).fg(Color::Yellow),
            ));
        }
    }

    // Phase 2 metrics badge (Go bv-tspo: ◌ metrics... until ready)
    if app.graph_metrics.is_none() {
        spans.push(Span::styled(
            " \u{25d0} metrics... ",
            Style::default().bg(Color::DarkGray).fg(Color::Cyan),
        ));
    }

    // Instance warning (Go bv-vrvn: ⚠ PID of other live instance)
    if let Some(pid) = app.instance_pid {
        spans.push(Span::styled(
            format!(" \u{26a0} PID {pid} "),
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Session indicator (Go bv-y836: cass sessions for selected bead)
    if app.session_count > 0 {
        let count_str = if app.session_count > 9 {
            "9+".to_string()
        } else {
            app.session_count.to_string()
        };
        spans.push(Span::styled(
            format!(" \u{1f4bc} {count_str} sessions "),
            Style::default().bg(Color::DarkGray).fg(Color::Cyan),
        ));
    }

    // Update badge (Go: Update <tag>)
    if let Some(tag) = &app.update_tag {
        spans.push(Span::styled(
            format!(" \u{2b06} Update {tag} "),
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Workspace repo filter badge (Go repoFilterSection 🗂)
    if app.workspace_repos.is_some() {
        let label = match &app.active_repo {
            Some(r) => r.clone(),
            None => {
                let repos = app.workspace_repos.as_deref().unwrap_or_default();
                let shown: Vec<&str> = repos.iter().take(3).map(|s| s.as_str()).collect();
                if repos.len() > 3 {
                    format!("{},+{}", shown.join(","), repos.len() - 3)
                } else {
                    shown.join(",")
                }
            }
        };
        spans.push(Span::styled(
            format!(" \u{1f5c2} {label} "),
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Large dataset warning (Go bv-9thm)
    if let Some(warn) = &app.dataset_warning {
        let huge = warn.contains("huge");
        spans.push(Span::styled(
            format!(" {warn} "),
            Style::default()
                .bg(if huge { Color::Red } else { Color::DarkGray })
                .fg(if huge { Color::White } else { Color::Yellow })
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Label hint (Go labelHint: "L:labels * h:detail")
    if app.current_view != ViewMode::Board {
        spans.push(Span::styled(
            " \u{2502} ",
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            "L:labels * h:detail",
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Keyboard hints (context-aware, matching Go footer)
    spans.push(Span::styled(
        " \u{2502} ",
        Style::default().fg(Color::DarkGray),
    ));

    let key_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let sep_style = Style::default().fg(Color::DarkGray);
    let mut hints: Vec<Span> = Vec::new();
    let push_hint = |hints: &mut Vec<Span>, key: &str, desc: &str| {
        if !hints.is_empty() {
            hints.push(Span::styled(" \u{2502} ", sep_style));
        }
        hints.push(Span::styled(key.to_string(), key_style));
        hints.push(Span::raw(format!(" {desc}")));
    };

    if app.focus_detail {
        push_hint(&mut hints, "esc", "back");
        push_hint(&mut hints, "C", "copy");
        push_hint(&mut hints, "O", "edit");
        push_hint(&mut hints, "Ctrl+R", "refresh");
        push_hint(&mut hints, "?", "help");
    } else if app.show_detail {
        push_hint(&mut hints, "tab", "focus");
        push_hint(&mut hints, "C", "copy");
        push_hint(&mut hints, "x", "export");
        push_hint(&mut hints, "Ctrl+R", "refresh");
        push_hint(&mut hints, "?", "help");
    } else if app.current_view == ViewMode::Board {
        push_hint(&mut hints, "h/l", "col");
        push_hint(&mut hints, "j/k", "move");
        push_hint(&mut hints, "b", "list");
        push_hint(&mut hints, "?", "help");
    } else {
        push_hint(&mut hints, "\u{23ce}", "details");
        push_hint(&mut hints, "t", "diff");
        push_hint(&mut hints, "S", "triage");
        push_hint(&mut hints, "l", "labels");
        push_hint(&mut hints, "Ctrl+R", "refresh");
        push_hint(&mut hints, "?", "help");
    }
    spans.extend(hints);

    // Count badge (right side, padded like Go countBadge)
    let count_text = format!(" {} issues ", app.filtered_indices.len());
    let count_width = count_text.len() as u16;
    let used: u16 = spans.iter().map(|s| s.width() as u16).sum::<u16>() + count_width;
    let filler_width = area.width.saturating_sub(used);
    if filler_width > 0 {
        spans.push(Span::raw(" ".repeat(filler_width as usize)));
    }
    spans.push(Span::styled(
        count_text,
        Style::default().fg(Color::DarkGray),
    ));

    let bar = Paragraph::new(Line::from(spans));
    f.render_widget(bar, area);
}

/// Run the TUI event loop. Returns when user quits.
pub fn run_tui(app: &mut App) -> io::Result<()> {
    // Instance lock (Go bv-vrvn)
    let beads_dir = std::env::current_dir()
        .ok()
        .map(|d| d.join(".beads"))
        .unwrap_or_default();
    app.instance_pid = acquire_instance_lock(&beads_dir);

    // Update check (Go updater): background thread, gated by BV_NO_UPDATE_CHECK
    let (update_tx, update_rx) = std::sync::mpsc::channel::<String>();
    if std::env::var("BV_NO_UPDATE_CHECK").is_err() {
        std::thread::spawn(move || {
            let output = std::process::Command::new("curl")
                .args([
                    "-sf",
                    "-m",
                    "5",
                    "-H",
                    "User-Agent: OpenAI File Downloader, XaiImageApiFetch/1.0",
                    "https://api.github.com/repos/Dicklesworthstone/beads_viewer/releases/latest",
                ])
                .output();
            if let Ok(out) = output {
                if let Ok(body) = String::from_utf8(out.stdout) {
                    if let Some(tag) = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v.get("tag_name").and_then(|t| t.as_str().map(String::from)))
                    {
                        let _ = update_tx.send(tag);
                    }
                }
            }
        });
    }

    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let terminal = ratatui::Terminal::new(backend)?;

    let result = tui_event_loop(terminal, app, &update_rx);

    release_instance_lock(&beads_dir);
    result
}

fn tui_event_loop(
    mut terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
    update_rx: &std::sync::mpsc::Receiver<String>,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| render(f, app))?;
        if app.quit_requested {
            break;
        }
        // Drain update-check channel (non-blocking)
        if let Ok(tag) = update_rx.try_recv() {
            let current = env!("CARGO_PKG_VERSION");
            let tag_clean = tag.trim_start_matches('v');
            if tag_clean != current {
                app.update_tag = Some(tag);
            }
        }
        // Poll events with timeout so freshness badge stays live
        if event::poll(std::time::Duration::from_millis(500))? {
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
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                        {
                            app.handle_ctrl_key(key.code);
                        } else {
                            app.handle_key(key.code);
                        }
                    }
                }
                CEvent::Mouse(mouse) => {
                    app.handle_mouse(mouse);
                }
                _ => {}
            }
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
pub mod context_help;
pub mod detail;
pub mod keybindings;
pub mod markdown;
pub mod shortcuts_sidebar;
pub mod theme;
pub mod tutorial;
pub mod update_modal;
pub mod views;
pub mod worker;

#[cfg(test)]
mod footer_state_tests {
    use super::*;

    #[test]
    fn dataset_warning_thresholds_match_go() {
        assert_eq!(dataset_warning_for(999), None);
        assert_eq!(dataset_warning_for(4_999), None); // medium tier: no warning
        assert!(dataset_warning_for(5_000).unwrap().contains("large"));
        assert!(dataset_warning_for(20_000).unwrap().contains("huge"));
        assert_eq!(
            dataset_warning_for(24_000).unwrap(),
            "\u{26a0} huge 24k issues"
        );
    }

    #[test]
    fn instance_lock_takeover_on_stale() {
        let dir = std::env::temp_dir().join(format!("bvr_lock_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join(".bv.lock");
        // Stale holder: PID that (almost certainly) doesn't exist
        std::fs::write(&lock, "{\"pid\":999999999}").unwrap();
        let holder = acquire_instance_lock(&dir);
        assert_eq!(holder, None, "stale lock should be taken over");
        // Now we hold it — second acquirer sees our PID
        let second = acquire_instance_lock(&dir);
        assert_eq!(second, Some(std::process::id()));
        release_instance_lock(&dir);
        assert!(!lock.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
