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

/// Drilldown overlay state: filtered issue list for one label (Go `d`
/// drilldown overlay — filterable, `Enter` jumps to List filtered by it).
pub struct LabelDrilldown {
    pub label: String,
    pub filter: String,
    pub cursor: usize,
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
    /// Current active view (list is default, toggled by b/E/G/i/etc).
    pub current_view: ViewMode,
    /// Search mode active (/ pressed).
    pub searching: bool,
    /// Current search query.
    pub search_query: String,
    /// Semantic search mode active (Ctrl+S pressed). Ranks by
    /// `bv_search` cosine similarity over title+description — the same
    /// text-mode engine `--robot-search` uses.
    pub semantic_searching: bool,
    /// Current semantic query.
    pub semantic_query: String,
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
    /// Recipe picker state (built-in recipes only — see `default_recipes`).
    pub recipe_picker: Option<crate::views::pickers::RecipePicker>,
    /// Workspace repo picker state (only meaningful when `workspace_repos`
    /// is `Some`).
    pub repo_picker: Option<crate::views::pickers::RepoPicker>,
    /// True once `history` has been populated with real correlation data
    /// (lazy-loaded on first `h` press — Go bv-h305-style deferred cost).
    pub history_loaded: bool,
    /// Revision-input buffer for Time-Travel (Go `focusTimeTravelInput`).
    /// `Some` = prompt open and capturing keys, `None` = closed.
    pub time_travel_prompt: Option<String>,
    /// Last computed revision diff (Go `SnapshotDiff`), via
    /// `bv_analysis::diff::diff_issues` over `GitLoader::load_at` output
    /// (the same plumbing `--robot-diff` uses — see plan Q4).
    pub time_travel_result: Option<bv_analysis::diff::DiffResult>,
    /// Error from the last Time-Travel diff attempt (bad ref, not a git
    /// repo). Shown in the view; mirrors the robot command's message.
    pub time_travel_error: Option<String>,
    /// Cursor over the flattened Time-Travel diff entry list
    /// (added, then removed, then changed).
    pub time_travel_cursor: usize,
    /// Per-label health dashboard (Go `focusLabelDashboard`), lazy-loaded
    /// on first `[` press like History (deferred analysis cost).
    pub label_health: Option<bv_analysis::label_health::LabelAnalysisResult>,
    pub label_health_loaded: bool,
    pub label_dashboard_cursor: usize,
    /// Detail modal for the cursor label (Go `h` health-detail modal).
    pub show_label_detail: bool,
    /// Drilldown overlay (Go `d`): filtered issue list for one label.
    pub label_drilldown: Option<LabelDrilldown>,
    /// Update-available modal visibility (Go `U` key / auto-show-once).
    pub show_update_modal: bool,
    /// True once the update modal has been auto-shown once this session,
    /// so it doesn't reappear every frame after the user dismisses it.
    pub update_modal_auto_shown: bool,
    /// Priority hints visibility (Go `p` key — `pkg/ui/model.go`
    /// `showPriorityHints`). Renders per-row up/down suggestion arrows in
    /// the list when true.
    pub show_priority_hints: bool,
    /// Lazily computed suggested-priority map (issue id -> suggested P),
    /// built once from `bv_analysis::impact::compute_impact_scores` (the
    /// same engine `--robot-priority` uses — Go `ComputeImpactScores` +
    /// `scoreToPriority`). `None` until first computed. Only entries where
    /// suggested != current are kept (that's the misalignment set).
    pub priority_hints: Option<std::collections::BTreeMap<String, i32>>,
    /// Live-reload watch target: the resolved `.beads/*.jsonl` path (Go
    /// watches this via `fsnotify`; bvr polls its mtime on the existing
    /// ~500ms terminal-event-poll tick instead of adding a background
    /// `notify` watcher thread — same user-visible effect, fewer moving
    /// parts. `None` if discovery failed (watching is then a no-op, not a
    /// startup error).
    pub watched_path: Option<std::path::PathBuf>,
    pub watched_mtime: Option<std::time::SystemTime>,
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
    /// Actionable items state (lazy-built on first `F`, like History).
    pub actionable: Option<crate::actionable::ActionableState>,
    /// Tutorial state.
    pub tutorial: Option<crate::tutorial::TutorialState>,
    /// Board swimlane grouping (Go `s` cycle: Status → Priority → Type).
    pub board_mode: crate::views::board::SwimlaneMode,
    /// Board selected column (Go `h`/`l` move, `1`-`9` jump).
    pub board_column: usize,
    /// Agent-prompt modal visibility (Go auto-shows on AGENTS.md detection;
    /// `g` reopens manually).
    pub show_agent_prompts: bool,
    /// Cursor inside the agent-prompt modal.
    pub agent_prompt_cursor: usize,
    /// Velocity-comparison overlay inside Sprint view (Go `v` sub-toggle).
    pub show_velocity: bool,
    /// Theme for consistent styling.
    pub theme: crate::theme::Theme,
    /// Key registry for help display.
    pub key_registry: crate::keybindings::KeyRegistry,
}

/// Which view is currently displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Board,
    Tree,
    Graph,
    /// Per-label health dashboard (Go `focusLabelDashboard`, `[` key).
    /// Badges computed by `bv_analysis::label_health` (same module backing
    /// Attention/FlowMatrix). See TUI_UX_PARITY_PLAN.md Phase C.
    LabelDashboard,
    FlowMatrix,
    Attention,
    Insights,
    Alerts,
    /// Bead↔commit correlation view (Go `pkg/ui/history.go`). Was
    /// previously aliased under `TimeTravel` — split out because Go treats
    /// History and Time-Travel as two distinct features with distinct keys
    /// (`h` vs `t`/`T`). See TUI_UX_PARITY_PLAN.md G1/G12.
    History,
    /// Revision-diff mode (Go `focusTimeTravelInput` → `SnapshotDiff`).
    /// `t` opens the revision prompt, `T` diffs instantly vs HEAD~5;
    /// Enter submits. See TUI_UX_PARITY_PLAN.md Phase B.
    TimeTravel,
    Sprint,
    Tutorial,
    Actionable,
}

/// A built-in recipe definition (name + description shown in the picker).
/// Matches Go's embedded `defaults/recipes.yaml` names/descriptions exactly;
/// user/project YAML recipe loading is not ported (see `open_recipe_picker`
/// doc comment).
pub struct RecipeDef {
    pub name: &'static str,
    pub description: &'static str,
}

/// The 6 recipes Go ships as embedded defaults (`recipe.Loader::loadBuiltin`).
pub fn default_recipe_defs() -> &'static [RecipeDef] {
    &[
        RecipeDef {
            name: "triage",
            description: "Open/blocked issues, sorted by priority",
        },
        RecipeDef {
            name: "release-cut",
            description: "Recently closed issues for changelog",
        },
        RecipeDef {
            name: "blocked-review",
            description: "All blocked items with blocker info",
        },
        RecipeDef {
            name: "dependency-risk",
            description: "High betweenness/PageRank items",
        },
        RecipeDef {
            name: "quick-wins",
            description: "Low-priority, no-dependency items",
        },
        RecipeDef {
            name: "stale",
            description: "Items not updated in 14+ days",
        },
    ]
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

/// Copy text to the system clipboard (Go copyIssueToClipboard).
/// Shared by issue-copy (`C`) and commit-SHA-copy (`y`, History view).
///
/// Primary path is `arboard` (cross-platform: Win32 on Windows, X11/Wayland
/// on Linux, pbcopy on macOS). When that fails — e.g. headless/SSH sessions
/// with no display server — fall back to shelling out to `pbcopy`/`wl-copy`/
/// `xclip`, preserving the pre-arboard behavior instead of hard-failing.
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.set_text(text.to_string()).is_ok() {
            return Ok(());
        }
    }
    shell_copy_fallback(text)
}

/// Legacy shell-out clipboard path, kept as a headless fallback for
/// `copy_to_clipboard` when `arboard` has no display to talk to.
fn shell_copy_fallback(text: &str) -> Result<(), String> {
    let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else if std::env::var("WAYLAND_DISPLAY").is_ok() {
        ("wl-copy", &[])
    } else {
        ("xclip", &["-selection", "clipboard"])
    };
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
        }
    }
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}
/// True when the repo being viewed has an AGENTS.md (Go auto-shows the
/// agent-prompt modal on detection). Checks the cwd and, when running from
/// inside `.beads/` discovery, the repo root above it.
fn detect_agents_md() -> bool {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("AGENTS.md").is_file() {
        return true;
    }
    bv_core::discovery::get_beads_dir(&cwd)
        .ok()
        .and_then(|dir| dir.parent().map(|p| p.join("AGENTS.md").is_file()))
        .unwrap_or(false)
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
                ..Default::default()
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
                                ..Default::default()
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
            label_health: None,
            label_health_loaded: false,
            label_dashboard_cursor: 0,
            show_label_detail: false,
            label_drilldown: None,
            graph_scroll: 0,
            graph_data: None,
            history: None,
            label_picker: None,
            recipe_picker: None,
            repo_picker: None,
            history_loaded: false,
            time_travel_prompt: None,
            time_travel_result: None,
            time_travel_error: None,
            time_travel_cursor: 0,
            show_update_modal: false,
            update_modal_auto_shown: false,
            show_priority_hints: false,
            priority_hints: None,
            watched_path: None,
            watched_mtime: None,
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
            semantic_searching: false,
            semantic_query: String::new(),
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
            actionable: None,
            tutorial: Some(crate::tutorial::TutorialState::new()),
            board_mode: crate::views::board::SwimlaneMode::Status,
            board_column: 0,
            // Auto-show happens in `tui_event_loop`, not here, so unit
            // tests (`App::new` directly) start with a clean slate.
            show_agent_prompts: false,
            agent_prompt_cursor: 0,
            show_velocity: false,
            theme: crate::theme::Theme::default(),
            key_registry: crate::keybindings::build_default_registry(),
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
        // Initialize history view (empty for now — populated when user presses h).
        app.history = Some(crate::views::history::HistoryState::build_from_beads(
            vec![],
        ));
        app.refresh_watch_target();
        app.apply_filter();
        app
    }

    /// (Re)discover the `.beads/*.jsonl` path and record its current mtime,
    /// for live-reload polling (Go `fsnotify`, `p` — see `check_for_reload`
    /// below and TUI_UX_PARITY_PLAN.md Phase F). Best-effort: if discovery
    /// fails (e.g. no `.beads` dir, or the active datasource is SQLite —
    /// out of scope here, matching Go's own `.beads/issues.jsonl`-specific
    /// watch target), watching is silently disabled rather than surfaced
    /// as a startup error.
    fn refresh_watch_target(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_default();
        self.watched_path = bv_core::discovery::get_beads_dir(&cwd)
            .ok()
            .and_then(|dir| bv_core::discovery::find_jsonl_path_with_warnings(&dir, |_| {}).ok())
            .flatten();
        self.watched_mtime = self
            .watched_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
    }

    /// Poll the watched file's mtime and reload if it changed externally.
    /// Called from the terminal event loop's existing ~500ms poll tick
    /// (`tui_event_loop`) — no background thread, no `notify` watcher
    /// dependency; same user-visible effect ("editing issues.jsonl updates
    /// the TUI without a manual keypress") with fewer moving parts. A no-op
    /// when `watched_path` is `None` (discovery failed or unsupported
    /// datasource). Returns true if a reload happened, for the caller to
    /// avoid double-work in the same tick.
    pub fn check_for_reload(&mut self) -> bool {
        let Some(path) = &self.watched_path else {
            return false;
        };
        let Some(current_mtime) = std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
        else {
            return false; // file briefly missing mid-write — try again next tick
        };
        if self.watched_mtime == Some(current_mtime) {
            return false;
        }
        self.reload_from_disk();
        true
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

    /// Fuzzy-ranked search (Go `/` fuzzy entry): nucleo-matcher scores each
    /// `id + title` haystack; matches sort by score desc, id asc tiebreak.
    /// Empty query restores the natural row order. Case-insensitive like
    /// the old substring check, so every previous contains-match still
    /// surfaces (possibly reordered by score).
    fn apply_search(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_indices = (0..self.rows.len()).collect();
        } else {
            // Lowercase the needle only: `Config::DEFAULT` already folds
            // haystack case, but folding must not be left to nucleo on the
            // needle side — an uppercase needle char that can only match via
            // folding (e.g. "Is" vs "issue") passes the prefilter yet trips
            // a `debug_assert!`-style reject in the optimal matcher
            // (nucleo-matcher 0.3.1 `fuzzy_optimal.rs`: "should have been
            // caught by prefilter"). Pre-folding keeps matching
            // case-insensitive without touching that path.
            let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
            let mut haystack_buf = Vec::new();
            let mut needle_buf = Vec::new();
            let folded = self.search_query.to_lowercase();
            let needle = nucleo_matcher::Utf32Str::new(&folded, &mut needle_buf);
            let mut scored: Vec<(u16, usize)> = Vec::new();
            for (i, r) in self.rows.iter().enumerate() {
                let haystack = format!("{} {}", r.id, r.title);
                haystack_buf.clear();
                let hay = nucleo_matcher::Utf32Str::new(&haystack, &mut haystack_buf);
                if let Some(score) = matcher.fuzzy_match(hay, needle) {
                    scored.push((score, i));
                }
            }
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| self.rows[a.1].id.cmp(&self.rows[b.1].id))
            });
            let idx: Vec<usize> = scored.into_iter().map(|(_, i)| i).collect();
            self.filtered_indices = idx;
        }
        self.cursor = 0;
    }
    /// Semantic search input (Ctrl+S mode): Esc clears and exits, Enter
    /// accepts (back to the standard filter order), typing re-ranks.
    fn handle_semantic_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                self.semantic_searching = false;
                self.semantic_query.clear();
                self.apply_filter();
                true
            }
            KeyCode::Enter => {
                self.semantic_searching = false;
                self.apply_filter();
                true
            }
            KeyCode::Backspace => {
                self.semantic_query.pop();
                self.apply_semantic();
                true
            }
            KeyCode::Char(c) => {
                self.semantic_query.push(c);
                self.apply_semantic();
                true
            }
            _ => true,
        }
    }

    /// Cosine-similarity ranking over title+description with the same
    /// hash-embedding text engine `--robot-search` uses
    /// (`bv_search::embedder::{hash_embed, cosine_similarity}`). Zero-score
    /// rows are hidden; ties break by id asc. Empty query restores order.
    fn apply_semantic(&mut self) {
        if self.semantic_query.is_empty() {
            self.filtered_indices = (0..self.rows.len()).collect();
        } else {
            let dim = bv_search::embedder::DEFAULT_DIM;
            let query_vec = bv_search::embedder::hash_embed(&self.semantic_query, dim);
            let mut scored: Vec<(i64, usize)> = Vec::new();
            for (i, r) in self.rows.iter().enumerate() {
                let text = format!("{} {}", r.title, r.description);
                let issue_vec = bv_search::embedder::hash_embed(&text, dim);
                let score = bv_search::embedder::cosine_similarity(&query_vec, &issue_vec);
                if score > 0.0 {
                    // Fixed-point rank key: deterministic across platforms.
                    scored.push(((score * 1_000_000.0) as i64, i));
                }
            }
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| self.rows[a.1].id.cmp(&self.rows[b.1].id))
            });
            let idx: Vec<usize> = scored.into_iter().map(|(_, i)| i).collect();
            self.filtered_indices = idx;
        }
        self.cursor = 0;
    }

    /// Handle mouse events (wheel scroll + click select).
    pub fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> bool {
        use crossterm::event::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if self.focus_detail {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                } else if self.cursor + 1 < self.filtered_indices.len() {
                    self.cursor += 1;
                }
                true
            }
            MouseEventKind::ScrollUp => {
                if self.focus_detail {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                }
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
        if self.semantic_searching {
            return self.handle_semantic_key(code);
        }
        if self.searching {
            return self.handle_search_key(code);
        }
        if self.show_help {
            self.show_help = false;
            return true;
        }
        if self.show_update_modal {
            // Go: "Press any key to dismiss".
            self.show_update_modal = false;
            return true;
        }
        if matches!(self.label_picker, Some(ref p) if p.visible) {
            return self.handle_label_picker_key(code);
        }
        if matches!(self.recipe_picker, Some(ref p) if p.visible) {
            return self.handle_recipe_picker_key(code);
        }
        if matches!(self.repo_picker, Some(ref p) if p.visible) {
            return self.handle_repo_picker_key(code);
        }
        if self.time_travel_prompt.is_some() {
            return self.handle_time_travel_prompt_key(code);
        }
        if self.label_drilldown.is_some() {
            return self.handle_label_drilldown_key(code);
        }
        if self.show_agent_prompts {
            return self.handle_agent_prompt_key(code);
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
                } else if self.show_detail {
                    self.show_detail = false;
                } else if self.label_drilldown.is_some() || self.show_label_detail {
                    self.label_drilldown = None;
                    self.show_label_detail = false;
                } else if self.current_view == ViewMode::Sprint && self.show_velocity {
                    self.show_velocity = false;
                } else if matches!(
                    self.current_view,
                    ViewMode::TimeTravel
                        | ViewMode::LabelDashboard
                        | ViewMode::Board
                        | ViewMode::Tree
                        | ViewMode::Insights
                        | ViewMode::Alerts
                        | ViewMode::FlowMatrix
                        | ViewMode::Attention
                        | ViewMode::Tutorial
                        | ViewMode::Actionable
                        | ViewMode::Sprint
                ) {
                    self.current_view = ViewMode::List;
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
                    }
                } else if self.current_view == ViewMode::History {
                    // Fixed: was previously nested unreachably inside the
                    // Graph arm above (dead code — see TUI_UX_PARITY_PLAN.md
                    // G11), so History's bead cursor never actually moved.
                    if let Some(ref mut h) = self.history {
                        h.move_bead_down();
                    }
                } else if self.current_view == ViewMode::Alerts {
                    if self.alerts_cursor + 1 < self.alerts.len() {
                        self.alerts_cursor += 1;
                    }
                } else if self.current_view == ViewMode::TimeTravel {
                    let max = self.time_travel_entry_count();
                    if max > 0 && self.time_travel_cursor + 1 < max {
                        self.time_travel_cursor += 1;
                    }
                } else if self.current_view == ViewMode::LabelDashboard {
                    let max = self.label_dashboard_count();
                    if max > 0 && self.label_dashboard_cursor + 1 < max {
                        self.label_dashboard_cursor += 1;
                    }
                } else if self.current_view == ViewMode::Sprint {
                    if let Some(s) = &mut self.sprint {
                        s.move_down();
                    }
                } else if self.current_view == ViewMode::Actionable {
                    if let Some(a) = &mut self.actionable {
                        a.move_down();
                    }
                } else if self.current_view == ViewMode::Tutorial {
                    if let Some(t) = &mut self.tutorial {
                        t.next_page();
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
                } else if self.current_view == ViewMode::History {
                    if let Some(ref mut h) = self.history {
                        h.move_bead_up();
                    }
                } else if self.current_view == ViewMode::Alerts {
                    self.alerts_cursor = self.alerts_cursor.saturating_sub(1);
                } else if self.current_view == ViewMode::TimeTravel {
                    self.time_travel_cursor = self.time_travel_cursor.saturating_sub(1);
                } else if self.current_view == ViewMode::LabelDashboard {
                    self.label_dashboard_cursor = self.label_dashboard_cursor.saturating_sub(1);
                } else if self.current_view == ViewMode::Sprint {
                    if let Some(s) = &mut self.sprint {
                        s.move_up();
                    }
                } else if self.current_view == ViewMode::Actionable {
                    if let Some(a) = &mut self.actionable {
                        a.move_up();
                    }
                } else if self.current_view == ViewMode::Tutorial {
                    if let Some(t) = &mut self.tutorial {
                        t.prev_page();
                    }
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
                if self.current_view == ViewMode::History {
                    // Cycle confidence threshold: 0.0 -> 0.5 -> 0.8 -> 0.0
                    // (Go's confidence filter cycle; bvr has no dedicated
                    // cycle method on HistoryState, so it's inlined here).
                    if let Some(ref mut h) = self.history {
                        h.min_confidence = if h.min_confidence >= 0.8 {
                            0.0
                        } else if h.min_confidence >= 0.5 {
                            0.8
                        } else {
                            0.5
                        };
                        self.status_msg = format!(
                            "History confidence \u{2265} {:.0}%",
                            h.min_confidence * 100.0
                        );
                    }
                } else {
                    self.filter_mode = FilterMode::Closed;
                    self.apply_filter();
                }
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
                    if let Some(t) = &mut self.tutorial {
                        t.current_page = 0;
                    }
                    ViewMode::Tutorial
                };
                true
            }
            KeyCode::Char('t') => {
                // Go Time-Travel (`focusTimeTravelInput` → `SnapshotDiff`):
                // open the revision-input prompt; Enter submits a
                // `diff_issues` diff of the loaded set vs `git show <rev>`.
                // Re-opens over an existing result to run another revision.
                self.current_view = ViewMode::TimeTravel;
                if self.time_travel_prompt.is_none() {
                    self.time_travel_prompt = Some(String::new());
                }
                true
            }
            KeyCode::Char('T') => {
                // Go instant variant: diff against HEAD~5 with no prompt.
                self.current_view = ViewMode::TimeTravel;
                self.time_travel_prompt = None;
                self.run_time_travel("HEAD~5");
                true
            }
            KeyCode::Char('[') => {
                // Go Label Dashboard (`focusLabelDashboard`).
                if self.current_view == ViewMode::LabelDashboard {
                    self.current_view = ViewMode::List;
                } else {
                    self.load_label_health_if_needed();
                    self.current_view = ViewMode::LabelDashboard;
                }
                true
            }
            // In the dashboard, `h` opens the health-detail modal for the
            // cursor label (Go `h`) instead of toggling History — this
            // guarded arm must precede the plain `h` arm below.
            KeyCode::Char('h') if self.current_view == ViewMode::LabelDashboard => {
                self.show_label_detail = !self.show_label_detail;
                self.label_drilldown = None;
                true
            }
            // Go `d` drilldown overlay: filtered issue list for the cursor
            // label. `d` is unbound elsewhere, so no fallback behavior lost.
            KeyCode::Char('d') if self.current_view == ViewMode::LabelDashboard => {
                self.open_label_drilldown();
                true
            }
            // Board column navigation (Go `board.go`): guarded arms must
            // precede the global `h`/`l`/`s` meanings (History toggle, label
            // picker, sort cycle), same pattern as the dashboard `h` above.
            KeyCode::Char('h') if self.current_view == ViewMode::Board => {
                self.board_column = self.board_column.saturating_sub(1);
                true
            }
            KeyCode::Char('l') if self.current_view == ViewMode::Board => {
                let max = crate::views::board::column_labels(self, self.board_mode).len();
                if max > 0 && self.board_column + 1 < max {
                    self.board_column += 1;
                }
                true
            }
            KeyCode::Char('s') if self.current_view == ViewMode::Board => {
                self.board_mode = self.board_mode.next();
                self.board_column = 0;
                self.status_msg = format!("Board: grouped by {}", self.board_mode.label());
                true
            }
            KeyCode::Char(c)
                if self.current_view == ViewMode::Board && ('1'..='9').contains(&c) =>
            {
                let idx = (c as u8 - b'1') as usize;
                let max = crate::views::board::column_labels(self, self.board_mode).len();
                if idx < max {
                    self.board_column = idx;
                }
                true
            }
            KeyCode::Char('h') => {
                if self.current_view == ViewMode::History {
                    self.current_view = ViewMode::List;
                } else {
                    self.load_history_if_needed();
                    self.current_view = ViewMode::History;
                }
                true
            }
            KeyCode::Char('J') if self.current_view == ViewMode::History => {
                if let Some(ref mut h) = self.history {
                    h.move_commit_down();
                }
                true
            }
            KeyCode::Char('K') if self.current_view == ViewMode::History => {
                if let Some(ref mut h) = self.history {
                    h.move_commit_up();
                }
                true
            }
            KeyCode::Char('v') if self.current_view == ViewMode::History => {
                if let Some(ref mut h) = self.history {
                    h.toggle_mode();
                }
                true
            }
            KeyCode::Char('y') if self.current_view == ViewMode::History => {
                self.copy_commit_sha();
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
                    self.load_sprint_if_needed();
                    ViewMode::Sprint
                };
                true
            }
            KeyCode::Char('F') => {
                self.current_view = if self.current_view == ViewMode::Actionable {
                    ViewMode::List
                } else {
                    self.load_actionable_if_needed();
                    ViewMode::Actionable
                };
                true
            }
            // Sprint velocity comparison (Go `velocity_comparison.go`
            // sub-toggle). `v` is History-scoped elsewhere, free here.
            KeyCode::Char('v') if self.current_view == ViewMode::Sprint => {
                self.show_velocity = !self.show_velocity;
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
                // When opening detail panel, auto-focus it so j/k scroll
                // the detail content immediately (UX parity with Go bv).
                if self.show_detail {
                    self.focus_detail = true;
                }
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
            KeyCode::Char('l') => {
                self.open_label_picker();
                true
            }
            KeyCode::Char('\'') => {
                self.open_recipe_picker();
                true
            }
            KeyCode::Char('w') if self.workspace_repos.is_some() => {
                self.open_repo_picker();
                true
            }
            KeyCode::Char('U') => {
                if self.update_tag.is_some() {
                    self.show_update_modal = !self.show_update_modal;
                } else {
                    self.status_msg = "No update available".to_string();
                }
                true
            }
            KeyCode::Char('p') => {
                self.toggle_priority_hints();
                true
            }
            // Agent-prompt modal (Go auto-shows on AGENTS.md detection;
            // lowercase `g` was unbound — uppercase `G` is the graph view).
            KeyCode::Char('g') => {
                self.agent_prompt_cursor = 0;
                self.show_agent_prompts = true;
                true
            }
            _ => false,
        }
    }

    /// Toggle Priority Hints (Go `p` — `pkg/ui/model.go`
    /// `showPriorityHints`). Lazily computes the suggestion set on first
    /// use via `bv_analysis::impact::compute_impact_scores` +
    /// `score_to_priority` — the same engine `--robot-priority` already
    /// uses, so this is wiring, not a new scoring algorithm. Unlike the
    /// existing `--robot-priority` CLI path (which only surfaces
    /// under-prioritized issues, `suggested < current`), this counts both
    /// directions to match Go's bidirectional ↑/↓ hint UI.
    fn toggle_priority_hints(&mut self) {
        self.show_priority_hints = !self.show_priority_hints;
        if !self.show_priority_hints {
            self.status_msg.clear();
            return;
        }
        if self.priority_hints.is_none() {
            self.priority_hints = Some(self.compute_priority_hints());
        }
        let n = self.priority_hints.as_ref().map(|m| m.len()).unwrap_or(0);
        self.status_msg = if n > 0 {
            format!("Priority hints: \u{2191} increase \u{2193} decrease ({n} suggestions)")
        } else {
            "Priority hints: No misalignments detected".to_string()
        };
    }

    /// Compute the suggested-priority map (Go `ComputeImpactScores` +
    /// `GenerateRecommendations` — `pkg/analysis/priority.go`). Only open
    /// issues are scored (Go parity — closed/tombstone are skipped inside
    /// `compute_impact_scores` itself); only entries where the suggestion
    /// differs from the current priority are kept, matching "misalignment."
    fn compute_priority_hints(&self) -> std::collections::BTreeMap<String, i32> {
        let issues: Vec<bv_core::model::Issue> = self.issue_map.values().cloned().collect();
        let g = bv_analysis::build_graph(&issues);
        let empty = BTreeMap::new();
        let (pagerank, betweenness) = match &self.graph_metrics {
            Some(gm) => (&gm.pagerank, &gm.betweenness),
            None => (&empty, &empty),
        };
        let inputs = bv_analysis::impact::ImpactInputs {
            issues: &issues,
            pagerank,
            betweenness,
            critical_path: None,
            g: &g,
            now: jiff::Timestamp::now(),
        };
        bv_analysis::impact::compute_impact_scores(&inputs)
            .into_iter()
            .filter_map(|r| {
                let suggested = bv_analysis::scoring::score_to_priority(r.score);
                (suggested != r.priority).then_some((r.id, suggested))
            })
            .collect()
    }

    /// Build and show the label-filter picker overlay (Go `label_picker.go`
    /// — real picker, replacing/augmenting the `L` one-key cycle).
    fn open_label_picker(&mut self) {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for row in &self.rows {
            for label in &row.labels {
                *counts.entry(label.clone()).or_insert(0) += 1;
            }
        }
        let options: Vec<crate::views::pickers::LabelOption> = counts
            .into_iter()
            .map(|(name, count)| crate::views::pickers::LabelOption { name, count })
            .collect();
        let mut picker = crate::views::pickers::LabelPicker::new(options);
        picker.toggle(); // sets visible = true
        self.label_picker = Some(picker);
    }

    fn handle_label_picker_key(&mut self, code: KeyCode) -> bool {
        let Some(picker) = self.label_picker.as_mut() else {
            return false;
        };
        match code {
            KeyCode::Esc => picker.visible = false,
            KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
            KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
            KeyCode::Backspace => {
                let mut t = picker.filter_text.clone();
                t.pop();
                picker.update_filter(&t);
            }
            KeyCode::Char(c) => {
                let mut t = picker.filter_text.clone();
                t.push(c);
                picker.update_filter(&t);
            }
            KeyCode::Enter => {
                let chosen = picker.selected_label().map(|s| s.to_string());
                picker.visible = false;
                if let Some(label) = chosen {
                    self.label_filter = Some(label);
                    self.apply_filter();
                }
            }
            _ => {}
        }
        true
    }

    /// Build and show the workspace repo picker overlay (Go
    /// `repo_picker.go` — real picker, replacing the old `w` one-key
    /// cycle-through per TUI_UX_PARITY_PLAN.md Q1).
    fn open_repo_picker(&mut self) {
        let Some(repos) = &self.workspace_repos else {
            return;
        };
        let counts: BTreeMap<&str, usize> =
            self.issue_map
                .values()
                .fold(BTreeMap::new(), |mut acc, issue| {
                    *acc.entry(issue.source_repo.as_str()).or_insert(0) += 1;
                    acc
                });
        let options: Vec<crate::views::pickers::RepoOption> = repos
            .iter()
            .map(|name| crate::views::pickers::RepoOption {
                name: name.clone(),
                path: name.clone(),
                issue_count: counts.get(name.as_str()).copied().unwrap_or(0),
            })
            .collect();
        let mut picker = crate::views::pickers::RepoPicker::new(options);
        picker.toggle();
        self.repo_picker = Some(picker);
    }

    fn handle_repo_picker_key(&mut self, code: KeyCode) -> bool {
        let Some(picker) = self.repo_picker.as_mut() else {
            return false;
        };
        match code {
            KeyCode::Esc => picker.visible = false,
            KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
            KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
            KeyCode::Enter => {
                let chosen = picker.selected_repo().map(|r| r.name.clone());
                picker.visible = false;
                self.active_repo = chosen;
                self.status_msg = match &self.active_repo {
                    Some(r) => format!("Repos: {r}"),
                    None => "Repos: all".to_string(),
                };
                self.apply_filter();
            }
            _ => {}
        }
        true
    }

    /// Build and show the recipe picker overlay (Go `recipe_picker.go`).
    /// Recipe *definitions* (name/filter/sort) come from
    /// `crate::views::pickers::default_recipes` — the same 6 built-in
    /// recipes Go embeds in `defaults/recipes.yaml`
    /// (triage/release-cut/blocked-review/dependency-risk/quick-wins/stale).
    /// User/project YAML recipe loading is a documented scope cut (same
    /// convention as the rest of this port — see `pickers.rs`'s own scope
    /// note); only the built-in set is real.
    fn open_recipe_picker(&mut self) {
        let options: Vec<crate::views::pickers::RecipeOption> = default_recipe_defs()
            .iter()
            .map(|r| crate::views::pickers::RecipeOption {
                name: r.name.to_string(),
                description: r.description.to_string(),
                labels: vec![],
            })
            .collect();
        let mut picker = crate::views::pickers::RecipePicker::new(options);
        picker.toggle();
        self.recipe_picker = Some(picker);
    }

    fn handle_recipe_picker_key(&mut self, code: KeyCode) -> bool {
        let Some(picker) = self.recipe_picker.as_mut() else {
            return false;
        };
        let mut apply: Option<usize> = None;
        match code {
            KeyCode::Esc => picker.visible = false,
            KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
            KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
            KeyCode::Enter => {
                apply = Some(picker.selected);
                picker.visible = false;
            }
            _ => {}
        }
        if let Some(idx) = apply {
            self.apply_recipe(idx);
        }
        true
    }

    /// Apply built-in recipe `idx` (from `default_recipe_defs`) by setting
    /// filter/sort state and, where the recipe needs a predicate the
    /// FilterMode enum can't express (blocked/dependency-risk/quick-wins/
    /// stale), computing `filtered_indices` directly — same pattern
    /// `apply_search` already uses to bypass FilterMode.
    fn apply_recipe(&mut self, idx: usize) {
        let defs = default_recipe_defs();
        let Some(def) = defs.get(idx) else { return };
        self.label_filter = None;
        self.active_repo = None;
        match def.name {
            "triage" => {
                self.filter_mode = FilterMode::All;
                self.apply_filter();
                let by_id: std::collections::HashMap<&str, &bv_core::model::Issue> = self
                    .issue_map
                    .iter()
                    .map(|(k, v)| (k.as_str(), v))
                    .collect();
                self.filtered_indices.retain(|&i| {
                    bv_analysis::blocker_chain::is_actionable(&by_id, &self.rows[i].id)
                });
                self.sort_mode = SortMode::Priority;
                self.filtered_indices
                    .sort_by_key(|&i| self.rows[i].priority);
            }
            "release-cut" => {
                self.filter_mode = FilterMode::Closed;
                self.sort_mode = SortMode::Updated;
                self.apply_filter();
                self.filtered_indices
                    .sort_by(|&a, &b| self.rows[b].created_at.cmp(&self.rows[a].created_at));
            }
            "blocked-review" => {
                self.filter_mode = FilterMode::All;
                self.apply_filter();
                let by_id: std::collections::HashMap<&str, &bv_core::model::Issue> = self
                    .issue_map
                    .iter()
                    .map(|(k, v)| (k.as_str(), v))
                    .collect();
                self.filtered_indices.retain(|&i| {
                    !bv_analysis::blocker_chain::open_blockers(&by_id, &self.rows[i].id).is_empty()
                });
            }
            "dependency-risk" => {
                self.filter_mode = FilterMode::All;
                self.apply_filter();
                if let Some(gm) = &self.graph_metrics {
                    let bw = gm.betweenness.clone();
                    self.filtered_indices.sort_by(|&a, &b| {
                        let sa = bw.get(&self.rows[a].id).copied().unwrap_or(0.0);
                        let sb = bw.get(&self.rows[b].id).copied().unwrap_or(0.0);
                        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    self.filtered_indices.truncate(20);
                }
            }
            "quick-wins" => {
                self.filter_mode = FilterMode::Open;
                self.apply_filter();
                self.filtered_indices.retain(|&i| {
                    self.rows[i].priority >= 2
                        && self
                            .issue_map
                            .get(&self.rows[i].id)
                            .map(|iss| iss.dependencies.is_empty())
                            .unwrap_or(false)
                });
            }
            "stale" => {
                self.filter_mode = FilterMode::Open;
                self.apply_filter();
                let now = jiff::Timestamp::now();
                self.filtered_indices.retain(|&i| {
                    self.rows[i]
                        .created_at
                        .as_ref()
                        .and_then(|_| self.issue_map.get(&self.rows[i].id))
                        .and_then(|iss| iss.updated_at.as_ref())
                        .and_then(|u| u.parse::<jiff::Timestamp>().ok())
                        .map(|t| now.since(t).map(|d| d.get_days()).unwrap_or(0) >= 14)
                        .unwrap_or(false)
                });
            }
            _ => {}
        }
        self.cursor = 0;
        self.status_msg = format!("Recipe: {}", def.name);
    }

    /// Handle a Ctrl-modified key event; returns true if consumed.
    pub fn handle_ctrl_key(&mut self, code: KeyCode) -> bool {
        if self.semantic_searching {
            return self.handle_semantic_key(code);
        }
        match code {
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.reload_from_disk();
                true
            }
            // Go semantic search entry (Ctrl+S): cosine-similarity ranking
            // via the same `bv_search` text engine `--robot-search` uses.
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.semantic_searching = true;
                self.semantic_query.clear();
                self.status_msg =
                    "Semantic search (bv-search cosine) — type to rank, Enter accepts, Esc clears"
                        .to_string();
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
        let id = row.id.clone();
        match copy_to_clipboard(&text) {
            Ok(()) => self.status_msg = format!("Copied {id} to clipboard"),
            Err(e) => self.status_msg = format!("Clipboard failed: {e}"),
        }
    }

    /// Copy the selected commit's SHA in the History view (Go `y`).
    fn copy_commit_sha(&mut self) {
        let Some(ref history) = self.history else {
            return;
        };
        let Some(commit) = history.selected_commit() else {
            self.status_msg = "No commit selected".to_string();
            return;
        };
        let sha = commit.sha.clone();
        match copy_to_clipboard(&sha) {
            Ok(()) => self.status_msg = format!("Copied {sha} to clipboard"),
            Err(e) => self.status_msg = format!("Clipboard failed: {e}"),
        }
    }
    /// Lazily build the Actionable view state on first `F` (same deferred
    /// pattern as History/LabelDashboard — see G15). Rebuilt from scratch on
    /// `reload_from_disk` because that reconstructs the whole `App`.
    fn load_actionable_if_needed(&mut self) {
        if self.actionable.is_some() {
            return;
        }
        let issues: Vec<bv_core::model::Issue> = self.issue_map.values().cloned().collect();
        self.actionable = Some(crate::actionable::ActionableState::new(&issues));
    }

    /// Lazily load sprints on first `P` (`.beads/sprints.jsonl` — same
    /// deferred pattern as History; previously `sprint` stayed `None`
    /// forever so the view could never show data). Errors resolve to an
    /// empty state so the view renders "No sprints defined" instead of
    /// retrying the filesystem on every toggle.
    fn load_sprint_if_needed(&mut self) {
        if self.sprint.is_some() {
            return;
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let sprints = bv_core::sprint::load_sprints(&cwd).unwrap_or_default();
        self.sprint = Some(crate::views::sprint::SprintState {
            sprints,
            selected_idx: 0,
        });
    }

    /// Keys while the agent-prompt modal is open (Go `agent_prompt_modal.go`:
    /// `j`/`k` navigate, `Enter` copies the command, `Esc` closes).
    fn handle_agent_prompt_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                self.show_agent_prompts = false;
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let max = crate::agent_prompt_modal::default_agent_prompts().len();
                if max > 0 && self.agent_prompt_cursor + 1 < max {
                    self.agent_prompt_cursor += 1;
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.agent_prompt_cursor = self.agent_prompt_cursor.saturating_sub(1);
                true
            }
            KeyCode::Enter => {
                let prompts = crate::agent_prompt_modal::default_agent_prompts();
                if let Some(p) = prompts.get(self.agent_prompt_cursor) {
                    let cmd = p.command.clone();
                    let label = p.label.clone();
                    match copy_to_clipboard(&cmd) {
                        Ok(()) => self.status_msg = format!("Copied {label} prompt to clipboard"),
                        Err(e) => self.status_msg = format!("Clipboard failed: {e}"),
                    }
                }
                self.show_agent_prompts = false;
                true
            }
            _ => true, // modal captures all keys; anything else just holds it open
        }
    }

    /// Per-sprint velocity points for the comparison overlay (Go
    /// `velocity_comparison.go`): planned = sprint bead count, completed =
    /// closed beads, velocity = completed count.
    fn velocity_points(&self) -> Vec<crate::views::velocity_comparison::VelocityPoint> {
        let Some(sprint) = &self.sprint else {
            return Vec::new();
        };
        sprint
            .sprints
            .iter()
            .map(|s| {
                let planned = s.bead_ids.len();
                let completed = s
                    .bead_ids
                    .iter()
                    .filter(|id| {
                        self.issue_map
                            .get(*id)
                            .is_some_and(|i| i.status.is_closed())
                    })
                    .count();
                crate::views::velocity_comparison::VelocityPoint {
                    sprint_name: s.name.clone(),
                    completed,
                    planned,
                    velocity: completed as f64,
                }
            })
            .collect()
    }

    /// Lazily populate the History view with real bead↔commit correlation
    /// data (Go's `pkg/ui` loads this eagerly; bvr defers the git-log walk
    /// to first entry, matching the `App::new` comment's original intent —
    /// see TUI_UX_PARITY_PLAN.md G12). No-op after the first successful load
    /// or if not inside a git repo.
    fn load_history_if_needed(&mut self) {
        if self.history_loaded {
            return;
        }
        self.history_loaded = true; // don't retry every keypress on failure
        let Ok(cwd) = std::env::current_dir() else {
            self.status_msg = "History: could not determine working directory".to_string();
            return;
        };
        let commits = match bv_correlation::correlator::walk_commits(&cwd, 1000) {
            Ok(c) => c,
            Err(e) => {
                self.status_msg = format!("History: git log failed ({e})");
                return;
            }
        };
        // CorrelatedCommit carries a correlation `reason`, not the original
        // commit subject line — look the real message up by sha for display.
        let messages_by_sha: std::collections::HashMap<String, String> = commits
            .iter()
            .map(|c| (c.sha.clone(), c.message.clone()))
            .collect();
        let issues: Vec<bv_core::model::Issue> = self.issue_map.values().cloned().collect();
        let report = bv_correlation::correlator::correlate(&issues, &commits);
        let mut bead_histories: Vec<crate::views::history::BeadHistory> = report
            .into_iter()
            .filter_map(|(bead_id, commits)| {
                let issue = self.issue_map.get(&bead_id)?;
                let mut hist_commits: Vec<crate::views::history::HistoryCommit> = commits
                    .into_iter()
                    .map(|c| crate::views::history::HistoryCommit {
                        short_sha: c.sha.chars().take(7).collect(),
                        message: messages_by_sha
                            .get(&c.sha)
                            .cloned()
                            .unwrap_or_else(|| c.reason.clone()),
                        sha: c.sha,
                        author: c.author,
                        timestamp: c.timestamp,
                        confidence: c.confidence,
                        files: c.files,
                    })
                    .collect();
                hist_commits.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                Some(crate::views::history::BeadHistory {
                    bead_id: bead_id.clone(),
                    title: issue.title.clone(),
                    status: issue.status.as_str().to_string(),
                    commits: hist_commits,
                })
            })
            .collect();
        bead_histories.sort_by(|a, b| a.bead_id.cmp(&b.bead_id));
        let has_data = !bead_histories.is_empty();
        self.history = Some(crate::views::history::HistoryState::build_from_beads(
            bead_histories,
        ));
        self.status_msg = if has_data {
            "History loaded".to_string()
        } else {
            "History: no correlated commits found".to_string()
        };
    }
    /// Number of flattened entries in the current Time-Travel diff
    /// (added + removed + changed), for cursor clamping.
    fn time_travel_entry_count(&self) -> usize {
        self.time_travel_result
            .as_ref()
            .map_or(0, |r| r.added.len() + r.removed.len() + r.changed.len())
    }

    /// Pure compute+store half of a Time-Travel diff (testable without git):
    /// diff `previous` (issues at `git_ref`) against the loaded issue set.
    fn apply_time_travel_result(&mut self, previous: Vec<bv_core::model::Issue>, git_ref: &str) {
        let current: Vec<bv_core::model::Issue> = self.issue_map.values().cloned().collect();
        let n_prev = previous.len();
        let result = bv_analysis::diff::diff_issues(&current, &previous, git_ref);
        self.time_travel_cursor = 0;
        self.time_travel_error = None;
        self.status_msg = format!(
            "Time-Travel vs {git_ref}: +{} -{} ~{} (from {n_prev} issues at ref)",
            result.added_count, result.removed_count, result.changed_count
        );
        self.time_travel_result = Some(result);
    }

    /// Fetch issues at `git_ref` via the shared `GitLoader` plumbing (the
    /// same path `--robot-diff` uses — see plan Q4) and store the diff.
    /// Error text mirrors the robot command's
    /// (`could not read issues at ref <ref>`).
    fn run_time_travel(&mut self, git_ref: &str) {
        let cwd = std::env::current_dir().unwrap_or_default();
        match bv_core::discovery::GitLoader::new(&cwd).load_at(git_ref) {
            Ok(previous) => self.apply_time_travel_result(previous, git_ref),
            Err(_) => {
                self.time_travel_result = None;
                self.time_travel_error = Some(format!("could not read issues at ref {git_ref}"));
                self.status_msg = format!("Time-Travel: could not read issues at ref {git_ref}");
            }
        }
    }

    /// Key handling while the Time-Travel revision prompt is open: typing
    /// edits the buffer, Enter submits, Esc cancels (Go `focusTimeTravelInput`).
    fn handle_time_travel_prompt_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                self.time_travel_prompt = None;
                if self.time_travel_result.is_none() {
                    self.current_view = ViewMode::List;
                }
                true
            }
            KeyCode::Enter => {
                let rev = self
                    .time_travel_prompt
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if rev.is_empty() {
                    return true; // keep prompt open; nothing to diff
                }
                self.time_travel_prompt = None;
                self.run_time_travel(&rev);
                true
            }
            KeyCode::Backspace => {
                if let Some(buf) = &mut self.time_travel_prompt {
                    buf.pop();
                }
                true
            }
            KeyCode::Char(c) => {
                if let Some(buf) = &mut self.time_travel_prompt {
                    buf.push(c);
                }
                true
            }
            _ => true,
        }
    }
    /// Lazily compute per-label health (Go `focusLabelDashboard` data).
    /// Deferred to first `[` press like History — same analysis the
    /// `--robot-label-health` path uses, computed once per session.
    fn load_label_health_if_needed(&mut self) {
        if self.label_health_loaded {
            return;
        }
        self.label_health_loaded = true; // don't retry every keypress on failure
        let issues: Vec<bv_core::model::Issue> = self.issue_map.values().cloned().collect();
        let cfg = bv_analysis::label_health::LabelHealthConfig::default();
        let now = jiff::Timestamp::now();
        let result = bv_analysis::label_health::compute_all_label_health(&issues, &cfg, now);
        self.status_msg = format!("Label dashboard: {} labels", result.total_labels);
        self.label_health = Some(result);
    }

    fn label_dashboard_count(&self) -> usize {
        self.label_health.as_ref().map_or(0, |r| r.labels.len())
    }

    /// Display order: worst health first, label asc tiebreak (so the
    /// labels needing attention sit at the top, Go dashboard style).
    fn label_dashboard_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.label_dashboard_count()).collect();
        if let Some(r) = &self.label_health {
            idx.sort_by(|&a, &b| {
                r.labels[a]
                    .health
                    .cmp(&r.labels[b].health)
                    .then_with(|| r.labels[a].label.cmp(&r.labels[b].label))
            });
        }
        idx
    }

    fn selected_dashboard_label(&self) -> Option<&bv_analysis::label_health::LabelHealth> {
        let order = self.label_dashboard_order();
        order
            .get(self.label_dashboard_cursor)
            .and_then(|&i| self.label_health.as_ref().map(|r| &r.labels[i]))
    }

    fn open_label_drilldown(&mut self) {
        if let Some(label) = self.selected_dashboard_label().map(|l| l.label.clone()) {
            // One overlay at a time (same convention as render_overlays).
            self.show_label_detail = false;
            self.label_drilldown = Some(LabelDrilldown {
                label,
                filter: String::new(),
                cursor: 0,
            });
        }
    }

    /// (id, title, status) rows for the open drilldown, substring-filtered
    /// on id+title and sorted by id (same filter semantics as `/` search).
    fn drilldown_rows(&self) -> Vec<(String, String, bv_core::model::Status)> {
        let Some(dd) = &self.label_drilldown else {
            return Vec::new();
        };
        let q = dd.filter.to_lowercase();
        let mut rows: Vec<(String, String, bv_core::model::Status)> = self
            .issue_map
            .values()
            .filter(|i| i.labels.iter().any(|l| l == &dd.label))
            .filter(|i| {
                q.is_empty()
                    || i.id.to_lowercase().contains(&q)
                    || i.title.to_lowercase().contains(&q)
            })
            .map(|i| (i.id.clone(), i.title.clone(), i.status))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    /// Key handling while the drilldown overlay is open: picker convention
    /// (`j`/`k` navigate, other chars filter, `Enter` jumps, `Esc` closes).
    fn handle_label_drilldown_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                self.label_drilldown = None;
                true
            }
            KeyCode::Enter => {
                // Jump to List filtered by this label (Go drilldown Enter).
                if let Some(dd) = self.label_drilldown.take() {
                    self.label_filter = Some(dd.label.clone());
                    self.current_view = ViewMode::List;
                    self.apply_filter();
                    self.status_msg = format!("Filtered by label: {}", dd.label);
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(dd) = &mut self.label_drilldown {
                    dd.cursor = dd.cursor.saturating_sub(1);
                }
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.drilldown_rows().len();
                if let Some(dd) = &mut self.label_drilldown {
                    if max > 0 && dd.cursor + 1 < max {
                        dd.cursor += 1;
                    }
                }
                true
            }
            KeyCode::Backspace => {
                if let Some(dd) = &mut self.label_drilldown {
                    dd.filter.pop();
                    dd.cursor = 0;
                }
                true
            }
            KeyCode::Char(c) => {
                if let Some(dd) = &mut self.label_drilldown {
                    dd.filter.push(c);
                    dd.cursor = 0;
                }
                true
            }
            _ => true,
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
            render_overlays(f, app);
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
            render_overlays(f, app);
            return;
        }
        ViewMode::Alerts => {
            crate::views::alerts::render_alerts(f, &app.alerts, app.alerts_cursor, f.area());
            render_status_bar(f, app);
            render_overlays(f, app);
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
            render_overlays(f, app);
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
            render_overlays(f, app);
            return;
        }
        ViewMode::Tutorial => {
            if let Some(ref t) = app.tutorial {
                crate::tutorial::render_tutorial(f, t, f.area());
            } else {
                let msg = ratatui::widgets::Paragraph::new("No tutorial content");
                f.render_widget(msg, f.area());
            }
            render_status_bar(f, app);
            render_overlays(f, app);
            return;
        }
        ViewMode::Actionable => {
            if let Some(ref state) = app.actionable {
                crate::actionable::render_actionable(f, &state.items, state.selected, f.area());
            } else {
                let msg = ratatui::widgets::Paragraph::new("No actionable items");
                f.render_widget(msg, f.area());
            }
            render_status_bar(f, app);
            render_overlays(f, app);
            return;
        }
        ViewMode::Board => {
            crate::views::board::render_board(f, app, f.area(), app.board_mode, app.board_column);
            render_status_bar(f, app);
            render_overlays(f, app);
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
            render_overlays(f, app);
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
            if app.show_velocity {
                let points = app.velocity_points();
                crate::views::velocity_comparison::render_velocity_comparison(f, &points, f.area());
            }
            render_status_bar(f, app);
            render_overlays(f, app);
            return;
        }
        ViewMode::History => {
            if let Some(ref history) = app.history {
                crate::views::history::render_history(f, history, f.area());
            } else {
                let msg =
                    ratatui::widgets::Paragraph::new("No history data available (press h to load)");
                f.render_widget(msg, f.area());
            }
            render_status_bar(f, app);
            render_overlays(f, app);
            return;
        }
        ViewMode::TimeTravel => {
            render_time_travel(f, app);
            render_status_bar(f, app);
            render_overlays(f, app);
            return;
        }
        ViewMode::LabelDashboard => {
            render_label_dashboard(f, app);
            render_status_bar(f, app);
            render_overlays(f, app);
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
    render_overlays(f, app);
}

/// Label Dashboard view (Go `focusLabelDashboard`): per-label health
/// badges computed by `bv_analysis::label_health` (thresholds
/// Healthy ≥ 70 / Warning ≥ 40 / else Critical live in that crate, shared
/// with `--robot-label-health`), worst-first order, `h` detail modal and
/// `d` drilldown overlay for the cursor label.
fn render_label_dashboard(f: &mut Frame, app: &App) {
    let area = f.area();
    let order = app.label_dashboard_order();
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        " Label Dashboard ",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];
    let Some(result) = &app.label_health else {
        lines.push(Line::from(""));
        lines.push(Line::from("No label data (press [ to load)."));
        let msg = ratatui::widgets::Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" LABELS "));
        f.render_widget(msg, area);
        return;
    };
    lines.push(Line::from(format!(
        "{} labels: {} healthy  {} warning  {} critical   (j/k move, h detail, d issues, [ close)",
        result.total_labels, result.healthy_count, result.warning_count, result.critical_count
    )));
    lines.push(Line::from(""));
    if order.is_empty() {
        lines.push(Line::from("No labeled issues."));
    }
    for (row, &i) in order.iter().enumerate() {
        let l = &result.labels[i];
        let (badge, color) = match l.health_level {
            "healthy" => ("●", Color::Green),
            "warning" => ("●", Color::Yellow),
            _ => ("●", Color::Red),
        };
        let blocked_ratio = if l.issue_count > 0 {
            l.blocked as f64 / l.issue_count as f64 * 100.0
        } else {
            0.0
        };
        let text = format!(
            "{} {:<24} {:>3}  open {:>3}/{:<3}  blocked {:>4.0}%  {}",
            badge, l.label, l.health, l.open_count, l.issue_count, blocked_ratio, l.health_level
        );
        if row == app.label_dashboard_cursor {
            lines.push(Line::from(vec![
                Span::styled(
                    "> ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {badge}"), Style::default().fg(color)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{badge} "), Style::default().fg(color)),
                Span::raw(text),
            ]));
        }
    }
    let msg = ratatui::widgets::Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" LABELS "));
    f.render_widget(msg, area);
    if app.show_label_detail {
        render_label_detail_modal(f, app);
    }
    if app.label_drilldown.is_some() {
        render_label_drilldown(f, app);
    }
}

/// Health-detail modal for the cursor label: velocity / staleness /
/// blocked-ratio / work-distribution breakdown (Go `h` modal).
fn render_label_detail_modal(f: &mut Frame, app: &App) {
    let Some(l) = app.selected_dashboard_label() else {
        return;
    };
    let blocked_ratio = if l.issue_count > 0 {
        l.blocked as f64 / l.issue_count as f64 * 100.0
    } else {
        0.0
    };
    let body: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(" {}  ({} {})", l.label, l.health, l.health_level),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(
            "Work: {} open / {} closed / {} total, {} blocked ({:.0}%)",
            l.open_count, l.closed_count, l.issue_count, l.blocked, blocked_ratio
        )),
        Line::from(format!(
            "Velocity: {} closed/7d, {} closed/30d, avg {:.1}d to close (score {}) — {} {:.0}%",
            l.velocity.closed_last_7_days,
            l.velocity.closed_last_30_days,
            l.velocity.avg_days_to_close,
            l.velocity.velocity_score,
            l.velocity.trend_direction,
            l.velocity.trend_percent
        )),
        Line::from(format!(
            "Freshness: {} stale, avg {:.1}d since update (score {})",
            l.freshness.stale_count, l.freshness.avg_days_since_update, l.freshness.freshness_score
        )),
        Line::from(format!(
            "Flow: {} in / {} out deps, {} blocked-by-external, {} blocking-external (score {})",
            l.flow.incoming_deps,
            l.flow.outgoing_deps,
            l.flow.blocked_by_external,
            l.flow.blocking_external,
            l.flow.flow_score
        )),
        Line::from(""),
        Line::from(Span::styled(
            " h / esc = close ",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let w = 72.min(app.width.saturating_sub(4));
    let h = (body.len() as u16 + 2).min(app.height.saturating_sub(2));
    let popup = ratatui::layout::Rect {
        x: app.width.saturating_sub(w) / 2,
        y: app.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(
        ratatui::widgets::Paragraph::new(body).block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(format!(" LABEL: {} ", l.label))
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        popup,
    );
}

/// Drilldown overlay: filtered issue list for one label; `Enter` jumps to
/// List filtered by that label (Go `d` drilldown).
fn render_label_drilldown(f: &mut Frame, app: &App) {
    let Some(dd) = &app.label_drilldown else {
        return;
    };
    let rows = app.drilldown_rows();
    let mut body: Vec<Line> = vec![Line::from(vec![
        Span::raw("Filter: "),
        Span::raw(dd.filter.clone()),
        Span::styled("█", Style::default().fg(Color::Yellow)),
    ])];
    let visible = app.height.saturating_sub(10).max(1) as usize;
    let start = dd.cursor.saturating_sub(visible.saturating_sub(1));
    for (row, (id, title, _)) in rows.iter().enumerate().skip(start).take(visible) {
        let text = format!("{id}  {title}");
        if row == dd.cursor {
            body.push(Line::from(vec![
                Span::styled(
                    "> ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            body.push(Line::from(format!("  {text}")));
        }
    }
    if rows.is_empty() {
        body.push(Line::from("(no issues match)"));
    }
    body.push(Line::from(""));
    body.push(Line::from(Span::styled(
        " Enter = list filtered by label   Esc = close ",
        Style::default().fg(Color::DarkGray),
    )));
    let w = 72.min(app.width.saturating_sub(4));
    let h = (body.len() as u16 + 2).min(app.height.saturating_sub(2));
    let popup = ratatui::layout::Rect {
        x: app.width.saturating_sub(w) / 2,
        y: app.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(
        ratatui::widgets::Paragraph::new(body).block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(format!(" LABEL: {} ({} issues) ", dd.label, rows.len()))
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

/// Time-Travel view (Go `focusTimeTravelInput` → `SnapshotDiff`): revision
/// prompt plus the added/removed/changed diff vs that git revision, backed
/// by `bv_analysis::diff::diff_issues` over `GitLoader::load_at` output.
fn render_time_travel(f: &mut Frame, app: &App) {
    let area = f.area();
    let mut head: Vec<Line> = vec![Line::from(Span::styled(
        " Time-Travel ",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];
    if let Some(buf) = &app.time_travel_prompt {
        head.push(Line::from(""));
        head.push(Line::from("Revision (branch / tag / SHA / HEAD~N):"));
        head.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Yellow)),
            Span::raw(buf.clone()),
            Span::styled("█", Style::default().fg(Color::Yellow)),
        ]));
        head.push(Line::from(Span::styled(
            " Enter = diff   Esc = cancel",
            Style::default().fg(Color::DarkGray),
        )));
    }
    // Flattened navigable entries: added, then removed, then changed.
    let mut entries: Vec<Line> = Vec::new();
    let mut title = " TIME-TRAVEL ".to_string();
    match (&app.time_travel_result, &app.time_travel_error) {
        (Some(r), _) => {
            title = format!(" TIME-TRAVEL vs {} ", r.diff_ref);
            head.push(Line::from(""));
            head.push(Line::from(format!(
                "+{} added   -{} removed   ~{} changed   (j/k navigate, t new revision, T HEAD~5, esc back)",
                r.added_count, r.removed_count, r.changed_count
            )));
            if r.added_count == 0 && r.removed_count == 0 && r.changed_count == 0 {
                head.push(Line::from(
                    "No differences — working set matches this revision.",
                ));
            }
            let mut idx = 0;
            let mut push_entry = |idx: &mut usize, text: String| {
                let (marker, style) = if *idx == app.time_travel_cursor {
                    (
                        "> ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("  ", Style::default())
                };
                entries.push(Line::from(vec![
                    Span::styled(marker, style),
                    Span::raw(text),
                ]));
                *idx += 1;
            };
            for id in &r.added {
                push_entry(&mut idx, format!("+ {id}"));
            }
            for id in &r.removed {
                push_entry(&mut idx, format!("- {id}"));
            }
            for c in &r.changed {
                let mut detail = String::new();
                if let Some(sc) = &c.status_change {
                    detail.push_str(&format!(" [{sc}]"));
                }
                if c.title_changed {
                    detail.push_str(" [title]");
                }
                if c.priority_changed {
                    detail.push_str(" [priority]");
                }
                push_entry(&mut idx, format!("~ {}{}", c.id, detail));
            }
        }
        (None, Some(e)) => {
            head.push(Line::from(""));
            head.push(Line::from(Span::styled(
                format!("Error: {e}"),
                Style::default().fg(Color::Red),
            )));
            head.push(Line::from("Press t for another revision, T for HEAD~5."));
        }
        (None, None) => {
            if app.time_travel_prompt.is_none() {
                head.push(Line::from(""));
                head.push(Line::from(
                    "Press t to enter a revision, T for instant HEAD~5.",
                ));
            }
        }
    }
    // Window the entry list around the cursor so huge diffs fit the view.
    let chrome = head.len() + 5; // borders + status bar + overflow line
    let visible = area
        .height
        .saturating_sub(chrome.min(u16::MAX as usize) as u16)
        .max(1) as usize;
    let start = app
        .time_travel_cursor
        .saturating_sub(visible.saturating_sub(1));
    let mut lines = head;
    lines.extend(entries.into_iter().skip(start).take(visible));
    let total = app.time_travel_entry_count();
    if total > visible {
        lines.push(Line::from(Span::styled(
            format!(
                "… {} of {total} entries (j/k to scroll)",
                visible.min(total)
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }
    let msg = ratatui::widgets::Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(msg, area);
}

/// Modal/popup overlays shown on top of whichever view is active: help,
/// update-available modal, and the label/recipe/repo pickers.
///
/// Previously the `?` help overlay (and nothing else) was drawn only in the
/// code path reached after the big `match app.current_view` in `render()`
/// fell through — which every non-List view `return`s before reaching, so
/// `?` silently did nothing while inside Board/Tree/Graph/Insights/Alerts/
/// FlowMatrix/Attention/Actionable/Sprint/History/TimeTravel (see
/// TUI_UX_PARITY_PLAN.md G8). Centralizing overlay rendering here and
/// calling it from every early-return branch fixes that for all of them at
/// once.
fn render_overlays(f: &mut Frame, app: &App) {
    // Help overlay (Go "?" help) — now generated from `key_registry`
    // instead of a separately hand-maintained literal list, so it can't
    // drift from the registry the way the old hardcoded text had (e.g. it
    // used to say "g Toggle graph view" while the real binding was `G`).
    if app.show_help {
        let focus = focus_for_view(app.current_view);
        let bindings = app.key_registry.bindings_for(focus);
        let mut help_lines: Vec<Line> = vec![
            Line::from(Span::styled(
                " Keyboard Shortcuts ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];
        if bindings.is_empty() {
            help_lines.push(Line::from("  (no bindings documented for this view yet)"));
        } else {
            for b in bindings {
                help_lines.push(Line::from(format!("  {:<12} {}", b.key, b.desc)));
            }
        }
        help_lines.push(Line::from(""));
        help_lines.push(Line::from(Span::styled(
            " Press any key to close ",
            Style::default().fg(Color::DarkGray),
        )));
        let w = 56.min(app.width.saturating_sub(4));
        let h = (help_lines.len() as u16 + 2).min(app.height.saturating_sub(2));
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
        return; // one overlay at a time, matching prior behavior
    }

    if app.show_update_modal {
        if let Some(tag) = &app.update_tag {
            crate::update_modal::render_update_modal(f, env!("CARGO_PKG_VERSION"), tag, f.area());
        }
        return;
    }

    if app.show_agent_prompts {
        let prompts = crate::agent_prompt_modal::default_agent_prompts();
        crate::agent_prompt_modal::render_agent_prompt(
            f,
            &prompts,
            app.agent_prompt_cursor,
            f.area(),
        );
        return;
    }

    if let Some(picker) = &app.label_picker {
        if picker.visible {
            picker.render(f, f.area());
            return;
        }
    }
    if let Some(picker) = &app.recipe_picker {
        if picker.visible {
            picker.render(f, f.area());
            return;
        }
    }
    if let Some(picker) = &app.repo_picker {
        if picker.visible {
            picker.render(f, f.area());
        }
    }
}

/// Map the active `ViewMode` to the `keybindings::Focus` bucket that
/// documents its bindings (used by the dynamic help overlay above).
fn focus_for_view(view: ViewMode) -> crate::keybindings::Focus {
    use crate::keybindings::Focus;
    match view {
        ViewMode::List => Focus::List,
        ViewMode::Board => Focus::Board,
        ViewMode::Tree => Focus::Tree,
        ViewMode::Graph => Focus::Graph,
        ViewMode::FlowMatrix => Focus::FlowMatrix,
        ViewMode::Attention => Focus::Attention,
        ViewMode::Insights => Focus::Insights,
        ViewMode::Alerts => Focus::Alerts,
        ViewMode::History => Focus::History,
        ViewMode::TimeTravel => Focus::TimeTravel,
        ViewMode::LabelDashboard => Focus::LabelDashboard,
        ViewMode::Sprint => Focus::Sprint,
        ViewMode::Tutorial => Focus::Tutorial,
        ViewMode::Actionable => Focus::Actionable,
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

            // Priority hint arrow (Go `p` — delegate.go): shown only while
            // hints are toggled on. suggested < current numeric priority
            // means the issue is under-prioritized (should be more urgent)
            // -> up-arrow "increase"; suggested > current -> down-arrow.
            if app.show_priority_hints {
                if let Some(hints) = &app.priority_hints {
                    match hints.get(&row.id) {
                        Some(&suggested) if suggested < row.priority => {
                            spans.push(Span::styled(
                                "\u{2191}",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        }
                        Some(&suggested) if suggested > row.priority => {
                            spans.push(Span::styled(
                                "\u{2193}",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        }
                        _ => spans.push(Span::raw(" ")),
                    }
                } else {
                    spans.push(Span::raw(" "));
                }
            }
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
    // Semantic search mode (Ctrl+S): same bar shape, distinct prefix.
    if app.semantic_searching {
        let bar = Paragraph::new(Line::from(vec![
            Span::styled("s/", Style::default().fg(Color::Magenta)),
            Span::styled(&app.semantic_query, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Magenta)),
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
        push_hint(&mut hints, "s", "group");
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
    // Go auto-shows the agent-prompt modal when the repo has an AGENTS.md
    // (repo root = cwd, or the parent of `.beads/`).
    if detect_agents_md() {
        app.show_agent_prompts = true;
    }
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
        // Live reload (Go fsnotify; TUI_UX_PARITY_PLAN.md Phase F): piggyback
        // on this same ~500ms tick instead of a background notify watcher.
        app.check_for_reload();
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
    fn history_toggle_loads_lazily_and_switches_view() {
        let mut app = make_app(3);
        assert!(!app.history_loaded);
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.current_view, ViewMode::History);
        assert!(app.history_loaded, "h should trigger lazy history load");
        // Toggling back to List must not reset history_loaded (no reload
        // needed the second time `h` is pressed).
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.current_view, ViewMode::List);
        assert!(app.history_loaded);
    }

    /// Regression test for TUI_UX_PARITY_PLAN.md G11: j/k inside the
    /// History view used to be dead code (misplaced inside the Graph arm's
    /// else-branch), so pressing j/k while in History silently moved the
    /// *main list's* cursor instead of doing nothing meaningful in that
    /// view. Confirm the main list cursor is now left untouched.
    #[test]
    fn history_j_k_does_not_move_main_list_cursor() {
        let mut app = make_app(5);
        app.handle_key(KeyCode::Char('h')); // enter History
        let cursor_before = app.cursor;
        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char('k'));
        assert_eq!(
            app.cursor, cursor_before,
            "History view's j/k must not move the List view's cursor"
        );
    }

    #[test]
    fn time_travel_t_opens_revision_prompt_not_history() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('t'));
        assert_eq!(
            app.current_view,
            ViewMode::TimeTravel,
            "t must open the TimeTravel view, not silently alias to History"
        );
        assert_eq!(
            app.time_travel_prompt,
            Some(String::new()),
            "t must open the revision-input prompt (Go focusTimeTravelInput)"
        );
    }

    #[test]
    fn time_travel_prompt_typing_backspace_and_cancel() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('t'));
        for c in "HEAD~".chars() {
            app.handle_key(KeyCode::Char(c));
        }
        app.handle_key(KeyCode::Char('5'));
        assert_eq!(app.time_travel_prompt.as_deref(), Some("HEAD~5"));
        app.handle_key(KeyCode::Backspace);
        assert_eq!(app.time_travel_prompt.as_deref(), Some("HEAD~"));
        app.handle_key(KeyCode::Esc);
        assert_eq!(app.time_travel_prompt, None);
        assert_eq!(
            app.current_view,
            ViewMode::List,
            "cancelling with no result returns to list"
        );
    }

    #[test]
    fn time_travel_prompt_enter_empty_keeps_prompt_open() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('t'));
        app.handle_key(KeyCode::Enter);
        assert_eq!(
            app.time_travel_prompt,
            Some(String::new()),
            "empty revision must not run git or close the prompt"
        );
        assert!(app.time_travel_result.is_none());
        assert!(app.time_travel_error.is_none());
    }

    /// Synthetic "previous revision" with one added, one removed, one
    /// changed issue relative to `make_app(3)` (T-0 closed, T-1/T-2 open).
    fn seed_time_travel_previous(app: &App) -> Vec<bv_core::model::Issue> {
        let mut previous: Vec<bv_core::model::Issue> = app.issue_map.values().cloned().collect();
        previous.retain(|i| i.id != "T-1"); // → added now
        previous
            .iter_mut()
            .find(|i| i.id == "T-0")
            .expect("T-0 in map")
            .status = Status::Open; // → changed (now Closed)
        let mut extra = app.issue_map["T-2"].clone();
        extra.id = "X-9".to_string();
        previous.push(extra); // → removed now
        previous
    }

    #[test]
    fn time_travel_apply_computes_added_removed_changed() {
        let mut app = make_app(3);
        let previous = seed_time_travel_previous(&app);
        app.apply_time_travel_result(previous, "HEAD~5");
        let r = app.time_travel_result.as_ref().expect("result stored");
        assert_eq!(r.diff_ref, "HEAD~5");
        assert_eq!(r.added, vec!["T-1".to_string()]);
        assert_eq!(r.removed, vec!["X-9".to_string()]);
        assert_eq!(r.changed.len(), 1);
        assert_eq!(r.changed[0].id, "T-0");
        assert!(r.changed[0].status_change.is_some());
        assert!(app.time_travel_error.is_none());
        assert_eq!(app.time_travel_cursor, 0);
        assert!(
            app.status_msg.starts_with("Time-Travel vs HEAD~5"),
            "unexpected status: {}",
            app.status_msg
        );
    }

    #[test]
    fn time_travel_j_k_moves_diff_cursor_not_list() {
        let mut app = make_app(3);
        let previous = seed_time_travel_previous(&app);
        app.apply_time_travel_result(previous, "HEAD~5");
        app.current_view = ViewMode::TimeTravel;
        assert_eq!(app.time_travel_entry_count(), 3);
        let cursor_before = app.cursor;
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.time_travel_cursor, 1);
        assert_eq!(app.cursor, cursor_before);
        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char('j')); // clamped at last
        assert_eq!(app.time_travel_cursor, 2);
        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.time_travel_cursor, 1);
    }

    #[test]
    fn time_travel_esc_in_view_returns_to_list() {
        let mut app = make_app(3);
        app.current_view = ViewMode::TimeTravel;
        app.handle_key(KeyCode::Esc);
        assert_eq!(app.current_view, ViewMode::List);
        assert!(!app.quit_requested, "esc in a view must not quit");
    }

    #[test]
    fn time_travel_upper_t_runs_instant_head5_diff() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('T'));
        assert_eq!(app.current_view, ViewMode::TimeTravel);
        assert_eq!(app.time_travel_prompt, None);
        assert!(
            app.status_msg.starts_with("Time-Travel"),
            "T must run the diff (or report the error) in the status line, got: {}",
            app.status_msg
        );
    }

    /// App with two real labels: backend on T-1/T-2, frontend on T-4/T-5
    /// (make_app statuses: every 3rd id closed, so all four are open).
    fn make_labeled_app() -> App {
        let mut app = make_app(6);
        for id in ["T-1", "T-2"] {
            app.issue_map.get_mut(id).unwrap().labels = vec!["backend".to_string()];
        }
        for id in ["T-4", "T-5"] {
            app.issue_map.get_mut(id).unwrap().labels = vec!["frontend".to_string()];
        }
        app
    }

    #[test]
    fn label_dashboard_opens_and_loads_lazily() {
        let mut app = make_labeled_app();
        assert!(!app.label_health_loaded);
        app.handle_key(KeyCode::Char('['));
        assert_eq!(app.current_view, ViewMode::LabelDashboard);
        assert!(app.label_health_loaded, "[ should trigger lazy health load");
        assert_eq!(app.label_dashboard_count(), 2);
        // Toggling back must not reload.
        app.handle_key(KeyCode::Char('['));
        assert_eq!(app.current_view, ViewMode::List);
        assert!(app.label_health_loaded);
    }

    #[test]
    fn label_dashboard_j_k_moves_dashboard_cursor_not_list() {
        let mut app = make_labeled_app();
        app.handle_key(KeyCode::Char('['));
        assert_eq!(app.label_dashboard_count(), 2);
        let cursor_before = app.cursor;
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.label_dashboard_cursor, 1);
        assert_eq!(app.cursor, cursor_before);
        app.handle_key(KeyCode::Char('j')); // clamped at last
        assert_eq!(app.label_dashboard_cursor, 1);
        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.label_dashboard_cursor, 0);
    }

    #[test]
    fn label_dashboard_h_toggles_detail_not_history() {
        let mut app = make_labeled_app();
        app.handle_key(KeyCode::Char('['));
        app.handle_key(KeyCode::Char('h'));
        assert!(
            app.show_label_detail,
            "h in dashboard must open the detail modal"
        );
        assert_eq!(
            app.current_view,
            ViewMode::LabelDashboard,
            "h in dashboard must not switch to History"
        );
        app.handle_key(KeyCode::Char('h'));
        assert!(!app.show_label_detail);
    }

    #[test]
    fn label_dashboard_d_drilldown_enter_applies_label_filter() {
        let mut app = make_labeled_app();
        app.handle_key(KeyCode::Char('['));
        let first = app
            .selected_dashboard_label()
            .expect("a label is selected")
            .label
            .clone();
        app.handle_key(KeyCode::Char('d'));
        assert_eq!(
            app.label_drilldown.as_ref().map(|d| d.label.as_str()),
            Some(first.as_str())
        );
        // Typing narrows the drilldown rows; Enter jumps to List filtered.
        app.handle_key(KeyCode::Char('T'));
        assert!(
            app.drilldown_rows()
                .iter()
                .all(|(id, title, _)| id.contains('T') || title.contains('T')),
            "filter must narrow rows to the typed substring"
        );
        app.handle_key(KeyCode::Enter);
        assert_eq!(app.label_filter, Some(first));
        assert_eq!(app.current_view, ViewMode::List);
        assert!(app.label_drilldown.is_none());
    }

    #[test]
    fn label_dashboard_drilldown_esc_closes_stays_in_view() {
        let mut app = make_labeled_app();
        app.handle_key(KeyCode::Char('['));
        app.handle_key(KeyCode::Char('d'));
        assert!(app.label_drilldown.is_some());
        app.handle_key(KeyCode::Esc);
        assert!(app.label_drilldown.is_none());
        assert_eq!(app.current_view, ViewMode::LabelDashboard);
    }

    #[test]
    fn label_dashboard_esc_returns_to_list() {
        let mut app = make_labeled_app();
        app.handle_key(KeyCode::Char('['));
        app.handle_key(KeyCode::Esc);
        assert_eq!(app.current_view, ViewMode::List);
        assert!(!app.quit_requested, "esc in a view must not quit");
    }

    #[test]
    fn label_picker_opens_with_real_label_counts() {
        let mut app = make_app(4);
        app.rows[0].labels = vec!["backend".to_string()];
        app.rows[1].labels = vec!["backend".to_string(), "urgent".to_string()];
        app.handle_key(KeyCode::Char('l'));
        let picker = app.label_picker.as_ref().expect("picker should be built");
        assert!(picker.visible);
        let backend = picker.labels.iter().find(|l| l.name == "backend").unwrap();
        assert_eq!(backend.count, 2);
        let urgent = picker.labels.iter().find(|l| l.name == "urgent").unwrap();
        assert_eq!(urgent.count, 1);
    }

    #[test]
    fn label_picker_enter_applies_filter_and_closes() {
        let mut app = make_app(4);
        app.rows[0].labels = vec!["backend".to_string()];
        app.handle_key(KeyCode::Char('l'));
        app.handle_key(KeyCode::Enter);
        assert_eq!(app.label_filter, Some("backend".to_string()));
        assert!(!app.label_picker.as_ref().unwrap().visible);
    }

    #[test]
    fn recipe_picker_opens_with_six_builtin_recipes() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('\''));
        let picker = app
            .recipe_picker
            .as_ref()
            .expect("recipe picker should be built");
        assert!(picker.visible);
        assert_eq!(picker.recipes.len(), 6);
        assert_eq!(picker.recipes[0].name, "triage");
    }

    #[test]
    fn quick_wins_recipe_filters_by_priority_and_no_dependencies() {
        // make_app: status closes every 3rd id (i%3==0), priority = i%4.
        // With n=8: T-2 (open, p2) and T-7 (open, p3) are the only
        // open+priority>=2 issues. Give T-2 a dependency so it should be
        // excluded by quick-wins, leaving T-7 as the sole quick win.
        let mut app = make_app(8);
        let dep = bv_core::model::Dependency {
            issue_id: "T-2".into(),
            depends_on_id: "T-0".into(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: Default::default(),
            created_at: None,
            created_by: String::new(),
        };
        app.issue_map.get_mut("T-2").unwrap().dependencies = vec![dep];
        app.apply_recipe(4); // index 4 == "quick-wins" in default_recipe_defs()
        let ids: Vec<&str> = app
            .filtered_indices
            .iter()
            .map(|&i| app.rows[i].id.as_str())
            .collect();
        assert!(
            ids.contains(&"T-7"),
            "T-7 (priority 3, no deps, open) should be a quick win"
        );
        assert!(
            !ids.contains(&"T-2"),
            "T-2 has a dependency — quick-wins requires none"
        );
    }

    #[test]
    fn update_modal_only_shows_when_tag_present() {
        let mut app = make_app(2);
        app.handle_key(KeyCode::Char('U'));
        assert!(
            !app.show_update_modal,
            "U with no update_tag must not open the modal"
        );
        app.update_tag = Some("v9.9.9".to_string());
        app.handle_key(KeyCode::Char('U'));
        assert!(app.show_update_modal);
    }

    #[test]
    fn priority_hints_toggle_computes_once_and_sets_status() {
        let mut app = make_app(6);
        assert!(app.priority_hints.is_none());
        app.handle_key(KeyCode::Char('p'));
        assert!(app.show_priority_hints);
        assert!(
            app.priority_hints.is_some(),
            "toggling on should trigger the lazy compute"
        );
        assert!(
            app.status_msg.starts_with("Priority hints:"),
            "unexpected status message: {}",
            app.status_msg
        );
        let computed_once = app.priority_hints.clone();
        // Toggling off then on again must not recompute (same Some(map)),
        // matching the lazy-compute-once pattern used by History/LabelDashboard.
        app.handle_key(KeyCode::Char('p'));
        assert!(!app.show_priority_hints);
        assert!(app.status_msg.is_empty());
        app.handle_key(KeyCode::Char('p'));
        assert_eq!(
            app.priority_hints.as_ref().map(|m| m.len()),
            computed_once.map(|m| m.len())
        );
    }

    #[test]
    fn check_for_reload_detects_mtime_change() {
        let mut app = make_app(2);
        let dir = std::env::temp_dir().join(format!("bvr_watch_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("issues.jsonl");
        std::fs::write(&path, "").unwrap();
        let mtime1 = std::fs::metadata(&path).unwrap().modified().unwrap();
        app.watched_path = Some(path.clone());
        app.watched_mtime = Some(mtime1);
        assert!(
            !app.check_for_reload(),
            "unchanged mtime must not trigger a reload"
        );
        // Some filesystems have coarse mtime resolution — sleep briefly to
        // guarantee a strictly-later timestamp on the rewrite below.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "{}\n").unwrap();
        assert!(
            app.check_for_reload(),
            "changed mtime must trigger reload_from_disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_for_reload_is_noop_without_a_watched_path() {
        let mut app = make_app(2);
        app.watched_path = None;
        assert!(!app.check_for_reload());
    }

    #[test]
    fn quit_sets_flag() {
        let mut app = make_app(3);
        app.handle_key(KeyCode::Char('q'));
        assert!(app.quit_requested);
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle_key(KeyCode::Char(c));
        }
    }

    #[test]
    fn fuzzy_search_surfaces_substring_match_ranked() {
        let mut app = make_app(9);
        app.handle_key(KeyCode::Char('/'));
        // Lowercase on purpose: matching must be case-insensitive, and the
        // old contains-match ("Issue 1") must still surface.
        type_text(&mut app, "issue 1");
        let t1 = app.rows.iter().position(|r| r.id == "T-1").unwrap();
        assert_eq!(
            app.filtered_indices,
            vec![t1],
            "only T-1 contains '1'; fuzzy must not invent matches"
        );
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn fuzzy_search_empty_query_restores_all_rows() {
        let mut app = make_app(4);
        app.handle_key(KeyCode::Char('/'));
        type_text(&mut app, "Issue");
        assert!(!app.filtered_indices.is_empty());
        for _ in 0..5 {
            app.handle_key(KeyCode::Backspace);
        }
        assert_eq!(app.filtered_indices.len(), app.rows.len());
    }

    #[test]
    fn semantic_search_ranks_by_embedding_overlap() {
        let mut app = make_app(3);
        app.rows[0].title = "database migration".to_string();
        app.rows[1].title = "database backup".to_string();
        app.rows[2].title = "button color".to_string();
        assert!(app.handle_ctrl_key(KeyCode::Char('s')));
        assert!(app.semantic_searching);
        type_text(&mut app, "database");
        let mut ids: Vec<&str> = app
            .filtered_indices
            .iter()
            .map(|&i| app.rows[i].id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec!["T-0", "T-1"],
            "token-overlapping rows rank; disjoint row is hidden"
        );
    }

    #[test]
    fn semantic_esc_clears_and_exits() {
        let mut app = make_app(3);
        app.handle_ctrl_key(KeyCode::Char('s'));
        type_text(&mut app, "x");
        assert!(app.semantic_searching);
        app.handle_key(KeyCode::Esc);
        assert!(!app.semantic_searching);
        assert!(app.semantic_query.is_empty());
        assert_eq!(app.filtered_indices.len(), app.rows.len());
    }

    #[test]
    fn semantic_enter_accepts_back_to_standard_order() {
        let mut app = make_app(3);
        app.handle_ctrl_key(KeyCode::Char('S'));
        type_text(&mut app, "Issue");
        app.handle_key(KeyCode::Enter);
        assert!(!app.semantic_searching);
        assert_eq!(app.filtered_indices.len(), app.rows.len());
    }
    /// Regression: mixed-case needles once panicked inside nucleo-matcher
    /// 0.3.1 (uppercase char matchable only via case folding passed the
    /// prefilter but tripped the optimal-matcher's reject assert). The
    /// needle is pre-folded now, so every ASCII prefix must rank without
    /// panicking and the full word must still find its row.
    #[test]
    fn fuzzy_search_mixed_case_prefixes_never_panic() {
        for q in [
            "I", "Is", "Iss", "Issu", "Issue", "i", "is", "issue", "issue 1", "1", "T-", "0",
        ] {
            let mut app = make_app(4);
            app.search_query = q.to_string();
            app.apply_search();
        }
        let mut app = make_app(4);
        app.search_query = "ISSUE 2".to_string();
        app.apply_search();
        let t2 = app.rows.iter().position(|r| r.id == "T-2").unwrap();
        assert_eq!(app.filtered_indices, vec![t2]);
    }
    #[cfg(test)]
    mod cleanup_tests {
        use super::*;

        #[test]
        fn board_group_cycle_resets_column() {
            use crate::views::board::SwimlaneMode;
            let mut app = make_app(6);
            app.handle_key(KeyCode::Char('b'));
            assert_eq!(app.current_view, ViewMode::Board);
            app.handle_key(KeyCode::Char('s'));
            assert_eq!(app.board_mode, SwimlaneMode::Priority);
            assert_eq!(app.board_column, 0);
            assert!(app.status_msg.contains("Priority"));
            app.handle_key(KeyCode::Char('s'));
            assert_eq!(app.board_mode, SwimlaneMode::Type);
            app.handle_key(KeyCode::Char('s'));
            assert_eq!(app.board_mode, SwimlaneMode::Status);
        }

        #[test]
        fn board_column_nav_and_digit_jump() {
            let mut app = make_app(6);
            app.handle_key(KeyCode::Char('b'));
            let cursor_before = app.cursor;
            // Status mode has 4 columns; `l` advances, `h` retreats, clamped.
            app.handle_key(KeyCode::Char('l'));
            assert_eq!(app.board_column, 1);
            app.handle_key(KeyCode::Char('h'));
            assert_eq!(app.board_column, 0);
            app.handle_key(KeyCode::Char('h')); // clamped at first
            assert_eq!(app.board_column, 0);
            // Priority mode has exactly 5 columns (P0-P4).
            app.handle_key(KeyCode::Char('s'));
            app.handle_key(KeyCode::Char('5'));
            assert_eq!(app.board_column, 4);
            app.handle_key(KeyCode::Char('6')); // no-op, clamped
            assert_eq!(app.board_column, 4);
            app.handle_key(KeyCode::Char('3'));
            assert_eq!(app.board_column, 2);
            assert_eq!(
                app.cursor, cursor_before,
                "board nav must not move the list cursor"
            );
        }

        #[test]
        fn board_s_does_not_sort() {
            let mut app = make_app(3);
            app.handle_key(KeyCode::Char('b'));
            let start = app.sort_mode;
            app.handle_key(KeyCode::Char('s'));
            assert_eq!(app.sort_mode, start, "s in Board cycles grouping, not sort");
        }

        #[test]
        fn actionable_lazy_loads_and_jk_routes_to_items() {
            let mut app = make_app(6);
            assert!(app.actionable.is_none());
            app.handle_key(KeyCode::Char('F'));
            assert_eq!(app.current_view, ViewMode::Actionable);
            assert!(
                app.actionable.is_some(),
                "F must lazy-build actionable state"
            );
            let cursor_before = app.cursor;
            app.handle_key(KeyCode::Char('j'));
            app.handle_key(KeyCode::Char('k'));
            assert_eq!(
                app.cursor, cursor_before,
                "actionable j/k must not move the list cursor"
            );
        }

        #[test]
        fn tutorial_jk_turns_pages_and_backtick_resets() {
            let mut app = make_app(3);
            app.handle_key(KeyCode::Char('`'));
            assert_eq!(app.current_view, ViewMode::Tutorial);
            let cursor_before = app.cursor;
            app.handle_key(KeyCode::Char('j'));
            assert_eq!(app.tutorial.as_ref().unwrap().current_page, 1);
            assert_eq!(app.cursor, cursor_before);
            app.handle_key(KeyCode::Char('k'));
            assert_eq!(app.tutorial.as_ref().unwrap().current_page, 0);
            // Re-entering restarts the walkthrough instead of resuming mid-way.
            app.handle_key(KeyCode::Char('`'));
            app.handle_key(KeyCode::Char('j'));
            app.handle_key(KeyCode::Char('`'));
            app.handle_key(KeyCode::Char('`'));
            assert_eq!(app.tutorial.as_ref().unwrap().current_page, 0);
        }

        #[test]
        fn sprint_lazy_loads_jk_and_velocity_toggle() {
            let mut app = make_app(3);
            assert!(app.sprint.is_none());
            app.handle_key(KeyCode::Char('P'));
            assert_eq!(app.current_view, ViewMode::Sprint);
            assert!(app.sprint.is_some(), "P must lazy-load sprints");
            let cursor_before = app.cursor;
            app.handle_key(KeyCode::Char('j'));
            app.handle_key(KeyCode::Char('k'));
            assert_eq!(
                app.cursor, cursor_before,
                "sprint j/k must not move the list cursor"
            );
            app.handle_key(KeyCode::Char('v'));
            assert!(app.show_velocity);
            app.handle_key(KeyCode::Esc);
            assert!(!app.show_velocity, "esc closes the overlay first");
            assert_eq!(app.current_view, ViewMode::Sprint);
            app.handle_key(KeyCode::Esc);
            assert_eq!(app.current_view, ViewMode::List);
            assert!(!app.quit_requested, "esc in a view must not quit");
        }

        #[test]
        fn agent_prompt_modal_open_navigate_close() {
            let mut app = make_app(3);
            app.show_agent_prompts = false; // isolate from AGENTS.md auto-detect
            app.handle_key(KeyCode::Char('g'));
            assert!(app.show_agent_prompts);
            app.handle_key(KeyCode::Char('j'));
            assert_eq!(app.agent_prompt_cursor, 1);
            app.handle_key(KeyCode::Char('k'));
            assert_eq!(app.agent_prompt_cursor, 0);
            app.handle_key(KeyCode::Esc);
            assert!(!app.show_agent_prompts);
        }

        #[test]
        fn esc_returns_toggle_views_to_list() {
            for key in ['b', 'E', 'i', 'F', '`', 'P', '!', 'f', 'A'] {
                let mut app = make_app(3);
                app.show_agent_prompts = false;
                app.handle_key(KeyCode::Char(key));
                assert_ne!(
                    app.current_view,
                    ViewMode::List,
                    "key {key} should leave List"
                );
                app.handle_key(KeyCode::Esc);
                assert_eq!(
                    app.current_view,
                    ViewMode::List,
                    "esc should close the {key} view"
                );
                assert!(!app.quit_requested, "esc in a view must not quit");
            }
        }
    }
}

pub mod actionable;
pub mod agent_prompt_modal;

pub mod chrome;
pub mod context;
pub mod context_help;
pub mod detail;
pub mod helpers;
pub mod keybindings;
pub mod markdown;
pub mod theme;
pub mod tutorial;
pub mod update_modal;
pub mod views;
pub mod visuals;
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
