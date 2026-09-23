//! Argv rewriter — port of Go argv normalization:
//! - single-dash long flags (`-robot-triage`) -> double dash
//! - short-flag aliases (`-f`, `-l`, `-r`) -> their long form
//! - agent-intent aliases: `bv triage` -> `bv --robot-triage`
//! - bare `--json` (when no primary) -> `bv --robot-triage --json`

/// Short-flag -> long-flag aliases advertised in Go `bv --help` (issue #5).
/// Go declares `-f, --format`, `-l, --label`, `-r, --recipe`; `-h, --help` is
/// handled separately by the help path, not by alias expansion.
const SHORT_ALIASES: &[(&str, &str)] = &[("-f", "--format"), ("-l", "--label"), ("-r", "--recipe")];

/// Rewrite raw args into canonical form.
pub fn rewrite_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        // Short-flag alias expansion first: `-f toon` -> `--format toon`,
        // `-l=foo` -> `--label=foo`. Only the flag token itself is rewritten;
        // a following value token is left for the normal parse path.
        let (head, tail) = match arg.strip_prefix('-') {
            Some(rest) if !rest.starts_with('-') => match rest.find('=') {
                Some(eq) => (&rest[..eq], &rest[eq..]),
                None => (rest, ""),
            },
            _ => ("", ""),
        };
        if let Some((_, long)) = SHORT_ALIASES
            .iter()
            .find(|(short, _)| head.len() == 1 && short.trim_start_matches('-') == head)
        {
            out.push(format!("{long}{tail}"));
            continue;
        }

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

    // Upgrade intent expansion (Go rewriteUpgradeIntent parity) — runs AFTER
    // alias rewriting so `upgrade` has already become `--update`. (Go rewriteUpgradeIntent parity):
    // `bvr upgrade [--check|--dry-run|--rollback] [--yes|-y]` maps onto the
    // self-update flags. Bare-word aliases accepted; unknown tokens pass through.
    if out.get(1).map(|s| s.as_str()) == Some("--update") {
        let mut mode = "update";
        let mut yes = false;
        let mut passthrough: Vec<String> = Vec::new();
        for arg in out.iter().skip(2) {
            match arg.to_lowercase().trim() {
                "check" | "--check" | "check-update" | "--check-update" => {
                    if mode == "update" {
                        mode = "check";
                    }
                }
                "dry-run" | "--dry-run" | "dryrun" | "--dryrun" => {
                    if mode == "update" {
                        mode = "dry-run";
                    }
                }
                "rollback" | "--rollback" => mode = "rollback",
                "yes" | "--yes" | "-y" | "force" | "--force" => yes = true,
                _ => passthrough.push(arg.clone()),
            }
        }
        let mut expanded = match mode {
            "check" => vec!["--check-update".to_string()],
            "dry-run" => vec!["--update-dry-run".to_string()],
            "rollback" => vec!["--rollback".to_string()],
            _ => {
                let mut v = vec!["--update".to_string()];
                if yes {
                    v.push("--yes".to_string());
                }
                v
            }
        };
        expanded.extend(passthrough);
        let mut result = vec![out[0].clone()];
        result.extend(expanded);
        return result;
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

/// First bare positional left unconsumed after `rewrite_args`, if any.
///
/// Go's `flag` package rejects any non-flag argument with
/// `unknown command "X" for "bv"`, so only agent-intent words (which
/// `rewrite_args` rewrites into `--robot-*`) and flag values are legal.
/// `bvr` must do the same instead of silently falling through to the
/// interactive TUI — an agent or CI caller has no way to drive a TUI and
/// would just block. Returns `None` when every bare token was consumed
/// (rewritten to a flag) or belongs to a flag's value.
pub fn unconsumed_positional(rewritten: &[String]) -> Option<String> {
    // Track whether the previous token was a flag awaiting a value. A flag
    // that takes a value swallows the next token, so `bv --label triage`
    // must not report `triage` as an unknown command.
    let mut prev_is_flag = false;
    // Input is argv-minus-program (see `rewrite_args` callers), so index 0 is
    // the first real argument — do not skip it.
    for tok in rewritten.iter() {
        if prev_is_flag && !tok.starts_with('-') {
            prev_is_flag = false;
            continue;
        }
        if tok.starts_with('-') {
            prev_is_flag = flag_takes_value(tok);
            continue;
        }
        return Some(tok.clone());
    }
    None
}

/// Whether a `--flag` consumes the following token as its value.
fn flag_takes_value(tok: &str) -> bool {
    let name = tok.split('=').next().unwrap_or(tok);
    let name = name.trim_start_matches('-');
    // A `-`/`--` token written as `name=value` never takes a following value.
    if tok.contains('=') {
        return false;
    }
    // Boolean switches listed in the Go flag registry never consume the next
    // token. Everything else in the registry is a string/number flag.
    !matches!(
        name,
        "help"
            | "version"
            | "robot-robot"
            | "brief"
            | "no-cache"
            | "stats"
            | "export-include-graph"
            | "generate-docs"
            | "force-full-analysis"
            | "profile-startup"
            | "profile-json"
            | "cpu-profile"
            | "check-update"
    )
}

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
        // Unmapped short flags (e.g. -x) still pass through; Go only advertises
        // -f/-l/-r/-h, so anything else is left alone.
        assert_eq!(rewrite_args(&s(&["bvr", "-x"])), s(&["bvr", "-x"]));
    }

    /// Go's `flag` package rejects bare positionals with
    /// `unknown command "X" for "bv"` (exit 1). `bvr` used to fall through to
    /// the TUI instead, which blocks any non-interactive caller.
    #[test]
    fn unknown_positional_is_reported() {
        for word in ["version", "help", "check-update", "rollback", "bogusxyz"] {
            let args = rewrite_args(&s(&[word]));
            assert_eq!(
                unconsumed_positional(&args).as_deref(),
                Some(word),
                "expected {word} to be reported as an unknown command"
            );
        }
    }

    /// A bare word that is an agent-intent alias is rewritten to `--robot-*`
    /// and must therefore NOT be reported as unknown.
    #[test]
    fn intent_alias_is_not_unknown() {
        let args = rewrite_args(&s(&["triage"]));
        assert_eq!(args, s(&["--robot-triage"]));
        assert_eq!(unconsumed_positional(&args), None);
    }

    /// A flag's value must be consumed, never mistaken for a command. This is
    /// the false-positive guard: `bv --label triage` is a filter, not a command.
    #[test]
    fn flag_value_is_not_unknown() {
        for argv in [
            vec!["--label", "triage"],
            vec!["--format", "toon"],
            vec!["--repo", "myrepo"],
            // `--flag=value` carries its own value.
            vec!["--label=triage"],
        ] {
            let args = rewrite_args(&s(&argv));
            assert_eq!(
                unconsumed_positional(&args),
                None,
                "expected {argv:?} to be fully consumed"
            );
        }
    }

    /// A boolean switch does not swallow the next token, so a following bare
    /// word is still an unknown command.
    #[test]
    fn bool_flag_does_not_swallow_next() {
        let args = rewrite_args(&s(&["--brief", "version"]));
        assert_eq!(unconsumed_positional(&args).as_deref(), Some("version"));
    }

    #[test]
    fn no_args_is_clean() {
        assert_eq!(unconsumed_positional(&rewrite_args(&[])), None);
    }

    /// Issue #5: Go `bv --help` advertises `-f/--format`, `-l/--label`, `-r/--recipe`.
    /// These were previously passing through unexpanded (a documented gap).
    #[test]
    fn short_flag_aliases_expanded() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "-f", "toon"])),
            s(&["bvr", "--format", "toon"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "-l", "backend"])),
            s(&["bvr", "--label", "backend"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "-r", "mytable"])),
            s(&["bvr", "--recipe", "mytable"])
        );
    }

    #[test]
    fn short_flag_alias_with_equals_expanded() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "-f=toon"])),
            s(&["bvr", "--format=toon"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "-l=backend"])),
            s(&["bvr", "--label=backend"])
        );
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
    fn upgrade_subcommand_variants() {
        assert_eq!(
            rewrite_args(&s(&["bvr", "upgrade", "check"])),
            s(&["bvr", "--check-update"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "upgrade", "dry-run"])),
            s(&["bvr", "--update-dry-run"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "upgrade", "rollback"])),
            s(&["bvr", "--rollback"])
        );
        assert_eq!(
            rewrite_args(&s(&["bvr", "upgrade", "--yes"])),
            s(&["bvr", "--update", "--yes"])
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
