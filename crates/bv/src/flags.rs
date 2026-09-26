//! Flag registry — port of Go `cmd/bv` flag definitions (main.go:1417-1663)
//! with category grouping, robot-primary classification, and the validation
//! tables Go drives its three post-parse checks from: the ordered
//! modifier-requires rules (main.go:1786-1843), the modifier-requires recovery
//! examples (main.go:257-316), and the enum-value rules (main.go:1845-1848).

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
/// Whether `name` is registered as a value-taking string flag.
///
/// Go's `isFlagActive` only applies the TrimSpace test to `stringValue` flags
/// (main.go:199-207); every other kind is "active" on presence. The
/// modifier-requires rules need this to tell `--diff-since ""` (present, but
/// not active) from `--diff-since HEAD~5`.
pub fn flag_is_string(name: &str) -> bool {
    let target = name.trim_start_matches('-');
    let is_str = |f: &&FlagDef| {
        f.name == target && matches!(f.kind, FlagKind::Str | FlagKind::RepeatableStr)
    };
    ROBOT_PRIMARIES.iter().find(is_str).is_some() || MODIFIER_FLAGS.iter().find(is_str).is_some()
}

pub fn flag_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ROBOT_PRIMARIES.iter().map(|f| f.name).collect();
    names.extend(MODIFIER_FLAGS.iter().map(|f| f.name));
    names
}

/// Whether `name` (a bare long name, no leading dashes) is a registered flag —
/// the test behind Go's `flags.Lookup(name) == nil` in
/// `rewriteSingleDashLongFlags` (main.go:548).
///
/// The registry is exactly `flag.CommandLine` as `main` builds it before
/// `newRootCommand` merges it into `cmd.Flags()` (main.go:518). It does *not*
/// contain cobra's auto `--help` flag: that is added by `InitDefaultHelpFlag`
/// during `Execute`, after `rewriteSingleDashLongFlags` has already run
/// (main.go:4546), which is why Go answers `bv -help` with pflag's
/// `unknown shorthand flag: 'e' in -elp` rather than printing help.
pub fn flag_lookup(name: &str) -> bool {
    let known = |f: &FlagDef| f.name == name;
    ROBOT_PRIMARIES.iter().any(known) || MODIFIER_FLAGS.iter().any(known)
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

/// Modifier-requires table — Go `modifierRules` (main.go:1786-1843), in Go's
/// order. All 58 rows.
///
/// The ORDER IS SEMANTICS, not presentation. Go's `validateModifierFlags`
/// walks this slice and returns the first rule whose modifier was supplied
/// without a satisfying co-flag, so `bv --graph-preset roomy --severity info`
/// reports `--graph-preset requires --export-graph` (row 1804), not
/// `--severity requires --robot-alerts` (row 1806). Consumers must therefore
/// iterate this slice front-to-back and take the first hit; a sorted or
/// re-derived order makes a two-problem command report the wrong error.
pub const MODIFIER_REQUIRES: &[(&str, &[&str])] = &[
    // main.go:1786-1788 — report-export options ride with a report command.
    ("export-format", &["export", "export-md"]),
    ("export-include-graph", &["export", "export-md"]),
    ("export-template", &["export", "export-md"]),
    // main.go:1789-1795
    ("robot-diff", &["diff-since"]),
    ("robot-search", &["search"]),
    ("search-limit", &["search"]),
    ("search-min-score", &["search"]),
    ("search-mode", &["search"]),
    ("search-preset", &["search"]),
    ("search-weights", &["search"]),
    // main.go:1796-1800
    ("attention-limit", &["robot-label-attention"]),
    ("schema-command", &["robot-schema"]),
    ("suggest-type", &["robot-suggest"]),
    ("suggest-confidence", &["robot-suggest"]),
    ("suggest-bead", &["robot-suggest"]),
    // main.go:1801-1805 — graph output vs. graph *export* are different commands.
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
    ("graph-preset", &["export-graph"]),
    ("graph-title", &["export-graph"]),
    // main.go:1806-1810
    ("severity", &["robot-alerts"]),
    ("alert-type", &["robot-alerts"]),
    ("alert-label", &["robot-alerts"]),
    ("profile-json", &["profile-startup"]),
    ("robot-drift", &["check-drift"]),
    // main.go:1811-1816
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
    (
        "robot-not-ready-labels",
        &[
            "robot-triage",
            "robot-triage-by-track",
            "robot-triage-by-label",
            "robot-next",
        ],
    ),
    ("min-confidence", &["robot-history", "bead-history"]),
    // main.go:1817-1826
    (
        "correlation-by",
        &["robot-confirm-correlation", "robot-reject-correlation"],
    ),
    (
        "correlation-reason",
        &["robot-confirm-correlation", "robot-reject-correlation"],
    ),
    ("orphans-min-score", &["robot-orphans"]),
    ("file-beads-limit", &["robot-file-beads"]),
    ("hotspots-limit", &["robot-file-hotspots"]),
    ("relations-threshold", &["robot-file-relations"]),
    ("relations-limit", &["robot-file-relations"]),
    ("related-min-relevance", &["robot-related"]),
    ("related-max-results", &["robot-related"]),
    ("related-include-closed", &["robot-related"]),
    // main.go:1827-1836
    ("network-depth", &["robot-impact-network"]),
    ("forecast-label", &["robot-forecast"]),
    ("forecast-sprint", &["robot-forecast"]),
    ("forecast-agents", &["robot-forecast"]),
    ("agents", &["robot-capacity"]),
    ("capacity-label", &["robot-capacity"]),
    ("robot-by-label", &["robot-priority"]),
    ("robot-by-assignee", &["robot-priority"]),
    ("script-limit", &["emit-script"]),
    ("script-format", &["emit-script"]),
    // main.go:1837-1843
    ("pages-title", &["export-pages"]),
    ("pages-include-closed", &["export-pages"]),
    ("pages-include-history", &["export-pages"]),
    ("no-live-reload", &["preview-pages"]),
    ("watch-export", &["export-pages"]),
    ("debug-width", &["debug-render"]),
    ("debug-height", &["debug-render"]),
];

// ---------------------------------------------------------------------------
// Modifier-requires recovery examples (Go `modifierRecoveryExamples`,
// main.go:257-316, formatted by `formatModifierRecoveryExamples`,
// main.go:238-255)
// ---------------------------------------------------------------------------

/// Concrete invocations Go appends to a modifier-requires error so the caller
/// can retry without reading the docs. Row order mirrors Go's `switch`, and
/// the eleven modifiers Go leaves undecorated simply have no row here — Go's
/// `default` arm returns `nil`, which formats to the empty string, so an
/// undecorated error must stay byte-identical to the undecorated form.
///
/// Strings are copied verbatim from Go, quotes and all. `search-min-score`
/// rides with the other five search modifiers on Go's 257-263 row.
pub const MODIFIER_RECOVERY_EXAMPLES: &[(&str, &[&str])] = &[
    // main.go:259-264
    (
        "robot-search",
        &[
            r#"bv robot-search "login oauth" --json"#,
            r#"bv --search "login oauth" --robot-search --format json"#,
        ],
    ),
    (
        "search-limit",
        &[
            r#"bv robot-search "login oauth" --json"#,
            r#"bv --search "login oauth" --robot-search --format json"#,
        ],
    ),
    (
        "search-min-score",
        &[
            r#"bv robot-search "login oauth" --json"#,
            r#"bv --search "login oauth" --robot-search --format json"#,
        ],
    ),
    (
        "search-mode",
        &[
            r#"bv robot-search "login oauth" --json"#,
            r#"bv --search "login oauth" --robot-search --format json"#,
        ],
    ),
    (
        "search-preset",
        &[
            r#"bv robot-search "login oauth" --json"#,
            r#"bv --search "login oauth" --robot-search --format json"#,
        ],
    ),
    (
        "search-weights",
        &[
            r#"bv robot-search "login oauth" --json"#,
            r#"bv --search "login oauth" --robot-search --format json"#,
        ],
    ),
    // main.go:265-269
    (
        "robot-diff",
        &[
            "bv robot-diff HEAD~1 --json",
            "bv --robot-diff --diff-since HEAD~1 --format json",
        ],
    ),
    // main.go:270-278
    ("schema-command", &["bv robot-schema triage --json"]),
    ("graph-format", &["bv robot-graph mermaid --json"]),
    ("graph-depth", &["bv robot-graph mermaid --json"]),
    ("graph-root", &["bv robot-graph json --graph-root A --json"]),
    // main.go:279-288
    ("severity", &["bv robot-alerts --severity critical --json"]),
    (
        "alert-type",
        &["bv robot-alerts --severity critical --json"],
    ),
    (
        "alert-label",
        &["bv robot-alerts --severity critical --json"],
    ),
    (
        "robot-drift",
        &["bv --check-drift --robot-drift --format json"],
    ),
    (
        "history-since",
        &[r#"bv robot-history --history-since "30 days ago" --json"#],
    ),
    // main.go:289-297
    (
        "history-limit",
        &[r#"bv robot-history --history-since "30 days ago" --json"#],
    ),
    (
        "min-confidence",
        &[r#"bv robot-history --history-since "30 days ago" --json"#],
    ),
    (
        "robot-history-timeout-ms",
        &["bv robot-triage --robot-history-timeout-ms 10000 --json"],
    ),
    // main.go:298-305
    ("brief", &["bv robot-triage --brief --json"]),
    (
        "correlation-by",
        &["bv robot-confirm-correlation deadbeef:A --correlation-by agent --json"],
    ),
    (
        "correlation-reason",
        &["bv robot-confirm-correlation deadbeef:A --correlation-by agent --json"],
    ),
    // main.go:306-310
    (
        "orphans-min-score",
        &["bv robot-orphans --orphans-min-score 30 --json"],
    ),
    (
        "file-beads-limit",
        &["bv robot-file-beads README.md --file-beads-limit 10 --json"],
    ),
    (
        "hotspots-limit",
        &["bv robot-file-hotspots --hotspots-limit 10 --json"],
    ),
    (
        "relations-threshold",
        &["bv robot-file-relations README.md --relations-limit 10 --json"],
    ),
    // main.go:311-320
    (
        "relations-limit",
        &["bv robot-file-relations README.md --relations-limit 10 --json"],
    ),
    (
        "related-min-relevance",
        &["bv robot-related A --related-max-results 5 --json"],
    ),
    (
        "related-max-results",
        &["bv robot-related A --related-max-results 5 --json"],
    ),
    (
        "related-include-closed",
        &["bv robot-related A --related-max-results 5 --json"],
    ),
    // main.go:321-327
    (
        "network-depth",
        &["bv robot-impact-network A --network-depth 2 --json"],
    ),
    (
        "forecast-label",
        &["bv robot-forecast all --forecast-agents 3 --json"],
    ),
    (
        "forecast-sprint",
        &["bv robot-forecast all --forecast-agents 3 --json"],
    ),
    // main.go:328-338
    (
        "forecast-agents",
        &["bv robot-forecast all --forecast-agents 3 --json"],
    ),
    ("agents", &["bv robot-capacity --agents 3 --json"]),
    ("capacity-label", &["bv robot-capacity --agents 3 --json"]),
    (
        "robot-by-label",
        &["bv robot-priority --robot-by-label backend --json"],
    ),
    // main.go:339-346
    (
        "robot-by-assignee",
        &["bv robot-priority --robot-by-label backend --json"],
    ),
    ("script-limit", &["bv --emit-script --script-limit 5"]),
    ("script-format", &["bv --emit-script --script-limit 5"]),
    // main.go:347-358
    (
        "pages-title",
        &[r#"bv --export-pages ./bv-pages --pages-title "Nightly Build""#],
    ),
    (
        "pages-include-closed",
        &[r#"bv --export-pages ./bv-pages --pages-title "Nightly Build""#],
    ),
    (
        "pages-include-history",
        &[r#"bv --export-pages ./bv-pages --pages-title "Nightly Build""#],
    ),
    (
        "no-live-reload",
        &["bv --preview-pages ./bv-pages --no-live-reload"],
    ),
    (
        "watch-export",
        &["bv --export-pages ./bv-pages --watch-export"],
    ),
    // main.go:359-361
    (
        "debug-width",
        &["bv --debug-render triage --debug-width 120 --debug-height 40"],
    ),
    (
        "debug-height",
        &["bv --debug-render triage --debug-width 120 --debug-height 40"],
    ),
];

/// Go `modifierRecoveryExamples` (main.go:257). A modifier with no row gets
/// Go's `default: return nil`, i.e. an empty list.
pub fn modifier_recovery_examples(modifier: &str) -> &'static [&'static str] {
    MODIFIER_RECOVERY_EXAMPLES
        .iter()
        .find(|(name, _)| *name == modifier)
        .map_or(&[][..], |(_, examples)| *examples)
}

/// Go `formatModifierRecoveryExamples` (main.go:238), the suffix
/// `validateModifierFlags` splices onto `--X requires Y` (main.go:232).
///
/// Three shapes, all returned verbatim (leading `\n` included):
///
/// * unmapped modifier → `""`
/// * exactly one example → `"\nTry: `<invocation>`."`
/// * several → `"\nTry one of:"` then `"\n  `<invocation>`"` per example
pub fn format_modifier_recovery_examples(modifier: &str) -> String {
    let examples = modifier_recovery_examples(modifier);
    match examples {
        [] => String::new(),
        [only] => format!("\nTry: `{only}`."),
        many => {
            let mut out = String::from("\nTry one of:");
            for example in many {
                out.push_str("\n  `");
                out.push_str(example);
                out.push('`');
            }
            out
        }
    }
}

// ---------------------------------------------------------------------------
// Enum flags (Go `validateEnumFlags`, main.go:363-395; rules at 1845-1848)
// ---------------------------------------------------------------------------

/// One `enumFlagRule` (main.go:203). `allowed` order is Go's slice order and
/// is load-bearing: it is both the "expected one of" list and the candidate
/// order `suggestClosest` walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumFlagRule {
    pub name: &'static str,
    pub allowed: &'static [&'static str],
}

/// Go `enumRules` (main.go:1845-1848) — every string flag Go constrains to a
/// closed set, checked after the modifier-requires pass and before the
/// exclusive-primary pass.
pub const ENUM_RULES: &[EnumFlagRule] = &[
    EnumFlagRule {
        name: "graph-format",
        allowed: &["json", "dot", "mermaid"],
    },
    EnumFlagRule {
        name: "script-format",
        allowed: &["bash", "fish", "zsh"],
    },
];

/// A rejected enum value, carrying the parts of Go's error text so a caller
/// can render it without re-deriving anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumFlagError {
    /// The `--`-less flag name (Go's `rule.name`).
    pub name: &'static str,
    /// The value exactly as supplied — Go quotes the raw value, not the
    /// lowercased/trimmed one.
    pub value: String,
    /// Go's `rule.allowed`, in declaration order.
    pub allowed: &'static [&'static str],
    /// Go's `formatDidYouMean` suffix: `"; did you mean \"zsh\"?"` or `""`.
    /// Always `""` for the empty-value error, which Go emits without a
    /// suggestion.
    pub did_you_mean: String,
}

impl EnumFlagError {
    /// Go's `validateEnumFlags` error text, byte-for-byte (main.go:378, 384).
    ///
    /// The two branches are distinct in Go and stay distinct here: a value that
    /// normalizes to empty gets no `did you mean`, because there is no typo to
    /// correct — the flag was simply given no value.
    pub fn message(&self) -> String {
        let expected = self.allowed.join(", ");
        if self.value_is_blank_after_normalizing() {
            format!(
                "invalid --{} {} (expected one of {expected})",
                self.name,
                go_quote(&self.value)
            )
        } else {
            format!(
                "invalid --{} {} (expected one of {expected}){}",
                self.name,
                go_quote(&self.value),
                self.did_you_mean
            )
        }
    }

    /// Go normalizes with `strings.ToLower(strings.TrimSpace(value))` and
    /// branches on the result (main.go:374). The message has to branch the same
    /// way, so re-derive the predicate instead of storing a second flag.
    fn value_is_blank_after_normalizing(&self) -> bool {
        self.value.trim().to_lowercase().is_empty()
    }
}

/// Go `validateEnumFlags` (main.go:363), reduced to its pure decision: the
/// first rule whose flag was supplied with a value outside `allowed` wins,
/// because Go `return`s immediately.
///
/// `supplied` pairs a flag name with its effective value — one entry per
/// changed string flag. Repeated flags are last-wins, matching pflag's
/// `FlagSet.GetString`, so pass the value the parser settled on.
#[allow(dead_code)] // consumed by the validation layer, which this file does not own
pub fn validate_enum_flags(supplied: &[(&str, &str)]) -> Option<EnumFlagError> {
    for rule in ENUM_RULES {
        let Some((_, value)) = supplied.iter().rev().find(|(name, _)| *name == rule.name) else {
            continue;
        };
        let normalized = value.trim().to_lowercase();
        if normalized.is_empty() {
            return Some(EnumFlagError {
                name: rule.name,
                value: (*value).to_string(),
                allowed: rule.allowed,
                did_you_mean: String::new(),
            });
        }
        if !rule.allowed.contains(&normalized.as_str()) {
            return Some(EnumFlagError {
                name: rule.name,
                value: (*value).to_string(),
                allowed: rule.allowed,
                did_you_mean: format_did_you_mean(&normalized, rule.allowed),
            });
        }
    }
    None
}

/// Go `formatDidYouMean` (main.go:409).
#[allow(dead_code)] // exercised through validate_enum_flags' error text
fn format_did_you_mean(value: &str, allowed: &[&'static str]) -> String {
    let suggestion = suggest_closest(value, allowed);
    if suggestion.is_empty() {
        return String::new();
    }
    format!("; did you mean {}?", go_quote(suggestion))
}

/// Go `suggestClosest` (main.go:413). The tie-break is transcribed literally:
/// a candidate at distance exactly `bestDist` only displaces the incumbent
/// when it sorts before it, and the walk keeps the FIRST row on a full tie.
#[allow(dead_code)] // exercised through validate_enum_flags' error text
fn suggest_closest(value: &str, allowed: &[&'static str]) -> &'static str {
    let value = normalize_enum_value(value);
    if value.is_empty() || allowed.is_empty() {
        return "";
    }
    let mut best: Option<&'static str> = None;
    let mut best_dist = max_suggestion_distance(&value);
    for candidate in allowed {
        let normalized = normalize_enum_value(candidate);
        if normalized.is_empty() {
            continue;
        }
        let dist = levenshtein_distance(&value, &normalized);
        // Go mutates `bestDist` from the threshold, so the very first
        // candidate is accepted whenever it lands within it.
        if dist <= best_dist
            && best.is_none_or(|incumbent| {
                dist < best_dist || normalized < normalize_enum_value(incumbent)
            })
        {
            best = Some(candidate);
            best_dist = dist;
        }
    }
    best.unwrap_or("")
}

/// Go's `strings.ToLower(strings.TrimSpace(...))`, the normalization applied
/// before every enum comparison and suggestion.
fn normalize_enum_value(value: &str) -> String {
    value.trim().to_lowercase()
}

/// Go `maxSuggestionDistance` (main.go:451) — a length-tiered ceiling on how
/// far a typo may be from an accepted value. `len` is a BYTE count, matching
/// Go, so a multibyte value gets the wider budget a longer string would.
fn max_suggestion_distance(value: &str) -> usize {
    match value.len() {
        0..=4 => 2,
        5..=10 => 3,
        _ => 4,
    }
}

/// Go `levenshteinDistance` (main.go:422), byte-for-byte. Go indexes
/// `a[i-1]`/`b[j-1]` directly, so the edit distance it computes is over BYTES,
/// not chars — a multibyte rune costs 2-4 edits. Porting over `char_indices`
/// would silently make Rust more forgiving than Go on non-ASCII input.
#[allow(dead_code)] // exercised through validate_enum_flags' error text
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (cur[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Go's `%q` (i.e. `strconv.Quote`) for the value and suggestion in enum error
/// text. Rust's `{:?}` is not a substitute: it spells non-printables as
/// `\u{7f}` where Go writes `\x7f`, and it escapes `'` where Go does not.
///
/// The ASCII half is an exact transcription of `strconv`'s cases. For the rest
/// it reuses `char::escape_debug` — the only std predicate tracking Unicode
/// printability — and reshapes its `\u{…}` spelling into Go's `\uXXXX` /
/// `\UXXXXXXXX`. Flag values are shell text, so the ASCII half is the one that
/// decides the goldens; the non-ASCII half exists so the two never drift into
/// emitting invalid UTF-8.
#[allow(dead_code)] // exercised through validate_enum_flags' error text
fn go_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        let code = ch as u32;
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            // Go's `strconv.Quote` is `QuoteToASCII`-free and never escapes the
            // apostrophe; Rust's `escape_debug` always does, so it is split out
            // here rather than left to fall through.
            '\'' => out.push('\''),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            // Go's `strconv` escape set for non-printables below U+0100,
            // plus DEL. These are exactly the runes with a named short form
            // or an ASCII-range fallback in `strconv.quoteWith`.
            _ if code < 0x20 || code == 0x7f => out.push_str(&format!("\\x{code:02x}")),
            _ => {
                let escaped = ch.escape_debug().to_string();
                if escaped == ch.to_string() {
                    out.push(ch);
                } else if let Some(hex) = escaped
                    .strip_prefix("\\u{")
                    .and_then(|h| h.strip_suffix('}'))
                {
                    // Go pads to 4 hex digits in the BMP and 8 above it.
                    let width = if u32::from_str_radix(hex, 16).is_ok_and(|c| c <= 0xFFFF) {
                        4
                    } else {
                        8
                    };
                    let prefix = if width == 4 { "\\u" } else { "\\U" };
                    out.push_str(prefix);
                    for _ in hex.len()..width {
                        out.push('0');
                    }
                    out.push_str(hex);
                } else {
                    out.push_str(&escaped);
                }
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `flag_lookup` is the whole basis of
    /// `argv::normalize_flag_spelling`: a name the registry does not know is
    /// left alone, which is what keeps a negative value (`-1e400`) from being
    /// rewritten into a flag. A flag missing from `ROBOT_PRIMARIES` /
    /// `MODIFIER_FLAGS` would therefore stop being recognized under its
    /// single-dash spelling, so the registry is pinned against the exact set
    /// Go's `main` builds into `flag.CommandLine` before `newRootCommand`
    /// merges it into `cmd.Flags()` (main.go:1458-1583, merged at :518).
    #[test]
    fn registry_covers_every_go_command_line_flag() {
        const GO_FLAGS: &[&str] = &[
            "agent-brief",
            "agents",
            "agents-add",
            "agents-check",
            "agents-dry-run",
            "agents-force",
            "agents-remove",
            "agents-update",
            "alert-label",
            "alert-type",
            "as-of",
            "attention-limit",
            "background-mode",
            "baseline-info",
            "bead-history",
            "brief",
            "capacity-label",
            "check-drift",
            "check-update",
            "correlation-by",
            "correlation-reason",
            "cpu-profile",
            "db",
            "debug-height",
            "debug-render",
            "debug-width",
            "diff-since",
            "emit-script",
            "export",
            "export-format",
            "export-graph",
            "export-include-graph",
            "export-md",
            "export-pages",
            "export-template",
            "feedback-accept",
            "feedback-ignore",
            "feedback-reset",
            "feedback-show",
            "file-beads-limit",
            "force-full-analysis",
            "forecast-agents",
            "forecast-label",
            "forecast-sprint",
            "format",
            "generate-docs",
            "graph-depth",
            "graph-format",
            "graph-preset",
            "graph-root",
            "graph-title",
            "history-limit",
            "history-since",
            "hotspots-limit",
            "id-pattern",
            "label",
            "min-confidence",
            "network-depth",
            "no-background-mode",
            "no-cache",
            "no-hooks",
            "no-live-reload",
            "orphans-min-score",
            "pages",
            "pages-include-closed",
            "pages-include-history",
            "pages-title",
            "preview-pages",
            "priority-brief",
            "profile-json",
            "profile-startup",
            "recipe",
            "related-include-closed",
            "related-max-results",
            "related-min-relevance",
            "relations-limit",
            "relations-threshold",
            "repo",
            "robot-alerts",
            "robot-blocker-chain",
            "robot-burndown",
            "robot-by-assignee",
            "robot-by-label",
            "robot-capabilities",
            "robot-capacity",
            "robot-causality",
            "robot-confirm-correlation",
            "robot-correlation-stats",
            "robot-diff",
            "robot-docs",
            "robot-drift",
            "robot-explain-correlation",
            "robot-file-beads",
            "robot-file-hotspots",
            "robot-file-relations",
            "robot-forecast",
            "robot-graph",
            "robot-help",
            "robot-history",
            "robot-history-timeout-ms",
            "robot-impact",
            "robot-impact-network",
            "robot-insights",
            "robot-label-attention",
            "robot-label-flow",
            "robot-label-health",
            "robot-max-results",
            "robot-metrics",
            "robot-min-confidence",
            "robot-next",
            "robot-not-ready-labels",
            "robot-orphans",
            "robot-plan",
            "robot-priority",
            "robot-recipes",
            "robot-reject-correlation",
            "robot-related",
            "robot-schema",
            "robot-search",
            "robot-sprint-list",
            "robot-sprint-show",
            "robot-suggest",
            "robot-triage",
            "robot-triage-by-label",
            "robot-triage-by-track",
            "rollback",
            "save-baseline",
            "schema-command",
            "script-format",
            "script-limit",
            "search",
            "search-limit",
            "search-min-score",
            "search-mode",
            "search-preset",
            "search-weights",
            "severity",
            "stats",
            "suggest-bead",
            "suggest-confidence",
            "suggest-type",
            "theme",
            "update",
            "update-dry-run",
            "version",
            "watch-export",
            "workspace",
            "yes",
        ];
        for name in GO_FLAGS {
            assert!(
                flag_lookup(name),
                "{name} is in Go's flag.CommandLine but not the registry"
            );
        }
    }

    /// cobra's auto `--help` is the one name that is deliberately *absent*:
    /// it is added by `InitDefaultHelpFlag` during `Execute`, after
    /// `rewriteSingleDashLongFlags` has already run (main.go:4546).
    #[test]
    fn cobra_help_is_not_in_the_rewrite_registry() {
        assert!(!flag_lookup("help"));
    }

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

    // -----------------------------------------------------------------------
    // A. Evaluation order
    // -----------------------------------------------------------------------

    /// Go's `modifierRules` in source order. Transcribed independently of
    /// [`MODIFIER_REQUIRES`] so the table cannot quietly drift: a reordering
    /// that keeps every rule present still fails here, which is the whole bug
    /// this ordering guards against.
    const GO_MODIFIER_RULE_ORDER: &[&str] = &[
        "export-format",
        "export-include-graph",
        "export-template",
        "robot-diff",
        "robot-search",
        "search-limit",
        "search-min-score",
        "search-mode",
        "search-preset",
        "search-weights",
        "attention-limit",
        "schema-command",
        "suggest-type",
        "suggest-confidence",
        "suggest-bead",
        "graph-format",
        "graph-root",
        "graph-depth",
        "graph-preset",
        "graph-title",
        "severity",
        "alert-type",
        "alert-label",
        "profile-json",
        "robot-drift",
        "history-since",
        "history-limit",
        "brief",
        "robot-history-timeout-ms",
        "robot-not-ready-labels",
        "min-confidence",
        "correlation-by",
        "correlation-reason",
        "orphans-min-score",
        "file-beads-limit",
        "hotspots-limit",
        "relations-threshold",
        "relations-limit",
        "related-min-relevance",
        "related-max-results",
        "related-include-closed",
        "network-depth",
        "forecast-label",
        "forecast-sprint",
        "forecast-agents",
        "agents",
        "capacity-label",
        "robot-by-label",
        "robot-by-assignee",
        "script-limit",
        "script-format",
        "pages-title",
        "pages-include-closed",
        "pages-include-history",
        "no-live-reload",
        "watch-export",
        "debug-width",
        "debug-height",
    ];

    #[test]
    fn modifier_rules_are_in_go_evaluation_order() {
        let actual: Vec<&str> = MODIFIER_REQUIRES.iter().map(|(name, _)| *name).collect();
        assert_eq!(actual, GO_MODIFIER_RULE_ORDER);
        assert_eq!(actual.len(), 58, "Go registers 58 modifier rules");
    }

    /// A two-problem invocation must report the FIRST rule Go hits, and a
    /// set-based or name-sorted implementation cannot distinguish that. These
    /// three pairs are the ones the old table got wrong: `graph-preset` and
    /// `graph-title` sat after the alert/severity rows instead of before them,
    /// and `robot-not-ready-labels` sat after all of them.
    #[test]
    fn first_violation_follows_table_order() {
        // --graph-preset is row 19, --severity is row 21 (Go main.go:1804, 1806).
        let hits: Vec<&str> = MODIFIER_REQUIRES
            .iter()
            .filter(|(modifier, _)| matches!(*modifier, "graph-preset" | "severity"))
            .map(|(modifier, _)| *modifier)
            .collect();
        assert_eq!(hits, ["graph-preset", "severity"]);

        // --robot-history-timeout-ms is row 28, --min-confidence is row 30
        // (Go main.go:1814, 1816) — the old table had them the other way round.
        let hits: Vec<&str> = MODIFIER_REQUIRES
            .iter()
            .filter(|(modifier, _)| {
                matches!(*modifier, "robot-history-timeout-ms" | "min-confidence")
            })
            .map(|(modifier, _)| *modifier)
            .collect();
        assert_eq!(hits, ["robot-history-timeout-ms", "min-confidence"]);

        // --pages-title is row 52, --no-live-reload is row 55
        // (Go main.go:1837, 1840) — the old table ran --no-live-reload first.
        let hits: Vec<&str> = MODIFIER_REQUIRES
            .iter()
            .filter(|(modifier, _)| matches!(*modifier, "pages-title" | "no-live-reload"))
            .map(|(modifier, _)| *modifier)
            .collect();
        assert_eq!(hits, ["pages-title", "no-live-reload"]);
    }

    #[test]
    fn every_modifier_rule_has_a_distinct_name() {
        let mut names: Vec<&str> = MODIFIER_REQUIRES.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            total,
            "duplicate modifier in MODIFIER_REQUIRES"
        );
        for (_, required) in MODIFIER_REQUIRES {
            assert!(
                !required.is_empty(),
                "a rule with no co-flag can never pass"
            );
        }
    }

    // -----------------------------------------------------------------------
    // B. Recovery examples
    // -----------------------------------------------------------------------

    /// Go's `modifierRecoveryExamples` switch arms, as the set of modifiers
    /// that carry a hint. The eleven members of `MODIFIER_REQUIRES` *not*
    /// listed here are the ones Go's `default: return nil` leaves bare, and
    /// they must keep formatting to the empty string.
    const GO_HINTED_MODIFIERS: &[&str] = &[
        "robot-search",
        "search-limit",
        "search-min-score",
        "search-mode",
        "search-preset",
        "search-weights",
        "robot-diff",
        "schema-command",
        "graph-format",
        "graph-depth",
        "graph-root",
        "severity",
        "alert-type",
        "alert-label",
        "robot-drift",
        "history-since",
        "history-limit",
        "min-confidence",
        "robot-history-timeout-ms",
        "brief",
        "correlation-by",
        "correlation-reason",
        "orphans-min-score",
        "file-beads-limit",
        "hotspots-limit",
        "relations-threshold",
        "relations-limit",
        "related-min-relevance",
        "related-max-results",
        "related-include-closed",
        "network-depth",
        "forecast-label",
        "forecast-sprint",
        "forecast-agents",
        "agents",
        "capacity-label",
        "robot-by-label",
        "robot-by-assignee",
        "script-limit",
        "script-format",
        "pages-title",
        "pages-include-closed",
        "pages-include-history",
        "no-live-reload",
        "watch-export",
        "debug-width",
        "debug-height",
    ];

    /// Every rule gets a hint except the eleven Go leaves bare, and the hint
    /// table invents no rules of its own.
    #[test]
    fn recovery_hints_cover_exactly_gos_switch_arms() {
        let covered: Vec<&str> = MODIFIER_RECOVERY_EXAMPLES
            .iter()
            .map(|(name, _)| *name)
            .collect();
        let mut expected: Vec<&str> = GO_HINTED_MODIFIERS.to_vec();
        expected.sort_unstable();
        let mut actual = covered.clone();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        assert_eq!(covered.len(), 47, "Go's switch arms cover 47 modifiers");

        // The complement — Go's `default: return nil` — in rule-table order.
        let unhinted: Vec<&str> = MODIFIER_REQUIRES
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| !covered.contains(name))
            .collect();
        assert_eq!(
            unhinted,
            [
                "export-format",
                "export-include-graph",
                "export-template",
                "attention-limit",
                "suggest-type",
                "suggest-confidence",
                "suggest-bead",
                "graph-preset",
                "graph-title",
                "profile-json",
                "robot-not-ready-labels",
            ],
            "the unhinted set is Go's default arm, nothing more and nothing less"
        );
    }

    #[test]
    fn recovery_hint_text_matches_go() {
        // Go main.go:265-269 — two examples get the "Try one of:" block, and
        // each invocation keeps its own backtick fence.
        assert_eq!(
            format_modifier_recovery_examples("search-min-score"),
            "\nTry one of:\n  `bv robot-search \"login oauth\" --json`\n  `bv --search \"login oauth\" --robot-search --format json`"
        );
        assert_eq!(
            format_modifier_recovery_examples("robot-diff"),
            "\nTry one of:\n  `bv robot-diff HEAD~1 --json`\n  `bv --robot-diff --diff-since HEAD~1 --format json`"
        );
        // Go main.go:270 — a single example gets the short "Try: `x`." form,
        // with a trailing period, and no "one of".
        assert_eq!(
            format_modifier_recovery_examples("schema-command"),
            "\nTry: `bv robot-schema triage --json`."
        );
        assert_eq!(
            format_modifier_recovery_examples("brief"),
            "\nTry: `bv robot-triage --brief --json`."
        );
        // Go main.go:296-297 — escaped quotes in a double-quoted Go literal.
        assert_eq!(
            format_modifier_recovery_examples("history-limit"),
            "\nTry: `bv robot-history --history-since \"30 days ago\" --json`."
        );
        // Go main.go:347-349 — a raw string keeps its double quotes literal.
        assert_eq!(
            format_modifier_recovery_examples("pages-title"),
            "\nTry: `bv --export-pages ./bv-pages --pages-title \"Nightly Build\"`."
        );
    }

    #[test]
    fn unmapped_modifiers_get_no_recovery_hint() {
        // Go's `default: return nil` formats to "", so these errors stay as
        // bare as they were before the hint existed.
        for modifier in [
            "export-format",
            "export-include-graph",
            "export-template",
            "attention-limit",
            "suggest-type",
            "suggest-confidence",
            "suggest-bead",
            "graph-preset",
            "graph-title",
            "profile-json",
            "robot-not-ready-labels",
        ] {
            assert!(
                modifier_recovery_examples(modifier).is_empty(),
                "{modifier} must carry no recovery examples"
            );
            assert_eq!(
                format_modifier_recovery_examples(modifier),
                "",
                "{modifier}"
            );
        }
        // A flag nobody has a rule for is equally undecorated.
        assert_eq!(format_modifier_recovery_examples("not-a-flag"), "");
    }

    #[test]
    fn recovery_example_rows_are_shared_across_each_go_case() {
        // Go returns ONE slice per `case`, so every modifier in a multi-name arm
        // must resolve to the same examples — a per-modifier typo in the table
        // is otherwise invisible.
        for group in [
            &["graph-format", "graph-depth"][..],
            &["severity", "alert-type", "alert-label"][..],
            &["history-since", "history-limit", "min-confidence"][..],
            &["relations-threshold", "relations-limit"][..],
            &[
                "related-min-relevance",
                "related-max-results",
                "related-include-closed",
            ][..],
            &["forecast-label", "forecast-sprint", "forecast-agents"][..],
            &["agents", "capacity-label"][..],
            &["robot-by-label", "robot-by-assignee"][..],
            &["script-limit", "script-format"][..],
            &[
                "pages-title",
                "pages-include-closed",
                "pages-include-history",
            ][..],
            &["debug-width", "debug-height"][..],
        ] {
            let first = modifier_recovery_examples(group[0]);
            for name in &group[1..] {
                assert_eq!(modifier_recovery_examples(name), first, "{name}");
            }
        }
        // All six search modifiers share the 257-263 pair, search-min-score
        // included — it is the row Go's newer flag joined.
        for name in [
            "robot-search",
            "search-limit",
            "search-min-score",
            "search-mode",
            "search-preset",
            "search-weights",
        ] {
            assert_eq!(modifier_recovery_examples(name).len(), 2, "{name}");
        }
    }

    // -----------------------------------------------------------------------
    // C. Enum validation
    // -----------------------------------------------------------------------

    #[test]
    fn enum_rules_match_go() {
        // Go main.go:1845-1848 — graph-format AND script-format, in that order.
        assert_eq!(ENUM_RULES.len(), 2);
        assert_eq!(ENUM_RULES[0].name, "graph-format");
        assert_eq!(ENUM_RULES[0].allowed, &["json", "dot", "mermaid"]);
        assert_eq!(ENUM_RULES[1].name, "script-format");
        assert_eq!(ENUM_RULES[1].allowed, &["bash", "fish", "zsh"]);
    }

    #[test]
    fn enum_accepts_every_allowed_value_case_insensitively() {
        for rule in ENUM_RULES {
            let flag = rule.name;
            for value in rule.allowed {
                assert_eq!(
                    validate_enum_flags(&[(flag, value)]),
                    None,
                    "--{flag} {value} must be accepted"
                );
                // Go normalizes with TrimSpace + ToLower before comparing.
                let shouty = value.to_uppercase();
                assert_eq!(validate_enum_flags(&[(flag, &shouty)]), None, "--{flag}");
                let padded = format!("  {value}\t");
                assert_eq!(validate_enum_flags(&[(flag, &padded)]), None, "--{flag}");
            }
        }
    }

    #[test]
    fn script_format_rejects_an_unknown_shell() {
        // Go's exact stderr for `--emit-script --script-format sh`:
        //   invalid --script-format "sh" (expected one of bash, fish, zsh); did you mean "zsh"?
        let err = validate_enum_flags(&[("script-format", "sh")])
            .expect("sh is not one of bash|fish|zsh");
        assert_eq!(err.name, "script-format");
        assert_eq!(err.value, "sh");
        assert_eq!(
            err.message(),
            "invalid --script-format \"sh\" (expected one of bash, fish, zsh); did you mean \"zsh\"?"
        );
    }

    #[test]
    fn enum_suggestion_follows_gos_distance_ceiling() {
        // "x" is 1 byte so the ceiling is 2, and it is 3-4 edits from every
        // allowed value — Go reports the value with no hint at all.
        let far = validate_enum_flags(&[("script-format", "x")]).expect("x is invalid");
        assert_eq!(far.did_you_mean, "");
        assert_eq!(
            far.message(),
            "invalid --script-format \"x\" (expected one of bash, fish, zsh)"
        );

        // "gh" is also 1 edit short of nothing, but 2 edits from "zsh" and 3
        // from the other two, so the same ceiling admits exactly one candidate.
        let edge = validate_enum_flags(&[("script-format", "gh")]).expect("gh is invalid");
        assert_eq!(
            edge.message(),
            "invalid --script-format \"gh\" (expected one of bash, fish, zsh); did you mean \"zsh\"?"
        );

        // "bashh" is 1 edit from "bash" and inside every ceiling. The two
        // 3-edit candidates never get a look once "bash" tightens `bestDist`.
        let near = validate_enum_flags(&[("script-format", "bashh")]).expect("bashh is invalid");
        assert_eq!(
            near.message(),
            "invalid --script-format \"bashh\" (expected one of bash, fish, zsh); did you mean \"bash\"?"
        );
    }

    /// Go emits a *different* message for a value that normalizes to empty
    /// (main.go:376): no `did you mean`, because nothing was typed. Keeping the
    /// two branches apart is the only way `--script-format ""` reads like Go.
    #[test]
    fn empty_enum_value_gets_no_did_you_mean() {
        for blank in ["", "   ", "\t\n"] {
            let err = validate_enum_flags(&[("script-format", blank)]).expect("blank is invalid");
            assert_eq!(err.did_you_mean, "", "{blank:?}");
            assert_eq!(
                err.message(),
                format!(
                    "invalid --script-format {} (expected one of bash, fish, zsh)",
                    go_quote(blank)
                )
            );
        }
    }

    #[test]
    fn graph_format_is_validated_too() {
        let err = validate_enum_flags(&[("graph-format", "jsonn")]).expect("jsonn is invalid");
        assert_eq!(
            err.message(),
            "invalid --graph-format \"jsonn\" (expected one of json, dot, mermaid); did you mean \"json\"?"
        );
    }

    /// Go `return`s on the first failing rule, so a command bad in both ways
    /// reports `graph-format` (main.go:1846) and never reaches `script-format`.
    #[test]
    fn first_failing_enum_rule_wins() {
        let err = validate_enum_flags(&[("script-format", "sh"), ("graph-format", "svg")])
            .expect("both are invalid");
        assert_eq!(err.name, "graph-format");
        assert_eq!(
            validate_enum_flags(&[("script-format", "sh")]).map(|e| e.name),
            Some("script-format")
        );
    }

    #[test]
    fn unsupplied_enum_flags_are_not_checked() {
        // Go skips a rule unless the flag was `Changed`.
        assert_eq!(validate_enum_flags(&[]), None);
        assert_eq!(validate_enum_flags(&[("severity", "nonsense")]), None);
    }

    #[test]
    fn repeated_enum_flags_use_the_last_value() {
        // pflag's GetString is last-wins, so the parser's settled value decides.
        assert_eq!(
            validate_enum_flags(&[("script-format", "sh"), ("script-format", "zsh")]),
            None
        );
        let err = validate_enum_flags(&[("script-format", "zsh"), ("script-format", "sh")])
            .expect("last value is invalid");
        assert_eq!(err.value, "sh");
    }

    #[test]
    fn levenshtein_is_byte_wise_like_go() {
        assert_eq!(levenshtein_distance("sh", "zsh"), 1);
        assert_eq!(levenshtein_distance("sh", "bash"), 2);
        assert_eq!(levenshtein_distance("sh", "fish"), 2);
        assert_eq!(levenshtein_distance("gh", "bash"), 3);
        assert_eq!(levenshtein_distance("", "fish"), 4);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        // Go indexes `a[i-1]`, so it scores a 2-byte rune as 2 edits, not the
        // 1 a char-wise port would report. Same first byte, so "é"→"è" is 1.
        assert_eq!(levenshtein_distance("e", "é"), 2);
        assert_eq!(levenshtein_distance("é", "è"), 1);
    }

    #[test]
    fn suggestion_ceiling_is_length_tiered_by_bytes() {
        // Go main.go:451-457 — <=4 bytes gives 2, <=10 gives 3, else 4.
        assert_eq!(max_suggestion_distance(""), 2);
        assert_eq!(max_suggestion_distance("sh"), 2);
        assert_eq!(max_suggestion_distance("merm"), 2);
        assert_eq!(max_suggestion_distance("bashh"), 3);
        assert_eq!(max_suggestion_distance("mermai"), 3);
        assert_eq!(max_suggestion_distance("jsonnnn"), 3);
        assert_eq!(max_suggestion_distance("a-very-long-value"), 4);
        // A 2-byte rune pushes a 5-char string out of the top tier.
        assert_eq!(max_suggestion_distance("ééé"), 3);
    }

    #[test]
    fn go_quote_matches_strconv_quote() {
        assert_eq!(go_quote("sh"), "\"sh\"");
        assert_eq!(go_quote(""), "\"\"");
        assert_eq!(go_quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(go_quote("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(go_quote("line\nfeed"), "\"line\\nfeed\"");
        assert_eq!(go_quote("tab\there"), "\"tab\\there\"");
        assert_eq!(
            go_quote("\u{1}\u{7}\u{8}\u{b}\u{c}\u{7f}"),
            "\"\\x01\\a\\b\\v\\f\\x7f\""
        );
        // Go leaves the apostrophe alone inside a string; Rust's Debug would
        // escape it, which is exactly why this is not `{:?}`.
        assert_eq!(go_quote("it's"), "\"it's\"");
        assert_eq!(go_quote("naïve café"), "\"naïve café\"");
    }
}
