//! Theme system — Dracula-inspired color palette for the TUI.
//! Port of Go `pkg/ui/theme.go` + `pkg/ui/styles.go`.

use ratatui::style::Color;
use std::io::Write;

/// Explicit light/dark palette selection, the port of Go's
/// `ui.BVThemeOverride` (pkg/ui/theme.go:25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePref {
    /// `"light"` — use the `Light` arm of every `AdaptiveColor`.
    Light,
    /// `"dark"` — use the `Dark` arm of every `AdaptiveColor`.
    Dark,
    /// `"auto"` — no pin. Go leaves `lipgloss`'s background detection in
    /// charge (pkg/ui/theme.go:64-71), which assumes dark whenever the
    /// terminal never answers the background query. Ratatui has no
    /// equivalent query, so this resolves to the dark palette, matching
    /// that default. See [`Theme::for_preference`].
    #[default]
    Auto,
}

impl ThemePref {
    /// The canonical lowercase name Go echoes back (`"light"`/`"dark"`/`"auto"`).
    pub fn as_str(self) -> &'static str {
        match self {
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
            ThemePref::Auto => "auto",
        }
    }
}

/// Map user input to a recognized theme name, ignoring case and surrounding
/// whitespace; anything unrecognized (including empty) yields `None`.
/// Port of Go `canonicalTheme` (cmd/bv/main.go:4639).
pub fn canonical_theme(s: &str) -> Option<ThemePref> {
    // Go uses `strings.ToLower`, which is Unicode-aware; `to_lowercase`
    // matches it. (`to_ascii_lowercase` would not.)
    match s.trim().to_lowercase().as_str() {
        "light" => Some(ThemePref::Light),
        "dark" => Some(ThemePref::Dark),
        "auto" => Some(ThemePref::Auto),
        _ => None,
    }
}

/// Resolve the color-theme preference with the precedence
/// `--theme` flag > `BV_THEME` > `theme:` in `~/.config/bv/config.yaml` >
/// auto-detect. Port of Go `effectiveThemePreference` (cmd/bv/main.go:4657).
///
/// Go returns `""` when nothing is configured and `"auto"` for a bad
/// `--theme`; both land in the same `SetThemeOverride` default branch
/// (pkg/ui/theme.go:64-71), so both collapse to [`ThemePref::Auto`] here.
pub fn effective_theme_preference(flag_val: Option<&str>, warn_to: &mut dyn Write) -> ThemePref {
    let env_val = std::env::var(THEME_ENV_VAR).ok();
    let config_val = theme_from_user_config();
    resolve_theme_preference(flag_val, env_val.as_deref(), config_val.as_deref(), warn_to)
}

/// The pure precedence walk behind [`effective_theme_preference`], with the
/// two ambient sources passed in rather than read from the process.
///
/// An explicitly passed but unrecognized flag value warns on `warn_to` and
/// resolves to auto-detection — the flag level is honored rather than
/// silently falling through to a lower-precedence source the user did not
/// intend (bv-128).
pub fn resolve_theme_preference(
    flag_val: Option<&str>,
    env_val: Option<&str>,
    config_val: Option<&str>,
    warn_to: &mut dyn Write,
) -> ThemePref {
    if let Some(raw) = flag_val {
        if let Some(v) = canonical_theme(raw) {
            return v;
        }
        // Byte-identical to Go's `fmt.Fprintf` at cmd/bv/main.go:4663:
        // both `Fprintf` and `writeln!` emit a bare "\n" on every platform.
        let _ = writeln!(
            warn_to,
            "Warning: unknown --theme value {raw:?} (expected light, dark, or auto); using auto-detection"
        );
        return ThemePref::Auto;
    }
    for lower in [env_val, config_val].into_iter().flatten() {
        if let Some(pref) = canonical_theme(lower) {
            return pref;
        }
    }
    ThemePref::Auto
}

/// Name of the environment variable Go registers as `env.Theme`
/// (internal/env/env.go:269), read with a plain `os.Getenv`
/// (internal/env/env.go:23).
pub const THEME_ENV_VAR: &str = "BV_THEME";

/// Read the top-level `theme:` key from `~/.config/bv/config.yaml`, returning
/// it when present and non-empty. Value validation is [`canonical_theme`]'s
/// job. Port of Go `loadThemeFromUserConfig` (cmd/bv/main.go:4681).
pub fn theme_from_user_config() -> Option<String> {
    let data = std::fs::read_to_string(user_config_path()?).ok()?;
    parse_user_config(&data)
}

/// Pull the non-empty `theme:` value out of a `config.yaml` body. Any YAML
/// error yields `None`, matching Go's `if err := yaml.Unmarshal(...); err != nil
/// { return "", false }` (cmd/bv/main.go:4696-4698).
fn parse_user_config(data: &str) -> Option<String> {
    let cfg: BvUserConfig = serde_yaml_ng::from_str(data).ok()?;
    let theme = cfg.theme.trim();
    if theme.is_empty() {
        return None;
    }
    Some(theme.to_string())
}

/// `~/.config/bv/config.yaml`, the path Go joins
/// (`os.UserHomeDir` + `.config/bv/config.yaml`, cmd/bv/main.go:4686).
fn user_config_path() -> Option<std::path::PathBuf> {
    // Go's `os.UserHomeDir` reads %USERPROFILE% on Windows and $HOME
    // elsewhere, erroring when either is empty. Spelled out here rather than
    // going through `std::env::home_dir`, whose fallbacks differ.
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    }?;
    if home.is_empty() {
        return None;
    }
    Some(
        std::path::PathBuf::from(home)
            .join(".config")
            .join("bv")
            .join("config.yaml"),
    )
}

#[derive(serde::Deserialize, Default)]
struct BvUserConfig {
    #[serde(default)]
    theme: String,
}

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
    /// Dim secondary text. Go `Theme.Subtext` (theme.go:102, values :186):
    /// Dark `#BFBFBF` / Light `#666666`. Distinct from `secondary`/`muted`
    /// (both `#6272A4` in the dark palette) — history.go paints event details,
    /// commit messages, timeline detail lines and empty-state text in it.
    pub subtext: Color,
    /// Panel borders and the timeline spine. Go `Theme.Border`
    /// (theme.go:121, values :204): Dark `#44475A` / Light `#AAAAAA`. Same hex
    /// as `highlight` in the dark palette but a different role: `highlight` is a
    /// selection *background*, `border` is a foreground.
    pub border: Color,
    /// Navigation hints in pane footers. Go `ColorFooterHint`
    /// (styles.go:90): Light `#444444` / Dark `#C8C8D0`.
    pub footer_hint: Color,
}

impl Default for Theme {
    /// The `Dark` arm of every Go `AdaptiveColor` — Dracula.
    fn default() -> Self {
        // Go pkg/ui/styles.go:30,34,36,39-43,47-49,54 and
        // pkg/ui/theme.go:205,209.
        Theme {
            bg: Color::Rgb(40, 42, 54),             // #282A36
            fg: Color::Rgb(248, 248, 242),          // #F8F8F2
            primary: Color::Rgb(189, 147, 249),     // #BD93F9 Purple
            secondary: Color::Rgb(98, 114, 164),    // #6272A4 Comment
            accent: Color::Rgb(139, 233, 253),      // #8BE9FD Cyan
            open: Color::Rgb(80, 250, 123),         // #50FA7B Green
            in_progress: Color::Rgb(139, 233, 253), // #8BE9FD Cyan
            blocked: Color::Rgb(255, 85, 85),       // #FF5555 Red
            closed: Color::Rgb(98, 114, 164),       // #6272A4 Comment
            warning: Color::Rgb(255, 184, 108),     // #FFB86C Orange
            highlight: Color::Rgb(68, 71, 90),      // #44475A Current line
            muted: Color::Rgb(98, 114, 164),        // #6272A4 Comment
            subtext: Color::Rgb(191, 191, 191),     // #BFBFBF Dim (theme.go:186)
            border: Color::Rgb(68, 71, 90),         // #44475A Border (theme.go:204)
            footer_hint: Color::Rgb(200, 200, 208), // #C8C8D0 (styles.go:90)
        }
    }
}

impl Theme {
    /// The `Light` arm of every Go `AdaptiveColor`, tuned for WCAG AA
    /// contrast on a white background (bv-3fcg). Every hex below is lifted
    /// verbatim from the Go source — see the per-field citations.
    pub fn light() -> Self {
        // Go pkg/ui/styles.go:30,36,39-43,47-49,54 and pkg/ui/theme.go:205,209.
        Theme {
            bg: Color::Rgb(255, 255, 255),        // #FFFFFF (styles.go:30)
            fg: Color::Rgb(0, 0, 0),              // #000000 (theme.go:209, `t.Base`)
            primary: Color::Rgb(107, 71, 217),    // #6B47D9 (styles.go:39)
            secondary: Color::Rgb(85, 85, 85),    // #555555 (styles.go:40)
            accent: Color::Rgb(0, 96, 128),       // #006080 (styles.go:41 `ColorInfo`)
            open: Color::Rgb(0, 119, 0),          // #007700 (styles.go:47)
            in_progress: Color::Rgb(0, 96, 128),  // #006080 (styles.go:48)
            blocked: Color::Rgb(204, 0, 0),       // #CC0000 (styles.go:49)
            closed: Color::Rgb(85, 85, 85),       // #555555 (styles.go:54)
            warning: Color::Rgb(176, 104, 0),     // #B06800 (styles.go:43)
            highlight: Color::Rgb(224, 224, 224), // #E0E0E0 (theme.go:205)
            muted: Color::Rgb(102, 102, 102),     // #666666 (styles.go:36)
            subtext: Color::Rgb(102, 102, 102),   // #666666 Dim (theme.go:186)
            border: Color::Rgb(170, 170, 170),    // #AAAAAA Border (theme.go:204)
            footer_hint: Color::Rgb(68, 68, 68),  // #444444 (styles.go:90)
        }
    }

    /// Build the palette for a resolved preference.
    ///
    /// [`ThemePref::Auto`] — and `None`, which Go expresses as the empty
    /// string — resolve to the dark palette. Go pins
    /// `lipgloss.HasDarkBackground` only when the user names a theme and
    /// otherwise leaves auto-detection in charge, which itself assumes dark
    /// whenever the terminal does not answer the background query
    /// (pkg/ui/theme.go:39-72). Ratatui exposes no background query, so
    /// "dark unless the user asked for light" is the faithful mapping.
    pub fn for_preference(pref: Option<ThemePref>) -> Self {
        match pref {
            Some(ThemePref::Light) => Theme::light(),
            _ => Theme::default(),
        }
    }

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

    #[test]
    fn canonical_theme_accepts_the_three_go_names() {
        assert_eq!(canonical_theme("light"), Some(ThemePref::Light));
        assert_eq!(canonical_theme("dark"), Some(ThemePref::Dark));
        assert_eq!(canonical_theme("auto"), Some(ThemePref::Auto));
    }

    #[test]
    fn canonical_theme_ignores_case_and_whitespace() {
        assert_eq!(canonical_theme("  LiGhT \n"), Some(ThemePref::Light));
        assert_eq!(canonical_theme("DARK"), Some(ThemePref::Dark));
        assert_eq!(canonical_theme("\taUtO "), Some(ThemePref::Auto));
    }

    #[test]
    fn canonical_theme_rejects_everything_else() {
        for bad in ["", "   ", "bogus", "lightish", "0", "tru", "light dark"] {
            assert_eq!(canonical_theme(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn theme_pref_round_trips_through_its_canonical_name() {
        for pref in [ThemePref::Light, ThemePref::Dark, ThemePref::Auto] {
            assert_eq!(canonical_theme(pref.as_str()), Some(pref));
        }
    }

    #[test]
    fn for_preference_picks_the_matching_palette() {
        assert_eq!(
            Theme::for_preference(Some(ThemePref::Light)).open,
            Theme::light().open
        );
        assert_eq!(
            Theme::for_preference(Some(ThemePref::Dark)).open,
            Theme::default().open
        );
    }

    #[test]
    fn for_preference_treats_none_and_auto_as_the_default_palette() {
        let d = Theme::default();
        assert_eq!(Theme::for_preference(None).fg, d.fg);
        assert_eq!(Theme::for_preference(Some(ThemePref::Auto)).fg, d.fg);
    }

    #[test]
    fn light_palette_is_not_the_dark_palette() {
        let light = Theme::light();
        let dark = Theme::default();
        assert_ne!(light.bg, dark.bg);
        assert_ne!(light.fg, dark.fg);
        assert_ne!(light.primary, dark.primary);
        assert_ne!(light.highlight, dark.highlight);
    }

    #[test]
    fn known_flag_set_bypasses_every_fallback() {
        let mut warn = Vec::new();
        let pref = resolve_theme_preference(Some("light"), Some("dark"), Some("dark"), &mut warn);
        assert_eq!(pref, ThemePref::Light);
        assert!(warn.is_empty(), "a valid value must not warn: {warn:?}");
    }

    #[test]
    fn bad_flag_warns_once_and_falls_back_to_auto() {
        let mut warn = Vec::new();
        let pref = resolve_theme_preference(Some("bogus"), None, None, &mut warn);
        assert_eq!(pref, ThemePref::Auto);
        assert_eq!(
            String::from_utf8(warn).unwrap(),
            "Warning: unknown --theme value \"bogus\" (expected light, dark, or auto); using auto-detection\n"
        );
    }

    #[test]
    fn bad_flag_keeps_the_users_exact_casing_in_the_warning() {
        // Go prints the raw `*themeFlag`, not the canonicalized name.
        let mut warn = Vec::new();
        resolve_theme_preference(Some("BOGUS"), None, None, &mut warn);
        assert!(String::from_utf8(warn).unwrap().contains("value \"BOGUS\""));
    }

    #[test]
    fn bad_flag_does_not_fall_through_to_lower_precedence_sources() {
        // The flag level is honored: a bad --theme must not silently inherit
        // a source the user did not intend (bv-128).
        let mut warn = Vec::new();
        let pref = resolve_theme_preference(Some("bogus"), Some("dark"), Some("light"), &mut warn);
        assert_eq!(pref, ThemePref::Auto);
    }

    #[test]
    fn env_beats_config_file() {
        let mut warn = Vec::new();
        assert_eq!(
            resolve_theme_preference(None, Some("dark"), Some("light"), &mut warn),
            ThemePref::Dark
        );
        assert!(warn.is_empty());
    }

    #[test]
    fn config_file_is_used_when_flag_and_env_are_silent() {
        let mut warn = Vec::new();
        assert_eq!(
            resolve_theme_preference(None, None, Some("light"), &mut warn),
            ThemePref::Light
        );
        assert!(warn.is_empty());
    }

    #[test]
    fn nothing_configured_resolves_to_auto_without_warning() {
        let mut warn = Vec::new();
        assert_eq!(
            resolve_theme_preference(None, None, None, &mut warn),
            ThemePref::Auto
        );
        assert!(warn.is_empty());
    }

    #[test]
    fn unrecognized_lower_precedence_sources_are_skipped_silently() {
        // Only an explicit --theme warns; a bad BV_THEME or config value
        // falls through (cmd/bv/main.go:4667-4673).
        let mut warn = Vec::new();
        assert_eq!(
            resolve_theme_preference(None, Some("solarized"), Some("nope"), &mut warn),
            ThemePref::Auto
        );
        assert!(warn.is_empty());
    }

    #[test]
    fn an_unrecognized_env_does_not_shadow_a_good_config_value() {
        let mut warn = Vec::new();
        assert_eq!(
            resolve_theme_preference(None, Some("solarized"), Some("light"), &mut warn),
            ThemePref::Light
        );
    }

    #[test]
    fn user_config_reads_the_top_level_theme_key() {
        assert_eq!(
            parse_user_config("theme: light\n").as_deref(),
            Some("light")
        );
        assert_eq!(
            parse_user_config("# comment\ntheme: \"dark\"\nother: 1\n").as_deref(),
            Some("dark")
        );
    }

    #[test]
    fn user_config_trims_and_ignores_a_blank_theme() {
        assert_eq!(
            parse_user_config("theme: '  dark  '\n").as_deref(),
            Some("dark")
        );
        assert_eq!(parse_user_config("theme: ''\n"), None);
        assert_eq!(parse_user_config("other: 1\n"), None);
    }

    #[test]
    fn user_config_ignores_a_nested_theme_key() {
        // Go unmarshals into a top-level-only struct, so `ui.theme` is not
        // the `theme:` key being resolved.
        assert_eq!(parse_user_config("ui:\n  theme: light\n"), None);
    }

    #[test]
    fn user_config_rejects_malformed_yaml() {
        assert_eq!(parse_user_config("theme: [unclosed\n"), None);
        assert_eq!(parse_user_config("\tnot: yaml\n"), None);
    }
}
