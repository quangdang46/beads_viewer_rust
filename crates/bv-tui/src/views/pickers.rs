//! Picker/modal views — label picker, recipe picker, repo picker.
//!
//! Port of Go `pkg/ui/label_picker.go`, `recipe_picker.go`, `repo_picker.go`.
//! These are modal overlays within the TUI, toggled by keybindings.
//!
//! Scope cut vs Go: fuzzy search input (textinput.Model from bubbles),
//! full keyboard-driven selection, and the recipe picker's YAML recipe
//! loading are simplified. Core selection and filtering behavior is real.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// A label option for the label picker.
#[derive(Debug, Clone)]
pub struct LabelOption {
    pub name: String,
    pub count: usize,
}

/// Label picker state.
pub struct LabelPicker {
    pub labels: Vec<LabelOption>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub filter_text: String,
    pub visible: bool,
}

impl LabelPicker {
    pub fn new(labels: Vec<LabelOption>) -> Self {
        let filtered: Vec<usize> = (0..labels.len()).collect();
        LabelPicker { labels, filtered, selected: 0, filter_text: String::new(), visible: false }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.selected = 0;
            self.filter();
        }
    }

    pub fn update_filter(&mut self, text: &str) {
        self.filter_text = text.to_string();
        self.filter();
    }

    fn filter(&mut self) {
        let query = self.filter_text.to_lowercase();
        self.filtered = self.labels
            .iter()
            .enumerate()
            .filter(|(_, l)| query.is_empty() || l.name.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_label(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(|&i| self.labels[i].name.as_str())
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup_width = 40.min(area.width.saturating_sub(4));
        let popup_height = (self.filtered.len() as u16 + 4).min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup = Rect { x, y, width: popup_width, height: popup_height };

        f.render_widget(Clear, popup);

        let mut lines: Vec<Line> = Vec::new();
        if !self.filter_text.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Filter: {}", self.filter_text),
                Style::default().fg(Color::DarkGray),
            )));
        }

        let display_limit = (popup_height.saturating_sub(3) as usize).max(1);
        let start = self.selected.saturating_sub(display_limit / 2);
        let end = (start + display_limit).min(self.filtered.len());

        for &idx in &self.filtered[start..end] {
            let label = &self.labels[idx];
            let style = if idx == self.filtered[self.selected] {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("  {} ({})", label.name, label.count),
                style,
            )));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Label Filter ")
            .border_style(Style::default().fg(Color::Cyan));

        let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
        f.render_widget(para, popup);
    }
}

/// A recipe option for the recipe picker.
#[derive(Debug, Clone)]
pub struct RecipeOption {
    pub name: String,
    pub description: String,
    pub labels: Vec<String>,
}

/// Recipe picker state.
pub struct RecipePicker {
    pub recipes: Vec<RecipeOption>,
    pub selected: usize,
    pub visible: bool,
}

impl RecipePicker {
    pub fn new(recipes: Vec<RecipeOption>) -> Self {
        RecipePicker { recipes, selected: 0, visible: false }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible { self.selected = 0; }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.recipes.len() {
            self.selected += 1;
        }
    }

    pub fn selected_recipe(&self) -> Option<&RecipeOption> {
        self.recipes.get(self.selected)
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup_width = 50.min(area.width.saturating_sub(4));
        let popup_height = (self.recipes.len() as u16 + 4).min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup = Rect { x, y, width: popup_width, height: popup_height };

        f.render_widget(Clear, popup);

        let mut lines: Vec<Line> = Vec::new();
        for (i, recipe) in self.recipes.iter().enumerate() {
            let style = if i == self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("  {} — {}", recipe.name, recipe.description),
                style,
            )));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Recipe Picker ")
            .border_style(Style::default().fg(Color::Cyan));

        let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
        f.render_widget(para, popup);
    }
}

/// A repo option for the workspace repo picker.
#[derive(Debug, Clone)]
pub struct RepoOption {
    pub name: String,
    pub path: String,
    pub issue_count: usize,
}

/// Repo picker state.
pub struct RepoPicker {
    pub repos: Vec<RepoOption>,
    pub selected: usize,
    pub visible: bool,
}

impl RepoPicker {
    pub fn new(repos: Vec<RepoOption>) -> Self {
        RepoPicker { repos, selected: 0, visible: false }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible { self.selected = 0; }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.repos.len() {
            self.selected += 1;
        }
    }

    pub fn selected_repo(&self) -> Option<&RepoOption> {
        self.repos.get(self.selected)
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup_width = 50.min(area.width.saturating_sub(4));
        let popup_height = (self.repos.len() as u16 + 4).min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup = Rect { x, y, width: popup_width, height: popup_height };

        f.render_widget(Clear, popup);

        let mut lines: Vec<Line> = Vec::new();
        for (i, repo) in self.repos.iter().enumerate() {
            let style = if i == self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("  {} — {} issues", repo.name, repo.issue_count),
                style,
            )));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Workspace Repo Picker ")
            .border_style(Style::default().fg(Color::Cyan));

        let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
        f.render_widget(para, popup);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_picker_navigation() {
        let labels = vec![
            LabelOption { name: "backend".into(), count: 5 },
            LabelOption { name: "frontend".into(), count: 3 },
            LabelOption { name: "urgent".into(), count: 2 },
        ];
        let mut picker = LabelPicker::new(labels);
        assert_eq!(picker.selected_label(), Some("backend"));
        picker.move_down();
        assert_eq!(picker.selected_label(), Some("frontend"));
        picker.move_up();
        assert_eq!(picker.selected_label(), Some("backend"));
    }

    #[test]
    fn label_picker_filter() {
        let labels = vec![
            LabelOption { name: "backend".into(), count: 5 },
            LabelOption { name: "frontend".into(), count: 3 },
        ];
        let mut picker = LabelPicker::new(labels);
        picker.update_filter("front");
        assert_eq!(picker.filtered.len(), 1);
        assert_eq!(picker.selected_label(), Some("frontend"));
    }

    #[test]
    fn recipe_picker_navigation() {
        let recipes = vec![
            RecipeOption { name: "R1".into(), description: "desc".into(), labels: vec![] },
            RecipeOption { name: "R2".into(), description: "desc".into(), labels: vec![] },
        ];
        let mut picker = RecipePicker::new(recipes);
        assert_eq!(picker.selected_recipe().unwrap().name, "R1");
        picker.move_down();
        assert_eq!(picker.selected_recipe().unwrap().name, "R2");
    }
}
