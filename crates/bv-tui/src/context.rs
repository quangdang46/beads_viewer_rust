//! Context management — tracks current focus, selected issue, and view state.
//! Port of Go `pkg/ui/context.go`.

/// Application context for the TUI.
#[derive(Debug, Clone)]
pub struct AppContext {
    /// Currently focused view/panel.
    pub focus: Focus,
    /// Selected issue ID (if any).
    pub selected_issue: Option<String>,
    /// Current workspace root path.
    pub workspace_root: Option<String>,
    /// Active label filter.
    pub label_filter: Option<String>,
    /// Active repo filter (workspace mode).
    pub repo_filter: Option<String>,
    /// Search query active.
    pub search_query: Option<String>,
    /// Sort mode.
    pub sort_mode: SortMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
    Board,
    Tree,
    Graph,
    Insights,
    Alerts,
    FlowMatrix,
    Attention,
    Sprint,
    History,
    Tutorial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Default,
    Priority,
    Status,
    Title,
    Created,
    Updated,
}

impl Default for AppContext {
    fn default() -> Self {
        AppContext {
            focus: Focus::List,
            selected_issue: None,
            workspace_root: None,
            label_filter: None,
            repo_filter: None,
            search_query: None,
            sort_mode: SortMode::Default,
        }
    }
}

impl AppContext {
    pub fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
    }

    pub fn select_issue(&mut self, issue_id: Option<String>) {
        self.selected_issue = issue_id;
    }

    pub fn cycle_sort(&mut self) {
        self.sort_mode = match self.sort_mode {
            SortMode::Default => SortMode::Priority,
            SortMode::Priority => SortMode::Status,
            SortMode::Status => SortMode::Title,
            SortMode::Title => SortMode::Created,
            SortMode::Created => SortMode::Updated,
            SortMode::Updated => SortMode::Default,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context() {
        let ctx = AppContext::default();
        assert_eq!(ctx.focus, Focus::List);
        assert!(ctx.selected_issue.is_none());
    }

    #[test]
    fn cycle_sort() {
        let mut ctx = AppContext::default();
        assert_eq!(ctx.sort_mode, SortMode::Default);
        ctx.cycle_sort();
        assert_eq!(ctx.sort_mode, SortMode::Priority);
        ctx.cycle_sort();
        assert_eq!(ctx.sort_mode, SortMode::Status);
    }
}
