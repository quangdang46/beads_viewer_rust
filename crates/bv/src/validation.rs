//! Validation — modifier-requires + exclusive-primary checks.
//! Port of Go validateModifierFlags / validateExclusivePrimaryCommands.

use crate::flags::{MODIFIER_REQUIRES, ROBOT_PRIMARIES};
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidationError {
    #[error("flag --{modifier} requires --{required}")]
    MissingRequirement { modifier: String, required: String },
    #[error("only one primary command allowed (found {count}: {found})")]
    ExclusivePrimaries { count: usize, found: String },
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

/// Validate modifier-requires rules. Returns list of violations.
pub fn validate_modifier_requires(present: &Presence) -> Vec<ValidationError> {
    let mut violations = Vec::new();
    for (modifier, required) in MODIFIER_REQUIRES {
        if present.has(modifier) && !required.iter().any(|r| present.has(r)) {
            violations.push(ValidationError::MissingRequirement {
                modifier: (*modifier).to_string(),
                required: (*required[0]).to_string(),
            });
        }
    }
    violations
}

/// Validate exclusive primary groups. Returns violation when >1 primary set.
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
}
