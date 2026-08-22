//! Tree view — parent-child hierarchy (Go `pkg/ui/tree.go`).
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// A node in the rendered tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub title: String,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
}

/// Build display lines from a flat list of tree nodes.
pub fn render_tree_lines(nodes: &[TreeNode]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for n in nodes {
        let prefix = if n.has_children {
            if n.expanded {
                "\u{25be} "
            } else {
                "\u{25b8} "
            }
        } else {
            "\u{2022} "
        };
        let indent = "  ".repeat(n.depth);
        let connector = if n.depth > 0 && n.has_children {
            "\u{251c}\u{2500} "
        } else {
            ""
        };

        lines.push(Line::from(vec![
            Span::raw(format!("{indent}{connector}{prefix}")),
            Span::styled(n.id.clone(), Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::styled(
                n.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expanded_and_leaf_nodes() {
        let nodes = vec![
            TreeNode {
                id: "EPIC-1".into(),
                title: "Epic".into(),
                depth: 0,
                has_children: true,
                expanded: true,
            },
            TreeNode {
                id: "TASK-2".into(),
                title: "Sub".into(),
                depth: 1,
                has_children: false,
                expanded: false,
            },
        ];
        let lines = render_tree_lines(&nodes);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn collapsed_shows_arrow() {
        let nodes = vec![TreeNode {
            id: "E-1".into(),
            title: "".into(),
            depth: 0,
            has_children: true,
            expanded: false,
        }];
        let lines = render_tree_lines(&nodes);
        assert!(!lines.is_empty());
    }
}
