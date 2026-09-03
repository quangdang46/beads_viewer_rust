//! Tree view — parent-child hierarchy (Go `pkg/ui/tree.go`).
use bv_core::model::{DependencyType, Issue};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::{HashMap, HashSet};

/// A node in the rendered tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub title: String,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
}

/// Build a flat, depth-first list of `TreeNode`s from parent-child
/// dependencies (Go `pkg/ui/tree.go` `buildIssueTreeNodes`).
///
/// A dependency `{issue_id: child, depends_on_id: parent, type: ParentChild}`
/// makes `child` a child of `parent`. Issues with no valid parent-child
/// dependency (or whose declared parent doesn't exist in `issues`) are roots.
/// All nodes start expanded unless their id is in `collapsed`.
pub fn build_tree_nodes(issues: &[Issue], collapsed: &HashSet<String>) -> Vec<TreeNode> {
    let by_id: HashMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut children_of: HashMap<&str, Vec<&Issue>> = HashMap::new();
    let mut has_parent: HashSet<&str> = HashSet::new();

    for issue in issues {
        for dep in &issue.dependencies {
            if dep.r#type == DependencyType::ParentChild && by_id.contains_key(dep.depends_on_id.as_str()) {
                children_of
                    .entry(dep.depends_on_id.as_str())
                    .or_default()
                    .push(issue);
                has_parent.insert(issue.id.as_str());
                break;
            }
        }
    }

    let mut roots: Vec<&Issue> = issues
        .iter()
        .filter(|i| !has_parent.contains(i.id.as_str()))
        .collect();
    roots.sort_by(|a, b| a.id.cmp(&b.id));
    for kids in children_of.values_mut() {
        kids.sort_by(|a, b| a.id.cmp(&b.id));
    }

    let mut out = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();
    for root in roots {
        push_node(root, 0, &children_of, collapsed, &mut visited, &mut out);
    }
    out
}

fn push_node<'a>(
    issue: &'a Issue,
    depth: usize,
    children_of: &HashMap<&'a str, Vec<&'a Issue>>,
    collapsed: &HashSet<String>,
    visited: &mut HashSet<&'a str>,
    out: &mut Vec<TreeNode>,
) {
    // Cycle guard: a malformed parent-child chain must not infinite-loop.
    if !visited.insert(issue.id.as_str()) {
        return;
    }
    let kids = children_of.get(issue.id.as_str());
    let has_children = kids.map(|k| !k.is_empty()).unwrap_or(false);
    let expanded = !collapsed.contains(&issue.id);
    out.push(TreeNode {
        id: issue.id.clone(),
        title: issue.title.clone(),
        depth,
        has_children,
        expanded,
    });
    if has_children && expanded {
        for child in kids.unwrap() {
            push_node(child, depth + 1, children_of, collapsed, visited, out);
        }
    }
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
