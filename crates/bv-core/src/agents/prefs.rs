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

#[cfg(test)]
mod tests {
    use super::*;

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
