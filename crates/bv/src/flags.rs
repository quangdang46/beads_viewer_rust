//! Flag registry — port of Go `cmd/bv` flag definitions (main.go:1417-1663)
//! with category grouping, robot-primary classification, and the
//! modifier-requires validation table (main.go:1699-1780).

/// A single CLI flag definition.
#[derive(Debug, Clone)]
pub struct FlagDef {
    /// Long name without leading dashes.
    pub name: &'static str,
    /// Value kind — used for clap value parsing and validation.
    pub kind: FlagKind,
    /// True when this flag alone is a "primary command" (exclusive group).
    pub primary: bool,
    /// Exclusive group id when primary (Go: 37 groups).
    pub group: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    Bool,
    Str,
    Int,
    Float,
    RepeatableStr,
}

const fn b(name: &'static str) -> FlagDef {
    FlagDef {
        name,
        kind: FlagKind::Bool,
        primary: false,
        group: None,
    }
}
const fn s(name: &'static str) -> FlagDef {
    FlagDef {
        name,
        kind: FlagKind::Str,
        primary: false,
        group: None,
    }
}
const fn i(name: &'static str) -> FlagDef {
    FlagDef {
        name,
        kind: FlagKind::Int,
        primary: false,
        group: None,
    }
}

/// Robot primary commands (~41). Group = exclusive-command family.
pub const ROBOT_PRIMARIES: &[FlagDef] = &[
    FlagDef {
        name: "robot-help",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("meta"),
    },
    FlagDef {
        name: "robot-capabilities",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("meta"),
    },
    FlagDef {
        name: "robot-docs",
        kind: FlagKind::Str,
        primary: true,
        group: Some("meta"),
    },
    FlagDef {
        name: "robot-schema",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("meta"),
    },
    FlagDef {
        name: "robot-recipes",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("meta"),
    },
    FlagDef {
        name: "robot-metrics",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("meta"),
    },
    // triage family shares one group
    FlagDef {
        name: "robot-triage",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("triage"),
    },
    FlagDef {
        name: "robot-next",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("triage"),
    },
    FlagDef {
        name: "robot-triage-by-track",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("triage"),
    },
    FlagDef {
        name: "robot-triage-by-label",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("triage"),
    },
    FlagDef {
        name: "robot-insights",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("insights"),
    },
    FlagDef {
        name: "robot-plan",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("plan"),
    },
    FlagDef {
        name: "robot-priority",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("priority"),
    },
    FlagDef {
        name: "robot-alerts",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("alerts"),
    },
    FlagDef {
        name: "robot-suggest",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("suggest"),
    },
    FlagDef {
        name: "robot-graph",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("graph"),
    },
    FlagDef {
        name: "robot-search",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("search"),
    },
    FlagDef {
        name: "robot-diff",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("diff"),
    },
    FlagDef {
        name: "robot-drift",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("drift"),
    },
    FlagDef {
        name: "robot-history",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("history"),
    },
    FlagDef {
        name: "robot-explain-correlation",
        kind: FlagKind::Str,
        primary: true,
        group: Some("corr-feedback"),
    },
    FlagDef {
        name: "robot-confirm-correlation",
        kind: FlagKind::Str,
        primary: true,
        group: Some("corr-feedback"),
    },
    FlagDef {
        name: "robot-reject-correlation",
        kind: FlagKind::Str,
        primary: true,
        group: Some("corr-feedback"),
    },
    FlagDef {
        name: "robot-correlation-stats",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("corr-stats"),
    },
    FlagDef {
        name: "robot-orphans",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("orphans"),
    },
    FlagDef {
        name: "robot-file-beads",
        kind: FlagKind::Str,
        primary: true,
        group: Some("file-beads"),
    },
    FlagDef {
        name: "robot-file-hotspots",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("hotspots"),
    },
    FlagDef {
        name: "robot-impact",
        kind: FlagKind::Str,
        primary: true,
        group: Some("impact"),
    },
    FlagDef {
        name: "robot-file-relations",
        kind: FlagKind::Str,
        primary: true,
        group: Some("file-relations"),
    },
    FlagDef {
        name: "robot-related",
        kind: FlagKind::Str,
        primary: true,
        group: Some("related"),
    },
    FlagDef {
        name: "robot-blocker-chain",
        kind: FlagKind::Str,
        primary: true,
        group: Some("blocker-chain"),
    },
    FlagDef {
        name: "robot-impact-network",
        kind: FlagKind::Str,
        primary: true,
        group: Some("impact-network"),
    },
    FlagDef {
        name: "robot-causality",
        kind: FlagKind::Str,
        primary: true,
        group: Some("causality"),
    },
    FlagDef {
        name: "robot-sprint-list",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("sprints"),
    },
    FlagDef {
        name: "robot-sprint-show",
        kind: FlagKind::Str,
        primary: true,
        group: Some("sprints"),
    },
    FlagDef {
        name: "robot-forecast",
        kind: FlagKind::Str,
        primary: true,
        group: Some("forecast"),
    },
    FlagDef {
        name: "robot-capacity",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("capacity"),
    },
    FlagDef {
        name: "robot-burndown",
        kind: FlagKind::Str,
        primary: true,
        group: Some("burndown"),
    },
    FlagDef {
        name: "robot-label-health",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("label-health"),
    },
    FlagDef {
        name: "robot-label-flow",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("label-flow"),
    },
    FlagDef {
        name: "robot-label-attention",
        kind: FlagKind::Bool,
        primary: true,
        group: Some("label-attention"),
    },
];

/// Non-primary flags (general/scoping/modifiers). Complete Go inventory.
pub const MODIFIER_FLAGS: &[FlagDef] = &[
    // General
    b("version"),
    s("db"),
    b("update"),
    b("check-update"),
    b("rollback"),
    b("update-dry-run"),
    b("yes"),
    s("format"),
    b("stats"),
    b("profile-startup"),
    b("profile-json"),
    b("no-cache"),
    b("force-full-analysis"),
    s("theme"),
    b("background-mode"),
    b("no-background-mode"),
    s("cpu-profile"),
    // Triage modifiers
    b("brief"),
    i("attention-limit"),
    s("robot-not-ready-labels"),
    i("robot-history-timeout-ms"),
    // Graph export
    s("graph-format"),
    s("graph-root"),
    i("graph-depth"),
    s("graph-preset"),
    s("graph-title"),
    s("export-graph"),
    // Alerts/suggest filters
    s("severity"),
    s("alert-type"),
    s("alert-label"),
    s("suggest-type"),
    s("suggest-confidence"),
    s("suggest-bead"),
    // Scoping
    s("label"),
    s("recipe"),
    s("workspace"),
    s("repo"),
    // History/correlation
    s("as-of"),
    s("diff-since"),
    s("save-baseline"),
    b("baseline-info"),
    b("check-drift"),
    s("bead-history"),
    s("history-since"),
    i("history-limit"),
    s("min-confidence"),
    s("id-pattern"), // repeatable
    s("correlation-by"),
    s("correlation-reason"),
    i("orphans-min-score"),
    i("file-beads-limit"),
    i("hotspots-limit"),
    s("relations-threshold"),
    i("relations-limit"),
    s("related-min-relevance"),
    i("related-max-results"),
    b("related-include-closed"),
    i("network-depth"),
    // Search
    s("search"),
    i("search-limit"),
    s("search-mode"),
    s("search-preset"),
    s("search-weights"),
    // Forecast/capacity/burndown
    s("forecast-label"),
    s("forecast-sprint"),
    i("forecast-agents"),
    i("agents"),
    s("capacity-label"),
    // Priority filters
    s("robot-by-label"),
    s("robot-by-assignee"),
    f2("robot-min-confidence"),
    i("robot-max-results"),
    // Script emission / feedback
    b("emit-script"),
    i("script-limit"),
    s("script-format"),
    s("feedback-accept"),
    s("feedback-ignore"),
    b("feedback-reset"),
    b("feedback-show"),
    // Exports
    s("export-md"),
    b("no-hooks"),
    s("priority-brief"),
    s("agent-brief"),
    s("export-pages"),
    s("pages-title"),
    b("pages-include-closed"),
    b("pages-include-history"),
    s("preview-pages"),
    b("no-live-reload"),
    b("watch-export"),
    b("pages"),
    // Schema
    s("schema-command"),
    // Debug/render
    s("debug-render"),
    i("debug-width"),
    i("debug-height"),
    // Agents-file mgmt
    b("agents-add"),
    b("agents-remove"),
    b("agents-update"),
    b("agents-check"),
    b("agents-dry-run"),
    b("agents-force"),
];

const fn f2(name: &'static str) -> FlagDef {
    FlagDef {
        name,
        kind: FlagKind::Float,
        primary: false,
        group: None,
    }
}

/// Modifier-requires table (subset of Go's ~50 rules covering all pairs).
pub const MODIFIER_REQUIRES: &[(&str, &[&str])] = &[
    ("robot-diff", &["diff-since"]),
    ("robot-search", &["search"]),
    ("search-limit", &["search"]),
    ("search-mode", &["search"]),
    ("search-preset", &["search"]),
    ("search-weights", &["search"]),
    ("attention-limit", &["robot-label-attention"]),
    ("schema-command", &["robot-schema"]),
    ("suggest-type", &["robot-suggest"]),
    ("suggest-confidence", &["robot-suggest"]),
    ("suggest-bead", &["robot-suggest"]),
    ("graph-format", &["robot-graph"]),
    (
        "graph-root",
        &[
            "robot-graph",
            "robot-triage",
            "robot-triage-by-track",
            "robot-triage-by-label",
            "robot-next",
        ],
    ),
    ("graph-depth", &["robot-graph"]),
    ("severity", &["robot-alerts"]),
    ("alert-type", &["robot-alerts"]),
    ("alert-label", &["robot-alerts"]),
    ("profile-json", &["profile-startup"]),
    ("robot-drift", &["check-drift"]),
    ("history-since", &["robot-history", "bead-history"]),
    ("history-limit", &["robot-history", "bead-history"]),
    (
        "brief",
        &[
            "robot-triage",
            "robot-triage-by-track",
            "robot-triage-by-label",
        ],
    ),
    (
        "robot-history-timeout-ms",
        &[
            "robot-triage",
            "robot-triage-by-track",
            "robot-triage-by-label",
            "robot-next",
        ],
    ),
    ("min-confidence", &["robot-history", "bead-history"]),
    ("orphans-min-score", &["robot-orphans"]),
    ("file-beads-limit", &["robot-file-beads"]),
    ("hotspots-limit", &["robot-file-hotspots"]),
    ("relations-threshold", &["robot-file-relations"]),
    ("relations-limit", &["robot-file-relations"]),
    ("related-min-relevance", &["robot-related"]),
    ("related-max-results", &["robot-related"]),
    ("network-depth", &["robot-impact-network"]),
    ("forecast-label", &["robot-forecast"]),
    ("forecast-sprint", &["robot-forecast"]),
    ("forecast-agents", &["robot-forecast"]),
    ("agents", &["robot-capacity"]),
    ("capacity-label", &["robot-capacity"]),
    ("script-limit", &["emit-script"]),
    ("script-format", &["emit-script"]),
    ("pages-title", &["export-pages"]),
    ("no-live-reload", &["preview-pages"]),
    ("watch-export", &["export-pages"]),
    ("debug-width", &["debug-render"]),
    ("debug-height", &["debug-render"]),
    // Missing rules from Go (main.go:1699-1780)
    (
        "robot-not-ready-labels",
        &[
            "robot-triage",
            "robot-triage-by-track",
            "robot-triage-by-label",
            "robot-next",
        ],
    ),
    (
        "correlation-by",
        &["robot-confirm-correlation", "robot-reject-correlation"],
    ),
    (
        "correlation-reason",
        &["robot-confirm-correlation", "robot-reject-correlation"],
    ),
    ("robot-by-label", &["robot-priority"]),
    ("robot-by-assignee", &["robot-priority"]),
    ("pages-include-closed", &["export-pages"]),
    ("pages-include-history", &["export-pages"]),
    ("graph-preset", &["export-graph"]),
    ("graph-title", &["export-graph"]),
    ("related-include-closed", &["robot-related"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robot_primaries_count_matches_go() {
        assert_eq!(ROBOT_PRIMARIES.len(), 41);
    }

    #[test]
    fn triage_family_is_one_exclusive_group() {
        let group: Vec<_> = ROBOT_PRIMARIES
            .iter()
            .filter(|f| f.group == Some("triage"))
            .collect();
        assert_eq!(group.len(), 4);
    }
}
