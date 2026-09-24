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

// ---------------------------------------------------------------------------
// Root `--help` rendering (Go `printRootHelp`, main.go:1380-1427)
// ---------------------------------------------------------------------------

/// Program name printed in the `--help` banner.
///
/// Byte-parity with Go bv v0.25.0 means printing `bv`, not `bvr`: the help
/// text is a frozen compatibility surface, the flag usage strings it embeds
/// already say "bv" (e.g. "Update bv to the latest version"), and the
/// differential gate diffs this output against the oracle byte-for-byte.
/// Change this one constant to print the Rust name instead.
pub const HELP_PROGRAM: &str = "bv";

/// Column budget Go passes to `pflag`'s `FlagUsagesWrapped` (main.go:1425).
const HELP_COLS: usize = 100;

/// Section headers, in the order Go emits them. `rootHelpSections`
/// (main.go:59-206) declares the first six; pflag's leftover sweep produces
/// the seventh (main.go:1392).
pub const HELP_SECTIONS: &[&str] = &[
    "General Flags",
    "Search & Filters",
    "Robot & Planning Flags",
    "History & Drift",
    "Export & Reporting",
    "Agent File Management",
    "Other Flags",
];

/// One row of the two-column `--help` listing.
///
/// A side table rather than new `FlagDef` fields: the help order is Go's
/// *registration* order (main.go:1461-1663), which matches neither
/// `ROBOT_PRIMARIES` nor `MODIFIER_FLAGS`, and `FlagDef` is const-evaluable
/// and consumed as such. Look a registry entry up with [`FlagDef::help`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpFlag {
    /// Long name without leading dashes.
    pub name: &'static str,
    /// Single-letter alias, when Go registered one (`flag.StringP`).
    pub short: Option<char>,
    /// A `HELP_SECTIONS` title.
    pub section: &'static str,
    /// pflag's `Value.Type()` as displayed after the flag name; empty for
    /// bool flags, which pflag prints with no type word.
    pub type_word: &'static str,
    /// Go's usage string plus the `(default …)` suffix pflag appends for a
    /// non-zero default (flag.go:755-761). Precomputed here so the renderer
    /// only has to lay out text.
    pub help: &'static str,
}

/// Every flag Go advertises, in registration order — the order `--help`
/// prints them in, because `flag.CommandLine.SortFlags` is `false`
/// (main.go:1457) and the per-section FlagSets inherit that order.
pub const HELP_FLAGS: &[HelpFlag] = &[
    HelpFlag {
        name: "cpu-profile",
        short: None,
        section: "General Flags",
        type_word: "string",
        help: "Write CPU profile to file",
    },
    HelpFlag {
        name: "generate-docs",
        short: None,
        section: "Other Flags",
        type_word: "",
        help: "Generate documentation markdown and JSON artifacts",
    },
    HelpFlag {
        name: "db",
        short: None,
        section: "General Flags",
        type_word: "string",
        help: "Path to beads database file or .beads directory (overrides BEADS_DB and BEADS_DIR env vars)",
    },
    HelpFlag {
        name: "version",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Show version",
    },
    HelpFlag {
        name: "update",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Update bv to the latest version",
    },
    HelpFlag {
        name: "check-update",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Check if a new version is available",
    },
    HelpFlag {
        name: "rollback",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Rollback to the previous version (from backup)",
    },
    HelpFlag {
        name: "update-dry-run",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Show what an update would do without installing (use via 'bv upgrade --dry-run')",
    },
    HelpFlag {
        name: "yes",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Skip confirmation prompts (use with --update)",
    },
    HelpFlag {
        name: "export-md",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Export issues to a Markdown file (e.g., report.md)",
    },
    HelpFlag {
        name: "export",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Export a report using recipe defaults or explicit export options",
    },
    HelpFlag {
        name: "export-format",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Report format: markdown, json, csv or mermaid",
    },
    HelpFlag {
        name: "export-include-graph",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Include dependency context in the report (explicit false overrides recipe) (default true)",
    },
    HelpFlag {
        name: "export-template",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Markdown template path; explicit empty disables a recipe template",
    },
    HelpFlag {
        name: "robot-help",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Show AI agent help",
    },
    HelpFlag {
        name: "robot-capabilities",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output machine-readable command capabilities for AI agents",
    },
    HelpFlag {
        name: "robot-docs",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Machine-readable JSON docs for AI agents. Topics: guide, commands, examples, env, exit-codes, all",
    },
    HelpFlag {
        name: "format",
        short: Some('f'),
        section: "General Flags",
        type_word: "string",
        help: "Structured output format for --robot-* commands: json or toon (env: BV_OUTPUT_FORMAT, TOON_DEFAULT_FORMAT)",
    },
    HelpFlag {
        name: "stats",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Show JSON vs TOON token estimates on stderr (env: TOON_STATS=1)",
    },
    HelpFlag {
        name: "robot-insights",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output graph analysis and insights as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-plan",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output dependency-respecting execution plan as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-priority",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output priority recommendations as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-triage",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output unified triage as JSON (the mega-command for AI agents)",
    },
    HelpFlag {
        name: "robot-triage-by-track",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Group triage recommendations by execution track (bv-87)",
    },
    HelpFlag {
        name: "robot-triage-by-label",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Group triage recommendations by label (bv-87)",
    },
    HelpFlag {
        name: "robot-next",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output only the top pick recommendation as JSON (minimal triage)",
    },
    HelpFlag {
        name: "brief",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Compact --robot-triage output: only decision-relevant fields (id, title, status, assignee, blockers, unblocks) (#183)",
    },
    HelpFlag {
        name: "robot-not-ready-labels",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Comma-separated labels marking a bead not-ready: excluded from claimable --robot-next/--robot-triage top picks (env: BV_ROBOT_NOT_READY_LABELS; #173)",
    },
    HelpFlag {
        name: "robot-diff",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output diff as JSON (use with --diff-since)",
    },
    HelpFlag {
        name: "robot-recipes",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output available recipes as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-label-health",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output label health metrics as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-label-flow",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output cross-label dependency flow as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-label-attention",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output attention-ranked labels as JSON for AI agents",
    },
    HelpFlag {
        name: "attention-limit",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Limit number of labels in --robot-label-attention output (default 5)",
    },
    HelpFlag {
        name: "robot-alerts",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output alerts (drift + proactive) as JSON for AI agents",
    },
    HelpFlag {
        name: "robot-metrics",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output performance metrics (timing, cache, memory) as JSON",
    },
    HelpFlag {
        name: "robot-schema",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output JSON Schema definitions for all robot commands",
    },
    HelpFlag {
        name: "schema-command",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output schema for specific command only (e.g., robot-triage)",
    },
    HelpFlag {
        name: "robot-suggest",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output smart suggestions (duplicates, dependencies, labels, cycles) as JSON",
    },
    HelpFlag {
        name: "suggest-type",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Filter suggestions by type: duplicate, dependency, label, cycle",
    },
    HelpFlag {
        name: "suggest-confidence",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "float",
        help: "Minimum confidence for suggestions (0.0-1.0)",
    },
    HelpFlag {
        name: "suggest-bead",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Filter suggestions for specific bead ID",
    },
    HelpFlag {
        name: "robot-graph",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output dependency graph as JSON/DOT/Mermaid for AI agents",
    },
    HelpFlag {
        name: "graph-format",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Graph output format: json, dot, mermaid (default \"json\")",
    },
    HelpFlag {
        name: "graph-root",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Subgraph from specific root issue ID",
    },
    HelpFlag {
        name: "graph-depth",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Max depth for subgraph (0 = unlimited)",
    },
    HelpFlag {
        name: "export-graph",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Export graph: .html for interactive, .png/.svg for static (auto-names if empty)",
    },
    HelpFlag {
        name: "graph-preset",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Graph layout preset: compact (default) or roomy (default \"compact\")",
    },
    HelpFlag {
        name: "graph-title",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Title for graph export (default: project name)",
    },
    HelpFlag {
        name: "robot-min-confidence",
        short: None,
        section: "Search & Filters",
        type_word: "float",
        help: "Filter robot outputs by minimum confidence (0.0-1.0)",
    },
    HelpFlag {
        name: "robot-max-results",
        short: None,
        section: "Search & Filters",
        type_word: "int",
        help: "Limit robot output count (0 = use defaults)",
    },
    HelpFlag {
        name: "robot-by-label",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Filter robot outputs by label (exact match)",
    },
    HelpFlag {
        name: "robot-by-assignee",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Filter robot outputs by assignee (exact match)",
    },
    HelpFlag {
        name: "label",
        short: Some('l'),
        section: "Search & Filters",
        type_word: "string",
        help: "Scope analysis to label's subgraph (applies to every --robot-* command that loads issues, e.g. --robot-insights, --robot-plan, --robot-priority, --robot-orphans)",
    },
    HelpFlag {
        name: "severity",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Filter robot alerts by severity (info|warning|critical)",
    },
    HelpFlag {
        name: "alert-type",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Filter robot alerts by alert type (e.g., stale_issue)",
    },
    HelpFlag {
        name: "alert-label",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Filter robot alerts by label match",
    },
    HelpFlag {
        name: "recipe",
        short: Some('r'),
        section: "Search & Filters",
        type_word: "string",
        help: "Apply a recipe by name (e.g., triage, actionable, high-impact) or by .yaml/.yml file path (e.g., .beads/recipes/sprint.yaml)",
    },
    HelpFlag {
        name: "search",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Hashed keyword search query (builds/updates index on first run)",
    },
    HelpFlag {
        name: "robot-search",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output keyword or hybrid search results as JSON for AI agents (use with --search)",
    },
    HelpFlag {
        name: "search-limit",
        short: None,
        section: "Search & Filters",
        type_word: "int",
        help: "Max results for --search/--robot-search (default 10)",
    },
    HelpFlag {
        name: "search-min-score",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Minimum text similarity before hybrid ranking (-1..1); exact IDs also obey this threshold",
    },
    HelpFlag {
        name: "search-mode",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Search ranking mode: text or hybrid (default: BV_SEARCH_MODE or text)",
    },
    HelpFlag {
        name: "search-preset",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Hybrid preset name (default: BV_SEARCH_PRESET or default)",
    },
    HelpFlag {
        name: "search-weights",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Hybrid weights JSON (overrides preset; keys: text,pagerank,status,impact,priority,recency)",
    },
    HelpFlag {
        name: "diff-since",
        short: None,
        section: "History & Drift",
        type_word: "string",
        help: "Show changes since historical point (commit SHA, branch, tag, or date)",
    },
    HelpFlag {
        name: "as-of",
        short: None,
        section: "History & Drift",
        type_word: "string",
        help: "View state at point in time (commit SHA, branch, tag, or date)",
    },
    HelpFlag {
        name: "no-cache",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Bypass disk cache for robot triage (also: BV_NO_CACHE=1)",
    },
    HelpFlag {
        name: "force-full-analysis",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Compute all metrics regardless of graph size (may be slow for large graphs)",
    },
    HelpFlag {
        name: "profile-startup",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Output detailed startup timing profile for diagnostics",
    },
    HelpFlag {
        name: "profile-json",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Output profile in JSON format (use with --profile-startup)",
    },
    HelpFlag {
        name: "no-hooks",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Skip running hooks during export",
    },
    HelpFlag {
        name: "workspace",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Load issues from workspace config file (.bv/workspace.yaml)",
    },
    HelpFlag {
        name: "repo",
        short: None,
        section: "Search & Filters",
        type_word: "string",
        help: "Filter issues by repository prefix (e.g., 'api-' or 'api')",
    },
    HelpFlag {
        name: "save-baseline",
        short: None,
        section: "History & Drift",
        type_word: "string",
        help: "Save current metrics as baseline with optional description",
    },
    HelpFlag {
        name: "baseline-info",
        short: None,
        section: "History & Drift",
        type_word: "",
        help: "Show information about the current baseline",
    },
    HelpFlag {
        name: "check-drift",
        short: None,
        section: "History & Drift",
        type_word: "",
        help: "Check for drift from baseline (exit codes: 0=OK, 1=critical, 2=warning)",
    },
    HelpFlag {
        name: "robot-drift",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output drift check as JSON (use with --check-drift)",
    },
    HelpFlag {
        name: "robot-history",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output bead-to-commit correlations as JSON",
    },
    HelpFlag {
        name: "bead-history",
        short: None,
        section: "History & Drift",
        type_word: "string",
        help: "Show history for specific bead ID",
    },
    HelpFlag {
        name: "history-since",
        short: None,
        section: "History & Drift",
        type_word: "string",
        help: "Limit history to commits after this date/ref (e.g., '30 days ago', '2024-01-01')",
    },
    HelpFlag {
        name: "history-limit",
        short: None,
        section: "History & Drift",
        type_word: "int",
        help: "Max commits to analyze (0 = unlimited) (default 500)",
    },
    HelpFlag {
        name: "robot-history-timeout-ms",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Budget in ms for the git-history prologue of robot triage (0 = unbounded; default 10000, env BV_ROBOT_HISTORY_TIMEOUT_MS) (default -1)",
    },
    HelpFlag {
        name: "min-confidence",
        short: None,
        section: "History & Drift",
        type_word: "float",
        help: "Filter correlations by minimum confidence (0.0-1.0)",
    },
    HelpFlag {
        name: "id-pattern",
        short: None,
        section: "Other Flags",
        type_word: "stringArray",
        help: "Custom bead ID regex for commit-message matching, e.g. 'bh-[a-z0-9]{5}' (repeatable; capture group 1 is the ID, else the whole match) (#188)",
    },
    HelpFlag {
        name: "robot-explain-correlation",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Explain why a commit is linked to a bead (format: SHA:beadID)",
    },
    HelpFlag {
        name: "robot-confirm-correlation",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Confirm a correlation is correct (format: SHA:beadID)",
    },
    HelpFlag {
        name: "robot-reject-correlation",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Reject an incorrect correlation (format: SHA:beadID)",
    },
    HelpFlag {
        name: "correlation-by",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Agent/user identifier for correlation feedback",
    },
    HelpFlag {
        name: "correlation-reason",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Reason for correlation feedback",
    },
    HelpFlag {
        name: "robot-correlation-stats",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output correlation feedback statistics as JSON",
    },
    HelpFlag {
        name: "robot-orphans",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output orphan commit candidates (commits that should be linked but aren't) as JSON",
    },
    HelpFlag {
        name: "orphans-min-score",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Minimum suspicion score for orphan candidates (0-100) (default 30)",
    },
    HelpFlag {
        name: "robot-file-beads",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output beads that touched a file path as JSON",
    },
    HelpFlag {
        name: "file-beads-limit",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Max closed beads to show (use with --robot-file-beads) (default 20)",
    },
    HelpFlag {
        name: "robot-file-hotspots",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output files touched by most beads as JSON",
    },
    HelpFlag {
        name: "hotspots-limit",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Max hotspots to show (use with --robot-file-hotspots) (default 10)",
    },
    HelpFlag {
        name: "robot-impact",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Analyze impact of modifying files (comma-separated paths)",
    },
    HelpFlag {
        name: "robot-file-relations",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output files that frequently co-change with the given file path",
    },
    HelpFlag {
        name: "relations-threshold",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "float",
        help: "Minimum correlation threshold (0.0-1.0) for related files (default 0.5)",
    },
    HelpFlag {
        name: "relations-limit",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Max related files to show (default 10)",
    },
    HelpFlag {
        name: "robot-related",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output beads related to a specific bead ID as JSON",
    },
    HelpFlag {
        name: "related-min-relevance",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "percent_or_fraction",
        help: "Minimum relevance score for related work (int 0-100 percent OR float 0.0-1.0 fraction) (default 20)",
    },
    HelpFlag {
        name: "related-max-results",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Max results per category for related work (default 10)",
    },
    HelpFlag {
        name: "related-include-closed",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Include closed beads in related work results",
    },
    HelpFlag {
        name: "robot-blocker-chain",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output full blocker chain analysis for issue ID as JSON",
    },
    HelpFlag {
        name: "robot-impact-network",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output bead impact network as JSON (empty for full, or bead ID for subnetwork)",
    },
    HelpFlag {
        name: "network-depth",
        short: None,
        section: "Other Flags",
        type_word: "int",
        help: "Depth of subnetwork when querying specific bead (1-3) (default 2)",
    },
    HelpFlag {
        name: "robot-causality",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output causal chain analysis for bead ID as JSON",
    },
    HelpFlag {
        name: "robot-sprint-list",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output sprints as JSON",
    },
    HelpFlag {
        name: "robot-sprint-show",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output specific sprint details as JSON",
    },
    HelpFlag {
        name: "robot-forecast",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output ETA forecast for bead ID, or 'all' for all open issues",
    },
    HelpFlag {
        name: "forecast-label",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Filter forecast by label",
    },
    HelpFlag {
        name: "forecast-sprint",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Filter forecast by sprint ID",
    },
    HelpFlag {
        name: "forecast-agents",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Number of parallel agents for capacity calculation (default 1)",
    },
    HelpFlag {
        name: "robot-capacity",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "",
        help: "Output capacity simulation and completion projection as JSON",
    },
    HelpFlag {
        name: "agents",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "int",
        help: "Number of parallel agents for capacity simulation (default 1)",
    },
    HelpFlag {
        name: "capacity-label",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Filter capacity simulation by label",
    },
    HelpFlag {
        name: "robot-burndown",
        short: None,
        section: "Robot & Planning Flags",
        type_word: "string",
        help: "Output burndown data for sprint ID, or 'current' for active sprint",
    },
    HelpFlag {
        name: "emit-script",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Emit shell script for top-N recommendations (agent workflows)",
    },
    HelpFlag {
        name: "script-limit",
        short: None,
        section: "Export & Reporting",
        type_word: "int",
        help: "Limit number of items in emitted script (use with --emit-script) (default 5)",
    },
    HelpFlag {
        name: "script-format",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Script format: bash, fish, or zsh (use with --emit-script) (default \"bash\")",
    },
    HelpFlag {
        name: "feedback-accept",
        short: None,
        section: "Other Flags",
        type_word: "string",
        help: "Record accept feedback for issue ID (tunes recommendation weights)",
    },
    HelpFlag {
        name: "feedback-ignore",
        short: None,
        section: "Other Flags",
        type_word: "string",
        help: "Record ignore feedback for issue ID (tunes recommendation weights)",
    },
    HelpFlag {
        name: "feedback-reset",
        short: None,
        section: "Other Flags",
        type_word: "",
        help: "Reset all feedback data to defaults",
    },
    HelpFlag {
        name: "feedback-show",
        short: None,
        section: "Other Flags",
        type_word: "",
        help: "Show current feedback status and weight adjustments",
    },
    HelpFlag {
        name: "priority-brief",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Export priority brief to Markdown file (e.g., brief.md)",
    },
    HelpFlag {
        name: "agent-brief",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Export agent brief bundle to directory (includes triage.json, insights.json, brief.md, helpers.md)",
    },
    HelpFlag {
        name: "export-pages",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Export static site to directory (e.g., ./bv-pages)",
    },
    HelpFlag {
        name: "pages-title",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Custom title for static site",
    },
    HelpFlag {
        name: "pages-include-closed",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Include closed issues in export (default: true) (default true)",
    },
    HelpFlag {
        name: "pages-include-history",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Include git history for time-travel (default: true) (default true)",
    },
    HelpFlag {
        name: "preview-pages",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Preview existing static site bundle",
    },
    HelpFlag {
        name: "no-live-reload",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Disable live-reload in preview mode",
    },
    HelpFlag {
        name: "watch-export",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Watch for beads changes and auto-regenerate export (use with --export-pages)",
    },
    HelpFlag {
        name: "pages",
        short: None,
        section: "Export & Reporting",
        type_word: "",
        help: "Launch interactive Pages deployment wizard",
    },
    HelpFlag {
        name: "debug-render",
        short: None,
        section: "Export & Reporting",
        type_word: "string",
        help: "Render a view and output to file (views: insights, board)",
    },
    HelpFlag {
        name: "debug-width",
        short: None,
        section: "Export & Reporting",
        type_word: "int",
        help: "Width for debug render (default 180)",
    },
    HelpFlag {
        name: "debug-height",
        short: None,
        section: "Export & Reporting",
        type_word: "int",
        help: "Height for debug render (default 50)",
    },
    HelpFlag {
        name: "theme",
        short: None,
        section: "General Flags",
        type_word: "string",
        help: "Color theme: light, dark, or auto (default: detect terminal background)",
    },
    HelpFlag {
        name: "background-mode",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Enable experimental background snapshot loading (TUI only)",
    },
    HelpFlag {
        name: "no-background-mode",
        short: None,
        section: "General Flags",
        type_word: "",
        help: "Disable experimental background snapshot loading (TUI only)",
    },
    HelpFlag {
        name: "agents-add",
        short: None,
        section: "Agent File Management",
        type_word: "",
        help: "Add beads workflow instructions to AGENTS.md (creates file if needed)",
    },
    HelpFlag {
        name: "agents-remove",
        short: None,
        section: "Agent File Management",
        type_word: "",
        help: "Remove beads workflow instructions from AGENTS.md",
    },
    HelpFlag {
        name: "agents-update",
        short: None,
        section: "Agent File Management",
        type_word: "",
        help: "Update beads workflow instructions to latest version",
    },
    HelpFlag {
        name: "agents-check",
        short: None,
        section: "Agent File Management",
        type_word: "",
        help: "Check AGENTS.md blurb status (default if no --agents-* action)",
    },
    HelpFlag {
        name: "agents-dry-run",
        short: None,
        section: "Agent File Management",
        type_word: "",
        help: "Show what would happen without executing (use with --agents-*)",
    },
    HelpFlag {
        name: "agents-force",
        short: None,
        section: "Agent File Management",
        type_word: "",
        help: "Skip confirmation prompts (use with --agents-*)",
    },
    HelpFlag {
        name: "help",
        short: Some('h'),
        section: "General Flags",
        type_word: "",
        help: "help for bv",
    },
];

impl FlagDef {
    /// The `--help` row for this flag, if Go advertises it.
    pub fn help_row(&self) -> Option<&'static HelpFlag> {
        HELP_FLAGS.iter().find(|h| h.name == self.name)
    }

    /// Single-letter alias, or `None` when the flag has no short form.
    pub fn short(&self) -> Option<char> {
        self.help_row().and_then(|h| h.short)
    }

    /// `--help` section header this flag is listed under.
    pub fn section(&self) -> &'static str {
        self.help_row().map(|h| h.section).unwrap_or("")
    }

    /// Description column text Go prints for this flag.
    pub fn help(&self) -> &'static str {
        self.help_row().map(|h| h.help).unwrap_or("")
    }
}

/// Byte-for-byte port of pflag `wrapN` (vendor/github.com/spf13/pflag/flag.go:639).
/// Byte-indexed like the original, which is why [`HELP_FLAGS`] is ASCII-only
/// (enforced by `help_table_is_ascii`).
fn wrap_n(i: usize, slop: usize, s: &str) -> (&str, &str) {
    if i + slop > s.len() {
        return (s, "");
    }
    let head = &s.as_bytes()[..i];
    let last_of = |needle: u8| {
        head.iter()
            .rposition(|&c| c == needle)
            .map_or(-1i64, |p| p as i64)
    };
    let w = [last_of(b' '), last_of(b'\t'), last_of(b'\n')]
        .into_iter()
        .max()
        .unwrap_or(-1);
    if w <= 0 {
        return (s, "");
    }
    let w = w as usize;
    let nl = last_of(b'\n');
    if nl > 0 && (nl as usize) < w {
        return (&s[..nl as usize], &s[nl as usize + 1..]);
    }
    (&s[..w], &s[w + 1..])
}

/// Byte-for-byte port of pflag `wrap` (flag.go:658). The first line is
/// assumed to be indented by the caller; continuations get `indent` spaces.
fn wrap(indent: usize, cols: usize, s: &str) -> String {
    let pad = " ".repeat(indent);
    if cols == 0 {
        return s.replace('\n', &format!("\n{pad}"));
    }
    let mut indent = indent as i64;
    let mut width = cols as i64 - indent;
    let mut r = String::new();
    if width < 24 {
        indent = 16;
        width = cols as i64 - indent;
        r.push('\n');
        r.push_str(&" ".repeat(indent as usize));
    }
    if width < 24 {
        return s.replace('\n', &r);
    }
    let slop = 5i64;
    width -= slop;
    let (first, mut rest) = wrap_n(width as usize, slop as usize, s);
    r.push_str(&first.replace('\n', &format!("\n{pad}")));
    while !rest.is_empty() {
        let (t, next) = wrap_n(width as usize, slop as usize, rest);
        r.push('\n');
        r.push_str(&pad);
        r.push_str(&t.replace('\n', &format!("\n{pad}")));
        rest = next;
    }
    r
}

/// Every long flag name the registry knows about, robot primaries first.
/// Used by `--generate-docs` to record the accepted surface as an artifact.
pub fn flag_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ROBOT_PRIMARIES.iter().map(|f| f.name).collect();
    names.extend(MODIFIER_FLAGS.iter().map(|f| f.name));
    names
}

/// Port of pflag `FlagUsagesWrapped` (flag.go:707-778) for one section: the
/// widest left column sets the description column for every row in it.
fn flag_usages(rows: &[&HelpFlag]) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(rows.len());
    let mut maxlen = 0usize;
    for f in rows {
        let mut line = match f.short {
            Some(c) => format!("  -{c}, --{}", f.name),
            None => format!("      --{}", f.name),
        };
        if !f.type_word.is_empty() {
            line.push(' ');
            line.push_str(f.type_word);
        }
        // Sentinel pflag swaps for padding once the column width is known.
        line.push('\u{0}');
        maxlen = maxlen.max(line.len());
        line.push_str(f.help);
        lines.push(line);
    }

    let mut buf = String::new();
    for line in &lines {
        let sidx = line.find('\u{0}').expect("sentinel present");
        // `Fprintln(prefix, spacing, wrapped)` inserts a space between each
        // argument, landing the description at maxlen + 2.
        buf.push_str(&line[..sidx]);
        buf.push(' ');
        buf.push_str(&" ".repeat(maxlen - sidx));
        buf.push(' ');
        buf.push_str(&wrap(maxlen + 2, HELP_COLS, &line[sidx + 1..]));
        buf.push('\n');
    }
    buf
}

/// Render the full root `--help` page, byte-identical to Go bv v0.25.0.
pub fn render_help(prog: &str) -> String {
    let mut out = format!("Usage: {prog} [flags]\n\nA TUI viewer for beads issue tracker.\n\n");
    for title in HELP_SECTIONS {
        let rows: Vec<&HelpFlag> = HELP_FLAGS.iter().filter(|f| f.section == *title).collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(title);
        out.push_str(":\n");
        out.push_str(&flag_usages(&rows));
        out.push('\n');
    }
    out.push_str(&format!(
        "Run `{prog} --robot-help` for detailed AI/robot command documentation.\n"
    ));
    out
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
    // Doc generation (Go main.go:1461) — emits markdown + JSON artifacts.
    b("generate-docs"),
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
    // Report export (Go main.go:1471-1474) — recipe defaults or explicit options.
    s("export"),
    s("export-format"),
    b("export-include-graph"),
    s("export-template"),
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
    s("search-min-score"),
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
    // Go main.go:1786-1792 — report-export options ride with a report command.
    ("export-format", &["export", "export-md"]),
    ("export-include-graph", &["export", "export-md"]),
    ("export-template", &["export", "export-md"]),
    ("robot-diff", &["diff-since"]),
    ("robot-search", &["search"]),
    ("search-limit", &["search"]),
    ("search-min-score", &["search"]),
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
    (
        "history-since",
        &["robot-history", "bead-history", "robot-causality"],
    ),
    (
        "history-limit",
        &["robot-history", "bead-history", "robot-causality"],
    ),
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

    /// Independent re-statement of Go's `rootHelpSections` matchers
    /// (main.go:59-206), so `HELP_FLAGS.section` is checked against the Go
    /// source rather than trusted.
    fn section_of(name: &str) -> &'static str {
        const GENERAL: &[&str] = &[
            "help",
            "version",
            "cpu-profile",
            "db",
            "update",
            "check-update",
            "update-dry-run",
            "rollback",
            "yes",
            "format",
            "stats",
            "profile-startup",
            "profile-json",
            "no-cache",
            "force-full-analysis",
            "theme",
            "background-mode",
            "no-background-mode",
        ];
        const SEARCH: &[&str] = &[
            "recipe",
            "search",
            "label",
            "severity",
            "alert-type",
            "alert-label",
            "workspace",
            "repo",
            "robot-min-confidence",
            "robot-max-results",
            "robot-by-label",
            "robot-by-assignee",
        ];
        const ROBOT: &[&str] = &[
            "attention-limit",
            "brief",
            "schema-command",
            "suggest-type",
            "suggest-confidence",
            "suggest-bead",
            "graph-format",
            "graph-root",
            "graph-depth",
            "orphans-min-score",
            "file-beads-limit",
            "hotspots-limit",
            "relations-threshold",
            "relations-limit",
            "related-min-relevance",
            "related-max-results",
            "related-include-closed",
            "forecast-label",
            "forecast-sprint",
            "forecast-agents",
            "agents",
            "capacity-label",
        ];
        const HISTORY: &[&str] = &[
            "diff-since",
            "as-of",
            "save-baseline",
            "baseline-info",
            "check-drift",
            "bead-history",
            "history-since",
            "history-limit",
            "min-confidence",
        ];
        const EXPORT: &[&str] = &[
            "export",
            "export-format",
            "export-include-graph",
            "export-template",
            "export-md",
            "no-hooks",
            "export-graph",
            "graph-preset",
            "graph-title",
            "emit-script",
            "script-limit",
            "script-format",
            "priority-brief",
            "agent-brief",
            "export-pages",
            "pages-title",
            "pages-include-closed",
            "pages-include-history",
            "preview-pages",
            "no-live-reload",
            "watch-export",
            "pages",
            "debug-render",
            "debug-width",
            "debug-height",
        ];
        if GENERAL.contains(&name) {
            "General Flags"
        } else if SEARCH.contains(&name) || name.starts_with("search-") {
            "Search & Filters"
        } else if name.starts_with("robot-")
            || ROBOT.contains(&name)
            || name.starts_with("correlation-")
        {
            "Robot & Planning Flags"
        } else if HISTORY.contains(&name) {
            "History & Drift"
        } else if EXPORT.contains(&name) {
            "Export & Reporting"
        } else if name.starts_with("agents-") {
            "Agent File Management"
        } else {
            "Other Flags"
        }
    }

    #[test]
    fn every_help_row_sits_in_its_go_section() {
        for h in HELP_FLAGS {
            assert_eq!(
                section_of(h.name),
                h.section,
                "--{} is filed under {:?}",
                h.name,
                h.section
            );
        }
    }

    /// Every flag the registry accepts must be advertised by `--help`, or a
    /// user has no way to discover it (issue #5).
    #[test]
    fn help_lists_every_registered_flag() {
        let rendered = render_help(HELP_PROGRAM);
        for f in ROBOT_PRIMARIES.iter().chain(MODIFIER_FLAGS) {
            let row = format!("--{}", f.name);
            assert!(
                rendered.contains(&row),
                "flag {row} is registered but missing from `--help`"
            );
            assert_eq!(
                f.help_row().map(|h| h.name),
                Some(f.name),
                "flag --{} has no HELP_FLAGS row",
                f.name
            );
        }
    }

    #[test]
    fn help_shows_every_section_header() {
        let rendered = render_help(HELP_PROGRAM);
        for title in HELP_SECTIONS {
            assert!(
                rendered.contains(&format!("{title}:\n")),
                "section header {title:?} missing from `--help`"
            );
        }
    }

    /// pflag wraps on byte offsets, so the layout is only reproducible while
    /// every help string stays ASCII.
    #[test]
    fn help_table_is_ascii() {
        for h in HELP_FLAGS {
            for field in [h.name, h.section, h.type_word, h.help] {
                assert!(field.is_ascii(), "non-ASCII help field: {field:?}");
            }
        }
    }

    #[test]
    fn help_table_names_are_unique() {
        let mut names: Vec<&str> = HELP_FLAGS.iter().map(|h| h.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate flag name in HELP_FLAGS");
    }

    /// Go registers one short alias per `flag.StringP` call (main.go:1476,
    /// 1521, 1523) plus cobra's `-h`.
    #[test]
    fn help_short_aliases_match_go() {
        let with_short: Vec<(&str, char)> = HELP_FLAGS
            .iter()
            .filter_map(|h| h.short.map(|c| (h.name, c)))
            .collect();
        assert_eq!(
            with_short,
            vec![
                ("format", 'f'),
                ("label", 'l'),
                ("recipe", 'r'),
                ("help", 'h'),
            ]
        );
        for (name, alias) in with_short {
            assert_eq!(
                HELP_FLAGS.iter().find(|h| h.name == name).unwrap().short,
                Some(alias)
            );
        }
    }

    /// Section membership is a property of the name (Go `rootHelpSections`),
    /// so a flag cannot drift out of its group without this firing.
    #[test]
    fn help_sections_match_go_matchers() {
        let general = [
            "help",
            "version",
            "cpu-profile",
            "db",
            "update",
            "check-update",
            "update-dry-run",
            "rollback",
            "yes",
            "format",
            "stats",
            "profile-startup",
            "profile-json",
            "no-cache",
            "force-full-analysis",
            "theme",
            "background-mode",
            "no-background-mode",
        ];
        for name in general {
            assert_eq!(section_of(name), "General Flags", "--{name}");
        }
        for name in [
            "label",
            "recipe",
            "search",
            "search-mode",
            "severity",
            "workspace",
            "repo",
            "robot-min-confidence",
            "robot-by-assignee",
        ] {
            assert_eq!(section_of(name), "Search & Filters", "--{name}");
        }
        for name in [
            "robot-help",
            "robot-triage",
            "robot-capabilities",
            "schema-command",
            "graph-format",
            "correlation-by",
            "brief",
            "agents",
            "forecast-label",
        ] {
            assert_eq!(section_of(name), "Robot & Planning Flags", "--{name}");
        }
        for name in [
            "diff-since",
            "as-of",
            "save-baseline",
            "check-drift",
            "bead-history",
            "history-limit",
            "min-confidence",
        ] {
            assert_eq!(section_of(name), "History & Drift", "--{name}");
        }
        for name in [
            "export",
            "export-md",
            "export-pages",
            "no-hooks",
            "pages",
            "debug-render",
            "watch-export",
            "graph-preset",
        ] {
            assert_eq!(section_of(name), "Export & Reporting", "--{name}");
        }
        for name in ["agents-add", "agents-remove", "agents-force"] {
            assert_eq!(section_of(name), "Agent File Management", "--{name}");
        }
        for name in [
            "generate-docs",
            "id-pattern",
            "network-depth",
            "feedback-show",
        ] {
            assert_eq!(section_of(name), "Other Flags", "--{name}");
        }
    }

    /// Column layout is pflag's: two-space short column, six-space long
    /// column, and every description starting at the section's widest left
    /// column + 2. Spot-check the three shapes — plain, wrapped continuation,
    /// and a `(default …)` suffix — in two different sections so the column
    /// width is visibly derived per section rather than global.
    #[test]
    fn help_layout_matches_pflag() {
        let out = render_help(HELP_PROGRAM);
        let expected = [
            // General Flags' widest left column is `      --force-full-analysis`
            // (27) → descriptions start at column 29 (0-indexed).
            "      --force-full-analysis   Compute all metrics regardless of graph size (may be slow for\n",
            "                              large graphs)\n",
            // short alias: `  -f, --format string` (22) padded to column 29.
            "  -f, --format string         Structured output format for --robot-* commands: json or toon\n",
            "                              (env: BV_OUTPUT_FORMAT, TOON_DEFAULT_FORMAT)\n",
            // Export & Reporting's widest left column is
            // `      --pages-include-closed` (29) → descriptions start at 31.
            "      --graph-preset string      Graph layout preset: compact (default) or roomy (default\n",
            "                                 \"compact\")\n",
            // string defaults are rendered %q-quoted, numeric ones bare.
            "      --relations-threshold float                   Minimum correlation threshold (0.0-1.0)\n",
            "                                                    for related files (default 0.5)\n",
            "      --related-min-relevance percent_or_fraction   Minimum relevance score for related work\n",
            "                                                    (int 0-100 percent OR float 0.0-1.0\n",
            "                                                    fraction) (default 20)\n",
        ];
        for line in expected {
            assert!(out.contains(line), "missing help line: {line:?}");
        }
    }

    /// The banner and footer carry the program name, so a caller can render
    /// the Rust name without touching the flag data.
    #[test]
    fn help_banner_uses_program_name() {
        let out = render_help("bvr");
        assert!(out.starts_with("Usage: bvr [flags]\n\n"));
        assert!(
            out.ends_with("Run `bvr --robot-help` for detailed AI/robot command documentation.\n")
        );
    }

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

    /// Issue #5: the six long flags Go advertises in `bv --help` must be
    /// registered or the CLI rejects a flag upstream accepts.
    #[test]
    fn issue_5_missing_flags_are_registered() {
        let registered: Vec<&str> = MODIFIER_FLAGS.iter().map(|f| f.name).collect();
        for flag in [
            "export",
            "export-format",
            "export-include-graph",
            "export-template",
            "generate-docs",
            "search-min-score",
        ] {
            assert!(
                registered.contains(&flag),
                "flag --{flag} is in Go --help but missing from MODIFIER_FLAGS"
            );
        }
    }

    /// Go main.go:1786-1792 — export options are rejected without a report command.
    #[test]
    fn issue_5_export_options_require_report_command() {
        for modifier in ["export-format", "export-include-graph", "export-template"] {
            let row = MODIFIER_REQUIRES
                .iter()
                .find(|(name, _)| *name == modifier)
                .unwrap_or_else(|| panic!("{modifier} missing from MODIFIER_REQUIRES"));
            assert_eq!(row.1, &["export", "export-md"]);
        }
    }

    #[test]
    fn search_min_score_requires_search() {
        let row = MODIFIER_REQUIRES
            .iter()
            .find(|(name, _)| *name == "search-min-score")
            .expect("search-min-score missing from MODIFIER_REQUIRES");
        assert_eq!(row.1, &["search"]);
    }
}
