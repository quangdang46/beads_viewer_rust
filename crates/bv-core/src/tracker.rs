//! Live tracker route + `actions` suggestions — port of Go
//! `pkg/loader/resolveIssueOrigin` + `pkg/model/types.go` (`IssueOrigin`,
//! `IssueCommand`, `Issue.Actions`).
//!
//! Go attaches an `IssueOrigin` to every loaded issue so a robot payload can
//! suggest executable `show`/`claim` commands against the real tracker. The
//! suggestions are advisory: the tracker re-checks claimability when run.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Go `model.IssueOrigin` (pkg/model/types.go:49).
#[derive(Debug, Clone, Default)]
pub struct IssueOrigin {
    pub local_id: String,
    pub working_directory: String,
    pub tracker_directory: String,
    pub database: String,
    pub tracker: String,
    pub executable: String,
    pub supports_claim: bool,
    pub read_only_reason: String,
}

impl IssueOrigin {
    /// Go `routeAvailable()` (pkg/model/types.go:83).
    pub fn route_available(&self) -> bool {
        !self.executable.is_empty()
            && !self.database.is_empty()
            && !self.working_directory.is_empty()
            && !self.tracker_directory.is_empty()
            && !self.local_id.is_empty()
            && (self.tracker == "br" || self.tracker == "bd")
    }
}

/// Go `model.IssueCommand` (pkg/model/types.go:62).
#[derive(Debug, Clone, Serialize)]
pub struct IssueCommand {
    pub working_directory: String,
    pub argv: Vec<String>,
    pub shell: String,
}

/// Go `model.IssueActions` (pkg/model/types.go:66).
#[derive(Debug, Clone, Default, Serialize)]
pub struct IssueActions {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub working_directory: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub local_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tracker: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show: Option<IssueCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim: Option<IssueCommand>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unavailable_reason: String,
}

/// Go `MutationKind` (pkg/model/types.go:108-116).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    AddDependency,
    Relate,
    RemoveDependency,
    AddLabel,
}

/// Go `Issue.MutationAction` (pkg/model/types.go:120-145). Returns the shell
/// command for a tracker mutation, or the reason it cannot be built — which
/// the caller surfaces as the suggestion's `action_unavailable_reason`
/// metadata. The absent-route wording is Go's own, deliberately not reusing
/// `origin.read_only_reason`.
pub fn mutation_action(
    origin: &IssueOrigin,
    kind: MutationKind,
    peer: Option<&IssueOrigin>,
    value: &str,
) -> Result<String, String> {
    // Go (types.go:120-145) returns the command, or a reason string. Its
    // wording for an absent route is its own, not whatever the origin
    // recorded, so return it explicitly rather than reusing
    // read_only_reason.
    if !origin.route_available() {
        return Err("source has no verified live tracker route".to_string());
    }
    if !origin.read_only_reason.is_empty() {
        return Err(origin.read_only_reason.clone());
    }
    match kind {
        MutationKind::AddLabel => {
            if value.trim().is_empty() || value.contains('\0') {
                return Err("suggested label is empty or contains a NUL byte".to_string());
            }
            Ok(build_command(
                origin,
                true,
                &[
                    "update",
                    "--json",
                    &format!("--add-label={value}"),
                    "--",
                    &origin.local_id,
                ],
            )
            .shell)
        }
        MutationKind::AddDependency => {
            let peer =
                peer.ok_or_else(|| "related issue has no verified live tracker route".to_string())?;
            if !peer.route_available() {
                return Err("related issue has no verified live tracker route".to_string());
            }
            Ok(build_command(
                origin,
                true,
                &["dep", "add", &peer.local_id, "--", &origin.local_id],
            )
            .shell)
        }
        MutationKind::Relate => {
            let peer =
                peer.ok_or_else(|| "related issue has no verified live tracker route".to_string())?;
            if !peer.route_available() {
                return Err("related issue has no verified live tracker route".to_string());
            }
            Ok(build_command(
                origin,
                true,
                &[
                    "dep",
                    "add",
                    &peer.local_id,
                    "--type=related",
                    "--",
                    &origin.local_id,
                ],
            )
            .shell)
        }
        MutationKind::RemoveDependency => {
            let peer =
                peer.ok_or_else(|| "related issue has no verified live tracker route".to_string())?;
            if !peer.route_available() {
                return Err("related issue has no verified live tracker route".to_string());
            }
            Ok(build_command(
                origin,
                true,
                &["dep", "remove", &peer.local_id, "--", &origin.local_id],
            )
            .shell)
        }
    }
}

/// Go `ShellQuote` (pkg/model/types.go:76) — wrap one literal argument.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Go `IssueOrigin.command` (pkg/model/types.go:89). Read-only inspection omits
/// `--no-auto-flush`; mutating operations keep normal export behavior.
pub fn build_command(origin: &IssueOrigin, mutating: bool, operation: &[&str]) -> IssueCommand {
    let mut args: Vec<String> = vec![
        "env".to_string(),
        format!("BEADS_DIR={}", origin.tracker_directory),
        format!("BEADS_DB={}", origin.database),
        format!("BD_DB={}", origin.database),
        origin.executable.clone(),
        "--db".to_string(),
        origin.database.clone(),
    ];
    if origin.tracker == "br" {
        args.push("--no-auto-import".to_string());
        if !mutating {
            args.push("--no-auto-flush".to_string());
        }
    }
    for op in operation {
        args.push((*op).to_string());
    }
    let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
    IssueCommand {
        working_directory: origin.working_directory.clone(),
        argv: args,
        shell: format!(
            "cd -- {} && {}",
            shell_quote(&origin.working_directory),
            quoted.join(" ")
        ),
    }
}

/// Go `Issue.Actions(claimable)` (pkg/model/types.go:163).
pub fn build_actions(origin: &IssueOrigin, claimable: bool) -> IssueActions {
    let mut actions = IssueActions {
        working_directory: origin.working_directory.clone(),
        local_id: origin.local_id.clone(),
        tracker: origin.tracker.clone(),
        ..Default::default()
    };
    if !origin.route_available() {
        actions.unavailable_reason = if origin.read_only_reason.is_empty() {
            "live tracker route is incomplete".to_string()
        } else {
            origin.read_only_reason.clone()
        };
        return actions;
    }
    actions.show = Some(build_command(
        origin,
        false,
        &["show", "--json", "--", &origin.local_id],
    ));
    if !origin.read_only_reason.is_empty() {
        actions.unavailable_reason = origin.read_only_reason.clone();
    } else if !claimable {
        actions.unavailable_reason = "snapshot does not establish claim readiness".to_string();
    } else if !origin.supports_claim {
        actions.unavailable_reason =
            "installed tracker does not advertise atomic --claim".to_string();
    } else {
        actions.claim = Some(build_command(
            origin,
            true,
            &["update", "--json", "--claim", "--", &origin.local_id],
        ));
    }
    actions
}

/// Go `installedTrackerCapabilities` (pkg/loader/loader.go:47) — probe the
/// tracker for the flags the explicit-database route depends on.
pub struct TrackerCapabilities {
    pub executable: String,
    pub claim: bool,
    pub error: String,
}

/// Resolve the `br` executable and detect atomic-claim support. Runs only
/// `update --help`, never a command that opens or mutates a tracker.
pub fn installed_tracker_capabilities(tracker: &str) -> TrackerCapabilities {
    let path = match lookup_path(tracker) {
        Some(p) => p,
        None => {
            return TrackerCapabilities {
                executable: String::new(),
                claim: false,
                error: format!("tracker executable is unavailable: {tracker}"),
            }
        }
    };
    let executable = std::fs::canonicalize(&path).unwrap_or(path);
    let out = std::process::Command::new(&executable)
        .args(["update", "--help"])
        .output();
    let mut caps = TrackerCapabilities {
        executable: executable.to_string_lossy().to_string(),
        claim: false,
        error: String::new(),
    };
    match out {
        Ok(o) if o.status.success() => {
            let help = String::from_utf8_lossy(&o.stdout);
            let has = |flag: &str| help.split_whitespace().any(|f| f == flag);
            if !has("--db")
                || !has("--json")
                || (tracker == "br" && (!has("--no-auto-import") || !has("--no-auto-flush")))
            {
                caps.error = "installed tracker cannot bind the required explicit database route"
                    .to_string();
            }
            caps.claim = has("--claim");
        }
        _ => caps.error = "cannot establish installed tracker capabilities".to_string(),
    }
    caps
}

fn lookup_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = Path::new(name);
        return if p.is_file() {
            Some(p.to_path_buf())
        } else {
            None
        };
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Go `resolveIssueOrigin(sourcePath)` (pkg/loader/loader.go:157) — bind the
/// loaded JSONL to a real tracker route, or explain why it cannot.
pub fn resolve_issue_origin(source_path: &str, local_id: &str) -> IssueOrigin {
    let mut origin = IssueOrigin {
        local_id: local_id.to_string(),
        ..Default::default()
    };
    let refuse = |origin: &mut IssueOrigin, reason: &str| -> IssueOrigin {
        origin.read_only_reason = reason.to_string();
        origin.clone()
    };

    let path = std::fs::canonicalize(source_path).unwrap_or_else(|_| PathBuf::from(source_path));
    let Some(beads_dir) = path.parent() else {
        return refuse(
            &mut origin,
            "source path cannot be resolved to a live tracker",
        );
    };
    let meta_raw = match std::fs::read_to_string(beads_dir.join("metadata.json")) {
        Ok(m) => m,
        Err(_) => return refuse(&mut origin, "source has no readable tracker metadata"),
    };
    let meta: serde_json::Value = match serde_json::from_str(&meta_raw) {
        Ok(m) => m,
        Err(_) => return refuse(&mut origin, "source tracker metadata is invalid"),
    };
    let backend = meta
        .get("backend")
        .and_then(|b| b.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !matches!(backend.as_str(), "" | "sqlite" | "dolt") {
        return refuse(
            &mut origin,
            &format!("unsupported tracker backend: {backend}"),
        );
    }

    let resolve_name = |name: &str| -> PathBuf {
        if name.is_empty() {
            return beads_dir.to_path_buf();
        }
        let p = Path::new(name);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            beads_dir.join(p)
        }
    };
    let database = resolve_name(
        meta.get("database")
            .and_then(|d| d.as_str())
            .unwrap_or("beads.db"),
    );

    origin.tracker_directory = beads_dir.to_string_lossy().to_string();
    origin.working_directory = beads_dir
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    origin.tracker = "br".to_string();
    origin.database = database.to_string_lossy().to_string();

    let caps = installed_tracker_capabilities("br");
    if !caps.error.is_empty() {
        origin.read_only_reason = caps.error;
    }
    origin.executable = caps.executable;
    origin.supports_claim = caps.claim;
    origin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> IssueOrigin {
        IssueOrigin {
            local_id: "abc-1".into(),
            working_directory: "/repo".into(),
            tracker_directory: "/repo/.beads".into(),
            database: "/repo/.beads/beads.db".into(),
            tracker: "br".into(),
            executable: "/usr/local/bin/br".into(),
            supports_claim: true,
            read_only_reason: String::new(),
        }
    }

    #[test]
    fn show_command_argv_matches_go() {
        let cmd = build_command(&origin(), false, &["show", "--json", "--", "abc-1"]);
        assert_eq!(
            cmd.argv,
            vec![
                "env",
                "BEADS_DIR=/repo/.beads",
                "BEADS_DB=/repo/.beads/beads.db",
                "BD_DB=/repo/.beads/beads.db",
                "/usr/local/bin/br",
                "--db",
                "/repo/.beads/beads.db",
                "--no-auto-import",
                "--no-auto-flush",
                "show",
                "--json",
                "--",
                "abc-1",
            ]
        );
    }

    #[test]
    fn claim_command_omits_no_auto_flush() {
        let cmd = build_command(
            &origin(),
            true,
            &["update", "--json", "--claim", "--", "abc-1"],
        );
        assert!(!cmd.argv.contains(&"--no-auto-flush".to_string()));
        assert!(cmd.argv.contains(&"--claim".to_string()));
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
        assert_eq!(shell_quote("plain"), "'plain'");
    }

    #[test]
    fn not_claimable_yields_reason_and_no_claim() {
        let a = build_actions(&origin(), false);
        assert!(a.claim.is_none());
        assert!(a.show.is_some());
        assert_eq!(
            a.unavailable_reason,
            "snapshot does not establish claim readiness"
        );
    }

    #[test]
    fn claimable_emits_both_commands() {
        let a = build_actions(&origin(), true);
        assert!(a.claim.is_some());
        assert!(a.show.is_some());
        assert!(a.unavailable_reason.is_empty());
    }

    #[test]
    fn incomplete_route_reports_reason() {
        let mut o = origin();
        o.executable = String::new();
        let a = build_actions(&o, true);
        assert!(a.show.is_none());
        assert_eq!(a.unavailable_reason, "live tracker route is incomplete");
    }
}
