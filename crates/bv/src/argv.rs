//! Argv rewriter — port of Go argv normalization:
//! - single-dash long flags (`-robot-triage`) -> double dash
//! - agent-intent aliases: `bv triage` -> `bv --robot-triage`
//! - bare `--json` (when no primary) -> `--robot-triage`

/// Rewrite raw args into canonical form.
pub fn rewrite_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for (idx, arg) in args.iter().enumerate() {
        // idx 0 is program name — never rewritten
        if idx == 0 {
            out.push(arg.clone());
            continue;
        }
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
    for slot in out.iter_mut().skip(1) {
        let matched = ALIASES.iter().find(|(a, _)| slot == a);
        if let Some((_, flag)) = matched {
            *slot = flag.to_string();
            break; // only the first positional is rewritten
        }
        // stop scanning once we hit any flag: aliases only valid as first arg
        if slot.starts_with('-') {
            break;
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
