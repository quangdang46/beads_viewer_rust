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
    // Check for duplicate prefixes (effective, like Go Validate → GetPrefix)
    let mut prefixes = std::collections::HashSet::new();
    for repo in &config.repos {
        if repo.path.is_empty() {
            return Err("repo.path is required".into());
        }
        let prefix = repo.get_prefix().to_lowercase();
        if !prefixes.insert(prefix.clone()) {
            return Err(format!("duplicate prefix: {prefix}"));
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

/// Result of loading one repo in a workspace.
#[derive(Debug, Clone)]
pub struct LoadResult {
    pub repo_name: String,
    pub issue_count: usize,
    pub error: Option<String>,
}

impl RepoConfig {
    /// Effective display name (Go GetName).
    pub fn get_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            std::path::Path::new(&self.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| self.path.clone())
        })
    }

    /// Effective ID prefix (Go GetPrefix): prefix, else lowercased name + "-".
    pub fn get_prefix(&self) -> String {
        if let Some(p) = &self.prefix {
            return p.clone();
        }
        format!("{}-", self.get_name().to_lowercase())
    }

    /// Effective beads dir relative to repo (Go GetBeadsPath).
    pub fn get_beads_path(&self) -> &str {
        self.beads_path.as_deref().unwrap_or(".beads")
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

/// Discover repos under root using the config's discovery settings (Go discoverRepos).
pub fn discover_repos(config: &WorkspaceConfig, root: &Path) -> Vec<RepoConfig> {
    let Some(disc) = &config.discovery else {
        return Vec::new();
    };
    if !disc.enabled {
        return Vec::new();
    }
    let mut found: Vec<RepoConfig> = Vec::new();
    for pattern in &disc.patterns {
        // Walk the pattern path segment by segment (e.g. "packages/*" →
        // root/packages/<name> containing .beads).
        let mut candidates = vec![root.to_path_buf()];
        for (depth, seg) in pattern.split('/').enumerate() {
            if depth >= disc.max_depth {
                break;
            }
            let mut next = Vec::new();
            for base in &candidates {
                let Ok(entries) = std::fs::read_dir(base) else {
                    continue;
                };
                let mut names: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                names.sort();
                for name in names {
                    if disc.exclude.iter().any(|x| x == &name) || name.starts_with('.') {
                        continue;
                    }
                    if seg == "*" || name == *seg {
                        next.push(base.join(&name));
                    }
                }
            }
            candidates = next;
        }
        for c in candidates {
            if !c.join(".beads").is_dir() {
                continue;
            }
            let rel = c
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if rel.is_empty() || found.iter().any(|r| r.path == rel) {
                continue;
            }
            found.push(RepoConfig {
                name: None,
                path: rel,
                prefix: None,
                beads_path: None,
                enabled: Some(true),
            });
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// Load all enabled repos and merge issues with prefixed IDs (Go AggregateLoader.LoadAll).
/// Returns merged issues plus per-repo results.
pub fn load_all(
    config: &WorkspaceConfig,
    root: &Path,
) -> Result<(Vec<crate::model::Issue>, Vec<LoadResult>), String> {
    let mut repos: Vec<RepoConfig> = config
        .repos
        .iter()
        .filter(|r| r.is_enabled())
        .cloned()
        .collect();
    repos.extend(discover_repos(config, root));
    if repos.is_empty() {
        return Err("no enabled repositories in workspace".into());
    }

    let mut all_issues = Vec::new();
    let mut results = Vec::new();
    let mut failures = 0usize;

    for repo in &repos {
        let repo_path = root.join(&repo.path);
        let name = repo.get_name();
        match crate::discovery::load_issues_from_repo(&repo_path) {
            Ok((mut issues, _stats)) => {
                let prefix = repo.get_prefix();
                for issue in &mut issues {
                    issue.id = qualify_id(&prefix, &issue.id);
                    issue.source_repo = name.clone();
                    // Rewrite dependency references into qualified IDs.
                    for dep in &mut issue.dependencies {
                        let target = dep.effective_depends_on().to_string();
                        dep.depends_on_id = qualify_id(&prefix, &target);
                    }
                }
                results.push(LoadResult {
                    repo_name: name,
                    issue_count: issues.len(),
                    error: None,
                });
                all_issues.extend(issues);
            }
            Err(e) => {
                failures += 1;
                results.push(LoadResult {
                    repo_name: name,
                    issue_count: 0,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    if failures == results.len() {
        return Err(format!(
            "all {failures} enabled repositories failed to load"
        ));
    }
    Ok((all_issues, results))
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

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    #[test]
    fn load_all_prefixes_ids_and_merges() {
        let dir = std::env::temp_dir().join(format!("bvr_ws_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for repo in ["api", "web"] {
            let beads = dir.join(repo).join(".beads");
            std::fs::create_dir_all(&beads).unwrap();
            std::fs::write(
                beads.join("issues.jsonl"),
                format!(
                    "{{\"id\":\"{repo}-1\",\"title\":\"T1\",\"status\":\"open\",\"priority\":1,\"issue_type\":\"task\",\"dependencies\":[]}}\n"
                ),
            )
            .unwrap();
        }
        let config = WorkspaceConfig {
            name: Some("test".into()),
            repos: vec![
                RepoConfig {
                    name: None,
                    path: "api".into(),
                    prefix: None,
                    beads_path: None,
                    enabled: None,
                },
                RepoConfig {
                    name: None,
                    path: "web".into(),
                    prefix: None,
                    beads_path: None,
                    enabled: None,
                },
            ],
            discovery: None,
            defaults: None,
        };
        let (issues, results) = load_all(&config, &dir).expect("load_all");
        assert_eq!(issues.len(), 2);
        assert!(issues
            .iter()
            .any(|i| i.id == "api-1" && i.source_repo == "api"));
        assert!(issues
            .iter()
            .any(|i| i.id == "web-1" && i.source_repo == "web"));
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.error.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_all_fails_when_all_repos_fail() {
        let dir = std::env::temp_dir().join(format!("bvr_ws_fail_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = WorkspaceConfig {
            name: None,
            repos: vec![RepoConfig {
                name: None,
                path: "nope".into(),
                prefix: None,
                beads_path: None,
                enabled: None,
            }],
            discovery: None,
            defaults: None,
        };
        assert!(load_all(&config, &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovery_finds_beads_dirs() {
        let dir = std::env::temp_dir().join(format!("bvr_ws_disc_{}", std::process::id()));
        for name in ["api", "packages/web", "node_modules/skip"] {
            let beads = dir.join(name).join(".beads");
            std::fs::create_dir_all(&beads).unwrap();
        }
        let config = WorkspaceConfig {
            name: None,
            repos: vec![],
            discovery: Some(DiscoveryConfig {
                enabled: true,
                patterns: default_patterns(),
                exclude: default_exclude(),
                max_depth: 2,
            }),
            defaults: None,
        };
        let found = discover_repos(&config, &dir);
        let paths: Vec<&str> = found.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"api"));
        assert!(paths.contains(&"packages/web"));
        assert!(!paths.iter().any(|p| p.contains("node_modules")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
