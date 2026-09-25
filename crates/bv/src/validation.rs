//! Validation — modifier-requires + exclusive-primary checks.
//! Port of Go validateModifierFlags / validateExclusivePrimaryCommands.

use crate::flags::{MODIFIER_REQUIRES, ROBOT_PRIMARIES};
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidationError {
    /// Go renders the acceptable set as a comma list with a final " or"
    /// (robot_registry.go:3797, joinRobotFlags at 3830).
    #[error("{message}")]
    MissingRequirement {
        modifier: String,
        required: String,
        message: String,
    },
    #[error("only one primary command allowed (found {count}: {found})")]
    ExclusivePrimaries { count: usize, found: String },
    /// Go rejects `--watch-export` alongside `--as-of` before any export runs
    /// (cmd/bv/main.go:1903-1906): watching would re-export the snapshot
    /// repeatedly instead of exporting the one fixed ref the user asked for.
    #[error("--watch-export cannot be combined with --as-of; omit --watch-export to export a fixed historical snapshot.")]
    WatchExportWithAsOf,
    /// Enum-value check happens at clap parse time (Phase 3c wiring).
    #[allow(dead_code)]
    #[error("invalid value for --{flag}: {value}")]
    BadEnum { flag: String, value: String },
}

/// Which flags are present in the parsed invocation. Owns its strings —
/// no leaks; lifetimes are self-contained.
pub struct Presence {
    present: HashSet<String>,
}

impl Presence {
    pub fn from_args(args: &[String]) -> Self {
        let mut present = HashSet::new();
        for arg in args {
            let name = arg.strip_prefix("--").unwrap_or(arg);
            let name = name.split('=').next().unwrap_or(name);
            present.insert(name.to_string());
        }
        Presence { present }
    }

    pub fn has(&self, flag: &str) -> bool {
        self.present.contains(flag)
    }
}

/// Render a co-flag set the way Go's `joinRobotFlags` does: every flag
/// prefixed with `--`, comma-separated, with a final " or" before the last
/// (robot_registry.go:3830). A two-entry set reads "a or b"; a one-entry set
/// has no conjunction at all.
fn format_required(required: &[&str]) -> String {
    let flags: Vec<String> = required.iter().map(|r| format!("--{r}")).collect();
    match flags.len() {
        0 => String::new(),
        1 => flags[0].clone(),
        _ => {
            let head = flags[..flags.len() - 1].join(", ");
            format!("{head} or {}", flags[flags.len() - 1])
        }
    }
}

/// Validate modifier-requires rules. Returns list of violations.
pub fn validate_modifier_requires(present: &Presence) -> Vec<ValidationError> {
    let mut violations = Vec::new();
    for (modifier, required) in MODIFIER_REQUIRES {
        if present.has(modifier) && !required.iter().any(|r| present.has(r)) {
            let rendered = format_required(required);
            violations.push(ValidationError::MissingRequirement {
                modifier: (*modifier).to_string(),
                required: rendered.clone(),
                // Go appends the recovery examples to the same message
                // (cmd/bv/main.go:238), so an agent reading stderr sees the
                // concrete invocations that satisfy the rule, not just the
                // requirement.
                message: if required.len() > 1 {
                    format!("--{modifier} requires one of {rendered}")
                } else {
                    format!("--{modifier} requires {rendered}")
                }
                .trim_end()
                .to_string()
                    + &crate::flags::format_modifier_recovery_examples(modifier),
            });
        }
    }
    violations
}

/// Validate exclusive primary groups. Returns violation when >1 primary set.
///
/// Also carries the `--watch-export`/`--as-of` cross-flag rejection, which Go
/// runs as a separate check immediately after the primary-group one
/// (cmd/bv/main.go:1898-1906). It lives here so `main` stays the only caller:
/// the check has to fire before the export-pages handler writes anything, and
/// folding it into an already-dispatched validation pass is what keeps it
/// ordered correctly without a second call site.
pub fn validate_exclusive_primaries(present: &Presence) -> Vec<ValidationError> {
    let mut group_counts: HashMap<&str, Vec<&str>> = HashMap::new();
    for f in ROBOT_PRIMARIES {
        if f.primary && present.has(f.name) {
            if let Some(g) = f.group {
                group_counts.entry(g).or_default().push(f.name);
            }
        }
    }
    let mut violations = Vec::new();
    for (_g, names) in group_counts {
        if names.len() > 1 {
            violations.push(ValidationError::ExclusivePrimaries {
                count: names.len(),
                found: names.join(", "),
            });
        }
    }
    if present.has("watch-export") && present.has("as-of") {
        violations.push(ValidationError::WatchExportWithAsOf);
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::argv::rewrite_args;

    fn check(args: &[&str]) -> Vec<ValidationError> {
        let rewritten = rewrite_args(&args.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        let p = Presence::from_args(&rewritten);
        let mut v = validate_modifier_requires(&p);
        v.extend(validate_exclusive_primaries(&p));
        v
    }

    #[test]
    fn robot_diff_requires_diff_since() {
        let v = check(&["bvr", "--robot-diff"]);
        assert!(matches!(
            v.first(),
            Some(ValidationError::MissingRequirement { .. })
        ));
        assert!(check(&["bvr", "--robot-diff", "--diff-since", "HEAD~5"]).is_empty());
    }

    #[test]
    fn brief_requires_triage_family() {
        // Go rule: brief requires triage/by-track/by-label — NOT robot-next
        assert!(!check(&["bvr", "--brief"]).is_empty());
        assert!(check(&["bvr", "--robot-triage", "--brief"]).is_empty());
        assert!(!check(&["bvr", "--robot-next", "--brief"]).is_empty());
    }

    #[test]
    fn history_timeout_includes_next() {
        // Go rule: robot-history-timeout-ms DOES include robot-next
        assert!(check(&["bvr", "--robot-next", "--robot-history-timeout-ms", "5000"]).is_empty());
    }

    #[test]
    fn history_window_modifiers_accept_robot_causality() {
        // Go main.go:1811-1812 — both history-since and history-limit list
        // robot-causality alongside the two history commands.
        for modifier in ["--history-since", "--history-limit"] {
            assert!(
                check(&["bvr", "--robot-causality", "A-1", modifier, "30d"]).is_empty(),
                "{modifier} must be allowed with --robot-causality"
            );
            assert!(
                !check(&["bvr", "--robot-triage", modifier, "30d"]).is_empty(),
                "{modifier} must still be rejected without a history command"
            );
        }
    }

    #[test]
    fn two_triage_family_flags_conflict() {
        let v = check(&["bvr", "--robot-triage", "--robot-next"]);
        assert!(matches!(
            v.first(),
            Some(ValidationError::ExclusivePrimaries { count: 2, .. })
        ));
    }

    #[test]
    fn single_primary_is_fine() {
        assert!(check(&["bvr", "--robot-triage"]).is_empty());
        assert!(check(&["bvr", "--robot-insights"]).is_empty());
    }

    #[test]
    fn unrelated_modifiers_ignored() {
        assert!(check(&["bvr", "--robot-insights", "--format", "toon"]).is_empty());
    }

    /// Go rejects `--watch-export` together with `--as-of` (cmd/bv/main.go:1903-1906,
    /// exit 1) instead of exporting a snapshot it would then re-watch. Either
    /// flag alone stays valid.
    #[test]
    fn watch_export_conflicts_with_as_of() {
        let v = check(&[
            "bvr",
            "--export-pages",
            "out",
            "--watch-export",
            "--as-of",
            "HEAD",
        ]);
        assert!(
            v.iter()
                .any(|e| matches!(e, ValidationError::WatchExportWithAsOf)),
            "expected the watch-export/as-of conflict, got {v:?}"
        );
        // The recovery advice is the whole point of the message.
        assert_eq!(
            v.iter()
                .find(|e| matches!(e, ValidationError::WatchExportWithAsOf))
                .map(|e| e.to_string()),
            Some("--watch-export cannot be combined with --as-of; omit --watch-export to export a fixed historical snapshot.".to_string())
        );

        assert!(check(&["bvr", "--export-pages", "out", "--watch-export"]).is_empty());
        assert!(check(&["bvr", "--export-pages", "out", "--as-of", "HEAD"]).is_empty());
    }
}
