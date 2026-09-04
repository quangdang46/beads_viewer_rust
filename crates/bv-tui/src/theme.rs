//! Theme system — Dracula-inspired color palette for the TUI.
//! Port of Go `pkg/ui/theme.go` + `pkg/ui/styles.go`.

use ratatui::style::Color;

/// Theme colors for the TUI.
#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub primary: Color,     // Purple - primary actions
    pub secondary: Color,   // Comment gray
    pub accent: Color,      // Cyan - accents
    pub open: Color,        // Green - open status
    pub in_progress: Color, // Cyan - in progress
    pub blocked: Color,     // Red - blocked
    pub closed: Color,      // Dark gray - closed
    pub warning: Color,     // Yellow-orange
    pub highlight: Color,   // Selection background
    pub muted: Color,       // Muted text
}

impl Default for Theme {
    fn default() -> Self {
        // Dracula palette
        Theme {
            bg: Color::Rgb(40, 42, 54),
            fg: Color::Rgb(248, 248, 242),
            primary: Color::Rgb(189, 147, 249),     // Purple
            secondary: Color::Rgb(98, 114, 164),    // Comment
            accent: Color::Rgb(139, 233, 253),      // Cyan
            open: Color::Rgb(80, 250, 123),         // Green
            in_progress: Color::Rgb(139, 233, 253), // Cyan
            blocked: Color::Rgb(255, 85, 85),       // Red
            closed: Color::Rgb(98, 114, 164),       // Comment
            warning: Color::Rgb(255, 184, 108),     // Orange
            highlight: Color::Rgb(68, 71, 90),      // Current line
            muted: Color::Rgb(98, 114, 164),        // Comment
        }
    }
}

impl Theme {
    /// Get the status color for an issue status.
    pub fn status_color(&self, status: &Status) -> Color {
        match status {
            Status::Open => self.open,
            Status::InProgress => self.in_progress,
            Status::Blocked => self.blocked,
            Status::Closed | Status::Tombstone => self.closed,
            _ => self.muted,
        }
    }

    /// Get the status icon for an issue status.
    pub fn status_icon(&self, status: &Status) -> &'static str {
        match status {
            Status::Open => "○",
            Status::InProgress => "◐",
            Status::Blocked => "◈",
            Status::Closed => "●",
            Status::Tombstone => "✝",
            Status::Deferred => "◎",
            Status::Draft => "◇",
            Status::Review => "◈",
            _ => "?",
        }
    }
}

use bv_core::model::Status;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_has_all_status_colors() {
        let theme = Theme::default();
        // Just verify they don't panic
        let _ = theme.status_color(&Status::Open);
        let _ = theme.status_icon(&Status::Blocked);
    }
}
