//! Argv rewriter — port of Go argv normalization:
//! - single-dash long flags (`-robot-triage`) -> double dash
//! - agent-intent aliases: `bv triage` -> `bv --robot-triage`
//! - bare `--json` (when no primary) -> `bv --robot-triage --json`

/// Rewrite raw args into canonical form.
pub fn rewrite_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        // single-dash long flag: starts with exactly one '-', len > 2, not a
        // known short cluster, and the rest matches a long-flag pattern.
        // Skip single-char flags like -o=val or -f (flag name before '=' or
        // end is exactly one character — that's a short flag, not a long flag).
        let flag_name_len = arg[1..].find('=').unwrap_or(arg.len() - 1);
        let is_long_flag = arg.len() > 2
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && flag_name_len > 1
            && arg[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '=');
        if is_long_flag {
            out.push(format!("-{}", arg));
        } else {
            out.push(arg.clone());
        }
    }

    // Agent intent aliases on the first bare positional (skipping prog name).
    const ALIASES: &[(&str, &str)] = &[
        // Existing 9
        ("triage", "--robot-triage"),
        ("next", "--robot-next"),
        ("plan", "--robot-plan"),
        ("insights", "--robot-insights"),
        ("priority", "--robot-priority"),
        ("alerts", "--robot-alerts"),
        ("suggest", "--robot-suggest"),
        ("graph", "--robot-graph"),
        ("history", "--robot-history"),
        // Triage family aliases
        ("recommend", "--robot-triage"),
        ("recommendations", "--robot-triage"),
        // Next family
        ("pick", "--robot-next"),
        // Insights family
        ("insight", "--robot-insights"),
        ("analysis", "--robot-insights"),
        ("analyze", "--robot-insights"),
        // Priority family
        ("priorities", "--robot-priority"),
        // Suggest family
        ("suggestions", "--robot-suggest"),
        // Schema / docs / search
        ("schemas", "--robot-schema"),
        ("schema", "--robot-schema"),
        ("docs", "--robot-docs"),
        ("doc", "--robot-docs"),
        ("find", "--robot-search"),
        ("search", "--robot-search"),
        // Labels
        ("labels", "--robot-label-health"),
        // File analysis
        ("hotspots", "--robot-file-hotspots"),
        ("impact", "--robot-impact"),
        ("related", "--robot-related"),
        ("blockers", "--robot-blocker-chain"),
        ("blocker-chain", "--robot-blocker-chain"),
        ("impact-network", "--robot-impact-network"),
        ("causality", "--robot-causality"),
        // Sprints / forecast / capacity
        ("sprints", "--robot-sprint-list"),
        ("sprint-list", "--robot-sprint-list"),
        ("sprint", "--robot-sprint-show"),
        ("sprint-show", "--robot-sprint-show"),
        ("forecast", "--robot-forecast"),
        ("capacity", "--robot-capacity"),
        ("burndown", "--robot-burndown"),
        // Upgrade
        ("upgrade", "--update"),
        ("self-update", "--update"),
        ("selfupdate", "--update"),
        // Meta
        ("capabilities", "--robot-capabilities"),
        ("capability", "--robot-capabilities"),
        ("manifest", "--robot-capabilities"),
        ("recipes", "--robot-recipes"),
        ("metrics", "--robot-metrics"),
        // Diff / drift / orphans / correlation-stats
        ("diff", "--robot-diff"),
        ("drift", "--robot-drift"),
        ("orphans", "--robot-orphans"),
        ("correlation-stats", "--robot-correlation-stats"),
    ];
    // Input here is argv-minus-program; scan from index 0.
    let mut replaced = false;
    for slot in out.iter_mut() {
        if replaced {
            break;
        }
        if slot.starts_with('-') {
            break; // aliases only valid as first positional
        }
        if let Some((_, flag)) = ALIASES.iter().find(|(a, _)| slot == a) {
            *slot = flag.to_string();
            replaced = true;
        }
    }

    // Bare --json auto-promote: if no robot primary flag is present after alias
    // rewriting, but the user passed a structured-output flag (--json, --toon,
    // --output=json, -o=json, etc.), insert --robot-triage as the default
    // primary command. This is the key agent ergonomic.
    let has_robot_primary = out.iter().any(|arg| {
        let name = arg.split('=').next().unwrap_or(arg);
        let name = name.strip_prefix("--").unwrap_or(name);
        ROBOT_PRIMARY_NAMES.contains(&name)
    });
    if !has_robot_primary && contains_structured_output_alias(&out) {
        // Insert --robot-triage after the program name (index 0).
        out.insert(1, "--robot-triage".to_string());
    }

    out
}

/// Check whether args contain a structured-output alias.
fn contains_structured_output_alias(args: &[String]) -> bool {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--json"
            || arg == "--json=true"
            || arg == "--json=false"
            || arg == "--toon"
            || arg == "--toon=true"
            || arg == "--toon=false"
        {
            return true;
        }
        if arg.eq_ignore_ascii_case("--output=json")
            || arg.eq_ignore_ascii_case("-o=json")
            || arg.eq_ignore_ascii_case("--output=toon")
            || arg.eq_ignore_ascii_case("-o=toon")
        {
            return true;
        }
        if (arg == "--output" || arg == "-o") && i + 1 < args.len() {
            let next = &args[i + 1];
            if next == "json" || next == "toon" {
                return true;
            }
        }
    }
    false
}

/// Robot primary flag names (stripped of `--` prefix) for auto-promote check.
const ROBOT_PRIMARY_NAMES: &[&str] = &[
    "robot-help",
    "robot-capabilities",
    "robot-docs",
    "robot-schema",
    "robot-recipes",
    "robot-metrics",
    "robot-triage",
    "robot-next",
    "robot-triage-by-track",
    "robot-triage-by-label",
    "robot-insights",
    "robot-plan",
    "robot-priority",
    "robot-alerts",
    "robot-suggest",
    "robot-graph",
    "robot-search",
    "robot-diff",
    "robot-drift",
    "robot-history",
    "robot-explain-correlation",
    "robot-confirm-correlation",
    "robot-reject-correlation",
    "robot-correlation-stats",
    "robot-orphans",
    "robot-file-beads",
    "robot-file-hotspots",
    "robot-impact",
    "robot-file-relations",
    "robot-related",
    "robot-blocker-chain",
    "robot-impact-network",
    "robot-causality",
    "robot-sprint-list",
    "robot-sprint-show",
    "robot-forecast",
    "robot-capacity",
    "robot-burndown",
    "robot-label-health",
    "robot-label-flow",
    "robot-label-attention",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn single_dash_long_flag_normalized() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "-robot-triage"])),
            s(&["bvr", "--robot-triage"])
        );
    }

    #[test]
    fn short_flags_untouched() {
        // -l is a legit short flag; stays as-is
        assert_eq!(rewrite_args(&s(&["bvr", "-l"])), s(&["bvr", "-l"]));
    }

    #[test]
    fn agent_intent_alias_rewritten() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "triage"])),
            s(&["bvr", "--robot-triage"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "next"])),
            s(&["bvr", "--robot-next"])
        );
    }

    #[test]
    fn double_dash_passthrough() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--robot-insights", "--format", "toon"])),
            s(&["bvr", "--robot-insights", "--format", "toon"])
        );
    }

    #[test]
    fn bare_json_auto_promotes_to_triage() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--json"])),
            s(&["bvr", "--robot-triage", "--json"])
        );
    }

    #[test]
    fn bare_toon_auto_promotes_to_triage() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--toon"])),
            s(&["bvr", "--robot-triage", "--toon"])
        );
    }

    #[test]
    fn output_json_auto_promotes_to_triage() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--output=json"])),
            s(&["bvr", "--robot-triage", "--output=json"])
        );
    }

    #[test]
    fn o_json_auto_promotes_to_triage() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "-o=json"])),
            s(&["bvr", "--robot-triage", "-o=json"])
        );
    }

    #[test]
    fn json_with_existing_primary_not_promoted() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--robot-insights", "--json"])),
            s(&["bvr", "--robot-insights", "--json"])
        );
    }

    #[test]
    fn output_space_json_auto_promotes() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--output", "json"])),
            s(&["bvr", "--robot-triage", "--output", "json"])
        );
    }

    #[test]
    fn alias_recommend_goes_to_triage() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "recommend"])),
            s(&["bvr", "--robot-triage"])
        );
    }

    #[test]
    fn alias_pick_goes_to_next() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "pick"])),
            s(&["bvr", "--robot-next"])
        );
    }

    #[test]
    fn alias_upgrade_goes_to_update() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "upgrade"])),
            s(&["bvr", "--update"])
        );
    }

    #[test]
    fn alias_capabilities_goes_to_robot_capabilities() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "capabilities"])),
            s(&["bvr", "--robot-capabilities"])
        );
    }

    #[test]
    fn alias_diff_goes_to_robot_diff() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "diff"])),
            s(&["bvr", "--robot-diff"])
        );
    }

    #[test]
    fn alias_drift_goes_to_robot_drift() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "drift"])),
            s(&["bvr", "--robot-drift"])
        );
    }

    #[test]
    fn alias_find_goes_to_robot_search() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "find"])),
            s(&["bvr", "--robot-search"])
        );
    }

    #[test]
    fn alias_blockers_goes_to_robot_blocker_chain() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "blockers"])),
            s(&["bvr", "--robot-blocker-chain"])
        );
    }

    #[test]
    fn no_auto_promote_when_version() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "--version"])),
            s(&["bvr", "--version"])
        );
    }
}
