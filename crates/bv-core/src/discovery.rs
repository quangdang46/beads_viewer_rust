//! Datasource discovery + GitLoader (`--as-of`) — port of Go
//! `pkg/loader/loader.go` (discovery chain) and `pkg/loader/git.go`.

use crate::loader::{parse_issues_with_options, ParseOptions};
use crate::model::Issue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

pub const BEADS_DB_ENV: &str = "BEADS_DB";
pub const BEADS_DIR_ENV: &str = "BEADS_DIR";

/// Priority order for beads JSONL filenames (Go: PreferredJSONLNames).
pub const PREFERRED_JSONL_NAMES: [&str; 3] = ["issues.jsonl", "beads.jsonl", "beads.base.jsonl"];

const MAX_REDIRECT_BYTES: u64 = 4096;
const MAX_REDIRECT_DEPTH: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("redirect file exceeds maximum size of {max} bytes: {path}")]
    RedirectTooLarge { max: u64, path: String },
    #[error("redirect file must be valid UTF-8: {0}")]
    RedirectNotUtf8(String),
    #[error("redirect loop detected at {0}")]
    RedirectLoop(String),
    #[error("redirect target must be a .beads or _beads directory: {0}")]
    RedirectBadTarget(String),
    #[error("max redirect depth ({0}) exceeded")]
    RedirectDepth(usize),
    #[error("git: {0}")]
    Git(String),
}

/// Go: `looksLikeBeadsDBFile`.
fn looks_like_beads_db_file(db_path: &str) -> bool {
    let ext = Path::new(db_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "jsonl" | "db" | "sqlite" | "sqlite3")
}

/// Go: `resolveBeadsDB` — BEADS_DB may be a file (-> parent dir) or a dir.
fn resolve_beads_db(db_path: &str) -> PathBuf {
    let p = Path::new(db_path);
    match std::fs::metadata(p) {
        Ok(meta) => {
            if meta.is_dir() {
                p.to_path_buf()
            } else {
                p.parent()
                    .map(|x| x.to_path_buf())
                    .unwrap_or_else(|| p.to_path_buf())
            }
        }
        Err(_) => {
            if looks_like_beads_db_file(db_path) {
                p.parent()
                    .map(|x| x.to_path_buf())
                    .unwrap_or_else(|| p.to_path_buf())
            } else {
                p.to_path_buf()
            }
        }
    }
}

/// Go: `readBeadsRedirect` — parse one `.beads/redirect` file.
fn read_beads_redirect(beads_dir: &Path) -> Result<Option<PathBuf>, DiscoveryError> {
    let redirect = beads_dir.join("redirect");
    let meta = match std::fs::metadata(&redirect) {
        Ok(m) if !m.is_dir() => m,
        _ => return Ok(None),
    };
    if meta.len() > MAX_REDIRECT_BYTES {
        return Err(DiscoveryError::RedirectTooLarge {
            max: MAX_REDIRECT_BYTES,
            path: redirect.display().to_string(),
        });
    }
    let data = std::fs::read_to_string(&redirect)
        .map_err(|_| DiscoveryError::RedirectNotUtf8(redirect.display().to_string()))?;
    let target = data.trim();
    if target.is_empty() {
        return Ok(None);
    }
    let mut path = PathBuf::from(target);
    if !path.is_absolute() {
        path = beads_dir.join(path);
    }
    Ok(Some(path))
}

/// Go: `followBeadsRedirect` — follow the chain with loop/depth guards.
fn follow_beads_redirect(beads_dir: &Path) -> Result<PathBuf, DiscoveryError> {
    let mut current = beads_dir.to_path_buf();
    for depth in 0..=MAX_REDIRECT_DEPTH {
        if depth == MAX_REDIRECT_DEPTH {
            return Err(DiscoveryError::RedirectDepth(MAX_REDIRECT_DEPTH));
        }
        // Target validity: must be named .beads or _beads and be a dir.
        let valid_name = current
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == ".beads" || n == "_beads")
            .unwrap_or(false);
        if current.exists() && !valid_name {
            return Err(DiscoveryError::RedirectBadTarget(
                current.display().to_string(),
            ));
        }
        match read_beads_redirect(&current)? {
            Some(next) => {
                if next == current {
                    return Err(DiscoveryError::RedirectLoop(current.display().to_string()));
                }
                current = next;
            }
            None => return Ok(current),
        }
    }
    unreachable!()
}

/// Go: `getMainRepoRoot` — worktree-aware main-repo resolution.
fn get_main_repo_root(repo_path: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let worktree_root = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let common = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo_path)
        .output()
        .ok();
    let git_dir = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-dir"])
        .current_dir(repo_path)
        .output()
        .ok();

    let (common, git_dir) = match (common, git_dir) {
        (Some(c), Some(g)) if c.status.success() && g.status.success() => (
            String::from_utf8_lossy(&c.stdout).trim().to_string(),
            String::from_utf8_lossy(&g.stdout).trim().to_string(),
        ),
        _ => return Some(PathBuf::from(worktree_root)),
    };
    if common == git_dir {
        return Some(PathBuf::from(worktree_root));
    }
    // Worktree: main repo root is parent of common dir (.../main/.git).
    Some(Path::new(&common).parent()?.to_path_buf())
}

/// Go: `GetBeadsDir`. Resolution order:
/// BEADS_DB > BEADS_DIR > `<repo>/.beads` > main-repo `.beads` (worktrees).
pub fn get_beads_dir(repo_path: &Path) -> Result<PathBuf, DiscoveryError> {
    if let Ok(env_db) = std::env::var(BEADS_DB_ENV) {
        if !env_db.is_empty() {
            return follow_beads_redirect(&resolve_beads_db(&env_db));
        }
    }
    if let Ok(env_dir) = std::env::var(BEADS_DIR_ENV) {
        if !env_dir.is_empty() {
            return follow_beads_redirect(Path::new(&env_dir));
        }
    }
    let repo = if repo_path.as_os_str().is_empty() {
        std::env::current_dir().map_err(|e| DiscoveryError::Git(e.to_string()))?
    } else {
        repo_path.to_path_buf()
    };
    let local = repo.join(".beads");
    if local.exists() {
        return follow_beads_redirect(&local);
    }
    if let Some(main_root) = get_main_repo_root(&repo) {
        if main_root != repo {
            let main_beads = main_root.join(".beads");
            if main_beads.exists() {
                return follow_beads_redirect(&main_beads);
            }
        }
    }
    Ok(local)
}

/// Go: `FindJSONLPathWithWarnings`. Picks the first preferred name with
/// size > 0; skips backups/merge artifacts; warns about left/right files.
pub fn find_jsonl_path_with_warnings(
    beads_dir: &Path,
    mut warn: impl FnMut(&str),
) -> Result<Option<PathBuf>, DiscoveryError> {
    let entries = match std::fs::read_dir(beads_dir) {
        Ok(e) => e,
        Err(e) => {
            return Err(DiscoveryError::Git(format!(
                "failed to read beads directory {}: {}",
                beads_dir.display(),
                e
            )))
        }
    };
    let mut candidates: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".jsonl") {
            continue;
        }
        if name.contains(".backup") || name.contains(".orig") || name.contains(".merge") {
            continue;
        }
        if name == "deletions.jsonl" {
            continue;
        }
        if name.starts_with("beads.left") || name.starts_with("beads.right") {
            warn(&format!(
                "Merge artifact files detected: {}. Clean them up before relying on the JSONL view.",
                name
            ));
            continue;
        }
        candidates.push(entry.file_name().to_string_lossy().to_string());
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    for preferred in PREFERRED_JSONL_NAMES {
        for name in &candidates {
            if name == preferred {
                let path = beads_dir.join(name);
                if let Ok(meta) = std::fs::metadata(&path) {
                    if meta.len() > 0 {
                        return Ok(Some(path));
                    }
                }
            }
        }
    }
    // Fall back to first non-empty candidate.
    for name in &candidates {
        let path = beads_dir.join(name);
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 0 {
                return Ok(Some(path));
            }
        }
    }
    Ok(Some(beads_dir.join(&candidates[0])))
}

/// Convenience: discovery + file pick + tolerant load.
pub fn load_issues_from_repo(
    repo_path: &Path,
) -> Result<(Vec<Issue>, crate::loader::ParseStats), DiscoveryError> {
    let beads_dir = get_beads_dir(repo_path)?;
    let jsonl = find_jsonl_path_with_warnings(&beads_dir, |_| {})?.ok_or_else(|| {
        DiscoveryError::Git(format!("no beads JSONL found in {}", beads_dir.display()))
    })?;
    let raw = std::fs::read_to_string(&jsonl)
        .map_err(|e| DiscoveryError::Git(format!("reading {}: {}", jsonl.display(), e)))?;
    let mut rdr = raw.as_bytes();
    parse_issues_with_options(&mut rdr, &ParseOptions::default(), |_| {})
        .map_err(|e| DiscoveryError::Git(e.to_string()))
}

// ---------------------------------------------------------------------------
// GitLoader (--as-of)
// ---------------------------------------------------------------------------

struct CacheEntry {
    issues: Vec<Issue>,
    loaded_at: Instant,
}

/// Go: `GitLoader` — SHA-keyed revision cache with TTL.
pub struct GitLoader {
    repo_path: PathBuf,
    cache: Mutex<HashMap<String, CacheEntry>>,
    max_age: Duration,
}

impl GitLoader {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        GitLoader {
            repo_path: repo_path.into(),
            cache: Mutex::new(HashMap::new()),
            max_age: Duration::from_secs(5 * 60),
        }
    }

    fn git(&self, args: &[&str]) -> Result<String, DiscoveryError> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .output()
            .map_err(|e| DiscoveryError::Git(e.to_string()))?;
        if !out.status.success() {
            return Err(DiscoveryError::Git(format!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Go: `resolveRevision` — rev-parse --verify first, then date fallback.
    pub fn resolve_revision(&self, revision: &str) -> Result<String, DiscoveryError> {
        match self.git(&["rev-parse", "--verify", "--end-of-options", revision]) {
            Ok(sha) => Ok(sha),
            Err(err) => {
                if let Some(t) = parse_date_string(revision) {
                    self.resolve_date_revision(t)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Go: `resolveDateRevision` — last commit at-or-before timestamp.
    fn resolve_date_revision(&self, t: jiff::Timestamp) -> Result<String, DiscoveryError> {
        let out = self.git(&["rev-list", "-1", &format!("--before={t}"), "HEAD"])?;
        if out.is_empty() {
            return Err(DiscoveryError::Git(format!(
                "no commit found at or before {}",
                t
            )));
        }
        Ok(out)
    }

    /// Load issues at a revision (SHA / branch / tag / HEAD~N / date string).
    pub fn load_at(&self, revision: &str) -> Result<Vec<Issue>, DiscoveryError> {
        let sha = self.resolve_revision(revision)?;
        // TTL cache lookup
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(&sha) {
                if entry.loaded_at.elapsed() < self.max_age {
                    return Ok(entry.issues.clone());
                }
            }
        }
        let issues = self.load_from_git(&sha)?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                sha.clone(),
                CacheEntry {
                    issues: issues.clone(),
                    loaded_at: Instant::now(),
                },
            );
        }
        Ok(issues)
    }

    /// Go: `loadFromGit` — try `.beads/<preferred>` paths in order via git show.
    fn load_from_git(&self, sha: &str) -> Result<Vec<Issue>, DiscoveryError> {
        for name in PREFERRED_JSONL_NAMES {
            let path = format!(".beads/{name}");
            if let Ok(raw) = self.git_show(sha, &path) {
                if raw.trim().is_empty() {
                    continue;
                }
                let mut rdr = raw.as_bytes();
                let (issues, _) =
                    parse_issues_with_options(&mut rdr, &ParseOptions::default(), |_| {})
                        .map_err(|e| DiscoveryError::Git(e.to_string()))?;
                return Ok(issues);
            }
        }
        Ok(Vec::new())
    }

    fn git_show(&self, sha: &str, path: &str) -> Result<String, DiscoveryError> {
        self.git(&["show", &format!("{}:{}", sha, path)])
    }
}

/// Go: `parseDateString` — RFC3339, then date-only/datetime layouts in LOCAL time.
fn parse_date_string(s: &str) -> Option<jiff::Timestamp> {
    use jiff::civil;
    // RFC3339 with offset parses directly to Timestamp.
    if let Ok(ts) = s.parse::<jiff::Timestamp>() {
        return Some(ts);
    }
    // Naive layouts: interpret in local timezone like Go ParseInLocation.
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%d"] {
        if let Ok(dt) = civil::DateTime::strptime(fmt, s) {
            let tz = jiff::tz::TimeZone::system();
            if let Ok(ts) = dt.to_zoned(tz) {
                return Some(ts.into());
            }
        }
    }
    None
}

// Silence unused warning until SQLite reader lands (deferred bead note).
#[allow(dead_code)]
fn _placeholder(_x: Option<RwLock<()>>) {}
