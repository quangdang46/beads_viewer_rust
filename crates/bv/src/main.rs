#![allow(clippy::empty_line_after_doc_comments)]
//! bvr — Beads Viewer in Rust.
//! CLI surface skeleton (Phase 3a): flag registry, argv rewriter, validation.
//! Command dispatch lands with bead p3-dispatch-3lv.

mod argv;
#[allow(dead_code)] // flag inventory is declarative data; consumed by dispatch phase
mod flags;
mod validation;

use std::process::ExitCode;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Go `strconv.ParseBool` — the exact set `flag.Bool` accepts.
fn parse_go_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// `--generate-docs` (Go cmd/bv/main.go:1757). Runs first in Go's RunE and
/// exits 0 after emitting the documentation artifacts. The Go tree writes
/// markdown + JSON under `docs/generated`; we emit the JSON artifact plus a
/// markdown index so the flag is a real, terminating command rather than a
/// fall-through to the TUI launcher.
fn run_generate_docs() -> ExitCode {
    let out_dir = std::path::Path::new("docs/generated");
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("Error: generating docs: {e}");
        return ExitCode::from(1);
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let issues = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((i, _)) => i,
        Err(e) => {
            eprintln!("Error: generating docs: {e}");
            return ExitCode::from(1);
        }
    };
    let doc = serde_json::json!({
        "generated_by": "bvr",
        "version": GO_APP_VERSION,
        "contract_version": bv_robot::ROBOT_CONTRACT_VERSION,
        "issue_count": issues.len(),
        "flags": flags::flag_names(),
    });
    let json_path = out_dir.join("bvr-docs.json");
    if let Err(e) = std::fs::write(&json_path, go_json_string(&doc)) {
        eprintln!("Error: generating docs: {e}");
        return ExitCode::from(1);
    }
    let md = format!(
        "# bv generated docs\n\n- version: {}\n- contract: {}\n- issues: {}\n- flags: {}\n",
        GO_APP_VERSION,
        bv_robot::ROBOT_CONTRACT_VERSION,
        issues.len(),
        flags::flag_names().len()
    );
    let md_path = out_dir.join("bvr-docs.md");
    if let Err(e) = std::fs::write(&md_path, md) {
        eprintln!("Error: generating docs: {e}");
        return ExitCode::from(1);
    }
    println!(
        "Generated docs: {} and {}",
        md_path.display(),
        json_path.display()
    );
    ExitCode::from(0)
}

/// `--export` (Go cmd/bv/main.go:4372). Writes a report using recipe defaults
/// with `--export-format` / `--export-include-graph` / `--export-template` as
/// explicit overrides. An empty path means "derive from the active recipe";
/// with no recipe we fall back to the default report name, matching Go's
/// auto-naming.
fn run_export_report(args: &[String], output_path: &str) -> ExitCode {
    let flag_value = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let format = match flag_value("--export-format") {
        Some(value) => value,
        // Go `ResolveReportOptions` seeds Format with "markdown" and only a
        // recipe or an explicit override replaces it.
        None => "markdown".to_string(),
    };
    let template = flag_value("--export-template").unwrap_or_default();
    // Go's `flag.Bool` reads a bare `--export-include-graph` as true and only
    // consumes a following token when that token is the value. `ResolveReportOptions`
    // otherwise derives the default from the format, so markdown keeps its
    // graph and csv never gets one.
    let include_graph = match flag_value("--export-include-graph") {
        None => format != "csv",
        Some(value) if value.starts_with('-') => true,
        Some(value) => match parse_go_bool(&value) {
            Some(parsed) => parsed,
            None => {
                eprintln!("invalid boolean value {value:?} for -export-include-graph: parse error");
                return ExitCode::from(2);
            }
        },
    };
    let options = bv_export::markdown::ReportOptions {
        format: format.clone(),
        template,
        include_graph,
        // Go stamps the report with `robotNow()`, which is UTC.
        generated_at: Some(jiff::Timestamp::now()),
        ..Default::default()
    };
    if let Err(e) = options.validate() {
        eprintln!("Error: {e}");
        return ExitCode::from(2);
    }
    let path = if output_path.is_empty() {
        "report.md".to_string()
    } else {
        output_path.to_string()
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, stats) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let mut options = options;
    attach_report_origins(&mut options, &issues, &cwd, stats);
    let body = match options.format.as_str() {
        "json" => go_json_string(&serde_json::json!(issues)),
        "csv" => {
            let mut out = String::from("id,title,status,priority,issue_type\n");
            for i in &issues {
                out.push_str(&format!(
                    "{},{},{},{},{}\n",
                    i.id,
                    i.title.replace(',', " "),
                    i.status.as_str(),
                    i.priority,
                    i.issue_type
                ));
            }
            out
        }
        "mermaid" => bv_export::mermaid::generate_mermaid(&issues),
        // Go runs `renderReportTemplate` here when `--export-template` names a
        // file (pkg/export/markdown.go:209): a Go `text/template` execution
        // with its own field escaping and 1 MiB read / 16 MiB render caps.
        // That interpreter is not ported yet, so a template path falls back to
        // the default document rather than a half-rendered one.
        _ => bv_export::markdown::generate_report(&issues, &issues, &options),
    };
    match std::fs::write(&path, &body) {
        Ok(_) => {
            println!("Exported {} issues to {}", issues.len(), path);
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Error writing {path}: {e}");
            ExitCode::from(1)
        }
    }
}

/// Go attaches an `IssueOrigin` to every loaded issue (pkg/loader/loader.go:94
/// `AttachIssueOrigins`), and `GenerateMarkdown` renders a Quick Actions block
/// and a per-issue Commands block from it. Reproduce that binding here so the
/// two blocks are not silently empty on a machine that does have a live
/// tracker, and are still empty (Go's `Origin == nil` shape) on one that does
/// not.
fn attach_report_origins(
    options: &mut bv_export::markdown::ReportOptions,
    issues: &[bv_core::model::Issue],
    cwd: &std::path::Path,
    stats: bv_core::loader::ParseStats,
) {
    let source_path = match bv_core::discovery::find_jsonl_path_with_warnings(
        &bv_core::discovery::get_beads_dir(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
        |_| {},
    )
    .ok()
    .flatten()
    {
        Some(path) => path.to_string_lossy().into_owned(),
        None => return,
    };
    // Go marks the whole source read-only when the load was not clean, so no
    // issue in it may present a mutation command.
    let complete = stats.errors == 0;
    for issue in issues {
        let mut origin = bv_core::tracker::resolve_issue_origin(&source_path, &issue.id);
        if !complete && origin.read_only_reason.is_empty() {
            origin.read_only_reason = "source authority is incomplete or stale".to_string();
        }
        options.origins.insert(issue.id.clone(), origin);
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = argv::rewrite_args(&raw);

    // --help/-h short-circuits everything else (Go parity): cobra's help flag
    // is consulted before the root command runs, so `--help` wins over
    // `--version`, over modifier-requires violations, over exclusive-primary
    // violations, and over any `--robot-*` dispatch. Exit code 0 either way.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", flags::render_help(flags::HELP_PROGRAM));
        return ExitCode::from(0);
    }

    // --version handled before validation (Go parity). Go prints
    // "bv <version>" from `pkg/version`, whose fallback at the parity commit
    // is v0.25.0 — the same string the envelope already carries. Printing the
    // Rust crate's own name and version here would make `--version` the one
    // place the binary contradicts its own output.
    if args.iter().any(|a| a == "--version") {
        println!("{} {}", flags::HELP_PROGRAM, GO_APP_VERSION);
        return ExitCode::from(0);
    }

    let presence = validation::Presence::from_args(&args);

    // Validation order mirrors Go: modifier-requires then exclusive primaries.
    let mut violations = validation::validate_modifier_requires(&presence);
    violations.extend(validation::validate_exclusive_primaries(&presence));

    // Enum-valued flags (--graph-format, --script-format) were registered and
    // ported but never checked, so an invalid value was accepted and only
    // failed later — or not at all. Go rejects at validation time with a
    // "did you mean" suggestion.
    let enum_supplied: Vec<(&str, &str)> = flags::ENUM_RULES
        .iter()
        .filter_map(|rule| {
            let value = flag_value(&args, rule.name)?;
            Some((rule.name, value))
        })
        .collect();
    if let Some(err) = flags::validate_enum_flags(&enum_supplied) {
        eprintln!("Error: {}", err.message());
        eprintln!("Usage: bvr --robot-help  (full robot surface arrives with dispatch phase)");
        return ExitCode::from(2);
    }

    if !violations.is_empty() {
        for v in &violations {
            eprintln!("Error: {v}");
        }
        eprintln!("Usage: bvr --robot-help  (full robot surface arrives with dispatch phase)");
        return ExitCode::from(1);
    }

    // Self-update (Go: --check-update / --update-dry-run / --update / --rollback).
    if presence.has("check-update") {
        return run_check_update();
    }
    if presence.has("update-dry-run") {
        return run_update_dry_run();
    }
    if presence.has("update") {
        return run_update(&args);
    }
    if presence.has("rollback") {
        return run_rollback();
    }
    if presence.has("robot-help") {
        print_robot_help();
        return ExitCode::from(0);
    }
    if presence.has("agents-add")
        || presence.has("agents-remove")
        || presence.has("agents-update")
        || presence.has("agents-check")
    {
        return run_agents_commands(&presence, &args);
    }
    if presence.has("robot-capabilities") {
        return run_robot_capabilities();
    }
    if presence.has("robot-schema") {
        return run_robot_schema(&args);
    }
    if presence.has("robot-metrics") {
        return run_robot_metrics();
    }
    if presence.has("robot-docs") {
        return run_robot_docs(&args);
    }

    // --generate-docs runs FIRST in Go's RunE (cmd/bv/main.go:1757) and exits 0
    // after writing the artifacts. Without a handler it falls through to the TUI
    // launcher, which hangs in any TTY and is the footgun AGENTS.md warns about.
    if presence.has("generate-docs") {
        return run_generate_docs();
    }

    // --export (Go main.go:4372) writes a report using recipe defaults, with
    // --export-format / --export-include-graph / --export-template as overrides.
    // Also must not fall through to the TUI.
    if let Some(export_idx) = args.iter().position(|a| a == "--export") {
        let output_path = args.get(export_idx + 1).cloned().unwrap_or_default();
        let md_idx = args.iter().position(|a| a == "--export-md");
        if !output_path.is_empty() {
            if let Some(m) = md_idx {
                let md_path = args.get(m + 1).cloned().unwrap_or_default();
                eprintln!("Error: --export and --export-md specify conflicting output paths");
                let _ = md_path;
                return ExitCode::from(2);
            }
        }
        return run_export_report(&args, &output_path);
    }

    // Export markdown (Phase 5a).
    if let Some(output_path_idx) = args.iter().position(|a| a == "--export-md") {
        let output_path = args
            .get(output_path_idx + 1)
            .cloned()
            .unwrap_or_else(|| "report.md".to_string());
        let cwd = std::env::current_dir().unwrap_or_default();
        let (issues, stats) = match bv_core::discovery::load_issues_from_repo(&cwd) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };
        let mut options = bv_export::markdown::ReportOptions {
            // Go forces `overrides.Format = "markdown"` for `--export-md`,
            // so the graph stays on (only `csv` turns it off).
            format: "markdown".to_string(),
            generated_at: Some(jiff::Timestamp::now()),
            ..Default::default()
        };
        attach_report_origins(&mut options, &issues, &cwd, stats);
        let md = bv_export::markdown::generate_report(&issues, &issues, &options);
        match std::fs::write(&output_path, &md) {
            Ok(_) => {
                println!("Exported {} issues to {}", issues.len(), output_path);
                return ExitCode::from(0);
            }
            Err(e) => {
                eprintln!("Error writing {}: {e}", output_path);
                return ExitCode::from(1);
            }
        }
    }

    // Export graph (Go --export-graph: .html interactive / json|dot|mermaid).
    if let Some(idx) = args.iter().position(|a| a == "--export-graph") {
        let output_path = args.get(idx + 1).cloned().unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_default();
        let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };

        // Format inferred from extension (Go: .html interactive, .dot, else json).
        // Note: --graph-format/--graph-depth are robot-graph-only in Go validation.
        let fmt = if output_path.ends_with(".html") {
            "html".to_string()
        } else if output_path.ends_with(".dot") {
            "dot".to_string()
        } else if output_path.ends_with(".md") {
            "mermaid".to_string()
        } else {
            "json".to_string()
        };
        let issues: Vec<bv_core::model::Issue> = issues;

        let content = match fmt.as_str() {
            "dot" => bv_export::graph_export::generate_dot(&issues, None),
            "mermaid" => bv_export::graph_export::generate_mermaid_graph(&issues),
            "html" => {
                let mermaid = bv_export::graph_export::generate_mermaid_graph(&issues);
                format!(
                    "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Beads Graph</title>\n<script src=\"https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js\"></script>\n<script>mermaid.initialize({{startOnLoad:true}});</script>\n</head>\n<body>\n<h1>Beads Dependency Graph</h1>\n<pre class=\"mermaid\">\n{mermaid}</pre>\n</body>\n</html>\n"
                )
            }
            _ => serde_json::to_string_pretty(&serde_json::json!({
                "format": "json",
                "graph": bv_export::graph_export::generate_adjacency(&issues),
                "nodes": issues.len(),
                "data_hash": hash,
            }))
            .unwrap_or_default(),
        };

        let out = if output_path.is_empty() {
            format!("beads_graph.{fmt}")
        } else {
            output_path
        };
        match std::fs::write(&out, &content) {
            Ok(_) => {
                println!("Exported {} issues to {} ({fmt})", issues.len(), out);
                return ExitCode::from(0);
            }
            Err(e) => {
                eprintln!("Error writing {out}: {e}");
                return ExitCode::from(1);
            }
        }
    }

    // Format validation (Go: exit 2 on invalid)
    if presence.has("format") {
        let fmt_val = args
            .iter()
            .position(|a| a == "--format")
            .and_then(|i| args.get(i + 1))
            .unwrap_or(&"json".to_string())
            .clone();
        if fmt_val != "json" && fmt_val != "toon" {
            eprintln!("Invalid --format \"{fmt_val}\" (expected json|toon)");
            return ExitCode::from(2);
        }
        // Go (cmd/bv/main.go:2037-2038) warns and downgrades toon -> json when
        // the external `tru` encoder is absent, doing it *before* the payload is
        // built so `output_format` never claims "toon" over JSON bytes on
        // stdout.
        if fmt_val == "toon" && !bv_robot::envelope::tru_available() {
            eprintln!("warning: tru not available; falling back to JSON");
            set_output_format("json");
        } else {
            set_output_format(&fmt_val);
        }
    }

    // Robot drift (Go: --robot-drift wraps --check-drift with JSON output).
    if presence.has("robot-drift") {
        return run_robot_drift();
    }
    // Export markdown (Phase 5a).
    if presence.has("check-drift") {
        return run_check_drift();
    }
    if let Some(desc) = args
        .iter()
        .zip(args.iter().skip(1))
        .find(|(a, _)| a.as_str() == "--save-baseline")
        .map(|(_, v)| v.clone())
    {
        return run_save_baseline(&desc);
    }

    // Correlation-family dispatch (Phase 3e).
    if presence.has("robot-history") || presence.has("bead-history") {
        return run_robot_history();
    }
    if presence.has("robot-orphans") {
        return run_robot_orphans();
    }

    // Triage family dispatch (Phase 3c first slice).
    // robot-next has its own flattened output shape (Go handleRobotNext).
    if presence.has("robot-next") {
        return run_robot_next();
    }
    let triage_family = [
        "robot-triage",
        "robot-triage-by-track",
        "robot-triage-by-label",
    ]
    .iter()
    .any(|f| presence.has(f));
    if triage_family {
        return run_robot_triage();
    }

    // Insights / Plan / Priority / Suggest / Alerts / Graph / Recipes / Label trio
    if presence.has("robot-insights") {
        return run_robot_insights();
    }
    if presence.has("robot-plan") {
        return run_robot_plan();
    }
    if presence.has("robot-priority") {
        return run_robot_priority(&args);
    }
    if presence.has("robot-suggest") {
        return run_robot_suggest(&args);
    }
    if presence.has("robot-alerts") {
        return run_robot_alerts();
    }
    if presence.has("robot-graph") {
        return run_robot_graph(&args);
    }
    if presence.has("robot-recipes") {
        return run_robot_recipes();
    }
    if presence.has("robot-label-health") {
        return run_robot_label_health();
    }
    if presence.has("robot-label-flow") {
        return run_robot_label_flow();
    }
    if presence.has("robot-label-attention") {
        return run_robot_label_attention();
    }
    if presence.has("robot-blocker-chain") {
        return run_robot_blocker_chain(&args);
    }
    if presence.has("robot-confirm-correlation") {
        return run_robot_correlation_feedback(&args, "confirm-correlation", "confirm");
    }
    if presence.has("robot-reject-correlation") {
        return run_robot_correlation_feedback(&args, "reject-correlation", "reject");
    }
    if presence.has("robot-explain-correlation") {
        return run_robot_explain_correlation(&args);
    }
    if presence.has("robot-correlation-stats") {
        return run_robot_correlation_stats();
    }
    if presence.has("robot-file-beads") {
        return run_robot_file_beads(&args);
    }
    if presence.has("robot-file-hotspots") {
        return run_robot_file_hotspots();
    }
    if presence.has("robot-file-relations") {
        return run_robot_file_relations(&args);
    }
    if presence.has("robot-search") {
        return run_robot_search(&args);
    }
    // A bare `--search Q` with no `--robot-*` primary. Go prints tab-separated
    // results here and returns (cmd/bv/main.go:3028-3035); reaching this
    // point instead launched the interactive TUI, which an agent caller
    // cannot drive — it just hangs, and blocks CI. Fail fast and scriptable
    // until the text-mode output path is ported.
    if let Some(q) = search_flag(&args, "search").filter(|q| !q.trim().is_empty()) {
        if !flags::ROBOT_PRIMARIES.iter().any(|f| presence.has(f.name)) {
            eprintln!(
                "Error: --search without --robot-search is not supported yet; \
                 use --robot-search for JSON output"
            );
            let _ = q;
            return ExitCode::from(2);
        }
    }
    if presence.has("robot-causality") {
        return run_robot_causality(&args);
    }
    if presence.has("robot-related") {
        return run_robot_related(&args);
    }
    if presence.has("robot-impact-network") {
        return run_robot_impact_network(&args);
    }
    if presence.has("robot-sprint-list") {
        return run_robot_sprint_list();
    }
    if presence.has("robot-sprint-show") {
        return run_robot_sprint_show(&args);
    }
    if presence.has("robot-burndown") {
        return run_robot_burndown(&args);
    }
    if presence.has("robot-forecast") {
        return run_robot_forecast(&args);
    }
    if presence.has("robot-capacity") {
        return run_robot_capacity(&args);
    }

    if presence.has("robot-impact") {
        return run_robot_impact(&args);
    }
    if presence.has("robot-diff") {
        return run_robot_diff(&args);
    }
    if presence.has("robot-not-ready-labels") {
        return run_robot_not_ready_labels(&args);
    }
    // Export pages (static site bundle, Go --export-pages).
    if let Some(idx) = args.iter().position(|a| a == "--export-pages") {
        let out_dir = args
            .get(idx + 1)
            .cloned()
            .unwrap_or_else(|| "./bv-pages".to_string());
        let title = args
            .iter()
            .position(|a| a == "--pages-title")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| "Beads Dashboard".to_string());
        let include_closed = args.iter().any(|a| a == "--pages-include-closed");
        let cwd = std::env::current_dir().unwrap_or_default();
        let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };

        let visible: Vec<&bv_core::model::Issue> = issues
            .iter()
            .filter(|i| include_closed || !i.status.is_closed())
            .collect();

        let open = visible
            .iter()
            .filter(|i| matches!(i.status, bv_core::model::Status::Open))
            .count();
        let in_prog = visible
            .iter()
            .filter(|i| matches!(i.status, bv_core::model::Status::InProgress))
            .count();
        let blocked = visible
            .iter()
            .filter(|i| matches!(i.status, bv_core::model::Status::Blocked))
            .count();
        let closed = issues.iter().filter(|i| i.status.is_closed()).count();

        let mermaid = bv_export::graph_export::generate_mermaid_graph(&issues);
        let rows: String = visible
            .iter()
            .map(|i| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>P{}</td><td>{}</td></tr>\n",
                    i.id,
                    html_escape(&i.title),
                    i.status.as_str(),
                    i.priority,
                    i.issue_type
                )
            })
            .collect();

        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
body {{ font-family: -apple-system, sans-serif; margin: 2rem; background: #282a36; color: #f8f8f2; }}
h1 {{ color: #bd93f9; }}
.stats span {{ margin-right: 1rem; padding: 0.2rem 0.6rem; border-radius: 4px; background: #44475a; }}
table {{ border-collapse: collapse; width: 100%; margin-top: 1rem; }}
td, th {{ border: 1px solid #44475a; padding: 0.4rem 0.6rem; text-align: left; }}
th {{ background: #44475a; }}
.mermaid {{ background: #f8f8f2; padding: 1rem; border-radius: 8px; margin-top: 1rem; }}
</style>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js"></script>
<script>mermaid.initialize({{startOnLoad:true, theme:'dark'}});</script>
</head>
<body>
<h1>{title}</h1>
<p class="stats">
<span>○ Open: {open}</span><span>◐ In-Progress: {in_prog}</span><span>◈ Blocked: {blocked}</span><span>● Closed: {closed}</span>
</p>
<table>
<tr><th>ID</th><th>Title</th><th>Status</th><th>Priority</th><th>Type</th></tr>
{rows}</table>
<div class="mermaid">
{mermaid}</div>
<p><small>data_hash: {hash} | Generated by bvr</small></p>
</body>
</html>
"#
        );

        std::fs::create_dir_all(&out_dir).ok();
        match std::fs::write(format!("{out_dir}/index.html"), html) {
            Ok(_) => {
                println!("Static site exported to {out_dir}");
                return ExitCode::from(0);
            }
            Err(e) => {
                eprintln!("Error writing {out_dir}/index.html: {e}");
                return ExitCode::from(1);
            }
        }
    }

    // Preview pages (Go --preview-pages): export then serve with livereload.
    if let Some(idx) = args.iter().position(|a| a == "--preview-pages") {
        let dir = args
            .get(idx + 1)
            .cloned()
            .unwrap_or_else(|| "./bv-pages".to_string());
        if !std::path::Path::new(&dir).join("index.html").exists() {
            eprintln!("No index.html in {dir} — run --export-pages first");
            return ExitCode::from(1);
        }
        let root = std::path::PathBuf::from(&dir);
        match bv_export::preview::start_preview(
            &root,
            |port| {
                println!("Preview serving at http://127.0.0.1:{port} (Ctrl+C to stop)");
            },
            true,
        ) {
            Ok(()) => {
                std::thread::sleep(std::time::Duration::MAX);
                return ExitCode::from(0);
            }
            Err(e) => {
                eprintln!("Preview failed: {e}");
                return ExitCode::from(1);
            }
        }
    }

    // Any recognized `--robot-*` primary that reached this point is a real
    // command (per the flag registry / robot-help) whose dispatch handler
    // hasn't landed yet. Go never falls through to the TUI for a robot
    // invocation — fail fast with a clear, scriptable error instead of
    // silently starting the interactive TUI (which a robot/agent caller has
    // no way to drive and would just hang or block CI).
    if let Some(unhandled) = flags::ROBOT_PRIMARIES
        .iter()
        .find(|f| f.name != "robot-help" && presence.has(f.name))
    {
        eprintln!(
            "Error: --{} is registered but not yet implemented in bvr",
            unhandled.name
        );
        eprintln!("Usage: bvr --robot-help  (see robot-help for currently-dispatched commands)");
        return ExitCode::from(2);
    }

    // Go's `flag` package rejects any non-flag argument with
    // `unknown command "X" for "bv"` (exit 1). Without this guard `bvr
    // version` fell through to the TUI, which an agent or CI caller cannot
    // drive — it blocks until the process is killed. Reject before the TUI.
    if let Some(unknown) = argv::unconsumed_positional(&args) {
        eprintln!("unknown command \"{unknown}\" for \"bvr\"");
        return ExitCode::from(1);
    }

    // Interactive TUI: no robot flags present.
    let cwd = std::env::current_dir().unwrap_or_default();

    // Workspace mode: .bv/workspace.yaml found → aggregate multi-repo load
    if let Some(ws_path) = bv_core::workspace::find_workspace_config(&cwd) {
        let ws_root = ws_path
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(&cwd)
            .to_path_buf();
        match bv_core::workspace::load_workspace(&ws_path)
            .and_then(|cfg| bv_core::workspace::load_all(&cfg, &ws_root))
        {
            Ok((issues, results)) => {
                let repo_names: Vec<String> = results
                    .iter()
                    .filter(|r| r.error.is_none())
                    .map(|r| r.repo_name.clone())
                    .collect();
                eprintln!(
                    "Workspace: loaded {} issues from {} repos — launching TUI",
                    issues.len(),
                    repo_names.len()
                );
                let mut app = bv_tui::App::new(issues.clone());
                app.workspace_repos = Some(repo_names);
                return launch_tui(&mut app, &issues);
            }
            Err(e) => {
                eprintln!("Workspace load failed: {e} — falling back to single-repo mode");
            }
        }
    }

    match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((issues, _)) => {
            eprintln!("Loaded {} issues — launching TUI", issues.len());
            let mut app = bv_tui::App::new(issues.clone());
            launch_tui(&mut app, &issues)
        }
        Err(e) => {
            eprintln!("Error loading beads: {e}");
            ExitCode::from(1)
        }
    }
}

/// Compute graph metrics and run the TUI event loop.
/// Go `limitMaps` / `limitMapInt` (cmd/bv/robot_registry.go:1884-1931) — keep
/// the top `limit` entries of a metric map, value descending with the key
/// ascending as tie-break, so the result is deterministic.
///
/// Without this every `--robot-insights` full_stats map carries every node
/// (600 on large_cyclic_600, 2500 on xl_2500) where Go carries 200.
fn limit_metric_map(
    m: serde_json::Map<String, serde_json::Value>,
    limit: usize,
) -> serde_json::Map<String, serde_json::Value> {
    if limit == 0 || m.len() <= limit {
        return m;
    }
    let mut items: Vec<(String, serde_json::Value)> = m.into_iter().collect();
    items.sort_by(|a, b| {
        let (ka, va) = (&a.0, &a.1);
        let (kb, vb) = (&b.0, &b.1);
        let na = va.as_f64().unwrap_or(f64::NEG_INFINITY);
        let nb = vb.as_f64().unwrap_or(f64::NEG_INFINITY);
        na.partial_cmp(&nb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .reverse()
            .then_with(|| ka.cmp(kb))
    });
    items.truncate(limit);
    items.into_iter().collect()
}

/// `BV_INSIGHTS_MAP_LIMIT` (Go internal/env/env.go:131). A positive integer
/// overrides the default; anything else keeps it.
fn insights_map_limit() -> usize {
    std::env::var("BV_INSIGHTS_MAP_LIMIT")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(200)
}

fn launch_tui(app: &mut bv_tui::App, issues: &[bv_core::model::Issue]) -> ExitCode {
    let g = bv_analysis::build_graph(issues);
    let pr = bv_graph_core::pagerank_default(&g);
    let bw = bv_graph_core::betweenness(&g);
    let ev = bv_graph_core::eigenvector_default(&g);
    let hits_result = bv_graph_core::hits_default(&g);

    let to_map = |scores: &[f64]| -> std::collections::BTreeMap<String, f64> {
        scores
            .iter()
            .enumerate()
            .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), *v))
            .collect()
    };

    app.graph_metrics = Some(bv_tui::GraphMetrics {
        pagerank: to_map(&pr),
        betweenness: to_map(&bw),
        eigenvector: to_map(&ev),
        hubs: to_map(&hits_result.hubs),
        authorities: to_map(&hits_result.authorities),
    });

    match bv_tui::run_tui(app) {
        Ok(_) => ExitCode::from(0),
        Err(e) => {
            eprintln!("TUI error: {e}");
            ExitCode::from(1)
        }
    }
}

/// Extract --as-of value from process args (Go parity: global flag).
fn extract_as_of() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 1..args.len() {
        if args[i] == "--as-of" {
            return args.get(i + 1).cloned();
        }
        if let Some(val) = args[i].strip_prefix("--as-of=") {
            return Some(val.to_string());
        }
    }
    None
}

/// Load issues from cwd, honoring workspace config if present (multi-repo).
/// Source provenance for the v0.25.0 envelope (`source_path`, `source_kind`).
/// Go derives these in `RobotContext`; we collect them at load time because our
/// discovery chain returns only the issue vector.
#[derive(Debug, Clone, Default)]
struct SourceMeta {
    path: String,
    kind: String,
    valid: usize,
    errors: usize,
    skipped: usize,
}

/// Build the Go `RobotSourceAuthority` for a single-source load
/// (Go `newRobotSourceAuthority` over one `RobotSourceReport`).
fn source_authority(meta: &SourceMeta, data_hash: &str) -> bv_robot::RobotSourceAuthority {
    use bv_robot::{RobotSourceAuthority, RobotSourceReport};
    let report = RobotSourceReport {
        name: String::new(),
        repo_path: String::new(),
        source_path: meta.path.clone(),
        source_kind: meta.kind.clone(),
        status: "loaded".to_string(),
        data_hash: data_hash.to_string(),
        valid: meta.valid,
        errors: meta.errors,
        skipped: meta.skipped,
        read_errors: 0,
        visible: meta.valid,
        tombstones: 0,
        stale: false,
        warning_count: 0,
        warnings: Vec::new(),
        error: String::new(),
    };
    let loaded = usize::from(!meta.path.is_empty());
    RobotSourceAuthority {
        state: "complete".to_string(),
        claim_safe: meta.errors == 0,
        readiness: "proven".to_string(),
        loaded,
        failed: 0,
        disabled: 0,
        valid: meta.valid,
        errors: meta.errors,
        skipped: meta.skipped,
        read_errors: 0,
        visible: meta.valid,
        tombstones: 0,
        warning_count: 0,
        sources: vec![report],
    }
}

fn load_issues_auto(
    cwd: &std::path::Path,
    as_of: Option<&str>,
) -> Result<(Vec<bv_core::model::Issue>, String, Option<String>), String> {
    load_issues_auto_meta(cwd, as_of).map(|(i, h, c, _)| (i, h, c))
}

/// Envelope for a command that already loaded `issues` — derives the source
/// provenance from the current directory so callers do not thread `cwd`.
/// The active `--label` / `--recipe` / `--repo` scoping flags, read straight
/// from argv so the envelope can publish them. Go threads these through the
/// RobotContext (robot_registry.go:261-268); the envelope helper has no
/// context, so it reads the process arguments.
fn active_scope_flags() -> (String, String, String) {
    let args: Vec<String> = std::env::args().collect();
    let value_of = |names: &[&str]| -> String {
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            for n in names {
                if a == n {
                    return args.get(i + 1).cloned().unwrap_or_default();
                }
                if let Some(v) = a.strip_prefix(&format!("{n}=")) {
                    return v.to_string();
                }
            }
            i += 1;
        }
        String::new()
    };
    (
        value_of(&["--label"]),
        value_of(&["--recipe"]),
        value_of(&["--repo"]),
    )
}

/// Apply `--label` scoping the way Go's `scopeLoadedIssues` does
/// (cmd/bv/main.go:4870-4900): the label's own issues plus their direct
/// neighbours replace the issue set the command analyses, so the graph, the
/// counts and the drift all see the scoped subgraph rather than the whole repo.
///
/// Returns the analysis set. The candidate set (the label's own issues) is
/// what the envelope's `scope`/`scope_hash` describe.
fn apply_label_scope(issues: &[bv_core::model::Issue]) -> Vec<bv_core::model::Issue> {
    let (label, _, _) = active_scope_flags();
    if label.is_empty() {
        return issues.to_vec();
    }
    let (_, analysis_ids) = bv_analysis::label_health::label_scope_ids(&label, issues);
    let mut out = Vec::with_capacity(analysis_ids.len());
    for id in &analysis_ids {
        if let Some(issue) = issues.iter().find(|i| &i.id == id) {
            out.push(issue.clone());
        }
    }
    out
}

fn full_envelope_for(data_hash: &str, issues: &[bv_core::model::Issue]) -> serde_json::Value {
    let source = source_meta_for(issues);
    full_envelope_json_with_source(data_hash, Some(&source), issues)
}

/// Derive `SourceMeta` for the current working directory. Used by command
/// handlers that build their envelope without an explicit cwd in scope.
fn source_meta_for(issues: &[bv_core::model::Issue]) -> SourceMeta {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Ok(dir) = bv_core::discovery::get_beads_dir(&cwd) {
        if let Ok(Some(jsonl)) = bv_core::discovery::find_jsonl_path_with_warnings(&dir, |_| {}) {
            return SourceMeta {
                path: jsonl.to_string_lossy().to_string(),
                kind: "jsonl_local".to_string(),
                valid: issues.len(),
                errors: 0,
                skipped: 0,
            };
        }
    }
    SourceMeta {
        valid: issues.len(),
        ..Default::default()
    }
}

/// Go `RobotContext` loader — returns the issues, their hash, the resolved
/// `--as-of` commit, and the source provenance the envelope reports.
fn load_issues_auto_meta(
    cwd: &std::path::Path,
    as_of: Option<&str>,
) -> Result<
    (
        Vec<bv_core::model::Issue>,
        String,
        Option<String>,
        SourceMeta,
    ),
    String,
> {
    // If --as-of is specified, use GitLoader for time-travel (Go parity).
    if let Some(revision) = as_of {
        let loader = bv_core::discovery::GitLoader::new(cwd);
        let resolved = loader
            .resolve_revision(revision)
            .map_err(|e| e.to_string())?;
        let issues = loader.load_at(revision).map_err(|e| e.to_string())?;
        eprintln!(
            "Loaded {} issues from {} ({})",
            issues.len(),
            revision,
            &resolved[..resolved.len().min(7)]
        );
        let hash = bv_core::data_hash::compute_data_hash(&issues);
        let valid = issues.len();
        return Ok((
            issues,
            hash,
            Some(resolved),
            SourceMeta {
                path: format!("@{revision}"),
                kind: "git".to_string(),
                valid,
                errors: 0,
                skipped: 0,
            },
        ));
    }
    if let Some(ws_path) = bv_core::workspace::find_workspace_config(cwd) {
        let ws_root = ws_path
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(cwd)
            .to_path_buf();
        match bv_core::workspace::load_workspace(&ws_path)
            .and_then(|cfg| bv_core::workspace::load_all(&cfg, &ws_root))
        {
            Ok((issues, _)) => {
                let hash = bv_core::data_hash::compute_data_hash(&issues);
                let valid = issues.len();
                return Ok((
                    issues,
                    hash,
                    None,
                    SourceMeta {
                        path: ws_path.to_string_lossy().to_string(),
                        kind: "workspace".to_string(),
                        valid,
                        errors: 0,
                        skipped: 0,
                    },
                ));
            }
            Err(e) => eprintln!("workspace load failed, falling back: {e}"),
        }
    }
    let beads_dir = bv_core::discovery::get_beads_dir(cwd).map_err(|e| e.to_string())?;
    let jsonl = bv_core::discovery::find_jsonl_path_with_warnings(&beads_dir, |_| {})
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no beads JSONL found in {}", beads_dir.display()))?;
    let (issues, stats) = {
        let raw = std::fs::read_to_string(&jsonl)
            .map_err(|e| format!("reading {}: {}", jsonl.display(), e))?;
        let mut rdr = raw.as_bytes();
        bv_core::loader::parse_issues_with_options(
            &mut rdr,
            &bv_core::loader::ParseOptions::default(),
            |_| {},
        )
        .map_err(|e| e.to_string())?
    };
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let valid = issues.len();
    Ok((
        issues,
        hash,
        None,
        SourceMeta {
            path: jsonl.to_string_lossy().to_string(),
            kind: "jsonl_local".to_string(),
            valid,
            errors: stats.errors,
            skipped: stats.skipped,
        },
    ))
}

/// Load issues from discovery chain and emit --robot-triage JSON.
/// Go `handleRobotNext`: single top claimable pick with the claimability
/// filter (open, non-epic, unassigned, no open blockers) and fail-closed
/// degraded output when no pick is claim-safe.
fn run_robot_next() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let as_of = extract_as_of();
    let (issues, hash, as_of_commit, source) = match load_issues_auto_meta(&cwd, as_of.as_deref()) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let mut payload = full_envelope_for(&hash, &issues);
    if let Some(ref a) = as_of {
        payload["as_of"] = serde_json::json!(a);
    }
    if let Some(ref c) = as_of_commit {
        payload["as_of_commit"] = serde_json::json!(c);
    }

    let usage_hints = serde_json::json!([
        "Use scripts/br_retry.sh actionable --json plus the claim gate before mutating Beads state in crowded swarms.",
        "No claim_command is emitted unless the item is open, unblocked, unassigned, and triage metrics are ready.",
        "Inspect .status for skipped, timeout, or pending graph phases.",
    ]);

    if issues.is_empty() {
        payload["actionable"] = serde_json::json!(false);
        payload["phase2_ready"] = serde_json::json!(false);
        payload["status"] = bv_analysis::analyzer::MetricStatus::default().to_json_map();
        payload["message"] = serde_json::json!("No proven actionable item available");
        payload["degraded"] = serde_json::json!([{
            "code": "no_actionable_recommendation",
            "severity": "info",
            "message": "No open, unblocked, unassigned non-epic recommendation passed the robot-next claimability filter.",
            "repair": "Use br ready --json or scripts/br_retry.sh actionable --json for authoritative claim candidates.",
        }]);
        payload["usage_hints"] = usage_hints;
        return emit_json(&payload);
    }

    let g = std::sync::Arc::new(bv_analysis::analyzer::build_graph(&issues));
    let out = bv_analysis::triage::build_triage(&issues, &g, robot_now());

    // Claimability filter (Go robotNextClaimabilityReasons): the pick must
    // be open, non-epic, unassigned, and free of open blockers.
    let issue_by_id: std::collections::HashMap<&str, &bv_core::model::Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();
    let claimability_reasons = |pick: &serde_json::Value| -> Vec<String> {
        let Some(id) = pick["id"].as_str() else {
            return vec!["pick lacks id".into()];
        };
        let Some(issue) = issue_by_id.get(id) else {
            return vec![format!("{id} is absent from loaded Beads records")];
        };
        let mut reasons = Vec::new();
        if issue.status != bv_core::model::Status::Open {
            reasons.push(format!("{id} status is {:?}", issue.status));
        }
        if issue.issue_type.eq_ignore_ascii_case("epic") {
            reasons.push(format!("{id} is an epic"));
        }
        let assignee = issue.assignee.trim();
        if !assignee.is_empty() {
            reasons.push(format!("{id} is already assigned to {assignee}"));
        }
        // Ancestor-epic parity (#2): route through the shared
        // blocker_chain helper so parent-child inherited blocking gates
        // the claimability filter, exactly like compute_blocked_set.
        let mut open_blockers: Vec<String> = bv_analysis::open_blockers(&issue_by_id, &issue.id);
        // Surface dangling direct-blocking edges as unclaimable too.
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let blocker_id = dep.effective_depends_on().trim().to_string();
            if blocker_id.is_empty() {
                open_blockers.push("<missing blocker id>".into());
            } else if !issue_by_id.contains_key(blocker_id.as_str())
                && !open_blockers.iter().any(|b| b == &blocker_id)
            {
                open_blockers.push(format!("{blocker_id} (missing)"));
            }
        }
        if !open_blockers.is_empty() {
            open_blockers.sort();
            reasons.push(format!("{id} is blocked by {}", open_blockers.join(", ")));
        }
        reasons
    };

    // Build top picks exactly as run_robot_triage does (top 5 recommendations
    // with direct unblock counts).
    let picks: Vec<serde_json::Value> = out
        .recommendations
        .iter()
        .map(|r| {
            let unblocks: usize = issues
                .iter()
                .filter(|o| {
                    o.dependencies
                        .iter()
                        .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
                })
                .count();
            serde_json::json!({
                "id": r.id,
                "title": r.title,
                "score": r.score,
                "reasons": r.reasons,
                "unblocks": unblocks,
            })
        })
        .collect();
    let mut chosen: Option<&serde_json::Value> = None;
    let mut first_unsafe: Option<Vec<String>> = None;
    for pick in &picks {
        let reasons = claimability_reasons(pick);
        if reasons.is_empty() {
            chosen = Some(pick);
            break;
        }
        if first_unsafe.is_none() {
            first_unsafe = Some(reasons);
        }
    }

    // Clear skipped reasons for Go omitempty parity (only "approximate" kept).
    let cleared_status = {
        let mut s = out.metric_status.clone();
        if s.page_rank.state == "skipped" {
            s.page_rank.reason.clear();
        }
        if s.eigenvector.state == "skipped" {
            s.eigenvector.reason.clear();
        }
        if s.hits.state == "skipped" {
            s.hits.reason.clear();
        }
        if s.critical.state == "skipped" {
            s.critical.reason.clear();
        }
        if s.cycles.state == "skipped" {
            s.cycles.reason.clear();
        }
        if s.kcore.state == "skipped" {
            s.kcore.reason.clear();
        }
        if s.articulation.state == "skipped" {
            s.articulation.reason.clear();
        }
        if s.slack.state == "skipped" {
            s.slack.reason.clear();
        }
        s.to_json_map()
    };
    match chosen {
        Some(top) => {
            payload["phase2_ready"] = serde_json::json!(true);
            payload["status"] = cleared_status.clone();
            let id = top["id"].as_str().unwrap_or_default().to_string();
            // Go v0.25.0: live tracker route suggestions replace the flat
            // claim_command/show_command strings.
            let origin = bv_core::tracker::resolve_issue_origin(&source.path, &id);
            let actions = bv_core::tracker::build_actions(&origin, true);
            payload["actions"] = serde_json::to_value(&actions).unwrap_or(serde_json::Value::Null);

            // Go (robot_registry.go:2558-2566) treats a missing claim route as a
            // degradation, not as an actionable pick: it reports the candidate
            // under `diagnostic_top_pick`, sets actionable=false, and explains
            // why. Claimability alone is not enough — a source with no readable
            // tracker metadata (every synthetic fixture) must take this path.
            if actions.claim.is_none() {
                payload["actionable"] = serde_json::json!(false);
                payload["message"] = serde_json::json!(format!(
                    "No claim command emitted: {}",
                    actions.unavailable_reason
                ));
                payload["diagnostic_top_pick"] = serde_json::json!({
                    "id": top["id"],
                    "title": top["title"],
                    "score": top["score"],
                    "reasons": top["reasons"],
                    "unblocks": top.get("unblocks").cloned().unwrap_or(serde_json::json!(0)),
                });
                payload["degraded"] = serde_json::json!([{
                    "code": "live_action_route_unavailable",
                    "severity": "info",
                    "message": actions.unavailable_reason,
                }]);
            } else {
                payload["actionable"] = serde_json::json!(true);
                payload["id"] = top["id"].clone();
                payload["title"] = top["title"].clone();
                payload["score"] = top["score"].clone();
                payload["reasons"] = top["reasons"].clone();
                if let Some(cmd) = &actions.claim {
                    payload["claim_command"] = serde_json::json!(cmd.shell);
                }
                if let Some(cmd) = &actions.show {
                    payload["show_command"] = serde_json::json!(cmd.shell);
                }
            }
        }
        None => {
            payload["actionable"] = serde_json::json!(false);
            payload["phase2_ready"] = serde_json::json!(true);
            payload["status"] = cleared_status;
            payload["message"] = serde_json::json!(
                "No claim command emitted because the top recommendation was not claim-safe"
            );
            if let Some(first_pick) = picks.first() {
                payload["diagnostic_top_pick"] = serde_json::json!({
                    "id": first_pick["id"],
                    "title": first_pick["title"],
                    "score": first_pick["score"],
                    "reasons": first_pick["reasons"],
                    "unblocks": first_pick["unblocks"],
                });
            }
            payload["degraded"] = serde_json::json!([{
                "code": "robot_next_claim_unsafe",
                "severity": "warning",
                "message": first_unsafe.unwrap_or_default().join("; "),
                "repair": "Use the authoritative Beads actionable queue plus claim gate before claiming work.",
            }]);
        }
    }
    payload["usage_hints"] = usage_hints;
    emit_json(&payload)
}
/// Go `opts.TopN` (default 10) applied to the scored recommendation list.
/// Go builds recommendations against the full scored set and slices only the
/// user-visible list, so top_picks and quick_wins are unaffected (issue #146).
fn recommendations_top_n(recs: &[bv_analysis::impact::IssueImpact]) -> Vec<serde_json::Value> {
    const TOP_N: usize = 10;
    recs.iter()
        .take(TOP_N)
        .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
        .collect()
}

/// Go's claimability gate for a triage recommendation (triage.go:657).
///
/// Shared by `top_picks` and `quick_wins`: Go derives both from the same
/// `claimableIDs` set, so a deferred/draft/blocked bead must never appear in
/// either (issue #199). Checking only `status == "open"` is not enough.
fn triage_claimable(
    r: &bv_analysis::impact::IssueImpact,
    issue_by_id: &std::collections::HashMap<&str, &bv_core::model::Issue>,
) -> bool {
    if r.status != "open" || r.issue_type == "epic" {
        return false;
    }
    if let Some(issue) = issue_by_id.get(r.id.as_str()) {
        if !issue.assignee.trim().is_empty() {
            return false;
        }
        // Ancestor-epic parity (#2): shared blocker_chain helper.
        if !bv_analysis::open_blockers(issue_by_id, &issue.id).is_empty() {
            return false;
        }
    }
    true
}

fn run_robot_triage() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let as_of = extract_as_of();
    // Use load_issues_auto_meta, not load_issues_auto: the latter discards the
    // SourceMeta, and re-deriving it via source_meta_for() is what hard-codes
    // errors/skipped to zero — making claim_safe unconditionally true.
    let (loaded, _hash, as_of_commit, loaded_source) =
        match load_issues_auto_meta(&cwd, as_of.as_deref()) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };
    // --label narrows the analysis to the label's subgraph (Go
    // scopeLoadedIssues, main.go:4870-4900), while the envelope keeps
    // describing the loaded file.
    let loaded_hash = _hash.clone();
    let issues = apply_label_scope(&loaded);
    if issues.is_empty() {
        println!(
            "{{\"generated_at\":\"{}\",\"data_hash\":\"empty\",\"triage\":{{}}}}",
            jiff_now()
        );
        return ExitCode::from(0);
    }
    // Go keeps the loader's data_hash: scopeLoadedIssues sets
    // DataHashMatchesIssues=false so the payload still names the file it came
    // from (main.go:4890-4900).
    let data_hash = loaded_hash;
    let g = std::sync::Arc::new(bv_analysis::analyzer::build_graph(&issues));
    let mut out = bv_analysis::triage::build_triage(&issues, &g, robot_now());
    // Go stamps every recommendation with `issue.Actions(claimable)`
    // (triage.go:658). The tracker route needs the loaded source path, which
    // only the CLI layer has, so it is resolved here rather than in the
    // analysis layer.
    // Describe the loaded file, not the scoped analysis set.
    let source = loaded_source;
    for rec in out.recommendations.iter_mut() {
        let origin = bv_core::tracker::resolve_issue_origin(&source.path, &rec.id);
        let actions = bv_core::tracker::build_actions(&origin, rec.claimable);
        rec.actions = Some(serde_json::to_value(&actions).unwrap_or(serde_json::Value::Null));
    }

    // Build top_picks: Go `buildTopPicks` — only claimable recommendations
    // (open, not epic, unassigned, no open blockers, not a parent with open
    // children). Use original issue data for assignee/blocker checks.
    let issue_by_id: std::collections::HashMap<&str, &bv_core::model::Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();
    let top_picks: Vec<serde_json::Value> = out
        .recommendations
        .iter()
        .filter(|r| {
            if r.status != "open" || r.issue_type == "epic" {
                return false;
            }
            if let Some(issue) = issue_by_id.get(r.id.as_str()) {
                if !issue.assignee.trim().is_empty() {
                    return false;
                }
                // Ancestor-epic parity (#2): shared blocker_chain helper.
                if !bv_analysis::open_blockers(&issue_by_id, &issue.id).is_empty() {
                    return false;
                }
            }
            true
        })
        .take(3)
        .map(|r| {
            let unblocks: usize = issues
                .iter()
                .filter(|o| {
                    o.dependencies
                        .iter()
                        .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
                })
                .count();
            serde_json::json!({
                "id": r.id,
                "title": r.title,
                "score": r.score,
                "reasons": r.reasons,
                "unblocks": unblocks,
            })
        })
        .collect();

    // Build quick_wins: low-effort high-impact (Go parity: buildQuickWins).
    // Go formula: (log2(unblocks+1)*0.4 + simplicity*0.4 + priorityBonus*0.2)
    // Sorted by quickWinScore desc, then ID asc.
    let quick_wins: Vec<serde_json::Value> = {
        let mut candidates: Vec<_> = out
            .recommendations
            .iter()
            .filter(|r| triage_claimable(r, &issue_by_id))
            .map(|r| {
                let unblocks_count = issues
                    .iter()
                    .filter(|o| {
                        o.dependencies
                            .iter()
                            .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
                    })
                    .count();
                let mut unblocks_ids: Vec<String> = issues
                    .iter()
                    .filter(|o| {
                        o.dependencies
                            .iter()
                            .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
                    })
                    .map(|o| o.id.clone())
                    .collect();
                // Go reads `unblocksMap[id]`, whose values are sorted for
                // determinism (triage.go:826-827), so the emitted
                // `unblocks_ids` is lexicographically sorted — not in issue
                // iteration order.
                unblocks_ids.sort();
                let unblock_impact = ((unblocks_count as f64) + 1.0).log2();
                // Go compares BlockerRatioNorm (triage.go:988), not the
                // weighted blocker_ratio. They differ by the 0.13 weight, so
                // using the weighted value scored every low-blocker item as
                // "simple" and doubled the quick-win score.
                let simplicity = if r.breakdown.blocker_ratio_norm < 0.2 {
                    1.0
                } else if r.breakdown.blocker_ratio_norm < 0.4 {
                    0.5
                } else {
                    0.0
                };
                let priority_bonus = if r.priority <= 1 { 0.5 } else { 0.0 };
                let qw_score = unblock_impact * 0.4 + simplicity * 0.4 + priority_bonus * 0.2;
                // Build reason (Go parity: buildQuickWins reason logic).
                let mut reason = "Low complexity".to_string();
                if unblocks_count > 0 {
                    reason = format!("Unblocks {unblocks_count} items");
                }
                if r.priority <= 1 {
                    reason.push_str(", high priority");
                }
                (qw_score, r, unblocks_ids, reason)
            })
            .collect();
        candidates.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.id.cmp(&b.1.id))
        });
        candidates
            .into_iter()
            .take(5)
            .map(|(qw_score, r, unblocks_ids, reason)| {
                // Go emits the quick-win score (impact/effort blend), not the
                // raw impact score, and carries the issue status alongside.
                let mut qw = serde_json::json!({
                    "id": r.id, "title": r.title, "status": r.status,
                    "score": qw_score,
                    "reason": reason,
                });
                if !unblocks_ids.is_empty() {
                    qw["unblocks_ids"] = serde_json::json!(unblocks_ids);
                }
                qw
            })
            .collect()
    };

    // Build blockers_to_clear: high betweenness blocking issues.
    // Go `buildBlockersToClearWithContext` (triage.go:1100-1148) builds this
    // from the triage unblocks map, not from blocker_ratio/betweenness: an item
    // qualifies when it is a non-closed candidate that unblocks something, and
    // the list is sorted by unblocks count desc then id asc.
    let issue_index: std::collections::HashMap<&str, &bv_core::model::Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut blockers: Vec<(&str, usize, Vec<String>)> = out
        .recommendations
        .iter()
        .filter_map(|r| {
            if r.blocked_by.is_empty() && r.unblocks_ids.is_empty() {
                return None;
            }
            let issue = issue_index.get(r.id.as_str())?;
            if matches!(
                issue.status,
                bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
            ) {
                return None;
            }
            Some((r.id.as_str(), r.unblocks_ids.len(), r.unblocks_ids.clone()))
        })
        .filter(|(_, n, _)| *n > 0)
        .collect();
    blockers.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let blockers_to_clear: Vec<serde_json::Value> = blockers
        .into_iter()
        .take(5)
        .map(|(id, unblocks_count, unblocks_ids)| {
            let actionable = !out
                .recommendations
                .iter()
                .find(|r| r.id == id)
                .map(|r| !r.blocked_by.is_empty())
                .unwrap_or(true);
            let mut item = serde_json::json!({
                "id": id,
                "title": issue_index.get(id).map(|i| i.title.clone()).unwrap_or_default(),
                "unblocks_count": unblocks_count,
                "actionable": actionable,
            });
            if !unblocks_ids.is_empty() {
                item["unblocks_ids"] = serde_json::json!(unblocks_ids);
            }
            if !actionable {
                item["blocked_by"] =
                    serde_json::json!(bv_analysis::blocker_chain::open_blockers(&issue_index, id));
            }
            item
        })
        .collect();

    // Project health overview.
    let graph_density = if out.counts.total > 1 {
        g.edge_count() as f64 / (out.counts.total as f64 * (out.counts.total as f64 - 1.0))
    } else {
        0.0
    };
    let project_health = serde_json::json!({
        "counts": {
            "total": out.counts.total,
            "open": out.counts.open,
            "closed": out.counts.closed,
            "blocked": out.counts.blocked,
            "actionable": out.counts.actionable,
            "not_closed": out.counts.not_closed,
            "dependency_blocked": out.counts.dependency_blocked,
            "by_status": out.counts.by_status,
            "by_type": out.counts.by_type,
            "by_priority": out.counts.by_priority,
        },
        "graph": {
            "node_count": out.counts.total,
            "edge_count": g.edge_count(),
            "density": graph_density,
            "has_cycles": false,
            "phase2_ready": true,
        },
        "velocity": out.velocity,
    });

    // Go `buildCommands` (pkg/analysis/triage.go:1243): helper commands come
    // from the live tracker route's `actions`. `show_top`/`claim_top` are the
    // tracker's shell strings (empty when no route is established) and
    // `list_ready`/`list_blocked` are always empty — there is no unique route
    // to name. `refresh_triage` is literally "bv --robot-triage".
    let top_id = out
        .recommendations
        .first()
        .map(|r| r.id.as_str())
        .unwrap_or("");
    let top_actions = bv_core::tracker::build_actions(
        &bv_core::tracker::resolve_issue_origin(&source.path, top_id),
        true,
    );
    let commands = serde_json::json!({
        "claim_top": top_actions.claim.as_ref().map(|c| c.shell.as_str()).unwrap_or(""),
        "show_top": top_actions.show.as_ref().map(|c| c.shell.as_str()).unwrap_or(""),
        "list_ready": "",
        "list_blocked": "",
        "refresh_triage": "bv --robot-triage",
    });

    let mut env = bv_robot::RobotEnvelope::new(
        data_hash.clone(),
        env!("CARGO_PKG_VERSION"),
        None,
        bv_robot::OutputFormat::Json,
    );
    env.generated_at = jiff_now(); // Truncate to seconds (Go parity).
                                   // Go's HistoryStatus (triage.go:63-71) is omitempty, so an empty value
                                   // omits the key — but when the optional git-history prologue is skipped Go
                                   // sets it to the literal "skipped" rather than leaving it empty. Omitting
                                   // it entirely, as this did, lost that distinction.
                                   // Go's HistoryStatus (triage.go:63-71) is omitempty. robot_registry.go:2155
                                   // decides the value: with open work AND SOURCE_DATE_EPOCH set
                                   // (main.go:1174), the history prologue is skipped outright and the status is
                                   // the literal "skipped" — a pinned clock would otherwise make history
                                   // output drift between runs. Only the unpinned path builds a real report
                                   // and reports "ok".
    let has_open_issues = issues.iter().any(|i| !i.status.is_closed());
    let source_date_epoch_active = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .map(|v| v.trim().parse::<i64>().is_ok())
        .unwrap_or(false);
    let history_status = if has_open_issues && source_date_epoch_active {
        "skipped"
    } else if has_open_issues {
        "ok"
    } else {
        ""
    };
    let mut meta = serde_json::json!({
        "version": bv_robot::ROBOT_CONTRACT_VERSION,
        "generated_at": env.generated_at,
        "phase2_ready": true,
        "issue_count": out.counts.total,
        "compute_time_ms": 0,
    });
    if !history_status.is_empty() {
        meta["history_status"] = serde_json::json!(history_status);
    }
    // Go parity: clear reason on all skipped entries except betweenness.
    let mut triage_status = out.metric_status.clone();
    if triage_status.page_rank.state == "skipped" {
        triage_status.page_rank.reason.clear();
    }
    if triage_status.eigenvector.state == "skipped" {
        triage_status.eigenvector.reason.clear();
    }
    if triage_status.hits.state == "skipped" {
        triage_status.hits.reason.clear();
    }
    if triage_status.critical.state == "skipped" {
        triage_status.critical.reason.clear();
    }
    if triage_status.cycles.state == "skipped" {
        triage_status.cycles.reason.clear();
    }
    if triage_status.kcore.state == "skipped" {
        triage_status.kcore.reason.clear();
    }
    if triage_status.articulation.state == "skipped" {
        triage_status.articulation.reason.clear();
    }
    if triage_status.slack.state == "skipped" {
        triage_status.slack.reason.clear();
    }
    let mut payload = full_envelope_for(&data_hash, &loaded);
    payload["output_format"] = serde_json::json!(env.output_format);
    payload["version"] = serde_json::json!(GO_APP_VERSION);
    let triage_body = serde_json::json!({
        "meta": meta,
        "status": triage_status.to_json_map(),
            "quick_ref": {
                "open_count": out.quick_ref.open_count,
                "actionable_count": out.quick_ref.actionable_count,
                "blocked_count": out.quick_ref.blocked_count,
                "in_progress_count": out.quick_ref.in_progress_count,
                "not_closed_count": out.quick_ref.not_closed_count,
                "not_actionable_count": out.quick_ref.not_actionable_count,
                "top_picks": top_picks,
            },
            // Go slices the scored set to opts.TopN (triage.go:593 sets the
            // default to 10) after building the full list, so top_picks
            // still searches the unsliced set (issue #146).
            "recommendations": recommendations_top_n(&out.recommendations),
            "quick_wins": quick_wins,
            "blockers_to_clear": blockers_to_clear,
            "project_health": project_health,
            "commands": commands,
    });
    payload["triage"] = triage_body;
    // Add as_of/as_of_commit only when --as-of was used (Go omitempty parity).
    if let Some(ref a) = as_of {
        payload["triage"]["as_of"] = serde_json::json!(a);
    }
    if let Some(ref c) = as_of_commit {
        payload["triage"]["as_of_commit"] = serde_json::json!(c);
    }
    payload["usage_hints"] = serde_json::json!([
        "jq '.triage.quick_ref.top_picks[:3]' - Top 3 picks for immediate work",
        "jq '.triage.recommendations[3:10] | map({id,title,score})' - Next candidates after top picks",
        "jq '.triage.blockers_to_clear | map(.id)' - High-impact blockers to clear",
        "jq '.triage.recommendations[] | select(.type == \"bug\")' - Bug-focused recommendations",
        "jq '.triage.quick_ref.top_picks[] | select(.unblocks > 2)' - High-impact picks",
        "jq '.triage.quick_wins' - Low-effort, high-impact items",
        "--robot-next - Get only the single top recommendation",
        "--brief - Compact output: only id/title/status/assignee/blockers/unblocks (#183)",
        "--robot-triage-by-track - Group by execution track for multi-agent coordination",
        "--robot-triage-by-label - Group by label for area-focused agents",
        "jq '.triage.recommendations_by_track[].top_pick' - Top pick per track",
        "jq '.triage.recommendations_by_label[].claim_command' - Claim commands per label",
        "jq '.feedback.weight_adjustments' - View feedback-adjusted weights (bv-90)",
        "--graph-root <id> - Scope triage to subgraph rooted at a specific epic (bv-140)",
    ]);
    match serde_json::to_string(&payload) {
        Ok(s) => {
            println!("{s}");
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Error: serialization failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn capture_baseline(
) -> Result<(bv_analysis::drift::BaselineStats, Vec<Vec<String>>, String), String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _hash, _as_of_commit) = load_issues_auto(&cwd, None)?;
    capture_baseline_for(&issues)
}

/// `capture_baseline` over an already-loaded (and possibly `--label`-scoped)
/// issue set, so a scoped command compares like with like instead of silently
/// measuring the whole repo.
fn capture_baseline_for(
    issues: &[bv_core::model::Issue],
) -> Result<(bv_analysis::drift::BaselineStats, Vec<Vec<String>>, String), String> {
    use bv_analysis::algorithms::cycles::tarjan_scc;
    let hash = bv_core::data_hash::compute_data_hash(issues);
    let g = bv_analysis::analyzer::build_graph(issues);
    let p1 = bv_analysis::analyzer::analyze_phase1(&g);
    let blocked = bv_analysis::triage::compute_blocked_set(issues);
    let actionable = issues
        .iter()
        .filter(|i| i.status.is_open() && !blocked.contains(&i.id))
        .count();

    let scc = tarjan_scc(&g);
    let mut new_cycles: Vec<Vec<String>> = Vec::new();
    // Non-trivial SCCs (size > 1) are cycles; single-node self-loops don't
    // exist in this graph model.
    for comp in &scc.components {
        if comp.len() > 1 {
            new_cycles.push(
                comp.iter()
                    .map(|i| g.node_id(*i).unwrap_or_default().to_string())
                    .collect(),
            );
        }
    }
    // Go keeps the top 10 PageRank entries for drift comparison
    // (main.go:5242 buildMetricItems): rank by value descending, then
    // truncate. Ranking by graph iteration order instead selected a
    // different set, so the "entered top" details disagreed with the
    // oracle even at the same count.
    let mut ranked: Vec<(String, f64)> = bv_analysis::algorithms::pagerank::pagerank_default(&g)
        .into_iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), v))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(10);
    let pr_map: std::collections::BTreeMap<String, f64> = ranked.into_iter().collect();
    Ok((
        bv_analysis::drift::BaselineStats {
            node_count: p1.node_count,
            edge_count: p1.edge_count,
            density: p1.density,
            open: issues
                .iter()
                .filter(|i| matches!(i.status, bv_core::model::Status::Open))
                .count(),
            closed: issues.iter().filter(|i| i.status.is_closed()).count(),
            blocked: blocked.len(),
            cycle_count: new_cycles.len(),
            actionable,
            pagerank: pr_map,
        },
        new_cycles,
        hash,
    ))
}

const BASELINE_PATH: &str = ".bv/baseline.json";

/// Application version Go bv reports in robot envelopes (`pkg/version`
/// fallback, pinned at parity commit 18afafa). Byte-parity with frozen
/// goldens requires emitting Go's version string, not the Rust crate's.
const GO_APP_VERSION: &str = "v0.25.0";

/// Output format for the current invocation, set from `--format`. Go tracks
/// this in the package-level `robotOutputFormat` and stamps it into every
/// `RobotEnvelope`; `emit_json` and `full_envelope_json` read it here.
static OUTPUT_FORMAT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn set_output_format(fmt: &str) {
    let v = if fmt == "toon" { 1 } else { 0 };
    OUTPUT_FORMAT.store(v, std::sync::atomic::Ordering::Relaxed);
}

fn output_format() -> &'static str {
    if OUTPUT_FORMAT.load(std::sync::atomic::Ordering::Relaxed) == 1 {
        "toon"
    } else {
        "json"
    }
}

fn run_save_baseline(desc: &str) -> ExitCode {
    match capture_baseline() {
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok((stats, cycles, hash)) => {
            let doc = serde_json::json!({
                "version": 1,
                "created_at": jiff_now(),
                "description": desc,
                "stats": stats,
                "commit_sha": "",
                "branch": "",
                "cycles": cycles,
                "data_hash": hash,
            });
            std::fs::create_dir_all(".bv").ok();
            match std::fs::write(BASELINE_PATH, serde_json::to_vec_pretty(&doc).unwrap()) {
                Ok(_) => {
                    println!("Baseline saved to {BASELINE_PATH} (desc: {desc})");
                    ExitCode::from(0)
                }
                Err(e) => {
                    eprintln!("Error writing baseline: {e}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

/// Handle `--check-update` (Go bv-182): report whether a newer release exists.
fn run_check_update() -> ExitCode {
    match bv_update::github::check_for_updates() {
        Err(e) => {
            eprintln!("Error checking for updates: {e}");
            ExitCode::from(1)
        }
        Ok(None) => {
            println!(
                "bvr is up to date (version {})",
                bv_update::current_version()
            );
            ExitCode::from(0)
        }
        Ok(Some(info)) => {
            println!(
                "New version available: {} (current: {})",
                info.new_version,
                bv_update::current_version()
            );
            println!("Download: {}", info.release_url);
            println!("\nRun 'bvr --update' to update automatically");
            ExitCode::from(0)
        }
    }
}

/// Handle `--update-dry-run`: report what an update would fetch/verify/install.
fn run_update_dry_run() -> ExitCode {
    let release = match bv_update::github::get_latest_release() {
        Err(e) => {
            eprintln!("Error fetching release info: {e}");
            return ExitCode::from(1);
        }
        Ok(r) => r,
    };
    if !bv_update::is_newer_than_current(&release.tag_name) {
        println!(
            "bvr is already up to date (version {})",
            bv_update::current_version()
        );
        return ExitCode::from(0);
    }
    println!(
        "[dry-run] Would update bvr from {} to {}",
        bv_update::current_version(),
        release.tag_name
    );
    match release.find_platform_asset() {
        Some(asset) => {
            println!(
                "[dry-run] Would download {} ({} bytes)",
                asset.name, asset.size
            );
            println!("[dry-run] From: {}", asset.browser_download_url);
        }
        None => {
            eprintln!("[dry-run] No matching release asset found for this platform");
        }
    }
    match release.find_checksum_asset() {
        Some(sum) => println!("[dry-run] Would verify SHA-256 checksum via {}", sum.name),
        None => println!(
            "[dry-run] Warning: no checksum file found; download integrity could not be verified"
        ),
    }
    println!("[dry-run] No changes made. Run 'bvr --update' to apply.");
    ExitCode::from(0)
}

/// Handle `--update` (Go bv-182): confirm unless `--yes`, then self-update.
fn run_update(args: &[String]) -> ExitCode {
    let release = match bv_update::github::get_latest_release() {
        Err(e) => {
            eprintln!("Error fetching release info: {e}");
            return ExitCode::from(1);
        }
        Ok(r) => r,
    };
    if !bv_update::is_newer_than_current(&release.tag_name) {
        println!(
            "bvr is already up to date (version {})",
            bv_update::current_version()
        );
        return ExitCode::from(0);
    }
    if !args.iter().any(|a| a == "--yes" || a == "-y") {
        print!(
            "Update bvr from {} to {}? [Y/n]: ",
            bv_update::current_version(),
            release.tag_name
        );
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        let mut response = String::new();
        if std::io::stdin().read_line(&mut response).is_ok() {
            let r = response.trim().to_lowercase();
            if !r.is_empty() && r != "y" && r != "yes" {
                println!("Update cancelled");
                return ExitCode::from(0);
            }
        }
    }
    match bv_update::perform_update(&release, &|line| println!("{line}")) {
        Ok(result) => {
            println!("{}", result.message);
            if let Some(backup) = result.backup_path {
                println!("Backup saved to: {backup}");
                println!("Run 'bvr --rollback' to restore if needed");
            }
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Update failed: {e}");
            ExitCode::from(1)
        }
    }
}

/// Handle `--rollback` (Go bv-182): restore the previous binary from backup.
fn run_rollback() -> ExitCode {
    match bv_update::perform_rollback() {
        Ok(()) => {
            println!("Rollback complete");
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Rollback failed: {e}");
            ExitCode::from(1)
        }
    }
}

/// Go agent management commands (--agents-add/remove/update/check).
fn run_agents_commands(presence: &validation::Presence, _args: &[String]) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let detection = bv_core::agents::detect::detect_agent_file_in_parents(&cwd, 3);
    let is_robot = presence.has("robot-robot") || std::env::var("BV_ROBOT").as_deref() == Ok("1");
    let dry_run = presence.has("agents-dry-run");
    let force = presence.has("agents-force");

    if is_robot {
        // JSON output for AI agents.
        let result = serde_json::json!({
            "found": detection.found(),
            "file_path": detection.file_path,
            "file_type": detection.file_type,
            "has_blurb": detection.has_blurb,
            "has_legacy_blurb": detection.has_legacy_blurb,
            "blurb_version": detection.blurb_version,
            "current_version": bv_core::agents::BLURB_VERSION,
            "needs_blurb": detection.found() && detection.needs_blurb(),
            "needs_upgrade": detection.needs_upgrade(),
        });
        return emit_json(&result);
    }

    let is_check = !presence.has("agents-add")
        && !presence.has("agents-remove")
        && !presence.has("agents-update");

    if is_check || presence.has("agents-check") {
        if !detection.found() {
            println!(
                "No agent file found (searched up to 3 parent directories from {})",
                cwd.display()
            );
            println!(
                "Run 'bvr --agents-add' to create AGENTS.md with beads workflow instructions."
            );
            return ExitCode::from(0);
        }
        if detection.has_legacy_blurb {
            println!(
                "Found {} at {} (legacy blurb — needs upgrade)",
                detection.file_type, detection.file_path
            );
            println!("Run 'bvr --agents-update' to upgrade to the current format.");
            return ExitCode::from(0);
        }
        if detection.has_blurb && detection.blurb_version < bv_core::agents::BLURB_VERSION {
            println!(
                "Found {} at {} (blurb v{}, current v{} — needs update)",
                detection.file_type,
                detection.file_path,
                detection.blurb_version,
                bv_core::agents::BLURB_VERSION
            );
            println!("Run 'bvr --agents-update' to update to the latest version.");
            return ExitCode::from(0);
        }
        if detection.has_blurb {
            println!(
                "Found {} at {} (blurb v{} — up to date)",
                detection.file_type, detection.file_path, detection.blurb_version
            );
            return ExitCode::from(0);
        }
        println!(
            "Found {} at {} (no beads workflow instructions)",
            detection.file_type, detection.file_path
        );
        println!("Run 'bvr --agents-add' to add beads workflow instructions.");
        return ExitCode::from(0);
    }

    if presence.has("agents-add") {
        if detection.found()
            && detection.has_blurb
            && detection.blurb_version >= bv_core::agents::BLURB_VERSION
        {
            println!(
                "{} already has current blurb (v{}) — no action needed.",
                detection.file_path, detection.blurb_version
            );
            return ExitCode::from(0);
        }
        if detection.found()
            && (detection.has_legacy_blurb
                || (detection.has_blurb
                    && detection.blurb_version < bv_core::agents::BLURB_VERSION))
        {
            println!("Existing blurb found but outdated. Use --agents-update instead.");
            return ExitCode::from(1);
        }

        let target_path = if detection.found() {
            detection.file_path.clone()
        } else {
            bv_core::agents::get_preferred_agent_file_path(&cwd)
                .to_string_lossy()
                .to_string()
        };
        let creating = !detection.found();

        if dry_run {
            if creating {
                println!("[dry-run] Would create {target_path} with beads workflow instructions.");
            } else {
                println!("[dry-run] Would append beads workflow instructions to {target_path}.");
            }
            return ExitCode::from(0);
        }

        if !force {
            let action = if creating {
                "Create"
            } else {
                "Append blurb to"
            };
            print!("{action} {target_path}? [Y/n]: ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            let resp = buf.trim().to_lowercase();
            if !resp.is_empty() && resp != "y" && resp != "yes" {
                println!("Cancelled.");
                return ExitCode::from(0);
            }
        }

        let result = if creating {
            bv_core::agents::file::create_agent_file(std::path::Path::new(&target_path))
        } else {
            bv_core::agents::file::append_blurb_to_file(std::path::Path::new(&target_path))
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }

        let msg = if creating {
            format!("Created {target_path} with beads workflow instructions.")
        } else {
            format!("Appended beads workflow instructions to {target_path}.")
        };
        println!("{msg}");

        let verified =
            bv_core::agents::file::verify_blurb_present(std::path::Path::new(&target_path))
                .unwrap_or(false);
        if !verified {
            eprintln!("Warning: verification failed — blurb may not have been written correctly.");
            return ExitCode::from(1);
        }
        return ExitCode::from(0);
    }

    if presence.has("agents-update") {
        if !detection.found() {
            println!("No agent file found. Use --agents-add to create one.");
            return ExitCode::from(1);
        }
        if !detection.has_blurb && !detection.has_legacy_blurb {
            println!(
                "{} has no blurb to update. Use --agents-add to add one.",
                detection.file_path
            );
            return ExitCode::from(1);
        }
        if detection.has_blurb && detection.blurb_version >= bv_core::agents::BLURB_VERSION {
            println!(
                "{} already has current blurb (v{}) — no update needed.",
                detection.file_path, detection.blurb_version
            );
            return ExitCode::from(0);
        }

        if dry_run {
            if detection.has_legacy_blurb {
                println!(
                    "[dry-run] Would upgrade legacy blurb to v{} in {}.",
                    bv_core::agents::BLURB_VERSION,
                    detection.file_path
                );
            } else {
                println!(
                    "[dry-run] Would update blurb from v{} to v{} in {}.",
                    detection.blurb_version,
                    bv_core::agents::BLURB_VERSION,
                    detection.file_path
                );
            }
            return ExitCode::from(0);
        }

        if !force {
            print!("Update blurb in {}? [Y/n]: ", detection.file_path);
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            let resp = buf.trim().to_lowercase();
            if !resp.is_empty() && resp != "y" && resp != "yes" {
                println!("Cancelled.");
                return ExitCode::from(0);
            }
        }

        if let Err(e) =
            bv_core::agents::file::update_blurb_in_file(std::path::Path::new(&detection.file_path))
        {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
        println!(
            "Updated blurb to v{} in {}.",
            bv_core::agents::BLURB_VERSION,
            detection.file_path
        );

        let verified =
            bv_core::agents::file::verify_blurb_present(std::path::Path::new(&detection.file_path))
                .unwrap_or(false);
        if !verified {
            eprintln!("Warning: verification failed — blurb may not have been written correctly.");
            return ExitCode::from(1);
        }
        return ExitCode::from(0);
    }

    if presence.has("agents-remove") {
        if !detection.found() {
            println!("No agent file found — nothing to remove.");
            return ExitCode::from(0);
        }
        if !detection.has_blurb && !detection.has_legacy_blurb {
            println!("{} has no blurb — nothing to remove.", detection.file_path);
            return ExitCode::from(0);
        }

        if dry_run {
            println!("[dry-run] Would remove blurb from {}.", detection.file_path);
            return ExitCode::from(0);
        }

        if !force {
            print!("Remove blurb from {}? [Y/n]: ", detection.file_path);
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            let resp = buf.trim().to_lowercase();
            if !resp.is_empty() && resp != "y" && resp != "yes" {
                println!("Cancelled.");
                return ExitCode::from(0);
            }
        }

        if let Err(e) = bv_core::agents::file::remove_blurb_from_file(std::path::Path::new(
            &detection.file_path,
        )) {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
        println!(
            "Removed beads workflow instructions from {}.",
            detection.file_path
        );
        return ExitCode::from(0);
    }

    // Default: should not reach here
    eprintln!("No agents action specified. Use --agents-add, --agents-remove, --agents-update, or --agents-check.");
    ExitCode::from(1)
}

fn run_check_drift() -> ExitCode {
    let baseline_doc = match std::fs::read_to_string(BASELINE_PATH) {
        Ok(raw) => raw,
        Err(_) => {
            eprintln!("No baseline found at {BASELINE_PATH}. Save one with --save-baseline.");
            return ExitCode::from(1);
        }
    };
    let base: serde_json::Value = serde_json::from_str(&baseline_doc).expect("baseline parses");
    let base_stats: bv_analysis::drift::BaselineStats =
        serde_json::from_value(base["stats"].clone()).expect("baseline stats shape");
    let old_cycles: Vec<Vec<String>> = base
        .get("cycles")
        .and_then(|c| serde_json::from_value(c.clone()).ok())
        .unwrap_or_default();

    let (current, new_cycles, _hash) = match capture_baseline() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let issues = load_issues_auto(&cwd, None)
        .map(|(i, _, _)| i)
        .unwrap_or_default();

    let norm: std::collections::HashSet<String> = old_cycles
        .iter()
        .map(|c| {
            let mut v = c.clone();
            v.sort();
            v.join("|")
        })
        .collect();
    let fresh_cycles: Vec<Vec<String>> = new_cycles
        .into_iter()
        .filter(|c| {
            let mut v = c.clone();
            v.sort();
            !norm.contains(&v.join("|"))
        })
        .collect();

    let result = bv_analysis::drift::calculate(
        &base_stats,
        &current,
        &bv_analysis::drift::DriftConfig::default(),
        &fresh_cycles,
        &issues,
        robot_now(),
    );
    println!(
        "{}",
        serde_json::json!({
            "has_drift": result.has_drift,
            "exit_code": result.exit_code(),
            "summary": format!(
                "{} critical, {} warning, {} info",
                result.critical_count, result.warning_count, result.info_count
            ),
            "alerts": result.alerts,
        })
    );
    ExitCode::from(result.exit_code())
}

/// Go `robot-drift` — wraps `--check-drift` with structured JSON output.
/// JSON schema matches Go output struct at cmd/bv/main.go:3509-3541.
fn run_robot_drift() -> ExitCode {
    let baseline_doc = match std::fs::read_to_string(BASELINE_PATH) {
        Ok(raw) => raw,
        Err(_) => {
            let payload = serde_json::json!({
                "generated_at": jiff_now(),
                "error": format!("No baseline found at {BASELINE_PATH}. Save one with --save-baseline."),
            });
            emit_json(&payload);
            return ExitCode::from(1);
        }
    };
    let base: serde_json::Value = serde_json::from_str(&baseline_doc).expect("baseline parses");
    let base_stats: bv_analysis::drift::BaselineStats =
        serde_json::from_value(base["stats"].clone()).expect("baseline stats shape");
    let old_cycles: Vec<Vec<String>> = base
        .get("cycles")
        .and_then(|c| serde_json::from_value(c.clone()).ok())
        .unwrap_or_default();
    let baseline_created_at = base
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let baseline_commit_sha = base
        .get("commit_sha")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let (current, new_cycles, _hash) = match capture_baseline() {
        Ok(x) => x,
        Err(e) => {
            let payload = serde_json::json!({
                "generated_at": jiff_now(),
                "error": format!("Error capturing baseline: {e}"),
            });
            emit_json(&payload);
            return ExitCode::from(1);
        }
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let issues = load_issues_auto(&cwd, None)
        .map(|(i, _, _)| i)
        .unwrap_or_default();

    let norm: std::collections::HashSet<String> = old_cycles
        .iter()
        .map(|c| {
            let mut v = c.clone();
            v.sort();
            v.join("|")
        })
        .collect();
    let fresh_cycles: Vec<Vec<String>> = new_cycles
        .into_iter()
        .filter(|c| {
            let mut v = c.clone();
            v.sort();
            !norm.contains(&v.join("|"))
        })
        .collect();

    let result = bv_analysis::drift::calculate(
        &base_stats,
        &current,
        &bv_analysis::drift::DriftConfig::default(),
        &fresh_cycles,
        &issues,
        robot_now(),
    );

    // Go parity: exact JSON structure from cmd/bv/main.go:3511-3541.
    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "has_drift": result.has_drift,
        "exit_code": result.exit_code(),
        "summary": {
            "critical": result.critical_count,
            "warning": result.warning_count,
            "info": result.info_count,
        },
        "alerts": result.alerts,
        "baseline": {
            "created_at": baseline_created_at,
            "commit_sha": baseline_commit_sha,
        },
    });
    emit_json(&payload);
    ExitCode::from(result.exit_code())
}

/// Go `correlation.ValidateRepository` — a `.git` directory plus at least one
/// of the known beads file names under `.beads/`.
fn validate_correlation_repository(repo: &std::path::Path) -> Result<(), String> {
    if !repo.join(".git").exists() {
        return Err(format!("not a git repository: {}", repo.to_string_lossy()));
    }
    let found = bv_correlation::extractor::DEFAULT_BEADS_FILES
        .iter()
        .any(|name| repo.join(".beads").join(name).exists());
    if !found {
        return Err(format!(
            "no beads file found in {}/.beads/",
            repo.to_string_lossy()
        ));
    }
    Ok(())
}

/// Read a string flag from the process argv, accepting both `--flag=value` and
/// `--flag value`. `rewrite_args` only rewrites bare boolean flags, so the raw
/// argv still carries every valued flag verbatim.
fn history_flag_value(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let with_eq = format!("--{name}=");
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix(&with_eq) {
            return Some(v.to_string());
        }
        if a == &format!("--{name}") {
            return args.get(i + 1).cloned();
        }
    }
    None
}

/// The zone Go's `now.Location()` has for the current run: a frozen
/// `SOURCE_DATE_EPOCH` instant is UTC, an unfrozen `time.Now()` is Local.
fn robot_now_zone() -> jiff::tz::TimeZone {
    if let Ok(v) = std::env::var("SOURCE_DATE_EPOCH") {
        if v.trim().parse::<i64>().is_ok() {
            return jiff::tz::TimeZone::UTC;
        }
    }
    jiff::tz::TimeZone::try_system().unwrap_or(jiff::tz::TimeZone::UTC)
}

/// Go `recipe.ParseRelativeTime` — `7d` / `3w` / `6m` / `1y` counted back from
/// now, else RFC3339, else `YYYY-MM-DDTHH:MM:SS`, else `YYYY-MM-DD`.
///
/// The reference "now" is `robot_now()` (SOURCE_DATE_EPOCH-aware), not the wall
/// clock: Go resolves relative times against the same instant it stamps
/// `generated_at` with, so a frozen-clock run stays frozen here too. An explicit
/// RFC3339 input keeps its own offset — Go's `time.Parse` retains the parsed
/// location, and the `window`/`git_range` fields echo that offset back.
fn parse_relative_time(raw: &str) -> Result<Option<String>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    // Relative form: a digit run followed by one of d/w/m/y.
    let lower = s.to_lowercase();
    if lower.len() >= 2 {
        let (digits, unit) = lower.split_at(lower.len() - 1);
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            let n: i64 = digits
                .parse()
                .map_err(|_| format!("could not parse time \"{raw}\""))?;
            let span = match unit {
                "d" => jiff::Span::new().days(-n),
                "w" => jiff::Span::new().days(-n * 7),
                "m" => jiff::Span::new().months(-n),
                "y" => jiff::Span::new().years(-n),
                _ => jiff::Span::new().days(0),
            };
            if !span.is_zero() {
                let shifted = robot_now()
                    .to_zoned(robot_now_zone())
                    .checked_add(span)
                    .map_err(|e| e.to_string())?;
                return Ok(Some(shifted.timestamp().to_string()));
            }
        }
    }
    // An explicit instant is already in the shape the report echoes back.
    if s.parse::<jiff::Timestamp>().is_ok() {
        return Ok(Some(s.to_string()));
    }
    // Naive layouts, read in the reference zone like Go's ParseInLocation.
    let tz = robot_now_zone();
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d"] {
        if let Ok(dt) = jiff::civil::DateTime::strptime(fmt, s) {
            if let Ok(zoned) = dt.to_zoned(tz.clone()) {
                return Ok(Some(zoned.timestamp().to_string()));
            }
        }
    }
    if let Ok(date) = jiff::civil::Date::strptime("%Y-%m-%d", s) {
        let dt = date.to_datetime(jiff::civil::Time::midnight());
        if let Ok(zoned) = dt.to_zoned(tz) {
            return Ok(Some(zoned.timestamp().to_string()));
        }
    }
    Err(format!("could not parse time \"{raw}\""))
}

fn run_robot_history() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Err(e) = validate_correlation_repository(&cwd) {
        eprintln!("Error: {e}");
        return ExitCode::from(1);
    }
    let (issues, _, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    // Go: CorrelatorOptions{Limit: 500} overridden by --history-limit, with
    // --bead-history narrowing to one bead and --history-since bounding the
    // window (a parse failure is fatal, as in Go's handleRobotHistory).
    let mut opts = bv_correlation::history::HistoryOptions {
        limit: 500,
        ..Default::default()
    };
    if let Some(bead) = history_flag_value("bead-history") {
        opts.bead_id = bead.trim().to_string();
    }
    if let Some(limit) = history_flag_value("history-limit") {
        match limit.trim().parse::<i64>() {
            Ok(v) => opts.limit = v,
            Err(_) => {
                eprintln!("Error: invalid --history-limit: {limit}");
                return ExitCode::from(2);
            }
        }
    }
    if let Some(since) = history_flag_value("history-since") {
        match parse_relative_time(&since) {
            Ok(Some(ts)) => opts.since = Some(ts),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error: parsing --history-since: {e}");
                return ExitCode::from(1);
            }
        }
    }

    // Go builds BeadInfo from the loaded issues, in load order.
    let beads: Vec<bv_correlation::history::BeadInfo> = issues
        .iter()
        .map(|i| bv_correlation::history::BeadInfo {
            id: i.id.clone(),
            title: i.title.clone(),
            status: i.status.as_str().to_string(),
        })
        .collect();

    let mut report = match bv_correlation::history::build_history_report(
        &cwd,
        &beads,
        &opts,
        None,
        jiff_now(),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: generating history report: {e}");
            return ExitCode::from(1);
        }
    };

    // Go: --min-confidence filters the assembled histories, then re-derives the
    // commit index and the beads_with_commits count.
    if let Some(min) = history_flag_value("min-confidence") {
        if let Ok(floor) = min.trim().parse::<f64>() {
            bv_correlation::history::filter_histories_by_confidence(&mut report, floor);
        }
    }

    // Same top-level keys as the report plus the shared envelope. The envelope
    // carries generated_at/data_hash (Go's output struct embeds the envelope
    // and copies the report's remaining fields rather than embedding it, so the
    // colliding keys would have been dropped).
    //
    // Go builds the envelope from the *loader's* source authority (whose
    // per-source data_hash is the full-file sha256) and only then overrides the
    // top-level data_hash with the report's own bead fingerprint — so
    // authority_hash is computed over the file hash while scope_hash is
    // computed over the report hash. Build with the file hash, then substitute.
    let file_hash = bv_core::data_hash::compute_data_hash(&issues);
    let mut payload = full_envelope_for(&file_hash, &issues);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("data_hash".into(), serde_json::json!(report.data_hash));
        let mut ids: Vec<String> = issues.iter().map(|i| i.id.clone()).collect();
        ids.sort();
        let scope_hash = bv_robot::scope_hash("", "", "", &report.data_hash, &ids);
        if !scope_hash.is_empty() {
            obj.insert("scope_hash".into(), serde_json::json!(scope_hash));
        }
        obj.insert("git_range".into(), serde_json::json!(report.git_range));
        if !report.latest_commit_sha.is_empty() {
            obj.insert(
                "latest_commit_sha".into(),
                serde_json::json!(report.latest_commit_sha),
            );
        }
        obj.insert(
            "window".into(),
            serde_json::to_value(&report.window).unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "stats".into(),
            serde_json::to_value(&report.stats).unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "histories".into(),
            serde_json::to_value(&report.histories).unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "commit_index".into(),
            serde_json::to_value(&report.commit_index).unwrap_or(serde_json::Value::Null),
        );
    }
    emit_json(&payload)
}

/// Go `handleRobotOrphans` — build the same correlation report `--robot-history`
/// produces, hand it to the orphan detector (which scans exactly the window
/// that index covered), then drop candidates below `--orphans-min-score`.
fn run_robot_orphans() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Err(e) = validate_correlation_repository(&cwd) {
        eprintln!("Error: {e}");
        return ExitCode::from(1);
    }
    let (issues, _, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    // Go: CorrelatorOptions{Limit: 500}, same default the history handler uses.
    let mut opts = bv_correlation::history::HistoryOptions {
        limit: 500,
        ..Default::default()
    };
    if let Some(limit) = history_flag_value("history-limit") {
        match limit.trim().parse::<i64>() {
            Ok(v) => opts.limit = v,
            Err(_) => {
                eprintln!("Error: invalid --history-limit: {limit}");
                return ExitCode::from(2);
            }
        }
    }

    let min_score: i32 = match history_flag_value("orphans-min-score") {
        Some(v) => match v.trim().parse::<i32>() {
            Ok(score) => score,
            Err(_) => {
                eprintln!("Error: invalid --orphans-min-score: {v}");
                return ExitCode::from(2);
            }
        },
        None => 30,
    };

    let beads: Vec<bv_correlation::history::BeadInfo> = issues
        .iter()
        .map(|i| bv_correlation::history::BeadInfo {
            id: i.id.clone(),
            title: i.title.clone(),
            status: i.status.as_str().to_string(),
        })
        .collect();

    let report = match bv_correlation::history::build_history_report(
        &cwd,
        &beads,
        &opts,
        None,
        jiff_now(),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: generating history report: {e}");
            return ExitCode::from(1);
        }
    };

    let now = robot_now();
    let mut orphan_report =
        match bv_correlation::orphan::detect_orphans(&cwd, &report, now, jiff_now()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: detecting orphans: {e}");
                return ExitCode::from(1);
            }
        };
    bv_correlation::orphan::filter_by_min_score(&mut orphan_report, min_score);

    // Go copies the report's fields onto a struct that embeds the envelope
    // rather than the report, so the envelope's generated_at/data_hash lead and
    // the report's own copies of those keys are dropped.
    let file_hash = bv_core::data_hash::compute_data_hash(&issues);
    let mut payload = full_envelope_for(&file_hash, &issues);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "data_hash".into(),
            serde_json::json!(orphan_report.data_hash),
        );
        let mut ids: Vec<String> = issues.iter().map(|i| i.id.clone()).collect();
        ids.sort();
        let scope_hash = bv_robot::scope_hash("", "", "", &orphan_report.data_hash, &ids);
        if !scope_hash.is_empty() {
            obj.insert("scope_hash".into(), serde_json::json!(scope_hash));
        }
        obj.insert(
            "git_range".into(),
            serde_json::json!(orphan_report.git_range),
        );
        obj.insert(
            "window".into(),
            serde_json::to_value(&orphan_report.window).unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "stats".into(),
            serde_json::to_value(&orphan_report.stats).unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "candidates".into(),
            serde_json::to_value(&orphan_report.candidates).unwrap_or(serde_json::Value::Null),
        );
        if !orphan_report.by_bead.is_empty() {
            obj.insert(
                "by_bead".into(),
                serde_json::to_value(&orphan_report.by_bead).unwrap_or(serde_json::Value::Null),
            );
        }
        obj.insert(
            "usage_hints".into(),
            serde_json::to_value(&orphan_report.usage_hints).unwrap_or(serde_json::Value::Null),
        );
    }
    emit_json(&payload)
}

type AnalysisTuple = (
    Vec<bv_core::model::Issue>,
    String,
    bv_analysis::analyzer::Phase1Stats,
    bv_analysis::MetricStatus,
    std::sync::Arc<bv_graph_core::DiGraph>,
);

/// Shared helper: load issues, build graph, run analysis phases.
fn load_and_analyze() -> Result<AnalysisTuple, ExitCode> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return Err(ExitCode::from(1));
        }
    };
    if issues.is_empty() {
        println!(
            "{{\"generated_at\":\"{}\",\"data_hash\":\"empty\",\"error\":\"no issues loaded\"}}",
            jiff_now()
        );
        return Err(ExitCode::from(0));
    }
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = std::sync::Arc::new(bv_analysis::build_graph(&issues));
    let p1 = bv_analysis::analyze_phase1(&g);
    let budget = bv_analysis::AnalysisBudget {
        density: p1.density,
        ..bv_analysis::AnalysisBudget::default()
    };

    // Phase 2 — run blocking since we're in a CLI context.
    // For parity with Go's async behavior we'd need the thread-per-metric approach;
    // for now use the synchronous path which is equivalent in output.
    let g2 = std::sync::Arc::clone(&g);
    let (status, _phase2) = bv_analysis::analyze_phase2_blocking(g2, &budget);

    Ok((issues, data_hash, p1, status, g))
}

/// Go `NewRobotEnvelope` parity: for handlers whose Go output embeds the
/// full `RobotEnvelope` struct (alerts, next, history, …) the envelope also
/// carries `output_format` and `version` — golden-verified.
/// Build the v0.25.0 envelope prefix. Field order matches Go `RobotEnvelope`
/// (cmd/bv/main.go:7215) because the serializer preserves insertion order.
/// Envelope with Go v0.25.0 source provenance + scope/authority hashes.
fn full_envelope_json_with_source(
    data_hash: &str,
    source: Option<&SourceMeta>,
    issues: &[bv_core::model::Issue],
) -> serde_json::Value {
    let mut env = serde_json::Map::new();
    env.insert("generated_at".into(), serde_json::json!(jiff_now()));
    env.insert("data_hash".into(), serde_json::json!(data_hash));
    let fmt = output_format();
    if !fmt.is_empty() {
        env.insert("output_format".into(), serde_json::json!(fmt));
    }
    env.insert("version".into(), serde_json::json!(GO_APP_VERSION));
    if let Some(meta) = source {
        if !meta.path.is_empty() {
            env.insert("source_path".into(), serde_json::json!(meta.path));
        }
        if !meta.kind.is_empty() {
            env.insert("source_kind".into(), serde_json::json!(meta.kind));
        }
        let authority = source_authority(meta, data_hash);
        let ahash = bv_robot::authority_hash(&authority);
        env.insert(
            "source_authority".into(),
            serde_json::to_value(&authority).unwrap_or(serde_json::Value::Null),
        );
        if !ahash.is_empty() {
            env.insert("authority_hash".into(), serde_json::json!(ahash));
        }
        let mut ids: Vec<String> = issues.iter().map(|i| i.id.clone()).collect();
        ids.sort();
        // Go derives the scope from the active --label/--recipe/--repo flags
        // (robot_registry.go:257-269) and emits the object alongside its hash.
        // Hashing empty strings here made scope_hash unreproducible and left
        // `scope` absent even when the caller was scoped.
        let (label, recipe, repo) = active_scope_flags();
        // A --label scope narrows the candidate set to the label's own issues
        // (their neighbours stay in the analysis as context) — see Go
        // scopeLoadedIssues, main.go:4870-4900.
        let (mut candidate_ids, _) = bv_analysis::label_health::label_scope_ids(&label, issues);
        candidate_ids.sort();
        let shash = bv_robot::scope_hash(&label, &recipe, &repo, data_hash, &candidate_ids);
        if !shash.is_empty() {
            env.insert("scope_hash".into(), serde_json::json!(shash));
        }
        if !label.is_empty() || !recipe.is_empty() || !repo.is_empty() {
            // Go's RobotScope fields are omitempty (robot_registry.go:262-266),
            // so only the active modifiers appear. Emitting the empty ones both
            // diverges from the oracle and feeds different values into the
            // scope hash.
            let mut scope = serde_json::Map::new();
            if !label.is_empty() {
                scope.insert("label".into(), serde_json::json!(label));
            }
            if !recipe.is_empty() {
                scope.insert("recipe".into(), serde_json::json!(recipe));
            }
            if !repo.is_empty() {
                scope.insert("repo".into(), serde_json::json!(repo));
            }
            env.insert("scope".into(), serde_json::Value::Object(scope));
        }
    }
    serde_json::Value::Object(env)
}

fn emit_json(v: &serde_json::Value) -> ExitCode {
    println!("{}", go_json_string(v));
    ExitCode::from(0)
}

/// Go `encoding/json`-compatible compact serializer. The one behavioral
/// difference vs serde_json: float formatting. Go emits `1` for 1.0 and
/// `30` for 30.0 (shortest round-trip, no trailing ".0"); serde_json/ryu
/// emits "1.0". Drop-in byte parity with Go goldens requires Go semantics.
fn go_json_string(v: &serde_json::Value) -> String {
    let mut out = String::new();
    write_go_json(v, &mut out);
    out
}

/// Quote a string the way Go's `encoding/json` does with its default HTML
/// escaping: `<`, `>` and `&` become \u003c, \u003e and \u0026.
fn go_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_go_json(v: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if n.as_i64().is_none() && n.as_u64().is_none() {
                    out.push_str(&go_format_f64(f));
                    return;
                }
            }
            out.push_str(&n.to_string());
        }
        // Go's encoding/json escapes <, > and & by default (SetEscapeHTML is
        // on unless explicitly disabled), emitting \u003c, \u003e and \u0026.
        // serde_json does not, so a commit message containing "<=" serialized
        // differently from the oracle.
        Value::String(s) => out.push_str(&go_escape_string(s)),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_go_json(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, val)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                write_go_json(val, out);
            }
            out.push('}');
        }
    }
}

/// Go `strconv.AppendFloat(f, 'f'/'e', -1, 64)` semantics: shortest
/// decimal that round-trips. 'f' (no exponent) normally; 'e' when
/// abs < 1e-6 or abs >= 1e21, with Go's e-09 → e-9 exponent cleanup.
fn go_format_f64(f: f64) -> String {
    if f.is_nan() || f.is_infinite() {
        // Go json errors on these; our data never produces them.
        return "null".into();
    }
    let abs = f.abs();
    if abs != 0.0 && !(1e-6..1e21).contains(&abs) {
        // Rust {:e} → "1.5e21"; Go → "1.5e+21". Reconstruct Go style.
        let s = format!("{:e}", f); // e.g. "1.5e21", "1e-7", "-2.5e-8"
        if let Some(pos) = s.find('e') {
            let (mantissa, exp) = s.split_at(pos);
            let exp = &exp[1..]; // strip 'e'
            let (sign, digits) = if let Some(stripped) = exp.strip_prefix('-') {
                ("-", stripped)
            } else {
                ("+", exp)
            };
            let digits = if digits.len() == 2 && digits.starts_with('0') {
                &digits[1..] // "09" → "9" (Go cleanup e-09 → e-9)
            } else {
                digits
            };
            return format!("{mantissa}e{sign}{digits}");
        }
        return s;
    }
    // 'f' shortest: Rust Display matches Go 'f' -1 (no trailing .0).
    format!("{f}")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod go_json_tests {
    use super::*;

    #[test]
    fn go_float_format_matches_go_json() {
        assert_eq!(go_format_f64(1.0), "1");
        assert_eq!(go_format_f64(30.0), "30");
        assert_eq!(go_format_f64(0.5), "0.5");
        assert_eq!(go_format_f64(0.7415134907189797), "0.7415134907189797");
        assert_eq!(go_format_f64(1e-7), "1e-7");
        assert_eq!(go_format_f64(1e21), "1e+21");
        assert_eq!(go_format_f64(233.5454), "233.5454");
        assert_eq!(go_format_f64(0.0), "0");
    }

    #[test]
    fn go_json_string_whole_floats_have_no_dot() {
        let v = serde_json::json!({"a": 1.0, "b": 30.0, "c": 0.125});
        assert_eq!(go_json_string(&v), r#"{"a":1,"b":30,"c":0.125}"#);
    }
}

type AnalysisResultFull = (
    Vec<bv_core::model::Issue>,
    String,
    bv_analysis::analyzer::Phase1Stats,
    bv_analysis::MetricStatus,
    std::sync::Arc<bv_graph_core::DiGraph>,
    bv_analysis::GraphAnalysisPhase2,
);

fn load_full() -> Result<AnalysisResultFull, ExitCode> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let as_of = extract_as_of();
    let issues = if let Some(ref revision) = as_of {
        let loader = bv_core::discovery::GitLoader::new(&cwd);
        match loader.load_at(revision) {
            Ok(issues) => {
                if let Ok(sha) = loader.resolve_revision(revision) {
                    eprintln!(
                        "Loaded {} issues from {} ({})",
                        issues.len(),
                        revision,
                        &sha[..sha.len().min(7)]
                    );
                }
                issues
            }
            Err(e) => {
                eprintln!("Error loading issues at {revision}: {e}");
                return Err(ExitCode::from(1));
            }
        }
    } else {
        match bv_core::discovery::load_issues_from_repo(&cwd) {
            Ok((issues, _)) => issues,
            Err(e) => {
                eprintln!("Error: {e}");
                return Err(ExitCode::from(1));
            }
        }
    };
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = std::sync::Arc::new(bv_analysis::build_graph(&issues));
    let p1 = bv_analysis::analyze_phase1(&g);
    let budget = bv_analysis::AnalysisBudget {
        density: p1.density,
        ..bv_analysis::AnalysisBudget::default()
    };
    let gc = std::sync::Arc::clone(&g);
    let (status, phase2) = bv_analysis::analyze_phase2_blocking(gc, &budget);
    Ok((issues, data_hash, p1, status, g, phase2))
}

fn to_id_map(
    g: &bv_graph_core::DiGraph,
    scores: &[f64],
) -> serde_json::Map<String, serde_json::Value> {
    // Insert in lexicographic ID order — Go json.Marshal sorts map keys,
    // so serialized key order must be lexicographic (index order is
    // issue-load order, which differs for IDs like FIX-10 vs FIX-2).
    let mut sorted: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for (i, v) in scores.iter().enumerate() {
        let id = g.node_id(i).unwrap_or_default();
        sorted.insert(id, *v);
    }
    let mut m = serde_json::Map::new();
    for (id, v) in sorted {
        if v.fract() == 0.0 && v.abs() < 1e15 {
            m.insert(id.to_string(), serde_json::json!(v.round() as i64));
        } else {
            m.insert(id.to_string(), serde_json::json!(v));
        }
    }
    m
}

/// Go `ConfigForSize` JSON shape (ns timeouts) — golden-verified per tier.
fn insights_analysis_config(nodes: usize) -> serde_json::Value {
    let (
        timeout_ns,
        mode,
        sample,
        pr_ns,
        hits_ns,
        cycles_ns,
        max_cycles,
        compute_cycles,
        cycles_skip,
    ) = match nodes {
        n if n < 100 => (
            2_000_000_000i64,
            "exact",
            0,
            2_000_000_000i64,
            2_000_000_000i64,
            2_000_000_000i64,
            1000,
            true,
            "",
        ),
        n if n < 500 => (
            500_000_000i64,
            "exact",
            0,
            500_000_000i64,
            500_000_000i64,
            500_000_000i64,
            100,
            true,
            "",
        ),
        n if n < 2000 => (
            500_000_000i64,
            "approximate",
            100,
            300_000_000i64,
            300_000_000i64,
            300_000_000i64,
            50,
            true,
            "",
        ),
        _ => (
            500_000_000i64,
            "approximate",
            200,
            200_000_000i64,
            200_000_000i64,
            0i64,
            10,
            false,
            "graph too large (>2000 nodes)",
        ),
    };
    serde_json::json!({
        "ComputeBetweenness": true,
        "BetweennessTimeout": timeout_ns,
        "BetweennessSkipReason": "",
        "BetweennessMode": mode,
        "BetweennessSampleSize": sample,
        "BetweennessIsApproximate": false,
        "ComputePageRank": true,
        "PageRankTimeout": pr_ns,
        "PageRankSkipReason": "",
        "ComputeHITS": true,
        "HITSTimeout": hits_ns,
        "HITSSkipReason": "",
        "ComputeCycles": compute_cycles,
        "CyclesTimeout": cycles_ns,
        "MaxCyclesToStore": max_cycles,
        "CyclesSkipReason": cycles_skip,
        "ComputeEigenvector": true,
        "ComputeCriticalPath": true,
        "ComputeKCore": true,
        "ComputeArticulation": true,
        "ComputeSlack": true,
    })
}

/// Go `getTopItems`: sort by value desc, ID asc tiebreak, cap at limit.
fn top_items_go(
    map: &serde_json::Map<String, serde_json::Value>,
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut items: Vec<(&String, f64)> = map
        .iter()
        .filter_map(|(k, v)| v.as_f64().map(|f| (k, f)))
        .collect();
    items.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    items
        .into_iter()
        .take(limit)
        .map(|(id, v)| serde_json::json!({"ID": id, "Value": v}))
        .collect()
}

fn run_robot_insights() -> ExitCode {
    let all = match load_full() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let (issues, hash, p1, status, g, phase2) = all;

    let pr_obj = to_id_map(&g, &bv_graph_core::pagerank_default(&g));
    // Go's betweenness here is the config-driven Phase 2 result, not a
    // direct exact call: `ConfigForSize` switches to approximate sampling above
    // the density/size thresholds (config.go:158-190). Computing exact
    // unconditionally gave `large_cyclic_600` max 184 / 337 non-zero where Go
    // reports 210 / 176, and `xl_2500` max 12 / 124 where Go reports
    // 37.5 / 15.
    let bw_nodes = g.len();
    let (use_approx, skip_bw) =
        bv_analysis::analyzer::AnalysisBudget::default().betweenness_mode(bw_nodes);
    let bw_sample =
        bv_analysis::analyzer::AnalysisBudget::default().recommend_sample_size(bw_nodes, 0);
    let bw_raw: Vec<f64> = if skip_bw {
        Vec::new()
    } else if use_approx {
        bv_graph_core::betweenness_approx(&g, bw_sample, Some(1))
    } else {
        bv_graph_core::betweenness(&g)
    };
    let mut bw_obj = to_id_map(&g, &bw_raw);
    // gonum Betweenness omits zero-score nodes (endpoints of a DAG chain).
    bw_obj.retain(|_, v| v.as_f64() != Some(0.0));
    let ev_raw = bv_graph_core::eigenvector_default(&g);
    let ev_obj = to_id_map(&g, &ev_raw);
    let hits_result = bv_graph_core::hits_default(&g);
    let hub_obj = to_id_map(&g, &hits_result.hubs);
    let auth_obj = to_id_map(&g, &hits_result.authorities);
    // Go computes critical-path heights only when Phase 1's topological sort
    // covered every node (graph.go:2161-2163) — i.e. the graph is acyclic. A
    // height DP needs a valid order; on a cyclic graph gonum returns an
    // Unorderable error, Go skips the metric and leaves the map empty.
    // large_cyclic_600 is cyclic: Go emits `{}` there, Rust emitted 200
    // entries of invented scores.
    let cp_heights: Vec<f64> =
        if bv_graph_core::algorithms::topo::topological_sort_gonum(&g).is_some() {
            bv_graph_core::critical_path_heights(&g)
        } else {
            Vec::new()
        };
    let cp_obj = to_id_map(&g, &cp_heights);
    let cores = bv_graph_core::kcore(&g);
    let core_obj = to_id_map(&g, &cores.iter().map(|&v| v as f64).collect::<Vec<_>>());
    let slacks = bv_graph_core::slack(&g);
    let slack_obj = to_id_map(&g, &slacks);
    let art_pts = bv_graph_core::algorithms::articulation::articulation_points(&g);
    let mut art_ids: Vec<String> = art_pts
        .iter()
        .map(|&i| g.node_id(i).unwrap_or_default().to_string())
        .collect();
    // Go ArticulationPoints() sorts with sort.Strings (lexicographic).
    art_ids.sort();

    let n = g.len() as f64;
    let density = if n <= 1.0 {
        0.0
    } else {
        g.edge_count() as f64 / (n * (n - 1.0))
    };

    let mut payload = full_envelope_for(&hash, &issues);
    payload["analysis_config"] = insights_analysis_config(g.len());
    // Go parity: only "approximate" reason is non-empty for skipped entries.
    // All other skipped metrics emit {"state":"skipped"} without reason field.
    let mut fixed_status = status.clone();
    if fixed_status.page_rank.state == "skipped" {
        fixed_status.page_rank.reason.clear();
    }
    if fixed_status.betweenness.state != "computed" { /* keep reason for approx */ }
    if fixed_status.eigenvector.state == "skipped" {
        fixed_status.eigenvector.reason.clear();
    }
    if fixed_status.hits.state == "skipped" {
        fixed_status.hits.reason.clear();
    }
    if fixed_status.critical.state == "skipped" {
        fixed_status.critical.reason.clear();
    }
    // NOT cleared, unlike the siblings above: Go keeps the cycles skip reason.
    // On an XL graph cycle detection never runs, and "graph too large (>2000
    // nodes)" is the only thing telling a reader the absence of cycles is
    // unknown rather than observed. Clearing it asserted a clean DAG.
    if fixed_status.kcore.state == "skipped" {
        fixed_status.kcore.reason.clear();
    }
    if fixed_status.articulation.state == "skipped" {
        fixed_status.articulation.reason.clear();
    }
    if fixed_status.slack.state == "skipped" {
        fixed_status.slack.reason.clear();
    }
    payload["status"] = fixed_status.to_json_map();

    // Go GenerateInsights(limit=50): value desc, ID asc tiebreak.
    const INSIGHTS_LIMIT: usize = 50;
    payload["Bottlenecks"] = serde_json::Value::Array(top_items_go(&bw_obj, INSIGHTS_LIMIT));
    payload["Keystones"] = serde_json::Value::Array(top_items_go(&cp_obj, INSIGHTS_LIMIT));
    payload["Influencers"] = serde_json::Value::Array(top_items_go(&ev_obj, INSIGHTS_LIMIT));
    payload["Hubs"] = serde_json::Value::Array(top_items_go(&hub_obj, INSIGHTS_LIMIT));
    payload["Authorities"] = serde_json::Value::Array(top_items_go(&auth_obj, INSIGHTS_LIMIT));
    payload["Cores"] = serde_json::Value::Array(top_items_go(&core_obj, INSIGHTS_LIMIT));
    // Go caps every insights list at the same `limit` (insights.go:88-96,
    // `limitStrings(artPts, limit)` at :94). Rust emitted the articulation
    // set uncapped — 204 entries on xl_2500 where Go caps at 50.
    let art_capped: Vec<&String> = art_ids.iter().take(INSIGHTS_LIMIT).collect();
    payload["Articulation"] = serde_json::json!(art_capped);
    payload["Slack"] = serde_json::Value::Array(top_items_go(&slack_obj, INSIGHTS_LIMIT));

    // Orphans: zero out-degree (nothing depends on them), sorted (Go findOrphans).
    let orphans: Vec<String> = (0..g.len())
        .filter(|&i| g.out_degree(i) == 0)
        .map(|i| g.node_id(i).unwrap_or_default().to_string())
        .collect();
    // Same `limit` applies to Orphans (insights.go:95) — 1855 uncapped here.
    let orphans_capped: Vec<&String> = orphans.iter().take(INSIGHTS_LIMIT).collect();
    payload["Orphans"] = serde_json::json!(orphans_capped);

    // Cycles: Go emits null when none detected.
    let cycles_from_phase2 = phase2.cycles.clone().unwrap_or_default();
    payload["Cycles"] = serde_json::json!(cycles_from_phase2);
    payload["ClusterDensity"] = serde_json::json!(density);

    // Velocity snapshot — Go VelocitySnapshot: weekly as plain ints (newest
    // first), estimated omitted when false, no week_start objects.
    if let Some(vel) = bv_analysis::triage::compute_project_velocity(&issues, robot_now()) {
        let weekly_ints: Vec<i64> = vel["weekly"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|w| w["closed"].as_i64().unwrap_or(0))
                    .collect()
            })
            .unwrap_or_default();
        let snapshot = serde_json::json!({
            "closed_last_7_days": vel["closed_last_7_days"],
            "closed_last_30_days": vel["closed_last_30_days"],
            "avg_days_to_close": vel["avg_days_to_close"],
            "weekly": weekly_ints,
        });
        payload["Velocity"] = snapshot;
    }

    // Stats sub-object (Go GraphStats JSON shape) — comes before
    // full_stats/top_what_ifs/advanced_insights in Go's output struct order.
    payload["Stats"] = serde_json::json!({
        "OutDegree": p1.out_degree,
        "InDegree": p1.in_degree,
        "TopologicalOrder": p1.topological_order,
        "Density": p1.density,
        "NodeCount": p1.node_count,
        "EdgeCount": p1.edge_count,
        "Config": insights_analysis_config(g.len()),
    });

    // full_stats — exactly Go's 9 fields (mapLimit=200 default).
    let map_limit = insights_map_limit();
    let mut fs = serde_json::Map::new();
    fs.insert(
        "pagerank".into(),
        serde_json::Value::Object(limit_metric_map(pr_obj, map_limit)),
    );
    fs.insert(
        "betweenness".into(),
        serde_json::Value::Object(limit_metric_map(bw_obj, map_limit)),
    );
    fs.insert(
        "eigenvector".into(),
        serde_json::Value::Object(limit_metric_map(ev_obj, map_limit)),
    );
    fs.insert(
        "hubs".into(),
        serde_json::Value::Object(limit_metric_map(hub_obj, map_limit)),
    );
    fs.insert(
        "authorities".into(),
        serde_json::Value::Object(limit_metric_map(auth_obj, map_limit)),
    );
    fs.insert(
        "critical_path_score".into(),
        serde_json::Value::Object(limit_metric_map(cp_obj, map_limit)),
    );
    fs.insert(
        "core_number".into(),
        serde_json::Value::Object(limit_metric_map(core_obj, map_limit)),
    );
    fs.insert(
        "slack".into(),
        serde_json::Value::Object(limit_metric_map(slack_obj, map_limit)),
    );
    // Go's `limitSlice` (robot_registry.go:1932-1939) caps this list at the
    // same mapLimit as the maps: `in[:limit]`, no sorting.
    let art_limited: Vec<&String> = art_ids.iter().take(map_limit).collect();
    fs.insert("articulation_points".into(), serde_json::json!(art_limited));
    payload["full_stats"] = serde_json::Value::Object(fs);

    // top_what_ifs — Go TopWhatIfDeltas (bv-83): exact delta semantics.
    let whatif_entries = compute_top_what_if_deltas(&issues, &g, &cp_heights);
    if !whatif_entries.is_empty() {
        payload["top_what_ifs"] = serde_json::Value::Array(whatif_entries);
    }

    // advanced_insights — Go GenerateAdvancedInsights parity (bv-145/152/153/154).
    payload["advanced_insights"] = generate_advanced_insights(&issues, &cycles_from_phase2);

    payload["usage_hints"] = serde_json::json!([
        "jq '.Bottlenecks[:5] | map(.ID)' - Top 5 bottleneck IDs",
        "jq '.Keystones[:3]' - Top 3 critical path scores",
        "jq '.top_what_ifs[] | select(.delta.direct_unblocks > 2)' - High-impact items",
        "jq '.full_stats.pagerank | to_entries | sort_by(-.value)[:5]' - Top PageRank",
        "jq '.full_stats.core_number | to_entries | sort_by(-.value)[:5]' - Strongly embedded nodes (k-core)",
        "jq '.full_stats.articulation_points' - Structural cut points",
        "jq '.Slack[:5]' - Nodes with slack (good parallel work candidates)",
        "jq '.Cycles | length' - Count of detected cycles",
        "jq '.advanced_insights.cycle_break' - Cycle break suggestions (bv-181)",
        "BV_INSIGHTS_MAP_LIMIT=50 bv --robot-insights - Reduce map sizes",
    ]);

    emit_json(&payload)
}

/// Go `TopWhatIfDeltas` (priority.go bv-83) — per-issue what-if deltas with
/// exact Go semantics: direct unblocks (newly-actionable dependents),
/// BFS-simulated transitive unblocks, depth reduction (cpValue/10 capped
/// at 1), days saved (Σ estimates / 480), parallel gain = direct − 1.
fn compute_top_what_if_deltas(
    issues: &[bv_core::model::Issue],
    g: &bv_graph_core::DiGraph,
    cp_heights: &[f64],
) -> Vec<serde_json::Value> {
    use bv_core::model::Status;
    const MAX_CRITICAL_PATH_DEPTH: f64 = 10.0;
    const MAX_UNBLOCKED_IDS_SHOWN: usize = 10;

    let is_closed = |id: &str| -> bool {
        issues
            .iter()
            .find(|i| i.id == id)
            .map(|i| matches!(i.status, Status::Closed | Status::Tombstone))
            .unwrap_or(true)
    };

    // Go computeUnblocks: dependents that are open and have no OTHER open blocker.
    let compute_unblocks = |issue_id: &str| -> Vec<String> {
        let mut unblocks = Vec::new();
        let Some(node) = g.node_idx(issue_id) else {
            return unblocks;
        };
        for &dep_node in g.predecessors_slice(node) {
            let dep_id = g.node_id(dep_node).unwrap_or_default();
            if is_closed(&dep_id) {
                continue;
            }
            let mut still_blocked = false;
            for &other in g.successors_slice(dep_node) {
                let other_id = g.node_id(other).unwrap_or_default();
                if other_id == issue_id {
                    continue;
                }
                if !is_closed(&other_id) {
                    still_blocked = true;
                    break;
                }
            }
            if !still_blocked {
                unblocks.push(dep_id.to_string());
            }
        }
        unblocks.sort();
        unblocks
    };

    // Go countTransitiveUnblocks: BFS with simulated-closed set.
    let count_transitive = |issue_id: &str| -> usize {
        let mut simulated: std::collections::HashSet<String> = std::collections::HashSet::new();
        simulated.insert(issue_id.to_string());
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(issue_id.to_string());
        let mut count = 0usize;
        while let Some(curr) = queue.pop_front() {
            let Some(node) = g.node_idx(curr.as_str()) else {
                continue;
            };
            for &dep_node in g.predecessors_slice(node) {
                let dep_id = g.node_id(dep_node).unwrap_or_default().to_string();
                if simulated.contains(&dep_id) || is_closed(&dep_id) {
                    continue;
                }
                // Unblocked if ALL blockers are (real or simulated) closed.
                let mut blocked = false;
                for &blocker_node in g.successors_slice(dep_node) {
                    let blocker_id = g.node_id(blocker_node).unwrap_or_default().to_string();
                    let blocker_closed = simulated.contains(&blocker_id) || is_closed(&blocker_id);
                    if !blocker_closed {
                        blocked = true;
                        break;
                    }
                }
                if !blocked {
                    simulated.insert(dep_id.clone());
                    queue.push_back(dep_id);
                    count += 1;
                }
            }
        }
        count
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    for issue in issues {
        if matches!(issue.status, Status::Closed | Status::Tombstone) {
            continue;
        }
        let direct_list = compute_unblocks(&issue.id);
        let direct = direct_list.len() as i64;
        let transitive = count_transitive(&issue.id) as i64;
        if direct == 0 && transitive == 0 {
            continue;
        }
        // Go counts unblocked issues whose dependency state is not satisfied
        // (priority.go:963-970). Checking `status == blocked` counted only
        // issues parked in that status; an open issue still held up by an open
        // blocker also counts, which is the common case on a chain.
        let blocked_reduction = direct_list
            .iter()
            .filter(|id| {
                issues.iter().any(|i| {
                    i.id == **id
                        && i.dependencies.iter().any(|d| {
                            d.r#type.is_blocking()
                                && !d.effective_depends_on().is_empty()
                                && issues.iter().any(|o| {
                                    o.id == d.effective_depends_on()
                                        && !matches!(o.status, Status::Closed | Status::Tombstone)
                                })
                        })
                })
            })
            .count() as i64;
        let cp_node = g.node_idx(issue.id.as_str()).unwrap_or(usize::MAX);
        let current_depth = cp_heights.get(cp_node).copied().unwrap_or(0.0);
        let mut depth_reduction = 0.0f64;
        if current_depth > 0.0 {
            depth_reduction = (current_depth / MAX_CRITICAL_PATH_DEPTH).min(1.0);
        }
        let estimated_days_saved: f64 = direct_list
            .iter()
            .map(|id| {
                issues
                    .iter()
                    .find(|i| i.id == *id)
                    .map(|i| i.estimated_minutes.unwrap_or(60) as f64)
                    .unwrap_or(60.0)
            })
            .sum::<f64>()
            / 480.0;
        let unblocked_ids: Vec<String> = direct_list
            .iter()
            .take(MAX_UNBLOCKED_IDS_SHOWN)
            .cloned()
            .collect();
        let parallel_gain = direct - 1;

        // Go generateWhatIfExplanation.
        let explanation = if direct == 0 {
            "No immediate downstream impact".to_string()
        } else {
            let mut e = format!("Completing this directly unblocks {direct} item");
            if direct != 1 {
                e.push('s');
            }
            if transitive > direct {
                e.push_str(&format!(" ({transitive} total including cascades)"));
            }
            if blocked_reduction > 0 {
                e.push_str(&format!(", clears {blocked_reduction} blocked"));
            }
            if estimated_days_saved >= 0.5 {
                e.push_str(&format!(
                    ", enabling ~{estimated_days_saved:.1} days of work"
                ));
            }
            e
        };

        results.push(serde_json::json!({
            "issue_id": issue.id,
            "title": issue.title,
            "delta": {
                "direct_unblocks": direct,
                "transitive_unblocks": transitive,
                "blocked_reduction": blocked_reduction,
                "depth_reduction": depth_reduction,
                "estimated_days_saved": estimated_days_saved,
                "unblocked_issue_ids": unblocked_ids,
                "parallelization_gain": parallel_gain,
                "explanation": explanation,
            },
        }));
    }

    // Sort: transitive desc, direct desc, ID asc; cap at 10.
    results.sort_by(|a, b| {
        let at = a["delta"]["transitive_unblocks"].as_i64().unwrap_or(0);
        let bt = b["delta"]["transitive_unblocks"].as_i64().unwrap_or(0);
        let ad = a["delta"]["direct_unblocks"].as_i64().unwrap_or(0);
        let bd = b["delta"]["direct_unblocks"].as_i64().unwrap_or(0);
        bt.cmp(&at)
            .then_with(|| bd.cmp(&ad))
            .then_with(|| a["issue_id"].as_str().cmp(&b["issue_id"].as_str()))
    });
    results.truncate(10);
    results
}

/// Go `FeatureStatus` JSON with omitempty semantics:
/// state always; reason/capped/count/limited omitted when zero-valued.
fn feature_status(
    state: &str,
    reason: &str,
    capped: bool,
    count: i64,
    limited: i64,
) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("state".into(), serde_json::json!(state));
    if !reason.is_empty() {
        m.insert("reason".into(), serde_json::json!(reason));
    }
    if capped {
        m.insert("capped".into(), serde_json::json!(true));
    }
    if count != 0 {
        m.insert("count".into(), serde_json::json!(count));
    }
    if limited != 0 {
        m.insert("limited".into(), serde_json::json!(limited));
    }
    serde_json::Value::Object(m)
}

/// Go `AdvancedInsights` port (advanced_insights.go) — greedy top-k set,
/// coverage set, k-paths, parallel cut, pending parallel gain, cycle break.
fn generate_advanced_insights(
    issues: &[bv_core::model::Issue],
    cycles: &[Vec<String>],
) -> serde_json::Value {
    use bv_core::model::Issue;
    let is_open = |i: &Issue| {
        !matches!(
            i.status,
            bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
        )
    };
    let title_of = |id: &str| -> String {
        issues
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.title.clone())
            .unwrap_or_default()
    };

    // ---- TopK Set (greedy submodular, k=5) — Go generateTopKSet ----
    let mut candidates: Vec<String> = issues
        .iter()
        .filter(|i| is_open(i))
        .map(|i| i.id.clone())
        .collect();
    candidates.sort();
    let mut topk_items: Vec<serde_json::Value> = Vec::new();
    let mut marginal_gains: Vec<i64> = Vec::new();
    let mut total_gain = 0i64;
    let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let by_id: std::collections::HashMap<&str, &Issue> =
            issues.iter().map(|i| (i.id.as_str(), i)).collect();
        let k = 5usize;
        for _ in 0..k {
            // Go re-derives `actionable` each step via
            // getActionableIssuesAfterCompletions(completed) and picks only
            // from that set (advanced_insights.go:492). Rust reused the static
            // open-issue list, so a blocked high-fanout node could be selected
            // ahead of its own prerequisite — which is exactly what "work these
            // in order" must never do.
            let blockers_resolved = |id: &str| -> bool {
                issues
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| {
                        i.dependencies
                            .iter()
                            .filter(|d| d.r#type.is_blocking())
                            .all(|d| {
                                let t = d.effective_depends_on();
                                completed.contains(t)
                                    || !issues.iter().any(|x| x.id == t && is_open(x))
                            })
                    })
                    .unwrap_or(false)
            };
            let mut remaining: Vec<String> = candidates
                .iter()
                .filter(|id| !completed.contains(*id) && blockers_resolved(id))
                .cloned()
                .collect();
            if remaining.is_empty() {
                break;
            }
            let mut best_id = String::new();
            let mut best_gain: i64 = -1;
            let mut best_unblocks: Vec<String> = Vec::new();
            for cand in &remaining {
                // Go computeMarginalUnblocks: direct newly-actionable count.
                let mut unblocks: Vec<String> = Vec::new();
                for issue in issues {
                    // Go excludes ids in `before` — the currently-actionable
                    // set — since an already-ready issue is not a new unlock
                    // (advanced_insights.go:582).
                    if !is_open(issue)
                        || completed.contains(&issue.id)
                        || remaining.contains(&issue.id)
                        || issue.id == *cand
                    {
                        continue;
                    }
                    let mut has_this_blocker = false;
                    let mut would_be_blocked = false;
                    for dep in &issue.dependencies {
                        if !dep.r#type.is_blocking() {
                            continue;
                        }
                        let b = dep.effective_depends_on();
                        if b == cand.as_str() {
                            has_this_blocker = true;
                            continue;
                        }
                        let blocker_closed = by_id.get(b).map(|bi| !is_open(bi)).unwrap_or(true);
                        let blocker_completed = completed.contains(b);
                        if !blocker_closed && !blocker_completed {
                            would_be_blocked = true;
                        }
                    }
                    if has_this_blocker && !would_be_blocked {
                        unblocks.push(issue.id.clone());
                    }
                }
                unblocks.sort();
                let gain = unblocks.len() as i64;
                if gain > best_gain
                    || (gain == best_gain && (best_id.is_empty() || cand < &best_id))
                {
                    best_id = cand.clone();
                    best_gain = gain;
                    best_unblocks = unblocks;
                }
            }
            if best_id.is_empty() {
                break;
            }
            completed.insert(best_id.clone());
            remaining.retain(|r| r != &best_id);
            // Go's TopKSetItem.Unblocks is `omitempty`
            // (advanced_insights.go:146), so an item that newly unblocks
            // nothing omits the key rather than emitting an empty array.
            let mut item = serde_json::json!({
                "id": best_id,
                "title": title_of(&best_id),
                "marginal_gain": best_gain,
            });
            if !best_unblocks.is_empty() {
                item["unblocks"] = serde_json::json!(best_unblocks);
            }
            topk_items.push(item);
            marginal_gains.push(best_gain);
            total_gain += best_gain;
        }
    }
    let topk_limited = candidates.len() as i64;
    let topk = serde_json::json!({
        "status": feature_status(
            "available",
            "",
            topk_items.len() as i64 >= 5 && topk_limited > 5,
            topk_items.len() as i64,
            topk_limited,
        ),
        "items": topk_items,
        "total_gain": total_gain,
        "marginal_gain": marginal_gains,
        "how_to_use": "Best k issues to complete for max downstream unlock. Work these in order.",
    });

    // ---- Coverage Set (greedy vertex cover, limit 5) — Go generateCoverageSet ----
    let mut edges: Vec<(String, String)> = Vec::new();
    let by_id: std::collections::HashMap<&str, &Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();
    for issue in issues {
        if !is_open(issue) {
            continue;
        }
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let target = dep.effective_depends_on();
            if let Some(t) = by_id.get(target) {
                if is_open(t) {
                    edges.push((issue.id.clone(), target.to_string()));
                }
            }
        }
    }
    let total_edges = edges.len() as i64;
    let coverage = if total_edges == 0 {
        serde_json::json!({
            "status": feature_status("available", "No blocking edges to cover", false, 0, 0),
            "edges_covered": 0,
            "total_edges": 0,
            "coverage_ratio": 1.0,
            "rationale": "Graph has no blocking dependencies.",
            "how_to_use": "Greedy dependency-edge coverage. Check coverage_ratio and capped before treating it as complete.",
        })
    } else {
        let mut uncovered: Vec<(String, String)> = edges.clone();
        let mut cov_items: Vec<serde_json::Value> = Vec::new();
        let mut edges_covered = 0i64;
        let mut selection = 0i64;
        while !uncovered.is_empty() && cov_items.len() < 5 {
            let mut deg: std::collections::BTreeMap<String, i64> =
                std::collections::BTreeMap::new();
            for (f, t) in &uncovered {
                *deg.entry(f.clone()).or_insert(0) += 1;
                *deg.entry(t.clone()).or_insert(0) += 1;
            }
            let mut best_id = String::new();
            let mut best_deg = -1i64;
            for (id, d) in &deg {
                if *d > best_deg || (*d == best_deg && (best_id.is_empty() || id < &best_id)) {
                    best_id = id.clone();
                    best_deg = *d;
                }
            }
            if best_id.is_empty() {
                break;
            }
            let mut added = 0i64;
            uncovered.retain(|(f, t)| {
                if f == &best_id || t == &best_id {
                    added += 1;
                    false
                } else {
                    true
                }
            });
            edges_covered += added;
            selection += 1;
            cov_items.push(serde_json::json!({
                "id": best_id,
                "title": title_of(&best_id),
                "edges_added": added,
                "total_degree": best_deg,
                "selection_seq": selection,
            }));
        }
        let capped = !uncovered.is_empty();
        serde_json::json!({
            "status": feature_status("available", "", capped, cov_items.len() as i64, total_edges),
            "items": cov_items,
            "edges_covered": edges_covered,
            "total_edges": total_edges,
            "coverage_ratio": edges_covered as f64 / total_edges as f64,
            "rationale": "Greedy vertex-cover heuristic: iteratively pick highest uncovered degree until edges are covered or cap is reached.",
            "how_to_use": "Greedy dependency-edge coverage. Check coverage_ratio and capped before treating it as complete.",
        })
    };

    // ---- K-Paths (k=5, cap=50) — Go generateKPaths (longest-path DP) ----
    // Nodes: open issues sorted by ID; edges blocker -> blocked among open.
    let mut nodes: Vec<String> = issues
        .iter()
        .filter(|i| is_open(i))
        .map(|i| i.id.clone())
        .collect();
    nodes.sort();
    let idx: std::collections::HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let n = nodes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_deg = vec![0usize; n];
    for issue in issues {
        if !is_open(issue) {
            continue;
        }
        let to = idx[issue.id.as_str()];
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            if let Some(&from) = idx.get(dep.effective_depends_on()) {
                adj[from].push(to);
                in_deg[to] += 1;
            }
        }
    }
    for a in &mut adj {
        a.sort();
    }
    // Kahn topo with min-heap (BTreeSet for determinism).
    let mut topo: Vec<usize> = Vec::new();
    let mut pq: std::collections::BTreeSet<usize> = (0..n).filter(|&i| in_deg[i] == 0).collect();
    let mut temp_deg = in_deg.clone();
    while let Some(&u) = pq.iter().next() {
        pq.remove(&u);
        topo.push(u);
        for &v in &adj[u] {
            temp_deg[v] -= 1;
            if temp_deg[v] == 0 {
                pq.insert(v);
            }
        }
    }
    let mut dist = vec![0i64; n];
    let mut pred = vec![-1i64; n];
    for &u in &topo {
        for &v in &adj[u] {
            if dist[u] + 1 > dist[v] {
                dist[v] = dist[u] + 1;
                pred[v] = u as i64;
            } else if dist[u] + 1 == dist[v] && (pred[v] == -1 || (u as i64) < pred[v]) {
                pred[v] = u as i64;
            }
        }
    }
    let mut path_ends: Vec<(usize, i64)> = (0..n).map(|i| (i, dist[i])).collect();
    path_ends.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| nodes[a.0].cmp(&nodes[b.0])));
    let mut paths: Vec<serde_json::Value> = Vec::new();
    let mut used_sources: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut total_paths = 0i64;
    // Go counts `representativeCount` as the distinct SOURCE nodes that have
    // at least one non-trivial path (advanced_insights.go:929-937), over every
    // candidate — not just the ones that survive into `paths`. Counting only
    // the emitted paths reported 5 where Go reports 391, which understated how
    // many sources were actually omitted.
    let mut representative_sources: std::collections::HashSet<usize> =
        std::collections::HashSet::new();
    for &(i, len) in &path_ends {
        if len == 0 {
            continue;
        }
        total_paths += 1;
        // Walk to the root the same way the emit loop does, to find the source.
        let mut head = i as i64;
        while pred[head as usize] != -1 {
            head = pred[head as usize];
        }
        representative_sources.insert(head as usize);
    }
    for &(i, len) in &path_ends {
        if paths.len() >= 5 {
            break;
        }
        if len == 0 {
            continue;
        }
        let mut chain = Vec::new();
        let mut cur = i as i64;
        while cur != -1 {
            chain.push(cur as usize);
            cur = pred[cur as usize];
        }
        chain.reverse();
        let source = chain[0];
        if !used_sources.insert(source) {
            continue;
        }
        let mut truncated = false;
        if chain.len() > 50 {
            chain.truncate(50);
            truncated = true;
        }
        let ids: Vec<String> = chain.iter().map(|&x| nodes[x].clone()).collect();
        let mut p = serde_json::json!({
            "rank": paths.len() + 1,
            "length": ids.len(),
            "issue_ids": ids,
        });
        if truncated {
            p["truncated"] = serde_json::json!(true);
        }
        paths.push(p);
    }
    let mut k_paths = serde_json::json!({
        // Go's KPathsResult.Limited is the number of representative sources
        // considered (advanced_insights.go:943), not the total path count.
        "status": feature_status(
            "available",
            "",
            paths.len() >= 5 && total_paths > 5,
            paths.len() as i64,
            representative_sources.len() as i64,
        ),
        "how_to_use": "Representative longest critical paths. Focus on issues appearing in multiple paths.",
    });
    // Go's KPathsResult.Paths is `omitempty` (advanced_insights.go:173), so a
    // run that found no path omits the key rather than emitting [].
    if !paths.is_empty() {
        k_paths["paths"] = serde_json::json!(paths);
    }

    // ---- Parallel Cut (limit 5) — Go generateParallelCut ----
    let open_set: std::collections::HashSet<&str> = issues
        .iter()
        .filter(|i| is_open(i))
        .map(|i| i.id.as_str())
        .collect();
    let mut blocker_of: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    let mut blocked_by: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for issue in issues {
        if !is_open(issue) {
            continue;
        }
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let b = dep.effective_depends_on();
            if open_set.contains(b) {
                blocker_of.entry(b).or_default().push(issue.id.as_str());
                blocked_by.entry(issue.id.as_str()).or_default().push(b);
            }
        }
    }
    // (The actionable count itself is no longer needed here: the suggestion
    // loop below re-derives per-candidate readiness, and max_parallel is now
    // projected from the completed set rather than from this base count.)
    let mut pc_candidates: Vec<(String, i64, i64, Vec<String>)> = Vec::new();
    // Go iterates `actionable`, not every open issue (advanced_insights.go:983):
    // the suggestion is "complete this to widen parallel work", which only
    // makes sense for something that can be completed now. Iterating all open
    // issues admits a candidate at every depth of the graph.
    let actionable_ids: Vec<&str> = open_set
        .iter()
        .copied()
        .filter(|id| blocked_by.get(*id).is_none_or(|v| v.is_empty()))
        .collect();
    for id in actionable_ids {
        let mut newly: Vec<String> = Vec::new();
        if let Some(dependents) = blocker_of.get(id) {
            for &dep_id in dependents {
                let all_others_resolved = blocked_by
                    .get(dep_id)
                    .map(|blockers| blockers.iter().all(|b| **b == *id || !open_set.contains(b)))
                    .unwrap_or(true);
                // Go requires `!before[id]` (advanced_insights.go:582): an issue
                // that is already actionable is not a *new* unlock, so it must
                // not raise the gain.
                let already_actionable = blocked_by
                    .get(dep_id)
                    .map(|blockers| blockers.is_empty())
                    .unwrap_or(true);
                if all_others_resolved && !already_actionable {
                    newly.push(dep_id.to_string());
                }
            }
        }
        let gain = newly.len() as i64 - 1;
        if gain > 0 {
            newly.sort();
            pc_candidates.push((id.to_string(), gain, newly.len() as i64, newly));
        }
    }
    pc_candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let pc_total = pc_candidates.len();
    pc_candidates.truncate(5);
    // Go projects the ready width after completing the returned cut as a SET
    // (advanced_insights.go:1037): `len(getActionableIssuesAfterCompletions(completedCut))`.
    // Rust added only the first suggestion's gain, which under-counts whenever
    // more than one suggestion is returned (1859 vs 1872 on xl_2500).
    let completed_cut: std::collections::HashSet<&str> = pc_candidates
        .iter()
        .map(|(id, _, _, _)| id.as_str())
        .collect();
    let max_parallel = open_set
        .iter()
        .filter(|id| {
            // A completed issue is no longer a candidate for future work, so
            // it drops out of the actionable set.
            if completed_cut.contains(*id) {
                return false;
            }
            blocked_by.get(*id).is_none_or(|blockers| {
                blockers
                    .iter()
                    .all(|b| completed_cut.contains(b) || !open_set.contains(b))
            })
        })
        .count() as i64;
    // Go emits the cut suggestions themselves (advanced_insights.go:186-199);
    // Rust computed the candidates and then dropped them.
    let pc_suggestions: Vec<serde_json::Value> = pc_candidates
        .iter()
        .map(|(id, gain, _, tracks)| {
            serde_json::json!({
                "id": id,
                "title": title_of(id),
                "parallel_gain": gain,
                "enabled_tracks": tracks,
            })
        })
        .collect();
    // Go's FeatureStatus.Count/Limited are omitempty, so an empty cut emits a
    // bare {"state":"available"} and omits `suggestions` entirely.
    let mut pc_status = serde_json::json!({"state": "available"});
    if !pc_suggestions.is_empty() {
        pc_status["count"] = serde_json::json!(pc_suggestions.len());
        pc_status["limited"] = serde_json::json!(pc_total);
    }
    // Go's FeatureStatus.Capped marks that the result was truncated at the
    // limit, distinct from `limited` which is the pre-cap count.
    if pc_total > pc_suggestions.len() {
        pc_status["capped"] = serde_json::json!(true);
    }
    let mut parallel_cut = serde_json::json!({
        "status": pc_status,
        "max_parallel": max_parallel,
        "how_to_use": "Issues that enable parallel work. Complete to maximize team throughput.",
    });
    if !pc_suggestions.is_empty() {
        parallel_cut["suggestions"] = serde_json::json!(pc_suggestions);
    }

    // ---- Parallel Gain — Go generateParallelGain (advanced_insights.go:1063) ----
    let parallel_gain = compute_parallel_gain(issues, 5);

    // ---- Cycle Break — Go generateCycleBreakSuggestions ----
    //
    // Go distinguishes "no cycles" from "cycles were never computed".
    // `ConfigForSize` disables cycle detection above the XL threshold
    // (config.go:190-197, CyclesSkipReason "graph too large (>2000 nodes)"),
    // so on a large graph an empty cycle list means *unknown*, not *acyclic*.
    // Rust reported "No cycles detected - the dependency graph is a proper
    // DAG" there, which is the one conclusion the data does not support.
    let cycles_skipped = bv_analysis::analyzer::AnalysisBudget::default().skip_cycles(issues.len());
    let cycle_break = if cycles_skipped {
        serde_json::json!({
            "status": feature_status(
                "skipped",
                "cycle detection skipped: graph too large (>2000 nodes)",
                false,
                0,
                0,
            ),
            "cycle_count": 0,
            "how_to_use": "Structural fix suggestions. Apply BEFORE working on cycle members.",
            "advisory": "Cycle analysis is unavailable; do not infer that the dependency graph is acyclic.",
        })
    } else if cycles.is_empty() {
        serde_json::json!({
            "status": feature_status("available", "", false, 0, 0),
            "cycle_count": 0,
            "how_to_use": "Structural fix suggestions. Apply BEFORE working on cycle members.",
            "advisory": "No cycles detected - dependency graph is a proper DAG.",
        })
    } else {
        let mut edge_freq: std::collections::BTreeMap<(String, String), Vec<i64>> =
            std::collections::BTreeMap::new();
        for (ci, cycle) in cycles.iter().enumerate() {
            if cycle.len() < 2 || cycle[0] == "CYCLE_DETECTION_TIMEOUT" || cycle[0] == "..." {
                continue;
            }
            for j in 0..cycle.len() - 1 {
                edge_freq
                    .entry((cycle[j].clone(), cycle[j + 1].clone()))
                    .or_default()
                    .push(ci as i64);
            }
            edge_freq
                .entry((cycle[cycle.len() - 1].clone(), cycle[0].clone()))
                .or_default()
                .push(ci as i64);
        }
        let mut ranked: Vec<(&(String, String), &Vec<i64>)> = edge_freq.iter().collect();
        ranked.sort_by(|a, b| {
            b.1.len()
                .cmp(&a.1.len())
                .then_with(|| a.0 .0.cmp(&b.0 .0))
                .then_with(|| a.0 .1.cmp(&b.0 .1))
        });
        let mut suggestions: Vec<serde_json::Value> = Vec::new();
        for ((from, to), cycs) in ranked.iter().take(5) {
            let collateral = issues
                .iter()
                .filter(|i| {
                    i.dependencies
                        .iter()
                        .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == to.as_str())
                })
                .count();
            suggestions.push(serde_json::json!({
                "edge_from": from,
                "edge_to": to,
                "impact": cycs.len(),
                "collateral": collateral,
                "in_cycles": cycs,
                "rationale": "Appears in most cycles; removing minimizes structural damage.",
            }));
        }
        let capped = ranked.len() > 5;
        serde_json::json!({
            "status": feature_status("available", "", capped, suggestions.len() as i64, ranked.len() as i64),
            "suggestions": suggestions,
            "cycle_count": cycles.len(),
            "how_to_use": "Structural fix suggestions. Apply BEFORE working on cycle members.",
            "advisory": "",
        })
    };

    serde_json::json!({
        "topk_set": topk,
        "coverage_set": coverage,
        "k_paths": k_paths,
        "parallel_cut": parallel_cut,
        "parallel_gain": parallel_gain,
        "cycle_break": cycle_break,
        "config": {
            "topk_set_limit": 5,
            "coverage_set_limit": 5,
            "k_paths_limit": 5,
            "path_length_cap": 50,
            "cycle_break_limit": 5,
            "parallel_cut_limit": 5, "parallel_gain_limit": 5,
        },
        // Go map keys serialize sorted alphabetically.
        "usage_hints": {
            "coverage_set": "Greedy dependency-edge coverage. Check coverage_ratio and capped before treating it as complete.",
            "cycle_break": "Structural fix suggestions. Apply BEFORE working on cycle members.",
            "k_paths": "Representative longest critical paths. Focus on issues appearing in multiple paths.",
            "parallel_cut": "Issues that enable parallel work. Complete to maximize team throughput.",
            "parallel_gain": "Independent work tracks gained by closing each actionable issue now (gain = tracks after - tracks now). Pick high-gain issues to widen parallel work; unblocks lists what opens up.",
            "topk_set": "Best k issues to complete for max downstream unlock. Work these in order.",
        },
    })
}

/// Go `AnalysisConfig` JSON shape (exported Go field names, ns timeouts).
/// `--robot-plan` variant: KCore/Articulation/Slack only, everything else
/// skipped with "not computed for --robot-plan".
fn plan_analysis_config(nodes: usize) -> serde_json::Value {
    // Go ConfigForSize timeout tiers (ns) — golden-verified per fixture size.
    let (bt_ns, pr_ns, cycles_ns, max_cycles, sample) = match nodes {
        n if n < 100 => (
            2_000_000_000i64,
            2_000_000_000i64,
            2_000_000_000i64,
            1000,
            0,
        ),
        n if n < 500 => (500_000_000i64, 500_000_000i64, 500_000_000i64, 100, 0),
        n if n < 2000 => (500_000_000i64, 300_000_000i64, 300_000_000i64, 50, 100),
        _ => (500_000_000i64, 200_000_000i64, 0i64, 10, 200),
    };
    serde_json::json!({
        "ComputeBetweenness": false,
        "BetweennessTimeout": bt_ns,
        "BetweennessSkipReason": "not computed for --robot-plan",
        "BetweennessMode": "skip",
        "BetweennessSampleSize": sample,
        "BetweennessIsApproximate": false,
        "ComputePageRank": false,
        "PageRankTimeout": pr_ns,
        "PageRankSkipReason": "not computed for --robot-plan",
        "ComputeHITS": false,
        "HITSTimeout": pr_ns,
        "HITSSkipReason": "not computed for --robot-plan",
        "ComputeCycles": false,
        "CyclesTimeout": cycles_ns,
        "MaxCyclesToStore": max_cycles,
        "CyclesSkipReason": "not computed for --robot-plan",
        "ComputeEigenvector": false,
        "ComputeCriticalPath": false,
        "ComputeKCore": true,
        "ComputeArticulation": true,
        "ComputeSlack": true,
    })
}

/// Go `AnalysisConfig` for `--robot-priority`: full Phase-2 config.
fn priority_analysis_config(nodes: usize) -> serde_json::Value {
    // Go's `ConfigForSize` is the single source of truth for these values
    // (pkg/analysis/config.go:98). The hand-rolled tier table this replaced
    // hard-coded `BetweennessMode: "exact"` and `BetweennessSampleSize: 0` for
    // every size, so `--robot-priority` reported the wrong analysis shape on
    // any graph large enough for Go to sample.
    serde_json::to_value(bv_analysis::analyzer::config_for_size(nodes, 0, 0.0)).unwrap_or_default()
}

/// Status map for plan/priority (golden-verified): only KCore, Articulation
/// and Slack run; the rest are skipped with the plan-specific reason.
/// Priority's Go handler reports the identical status shape.
fn plan_priority_status(g: &bv_graph_core::DiGraph) -> serde_json::Value {
    let t0 = std::time::Instant::now();
    let kcore = bv_analysis::algorithms::kcore::kcore(g);
    let kcore_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = std::time::Instant::now();
    let articulation = bv_analysis::algorithms::articulation::articulation_points(g);
    let art_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let t2 = std::time::Instant::now();
    let slack = bv_analysis::algorithms::slack::slack(g);
    let slack_ms = t2.elapsed().as_secs_f64() * 1000.0;
    let _ = (kcore, articulation, slack);

    let plan_skip = |reason: &str| bv_analysis::analyzer::StatusEntry::skipped(reason);
    serde_json::json!({
        "PageRank": plan_skip(""),
        "Betweenness": plan_skip("not computed for --robot-plan"),
        "Eigenvector": plan_skip(""),
        "HITS": plan_skip("not computed for --robot-plan"),
        "Critical": plan_skip(""),
        "Cycles": plan_skip("not computed for --robot-plan"),
        "KCore": bv_analysis::analyzer::StatusEntry::computed(kcore_ms),
        "Articulation": bv_analysis::analyzer::StatusEntry::computed(art_ms),
        "Slack": bv_analysis::analyzer::StatusEntry::computed(slack_ms),
    })
}

/// Go `generateTrackID` — 1-based n to base-26 alphabetic (A, B, ..., Z, AA...).
fn go_track_id(mut n: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn run_robot_plan() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = bv_analysis::analyzer::build_graph(&issues);
    let blocked = bv_analysis::triage::compute_blocked_set(&issues);
    let _actionable: Vec<&bv_core::model::Issue> = issues
        .iter()
        .filter(|i| i.status.is_open() && !blocked.contains(&i.id))
        .collect();

    // Go GetExecutionPlan parity: union-find over ALL issues (blocking +
    // parent-child deps), deterministic merge (smaller root wins), tracks
    // filtered to actionable members, unblocks = newly-actionable dependents.
    let by_id: std::collections::HashMap<&str, &bv_core::model::Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();

    let mut parent: std::collections::HashMap<String, String> = issues
        .iter()
        .map(|i| (i.id.clone(), i.id.clone()))
        .collect();
    fn find(parent: &std::collections::HashMap<String, String>, x: &str) -> String {
        let mut root = x.to_string();
        while parent[&root] != root {
            root = parent[&root].clone();
        }
        root
    }
    let mut sorted_ids: Vec<&str> = issues.iter().map(|i| i.id.as_str()).collect();
    sorted_ids.sort();
    for id in &sorted_ids {
        let issue = by_id[*id];
        for dep in &issue.dependencies {
            let is_linking = dep.r#type.is_blocking()
                || matches!(dep.r#type, bv_core::model::DependencyType::ParentChild);
            if is_linking && by_id.contains_key(dep.effective_depends_on()) {
                let px = find(&parent, id);
                let py = find(&parent, dep.effective_depends_on());
                if px != py {
                    // Deterministic merge: smaller root wins.
                    if px < py {
                        parent.insert(py, px);
                    } else {
                        parent.insert(px, py);
                    }
                }
            }
        }
    }

    // Go computeUnblocks: dependents that are open and have no OTHER open blocker.
    let is_closed_like = |id: &str| -> bool {
        by_id
            .get(id)
            .map(|i| {
                matches!(
                    i.status,
                    bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
                )
            })
            .unwrap_or(true)
    };
    let compute_unblocks = |issue_id: &str| -> Vec<String> {
        let mut unblocks = Vec::new();
        let Some(node) = g.node_idx(issue_id) else {
            return unblocks;
        };
        for &dep_node in g.predecessors_slice(node) {
            let dep_id = g.node_id(dep_node).unwrap_or_default();
            if is_closed_like(&dep_id) {
                continue;
            }
            let mut still_blocked = false;
            for &other in g.successors_slice(dep_node) {
                let other_id = g.node_id(other).unwrap_or_default();
                if other_id == issue_id {
                    continue;
                }
                if !is_closed_like(&other_id) {
                    still_blocked = true;
                    break;
                }
            }
            if !still_blocked {
                unblocks.push(dep_id);
            }
        }
        unblocks.sort();
        unblocks
    };

    // Components grouped by root (all issues, sorted iteration).
    let mut components: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for id in &sorted_ids {
        let root = find(&parent, id);
        components.entry(root).or_default().push(id.to_string());
    }
    let total_components = components.len();

    // Actionable set: open AND not blocked by open deps (Go actionableSet).
    let blocked = bv_analysis::triage::compute_blocked_set(&issues);
    let actionable: Vec<&bv_core::model::Issue> = issues
        .iter()
        .filter(|i| i.status.is_open() && !blocked.contains(&i.id))
        .collect();
    let actionable_set: std::collections::HashSet<&str> =
        actionable.iter().map(|i| i.id.as_str()).collect();

    // Build tracks: roots sorted; only components with actionable members.
    let mut tracks: Vec<serde_json::Value> = Vec::new();
    let mut track_num = 1usize;
    for members in components.values() {
        let mut actionable_members: Vec<&bv_core::model::Issue> = members
            .iter()
            .filter(|&id| actionable_set.contains(id.as_str()))
            .map(|id| by_id[id.as_str()])
            .collect();
        if actionable_members.is_empty() {
            continue;
        }
        actionable_members
            .sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
        let track_items: Vec<serde_json::Value> = actionable_members
            .iter()
            .map(|i| {
                let unblocks = compute_unblocks(&i.id);
                // Go `PlanItem.UnblocksIDs` (plan.go:15) has no `omitempty`,
                // so an item that unblocks nothing still emits `"unblocks": []`.
                // A nil slice would serialize as null, which the oracle does
                // not produce — verified against bv v0.25.0.
                let unblocks_val = serde_json::json!(unblocks);
                serde_json::json!({
                    "id": i.id, "title": i.title, "priority": i.priority,
                    "status": i.status.as_str(),
                    "unblocks": unblocks_val,
                })
            })
            .collect();
        let reason = if actionable_members.len() == 1 {
            "Single actionable item"
        } else if total_components == 1 {
            "All issues in connected graph"
        } else {
            "Independent work stream"
        };
        let label = go_track_id(track_num);
        tracks.push(serde_json::json!({
            "track_id": format!("track-{label}"),
            "items": track_items,
            "reason": reason,
        }));
        track_num += 1;
    }

    // Summary: actionable sorted by ID; strictly-greater count wins (Go).
    let mut highest_id = String::new();
    let mut highest_count = -1i64;
    let mut sorted_actionable: Vec<&&bv_core::model::Issue> = actionable.iter().collect();
    sorted_actionable.sort_by(|a, b| a.id.cmp(&b.id));
    for issue in &sorted_actionable {
        let count = compute_unblocks(&issue.id).len() as i64;
        if count > highest_count {
            highest_count = count;
            highest_id = issue.id.clone();
        }
    }
    let impact_reason = if highest_count == 1 {
        "Unblocks 1 task"
    } else if highest_count > 1 {
        "Unblocks multiple tasks"
    } else {
        "No downstream dependencies"
    };

    // Go: TotalBlocked = totalOpen (non-closed-like) - len(actionable).
    let total_open = issues
        .iter()
        .filter(|i| {
            !matches!(
                i.status,
                bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
            )
        })
        .count();
    let total_blocked_count = total_open - actionable.len();

    // Go emits the full v0.25.0 envelope ahead of the plan payload
    // (generated_at, data_hash, output_format, version, source_path,
    // source_kind, source_authority, authority_hash, scope_hash), so build it
    // with the shared helper instead of the hand-rolled two-field prefix.
    let mut payload = full_envelope_for(&hash, &issues);
    payload["analysis_config"] = plan_analysis_config(g.len());
    payload["status"] = plan_priority_status(&g);
    payload["plan"] = serde_json::json!({
        "tracks": tracks,
        "total_actionable": actionable.len(),
        "total_blocked": total_blocked_count,
        "summary": {
            "highest_impact": highest_id,
            "impact_reason": impact_reason,
            "unblocks_count": highest_count,
        },
    });
    payload["usage_hints"] = serde_json::json!([
        "jq '.plan.tracks | length' - Number of parallel execution tracks",
        "jq '.plan.tracks[0].items | map(.id)' - First track item IDs",
        "jq '.plan.tracks[].items[] | select(.unblocks | length > 0)' - Items that unblock others",
        "jq '.plan.summary' - High-level execution summary",
        "jq '[.plan.tracks[].items[]] | length' - Total items across all tracks",
    ]);
    emit_json(&payload)
}

/// Go `DefaultThresholds` (pkg/analysis/priority.go:645).
struct PriorityThresholds {
    high_pagerank: f64,
    high_betweenness: f64,
    staleness_days: i64,
    min_confidence: f64,
    significant_delta: f64,
}

impl Default for PriorityThresholds {
    fn default() -> Self {
        Self {
            high_pagerank: 0.3,
            high_betweenness: 0.5,
            staleness_days: 14,
            min_confidence: 0.3,
            significant_delta: 0.15,
        }
    }
}

/// How many issues each issue directly unblocks, keyed by blocker id.
/// Go's `buildUnblocksMap` feeds the unblocks-count signal.
fn build_unblocks_map(
    issues: &[bv_core::model::Issue],
) -> std::collections::BTreeMap<String, usize> {
    let mut map = std::collections::BTreeMap::new();
    for issue in issues {
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let target = dep.effective_depends_on();
            if !target.is_empty() {
                *map.entry(target.to_string()).or_insert(0) += 1;
            }
        }
    }
    map
}

/// Go `generateRecommendation` (priority.go:735-869) plus `calculateConfidence`
/// (priority.go:904). Returns `None` when no signal fires or the derived
/// priority already matches the current one.
#[allow(clippy::too_many_arguments)]
fn build_priority_recommendation(
    r: &bv_analysis::impact::IssueImpact,
    issue: &bv_core::model::Issue,
    unblocks_by_id: &std::collections::BTreeMap<String, usize>,
    th: &PriorityThresholds,
    issues: &[bv_core::model::Issue],
    cp_height: &std::collections::BTreeMap<String, f64>,
    structural: (
        &std::collections::BTreeMap<String, u32>,
        &std::collections::BTreeSet<String>,
        &std::collections::BTreeMap<String, f64>,
        u32,
    ),
) -> Option<serde_json::Value> {
    let (core_map, art_set, slack_map, max_core) = structural;
    let b = &r.breakdown;
    let issue_by_id: std::collections::HashMap<&str, &bv_core::model::Issue> =
        issues.iter().map(|i| (i.id.as_str(), i)).collect();
    // Go's what-if names the issues this one directly unblocks, capped at 10
    // in the output; the full list is needed for the days-saved estimate.
    let unblocks_ids: Vec<String> = issues
        .iter()
        .filter(|o| {
            o.dependencies
                .iter()
                .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
        })
        .map(|o| o.id.clone())
        .collect();
    let unblocks_count = unblocks_ids
        .len()
        .max(unblocks_by_id.get(&r.id).copied().unwrap_or(0));
    // Go counts dependency-blocked work that becomes ready once this issue
    // lands (priority.go:963-970). Without the transitive id set here, the
    // direct unblock count is the available proxy; the transitive cascade is
    // what count_transitive_unblocks computes separately.
    let blocked_reduction = unblocks_count;
    // Go scales the issue's critical-path height (priority.go:974-980), which
    // is not the pagerank norm.
    let depth_reduction = (cp_height.get(&r.id).copied().unwrap_or(0.0) / 10.0).min(1.0);

    let mut reasoning: Vec<String> = Vec::new();
    let mut signals = 0usize;
    let mut signal_strength = 0.0f64;

    if b.pagerank_norm > th.high_pagerank {
        reasoning.push("High centrality in dependency graph".into());
        signals += 1;
        signal_strength += b.pagerank_norm;
    }
    if b.betweenness_norm > th.high_betweenness {
        reasoning.push("Critical path bottleneck".into());
        signals += 1;
        signal_strength += b.betweenness_norm;
    }
    match unblocks_count {
        n if n >= 3 => {
            reasoning.push(format!("Blocks {n} other items"));
            signals += 1;
            signal_strength += 0.5 + n as f64 / 10.0;
        }
        2 => {
            reasoning.push("Blocks 2 other items".into());
            signals += 1;
            signal_strength += 0.3;
        }
        1 => {
            reasoning.push("Blocks 1 other item".into());
            signals += 1;
            signal_strength += 0.2;
        }
        _ => {}
    }
    if b.staleness_norm >= th.staleness_days as f64 / 30.0 {
        reasoning.push(format!(
            "Stale for {}+ days",
            (b.staleness_norm * 30.0) as i64
        ));
        signals += 1;
        signal_strength += 0.2;
    }
    if b.time_to_impact_norm > 0.5 {
        reasoning.push(if b.time_to_impact_explanation.is_empty() {
            "High time-to-impact score".to_string()
        } else {
            b.time_to_impact_explanation.clone()
        });
        signals += 1;
        signal_strength += b.time_to_impact_norm;
    }
    if b.urgency_norm > 0.3 {
        reasoning.push(if b.urgency_explanation.is_empty() {
            "Elevated urgency".to_string()
        } else {
            b.urgency_explanation.clone()
        });
        signals += 1;
        signal_strength += b.urgency_norm;
    }
    if b.risk_norm > 0.4 {
        reasoning.push(if b.risk_explanation.is_empty() {
            "Elevated risk/volatility".to_string()
        } else {
            b.risk_explanation.clone()
        });
        signals += 1;
        signal_strength += b.risk_norm;
    }

    // Structural signals (Go priority.go:809-829).
    if art_set.contains(&r.id) {
        reasoning.push("Articulation point (disconnects graph)".into());
        signals += 1;
        signal_strength += 0.35;
    }
    let core = core_map.get(&r.id).copied().unwrap_or(0);
    if max_core > 0 && core == max_core {
        reasoning.push(format!("High cohesion (k-core {core})"));
        signals += 1;
        signal_strength += 0.3;
    }
    let slack = slack_map.get(&r.id).copied().unwrap_or(0.0);
    if slack == 0.0 {
        reasoning.push("Zero slack on critical chain".into());
        signals += 1;
        signal_strength += 0.25;
    } else if slack > 2.0 {
        // Softer weight so parallel-friendly work does not outweigh bottlenecks.
        reasoning.push("Parallel-friendly (slack available)".into());
        signals += 1;
        signal_strength += 0.15;
    }

    // No signals = no recommendation needed (priority.go:832).
    if signals == 0 {
        return None;
    }

    let suggested = bv_analysis::scoring::score_to_priority(r.score);
    if suggested == issue.priority {
        return None;
    }

    // Go calculateConfidence (priority.go:904-930).
    let mut confidence = (signals as f64 / 10.0).min(1.0);
    confidence += (signal_strength / 2.0).min(0.3);
    let score_delta = (r.score - bv_analysis::scoring::priority_to_score(issue.priority)).abs();
    if score_delta >= th.significant_delta {
        confidence += 0.2;
    }
    confidence = confidence.min(1.0);

    // Go drops anything below the confidence floor (priority.go:712).
    if confidence < th.min_confidence {
        return None;
    }

    // Go caps reasoning at three entries for conciseness (bv-83).
    reasoning.truncate(3);

    let direction = if suggested > issue.priority {
        "decrease"
    } else {
        "increase"
    };

    Some(serde_json::json!({
        "issue_id": r.id,
        "title": r.title,
        "current_priority": issue.priority,
        "suggested_priority": suggested,
        "impact_score": r.score,
        "confidence": confidence,
        "reasoning": reasoning,
        "direction": direction,
        "what_if": what_if_delta(
            &unblocks_ids,
            count_transitive_unblocks(&r.id, issues, &issue_by_id),
            blocked_reduction,
            depth_reduction,
            issues,
        ),
        // Go PriorityExplanation (whatif.go:9-21) nests the same what-if delta
        // plus an inline status block alongside the ranked reasons.
        "explanation": {
            "top_reasons": top_reasons(b),
            "what_if": what_if_delta(
                &unblocks_ids,
                count_transitive_unblocks(&r.id, issues, &issue_by_id),
                blocked_reduction,
                depth_reduction,
                issues,
            ),
            "status": {
                "computed_at": jiff_now(),
                "data_hash": "",
                "phase2_ready": true,
                "deterministic": true,
                "capped": false,
            },
        },
    }))
}

/// Go `GenerateTopReasons` (pkg/analysis/whatif.go:56-107): rank the eight
/// weighted components, keep the top three above a 0.01 contribution, and
/// prefix the blurb by how strong the normalized value is.
fn top_reasons(b: &bv_analysis::impact::Breakdown) -> Vec<serde_json::Value> {
    let factors: [(&str, f64, f64, &str, &str); 8] = [
        (
            "pagerank",
            b.pagerank,
            b.pagerank_norm,
            "Central in dependency graph",
            "🎯",
        ),
        (
            "betweenness",
            b.betweenness,
            b.betweenness_norm,
            "Critical path bottleneck",
            "🔀",
        ),
        (
            "blockers",
            b.blocker_ratio,
            b.blocker_ratio_norm,
            "High blocker count",
            "🚧",
        ),
        (
            "staleness",
            b.staleness,
            b.staleness_norm,
            "Needs attention (aging)",
            "⏰",
        ),
        (
            "priority",
            b.priority_boost,
            b.priority_boost_norm,
            "Explicit priority set",
            "⭐",
        ),
        (
            "time_to_impact",
            b.time_to_impact,
            b.time_to_impact_norm,
            "Fast impact potential",
            "⚡",
        ),
        (
            "urgency",
            b.urgency,
            b.urgency_norm,
            "Urgent labels/timing",
            "🔥",
        ),
        ("risk", b.risk, b.risk_norm, "Risk/volatility factors", "⚠️"),
    ];
    let mut ranked: Vec<&(&str, f64, f64, &str, &str)> = factors.iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut out = Vec::new();
    for f in ranked {
        if out.len() >= 3 {
            break;
        }
        if f.1 < 0.01 {
            break;
        }
        let prefix = if f.2 > 0.7 {
            "Very high: "
        } else if f.2 > 0.4 {
            "High: "
        } else if f.2 > 0.2 {
            "Moderate: "
        } else {
            ""
        };
        out.push(serde_json::json!({
            "factor": f.0,
            "weight": f.1,
            "explanation": format!("{prefix}{}", f.3),
            "emoji": f.4,
        }));
    }
    out
}

/// Go `WhatIfDelta` (priority.go:602-620). Reported from the unblocks count,
/// which drives the parallelization term, and the slack-derived depth
/// reduction.
/// Go `WhatIfDelta` (priority.go:602-620, assembled at 1001-1010).
/// `unblocks_ids` are the direct dependents, capped at MaxUnblockedIDsShown
/// (10), exactly as Go does before populating the field.
fn what_if_delta(
    direct_unblocks: &[String],
    transitive_unblocks: usize,
    blocked_reduction: usize,
    depth_reduction: f64,
    issues: &[bv_core::model::Issue],
) -> serde_json::Value {
    let direct_count = direct_unblocks.len();
    const MAX_UNBLOCKED_IDS_SHOWN: usize = 10;
    let shown: Vec<String> = direct_unblocks
        .iter()
        .take(MAX_UNBLOCKED_IDS_SHOWN)
        .cloned()
        .collect();

    // Go `estimateDaysSaved` (priority.go:1080): sum the estimates of the
    // unblocked issues, falling back to the default estimate, and convert to
    // days.
    let mut total_minutes = 0.0f64;
    let mut counted = 0usize;
    for id in direct_unblocks {
        if let Some(i) = issues.iter().find(|i| i.id == *id) {
            match i.estimated_minutes {
                Some(m) if m > 0 => {
                    total_minutes += m as f64;
                    counted += 1;
                }
                _ => {
                    total_minutes += 60.0; // DefaultEstimatedMinutes
                    counted += 1;
                }
            }
        }
    }
    let estimated_days_saved = if counted == 0 {
        0.0
    } else {
        total_minutes / counted as f64 / 480.0
    };

    let mut delta = serde_json::json!({
        "direct_unblocks": direct_count,
        "transitive_unblocks": transitive_unblocks,
        "blocked_reduction": blocked_reduction,
        "depth_reduction": depth_reduction,
    });
    if estimated_days_saved > 0.0 {
        delta["estimated_days_saved"] = serde_json::json!(estimated_days_saved);
    }
    if !shown.is_empty() {
        delta["unblocked_issue_ids"] = serde_json::json!(shown);
    }
    // Go's parallelization_gain is a pointer and is nil below the top-N; for
    // the recommendations that carry a what-if it is always computed.
    delta["parallelization_gain"] = serde_json::json!(direct_count as i64 - 1);
    delta["explanation"] = serde_json::json!(what_if_explanation(
        direct_count,
        transitive_unblocks,
        blocked_reduction,
        estimated_days_saved
    ));
    delta
}

/// Go `generateWhatIfExplanation` (priority.go:1110-1133).
fn what_if_explanation(
    direct: usize,
    transitive: usize,
    blocked_reduction: usize,
    days_saved: f64,
) -> String {
    if direct == 0 {
        return "No immediate downstream impact".to_string();
    }
    let mut out = format!("Completing this directly unblocks {direct} item");
    if direct != 1 {
        out.push('s');
    }
    if transitive > direct {
        out.push_str(&format!(" ({transitive} total including cascades)"));
    }
    if blocked_reduction > 0 {
        out.push_str(&format!(", clears {blocked_reduction} blocked"));
    }
    if days_saved >= 0.5 {
        out.push_str(&format!(", enabling ~{days_saved:.1} days of work"));
    }
    out
}

/// Go `countTransitiveUnblocks` (priority.go:1041-1076): BFS over the
/// dependency cascade, counting issues that become actionable once the
/// simulated set of completed issues grows. Existing ready work is skipped —
/// it was not caused by this completion.
fn count_transitive_unblocks(
    issue_id: &str,
    issues: &[bv_core::model::Issue],
    by_id: &std::collections::HashMap<&str, &bv_core::model::Issue>,
) -> usize {
    let Some(issue) = by_id.get(issue_id) else {
        return 0;
    };
    if matches!(
        issue.status,
        bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
    ) {
        return 0;
    }
    let has_deps = issues.iter().any(|o| {
        o.dependencies
            .iter()
            .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == issue_id)
    });
    if !has_deps {
        return 0;
    }

    // Direct dependents of a node, following blocking edges.
    let dependents = |id: &str| -> Vec<String> {
        issues
            .iter()
            .filter(|o| {
                o.dependencies
                    .iter()
                    .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == id)
            })
            .map(|o| o.id.clone())
            .collect()
    };

    let actionable_with = |id: &str, closed: &std::collections::BTreeSet<String>| -> bool {
        match by_id.get(id) {
            Some(i) => {
                i.status.is_open()
                    && i.assignee.trim().is_empty()
                    && !i.issue_type.eq_ignore_ascii_case("epic")
                    && i.dependencies.iter().all(|d| {
                        !d.r#type.is_blocking()
                            || d.effective_depends_on().is_empty()
                            || closed.contains(d.effective_depends_on())
                    })
            }
            None => false,
        }
    };

    let mut simulated_closed: std::collections::BTreeSet<String> = Default::default();
    simulated_closed.insert(issue_id.to_string());
    let mut queue: std::collections::VecDeque<String> = Default::default();
    queue.push_back(issue_id.to_string());
    let mut count = 0usize;

    while let Some(curr) = queue.pop_front() {
        for candidate in dependents(&curr) {
            if simulated_closed.contains(&candidate)
                || actionable_with(&candidate, &Default::default())
            {
                continue;
            }
            if actionable_with(&candidate, &simulated_closed) {
                simulated_closed.insert(candidate.clone());
                queue.push_back(candidate);
                count += 1;
            }
        }
    }
    count
}

/// Go `generateParallelGain` (advanced_insights.go:1063-1230): report the
/// current number of independent work tracks and, per actionable issue, how
/// many more tracks completing it would open.
///
/// A "track" is a connected component of the open dependency graph that holds
/// at least one actionable issue, so completing an issue is a gain only when it
/// splits a component or joins its dependents into a new one.
fn compute_parallel_gain(issues: &[bv_core::model::Issue], limit: usize) -> serde_json::Value {
    const HOW_TO_USE: &str = "Independent work tracks gained by closing each actionable issue now (gain = tracks after - tracks now). Pick high-gain issues to widen parallel work; unblocks lists what opens up.";
    let is_open = |i: &bv_core::model::Issue| i.status.is_open();
    let has_open_blocker = |i: &bv_core::model::Issue| {
        i.dependencies.iter().any(|d| {
            d.r#type.is_blocking()
                && issues
                    .iter()
                    .any(|o| o.id == d.effective_depends_on() && is_open(o))
        })
    };
    let actionable: Vec<&bv_core::model::Issue> = issues
        .iter()
        .filter(|i| is_open(i) && !has_open_blocker(i))
        .collect();
    let active: std::collections::BTreeSet<String> =
        actionable.iter().map(|i| i.id.clone()).collect();

    // Union-find over the open graph (blocking and parent-child edges), counting
    // components that still contain an actionable member.
    let count_tracks =
        |excluded: Option<&str>, active: &std::collections::BTreeSet<String>| -> usize {
            let mut parent: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            for i in issues.iter().filter(|i| is_open(i)) {
                if Some(i.id.as_str()) != excluded {
                    parent.insert(i.id.clone(), i.id.clone());
                }
            }
            fn find(parent: &mut std::collections::BTreeMap<String, String>, x: &str) -> String {
                let mut cur = x.to_string();
                while let Some(next) = parent.get(&cur).cloned() {
                    if next == cur {
                        break;
                    }
                    cur = next;
                }
                cur
            }
            for i in issues.iter().filter(|i| is_open(i)) {
                if Some(i.id.as_str()) == excluded {
                    continue;
                }
                for d in &i.dependencies {
                    if !(d.r#type.is_blocking()
                        || d.r#type == bv_core::model::DependencyType::ParentChild)
                    {
                        continue;
                    }
                    let target = d.effective_depends_on().to_string();
                    if Some(target.as_str()) == excluded || !parent.contains_key(&target) {
                        continue;
                    }
                    let (pf, pt) = (find(&mut parent, &i.id), find(&mut parent, &target));
                    if pf != pt {
                        parent.insert(pt, pf);
                    }
                }
            }
            let mut roots: std::collections::BTreeSet<String> = Default::default();
            for id in active {
                if Some(id.as_str()) == excluded {
                    continue;
                }
                if parent.contains_key(id) {
                    roots.insert(find(&mut parent, id));
                }
            }
            roots.len()
        };

    let tracks_now = count_tracks(None, &active);
    let mut out = serde_json::json!({
        "status": {"state": "computed"},
        "current_parallel": tracks_now,
        "how_to_use": HOW_TO_USE,
    });
    if actionable.is_empty() {
        out["status"] =
            serde_json::json!({"state": "computed", "count": 0, "reason": "No actionable issues"});
        return out;
    }

    let mut items: Vec<(String, String, usize, i64, f64, Vec<String>)> = Vec::new();
    for issue in &actionable {
        // Go `generateParallelGain` (advanced_insights.go:985) uses the same
        // marginal-unblocks rule as parallel_cut: the issues that become ready
        // once this one is done, minus the completed node itself. Rust was
        // measuring a track-count delta instead, which is a different
        // quantity and admitted far more positive-gain candidates.
        let mut unblocks: Vec<String> = Vec::new();
        for o in issues.iter().filter(|o| is_open(o) && o.id != issue.id) {
            let mut blocks_on_this = false;
            let mut other_open_blocker = false;
            for d in o.dependencies.iter().filter(|d| d.r#type.is_blocking()) {
                let target = d.effective_depends_on();
                if target == issue.id {
                    blocks_on_this = true;
                } else if issues.iter().any(|x| x.id == target && is_open(x)) {
                    other_open_blocker = true;
                    break;
                }
            }
            // `other_open_blocker` already excludes issues that were ready
            // before (Go's `!before[id]`); re-testing has_open_blocker here
            // would also reject the very node being completed, since it is
            // still open in the global view.
            if blocks_on_this && !other_open_blocker {
                unblocks.push(o.id.clone());
            }
        }
        unblocks.sort();
        let gain = unblocks.len() as i64 - 1;
        let tracks_after = tracks_now as i64 + gain;
        if gain <= 0 {
            continue;
        }
        let pct = if tracks_now > 0 {
            gain as f64 / tracks_now as f64 * 100.0
        } else {
            0.0
        };
        let tracks_after = tracks_after as usize;
        items.push((
            issue.id.clone(),
            issue.title.clone(),
            tracks_after,
            gain,
            pct,
            unblocks,
        ));
    }

    // Go's ordering: gain desc, then unblocks count desc, then id.
    items.sort_by(|a, b| {
        b.3.cmp(&a.3)
            .then_with(|| b.5.len().cmp(&a.5.len()))
            .then_with(|| a.0.cmp(&b.0))
    });
    let total = items.len();
    items.truncate(limit);
    // Go's FeatureStatus carries `count` (returned) and `limited` (original
    // count before capping); both are omitempty and both feed the digest.
    let mut status = serde_json::json!({"state": "computed"});
    if total > 0 {
        status["count"] = serde_json::json!(items.len());
        status["limited"] = serde_json::json!(total);
    }
    if total > items.len() {
        status["capped"] = serde_json::json!(true);
    }
    out["status"] = status;
    if items.is_empty() {
        return out;
    }
    // Go's field is `metrics`, not `items` (advanced_insights.go:212).
    out["metrics"] = serde_json::json!(items
        .into_iter()
        .map(|(id, title, potential, gain, pct, unblocks)| {
            serde_json::json!({
                "id": id,
                "title": title,
                "current_parallel": tracks_now,
                "potential_parallel": potential,
                "gain": gain,
                "gain_percent": pct,
                "unblocks": unblocks,
            })
        })
        .collect::<Vec<_>>());
    out
}

/// Go: `--robot-by-label`/`--robot-by-assignee` are modifiers of
/// `--robot-priority` (main.go:1799-1800) — exact-match filters applied to
/// the recommendation list, not standalone commands.
/// Go `generateRecommendation` (pkg/analysis/priority.go:735-869) over the full
/// impact-scoring engine.
///
/// Shared by `--robot-priority` and the `priority_mismatch` proactive alert
/// (pkg/drift/drift.go:1062), which needs the same recommendations.
fn priority_recommendations(issues: &[bv_core::model::Issue]) -> Vec<serde_json::Value> {
    let g = bv_analysis::build_graph(issues);
    let pr = bv_graph_core::pagerank_default(&g);
    let bw = bv_graph_core::betweenness(&g);
    let cp = bv_graph_core::critical_path_heights(&g);

    let pr_map: std::collections::BTreeMap<String, f64> = pr
        .iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), *v))
        .collect();
    let bw_map: std::collections::BTreeMap<String, f64> = bw
        .iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), *v))
        .collect();
    let cp_map: std::collections::BTreeMap<String, f64> = cp
        .iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), *v))
        .collect();

    let now = robot_now();
    let inputs = bv_analysis::impact::ImpactInputs {
        issues,
        pagerank: &pr_map,
        betweenness: &bw_map,
        critical_path: Some(&cp_map),
        g: &g,
        now,
    };

    // Structural signals used by the recommendation engine (Go priority.go:695-708):
    // k-core number, articulation membership and critical-path slack. On the
    // real corpus these are the only signals that fire for most issues, so
    // omitting them silently suppressed every recommendation.
    let core_map: std::collections::BTreeMap<String, u32> = bv_graph_core::kcore(&g)
        .into_iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), v))
        .collect();
    // articulation_points returns node indices, not ids.
    let art_set: std::collections::BTreeSet<String> =
        bv_analysis::algorithms::articulation::articulation_points(&g)
            .into_iter()
            .map(|i| g.node_id(i).unwrap_or_default().to_string())
            .collect();
    let slack_map: std::collections::BTreeMap<String, f64> = bv_graph_core::slack(&g)
        .into_iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), v))
        .collect();
    let max_core = core_map.values().copied().max().unwrap_or(0);

    // Use the full impact scoring engine
    let impact_results = bv_analysis::impact::compute_impact_scores(&inputs);

    // Go `generateRecommendation` (priority.go:735-869). An issue is only
    // recommended when at least one structural signal fires AND the derived
    // priority differs from the current one; the previous code compared
    // weighted breakdown values against ad-hoc constants and hardcoded
    // confidence 1, so it never agreed with the oracle.
    let th = PriorityThresholds::default();
    let unblocks_by_id = build_unblocks_map(issues);
    let mut recommendations: Vec<serde_json::Value> = Vec::new();
    for r in &impact_results {
        let Some(issue) = issues.iter().find(|i| i.id == r.id) else {
            continue;
        };
        if let Some(rec) = build_priority_recommendation(
            r,
            issue,
            &unblocks_by_id,
            &th,
            issues,
            &cp_map,
            (&core_map, &art_set, &slack_map, max_core),
        ) {
            recommendations.push(rec);
        }
    }
    recommendations
}

fn run_robot_priority(args: &[String]) -> ExitCode {
    let by_label = args
        .iter()
        .position(|a| a == "--robot-by-label")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let by_assignee = args
        .iter()
        .position(|a| a == "--robot-by-assignee")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let (issues, _hash, _p1, status, _g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    // Go scores the whole graph and filters the *recommendation list*
    // afterwards (robot_registry.go:940-951). Filtering the issue set first
    // shrank total_issues and stripped the graph context the surviving
    // recommendations need, so the filter returned nothing.
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = bv_analysis::build_graph(&issues);

    // Shared with the `priority_mismatch` alert so both consume one
    // implementation of Go's recommendation engine.
    let mut recommendations = priority_recommendations(&issues);

    // Go (priority.go:720) sorts by confidence descending, then impact score,
    // then issue id, so the ordering is stable across runs.
    // Go filters the scored recommendations, not the issue set
    // (robot_registry.go:940-951).
    // The emitted recommendation carries no labels or assignee, so match against
    // the source issue the recommendation names.
    let rec_issue = |r: &serde_json::Value| -> Option<&bv_core::model::Issue> {
        let id = r["issue_id"].as_str()?;
        issues.iter().find(|i| i.id == id)
    };
    if let Some(label) = by_label.as_deref().filter(|v| !v.is_empty()) {
        recommendations.retain(|r| {
            rec_issue(r)
                .map(|i| i.labels.iter().any(|l| l == label))
                .unwrap_or(false)
        });
    }
    if let Some(assignee) = by_assignee.as_deref().filter(|v| !v.is_empty()) {
        recommendations.retain(|r| {
            rec_issue(r)
                .map(|i| i.assignee == assignee)
                .unwrap_or(false)
        });
    }

    recommendations.sort_by(|a, b| {
        b["confidence"]
            .as_f64()
            .partial_cmp(&a["confidence"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b["impact_score"]
                    .as_f64()
                    .partial_cmp(&a["impact_score"].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a["issue_id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["issue_id"].as_str().unwrap_or_default())
            })
    });
    recommendations.truncate(10);

    let mut payload = full_envelope_for(&hash, &issues);
    payload["analysis_config"] = priority_analysis_config(g.len());
    // Real status from phase2 analysis (Go priority uses full config).
    let priority_status = {
        let mut s = status.clone();
        if s.page_rank.state == "skipped" {
            s.page_rank.reason.clear();
        }
        if s.eigenvector.state == "skipped" {
            s.eigenvector.reason.clear();
        }
        if s.hits.state == "skipped" {
            s.hits.reason.clear();
        }
        if s.critical.state == "skipped" {
            s.critical.reason.clear();
        }
        if s.cycles.state == "skipped" {
            s.cycles.reason.clear();
        }
        if s.kcore.state == "skipped" {
            s.kcore.reason.clear();
        }
        if s.articulation.state == "skipped" {
            s.articulation.reason.clear();
        }
        if s.slack.state == "skipped" {
            s.slack.reason.clear();
        }
        s.to_json_map()
    };
    payload["status"] = priority_status;
    payload["recommendations"] = serde_json::Value::Array(recommendations.clone());
    payload["field_descriptions"] = serde_json::json!({
        "status.capped": "Whether results were truncated to prevent overload",
        "status.phase2": "Whether expensive graph metrics (PageRank, betweenness) are included",
        "top_reasons": "Top 3 factors contributing to priority score, ordered by weight",
        "what_if.cascade": "Total issues transitively unblocked (including indirect)",
        "what_if.days_saved": "Estimated days saved based on issue estimates",
        "what_if.depth": "Critical path depth reduction if completed",
        "what_if.parallelization_gain": "Net change in parallel work capacity (direct_unblocks - 1); positive = more parallel work possible",
        "what_if.unblocks": "Number of issues directly waiting on this one",
    });
    // Go echoes the active scoping modifiers into `filters`
    // (robot-priority handlers build it from by_label / by_assignee).
    let mut filters = serde_json::json!({"max_results": 10});
    if let Some(v) = by_label.as_deref().filter(|v| !v.is_empty()) {
        filters["by_label"] = serde_json::json!(v);
    }
    if let Some(v) = by_assignee.as_deref().filter(|v| !v.is_empty()) {
        filters["by_assignee"] = serde_json::json!(v);
    }
    payload["filters"] = filters;
    payload["summary"] = serde_json::json!({
        "total_issues": issues.len(),
        "recommendations": recommendations.len(),
        "high_confidence": recommendations.iter()
            .filter(|r| r["impact_score"].as_f64().unwrap_or(0.0) > 0.5)
            .count(),
    });
    payload["usage_hints"] = serde_json::json!([
        "jq '.recommendations[] | select(.confidence > 0.7)' - Filter high confidence",
        "jq '.recommendations[] | {id: .issue_id, score: .impact_score, prio: .suggested_priority}' - Extract essentials",
        "jq '.summary' - Overview counts",
    ]);
    emit_json(&payload)
}

fn run_robot_suggest(args: &[String]) -> ExitCode {
    // Parse --suggest-type, --suggest-confidence, --suggest-bead (Go parity).
    let suggest_type = args
        .iter()
        .position(|a| a == "--suggest-type")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let suggest_confidence = args
        .iter()
        .position(|a| a == "--suggest-confidence")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<f64>().ok());
    let suggest_bead = args
        .iter()
        .position(|a| a == "--suggest-bead")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit, loaded_source) = match load_issues_auto_meta(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let mut config = bv_analysis::suggestions::SuggestAllConfig::default();

    if let Some(conf) = suggest_confidence {
        config.min_confidence = conf;
    }
    if let Some(ref bead) = suggest_bead {
        config.filter_bead = bead.clone();
    }

    // Map suggest-type strings (Go parity).
    match suggest_type.as_deref() {
        None | Some("") => {}
        Some("duplicate") | Some("duplicates") => {
            config.filter_type = Some("potential_duplicate".to_string());
        }
        Some("dependency") | Some("dependencies") => {
            config.filter_type = Some("missing_dependency".to_string());
        }
        Some("label") | Some("labels") => {
            config.filter_type = Some("label_suggestion".to_string());
        }
        Some("cycle") | Some("cycles") => {
            config.filter_type = Some("cycle_warning".to_string());
        }
        Some(other) => {
            eprintln!("Invalid suggest-type: {other} (use: duplicate, dependency, label, cycle)");
            return ExitCode::from(1);
        }
    }
    // Tracker-backed mutation commands need the loaded source path; the
    // suggestion layer cannot resolve an issue's live route without it.
    let source = loaded_source;
    let output = bv_analysis::suggestions::generate_robot_suggest_output(
        &issues,
        &config,
        &hash,
        &source.path,
    );
    match serde_json::to_value(&output) {
        Ok(mut v) => {
            // Go v0.25.0 stamps the full envelope ahead of the suggestion
            // payload; the analysis layer has no source path, so fill it in
            // from the shared envelope helper here.
            let envelope = full_envelope_for(&hash, &issues);
            if let Some(obj) = v.as_object_mut() {
                for key in [
                    "output_format",
                    "version",
                    "source_path",
                    "source_kind",
                    "source_authority",
                    "authority_hash",
                    "scope_hash",
                ] {
                    if let Some(env) = envelope.get(key) {
                        obj.insert(key.to_string(), env.clone());
                    }
                }
            }
            emit_json(&v)
        }
        Err(e) => {
            eprintln!("Error: serialization failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn run_robot_alerts() -> ExitCode {
    let arg_value = |names: &[&str]| -> Option<String> {
        let args: Vec<String> = std::env::args().collect();
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            for n in names {
                if a == n {
                    let v = args.get(i + 1).cloned().unwrap_or_default();
                    return (!v.trim().is_empty()).then_some(v);
                }
                if let Some(v) = a.strip_prefix(&format!("{n}=")) {
                    return (!v.trim().is_empty()).then(|| v.to_string());
                }
            }
            i += 1;
        }
        None
    };
    let want_severity = arg_value(&["--severity"]);
    let want_type = arg_value(&["--alert-type"]);
    let want_label = arg_value(&["--alert-label"]);
    let cwd = std::env::current_dir().unwrap_or_default();
    let (loaded, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    // --label narrows the analysis to the label's subgraph before the drift
    // baseline is captured, as Go's scopeLoadedIssues does (main.go:4870-4900).
    // The envelope keeps describing the *loaded* source, so source_authority and
    // authority_hash still report the whole file.
    let issues = apply_label_scope(&loaded);
    // Go (robot_registry.go:1158-1186) compares the live graph against the
    // saved baseline at `.bv/baseline.json`, falling back to comparing the
    // current stats against themselves when no baseline exists. Passing the
    // same stats for both sides — as this handler used to — made every
    // drift check trivially zero, so --robot-alerts always reported none.
    let (current, cycles, _hash) = match capture_baseline_for(&issues) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    // Read the baseline from the same place Go does. Go's `baseline.Load`
    // populates `TopMetrics.PageRank` from a top-level `top_metrics.pagerank`
    // array of {id, value}; our baseline files predate that field, so Go sees
    // an empty top-list and reports every current entry as "entered top".
    // Reading `stats.pagerank` here instead made the two binaries disagree
    // about the same file, so the comparison is anchored to Go's layout.
    // Whether a baseline was actually recorded matters: with no file at all
    // there is nothing to have changed, and Go exits rather than inventing a
    // comparison. With a file present, Go honours its exact layout below —
    // including an absent `top_metrics.pagerank`, which it reads as an empty
    // list and therefore reports every current entry as newly entered.
    let baseline_on_disk = std::fs::read_to_string(BASELINE_PATH)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
    let baseline_doc = baseline_on_disk.clone().unwrap_or_default();
    let mut baseline_stats: bv_analysis::drift::BaselineStats = baseline_doc
        .get("stats")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| current.clone());
    // Go keys the top-list by `top_metrics.pagerank`. When that key is
    // genuinely present, honour it. When it is absent, leave the baseline's
    // own PageRank alone: overwriting it with an empty map made every current
    // entry count as having "entered top", inventing a change that no
    // recorded baseline ever observed. A missing top-list is not evidence of
    // change.
    if let Some(doc) = &baseline_on_disk {
        baseline_stats.pagerank = doc
            .get("top_metrics")
            .and_then(|tm| tm.get("pagerank"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
    }
    let result = bv_analysis::drift::calculate(
        &baseline_stats,
        &current,
        &bv_analysis::drift::DriftConfig::default(),
        &cycles,
        &issues,
        robot_now(),
    );

    // Go robot-alerts embeds the full RobotEnvelope (output_format+version)
    // and provides non-empty usage hints.
    // Go robot-alerts embeds the full RobotEnvelope and filters the computed
    // alerts before emitting them (robot_registry.go:1190-1215): exact match on
    // severity and type, then a case-insensitive label match against the
    // alert's label and its detail substrings.
    let filtered_alerts: Vec<serde_json::Value> = result
        .alerts
        .iter()
        .filter(|a| {
            want_severity
                .as_deref()
                .map(|w| format!("{:?}", a.severity).to_lowercase() == w.to_lowercase())
                .unwrap_or(true)
        })
        .filter(|a| {
            want_type
                .as_deref()
                .map(|w| format!("{:?}", a.alert_type).to_lowercase() == w.to_lowercase())
                .unwrap_or(true)
        })
        .filter(|a| match want_label.as_deref() {
            None => true,
            Some(want) => {
                let want = want.to_lowercase();
                let is_label = !a.label.is_empty() && a.label.to_lowercase() == want;
                let in_details = a.details.iter().any(|d| d.to_lowercase().contains(&want));
                is_label || in_details
            }
        })
        .map(|a| serde_json::to_value(a).unwrap_or_default())
        .collect();
    let mut payload = full_envelope_for(&hash, &loaded);

    // Go `checkPriorityMismatch` (pkg/drift/drift.go:1062-1100). Uses the same
    // recommendations as `--robot-priority` so both agree on what
    // "under-prioritised" means. Only "increase" directions alert: "could be
    // lower" is hygiene for --robot-priority and fires for nearly every leaf
    // on a small graph.
    let mut priority_alerts: Vec<serde_json::Value> = Vec::new();
    let mut duplicate_alerts: Vec<serde_json::Value> = Vec::new();
    // Go stamps every alert with `c.nowUTC()` (drift.go:1048, 1098).
    let detected_at = robot_now()
        .to_string()
        .get(..19)
        .map(|t| format!("{t}Z"))
        .unwrap_or_default();
    {
        const MIN_CONFIDENCE: f64 = 0.6; // Go default, drift/config.go:124
        let issue_by_id: std::collections::HashMap<&str, &bv_core::model::Issue> =
            loaded.iter().map(|i| (i.id.as_str(), i)).collect();
        for rec in priority_recommendations(&loaded) {
            let conf = rec["confidence"].as_f64().unwrap_or(0.0);
            if conf < MIN_CONFIDENCE || rec["direction"].as_str() != Some("increase") {
                continue;
            }
            let id = rec["issue_id"].as_str().unwrap_or_default();
            let cur = rec["current_priority"].as_i64().unwrap_or(0);
            let sug = rec["suggested_priority"].as_i64().unwrap_or(0);
            let labels: Vec<&str> = issue_by_id
                .get(id)
                .map(|i| i.labels.iter().map(|l| l.as_str()).collect())
                .unwrap_or_default();
            // Go declares baseline_value/current_value/delta with
            // `omitempty` (drift.go:75-77), so a priority of P0 omits
            // current_value rather than emitting 0.
            let mut alert = serde_json::json!({
                "type": "priority_mismatch",
                "severity": "warning",
                "message": format!("{id} is P{cur} but graph impact suggests P{sug} (confidence {conf:.2})"),
                "issue_id": id,
                "labels": labels,
                "delta": sug - cur,
                "details": rec["reasoning"].clone(),
                "detected_at": detected_at.clone(),
                "suggested_action": format!("Review with bv --robot-priority; if it holds, set the priority to P{sug}"),
            });
            if cur != 0 {
                alert["baseline_value"] = serde_json::json!(cur);
            }
            if sug != 0 {
                alert["current_value"] = serde_json::json!(sug);
            }
            priority_alerts.push(alert);
        }
    }

    // Go `checkPotentialDuplicate` (pkg/drift/drift.go:1011-1053). Closed and
    // tombstoned issues are excluded: pairing them buries the live duplicates
    // under history. Runs after priority_mismatch (drift.go:300-301).
    {
        let live: Vec<&bv_core::model::Issue> = loaded
            .iter()
            .filter(|i| {
                !matches!(
                    i.status,
                    bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
                )
            })
            .collect();
        if live.len() >= 2 {
            let owned: Vec<bv_core::model::Issue> = live.into_iter().cloned().collect();
            let cfg = bv_analysis::suggestions::DuplicateConfig::default();
            // Go caps duplicate alerts at config.DuplicateMaxAlerts
            // (drift/config.go:123, default 10) and breaks out of the loop
            // once the cap is reached, on top of the detector's own
            // MaxSuggestions=20. Both caps are needed: the detector alone
            // yields 20 here, Go emits 10.
            const MAX_ALERTS: usize = 10;
            for s in bv_analysis::suggestions::detect_duplicates(&owned, &cfg) {
                if duplicate_alerts.len() >= MAX_ALERTS {
                    break;
                }
                let rel = s.related_bead.clone();
                duplicate_alerts.push(serde_json::json!({
                    "type": "potential_duplicate",
                    "severity": "info",
                    "message": s.summary,
                    "issue_id": s.target_bead,
                    "related_issue_id": rel,
                    "details": [s.reason],
                    "detected_at": detected_at.clone(),
                    "suggested_action": "Compare the two issues; close one as a duplicate or link them with a related dependency",
                }));
            }
        }
    }

    // Go appends the proactive alerts after staleness (drift.go:288 runs
    // checkStaleness, then :300 potential duplicate and :301 priority
    // mismatch), so both land at the end.
    let all_alerts: Vec<serde_json::Value> = filtered_alerts
        .iter()
        .cloned()
        .chain(duplicate_alerts)
        .chain(priority_alerts)
        .collect();
    payload["alerts"] = serde_json::to_value(&all_alerts).unwrap_or_default();
    // Go emits `skipped_checks` alongside the alerts so a check that did not
    // run is never read as one that found nothing. Derived directly from Go's
    // `expensiveCheckAllowed` rule rather than by re-running the analysis.
    {
        let limit = bv_analysis::drift::DriftConfig::default().proactive_max_issues;
        if limit > 0 && loaded.len() > limit {
            let skipped: Vec<bv_analysis::drift::SkippedCheck> =
                ["potential_duplicate", "priority_mismatch"]
                    .iter()
                    .map(|t| bv_analysis::drift::SkippedCheck {
                        check_type: (*t).to_string(),
                        reason: format!(
                            "{} issues exceed proactive_max_issues={}",
                            loaded.len(),
                            limit
                        ),
                    })
                    .collect();
            if !skipped.is_empty() {
                payload["skipped_checks"] = serde_json::to_value(&skipped).unwrap_or_default();
            }
        }
    }
    // The summary covers the alerts actually emitted, including the
    // proactive checks appended above.
    let count_sev_in = |list: &[serde_json::Value], s: &str| {
        list.iter()
            .filter(|a| a.get("severity").and_then(|v| v.as_str()) == Some(s))
            .count()
    };
    payload["summary"] = serde_json::json!({
        "total": all_alerts.len(),
        "critical": count_sev_in(&all_alerts, "critical"),
        "warning": count_sev_in(&all_alerts, "warning"),
        "info": count_sev_in(&all_alerts, "info"),
    });
    // Go robot_registry.go:1240-1248 — the full seven-hint list, including the
    // proactive and drift-vs-baseline filter combinations.
    payload["usage_hints"] = serde_json::json!([
        "--severity=warning --alert-type=stale_issue   # stale warnings only",
        "--alert-type=blocking_cascade                 # high-unblock opportunities",
        "--alert-type=high_impact_unblock|abandoned_claim|potential_duplicate|priority_mismatch|velocity_drop   # proactive checks (no baseline needed)",
        "--alert-type=new_cycle|density_growth|node_count_change|edge_count_change|scope_creep|blocked_increase|actionable_change|pagerank_change   # drift vs saved baseline (bv --save-baseline)",
        "--alert-label=backend                        # only alerts on issues carrying that label",
        "jq '.alerts | map({issue_id, type, suggested_action})'   # what to do about each",
        "thresholds: .bv/drift.yaml; every key and its default is listed in the README 'Alerts System' table",
    ]);
    emit_json(&payload)
}

fn run_robot_graph(args: &[String]) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let g = bv_analysis::build_graph(&issues);

    let fmt = args
        .iter()
        .position(|a| a == "--graph-format")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "json".to_string());
    if fmt != "json" && fmt != "dot" && fmt != "mermaid" {
        eprintln!("Invalid --graph-format \"{fmt}\" (expected json|dot|mermaid)");
        return ExitCode::from(2);
    }

    let mut issues: Vec<bv_core::model::Issue> = issues;
    if let Some(gi) = args.iter().position(|a| a == "--graph-root") {
        let root = args.get(gi + 1).cloned().unwrap_or_default();
        let depth = args
            .iter()
            .position(|a| a == "--graph-depth")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let refs: Vec<&bv_core::model::Issue> = issues.iter().collect();
        issues = bv_export::graph_export::subgraph(&refs, &root, depth)
            .into_iter()
            .cloned()
            .collect();
    }

    if fmt != "json" {
        let content = if fmt == "dot" {
            bv_export::graph_export::generate_dot(&issues, None)
        } else {
            bv_export::graph_export::generate_mermaid_graph(&issues)
        };
        let mut payload = full_envelope_for(&hash, &issues);
        payload["format"] = serde_json::json!(fmt);
        payload["graph"] = serde_json::json!(content);
        payload["nodes"] = serde_json::json!(issues.len());
        emit_json(&payload);
        return ExitCode::from(0);
    }

    // Go generateAdjacency: nodes sorted by ID with labels+pagerank; edges
    // follow sorted issues with deps sorted by DependsOnID; type normalized
    // ("blocks" when empty). Counts at top level (Go GraphExportResult).
    let pagerank = bv_graph_core::pagerank_default(&g);
    let pr_by_id: std::collections::BTreeMap<String, f64> = pagerank
        .iter()
        .enumerate()
        .map(|(i, v)| (g.node_id(i).unwrap_or_default().to_string(), *v))
        .collect();

    let mut sorted_issues: Vec<&bv_core::model::Issue> = issues.iter().collect();
    sorted_issues.sort_by(|a, b| a.id.cmp(&b.id));

    let adj_nodes: Vec<serde_json::Value> = sorted_issues
        .iter()
        .map(|i| {
            let mut node = serde_json::json!({
                "id": i.id,
                "title": i.title,
                "status": i.status.as_str(),
                "priority": i.priority,
            });
            if !i.labels.is_empty() {
                node["labels"] = serde_json::json!(i.labels);
            }
            if let Some(pr) = pr_by_id.get(&i.id) {
                if *pr != 0.0 {
                    node["pagerank"] = serde_json::json!(pr);
                }
            }
            node
        })
        .collect();

    let issue_ids: std::collections::HashSet<&str> = issues.iter().map(|i| i.id.as_str()).collect();
    let mut adj_edges: Vec<serde_json::Value> = Vec::new();
    for i in &sorted_issues {
        let mut deps: Vec<&bv_core::model::Dependency> = i.dependencies.iter().collect();
        deps.sort_by(|a, b| a.effective_depends_on().cmp(b.effective_depends_on()));
        for dep in deps {
            if !issue_ids.contains(dep.effective_depends_on()) {
                continue;
            }
            let edge_type = dep.r#type.as_str();
            adj_edges.push(serde_json::json!({
                "from": i.id,
                "to": dep.effective_depends_on(),
                "type": edge_type,
            }));
        }
    }
    let edge_count = issues
        .iter()
        .filter(|i| issue_ids.contains(i.id.as_str()))
        .flat_map(|i| i.dependencies.iter())
        .filter(|dep| issue_ids.contains(dep.effective_depends_on()))
        .count();

    // Go emits the v0.25.0 envelope first, then format/nodes/edges/
    // explanation/adjacency. This branch hand-rolled its payload and put
    // data_hash last, so it missed the envelope entirely.
    let mut payload = full_envelope_for(&hash, &issues);
    payload["format"] = serde_json::json!("json");
    payload["nodes"] = serde_json::json!(sorted_issues.len());
    payload["edges"] = serde_json::json!(edge_count);
    payload["explanation"] = serde_json::json!({
        "what": "Dependency graph as JSON adjacency list",
        "when_to_use": "When you need programmatic access to the graph structure",
    });
    payload["adjacency"] = serde_json::json!({"nodes": adj_nodes, "edges": adj_edges});
    emit_json(&payload)
}

fn run_robot_recipes() -> ExitCode {
    let mut recipes: Vec<(String, &'static str)> = [
        (
            "default",
            "Default view showing all open issues sorted by priority",
        ),
        ("actionable", "Issues ready to work on (no open blockers)"),
        ("recent", "Issues updated in the last 7 days"),
        ("blocked", "Issues waiting on dependencies"),
        (
            "high-impact",
            "Issues with highest blocking impact (PageRank)",
        ),
        ("stale", "Open issues not updated in 30+ days"),
        (
            "triage",
            "Issues sorted by computed triage score (high impact + unblocking potential)",
        ),
        ("closed", "Recently closed issues"),
        (
            "release-cut",
            "Recently closed items for changelog generation",
        ),
        (
            "quick-wins",
            "Easy items with no blockers - good for quick progress",
        ),
        (
            "bottlenecks",
            "High betweenness nodes - potential project bottlenecks",
        ),
    ]
    .iter()
    .map(|(name, desc)| (name.to_string(), *desc))
    .collect();
    // Go `ListSummaries` sorts recipe summaries alphabetically by name.
    recipes.sort_by(|a, b| a.0.cmp(&b.0));
    let recipes: Vec<serde_json::Value> = recipes
        .into_iter()
        .map(|(name, desc)| serde_json::json!({"name": name, "description": desc, "source": "builtin"}))
        .collect();

    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "output_format": output_format(),
        "version": GO_APP_VERSION,
        "recipes": recipes,
    });
    emit_json(&payload)
}

type CorrelationReport =
    std::collections::BTreeMap<String, Vec<bv_correlation::correlator::CorrelatedCommit>>;

/// Read `--<name>` / `--<name>=<value>` out of the raw argv. Go's `flag`
/// package accepts both spellings and several Rust handlers already rely on
/// the `=` form (see `history_flag_value`).
/// Value of a `--name value` / `--name=value` flag, borrowed from `args`.
/// Same scan as `search_flag`; this one avoids allocating.
fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let long = format!("--{name}");
    let with_eq = format!("--{name}=");
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix(&with_eq) {
            return Some(v);
        }
        if a == &long {
            return args.get(i + 1).map(|s| s.as_str());
        }
    }
    None
}

fn search_flag(args: &[String], name: &str) -> Option<String> {
    let long = format!("--{name}");
    let with_eq = format!("--{name}=");
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix(&with_eq) {
            return Some(v.to_string());
        }
        if a == &long {
            return args.get(i + 1).cloned();
        }
    }
    None
}

/// Go `search.EmbeddingConfigFromEnv` + `EmbeddingConfig.Normalized`
/// (config.go:15, embedder.go:29). `BV_SEMANTIC_EMBEDDER` /
/// `BV_SEMANTIC_MODEL` / `BV_SEMANTIC_DIM`; the provider falls back to
/// `"hash"` when unset and a non-positive or unparseable dim becomes
/// `DefaultEmbeddingDim` (384).
struct SearchEmbedderConfig {
    provider: String,
    model: String,
    dim: usize,
}

fn search_embedding_config_from_env() -> SearchEmbedderConfig {
    let provider = std::env::var("BV_SEMANTIC_EMBEDDER")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let model = std::env::var("BV_SEMANTIC_MODEL")
        .unwrap_or_default()
        .trim()
        .to_string();
    // Go calls `strconv.Atoi` without trimming: a padded value is a parse
    // error, which leaves dim at 0 and therefore normalizes to 384 anyway.
    let dim: i64 = std::env::var("BV_SEMANTIC_DIM")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or(0);
    SearchEmbedderConfig {
        provider: if provider.is_empty() {
            "hash".to_string()
        } else {
            provider
        },
        model,
        dim: if dim <= 0 {
            bv_search::embedder::DEFAULT_DIM
        } else {
            dim as usize
        },
    }
}

/// Go `search.NewEmbedderFromConfig` (config.go:34). The hash embedder is the
/// only provider `bv-search` ships, so the two placeholder providers and the
/// unknown-provider branch are reproduced verbatim as errors — an unimplemented
/// provider must not silently fall back to a different ranking.
fn search_embedder_dim(cfg: &SearchEmbedderConfig) -> Result<usize, String> {
    match cfg.provider.as_str() {
        "" | "hash" => Ok(cfg.dim),
        "python-sentence-transformers" => Err(format!(
            "semantic embedder {:?} not implemented (mvp placeholder); set BV_SEMANTIC_EMBEDDER={:?} for deterministic fallback",
            cfg.provider, "hash"
        )),
        "openai" => Err(format!(
            "semantic embedder {:?} not implemented (placeholder); set BV_SEMANTIC_EMBEDDER={:?} for deterministic fallback",
            cfg.provider, "hash"
        )),
        other => Err(format!(
            "unknown semantic embedder {other:?}; expected {:?}",
            "hash"
        )),
    }
}

/// Go `parseSearchMinScore` (search_output.go:64). An empty string means "no
/// threshold" (`nil`); a non-finite value or one outside `[-1, 1]` is rejected
/// with Go's exact message, because the threshold is compared against raw
/// cosine similarity and must never be able to reject the whole result set.
fn search_parse_min_score(raw: &str) -> Result<Option<f64>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    let score: f64 = raw.parse().map_err(|_| search_min_score_error(raw))?;
    if !score.is_finite() || !(-1.0..=1.0).contains(&score) {
        return Err(search_min_score_error(raw));
    }
    Ok(Some(score))
}

fn search_min_score_error(raw: &str) -> String {
    format!("invalid --search-min-score {raw:?} (expected a finite number from -1 to 1)")
}

/// Go `search.SearchConfig` (config.go:57). `Weights` starts at Go's zero
/// value — `resolveSearchWeights` only consults it when `has_weights` is set.
struct SearchConfig {
    mode: String,
    preset: String,
    weights: bv_search::hybrid::Weights,
    has_weights: bool,
}

const SEARCH_ZERO_WEIGHTS: bv_search::hybrid::Weights = bv_search::hybrid::Weights {
    text_relevance: 0.0,
    pagerank: 0.0,
    status: 0.0,
    impact: 0.0,
    priority: 0.0,
    recency: 0.0,
};

/// A `resolveSearchConfig` failure. Go reports every one of these through
/// `resolveSearchConfig` and exits 1 (`go`); only an unknown
/// `--search-preset` is reported as a usage error with exit 2, the one
/// documented divergence (see `run_robot_search`).
struct SearchConfigError {
    message: String,
    exit_code: u8,
}

impl SearchConfigError {
    fn go(message: String) -> Self {
        Self {
            message,
            exit_code: 1,
        }
    }

    fn usage(message: String) -> Self {
        Self {
            message,
            exit_code: 2,
        }
    }
}

/// Go `search.ParseSearchConfig` (config.go:70) — validate the values the
/// environment supplied. A preset owns its implied mode: naming a hybrid
/// preset with no explicit mode selects hybrid, `text-only` selects text, and
/// an explicit text mode under a hybrid preset is an error rather than a
/// silently ignored flag.
fn search_parse_config(
    mode_value: &str,
    preset_value: &str,
    weights_value: &str,
) -> Result<SearchConfig, SearchConfigError> {
    let mut cfg = SearchConfig {
        mode: "text".to_string(),
        preset: "default".to_string(),
        weights: SEARCH_ZERO_WEIGHTS,
        has_weights: false,
    };

    let mut mode_set = false;
    let mode = mode_value.trim();
    if !mode.is_empty() {
        let lowered = mode.to_lowercase();
        match lowered.as_str() {
            "text" | "hybrid" => {
                cfg.mode = lowered;
                mode_set = true;
            }
            _ => {
                return Err(SearchConfigError::go(format!(
                    "invalid search mode: {mode:?} (expected text|hybrid)"
                )))
            }
        }
    }

    let preset = preset_value.trim();
    if !preset.is_empty() {
        let name = preset.to_lowercase();
        if bv_search::hybrid::get_preset(&name).is_none() {
            return Err(SearchConfigError::go(format!("unknown preset {name:?}")));
        }
        cfg.preset = name.clone();
        if name == "text-only" {
            if !mode_set {
                cfg.mode = "text".to_string();
            }
        } else if !mode_set {
            cfg.mode = "hybrid".to_string();
        } else if cfg.mode == "text" {
            return Err(SearchConfigError::go(format!(
                "search preset {preset:?} needs hybrid mode; use hybrid mode or the text-only preset"
            )));
        }
    }

    let weights = weights_value.trim();
    if !weights.is_empty() {
        cfg.weights = search_parse_weights_json(weights).map_err(SearchConfigError::go)?;
        cfg.has_weights = true;
    }

    Ok(cfg)
}

/// Go `applySearchConfigOverrides` (search_output.go:100) — apply the flags on
/// top of the parsed environment config. Unlike `ParseSearchConfig` the flag
/// values are *not* trimmed here, because a padded mode or preset name is a
/// typo worth reporting rather than quietly repairing.
fn search_apply_config_overrides(
    mut cfg: SearchConfig,
    mode_flag: &str,
    preset_flag: &str,
    weights_flag: &str,
) -> Result<SearchConfig, SearchConfigError> {
    if !mode_flag.is_empty() {
        let lowered = mode_flag.to_lowercase();
        match lowered.as_str() {
            "text" | "hybrid" => cfg.mode = lowered,
            _ => {
                return Err(SearchConfigError::go(format!(
                    "invalid --search-mode: {mode_flag:?} (expected text|hybrid)"
                )))
            }
        }
    }

    if !preset_flag.is_empty() {
        let name = preset_flag.to_lowercase();
        if bv_search::hybrid::get_preset(&name).is_none() {
            // The one deliberate divergence from Go: Rust names the flag the
            // user typed, lists the valid presets, and exits 2. Go exits 1
            // with a bare `unknown preset %q`.
            return Err(SearchConfigError::usage(format!(
                "unknown --search-preset {preset_flag:?} (expected one of default, bug-hunting, sprint-planning, impact-first, text-only)"
            )));
        }
        cfg.preset = name.clone();
        if name == "text-only" {
            if mode_flag.is_empty() {
                cfg.mode = "text".to_string();
            }
        } else if mode_flag.is_empty() {
            cfg.mode = "hybrid".to_string();
        } else if cfg.mode == "text" {
            return Err(SearchConfigError::go(format!(
                "--search-preset {preset_flag:?} needs hybrid mode; drop --search-mode text or use --search-preset text-only"
            )));
        }
    }

    if !weights_flag.is_empty() {
        cfg.weights = search_parse_weights_json(weights_flag).map_err(SearchConfigError::go)?;
        cfg.has_weights = true;
    }

    Ok(cfg)
}

/// Go `resolveSearchConfig` (search_output.go:80). The environment supplies
/// the defaults; naming a flag on a dimension suppresses the inherited value
/// for that dimension only, so `--search-mode hybrid` still inherits
/// `BV_SEARCH_PRESET` while `--search-preset` does not inherit
/// `BV_SEARCH_MODE`.
fn search_resolve_config(
    mode_flag: &str,
    preset_flag: &str,
    weights_flag: &str,
) -> Result<SearchConfig, SearchConfigError> {
    let mut mode = std::env::var("BV_SEARCH_MODE").unwrap_or_default();
    let mut preset = std::env::var("BV_SEARCH_PRESET").unwrap_or_default();
    let mut weights = std::env::var("BV_SEARCH_WEIGHTS").unwrap_or_default();
    // Validate only inherited values. A selected preset owns the implied
    // mode, and explicit text mode makes an inherited hybrid preset irrelevant.
    if !mode_flag.is_empty() || !preset_flag.is_empty() {
        mode = String::new();
    }
    if !preset_flag.is_empty() || mode_flag.eq_ignore_ascii_case("text") {
        preset = String::new();
    }
    if !weights_flag.is_empty() {
        weights = String::new();
    }
    let cfg = search_parse_config(&mode, &preset, &weights)?;
    search_apply_config_overrides(cfg, mode_flag, preset_flag, weights_flag)
}

/// Go `Weights.sum` (weights.go).
fn search_weights_sum(w: &bv_search::hybrid::Weights) -> f64 {
    w.text_relevance + w.pagerank + w.status + w.impact + w.priority + w.recency
}

/// Go `Weights.Normalize` (weights.go).
fn search_weights_normalize(w: bv_search::hybrid::Weights) -> bv_search::hybrid::Weights {
    let sum = search_weights_sum(&w);
    if sum == 0.0 {
        return w;
    }
    bv_search::hybrid::Weights {
        text_relevance: w.text_relevance / sum,
        pagerank: w.pagerank / sum,
        status: w.status / sum,
        impact: w.impact / sum,
        priority: w.priority / sum,
        recency: w.recency / sum,
    }
}

/// Go `Weights.validateComponents` (weights.go) — finite and non-negative,
/// checked before normalization can hide a negative behind a rescaled sum.
fn search_weights_validate_components(w: &bv_search::hybrid::Weights) -> Result<(), String> {
    for (name, value) in [
        ("text", w.text_relevance),
        ("pagerank", w.pagerank),
        ("status", w.status),
        ("impact", w.impact),
        ("priority", w.priority),
        ("recency", w.recency),
    ] {
        if !value.is_finite() {
            return Err(format!("weight {name:?} must be finite"));
        }
        if value < 0.0 {
            return Err("weights must be non-negative".to_string());
        }
    }
    Ok(())
}

/// Go `Weights.Validate` (weights.go) — components, the low-text-relevance
/// warning, then the 1.0 sum check with the 0.001 tolerance. Go logs the
/// warning through `log.Printf`; stderr is not part of the robot contract, so
/// it is emitted bare.
fn search_weights_validate(w: &bv_search::hybrid::Weights) -> Result<(), String> {
    search_weights_validate_components(w)?;
    if w.text_relevance < 0.1 {
        eprintln!(
            "WARNING: text weight {:.2} is very low; results may not match query",
            w.text_relevance
        );
    }
    let sum = search_weights_sum(w);
    if !sum.is_finite() {
        return Err("weights sum must be finite".to_string());
    }
    if (sum - 1.0).abs() > 0.001 {
        return Err(format!("weights must sum to 1.0, got {sum:.3}"));
    }
    Ok(())
}

/// Go `AdjustWeightsForQuery` (query_adjust.go) — floor a short query's text
/// relevance at 0.55 so an unrelated high-impact issue cannot outrank a
/// literal match, rescaling the graph weights into whatever is left.
///
/// Go ends its rescale path with `adjusted.Normalize()`. `bv-search`'s port
/// returns the rescaled weights without that final normalize, and the two
/// differ in the last ULP of every component (the rescaled sum is 1.0 only in
/// exact arithmetic), which is enough to change both the emitted `weights`
/// object and the `ranking_hash` that digests it. Re-applying `Normalize`
/// under exactly Go's own condition — a short query whose text weight was
/// below the floor — restores parity; every other Go path returns the input,
/// which is already normalized by the caller.
fn search_adjust_weights_for_query(
    weights: bv_search::hybrid::Weights,
    query: &str,
) -> bv_search::hybrid::Weights {
    let rescaled = bv_search::query::is_short_query(query)
        && weights.text_relevance < bv_search::query::SHORT_QUERY_MIN_TEXT_WEIGHT;
    let adjusted = bv_search::query::adjust_weights_for_query(weights, query);
    if rescaled {
        search_weights_normalize(adjusted)
    } else {
        adjusted
    }
}

/// Go `search.ParseWeightsJSON` (config.go:126) — all six keys are required,
/// no unknown keys, and the result must pass `Validate`. `bv-search`'s
/// `Weights` has no `Deserialize` impl (its own `Serialize` uses Rust field
/// names), so the payload is decoded here and the emitted object is assembled
/// by hand in Go's struct order.
fn search_parse_weights_json(raw: &str) -> Result<bv_search::hybrid::Weights, String> {
    const REQUIRED: [&str; 6] = [
        "text", "pagerank", "status", "impact", "priority", "recency",
    ];
    let payload: std::collections::BTreeMap<String, f64> =
        serde_json::from_str(raw).map_err(|e| format!("invalid weights JSON: {e}"))?;
    for key in REQUIRED {
        if !payload.contains_key(key) {
            return Err(format!("weights JSON missing {key:?}"));
        }
    }
    for key in payload.keys() {
        if !REQUIRED.contains(&key.as_str()) {
            return Err(format!("weights JSON has unknown key {key:?}"));
        }
    }
    let weights = bv_search::hybrid::Weights {
        text_relevance: payload["text"],
        pagerank: payload["pagerank"],
        status: payload["status"],
        impact: payload["impact"],
        priority: payload["priority"],
        recency: payload["recency"],
    };
    search_weights_validate(&weights)?;
    Ok(weights)
}

/// Go `search.Weights` JSON shape (weights.go struct tags) — `text`,
/// `pagerank`, `status`, `impact`, `priority`, `recency` in that order.
fn search_weights_json(w: &bv_search::hybrid::Weights) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("text".into(), serde_json::json!(w.text_relevance));
    m.insert("pagerank".into(), serde_json::json!(w.pagerank));
    m.insert("status".into(), serde_json::json!(w.status));
    m.insert("impact".into(), serde_json::json!(w.impact));
    m.insert("priority".into(), serde_json::json!(w.priority));
    m.insert("recency".into(), serde_json::json!(w.recency));
    serde_json::Value::Object(m)
}

/// Go `resolveSearchWeights` (search_output.go:144) — explicit weights report
/// the synthetic preset name `custom` so a consumer can tell a hand-tuned
/// ranking from a named one.
fn search_resolve_weights(cfg: &SearchConfig) -> (bv_search::hybrid::Weights, String) {
    if cfg.has_weights {
        return (cfg.weights, "custom".to_string());
    }
    (
        bv_search::hybrid::get_preset(&cfg.preset).unwrap_or(SEARCH_ZERO_WEIGHTS),
        cfg.preset.clone(),
    )
}

/// Go `search.NewHybridScorerAt`'s weight reconciliation
/// (hybrid_scorer_impl.go): the incoming weights are re-validated and
/// re-normalized, and anything that fails falls back to the `default` preset
/// instead of scoring with nonsense weights.
fn search_scorer_weights(w: bv_search::hybrid::Weights) -> bv_search::hybrid::Weights {
    let default_weights =
        bv_search::hybrid::get_preset("default").unwrap_or(bv_search::hybrid::Weights {
            text_relevance: 1.0,
            pagerank: 0.0,
            status: 0.0,
            impact: 0.0,
            priority: 0.0,
            recency: 0.0,
        });
    let mut normalized = default_weights;
    if search_weights_validate_components(&w).is_ok() {
        normalized = search_weights_normalize(w);
    }
    if search_weights_validate(&normalized).is_err() {
        normalized = default_weights;
    }
    normalized
}

/// Go `search.VectorSearchOptions` (vector_index.go). `min_score` is an
/// inclusive floor on the *raw* cosine similarity, applied before the lexical
/// boost, so a prefix match cannot be discarded before ranking and an exact
/// issue-ID query is filtered by the same threshold as everything else.
struct SearchOptions<'a> {
    /// `None` selects the whole index; an empty set selects nothing.
    eligible: Option<&'a std::collections::BTreeSet<String>>,
    exact_id: &'a str,
    min_score: Option<f64>,
    score_boosts: &'a std::collections::BTreeMap<String, f64>,
}

/// Go `search.SearchResult` plus the `ExactIDMatch` flag Go keeps off the wire
/// (`json:"-"`), which lets the hybrid stage promote the winner without
/// re-resolving a case-folded match from an already truncated list.
#[derive(Clone)]
struct ScoredHit {
    issue_id: String,
    score: f64,
    exact_id_match: bool,
}

/// Go `VectorIndex.SearchTopKWithOptions` (vector_index.go).
///
/// Go collects into a bounded heap ordered by `(score desc, issue_id asc)`.
/// Issue IDs are unique, so that comparator is a total order and a plain
/// sort-and-truncate produces the identical list.
fn search_top_k_with_options(
    idx: &bv_search::vector_index::VectorIndex,
    query: &[f32],
    k: usize,
    opts: &SearchOptions<'_>,
) -> Result<Vec<ScoredHit>, String> {
    let exact_id = opts.exact_id.trim();
    if let Some(min) = opts.min_score {
        if !min.is_finite() {
            return Err("minimum score must be finite".to_string());
        }
    }
    for (id, boost) in opts.score_boosts {
        if !boost.is_finite() || *boost < 0.0 {
            return Err(format!(
                "score boost for {id:?} must be finite and nonnegative"
            ));
        }
    }
    if k == 0 {
        return Ok(Vec::new());
    }
    if query.len() != idx.dim {
        return Err(format!(
            "query dim mismatch: {} != {}",
            query.len(),
            idx.dim
        ));
    }
    search_validate_finite(query).map_err(|e| format!("invalid query: {e}"))?;

    // Go clamps k to the index size, not to the eligible count.
    let ids = idx.sorted_ids();
    let k = k.min(ids.len());
    if k == 0 {
        return Ok(Vec::new());
    }

    let mut scored: Vec<ScoredHit> = Vec::new();
    let mut exact_case: Option<ScoredHit> = None;
    let mut folded: Option<ScoredHit> = None;
    let mut folded_matches = 0usize;
    for id in &ids {
        if let Some(eligible) = opts.eligible {
            if !eligible.contains(id) {
                continue;
            }
        }
        let Some(entry) = idx.get(id) else {
            continue;
        };
        let mut score = search_dot_f32(query, &entry.vector);
        if let Some(min) = opts.min_score {
            if score < min {
                continue;
            }
        }
        score += opts.score_boosts.get(id).copied().unwrap_or(0.0);
        let hit = ScoredHit {
            issue_id: id.clone(),
            score,
            exact_id_match: false,
        };
        // Only a case-insensitive match needs a copy kept for the exact-match
        // bookkeeping below; every other entry is just pushed.
        let case_folds = !exact_id.is_empty() && id.eq_ignore_ascii_case(exact_id);
        scored.push(hit);
        if case_folds {
            let recorded = scored.last().expect("just pushed").clone();
            if id == exact_id {
                exact_case = Some(recorded);
            } else {
                folded = Some(recorded);
                folded_matches += 1;
            }
        }
    }
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.issue_id.cmp(&b.issue_id))
    });
    scored.truncate(k);
    let mut results = scored;

    // An exact-case match always wins; a case-folded match is promoted only
    // when it is unambiguous, because issue IDs are opaque and a query of
    // `bv-1` must not pick a winner between `BV-1` and `bv-10`.
    let exact = match exact_case {
        Some(hit) => Some(hit),
        None if folded_matches == 1 => folded,
        None => None,
    };
    let Some(mut exact) = exact else {
        return Ok(results);
    };
    exact.exact_id_match = true;
    match results.iter().position(|r| r.issue_id == exact.issue_id) {
        Some(0) => results[0] = exact,
        Some(i) => {
            results[0..=i].rotate_right(1);
            results[0] = exact;
        }
        None => {
            if results.len() < k {
                results.push(exact);
            } else {
                let last = results.len() - 1;
                results[last] = exact;
            }
        }
    }
    Ok(results)
}

/// Go `dotFloat32` (vector_index.go) — f64 accumulator over f32 operands.
fn search_dot_f32(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum()
}

/// Go `validateFiniteVector` (vector_index.go).
fn search_validate_finite(vec: &[f32]) -> Result<(), String> {
    for (i, v) in vec.iter().enumerate() {
        if !v.is_finite() {
            return Err(format!("vector component {i} must be finite"));
        }
    }
    Ok(())
}

/// Go `search.IssueMetrics` (metrics_cache.go) — the graph-derived inputs the
/// hybrid scorer mixes into the text score.
struct SearchIssueMetrics {
    pagerank: f64,
    status: String,
    priority: i32,
    blocker_count: usize,
    updated_at: Option<jiff::Timestamp>,
}

/// Go `defaultPageRank` (metrics_cache_impl.go) — the neutral PageRank for an
/// issue the graph has no entry for (isolated after scoping).
const SEARCH_DEFAULT_PAGERANK: f64 = 0.5;

/// Go `metricsCache` after `Refresh` (metrics_cache_impl.go): one entry per
/// indexed issue, PageRank defaulted when absent, and the corpus-wide maximum
/// blocker count that `normalizeImpact` divides by.
struct SearchMetricsCache {
    metrics: std::collections::BTreeMap<String, SearchIssueMetrics>,
    max_blocker_count: usize,
}

fn search_build_metrics_cache(issues: &[bv_core::model::Issue]) -> SearchMetricsCache {
    let graph = std::sync::Arc::new(bv_analysis::build_graph(issues));
    let phase1 = bv_analysis::analyze_phase1(&graph);
    let budget = bv_analysis::AnalysisBudget {
        density: phase1.density,
        ..Default::default()
    };
    let (_status, phase2) = bv_analysis::analyze_phase2_blocking(graph, &budget);
    let page_rank = phase2.page_rank.unwrap_or_default();

    let mut metrics = std::collections::BTreeMap::new();
    let mut max_blocker_count = 0usize;
    for issue in issues {
        let blocker_count = phase1.in_degree.get(&issue.id).copied().unwrap_or(0);
        max_blocker_count = max_blocker_count.max(blocker_count);
        metrics.insert(
            issue.id.clone(),
            SearchIssueMetrics {
                pagerank: page_rank
                    .get(&issue.id)
                    .copied()
                    .unwrap_or(SEARCH_DEFAULT_PAGERANK),
                status: issue.status.as_str().to_string(),
                priority: issue.priority,
                blocker_count,
                updated_at: issue
                    .updated_at
                    .as_deref()
                    .and_then(|s| s.parse::<jiff::Timestamp>().ok()),
            },
        );
    }
    SearchMetricsCache {
        metrics,
        max_blocker_count,
    }
}

/// Go `normalizeRecencyAt` (normalizers.go). A zero `updated_at` scores the
/// neutral 0.5, a future timestamp scores 1.0, otherwise exponential decay
/// with a 30-day constant measured against the scorer's pinned clock.
fn search_normalize_recency_at(updated_at: Option<jiff::Timestamp>, now: jiff::Timestamp) -> f64 {
    let Some(updated_at) = updated_at else {
        return 0.5;
    };
    // Go: `now.Sub(updatedAt).Hours() / 24`, i.e. a fractional day count
    // measured from the scorer's pinned clock rather than the wall clock.
    let days = (now.as_nanosecond() - updated_at.as_nanosecond()) as f64 / 86_400_000_000_000.0;
    if days < 0.0 {
        return 1.0;
    }
    let score = (-days / 30.0).exp();
    if score > 1.0 {
        1.0
    } else {
        score
    }
}

/// Go `search.HybridScore` (hybrid_scorer.go).
struct SearchHybridRow {
    issue_id: String,
    final_score: f64,
    text_score: f64,
    /// `None` where Go leaves the map nil, which `omitempty` then drops.
    components: Option<std::collections::BTreeMap<String, f64>>,
}

/// Go `hybridScorer.Score` (hybrid_scorer_impl.go). An issue the metrics cache
/// does not know scores as pure text with no component breakdown, and Go skips
/// normalizing any component whose weight is zero while still emitting its key
/// — so a zeroed component reads 0.0 rather than the issue's real value.
fn search_hybrid_score(
    issue_id: &str,
    text_score: f64,
    weights: &bv_search::hybrid::Weights,
    cache: &SearchMetricsCache,
    reference_time: jiff::Timestamp,
) -> Result<SearchHybridRow, String> {
    if issue_id.is_empty() {
        return Err("issueID is required".to_string());
    }
    let Some(metrics) = cache.metrics.get(issue_id) else {
        return Ok(SearchHybridRow {
            issue_id: issue_id.to_string(),
            final_score: text_score,
            text_score,
            components: None,
        });
    };

    let status = if weights.status > 0.0 {
        bv_search::hybrid::ComponentScores::normalize_status(&metrics.status)
    } else {
        0.0
    };
    let priority = if weights.priority > 0.0 {
        bv_search::hybrid::ComponentScores::normalize_priority(metrics.priority)
    } else {
        0.0
    };
    let impact = if weights.impact > 0.0 {
        bv_search::hybrid::ComponentScores::normalize_impact(
            metrics.blocker_count,
            cache.max_blocker_count,
        )
    } else {
        0.0
    };
    let recency = if weights.recency > 0.0 {
        search_normalize_recency_at(metrics.updated_at, reference_time)
    } else {
        0.0
    };

    let components_struct = bv_search::hybrid::ComponentScores {
        pagerank: metrics.pagerank,
        status,
        impact,
        priority,
        recency,
    };
    let final_score = bv_search::hybrid::hybrid_score(text_score, weights, &components_struct);
    // Go's map marshals with sorted keys.
    let mut components = std::collections::BTreeMap::new();
    components.insert("impact".to_string(), impact);
    components.insert("pagerank".to_string(), metrics.pagerank);
    components.insert("priority".to_string(), priority);
    components.insert("recency".to_string(), recency);
    components.insert("status".to_string(), status);
    Ok(SearchHybridRow {
        issue_id: issue_id.to_string(),
        final_score,
        text_score,
        components: Some(components),
    })
}

/// Go `buildHybridScores` (search_output.go:156) — score every candidate, then
/// sort by final score descending with the issue ID as the tiebreak.
fn search_build_hybrid_scores(
    results: &[ScoredHit],
    weights: &bv_search::hybrid::Weights,
    cache: &SearchMetricsCache,
    reference_time: jiff::Timestamp,
) -> Result<Vec<SearchHybridRow>, String> {
    let mut out = Vec::with_capacity(results.len());
    for hit in results {
        out.push(search_hybrid_score(
            &hit.issue_id,
            hit.score,
            weights,
            cache,
            reference_time,
        )?);
    }
    out.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.issue_id.cmp(&b.issue_id))
    });
    Ok(out)
}

/// Go `promoteExactHybridResult` (search_output.go:195) — hoist the row whose
/// issue ID equals the promoted exact match verbatim, as
/// `SearchTopKWithOptions` already did for the text list.
fn search_promote_exact_hybrid(exact_id: &str, rows: &mut [SearchHybridRow]) {
    if exact_id.is_empty() {
        return;
    }
    if let Some(i) = rows.iter().position(|r| r.issue_id == exact_id) {
        if i > 0 {
            rows[0..=i].rotate_right(1);
        }
    }
}

/// Go's `time.Time` JSON encoding is RFC3339 with trailing zeros trimmed from
/// the fractional part, and the whole part dropped when it is zero.
fn search_go_rfc3339_nano(ts: jiff::Timestamp) -> String {
    let rendered = ts.to_string();
    let Some(dot) = rendered.find('.') else {
        return rendered;
    };
    let (head, rest) = rendered.split_at(dot);
    let digits = rest[1..].strip_suffix('Z').unwrap_or(&rest[1..]);
    let trimmed = digits.trim_end_matches('0');
    if trimmed.is_empty() {
        format!("{head}Z")
    } else {
        format!("{head}.{trimmed}Z")
    }
}

/// The retrieval configuration Go's `searchRankingHash` digests
/// (search_output.go:48). It deliberately excludes volatile invocation state —
/// `generated_at`, `loaded` and the per-result scores — so the same search
/// over the same corpus and settings hashes identically across invocations.
struct SearchRankingIdentity<'a> {
    index_data_hash: &'a str,
    candidate_hash: &'a str,
    source_path: &'a str,
    source_kind: &'a str,
    as_of_commit: &'a str,
    query: &'a str,
    mode: &'a str,
    preset: &'a str,
    weights: Option<bv_search::hybrid::Weights>,
    min_score: Option<f64>,
    limit: usize,
    provider: &'a str,
    model: &'a str,
    dim: usize,
    ranking_time: Option<jiff::Timestamp>,
    scope: &'a serde_json::Value,
}

/// Go `searchRankingHash` (search_output.go:48) — sha256 over the canonical
/// identity of the retrieval configuration. Go marshals a `map[string]any`,
/// which `encoding/json` emits with keys in sorted order, so the keys below
/// are inserted alphabetically.
fn search_ranking_hash(identity: &SearchRankingIdentity<'_>) -> String {
    let mut m = serde_json::Map::new();
    m.insert(
        "as_of_commit".into(),
        serde_json::json!(identity.as_of_commit),
    );
    m.insert(
        "candidate_hash".into(),
        serde_json::json!(identity.candidate_hash),
    );
    m.insert("dim".into(), serde_json::json!(identity.dim));
    m.insert(
        "index_data_hash".into(),
        serde_json::json!(identity.index_data_hash),
    );
    m.insert("limit".into(), serde_json::json!(identity.limit));
    m.insert(
        "min_score".into(),
        identity
            .min_score
            .map_or(serde_json::Value::Null, serde_json::Value::from),
    );
    m.insert("mode".into(), serde_json::json!(identity.mode));
    m.insert("model".into(), serde_json::json!(identity.model));
    m.insert("preset".into(), serde_json::json!(identity.preset));
    m.insert("provider".into(), serde_json::json!(identity.provider));
    m.insert("query".into(), serde_json::json!(identity.query));
    m.insert(
        "ranking_time".into(),
        identity.ranking_time.map_or(serde_json::Value::Null, |ts| {
            serde_json::json!(search_go_rfc3339_nano(ts))
        }),
    );
    m.insert("scope".into(), identity.scope.clone());
    m.insert(
        "source_kind".into(),
        serde_json::json!(identity.source_kind),
    );
    m.insert(
        "source_path".into(),
        serde_json::json!(identity.source_path),
    );
    m.insert(
        "weights".into(),
        identity
            .weights
            .map_or(serde_json::Value::Null, |w| search_weights_json(&w)),
    );
    let encoded = go_json_string(&serde_json::Value::Object(m));
    bv_search::vector_index::compute_content_hash(&encoded)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Go `--robot-search` (main.go:2814-3037 plus `cmd/bv/search_output.go`).
///
/// The handler drives the ported `bv-search` library the way Go drives
/// `pkg/search`: `EmbeddingConfigFromEnv` → `NewEmbedderFromConfig` →
/// `LoadOrNewVectorIndex` → `DocumentsFromIssues` → `SyncVectorIndex` →
/// `SearchTopKWithOptions` → (hybrid only) `NewHybridScorerAt` → the
/// `robotSearchOutput` struct in Go's field order. `--search QUERY` is
/// required (modifier-requires table), `--search-limit` caps results at 10.
///
/// One deliberate deviation: an unknown `--search-preset` exits 2 and names the
/// flag, where Go exits 1 with a bare preset name — the repo's exit-code
/// contract puts usage errors at 2 and `crates/bv/tests/cli_behavior.rs` pins
/// the Rust wording. Everything else in this function copies Go's text and
/// behaviour.
fn run_robot_search(args: &[String]) -> ExitCode {
    let query = search_flag(args, "search").unwrap_or_default();
    if query.trim().is_empty() {
        eprintln!("Error: --search requires a non-empty query");
        return ExitCode::from(2);
    }

    // Go main.go:2817 — the threshold is parsed before the search config and
    // is a usage error, not a runtime failure.
    let min_score =
        match search_parse_min_score(&search_flag(args, "search-min-score").unwrap_or_default()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(2);
            }
        };

    let mode_flag = search_flag(args, "search-mode").unwrap_or_default();
    let preset_flag = search_flag(args, "search-preset").unwrap_or_default();
    let weights_flag = search_flag(args, "search-weights").unwrap_or_default();
    let cfg = match search_resolve_config(&mode_flag, &preset_flag, &weights_flag) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error: {}", e.message);
            return ExitCode::from(e.exit_code);
        }
    };

    let embed_cfg = search_embedding_config_from_env();
    let dim = match search_embedder_dim(&embed_cfg) {
        Ok(dim) => dim,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    // Go main.go:2838 — one `os.Getwd` feeds both the loader and the index
    // path, so the two can never disagree.
    let project_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let as_of = search_flag(args, "as-of").filter(|s| !s.is_empty());
    let (issues_for_search, hash, as_of_commit) =
        match load_issues_auto(&project_dir, as_of.as_deref()) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };

    // Go main.go:2770/2789 — the indexed corpus is the *unscoped* issue set
    // while the ranked candidates are the scoped set's core issues, so a
    // `--label` search indexes everything but only ranks the label.
    let (label, _recipe, _repo) = active_scope_flags();
    let scoped_issues = apply_label_scope(&issues_for_search);
    let (candidate_ids, _all_ids) =
        bv_analysis::label_health::label_scope_ids(&label, &issues_for_search);
    let candidates: std::collections::BTreeSet<String> = candidate_ids.iter().cloned().collect();
    let selected_issues: Vec<bv_core::model::Issue> = scoped_issues
        .iter()
        .filter(|i| candidates.contains(&i.id))
        .cloned()
        .collect();
    let eligible: std::collections::BTreeSet<String> =
        selected_issues.iter().map(|i| i.id.clone()).collect();

    let mut index_path = bv_search::index_sync::default_index_path(&project_dir, dim);
    if let Some(resolved) = as_of_commit.as_deref().filter(|s| !s.is_empty()) {
        // Go main.go:2841 — a historical search gets its own index file so it
        // cannot overwrite the live one.
        let historical = index_path.parent().unwrap_or(&project_dir).join(format!(
            "historical-{resolved}-{}",
            index_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("index.bvvi")
        ));
        index_path = historical;
    }

    let (mut idx, loaded) = bv_search::index_sync::load_or_new(&index_path, dim);
    let mut docs = std::collections::BTreeMap::new();
    for issue in &issues_for_search {
        if issue.id.is_empty() {
            continue;
        }
        docs.insert(
            issue.id.clone(),
            bv_search::query::issue_document(
                &issue.id,
                &issue.title,
                &issue.labels,
                &issue.description,
            ),
        );
    }
    let sync_stats =
        match bv_search::index_sync::sync_index(&mut idx, &docs, |texts: &[String]| {
            texts
                .iter()
                .map(|t| bv_search::embedder::hash_embed(t, dim))
                .collect()
        }) {
            Ok(stats) => stats,
            Err(e) => {
                eprintln!("Error building semantic index: {e}");
                return ExitCode::from(1);
            }
        };
    if !loaded || sync_stats.changed() {
        if let Err(e) = idx.save(&index_path) {
            eprintln!("Error saving semantic index: {e}");
            return ExitCode::from(1);
        }
    }

    let query_vec = bv_search::embedder::hash_embed(&query, dim);

    // Go main.go:2887 — `--search-limit` defaults to 10 and a non-positive
    // value falls back to it. `--robot-max-results` is a Rust-side alias the
    // help text advertises; Go applies it elsewhere, never to the search limit.
    let limit = search_flag(args, "search-limit")
        .or_else(|| search_flag(args, "robot-max-results"))
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(10) as usize;
    let hybrid_mode = cfg.mode == "hybrid";
    // Hybrid widens the candidate pool so the re-ranker has something to
    // reorder; short queries widen it further (Go `HybridCandidateLimit`).
    let fetch_limit = if hybrid_mode {
        bv_search::query::hybrid_candidate_limit(limit, selected_issues.len(), &query)
    } else {
        limit
    };

    let mut score_boosts: std::collections::BTreeMap<String, f64> =
        std::collections::BTreeMap::new();
    if bv_search::query::is_short_query(&query) {
        for (id, doc) in &docs {
            if !eligible.contains(id) {
                continue;
            }
            let boost = bv_search::query::short_query_lexical_boost(&query, doc);
            if boost > 0.0 {
                score_boosts.insert(id.clone(), boost);
            }
        }
    }

    let results = match search_top_k_with_options(
        &idx,
        &query_vec,
        fetch_limit,
        &SearchOptions {
            eligible: Some(&eligible),
            exact_id: &query,
            min_score,
            score_boosts: &score_boosts,
        },
    ) {
        Ok(results) => results,
        Err(e) => {
            eprintln!("Error searching index: {e}");
            return ExitCode::from(1);
        }
    };
    let exact_id = results
        .iter()
        .find(|r| r.exact_id_match)
        .map(|r| r.issue_id.clone())
        .unwrap_or_default();

    let mut title_by_id: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for issue in &issues_for_search {
        title_by_id.insert(issue.id.clone(), issue.title.clone());
    }

    let mut ranking_time: Option<jiff::Timestamp> = None;
    let mut resolved_preset = String::new();
    let mut resolved_weights: Option<bv_search::hybrid::Weights> = None;
    let mut hybrid_rows: Vec<SearchHybridRow> = Vec::new();
    if hybrid_mode {
        let (weights, preset) = search_resolve_weights(&cfg);
        let weights = search_weights_normalize(weights);
        let weights = search_adjust_weights_for_query(weights, &query);
        resolved_preset = preset;
        resolved_weights = Some(weights);

        let cache = search_build_metrics_cache(&issues_for_search);
        // The reference clock is pinned and published as `ranking_time` so a
        // cached hybrid ranking does not drift as wall time advances.
        let reference_time = robot_now();
        ranking_time = Some(reference_time);
        let scorer_weights = search_scorer_weights(weights);
        match search_build_hybrid_scores(&results, &scorer_weights, &cache, reference_time) {
            Ok(mut rows) => {
                search_promote_exact_hybrid(&exact_id, &mut rows);
                rows.truncate(limit);
                hybrid_rows = rows;
            }
            Err(e) => {
                eprintln!("Error scoring hybrid results: {e}");
                return ExitCode::from(1);
            }
        }
    }

    let index_data_hash = bv_core::data_hash::compute_data_hash(&issues_for_search);
    let candidate_hash = bv_core::data_hash::compute_data_hash(&selected_issues);
    let mut payload = match full_envelope_for(&hash, &issues_for_search) {
        serde_json::Value::Object(map) => map,
        other => other.as_object().cloned().unwrap_or_default(),
    };
    let env_str = |key: &str| -> String {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let source_path = env_str("source_path");
    let source_kind = env_str("source_kind");
    let scope = payload
        .get("scope")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let ranking_hash = search_ranking_hash(&SearchRankingIdentity {
        index_data_hash: &index_data_hash,
        candidate_hash: &candidate_hash,
        source_path: &source_path,
        source_kind: &source_kind,
        as_of_commit: as_of_commit.as_deref().unwrap_or_default(),
        query: &query,
        mode: &cfg.mode,
        preset: &resolved_preset,
        weights: resolved_weights,
        min_score,
        limit,
        provider: &embed_cfg.provider,
        model: &embed_cfg.model,
        dim,
        ranking_time,
        scope: &scope,
    });

    // Go `robotSearchResult` field order (search_output.go:18):
    // issue_id, score, text_score (omitempty), title (omitempty),
    // component_scores (omitempty).
    let mut result_rows: Vec<serde_json::Value> = Vec::new();
    if hybrid_mode {
        for row in &hybrid_rows {
            let mut m = serde_json::Map::new();
            m.insert("issue_id".into(), serde_json::json!(row.issue_id));
            m.insert("score".into(), serde_json::json!(row.final_score));
            if row.text_score != 0.0 {
                m.insert("text_score".into(), serde_json::json!(row.text_score));
            }
            let title = title_by_id.get(&row.issue_id).cloned().unwrap_or_default();
            if !title.is_empty() {
                m.insert("title".into(), serde_json::json!(title));
            }
            if let Some(components) = &row.components {
                if !components.is_empty() {
                    let mut cm = serde_json::Map::new();
                    for (key, value) in components {
                        cm.insert(key.clone(), serde_json::json!(value));
                    }
                    m.insert("component_scores".into(), serde_json::Value::Object(cm));
                }
            }
            result_rows.push(serde_json::Value::Object(m));
        }
    } else {
        for hit in &results {
            let mut m = serde_json::Map::new();
            m.insert("issue_id".into(), serde_json::json!(hit.issue_id));
            m.insert("score".into(), serde_json::json!(hit.score));
            let title = title_by_id.get(&hit.issue_id).cloned().unwrap_or_default();
            if !title.is_empty() {
                m.insert("title".into(), serde_json::json!(title));
            }
            result_rows.push(serde_json::Value::Object(m));
        }
    }

    // Go `search.IndexSyncStats` field order (index_sync.go).
    let mut index_stats = serde_json::Map::new();
    index_stats.insert("total".into(), serde_json::json!(sync_stats.total));
    index_stats.insert("added".into(), serde_json::json!(sync_stats.added));
    index_stats.insert("updated".into(), serde_json::json!(sync_stats.updated));
    index_stats.insert("removed".into(), serde_json::json!(sync_stats.removed));
    index_stats.insert("skipped".into(), serde_json::json!(sync_stats.skipped));
    index_stats.insert("embedded".into(), serde_json::json!(sync_stats.embedded));

    // Go `robotSearchOutput` field order (search_output.go:26): the embedded
    // envelope, then these. `ranking_time`, `min_score`, `model`, `preset` and
    // `weights` are `omitempty` and appear only when the search produced them.
    payload.insert("index_data_hash".into(), serde_json::json!(index_data_hash));
    payload.insert("candidate_hash".into(), serde_json::json!(candidate_hash));
    payload.insert("ranking_hash".into(), serde_json::json!(ranking_hash));
    if let Some(ts) = ranking_time {
        payload.insert(
            "ranking_time".into(),
            serde_json::json!(search_go_rfc3339_nano(ts)),
        );
    }
    if let Some(v) = min_score {
        payload.insert("min_score".into(), serde_json::json!(v));
    }
    payload.insert("query".into(), serde_json::json!(query));
    payload.insert("provider".into(), serde_json::json!(embed_cfg.provider));
    if !embed_cfg.model.is_empty() {
        payload.insert("model".into(), serde_json::json!(embed_cfg.model));
    }
    payload.insert("dim".into(), serde_json::json!(dim));
    payload.insert(
        "index_path".into(),
        serde_json::json!(index_path.to_string_lossy()),
    );
    payload.insert("index".into(), serde_json::Value::Object(index_stats));
    payload.insert("loaded".into(), serde_json::json!(loaded));
    payload.insert("limit".into(), serde_json::json!(limit));
    payload.insert("mode".into(), serde_json::json!(cfg.mode));
    if hybrid_mode {
        payload.insert("preset".into(), serde_json::json!(resolved_preset));
        if let Some(w) = resolved_weights {
            payload.insert("weights".into(), search_weights_json(&w));
        }
    }
    payload.insert("results".into(), serde_json::Value::Array(result_rows));
    let usage_hints: &[&str] = if hybrid_mode {
        &[
            "jq '.results[] | {id: .issue_id, score: .score, text: .text_score}' - Extract scores",
            "jq '.results[] | {id: .issue_id, components: .component_scores}' - Hybrid breakdown",
            "jq '.index' - Index update stats (added/updated/removed/embedded)",
        ]
    } else {
        &[
            "jq '.results[] | {id: .issue_id, score: .score, title: .title}' - Extract results",
            "jq '.index' - Index update stats (added/updated/removed/embedded)",
        ]
    };
    payload.insert("usage_hints".into(), serde_json::json!(usage_hints));
    emit_json(&serde_json::Value::Object(payload))
}

/// Go `handleRobotCausality` — `--robot-causality <bead-id>`.
/// Go `handleRobotCausality` — build the same correlation report
/// `--robot-history` produces, but with the target's committed constraint
/// observations retained, then render the causal chain and its insights.
///
/// The `data_hash` in the output is the report's own bead fingerprint, not the
/// issue-corpus hash the loader computes: Go's `CausalityResult` carries
/// `hr.DataHash` and `withEnvelope` lets it win over the envelope's own value.
fn run_robot_causality(args: &[String]) -> ExitCode {
    let bead_id = args
        .iter()
        .position(|a| a == "--robot-causality")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Err(e) = validate_correlation_repository(&cwd) {
        eprintln!("Error: {e}");
        return ExitCode::from(1);
    }
    let (issues, _hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    // Go: CorrelatorOptions{Limit: 500, CausalityBeadID: <bead>} overridden by
    // --history-limit, with --history-since bounding the window.
    let mut opts = bv_correlation::history::HistoryOptions {
        causality_bead_id: bead_id.clone(),
        limit: 500,
        ..Default::default()
    };
    if let Some(limit) = history_flag_value("history-limit") {
        match limit.trim().parse::<i64>() {
            Ok(v) => opts.limit = v,
            Err(_) => {
                eprintln!("Error: invalid --history-limit: {limit}");
                return ExitCode::from(2);
            }
        }
    }
    if let Some(since) = history_flag_value("history-since") {
        match parse_relative_time(&since) {
            Ok(Some(ts)) => opts.since = Some(ts),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error: parsing --history-since: {e}");
                return ExitCode::from(1);
            }
        }
    }

    // Go builds BeadInfo from the loaded issues, in load order.
    let beads: Vec<bv_correlation::history::BeadInfo> = issues
        .iter()
        .map(|i| bv_correlation::history::BeadInfo {
            id: i.id.clone(),
            title: i.title.clone(),
            status: i.status.as_str().to_string(),
        })
        .collect();

    // One frozen instant for both the chain's open end and the result stamp, so
    // `end_time` and `generated_at` cannot disagree.
    let now = jiff_now();
    let report =
        match bv_correlation::history::build_history_report(&cwd, &beads, &opts, None, now.clone())
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: generating history report: {e}");
                return ExitCode::from(1);
            }
        };

    let Some(history) = report.histories.get(&bead_id) else {
        eprintln!("Bead not found: {bead_id}");
        return ExitCode::from(1);
    };

    let mut blocker_titles = std::collections::BTreeMap::new();
    for issue in &issues {
        blocker_titles.insert(issue.id.clone(), issue.title.clone());
    }
    let caus_opts = bv_correlation::causality::CausalityOptions {
        include_commits: true,
        blocker_titles,
    };
    // Go's `robotNow()` is the reference instant, so the chain's open end and
    // the result stamp cannot disagree.
    let Ok(now) = bv_correlation::causality::GoTime::parse(&now) else {
        eprintln!("Error: invalid reference instant");
        return ExitCode::from(1);
    };
    let Some(mut result) = bv_correlation::causality::build_causality_chain_at(
        history,
        report.causal_history.as_ref(),
        &caus_opts,
        &now,
    ) else {
        eprintln!("Bead not found: {bead_id}");
        return ExitCode::from(1);
    };
    result.data_hash = report.data_hash.clone();

    // Go builds the envelope from the loader's source authority (whose
    // per-source data_hash is the full-file sha256) and only then substitutes
    // the report's bead fingerprint at the top level, so `authority_hash` is
    // computed over the file hash while `data_hash`/`scope_hash` use the
    // report's. Same shape as --robot-history.
    let file_hash = bv_core::data_hash::compute_data_hash(&issues);
    let mut payload = full_envelope_for(&file_hash, &issues);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("data_hash".into(), serde_json::json!(result.data_hash));
        let mut ids: Vec<String> = issues.iter().map(|i| i.id.clone()).collect();
        ids.sort();
        let scope_hash = bv_robot::scope_hash("", "", "", &result.data_hash, &ids);
        if !scope_hash.is_empty() {
            obj.insert("scope_hash".into(), serde_json::json!(scope_hash));
        }
    }
    payload["chain"] = serde_json::to_value(&result.chain).unwrap_or_default();
    payload["insights"] = result.insights_value();

    // Go's `withEnvelope` returns a `map[string]json.RawMessage`, and
    // `encoding/json` emits map keys in sorted order. So --robot-causality is
    // the one robot command whose TOP-LEVEL keys are alphabetical, while the
    // nested chain/insights/authority objects keep their struct field order.
    // Rebuilding the outer object in key order reproduces that exactly; the
    // commands that embed the envelope as a struct keep insertion order and
    // must not be sorted.
    let sorted = match payload.as_object() {
        Some(obj) => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::with_capacity(keys.len());
            for k in keys {
                out.insert(k.clone(), obj[k].clone());
            }
            serde_json::Value::Object(out)
        }
        None => payload.clone(),
    };
    emit_json(&sorted)
}

/// Go `handleRobotRelated` — `--robot-related <bead-id>`.
fn run_robot_related(args: &[String]) -> ExitCode {
    let bead_id = args
        .iter()
        .position(|a| a == "--robot-related")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let max_results: usize = args
        .iter()
        .position(|a| a == "--related-max-results")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    if !issues.iter().any(|i| i.id == bead_id) {
        eprintln!("Bead not found: {bead_id}");
        return ExitCode::from(1);
    }
    let network = bv_correlation::network::build_network(&issues, &report);
    let sub = bv_correlation::network::sub_network(&network, &bead_id, 2);

    let mut related: Vec<serde_json::Value> = sub
        .edges
        .iter()
        .filter(|e| e.from == bead_id || e.to == bead_id)
        .map(|e| {
            let other = if e.from == bead_id { &e.to } else { &e.from };
            serde_json::json!({
                "bead_id": other,
                "title": sub.nodes.get(other).map(|n| n.title.clone()).unwrap_or_default(),
                "relation_type": e.edge_type,
                "weight": e.weight,
                "shared": e.shared,
            })
        })
        .collect();
    related.sort_by(|a, b| b["weight"].as_u64().cmp(&a["weight"].as_u64()));
    related.truncate(max_results);

    let mut payload = full_envelope_for(&hash, &issues);
    payload["bead_id"] = serde_json::json!(bead_id);
    payload["related"] = serde_json::Value::Array(related);
    emit_json(&payload)
}

/// Go `handleRobotImpactNetwork` — `--robot-impact-network <bead-id|all>`.
fn run_robot_impact_network(args: &[String]) -> ExitCode {
    let target = args
        .iter()
        .position(|a| a == "--robot-impact-network")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let depth: usize = args
        .iter()
        .position(|a| a == "--network-depth")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .clamp(1, 3);

    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let network = bv_correlation::network::build_network(&issues, &report);

    let result = if target.is_empty() || target == "all" {
        network
    } else {
        if !network.nodes.contains_key(&target) {
            eprintln!("Bead not found in network: {target}");
            return ExitCode::from(1);
        }
        bv_correlation::network::sub_network(&network, &target, depth)
    };

    let mut payload = full_envelope_for(&hash, &issues);
    payload["network"] = serde_json::to_value(&result).unwrap_or_default();
    payload["node_count"] = serde_json::json!(result.nodes.len());
    payload["edge_count"] = serde_json::json!(result.edges.len());
    emit_json(&payload)
}

/// Go `robot-sprint-list` — loads `.beads/sprints.jsonl` and emits all sprints.
fn run_robot_sprint_list() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let sprints = match bv_core::sprint::load_sprints(&cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let active_id = sprints.iter().find(|s| s.is_active()).map(|s| s.id.clone());
    let mut payload = full_envelope_for(&hash, &issues);
    payload["sprint_count"] = serde_json::json!(sprints.len());
    payload["sprints"] = serde_json::to_value(&sprints).unwrap_or_default();
    if let Some(id) = &active_id {
        payload["active_sprint_id"] = serde_json::json!(id);
    }
    payload["issue_count"] = serde_json::json!(issues.len());
    emit_json(&payload)
}

/// Go `robot-sprint-show` — `--robot-sprint-show <sprint-id>`.
fn run_robot_sprint_show(args: &[String]) -> ExitCode {
    let sprint_id = args
        .iter()
        .position(|a| a == "--robot-sprint-show")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let sprints = match bv_core::sprint::load_sprints(&cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let Some(sprint) = sprints.iter().find(|s| s.id == sprint_id) else {
        eprintln!("Sprint not found: {sprint_id}");
        return ExitCode::from(1);
    };
    let sprint_issues: Vec<&serde_json::Value> = Vec::new(); // populated below
    let sprint_issue_ids: Vec<&str> = sprint.bead_ids.iter().map(|s| s.as_str()).collect();
    let issue_details: Vec<serde_json::Value> = sprint_issue_ids
        .iter()
        .filter_map(|id| {
            issues.iter().find(|i| &i.id == id).map(|i| {
                serde_json::json!({
                    "id": i.id,
                    "title": i.title,
                    "status": i.status.as_str(),
                    "priority": i.priority,
                })
            })
        })
        .collect();
    let open_count = issue_details
        .iter()
        .filter(|d| d["status"] != "closed" && d["status"] != "tombstone")
        .count();
    let closed_count = issue_details.len() - open_count;
    let mut payload = full_envelope_for(&hash, &issues);
    payload["sprint"] = serde_json::to_value(sprint).unwrap_or_default();
    payload["issues"] = serde_json::Value::Array(issue_details);
    payload["open_count"] = serde_json::json!(open_count);
    payload["closed_count"] = serde_json::json!(closed_count);
    let _ = sprint_issues; // suppressed unused
    emit_json(&payload)
}

/// Go `robot-burndown` — `--robot-burndown [--burndown-sprint <id>]`.
fn run_robot_burndown(args: &[String]) -> ExitCode {
    let target_sprint_id = args
        .iter()
        .position(|a| a == "--burndown-sprint")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let sprints = match bv_core::sprint::load_sprints(&cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let target = if let Some(id) = &target_sprint_id {
        sprints.iter().find(|s| &s.id == id)
    } else {
        sprints.iter().find(|s| s.is_active())
    };
    let Some(sprint) = target else {
        eprintln!(
            "No {} sprint found",
            if target_sprint_id.is_some() {
                "matching"
            } else {
                "active"
            }
        );
        return ExitCode::from(1);
    };
    let now = robot_now();
    let (points, total) = bv_core::sprint::calculate_burndown(sprint, &issues, now);
    let mut payload = full_envelope_for(&hash, &issues);
    payload["sprint"] = serde_json::to_value(sprint).unwrap_or_default();
    payload["total_issues"] = serde_json::json!(total);
    payload["points"] = serde_json::to_value(&points).unwrap_or_default();
    emit_json(&payload)
}

/// Go `robot-forecast` — `--robot-forecast [--forecast-sprint <id>]`.
fn run_robot_forecast(args: &[String]) -> ExitCode {
    let target_sprint_id = args
        .iter()
        .position(|a| a == "--forecast-sprint")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let sprints = match bv_core::sprint::load_sprints(&cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let target = if let Some(id) = &target_sprint_id {
        sprints.iter().find(|s| &s.id == id)
    } else {
        sprints.iter().find(|s| s.is_active())
    };
    let Some(sprint) = target else {
        eprintln!(
            "No {} sprint found",
            if target_sprint_id.is_some() {
                "matching"
            } else {
                "active"
            }
        );
        return ExitCode::from(1);
    };
    let now = robot_now();
    let forecast = bv_core::sprint::estimate_forecast(sprint, &issues, now);
    let mut payload = full_envelope_for(&hash, &issues);
    payload["sprint"] = serde_json::to_value(sprint).unwrap_or_default();
    match forecast {
        Some(f) => {
            payload["forecast"] = serde_json::to_value(&f).unwrap_or_default();
        }
        None => {
            payload["forecast"] = serde_json::json!(null);
            payload["message"] =
                serde_json::json!("all sprint issues are closed — no forecast needed");
        }
    }
    emit_json(&payload)
}

/// Go `robot-capacity` — `--robot-capacity [--capacity-label <label>]`.
fn run_robot_capacity(args: &[String]) -> ExitCode {
    let label = args
        .iter()
        .position(|a| a == "--capacity-label")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let filtered: Vec<&bv_core::model::Issue> = if let Some(ref lbl) = label {
        issues
            .iter()
            .filter(|i| i.labels.iter().any(|l| l == lbl))
            .collect()
    } else {
        issues.iter().filter(|i| !i.status.is_closed()).collect()
    };
    let open_count = filtered.len();
    let blocked_count = filtered
        .iter()
        .filter(|i| i.status == bv_core::model::Status::Blocked)
        .count();
    let in_progress = filtered
        .iter()
        .filter(|i| i.status == bv_core::model::Status::InProgress)
        .count();
    let avg_priority: f64 = if open_count > 0 {
        filtered.iter().map(|i| i.priority as f64).sum::<f64>() / open_count as f64
    } else {
        0.0
    };
    let estimated_minutes: i64 = filtered.iter().filter_map(|i| i.estimated_minutes).sum();
    let mut payload = full_envelope_for(&hash, &issues);
    payload["capacity"] = serde_json::json!({
        "open_count": open_count,
        "blocked_count": blocked_count,
        "in_progress_count": in_progress,
        "avg_priority": avg_priority,
        "estimated_minutes": estimated_minutes,
        "label_filter": label,
    });
    payload["usage_hints"] = serde_json::json!([
        "This is a simplified capacity snapshot. Go's robot-capacity uses a more \
         complex simulation with historical velocity data (see plan doc §11).",
    ]);
    emit_json(&payload)
}

/// Shared loader for the correlator-backed commands: issues + a full
/// correlation report (`bv_correlation::correlator::correlate`). Walks up
/// to 1000 commits — Go's default `--history-limit` is 500; doubled here
/// since file-hotspots/file-relations benefit from more history and this
/// pipeline has no caching layer yet (see plan doc §11).
fn load_correlation_report(
    cwd: &std::path::Path,
) -> Result<(Vec<bv_core::model::Issue>, String, CorrelationReport), String> {
    let (issues, hash, _as_of_commit) = load_issues_auto(cwd, None)?;
    let commits = bv_correlation::correlator::walk_commits(cwd, 1000)?;
    let report = bv_correlation::correlator::correlate(&issues, &commits);
    Ok((issues, hash, report))
}

/// Go `handleRobotExplainCorrelation` — `--robot-explain-correlation SHA:beadID`.
fn run_robot_explain_correlation(args: &[String]) -> ExitCode {
    let raw = args
        .iter()
        .position(|a| a == "--robot-explain-correlation")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let Some((sha, bead_id)) = raw.split_once(':') else {
        eprintln!("Error: expected format SHA:beadID, got: {raw:?}");
        return ExitCode::from(2);
    };
    let (sha, bead_id) = (sha.trim().to_lowercase(), bead_id.trim());

    let cwd = std::env::current_dir().unwrap_or_default();
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let Some(commits) = report.get(bead_id) else {
        eprintln!("Bead not found in correlation report: {bead_id}");
        return ExitCode::from(1);
    };
    let Some(hit) = commits
        .iter()
        .find(|c| c.sha.to_lowercase().starts_with(&sha))
    else {
        eprintln!("Commit {sha} not found in bead {bead_id} correlations");
        return ExitCode::from(1);
    };
    let mut payload = full_envelope_for(&hash, &[]);
    payload["explanation"] = serde_json::to_value(hit).unwrap_or_default();
    emit_json(&payload)
}

/// Go `handleRobotCorrelationStats` — `--robot-correlation-stats`.
fn run_robot_correlation_stats() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let total_commits: usize = report.values().map(|v| v.len()).sum();
    let (mut explicit, mut temporal) = (0usize, 0usize);
    let mut confidences: Vec<f64> = Vec::new();
    for commits in report.values() {
        for c in commits {
            confidences.push(c.confidence);
            if c.methods.contains(&"explicit_id") {
                explicit += 1;
            }
            if c.methods.contains(&"temporal_author") {
                temporal += 1;
            }
        }
    }
    let avg_confidence = if confidences.is_empty() {
        0.0
    } else {
        confidences.iter().sum::<f64>() / confidences.len() as f64
    };
    let beads_dir = cwd.join(".beads");
    let store = bv_correlation::feedback::FeedbackStore::new(&beads_dir);
    let (confirmed, rejected, ignored, accuracy) = store.stats();

    let mut payload = full_envelope_for(&hash, &_issues);
    payload["stats"] = serde_json::json!({
        "correlated_beads": report.len(),
        "total_correlated_commits": total_commits,
        "by_method": { "explicit_id": explicit, "temporal_author": temporal },
        "avg_confidence": avg_confidence,
        "feedback": {
            "confirmed": confirmed,
            "rejected": rejected,
            "ignored": ignored,
            "accuracy": accuracy,
        },
    });
    emit_json(&payload)
}

/// Go `handleRobotFileBeads` — `--robot-file-beads <path>`.
fn run_robot_file_beads(args: &[String]) -> ExitCode {
    let path = args
        .iter()
        .position(|a| a == "--robot-file-beads")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let mut beads: Vec<serde_json::Value> = Vec::new();
    for (bead_id, commits) in &report {
        let touching: Vec<&bv_correlation::correlator::CorrelatedCommit> = commits
            .iter()
            .filter(|c| c.files.iter().any(|f| f == &path))
            .collect();
        if !touching.is_empty() {
            let max_conf = touching.iter().map(|c| c.confidence).fold(0.0, f64::max);
            beads.push(serde_json::json!({
                "bead_id": bead_id,
                "commit_count": touching.len(),
                "max_confidence": max_conf,
            }));
        }
    }
    beads.sort_by(|a, b| {
        b["max_confidence"]
            .as_f64()
            .partial_cmp(&a["max_confidence"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut payload = full_envelope_for(&hash, &_issues);
    payload["path"] = serde_json::json!(path);
    payload["beads"] = serde_json::Value::Array(beads);
    emit_json(&payload)
}

/// Go `handleRobotFileHotspots` — `--robot-file-hotspots`.
fn run_robot_file_hotspots() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let mut per_file: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for (bead_id, commits) in &report {
        for c in commits {
            for f in &c.files {
                per_file
                    .entry(f.clone())
                    .or_default()
                    .insert(bead_id.clone());
            }
        }
    }
    let mut hotspots: Vec<serde_json::Value> = per_file
        .iter()
        .map(|(path, beads)| {
            serde_json::json!({
                "path": path,
                "bead_count": beads.len(),
                "beads": beads.iter().collect::<Vec<_>>(),
            })
        })
        .collect();
    hotspots.sort_by(|a, b| {
        b["bead_count"]
            .as_u64()
            .cmp(&a["bead_count"].as_u64())
            .then_with(|| a["path"].as_str().cmp(&b["path"].as_str()))
    });
    hotspots.truncate(20);
    let mut payload = full_envelope_for(&hash, &_issues);
    payload["hotspots"] = serde_json::Value::Array(hotspots);
    emit_json(&payload)
}

/// Go `handleRobotFileRelations` — `--robot-file-relations <path>`: files
/// that co-change with the target across correlated commits.
fn run_robot_file_relations(args: &[String]) -> ExitCode {
    let path = args
        .iter()
        .position(|a| a == "--robot-file-relations")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let mut co_change: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut seen_shas: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for commits in report.values() {
        for c in commits {
            if !c.files.iter().any(|f| f == &path) || !seen_shas.insert(c.sha.as_str()) {
                continue;
            }
            for other in &c.files {
                if other != &path {
                    *co_change.entry(other.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    let mut related: Vec<serde_json::Value> = co_change
        .into_iter()
        .map(|(f, count)| serde_json::json!({ "path": f, "co_change_count": count }))
        .collect();
    related.sort_by(|a, b| {
        b["co_change_count"]
            .as_u64()
            .cmp(&a["co_change_count"].as_u64())
            .then_with(|| a["path"].as_str().cmp(&b["path"].as_str()))
    });
    related.truncate(20);
    let mut payload = full_envelope_for(&hash, &_issues);
    payload["path"] = serde_json::json!(path);
    payload["related_files"] = serde_json::Value::Array(related);
    emit_json(&payload)
}

/// The subset of `flags::ROBOT_PRIMARIES` that actually has a dispatch
/// handler wired up in this binary today. Kept as an explicit list (rather
/// than derived from control flow) so `robot-capabilities`/`robot-schema`
/// report real status instead of guessing — update this when wiring a new
/// command. Source of truth cross-checked against the dispatch chain above.
const DISPATCHED_ROBOT_COMMANDS: &[&str] = &[
    "robot-help",
    "robot-capabilities",
    "robot-schema",
    "robot-metrics",
    "robot-docs",
    "robot-triage",
    "robot-next",
    "robot-triage-by-track",
    "robot-triage-by-label",
    "robot-history",
    "bead-history",
    "robot-orphans",
    "robot-insights",
    "robot-plan",
    "robot-priority",
    "robot-suggest",
    "robot-alerts",
    "robot-drift",
    "robot-graph",
    "robot-recipes",
    "robot-label-health",
    "robot-label-flow",
    "robot-label-attention",
    "robot-blocker-chain",
    "robot-confirm-correlation",
    "robot-reject-correlation",
    "robot-explain-correlation",
    "robot-correlation-stats",
    "robot-file-beads",
    "robot-file-hotspots",
    "robot-file-relations",
    "robot-search",
    "robot-causality",
    "robot-related",
    "robot-impact-network",
    "robot-sprint-list",
    "robot-sprint-show",
    "robot-burndown",
    "robot-forecast",
    "robot-capacity",
    "robot-impact",
    "robot-diff",
    "robot-not-ready-labels",
];

/// Go `generateRobotCapabilities` (lower-fidelity first pass — see plan
/// doc §11: Go's version embeds a large hand-authored per-command doc map
/// with param schemas, key_fields, needs_git/needs_sprint flags etc. that
/// isn't ported. This reports real, verified implementation status per
/// command from `flags::ROBOT_PRIMARIES` cross-referenced against
/// `DISPATCHED_ROBOT_COMMANDS` — not fabricated).
fn run_robot_capabilities() -> ExitCode {
    let mut commands: Vec<serde_json::Value> = flags::ROBOT_PRIMARIES
        .iter()
        .map(|f| {
            let implemented = DISPATCHED_ROBOT_COMMANDS.contains(&f.name);
            let mut entry = serde_json::json!({
                "name": f.name,
                "flag": format!("--{}", f.name),
                "status": if implemented { "implemented" } else { "not_implemented" },
                "preferred_invocation": format!("bvr --{} --json", f.name),
                "accepted_invocations": [
                    format!("bvr --{} --format json", f.name),
                    format!("bvr --{} --json", f.name),
                ],
            });
            // Add needs_* flags matching Go.
            let obj = entry.as_object_mut().unwrap();
            obj.insert("needs_issues".into(), serde_json::json!(true));
            obj.insert("needs_git".into(), serde_json::json!(false));
            obj.insert("needs_sprint".into(), serde_json::json!(false));
            obj.insert("needs_baseline".into(), serde_json::json!(false));
            obj.insert("mutates_state".into(), serde_json::json!(false));
            entry
        })
        .collect();
    commands.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "tool": "bvr",
        "version": env!("CARGO_PKG_VERSION"),
        "contract_version": bv_robot::ROBOT_CONTRACT_VERSION,
        "default_robot_command": "bvr --robot-triage",
        "output_formats": ["json", "toon"],
        "commands": commands,
        "implemented_count": DISPATCHED_ROBOT_COMMANDS.len(),
        "total_count": flags::ROBOT_PRIMARIES.len(),
        "schema_command": "bvr --robot-schema",
        "stream_contract": {
            "stdout": "Structured robot data only for robot commands.",
            "stderr": "Diagnostics, warnings, and actionable errors.",
        },
    });
    emit_json(&payload)
}

/// Go `handleRobotSchema` (`--robot-schema`, optional `--schema-command NAME`).
/// Scope cut: returns a minimal real schema shape (name/status/flag), not
/// Go's full per-field JSON-schema definitions (`generateRobotSchemas`) —
/// those aren't ported. See plan doc §11.
fn run_robot_schema(args: &[String]) -> ExitCode {
    // Go: --schema-command filters to a single command's schema.
    let command = args
        .iter()
        .position(|a| a == "--schema-command")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let full = bv_robot::schema::generate_robot_schemas(&jiff_now());
    let Some(name) = &command else {
        return emit_json(&full);
    };

    let Some(schema) = full["commands"].get(name) else {
        eprintln!("Unknown command: {name}");
        eprintln!("Available commands:");
        let mut names: Vec<&str> = full["commands"]
            .as_object()
            .map(|o| o.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        names.sort();
        for n in names {
            eprintln!("  {n}");
        }
        return ExitCode::from(1);
    };
    let payload = serde_json::json!({
        "schema_version": full["schema_version"],
        "generated_at": full["generated_at"],
        "command": name,
        "schema": schema,
    });
    emit_json(&payload)
}

/// Go `handleRobotMetrics` (`--robot-metrics`). Scope cut: Go tracks live
/// per-command timing/cache-hit histograms via a `metrics` package that
/// has no Rust equivalent (nothing instruments handler timing here yet).
/// Reporting fabricated timing numbers would be worse than reporting none
/// — this returns only what's actually true: process memory (best-effort,
/// platform-dependent) and dataset size for the current working directory.
fn run_robot_metrics() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let t_load = bv_analysis::metrics::time(&bv_analysis::metrics::TIMING_GRAPH_LOAD);
    let issue_count = bv_core::discovery::load_issues_from_repo(&cwd)
        .map(|(issues, _)| issues.len())
        .unwrap_or(0);
    drop(t_load);
    let mut payload = serde_json::json!({
        "generated_at": jiff_now(),
        "tool": "bvr",
        "version": env!("CARGO_PKG_VERSION"),
        "dataset": { "issue_count": issue_count },
    });
    let m = bv_analysis::metrics::get_all_metrics();
    payload["timing"] = m["timing"].clone();
    payload["cache"] = m["cache"].clone();
    payload["memory"] = m["memory"].clone();
    payload["usage_hints"] = serde_json::json!([
        "Set BV_METRICS=0 to disable collection entirely",
        "jq '.timing[] | select(.count > 0)' - Only measured operations",
        "jq '.cache[] | select(.total > 0) | {name, hit_rate}' - Cache efficiency",
    ]);
    emit_json(&payload)
}

/// Go `handleRobotDocs` (`--robot-docs [topic]`). Scope cut: Go's
/// `generateRobotDocs` embeds a large hand-authored guide per topic; this
/// returns a minimal real index instead of that text (see plan doc §11).
fn run_robot_docs(args: &[String]) -> ExitCode {
    // Go: `--robot-docs` with no topic defaults to "guide" (cobra flag value).
    let topic = args
        .iter()
        .position(|a| a == "--robot-docs")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "guide".to_string());
    let payload = bv_robot::docs::generate_robot_docs(&topic, GO_APP_VERSION, &jiff_now());
    emit_json(&payload)
}

/// Go `Analyzer.GetBlockerChain` — `--robot-blocker-chain <issue-id>`.
fn run_robot_blocker_chain(args: &[String]) -> ExitCode {
    let issue_id = args
        .iter()
        .position(|a| a == "--robot-blocker-chain")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    match bv_analysis::blocker_chain::get_blocker_chain(&issues, &issue_id) {
        Some(result) => {
            let mut payload = full_envelope_for(&hash, &issues);
            payload["result"] = serde_json::to_value(&result).unwrap_or_default();
            emit_json(&payload)
        }
        None => {
            eprintln!("Issue not found: {issue_id}");
            ExitCode::from(1)
        }
    }
}

/// Go `handleRobotCorrelationFeedback` — `--robot-confirm-correlation SHA:beadID`
/// / `--robot-reject-correlation SHA:beadID`.
///
/// Runs the correlator pipeline to generate a fresh report, validates that
/// the given SHA exists in the bead's correlation history (exact match,
/// short SHA, or unambiguous prefix), and records feedback with the
/// commit's `original_conf` from the report. Returns exit code 1 if the
/// SHA is not found or ambiguous, matching Go behavior.
fn run_robot_correlation_feedback(args: &[String], flag: &str, feedback_type: &str) -> ExitCode {
    let flag_name = format!("--robot-{flag}");
    let raw = args
        .iter()
        .position(|a| a == &flag_name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let Some((sha, bead_id)) = raw.split_once(':') else {
        eprintln!("Error: expected format SHA:beadID, got: {raw:?}");
        return ExitCode::from(2);
    };
    let (sha, bead_id) = (sha.trim(), bead_id.trim());
    if sha.is_empty() || bead_id.is_empty() {
        eprintln!("Error: expected non-empty SHA and bead ID in format SHA:beadID, got: {raw:?}");
        return ExitCode::from(2);
    }

    let cwd = std::env::current_dir().unwrap_or_default();

    // Generate correlation report via the correlator pipeline.
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    // Look up bead's correlation history.
    let Some(commits) = report.get(bead_id) else {
        eprintln!("Bead not found: {bead_id}");
        return ExitCode::from(1);
    };

    // Resolve SHA against the bead's correlated commits.
    let target = match bv_correlation::correlator::resolve_correlated_commit(commits, sha) {
        Ok(Some(c)) => c,
        Ok(None) => {
            eprintln!("Commit SHA not found in bead {bead_id} correlations");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let resolved_sha = target.sha.clone();
    let original_conf = target.confidence;

    let beads_dir = cwd.join(".beads");
    let store = bv_correlation::feedback::FeedbackStore::new(&beads_dir);
    let fb = bv_correlation::feedback::CorrelationFeedback {
        commit_sha: resolved_sha.to_lowercase(),
        bead_id: bead_id.to_string(),
        feedback_at: jiff_now(),
        feedback_by: "cli".to_string(),
        feedback_type: feedback_type.to_string(),
        reason: String::new(),
        original_conf,
    };
    if let Err(e) = store.record(&fb) {
        eprintln!("Error saving feedback: {e}");
        return ExitCode::from(1);
    }

    let mut payload = full_envelope_for(&hash, &[]);
    payload["commit"] = serde_json::json!(resolved_sha);
    payload["bead"] = serde_json::json!(bead_id);
    payload["status"] = serde_json::json!(if feedback_type == "confirm" {
        "confirmed"
    } else {
        "rejected"
    });
    payload["orig_conf"] = serde_json::json!(original_conf);
    emit_json(&payload)
}

fn run_robot_label_health() -> ExitCode {
    let (issues, hash, _p1, _status, _g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let cfg = bv_analysis::label_health::LabelHealthConfig::default();
    let results = bv_analysis::label_health::compute_all_label_health(&issues, &cfg, robot_now());
    let mut payload = full_envelope_for(&hash, &issues);
    payload["analysis_config"] = serde_json::to_value(&cfg).unwrap_or_default();
    payload["results"] = serde_json::to_value(&results).unwrap_or_default();
    payload["usage_hints"] = serde_json::json!([
        "jq '.results.summaries | sort_by(.health) | .[:3]' - Critical labels",
        "jq '.results.labels[] | select(.health_level == \"critical\")' - Critical details",
        "jq '.results.cross_label_flow.bottleneck_labels' - Bottleneck labels",
        "jq '.results.attention_needed' - Labels needing attention",
    ]);
    emit_json(&payload)
}

fn run_robot_label_flow() -> ExitCode {
    let (issues, hash, _p1, _status, _g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let cfg = bv_analysis::label_health::LabelHealthConfig::default();
    let flow = bv_analysis::label_health::compute_cross_label_flow(&issues, &cfg);
    let mut payload = full_envelope_for(&hash, &issues);
    // Go: nil arrays serialize as null (not []) for empty list fields.
    let mut flow_obj = serde_json::Map::new();
    flow_obj.insert("labels".into(), serde_json::json!(flow.labels));
    flow_obj.insert("flow_matrix".into(), serde_json::json!(flow.flow_matrix));
    {
        flow_obj.insert("dependencies".into(), serde_json::json!(flow.dependencies));
    }
    {
        flow_obj.insert(
            "critical_paths".into(),
            serde_json::json!(flow.critical_paths),
        );
    }
    {
        flow_obj.insert(
            "bottleneck_labels".into(),
            serde_json::json!(flow.bottleneck_labels),
        );
    }
    flow_obj.insert(
        "total_cross_label_deps".into(),
        serde_json::json!(flow.total_cross_label_deps),
    );
    payload["flow"] = serde_json::Value::Object(flow_obj);
    payload["analysis_config"] = serde_json::to_value(&cfg).unwrap_or_default();
    payload["usage_hints"] = serde_json::json!([
        "jq '.flow.bottleneck_labels' - labels blocking the most others",
        "jq '.flow.dependencies[] | select(.issue_count > 0) | {from:.from_label,to:.to_label,count:.issue_count}'",
        "jq '.flow.flow_matrix' - raw matrix (row=from, col=to, align with .flow.labels)",
    ]);
    emit_json(&payload)
}

/// Go `buildAttentionReason` — human-readable attention reason.
fn build_attention_reason(s: &bv_analysis::label_health::LabelAttentionScore) -> String {
    let mut parts: Vec<String> = Vec::new();
    if s.pagerank_sum > 0.5 {
        parts.push("High PageRank".into());
    }
    if s.blocked_count > 0 {
        parts.push(format!("{} blocked", s.blocked_count));
    }
    if s.stale_count > 0 {
        parts.push(format!("{} stale", s.stale_count));
    }
    if s.velocity_factor <= 1.0 {
        parts.push("low velocity".into());
    }
    if parts.is_empty() {
        return format!("{} open issues", s.open_count);
    }
    parts.join(", ")
}

fn run_robot_label_attention() -> ExitCode {
    let (issues, hash) = match bv_core::discovery::load_issues_from_repo(
        &std::env::current_dir().unwrap_or_default(),
    ) {
        Ok((issues, _)) => {
            let h = bv_core::data_hash::compute_data_hash(&issues);
            (issues, h)
        }
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let cfg = bv_analysis::label_health::LabelHealthConfig::default();
    let result =
        bv_analysis::label_health::compute_label_attention_scores(&issues, &cfg, robot_now());
    let mut payload = full_envelope_for(&hash, &issues);
    // Go's --attention-limit defaults to 5 (main.go:1494). Read the flag when
    // given and apply it to the emitted list, not just to the reported limit.
    let attention_limit: usize = {
        let args: Vec<String> = std::env::args().collect();
        let mut found = None;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--attention-limit" {
                found = args.get(i + 1).and_then(|v| v.trim().parse::<usize>().ok());
                break;
            }
            if let Some(v) = a.strip_prefix("--attention-limit=") {
                found = v.trim().parse::<usize>().ok();
                break;
            }
            i += 1;
        }
        found.unwrap_or(5)
    };
    let effective_limit = result.labels.len().min(attention_limit);
    payload["limit"] = serde_json::json!(effective_limit);
    payload["total_labels"] = serde_json::json!(result.total_labels);
    // Go builds labels from a separate struct with specific field order:
    // rank, label, attention_score, normalized_score, reason, open_count,
    // blocked_count, stale_count, pagerank_sum, velocity_factor.
    // Go truncates the ranked list to --attention-limit (robot_registry.go:1833)
    // and reports the same bound as `limit`; without the truncation the array
    // carried every scored label.
    let attention_labels: Vec<serde_json::Value> = result
        .labels
        .iter()
        .take(effective_limit)
        .map(|s| {
            let reason = build_attention_reason(s);
            serde_json::json!({
                "rank": s.rank,
                "label": s.label,
                "attention_score": s.attention_score,
                "normalized_score": s.normalized_score,
                "reason": reason,
                "open_count": s.open_count,
                "blocked_count": s.blocked_count,
                "stale_count": s.stale_count,
                "pagerank_sum": s.pagerank_sum,
                "velocity_factor": s.velocity_factor,
            })
        })
        .collect();
    payload["labels"] = serde_json::Value::Array(attention_labels);
    payload["usage_hints"] = serde_json::json!([
        "jq '.labels[0]' - top attention label details",
        "jq '.labels[] | select(.blocked_count > 0)' - labels with blocked issues",
        "jq '.labels[] | {label:.label,score:.attention_score,reason:.reason}'",
    ]);
    emit_json(&payload)
}

/// Go handleRobotImpact — file-based impact analysis.
fn run_robot_impact(args: &[String]) -> ExitCode {
    let files_str = args
        .iter()
        .position(|a| a == "--robot-impact")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let files: Vec<String> = files_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if files.is_empty() {
        eprintln!("Error: --robot-impact requires comma-separated file paths");
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let (_issues, hash, report) = match load_correlation_report(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let result = bv_analysis::file_impact::compute_file_impact(&files, &report);
    let mut payload = full_envelope_for(&hash, &[]);
    payload["files"] = serde_json::json!(result.files);
    payload["risk_level"] = serde_json::json!(result.risk_level);
    payload["risk_score"] = serde_json::json!(result.risk_score);
    payload["summary"] = serde_json::json!(result.summary);
    payload["affected_beads"] = serde_json::to_value(&result.affected_beads).unwrap_or_default();
    emit_json(&payload)
}

/// Go handleRobotDiff — git snapshot comparison.
///
/// Go builds two `analysis.Snapshot`s (one per revision) and diffs the graphs
/// between them (cmd/bv/robot_registry.go:1614-1647, main.go:4336-4340), so
/// the output carries `resolved_revision`, `from_data_hash`, `to_data_hash`
/// and the full `SnapshotDiff` — not the flat issue-list diff this handler
/// used to emit.
fn run_robot_diff(args: &[String]) -> ExitCode {
    let diff_ref = args
        .iter()
        .position(|a| a == "--diff-since")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let Some(ref_str) = diff_ref else {
        eprintln!("Error: --robot-diff requires --diff-since <git-ref>");
        return ExitCode::from(2);
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let (current, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    // Shared plumbing with the TUI Time-Travel view (TUI_UX_PARITY_PLAN.md
    // Q4): revision resolution + `git show` + tolerant JSONL parse live in
    // `bv_core::discovery::GitLoader` (the same loader `--as-of` uses), so
    // the diff algorithm is built once and reused, not duplicated.
    let git = bv_core::discovery::GitLoader::new(&cwd);
    let previous = match git.load_at(&ref_str) {
        Ok(prev) => prev,
        Err(_) => {
            eprintln!("Error: could not read issues at ref {ref_str}");
            return ExitCode::from(1);
        }
    };
    // Go falls back to the raw ref when resolution fails (main.go:4330-4333).
    let revision = git
        .resolve_revision(&ref_str)
        .unwrap_or_else(|_| ref_str.clone());

    // Go: NewSnapshotAt(historical, time.Time{}, revision) and
    // NewSnapshot(issues), then the handler stamps ToTimestamp with
    // robotNow() so SOURCE_DATE_EPOCH pins the output.
    let from = bv_analysis::snapshot_diff::Snapshot::new(
        previous.clone(),
        bv_analysis::snapshot_diff::GO_ZERO_TIME.to_string(),
        revision.clone(),
    );
    let to = bv_analysis::snapshot_diff::Snapshot::new(current.clone(), jiff_now(), String::new());
    let diff = bv_analysis::snapshot_diff::compare_snapshots(&from, &to);

    // The envelope is built from the live issues (Go's ctx.Issues), not from
    // the historical ones, so `scope_hash` agrees with every other command.
    let mut payload = full_envelope_for(&hash, &current);
    payload["resolved_revision"] = serde_json::json!(revision);
    payload["from_data_hash"] = serde_json::json!(bv_core::data_hash::compute_data_hash(&previous));
    payload["to_data_hash"] = serde_json::json!(hash);
    payload["diff"] = serde_json::to_value(&diff).unwrap_or_default();
    emit_json(&payload)
}

/// Go handleRobotNotReadyLabels — filter triage by not-ready labels.
fn run_robot_not_ready_labels(args: &[String]) -> ExitCode {
    let labels_str = args
        .iter()
        .position(|a| a == "--robot-not-ready-labels")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let not_ready: Vec<String> = labels_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if not_ready.is_empty() {
        eprintln!("Error: --robot-not-ready-labels requires comma-separated labels");
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let excluded = issues
        .iter()
        .filter(|i| i.labels.iter().any(|l| not_ready.contains(l)))
        .count();
    let remaining: Vec<&str> = issues
        .iter()
        .filter(|i| !i.labels.iter().any(|l| not_ready.contains(l)))
        .map(|i| i.id.as_str())
        .collect();
    let mut payload = full_envelope_for(&hash, &issues);
    payload["not_ready_labels"] = serde_json::json!(not_ready);
    payload["total_issues"] = serde_json::json!(issues.len());
    payload["excluded_count"] = serde_json::json!(excluded);
    payload["remaining_count"] = serde_json::json!(remaining.len());
    payload["remaining_ids"] = serde_json::json!(remaining);
    emit_json(&payload)
}

/// Go `robotNow` parity: `SOURCE_DATE_EPOCH` (unix seconds) pins the clock
/// for deterministic output (reproducible builds / differential testing).
fn robot_now() -> jiff::Timestamp {
    if let Ok(v) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(secs) = v.trim().parse::<i64>() {
            if let Ok(ts) = jiff::Timestamp::from_second(secs) {
                return ts;
            }
        }
    }
    jiff::Timestamp::now()
}

fn jiff_now() -> String {
    // Go parity: truncate to second precision (no microseconds).
    let ts = robot_now();
    let s = ts.to_string();
    // Strip sub-second portion: "2026-08-22T14:07:01.790741Z" → "2026-08-22T14:07:01Z"
    if let Some(pos) = s.find('.') {
        format!("{}Z", &s[..pos])
    } else {
        s
    }
}

/// Go `handleRobotImpact` — `--robot-impact <file1,file2,...>`.
/// Analyzes which beads would be affected by modifying the given files,
/// using the correlator pipeline's file→bead mapping.
/// Go `handleRobotDiff` — `--robot-diff --diff-since <ref>`.
/// Compares current issue set against a previous state.
/// Go `handleRobotNotReadyLabels` — `--robot-not-ready-labels <label1,label2,...>`.
/// Filters triage results to exclude issues with "not-ready" labels.

fn print_robot_help() {
    println!("bvr robot commands (AI agent interface)");
    println!();
    println!("PRIMARY COMMANDS:");
    for f in flags::ROBOT_PRIMARIES {
        println!(
            "  --{}{}",
            f.name,
            match f.kind {
                flags::FlagKind::Str => " <value>",
                flags::FlagKind::Int => " <n>",
                flags::FlagKind::Float => " <f>",
                _ => "",
            }
        );
    }
    println!();
    println!("Output contract: stdout=data only; stderr=diagnostics;");
    println!("exit 0=success, 1=error/critical-drift, 2=usage/warning-drift.");
}
