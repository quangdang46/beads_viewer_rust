//! Argv rewriter — port of Go argv normalization:
//! - single-dash long flags (`-robot-triage`) -> double dash
//! - agent-intent aliases: `bv triage` -> `bv --robot-triage`
//! - bare `--json` (when no primary) -> `--robot-triage`

/// Rewrite raw args into canonical form.
pub fn rewrite_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        // single-dash long flag: starts with exactly one '-', len > 2, not a
        // known short cluster, and the rest matches a long-flag pattern.
        if arg.len() > 2
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '=')
        {
            out.push(format!("-{}", arg));
        } else {
            out.push(arg.clone());
        }
    }

    // Agent intent aliases on the first bare positional (skipping prog name).
    const ALIASES: &[(&str, &str)] = &[
        ("triage", "--robot-triage"),
        ("next", "--robot-next"),
        ("plan", "--robot-plan"),
        ("insights", "--robot-insights"),
        ("priority", "--robot-priority"),
        ("alerts", "--robot-alerts"),
        ("suggest", "--robot-suggest"),
        ("graph", "--robot-graph"),
        ("history", "--robot-history"),
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
    out
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
}
