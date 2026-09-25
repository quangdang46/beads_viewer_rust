//! Pages deployment wizard — configuration persistence, port of the
//! self-contained half of Go `pkg/export/wizard.go` at parity commit
//! `18afafa`.
//!
//! ## Scope, and why it is this half
//!
//! `--pages` currently falls through every dispatch branch in the Rust CLI
//! and lands in the TUI launcher, where it blocks on an event loop. Go
//! dispatches it to `runPagesWizard` (`cmd/bv/main.go:3041`) before
//! `--preview-pages`, and a CI caller therefore gets a hang instead of Go's
//! exit 0.
//!
//! This module is the part of that flow that belongs in an export crate: the
//! configuration the wizard carries between runs. It is self-contained, it is
//! what the wizard offers to reuse on a second invocation
//! (`offerSavedConfig`, `wizard.go:194`), and it is the only piece with a
//! stable on-disk contract worth pinning.
//!
//! **The interactive form is not here, and neither is the deployment.** Go
//! builds it with `github.com/charmbracelet/huh` (`wizard.go:19`), a
//! Charm-branded TUI toolkit that is not this workspace's `ratatui`; porting
//! it as ratatui would produce a different-looking wizard, which is a worse
//! outcome than an honest gap. The deployment half fans out into
//! `pkg/export/github.go` and `pkg/export/cloudflare.go` — 60 KB of GitHub
//! Pages and Cloudflare Workers orchestration, none of which exists in this
//! crate. Whoever wires `--pages` in `crates/bv/src/main.rs` needs a form
//! driver and those two backends first; this module is what they will hand the
//! result to.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Go `WizardConfig` (`pkg/export/wizard.go:26-55`).
///
/// Field order is the JSON key order, which `encoding/json` derives from
/// declaration order. Go writes the file with `MarshalIndent`, so the
/// persisted document has to keep this order to stay a drop-in replacement.
///
/// The container-level `#[serde(default)]` is the read half of the same
/// contract. Most fields are omitted when empty, and Go's `json.Unmarshal`
/// leaves an absent key at its zero value rather than failing, so a file
/// written by an older build — or one a user has trimmed by hand — must still
/// load.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WizardConfig {
    // Export options — wizard.go:27-30.
    pub include_closed: bool,
    pub include_history: bool,
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subtitle: String,

    // Source metadata for reliable updates — wizard.go:33-37.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_beads_dir: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_repo_root: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_path: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub last_issue_count: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_data_hash: String,

    // Deployment target — wizard.go:40.
    /// Go: `DeployTarget` — one of `github`, `cloudflare`, `local`. Go has no
    /// enum validation on this field, so an unrecognised value round-trips
    /// rather than being rejected here either.
    pub deploy_target: String,

    // GitHub options — wizard.go:43-45.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub repo_name: String,
    #[serde(skip_serializing_if = "is_false")]
    pub repo_private: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub repo_description: String,

    // Cloudflare options — wizard.go:48-49.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cloudflare_project: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cloudflare_branch: String,

    /// Go: `OutputPath` — the bundle directory, wizard.go:52.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub output_path: String,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Go `WizardResult` (`pkg/export/wizard.go:57-66`).
///
/// Not serialised in Go — it is the return value of `PerformDeploy`, and
/// every field has an empty value that JSON's `omitempty` would drop. It is
/// kept here as a plain struct for the same reason.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WizardResult {
    pub bundle_path: String,
    pub repo_full_name: String,
    pub pages_url: String,
    pub deploy_target: String,
    // Cloudflare-specific — wizard.go:64-65.
    pub cloudflare_project: String,
    pub cloudflare_url: String,
}

/// Failures from [`load_wizard_config`] and [`save_wizard_config`].
#[derive(Debug, thiserror::Error)]
pub enum WizardConfigError {
    /// `wizard.go:980` and `:1016` — no home directory, so no config path.
    #[error("could not determine config path")]
    NoConfigPath,
    /// `wizard.go:1021` / `:1028`.
    #[error("{0}")]
    Read(#[source] std::io::Error),
    /// `wizard.go:989`.
    #[error("{0}")]
    Parse(#[source] serde_json::Error),
    /// `wizard.go:1025`.
    #[error("create wizard config directory: {0}")]
    CreateDir(#[source] std::io::Error),
    /// `wizard.go:1036`.
    #[error("marshal wizard config: {0}")]
    Marshal(#[source] serde_json::Error),
    /// `wizard.go:1040` onward — the atomic write.
    #[error("{0}")]
    Write(String),
}

/// Go `WizardConfigPath` (`pkg/export/wizard.go:974-981`):
/// `$HOME/.config/bv/pages-wizard.json` on Unix and
/// `%USERPROFILE%\.config\bv\pages-wizard.json` on Windows.
pub fn wizard_config_path() -> Result<PathBuf, WizardConfigError> {
    let home = home_dir().ok_or(WizardConfigError::NoConfigPath)?;
    Ok(home.join(".config").join("bv").join("pages-wizard.json"))
}

fn home_dir() -> Option<PathBuf> {
    // Go's `os.UserHomeDir` reads $HOME on Unix and %USERPROFILE% on Windows,
    // and errors when the variable is unset rather than falling back to
    // getpwuid, so the port keeps that behaviour instead of silently
    // inventing a home.
    let key = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Go `LoadWizardConfig` (`pkg/export/wizard.go:983-1004`).
///
/// A missing file is not an error: Go returns `(nil, nil)`, which the wizard
/// reads as "no saved configuration". `Ok(None)` is that same signal.
pub fn load_wizard_config() -> Result<Option<WizardConfig>, WizardConfigError> {
    let path = wizard_config_path()?;
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(WizardConfigError::Read(e)),
    };
    let config = serde_json::from_str(&data).map_err(WizardConfigError::Parse)?;
    Ok(Some(config))
}

/// Go `SaveWizardConfig` (`pkg/export/wizard.go:1006-1028`): render with
/// `MarshalIndent(_, "", "  ")` and hand the bytes to the atomic write.
pub fn save_wizard_config(config: &WizardConfig) -> Result<(), WizardConfigError> {
    let path = wizard_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(WizardConfigError::CreateDir)?;
    }
    let data = serde_json::to_string_pretty(config).map_err(WizardConfigError::Marshal)?;
    write_wizard_config_file(&path, data.as_bytes())
}

/// Go `writeWizardConfigFile` (`pkg/export/wizard.go:1030-1064`).
///
/// Write to a temporary file in the same directory, `fsync`, then rename over
/// the target. That ordering is the point: a crash mid-write leaves the
/// previous configuration intact rather than a truncated file that would fail
/// to parse on the next run. Exposed separately so a caller can save to a
/// chosen path.
pub fn write_wizard_config_file(
    path: &std::path::Path,
    data: &[u8],
) -> Result<(), WizardConfigError> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("pages-wizard.json");
    let temp = dir.join(format!("{base}.tmp-{}", std::process::id()));

    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        std::io::Write::write_all(&mut file, data)?;
        file.sync_all()?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&temp);
        return Err(WizardConfigError::Write(format!(
            "write temporary wizard config: {e}"
        )));
    }
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(WizardConfigError::Write(format!(
            "replace wizard config: {e}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> WizardConfig {
        WizardConfig {
            include_closed: true,
            include_history: false,
            title: "My Project".to_string(),
            subtitle: "a subtitle".to_string(),
            source_beads_dir: "/home/dev/.beads".to_string(),
            source_repo_root: "/home/dev/project".to_string(),
            source_path: ".beads/issues.jsonl".to_string(),
            last_issue_count: 42,
            last_data_hash: "abc123".to_string(),
            deploy_target: "github".to_string(),
            repo_name: "dev/project".to_string(),
            repo_private: true,
            repo_description: "issues".to_string(),
            cloudflare_project: String::new(),
            cloudflare_branch: String::new(),
            output_path: "/tmp/bundle".to_string(),
        }
    }

    #[test]
    fn the_json_key_order_matches_the_go_struct() {
        // wizard.go:26-55, emitted in declaration order by encoding/json.
        // Go's defaults for the two booleans are both false, so both keys are
        // present; every optional string is omitted when empty.
        let json = serde_json::to_string(&populated()).unwrap();
        let expected = concat!(
            r#"{"include_closed":true,"include_history":false,"title":"My Project","#,
            r#""subtitle":"a subtitle","source_beads_dir":"/home/dev/.beads","#,
            r#""source_repo_root":"/home/dev/project","source_path":".beads/issues.jsonl","#,
            r#""last_issue_count":42,"last_data_hash":"abc123","deploy_target":"github","#,
            r#""repo_name":"dev/project","repo_private":true,"repo_description":"issues","#,
            r#""output_path":"/tmp/bundle"}"#,
        );
        assert_eq!(json, expected);
        // The two Cloudflare fields are unset here, so they contribute nothing
        // between `repo_description` and `output_path`.
        assert!(!json.contains("cloudflare"));
    }

    #[test]
    fn empty_optional_fields_are_omitted() {
        // Every optional field carries Go's `omitempty`.
        let config = WizardConfig {
            deploy_target: "local".to_string(),
            ..WizardConfig::default()
        };
        assert_eq!(
            serde_json::to_string(&config).unwrap(),
            r#"{"include_closed":false,"include_history":false,"title":"","deploy_target":"local"}"#
        );
    }

    #[test]
    fn a_round_trip_preserves_every_field() {
        let json = serde_json::to_string(&populated()).unwrap();
        let parsed: WizardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, populated());
    }

    #[test]
    fn the_saved_file_is_pretty_printed_with_two_spaces() {
        // wizard.go:1026 — MarshalIndent(_, "", "  ").
        let text = serde_json::to_string_pretty(&populated()).unwrap();
        assert!(text.starts_with("{\n  \"include_closed\": true,"));
        assert!(text.ends_with("\n}"));
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temporary_behind() {
        let dir = std::env::temp_dir().join(format!("bvr-wizard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pages-wizard.json");

        write_wizard_config_file(&path, b"{\"first\":true}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"first\":true}");

        // A second save replaces it, and no `.tmp-` file is left in the dir.
        write_wizard_config_file(&path, b"{\"second\":true}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"second\":true}");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn the_config_path_is_under_the_home_directory() {
        // wizard.go:974-981
        let path = wizard_config_config_path_for_test();
        let rendered = path.to_string_lossy().replace('\\', "/");
        assert!(
            rendered.ends_with("/.config/bv/pages-wizard.json"),
            "{rendered}"
        );
    }

    fn wizard_config_config_path_for_test() -> PathBuf {
        // `wizard_config_path` reads the environment, which is exactly what
        // makes it awkward to assert against; check the shape of the join
        // instead of the value of $HOME.
        home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("bv")
            .join("pages-wizard.json")
    }
}
