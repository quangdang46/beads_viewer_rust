//! Agent prompt preferences — port of Go `pkg/agents/prefs.go`.
//!
//! Stores per-project preference for AGENTS.md prompts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentPromptPreference {
    pub project_path: String,
    #[serde(default)]
    pub dont_ask_again: bool,
    #[serde(default)]
    pub declined_at: Option<String>,
    #[serde(default)]
    pub blurb_version_offered: i32,
    #[serde(default)]
    pub blurb_version_added: Option<i32>,
    #[serde(default)]
    pub added_at: Option<String>,
}

/// Get the preferences directory path.
/// Uses $XDG_CONFIG_HOME on Linux, ~/Library/Application Support on macOS,
/// %APPDATA% on Windows (Go `os.UserConfigDir()` semantics).
pub fn get_prefs_dir() -> Result<std::path::PathBuf, String> {
    let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config")
    } else {
        return Err("cannot determine config directory".into());
    };
    Ok(config_dir.join("bv").join("agent-prompts"))
}

/// Generate a project hash from work directory (SHA256 truncated to 16 hex).
pub fn project_hash(work_dir: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let abs = std::fs::canonicalize(work_dir).map_err(|e| format!("cannot resolve path: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(abs.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    Ok(hash[..8].iter().map(|b| format!("{b:02x}")).collect())
}

/// File name of the preference record for a project work dir.
pub fn pref_file_path(work_dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let hash = project_hash(work_dir)?;
    Ok(get_prefs_dir()?.join(format!("{hash}.json")))
}

/// Load the stored preference for a project. Missing/unreadable file → default.
pub fn load_preference(work_dir: &std::path::Path) -> AgentPromptPreference {
    let path = match pref_file_path(work_dir) {
        Ok(p) => p,
        Err(_) => return AgentPromptPreference::default(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return AgentPromptPreference::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Save the preference for a project (creates dir, atomic write).
pub fn save_preference(
    work_dir: &std::path::Path,
    pref: &AgentPromptPreference,
) -> Result<(), String> {
    let path = pref_file_path(work_dir)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create prefs dir: {e}"))?;
    }
    let data = serde_json::to_string_pretty(pref).map_err(|e| format!("encode prefs: {e}"))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data.as_bytes()).map_err(|e| format!("write prefs: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename prefs: {e}"))?;
    Ok(())
}

/// Port of Go `ShouldPromptForAgentFile`: true when the TUI should show the
/// blurb prompt for this project.
pub fn should_prompt_for_agent_file(
    work_dir: &std::path::Path,
    detection: &super::detect::AgentFileDetection,
) -> bool {
    if !detection.found() {
        return false;
    }
    if !detection.needs_blurb() && !detection.needs_upgrade() {
        return false;
    }
    let pref = load_preference(work_dir);
    if pref.dont_ask_again {
        return false;
    }
    if let Some(added) = pref.blurb_version_added {
        if added >= super::BLURB_VERSION {
            return false;
        }
    }
    true
}

/// Record an accepted blurb injection (Go `RecordAccept`).
pub fn record_accept(work_dir: &std::path::Path, version_added: i32) -> Result<(), String> {
    let abs = std::fs::canonicalize(work_dir).map_err(|e| format!("cannot resolve path: {e}"))?;
    let mut pref = load_preference(work_dir);
    pref.project_path = abs.to_string_lossy().to_string();
    pref.blurb_version_added = Some(version_added);
    pref.added_at = Some(jiff::Timestamp::now().to_string());
    save_preference(work_dir, &pref)
}

/// Record a declined prompt (Go `RecordDecline`). `never` persists don't-ask-again.
pub fn record_decline(
    work_dir: &std::path::Path,
    never: bool,
    version_offered: i32,
) -> Result<(), String> {
    let abs = std::fs::canonicalize(work_dir).map_err(|e| format!("cannot resolve path: {e}"))?;
    let mut pref = load_preference(work_dir);
    pref.project_path = abs.to_string_lossy().to_string();
    pref.dont_ask_again = never;
    pref.declined_at = Some(jiff::Timestamp::now().to_string());
    pref.blurb_version_offered = version_offered;
    save_preference(work_dir, &pref)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Single test: both cases mutate the process-global XDG_CONFIG_HOME, so
    // they must not run in parallel (Rust runs tests in one binary
    // multithreaded by default).
    #[test]
    fn preference_roundtrip_and_prompt_matrix() {
        use super::super::detect::AgentFileDetection;
        // Isolate from real user config.
        let cfg = std::env::temp_dir().join("bvr_prefs_cfg_matrix");
        let _ = std::fs::remove_dir_all(&cfg);
        std::env::set_var("XDG_CONFIG_HOME", &cfg);
        let dir = std::env::temp_dir().join("bvr_prefs_matrix");
        let _ = std::fs::create_dir_all(&dir);

        // Roundtrip.
        let pref = AgentPromptPreference {
            project_path: dir.to_string_lossy().to_string(),
            dont_ask_again: true,
            declined_at: Some("t".into()),
            blurb_version_offered: 3,
            blurb_version_added: Some(3),
            added_at: Some("t".into()),
        };
        save_preference(&dir, &pref).unwrap();
        let loaded = load_preference(&dir);
        assert!(loaded.dont_ask_again);
        assert_eq!(loaded.blurb_version_added, Some(3));
        assert_eq!(loaded.blurb_version_offered, 3);

        // Back to default prefs for the prompt matrix.
        save_preference(&dir, &AgentPromptPreference::default()).unwrap();

        let no_file = AgentFileDetection::default();
        assert!(!should_prompt_for_agent_file(&dir, &no_file));

        let with_blurb = AgentFileDetection {
            file_path: "AGENTS.md".into(),
            file_type: "AGENTS.md".into(),
            has_blurb: true,
            has_legacy_blurb: false,
            blurb_version: super::super::BLURB_VERSION,
        };
        assert!(!should_prompt_for_agent_file(&dir, &with_blurb));

        let needs = AgentFileDetection {
            file_path: "AGENTS.md".into(),
            file_type: "AGENTS.md".into(),
            has_blurb: false,
            has_legacy_blurb: false,
            blurb_version: 0,
        };
        assert!(should_prompt_for_agent_file(&dir, &needs));

        record_decline(&dir, true, super::super::BLURB_VERSION).unwrap();
        assert!(!should_prompt_for_agent_file(&dir, &needs));

        // Reset to accepted state → no prompt for current version.
        let pref = AgentPromptPreference {
            dont_ask_again: false,
            ..Default::default()
        };
        save_preference(&dir, &pref).unwrap();
        record_accept(&dir, super::super::BLURB_VERSION).unwrap();
        assert!(!should_prompt_for_agent_file(&dir, &needs));

        let _ = std::fs::remove_dir_all(&cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_hash_deterministic() {
        let dir = std::env::temp_dir().join("bvr_prefs_test");
        let _ = std::fs::create_dir(&dir);
        let h1 = project_hash(&dir).unwrap();
        let h2 = project_hash(&dir).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
        let _ = std::fs::remove_dir(&dir);
    }
}
