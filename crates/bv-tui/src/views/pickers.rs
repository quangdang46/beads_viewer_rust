//! Picker views — label picker, recipe picker, repo picker.
//!
//! Port of Go `pkg/ui/label_picker.go`, `recipe_picker.go`, `repo_picker.go`.
//! These are modal overlays within the TUI, toggled by keybindings.
//!
//! Scope note: Go draws these with lipgloss/bubbles and embeds a
//! `textinput.Model` in the label picker. Ratatui has no `textinput`, so
//! `filter_text` is fed one key at a time by `App::handle_label_picker_key`
//! (lib.rs). Everything else — the fuzzy scorer, the sort orders, the row
//! chrome, the footers — is ported line-for-line from the Go above and is
//! cited per function.
//!
//! The recipe picker's *list* still comes from `App::open_recipe_picker`
//! (lib.rs), which passes `default_recipe_defs()`. Go passes
//! `recipeLoader.List()` (`model.go:1793`), i.e. the embedded defaults plus
//! any user-level and project-level `recipes.yaml`. Wiring `bv-recipe`'s
//! `Loader` into `open_recipe_picker` needs a `bv-recipe` dependency plus a
//! lib.rs edit — see the report; `RecipeOption` is already shaped to carry
//! what a `Loader` row needs.

use std::collections::{BTreeSet, HashSet};

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
    Frame,
};

// ============================================================================
// helpers — Go pkg/ui/helpers.go
// ============================================================================

/// East Asian Wide + Fullwidth ranges, as classified by go-runewidth.
/// These render in two terminal cells.
const WIDE_RANGES: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x1B000, 0x1B001),
    (0x1F200, 0x1F251),
    (0x1F300, 0x1F5FF),
    (0x1F600, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x20000, 0x3FFFD),
];

/// Zero-width ranges: combining diacritics, ZWSP/ZWNJ, variation selectors.
const ZERO_WIDTH_RANGES: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x1AB0, 0x1AFF),
    (0x200B, 0x200F),
    (0x20D0, 0x20FF),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
];

fn in_ranges(cp: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|(lo, hi)| cp >= *lo && cp <= *hi)
}

/// Terminal cell width of a single char — the stand-in for go-runewidth's
/// `RuneWidth`. Go's `truncateRunesHelper` measures with
/// `runewidth.StringWidth`; without a runewidth crate we reproduce its
/// width classes directly.
fn rune_width(c: char) -> usize {
    let cp = c as u32;
    if cp < 0x20 || cp == 0x7F {
        return 0; // control
    }
    if in_ranges(cp, ZERO_WIDTH_RANGES) {
        return 0; // combining / zero-width
    }
    if in_ranges(cp, WIDE_RANGES) {
        return 2; // east asian wide + fullwidth
    }
    1
}

/// Visual width of `s` in terminal cells (Go `runewidth.StringWidth`).
pub fn string_width(s: &str) -> usize {
    s.chars().map(rune_width).sum()
}

/// Truncate `s` to `max_width` cells, appending `suffix` when it does not fit.
/// Port of Go `truncateRunesHelper` (`pkg/ui/helpers.go:42-60`).
///
/// Go: `runewidth.Truncate(s, targetWidth, "") + suffix`, and when the suffix
/// alone is wider than `maxWidth` the suffix itself is truncated.
pub fn truncate_runes_helper(s: &str, max_width: usize, suffix: &str) -> String {
    if max_width == 0 {
        return String::new();
    }
    if string_width(s) <= max_width {
        return s.to_string();
    }
    let suffix_width = string_width(suffix);
    if suffix_width > max_width {
        // Even the suffix is too wide — truncate the suffix.
        return take_width(suffix, max_width);
    }
    let target = max_width - suffix_width;
    format!("{}{suffix}", take_width(s, target))
}

/// Longest prefix of `s` whose visual width is at most `max_width` cells.
fn take_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = rune_width(c);
        if w + cw > max_width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out
}

/// Centered popup rect for a box `box_width` cells wide and `content_lines`
/// rows of content. Go centers with `lipgloss.Place` inside a
/// `Padding(1, 2)` + `RoundedBorder()` style, so the drawn box is
/// `box_width + 2` columns by `content_lines + 4` rows.
fn centered_popup(area: Rect, box_width: u16, content_lines: usize) -> Rect {
    let w = (box_width as u32 + 2).min(area.width as u32);
    let h = (content_lines as u32 + 4).min(area.height as u32);
    let w = w as u16;
    let h = h as u16;
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// Outer block: rounded border in `Primary`, `Padding(1, 2)` — Go's
/// `boxStyle` shared by all three pickers.
fn box_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::new(2, 2, 1, 1))
}

/// The dim-italic footer style Go's `footerStyle` / `ColorFooterHint` gives.
fn footer_style() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC)
}

/// The `dim italic` style Go uses for empty/secondary body text.
fn dim_italic() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC)
}

// ============================================================================
// Label picker — Go pkg/ui/label_picker.go
// ============================================================================

/// A label option for the label picker.
#[derive(Debug, Clone)]
pub struct LabelOption {
    pub name: String,
    pub count: usize,
}

/// Sort labels by count descending, then alphabetically for ties.
/// Port of Go `sortLabelsByCountDesc` (`label_picker.go:57-69`).
pub fn sort_labels_by_count_desc(labels: &[LabelOption]) -> Vec<LabelOption> {
    let mut sorted = labels.to_vec();
    sorted.sort_by(|a, b| match b.count.cmp(&a.count) {
        std::cmp::Ordering::Equal => a.name.cmp(&b.name),
        other => other,
    });
    sorted
}

/// Score how well `query` matches `label` (0 = no match). fzf-style:
/// consecutive-match and word-boundary bonuses.
///
/// Port of Go `fuzzyScore` (`label_picker.go:166-220`). Go indexes the
/// lowercased strings as **bytes** (`label[li]`, `len(query)`), so this walks
/// bytes too — that is what makes a mid-codepoint match score at all.
pub fn fuzzy_score(label: &str, query: &str) -> i32 {
    let label = label.to_lowercase();
    let query = query.to_lowercase();

    // Exact match gets highest score.
    if label == query {
        return 1000;
    }
    // Prefix match gets high score.
    if label.starts_with(&query) {
        return 500 + query.len() as i32;
    }
    // Contains match.
    if label.contains(&query) {
        return 200 + query.len() as i32;
    }

    // Fuzzy subsequence match.
    let lb = label.as_bytes();
    let qb = query.as_bytes();
    let mut li = 0usize;
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut consecutive = 0i32;
    let mut last_match_idx: i64 = -1;

    while li < lb.len() && qi < qb.len() {
        if lb[li] == qb[qi] {
            qi += 1;
            let mut match_score = 10i32;

            // Bonus for consecutive matches.
            if last_match_idx == li as i64 - 1 {
                consecutive += 1;
                match_score += consecutive * 5;
            } else {
                consecutive = 0;
            }

            // Bonus for word boundary match. Go evaluates
            // `unicode.IsLetter(rune(label[li-1]))` on a single BYTE, so any
            // continuation byte (0x80..=0xBF) reads as a non-letter and earns
            // the bonus; only ASCII letters suppress it.
            if li == 0 || !byte_is_letter(lb[li - 1]) {
                match_score += 15;
            }

            score += match_score;
            last_match_idx = li as i64;
        }
        li += 1;
    }

    // Only count as match if all query chars were found.
    if qi == qb.len() {
        return score;
    }
    0
}

/// `unicode.IsLetter(rune(b))` for a single byte, as Go calls it above.
///
/// Go widens the byte to a `rune`, so 0x80..=0xFF become the Latin-1
/// codepoints U+0080..U+00FF and many of them ARE letters (e.g. 0xC3 = 'Ã').
/// `u8 as char` widens identically, so do not restrict this to ASCII.
fn byte_is_letter(b: u8) -> bool {
    char::from(b).is_alphabetic()
}

/// Label picker state (Go `LabelPickerModel`).
pub struct LabelPicker {
    /// `allLabels` — already sorted by count desc (Go `NewLabelPickerModel`
    /// stores `sortLabelsByCountDesc(labels, counts)`).
    pub labels: Vec<LabelOption>,
    /// `filtered` — indices into `labels`, ordered by fuzzy score when a
    /// query is present and by count desc when it is not.
    pub filtered: Vec<usize>,
    /// `selectedIndex`.
    pub selected: usize,
    /// The `textinput` value, fed one key at a time.
    pub filter_text: String,
    pub visible: bool,
}

impl LabelPicker {
    /// Go `NewLabelPickerModel` (`label_picker.go:27-44`): the caller passes
    /// labels already sorted, and the model re-sorts anyway; `filtered`
    /// starts as the full sorted list.
    pub fn new(labels: Vec<LabelOption>) -> Self {
        let sorted = sort_labels_by_count_desc(&labels);
        let filtered: Vec<usize> = (0..sorted.len()).collect();
        LabelPicker {
            labels: sorted,
            filtered,
            selected: 0,
            filter_text: String::new(),
            visible: false,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.selected = 0;
            self.filter();
        }
    }

    /// Go `UpdateInput` (`label_picker.go:108-113`) — sets the whole value.
    pub fn update_filter(&mut self, text: &str) {
        self.filter_text = text.to_string();
        self.filter();
    }

    /// Go `Reset` (`label_picker.go:116-119`).
    pub fn reset(&mut self) {
        self.filter_text.clear();
        self.filter();
    }

    /// Go `filterLabels` (`label_picker.go:122-162`): an empty (trimmed,
    /// lowercased) query restores the count-sorted list, otherwise every
    /// label is scored and the score>0 hits are sorted by score desc then
    /// label asc. The selection is *clamped*, not reset.
    fn filter(&mut self) {
        let query = self.filter_text.trim().to_lowercase();
        if query.is_empty() {
            self.filtered = (0..self.labels.len()).collect();
            self.selected = 0;
            return;
        }

        let mut matches: Vec<(usize, i32)> = self
            .labels
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let score = fuzzy_score(&l.name, &query);
                if score > 0 {
                    Some((i, score))
                } else {
                    None
                }
            })
            .collect();

        matches.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| self.labels[a.0].name.cmp(&self.labels[b.0].name))
        });

        self.filtered = matches.into_iter().map(|(i, _)| i).collect();

        // Keep selection in bounds.
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    /// Go `MoveUp` (`label_picker.go:85-90`).
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Go `MoveDown` (`label_picker.go:92-97`).
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// Go `SelectedLabel` (`label_picker.go:99-104`).
    pub fn selected_label(&self) -> Option<&str> {
        self.filtered
            .get(self.selected)
            .map(|&i| self.labels[i].name.as_str())
    }

    /// Go `FilteredCount` (`label_picker.go:363-366`).
    pub fn filtered_count(&self) -> usize {
        self.filtered.len()
    }

    /// Go `boxWidth` math (`label_picker.go:234-240`).
    fn box_width(term_width: i32) -> i32 {
        let mut w = 40;
        if term_width < 50 {
            w = term_width - 10;
        }
        if w < 25 {
            w = 25;
        }
        w
    }

    /// Go `maxVisible` math (`label_picker.go:242-248`).
    fn max_visible(term_height: i32) -> i32 {
        let mut m = 10;
        if term_height < 15 {
            m = term_height - 7;
        }
        if m < 3 {
            m = 3;
        }
        m
    }

    /// The label list lines, exposed for tests and for callers that want to
    /// assert on layout without a terminal. Mirrors the `for i := start; i <
    /// end` body of Go `View` (`label_picker.go:286-314`).
    fn list_lines(&self, box_width: i32, max_visible: i32) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        if self.filtered.is_empty() {
            // `label_picker.go:270-275`
            out.push(Line::from(Span::styled(
                "  No matching labels",
                dim_italic(),
            )));
            return out;
        }

        // Calculate visible window (`label_picker.go:277-285`).
        let start = if self.selected as i32 >= max_visible {
            self.selected - max_visible as usize + 1
        } else {
            0
        };
        let end = (start + max_visible as usize).min(self.filtered.len());

        for i in start..end {
            let label = &self.labels[self.filtered[i]];
            let is_selected = i == self.selected;

            let item_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let count_style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let prefix = if is_selected { "> " } else { "  " };

            // Reserve space for the count when truncating the label.
            let count_str = format!(" ({})", label.count);
            let mut max_label_len = box_width - 8 - count_str.len() as i32;
            if max_label_len < 10 {
                max_label_len = 10;
            }
            let display_label = truncate_runes_helper(&label.name, max_label_len as usize, "...");

            out.push(Line::from(vec![
                Span::styled(format!("{prefix}{display_label}"), item_style),
                Span::styled(count_str, count_style),
            ]));
        }

        // Show count if scrolling (`label_picker.go:317-326`).
        if self.filtered.len() > max_visible as usize {
            out.push(Line::from(""));
            out.push(Line::from(Span::styled(
                format!(
                    "  {}({}/{})",
                    " ".repeat((box_width / 2 - 10).max(0) as usize),
                    itoa(self.selected + 1),
                    itoa(self.filtered.len())
                ),
                dim_italic(),
            )));
        }
        out
    }

    /// Go `View` (`label_picker.go:223-355`).
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let box_width = Self::box_width(area.width as i32) as u16;
        let max_visible = Self::max_visible(area.height as i32) as usize;

        let list = self.list_lines(box_width as i32, max_visible as i32);
        // title(1) + blank(1) + input(3) + blank(1) + list + blank(1) + footer(1)
        let content_lines = 8 + list.len();

        let popup = centered_popup(area, box_width, content_lines);
        f.render_widget(Clear, popup);
        let inner = box_block().inner(popup);

        // Title (`label_picker.go:253-258`).
        let lines: Vec<Line<'static>> = vec![
            Line::from(Span::styled(
                "Filter by Label",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::default(),
        ];
        let title_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 2.min(inner.height),
        };
        f.render_widget(Paragraph::new(lines), title_area);

        // Search input: a bordered box the full inner width, showing the
        // placeholder when empty (`label_picker.go:261-267`).
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .padding(Padding::horizontal(1));
        let input_text = if self.filter_text.is_empty() {
            Span::styled("type to filter...", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled(self.filter_text.clone(), Style::default().fg(Color::White))
        };
        let input_h = 3u16;
        let input_y = inner.y + 2;
        let input_area = Rect {
            x: inner.x,
            y: input_y,
            width: inner.width,
            height: input_h.min((inner.y + inner.height).saturating_sub(input_y)),
        };
        f.render_widget(Clear, input_area);
        f.render_widget(
            Paragraph::new(Line::from(input_text)).block(input_block),
            input_area,
        );

        // Label list + footer, below the input (`label_picker.go:269-334`).
        let list_y = input_y + input_h;
        let rest = Rect {
            x: inner.x,
            y: list_y,
            width: inner.width,
            height: (inner.y + inner.height).saturating_sub(list_y),
        };
        // Go's blank line after the input box (`label_picker.go:267`).
        let mut rest_lines: Vec<Line<'static>> = vec![Line::default()];
        rest_lines.extend(list);
        rest_lines.push(Line::default());
        rest_lines.push(Line::from(Span::styled(
            "j/k: navigate | enter: apply | esc: cancel",
            footer_style(),
        )));
        f.render_widget(Paragraph::new(rest_lines), rest);
    }
}

/// Go's local `itoa` (`label_picker.go:368-381`) — plain integer formatting.
fn itoa(n: usize) -> String {
    n.to_string()
}

// ============================================================================
// Recipe picker — Go pkg/ui/recipe_picker.go
// ============================================================================

/// A recipe option for the recipe picker.
#[derive(Debug, Clone)]
pub struct RecipeOption {
    pub name: String,
    pub description: String,
    pub labels: Vec<String>,
}

/// Formatted string for the active-recipe display.
/// Port of Go `FormatRecipeInfo` (`recipe_picker.go:163-168`).
pub fn format_recipe_info(r: &RecipeOption) -> String {
    format!("Recipe: {}", r.name)
}

/// Recipe picker state (Go `RecipePickerModel`).
pub struct RecipePicker {
    pub recipes: Vec<RecipeOption>,
    pub selected: usize,
    pub visible: bool,
}

impl RecipePicker {
    /// Go `NewRecipePickerModel` (`recipe_picker.go:22-28`) — the caller is
    /// responsible for the list order (Go passes `recipeLoader.List()`,
    /// already name-sorted).
    pub fn new(recipes: Vec<RecipeOption>) -> Self {
        RecipePicker {
            recipes,
            selected: 0,
            visible: false,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.selected = 0;
        }
    }

    /// Go `MoveUp` (`recipe_picker.go:37-42`).
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Go `MoveDown` (`recipe_picker.go:44-49`).
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.recipes.len() {
            self.selected += 1;
        }
    }

    /// Go `SelectedRecipe` (`recipe_picker.go:51-57`).
    pub fn selected_recipe(&self) -> Option<&RecipeOption> {
        if self.selected >= self.recipes.len() {
            return None;
        }
        self.recipes.get(self.selected)
    }

    /// Go `SelectedIndex` (`recipe_picker.go:59-61`).
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Go `RecipeCount` (`recipe_picker.go:158-160`).
    pub fn recipe_count(&self) -> usize {
        self.recipes.len()
    }

    /// The recipe list lines, mirroring Go `View` (`recipe_picker.go:94-127`):
    /// a name line per recipe, then its description indented 4 and truncated
    /// to `boxWidth-8` with `…`, then a blank separator between recipes.
    fn list_lines(&self, box_width: i32) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        for (i, r) in self.recipes.iter().enumerate() {
            let is_selected = i == self.selected;
            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            out.push(Line::from(Span::styled(
                format!("{prefix}{}", r.name),
                name_style,
            )));

            if !r.description.is_empty() {
                let desc =
                    truncate_runes_helper(&r.description, (box_width - 8).max(0) as usize, "…");
                out.push(Line::from(Span::styled(
                    format!("    {desc}"),
                    dim_italic(),
                )));
            }

            if i < self.recipes.len() - 1 {
                out.push(Line::from(""));
            }
        }
        out
    }

    /// Go `View` (`recipe_picker.go:64-155`).
    pub fn render(&self, f: &mut Frame, area: Rect) {
        // Go `boxWidth` math (`recipe_picker.go:76-81`).
        let mut box_width = 50i32;
        if (area.width as i32) < 60 {
            box_width = area.width as i32 - 10;
        }
        if box_width < 30 {
            box_width = 30;
        }

        let list = self.list_lines(box_width);
        // title, blank, list..., blank, footer
        let content_lines = 4 + list.len();

        let popup = centered_popup(area, box_width as u16, content_lines);
        f.render_widget(Clear, popup);
        let inner = box_block().inner(popup);

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            "Select Recipe",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.extend(list);
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "j/k: navigate • enter: apply • esc: cancel",
            footer_style(),
        )));
        f.render_widget(Paragraph::new(lines), inner);
    }
}

// ============================================================================
// Repo picker — Go pkg/ui/repo_picker.go
// ============================================================================

/// A repo option for the workspace repo picker.
#[derive(Debug, Clone)]
pub struct RepoOption {
    pub name: String,
    pub path: String,
    /// Rust-only: Go's repo picker has no per-row count. Retained for the
    /// status line and for callers that already compute it.
    pub issue_count: usize,
}

/// Sort the selected repo keys, keeping only the ones set to true.
/// Port of Go `sortedRepoKeys` (`pkg/ui/workspace_repos.go:52-64`).
pub fn sorted_repo_keys(selected: &HashSet<String>) -> Vec<String> {
    if selected.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = selected.iter().cloned().collect();
    out.sort();
    out
}

/// Format a sorted repo list, truncating after `max_names`.
/// Port of Go `formatRepoList` (`pkg/ui/workspace_repos.go:68-79`).
/// `["api","web","lib"]` with `max_names=2` -> `"api,web+1"`.
pub fn format_repo_list(repos: &[String], max_names: usize) -> String {
    if repos.is_empty() {
        return String::new();
    }
    if max_names == 0 {
        return format!("{} repos", repos.len());
    }
    if repos.len() <= max_names {
        return repos.join(",");
    }
    format!(
        "{}+{}",
        repos[..max_names].join(","),
        repos.len() - max_names
    )
}

/// What `App.active_repo` should become when the user presses Enter.
/// Port of Go's `handleRepoPickerKeys` "enter" arm (`model.go:5963-5984`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFilterOutcome {
    /// `None` means "all repos" (no filter) — Go's `activeRepos = nil`.
    /// An empty selection also normalizes to `None` so the list never goes blank.
    pub active: Option<BTreeSet<String>>,
    /// Go's `statusMsg` for this branch.
    pub status_msg: String,
}

/// Repo picker state (Go `RepoPickerModel`).
pub struct RepoPicker {
    pub repos: Vec<RepoOption>,
    /// `selectedIndex` — the cursor row.
    pub selected: usize,
    pub visible: bool,
    /// Go's `selected map[string]bool` — the checkbox state, all-on at open.
    pub selected_repos_set: HashSet<String>,
}

impl RepoPicker {
    /// Go `NewRepoPickerModel` (`repo_picker.go:20-31`): every repo starts
    /// checked.
    pub fn new(repos: Vec<RepoOption>) -> Self {
        let selected_repos_set = repos.iter().map(|r| r.name.clone()).collect();
        RepoPicker {
            repos,
            selected: 0,
            visible: false,
            selected_repos_set,
        }
    }

    /// Initialize selection from the active repo filter (`None` = all).
    /// Port of Go `SetActiveRepos` (`repo_picker.go:40-59`).
    pub fn set_active_repos(&mut self, active: Option<&BTreeSet<String>>) {
        if self.repos.is_empty() {
            self.selected_repos_set = HashSet::new();
            return;
        }
        self.selected_repos_set = match active {
            None => self.repos.iter().map(|r| r.name.clone()).collect(),
            Some(a) => self
                .repos
                .iter()
                .filter(|r| a.contains(&r.name))
                .map(|r| r.name.clone())
                .collect(),
        };
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.selected = 0;
        }
    }

    /// Go `MoveUp` (`repo_picker.go:62-67`).
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Go `MoveDown` (`repo_picker.go:69-73`).
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.repos.len() {
            self.selected += 1;
        }
    }

    /// Go `ToggleSelected` (`repo_picker.go:76-82`) — the `space` key.
    pub fn toggle_selected(&mut self) {
        if self.selected >= self.repos.len() {
            return;
        }
        let name = self.repos[self.selected].name.clone();
        if !self.selected_repos_set.remove(&name) {
            self.selected_repos_set.insert(name);
        }
    }

    /// Go `SelectAll` (`repo_picker.go:85-89`) — the `a` key.
    pub fn select_all(&mut self) {
        self.selected_repos_set = self.repos.iter().map(|r| r.name.clone()).collect();
    }

    /// Whether the cursor row is checked.
    pub fn is_selected(&self, name: &str) -> bool {
        self.selected_repos_set.contains(name)
    }

    /// Go `SelectedRepos` (`repo_picker.go:92-99`) — a repo -> true map,
    /// materialised as a `HashSet`.
    pub fn selected_repos(&self) -> HashSet<String> {
        self.repos
            .iter()
            .filter(|r| self.selected_repos_set.contains(&r.name))
            .map(|r| r.name.clone())
            .collect()
    }

    /// Go's Enter handling (`model.go:5963-5984`): normalize empty and full
    /// selections to "all", otherwise keep the partial set, and build the
    /// status line. Go truncates the name list after 3.
    pub fn apply_selection(&self) -> RepoFilterOutcome {
        let selected = self.selected_repos();
        if selected.is_empty() || selected.len() == self.repos.len() {
            return RepoFilterOutcome {
                active: None,
                status_msg: "Repo filter: all repos".to_string(),
            };
        }
        let sorted = sorted_repo_keys(&selected);
        let msg = format!("Repo filter: {}", format_repo_list(&sorted, 3));
        RepoFilterOutcome {
            active: Some(sorted.into_iter().collect()),
            status_msg: msg,
        }
    }

    /// The repo under the cursor. Retained for `App::handle_repo_picker_key`
    /// (lib.rs), which used the picker single-select; Go reads the checkbox
    /// set instead — see `selected_repos` / `apply_selection`.
    pub fn selected_repo(&self) -> Option<&RepoOption> {
        self.repos.get(self.selected)
    }

    /// The repo rows, mirroring Go `View` (`repo_picker.go:131-156`):
    /// `▸ [x] name` on the cursor row, `  [ ] name` elsewhere.
    fn list_lines(&self) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        if self.repos.is_empty() {
            out.push(Line::from(Span::styled(
                "No repos available.",
                dim_italic(),
            )));
            return out;
        }
        for (i, repo) in self.repos.iter().enumerate() {
            let is_cursor = i == self.selected;
            let style = if is_cursor {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_cursor { "▸ " } else { "  " };
            let check = if self.selected_repos_set.contains(&repo.name) {
                "[x]"
            } else {
                "[ ]"
            };
            out.push(Line::from(Span::styled(
                format!("{prefix}{check} {}", repo.name),
                style,
            )));
        }
        out
    }

    /// Go `View` (`repo_picker.go:103-180`).
    pub fn render(&self, f: &mut Frame, area: Rect) {
        // Go `boxWidth` math (`repo_picker.go:114-120`).
        let mut box_width = 50i32;
        if (area.width as i32) < 60 {
            box_width = area.width as i32 - 10;
        }
        if box_width < 30 {
            box_width = 30;
        }

        let list = self.list_lines();
        // title, blank, rows (or the empty-state line), blank, footer
        let content_lines = 4 + list.len();

        let popup = centered_popup(area, box_width as u16, content_lines);
        f.render_widget(Clear, popup);
        let inner = box_block().inner(popup);

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            "Repo Filter",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.extend(list);
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "j/k: navigate • space: toggle • a: all • enter: apply • esc: cancel",
            footer_style(),
        )));
        f.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Go pkg/ui/label_picker_test.go ------------------------------------

    #[test]
    fn fuzzy_score_exact_match() {
        assert_eq!(fuzzy_score("api", "api"), 1000);
    }

    #[test]
    fn fuzzy_score_prefix_match() {
        assert!(fuzzy_score("backend", "back") >= 500);
        // 500 + len("back")
        assert_eq!(fuzzy_score("backend", "back"), 504);
    }

    #[test]
    fn fuzzy_score_contains_match() {
        assert!(fuzzy_score("my-backend-api", "backend") >= 200);
        assert_eq!(fuzzy_score("my-backend-api", "backend"), 207);
    }

    #[test]
    fn fuzzy_score_subsequence_match() {
        // 'a-p' finds a in "backend" and p in "api" even though they are not
        // adjacent — this is the case the old to_lowercase().contains()
        // filter dropped.
        assert!(fuzzy_score("backend", "bnd") > 0);
        assert!(fuzzy_score("backend-api", "a-p") > 0);
    }

    #[test]
    fn fuzzy_score_non_ascii_widens_bytes_like_go() {
        // Go widens a single byte to a rune before IsLetter, so 0x80..=0xFF
        // become Latin-1 codepoints and many of them ARE letters (0xC3 = 'Ã').
        // A previous version of this port guarded on is_ascii() and scored
        // this 105 instead of 75; verified against the Go oracle over 5,795
        // (label, query) pairs including non-ASCII fuzz.
        assert_eq!(fuzzy_score("éaaé--", "éé"), 75);
        assert_eq!(fuzzy_score("-aé-日a", "a日"), 100);
    }

    #[test]
    fn fuzzy_score_no_match() {
        assert_eq!(fuzzy_score("api", "xyz"), 0);
    }

    #[test]
    fn fuzzy_score_case_insensitive() {
        assert_eq!(fuzzy_score("API", "api"), 1000);
        assert_eq!(fuzzy_score("api", "API"), 1000);
    }

    #[test]
    fn fuzzy_score_word_boundary_bonus() {
        // "a" at a word boundary, "s" inside "service".
        let boundary = fuzzy_score("my-api-service", "as");
        // Neither "a" nor "s" at a boundary.
        let no_boundary = fuzzy_score("myapiservice", "as");
        assert!(
            boundary > no_boundary,
            "boundary={boundary} no-boundary={no_boundary}"
        );
    }

    #[test]
    fn new_label_picker_sorts_by_count_desc() {
        // Go TestNewLabelPickerModel: api(10), core(7), zebra(5), backend(3).
        let labels = vec![
            LabelOption {
                name: "zebra".into(),
                count: 5,
            },
            LabelOption {
                name: "api".into(),
                count: 10,
            },
            LabelOption {
                name: "backend".into(),
                count: 3,
            },
            LabelOption {
                name: "core".into(),
                count: 7,
            },
        ];
        let picker = LabelPicker::new(labels);
        let names: Vec<&str> = picker.labels.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["api", "core", "zebra", "backend"]);
        assert_eq!(picker.filtered_count(), 4);
    }

    #[test]
    fn sort_labels_by_count_desc_breaks_ties_alphabetically() {
        let labels = vec![
            LabelOption {
                name: "zebra".into(),
                count: 5,
            },
            LabelOption {
                name: "apple".into(),
                count: 5,
            },
        ];
        let sorted = sort_labels_by_count_desc(&labels);
        assert_eq!(sorted[0].name, "apple");
        assert_eq!(sorted[1].name, "zebra");
    }

    #[test]
    fn label_picker_ranks_exact_hit_above_substring() {
        // An exact (score 1000) hit must not sit below a substring (200+) hit.
        let labels = vec![
            LabelOption {
                name: "my-api-service".into(),
                count: 9,
            },
            LabelOption {
                name: "api".into(),
                count: 1,
            },
        ];
        let mut picker = LabelPicker::new(labels);
        picker.update_filter("api");
        assert_eq!(picker.filtered.len(), 2);
        assert_eq!(picker.selected_label(), Some("api"));
    }

    #[test]
    fn label_picker_filter_clamms_selection() {
        let labels: Vec<LabelOption> = ["alpha", "beta", "gamma"]
            .iter()
            .map(|n| LabelOption {
                name: (*n).into(),
                count: 1,
            })
            .collect();
        let mut picker = LabelPicker::new(labels);
        picker.move_down();
        picker.move_down();
        assert_eq!(picker.selected, 2);
        // Filtering to a single hit clamps the cursor back into bounds.
        picker.update_filter("alpha");
        assert_eq!(picker.filtered.len(), 1);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn label_picker_navigation() {
        let labels = vec![
            LabelOption {
                name: "backend".into(),
                count: 5,
            },
            LabelOption {
                name: "frontend".into(),
                count: 3,
            },
            LabelOption {
                name: "urgent".into(),
                count: 2,
            },
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
            LabelOption {
                name: "backend".into(),
                count: 5,
            },
            LabelOption {
                name: "frontend".into(),
                count: 3,
            },
        ];
        let mut picker = LabelPicker::new(labels);
        picker.update_filter("front");
        assert_eq!(picker.filtered.len(), 1);
        assert_eq!(picker.selected_label(), Some("frontend"));
    }

    // -- Go pkg/ui/recipe_picker_test.go ------------------------------------

    #[test]
    fn recipe_picker_navigation() {
        let recipes = vec![
            RecipeOption {
                name: "R1".into(),
                description: "desc".into(),
                labels: vec![],
            },
            RecipeOption {
                name: "R2".into(),
                description: "desc".into(),
                labels: vec![],
            },
        ];
        let mut picker = RecipePicker::new(recipes);
        assert_eq!(picker.selected_recipe().unwrap().name, "R1");
        picker.move_down();
        assert_eq!(picker.selected_recipe().unwrap().name, "R2");
        picker.move_down();
        assert_eq!(picker.selected_recipe().unwrap().name, "R2");
    }

    #[test]
    fn test_format_recipe_info() {
        let r = RecipeOption {
            name: "actionable".into(),
            description: String::new(),
            labels: vec![],
        };
        assert_eq!(format_recipe_info(&r), "Recipe: actionable");
    }

    // -- Go pkg/ui/repo_picker_test.go --------------------------------------

    fn repo(name: &str) -> RepoOption {
        RepoOption {
            name: name.into(),
            path: name.into(),
            issue_count: 0,
        }
    }

    #[test]
    fn repo_picker_starts_all_selected() {
        let picker = RepoPicker::new(vec![repo("api"), repo("web")]);
        assert_eq!(picker.selected_repos().len(), 2);
    }

    #[test]
    fn repo_picker_toggle_and_select_all() {
        let mut picker = RepoPicker::new(vec![repo("api"), repo("web")]);
        picker.toggle_selected(); // untoggle "api"
        assert_eq!(picker.selected_repos(), HashSet::from(["web".to_string()]));
        picker.select_all();
        assert_eq!(picker.selected_repos().len(), 2);
    }

    #[test]
    fn repo_picker_set_active_repos() {
        let mut picker = RepoPicker::new(vec![repo("api"), repo("web"), repo("lib")]);
        picker.set_active_repos(None);
        assert_eq!(picker.selected_repos().len(), 3);
        let active = BTreeSet::from(["api".to_string()]);
        picker.set_active_repos(Some(&active));
        assert_eq!(picker.selected_repos(), HashSet::from(["api".to_string()]));
    }

    #[test]
    fn repo_filter_normalizes_all_and_empty() {
        let picker = RepoPicker::new(vec![repo("api"), repo("web")]);
        let out = picker.apply_selection();
        assert_eq!(out.active, None);
        assert_eq!(out.status_msg, "Repo filter: all repos");

        let mut picker = RepoPicker::new(vec![repo("api"), repo("web")]);
        picker.toggle_selected();
        picker.toggle_selected();
        let out = picker.apply_selection();
        assert_eq!(out.active, None, "empty selection normalizes to all");
    }

    #[test]
    fn test_format_repo_list_truncates() {
        let repos: Vec<String> = ["api", "lib", "web"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(format_repo_list(&repos, 3), "api,lib,web");
        assert_eq!(format_repo_list(&repos, 2), "api,lib+1");
        assert_eq!(format_repo_list(&repos, 0), "3 repos");
        assert_eq!(format_repo_list(&[], 3), "");
    }

    // -- Go pkg/ui/helpers.go ----------------------------------------------

    #[test]
    fn test_truncate_runes_helper_matches_go() {
        assert_eq!(truncate_runes_helper("hello", 10, "..."), "hello");
        assert_eq!(truncate_runes_helper("hello world", 8, "..."), "hello...");
        assert_eq!(truncate_runes_helper("hello", 0, "..."), "");
        // Suffix wider than the budget: the suffix is truncated instead.
        assert_eq!(truncate_runes_helper("hello world", 2, "..."), "..");
    }

    #[test]
    fn test_string_width_counts_wide_chars_as_two() {
        assert_eq!(string_width("abc"), 3);
        assert_eq!(string_width("日本語"), 6);
    }
}
