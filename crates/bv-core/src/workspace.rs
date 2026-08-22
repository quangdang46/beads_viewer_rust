//! Multi-repo workspace support — port of Go `pkg/workspace/types.go`.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub discovery: Option<DiscoveryConfig>,
    #[serde(default)]
    pub defaults: Option<Defaults>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub path: String,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default, rename = "beads_path")]
    pub beads_path: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_patterns")]
    pub patterns: Vec<String>,
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
}

fn default_patterns() -> Vec<String> {
    [
        "*",
        "packages/*",
        "apps/*",
        "services/*",
        "libs/*",
        "modules/*",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
fn default_exclude() -> Vec<String> {
    ["node_modules", "vendor", ".git", "dist", "build", "target"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
fn default_max_depth() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default, rename = "beads_path")]
    pub beads_path: Option<String>,
}

/// Find workspace config by walking up from `dir` to root.
pub fn find_workspace_config(dir: &Path) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        let candidate = current.join(".bv").join("workspace.yaml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}
use std::path::PathBuf;

/// Load and validate a workspace config.
pub fn load_workspace(path: &Path) -> Result<WorkspaceConfig, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let config: WorkspaceConfig =
        serde_yaml_ng::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))?;

    if config.repos.is_empty() && config.discovery.as_ref().is_some_and(|d| !d.enabled) {
        return Err("workspace requires at least 1 repo or discovery.enabled".into());
    }

    // Check for duplicate prefixes
    let mut prefixes = std::collections::HashSet::new();
    for repo in &config.repos {
        if repo.path.is_empty() {
            return Err("repo.path is required".into());
        }
        let prefix = repo.prefix.as_deref().unwrap_or("").to_lowercase();
        if !prefixes.insert(prefix) {
            return Err(format!(
                "duplicate prefix: {}",
                repo.prefix.as_deref().unwrap_or("")
            ));
        }
    }
    Ok(config)
}

/// Qualify an issue ID with the repo prefix (idempotent).
pub fn qualify_id(prefix: &str, local_id: &str) -> String {
    if local_id.starts_with(prefix) {
        local_id.to_string()
    } else {
        format!("{prefix}{local_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualify_id_is_idempotent() {
        assert_eq!(qualify_id("api-", "AUTH-1"), "api-AUTH-1");
        assert_eq!(qualify_id("api-", "api-AUTH-1"), "api-AUTH-1");
    }

    #[test]
    fn parse_workspace_yaml() {
        let yaml = r#"
repos:
  - name: api
    path: services/api
  - name: web
    path: apps/web
    prefix: web-
"#;
        let config: WorkspaceConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.repos.len(), 2);
        assert_eq!(config.repos[0].path, "services/api");
    }
}
