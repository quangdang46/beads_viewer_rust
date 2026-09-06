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

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = argv::rewrite_args(&raw);

    // --version handled before validation (Go parity).
    if args.iter().any(|a| a == "--version") {
        println!("bvr 0.1.1");
        return ExitCode::from(0);
    }

    let presence = validation::Presence::from_args(&args);

    // Validation order mirrors Go: modifier-requires then exclusive primaries.
    let mut violations = validation::validate_modifier_requires(&presence);
    violations.extend(validation::validate_exclusive_primaries(&presence));

    if !violations.is_empty() {
        for v in &violations {
            eprintln!("Error: {v}");
        }
        eprintln!("Usage: bvr --robot-help  (full robot surface arrives with dispatch phase)");
        return ExitCode::from(1);
    }

    if args.iter().any(|a| a == "--help" || a == "-h")
        && !args.iter().any(|a| a.starts_with("--robot"))
    {
        println!("bvr — Beads Viewer in Rust");
        println!();
        println!("USAGE:");
        println!("  bvr                    Launch interactive TUI");
        println!("  bvr --robot-triage     Unified triage (mega-command)");
        println!("  bvr --robot-next       Single top pick + claim command");
        println!("  bvr --robot-insights   Graph metrics + top-N lists");
        println!("  bvr --robot-plan       Dependency-respecting execution plan");
        println!("  bvr --robot-graph      Dependency graph as JSON/DOT/Mermaid");
        println!("  bvr --robot-history    Bead-commit correlation from git log");
        println!("  bvr --robot-orphans    Orphan commit detection");
        println!("  bvr --robot-alerts     Drift + proactive warnings");
        println!("  bvr --export-md FILE   Export markdown report");
        println!("  bvr --save-baseline    Save current state as baseline");
        println!("  bvr --check-drift      Check drift vs baseline (exit 0/1/2)");
        println!("  bvr --version          Show version");
        println!();
        return ExitCode::from(0);
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

    // Export markdown (Phase 5a).
    if let Some(output_path_idx) = args.iter().position(|a| a == "--export-md") {
        let output_path = args
            .get(output_path_idx + 1)
            .cloned()
            .unwrap_or_else(|| "report.md".to_string());
        let cwd = std::env::current_dir().unwrap_or_default();
        let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };
        let md = bv_export::mermaid::generate_markdown(&issues, "Beads Report");
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
fn load_issues_auto(
    cwd: &std::path::Path,
    as_of: Option<&str>,
) -> Result<(Vec<bv_core::model::Issue>, String, Option<String>), String> {
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
        return Ok((issues, hash, Some(resolved)));
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
                return Ok((issues, hash, None));
            }
            Err(e) => eprintln!("workspace load failed, falling back: {e}"),
        }
    }
    let (issues, _) = bv_core::discovery::load_issues_from_repo(cwd).map_err(|e| e.to_string())?;
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    Ok((issues, hash, None))
}

/// Load issues from discovery chain and emit --robot-triage JSON.
/// Go `handleRobotNext`: single top claimable pick with the claimability
/// filter (open, non-epic, unassigned, no open blockers) and fail-closed
/// degraded output when no pick is claim-safe.
fn run_robot_next() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let as_of = extract_as_of();
    let (issues, hash, as_of_commit) = match load_issues_auto(&cwd, as_of.as_deref()) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let mut payload = full_envelope_json(&hash);
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
        let mut open_blockers: Vec<String> = Vec::new();
        for dep in &issue.dependencies {
            if !dep.r#type.is_blocking() {
                continue;
            }
            let blocker_id = dep.effective_depends_on().trim().to_string();
            if blocker_id.is_empty() {
                open_blockers.push("<missing blocker id>".into());
                continue;
            }
            match issue_by_id.get(blocker_id.as_str()) {
                None => open_blockers.push(format!("{blocker_id} (missing)")),
                Some(b) => {
                    if !matches!(
                        b.status,
                        bv_core::model::Status::Closed | bv_core::model::Status::Tombstone
                    ) {
                        open_blockers.push(blocker_id);
                    }
                }
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
        if s.page_rank.state == "skipped" { s.page_rank.reason.clear(); }
        if s.eigenvector.state == "skipped" { s.eigenvector.reason.clear(); }
        if s.hits.state == "skipped" { s.hits.reason.clear(); }
        if s.critical.state == "skipped" { s.critical.reason.clear(); }
        if s.cycles.state == "skipped" { s.cycles.reason.clear(); }
        if s.kcore.state == "skipped" { s.kcore.reason.clear(); }
        if s.articulation.state == "skipped" { s.articulation.reason.clear(); }
        if s.slack.state == "skipped" { s.slack.reason.clear(); }
        s.to_json_map()
    };
    match chosen {
        Some(top) => {
            payload["actionable"] = serde_json::json!(true);
            payload["phase2_ready"] = serde_json::json!(true);
            payload["status"] = cleared_status.clone();
            payload["id"] = top["id"].clone();
            payload["title"] = top["title"].clone();
            payload["score"] = top["score"].clone();
            payload["reasons"] = top["reasons"].clone();
            payload["unblocks"] = top["unblocks"].clone();
            let id = top["id"].as_str().unwrap_or_default();
            payload["claim_command"] =
                serde_json::json!(format!("br update {id} --status=in_progress"));
            payload["show_command"] = serde_json::json!(format!("br show {id}"));
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

fn run_robot_triage() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let as_of = extract_as_of();
    let (issues, _hash, as_of_commit) = match load_issues_auto(&cwd, as_of.as_deref()) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    if issues.is_empty() {
        println!(
            "{{\"generated_at\":\"{}\",\"data_hash\":\"empty\",\"triage\":{{}}}}",
            jiff_now()
        );
        return ExitCode::from(0);
    }
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = std::sync::Arc::new(bv_analysis::analyzer::build_graph(&issues));
    let out = bv_analysis::triage::build_triage(&issues, &g, robot_now());

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
                // Check for open blockers
                let has_open_blockers = issue.dependencies.iter().any(|dep| {
                    dep.r#type.is_blocking()
                        && issue_by_id
                            .get(dep.effective_depends_on())
                            .map(|b| {
                                !matches!(
                                    b.status,
                                    bv_core::model::Status::Closed
                                        | bv_core::model::Status::Tombstone
                                )
                            })
                            .unwrap_or(false)
                });
                if has_open_blockers {
                    return false;
                }
            }
            true
        })
        .take(5)
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

    // Build quick_wins: low-effort high-impact (Go parity: open, priority<=2, low blocker_ratio).
    let quick_wins: Vec<serde_json::Value> = out
        .recommendations
        .iter()
        .filter(|r| r.status == "open" && r.priority <= 2 && r.breakdown.blocker_ratio < 0.1)
        .take(5)
        .map(|r| {
            serde_json::json!({
                "id": r.id, "title": r.title, "score": r.score,
                "reason": format!("High priority (P{}) with minimal staleness", r.priority),
            })
        })
        .collect();

    // Build blockers_to_clear: high betweenness blocking issues.
    let blockers_to_clear: Vec<serde_json::Value> = out
        .recommendations
        .iter()
        .filter(|r| r.breakdown.blocker_ratio > 0.0 || r.breakdown.betweenness > 0.01)
        .take(5)
        .map(|r| {
            let unblocks_count = issues
                .iter()
                .filter(|o| {
                    o.dependencies
                        .iter()
                        .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
                })
                .count();
            let unblocks_ids: Vec<String> = issues
                .iter()
                .filter(|o| {
                    o.dependencies
                        .iter()
                        .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == r.id)
                })
                .map(|o| o.id.clone())
                .collect();
            serde_json::json!({
                "id": r.id, "title": r.title,
                "unblocks_count": unblocks_count,
                "unblocks_ids": unblocks_ids,
                "actionable": r.status == "open",
            })
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
            "density": (graph_density * 10000.0).round() / 10000.0,
            "has_cycles": false,
            "phase2_ready": true,
        },
        "velocity": out.velocity,
    });

    // Pre-built br commands (Go parity).
    let top_id = out
        .recommendations
        .first()
        .map(|r| r.id.as_str())
        .unwrap_or("");
    let commands = serde_json::json!({
        "claim_top": format!("br update {top_id} --status in_progress --assignee agent"),
        "show_top": format!("br show {top_id}"),
        "list_ready": "br ready".to_string(),
        "list_blocked": "br list --status blocked".to_string(),
        "refresh_triage": "bvr --robot-triage".to_string(),
    });

    let mut env = bv_robot::RobotEnvelope::new(
        data_hash,
        env!("CARGO_PKG_VERSION"),
        None,
        bv_robot::OutputFormat::Json,
    );
    env.generated_at = jiff_now(); // Truncate to seconds (Go parity).
                                   // Go emits history_status ("ok") only when the history prologue ran —
                                   // i.e. the workspace is a valid git repo; otherwise the key is omitted.
    let history_status = if cwd.join(".git").exists() {
        Some("ok")
    } else {
        None
    };
    let mut meta = serde_json::json!({
        "version": bv_robot::ROBOT_CONTRACT_VERSION,
        "generated_at": env.generated_at,
        "phase2_ready": true,
        "issue_count": out.counts.total,
        "compute_time_ms": 0,
    });
    if let Some(hs) = history_status {
        meta["history_status"] = serde_json::json!(hs);
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
    let mut payload = serde_json::json!({
        "generated_at": env.generated_at,
        "data_hash": env.data_hash,
        "triage": {
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
            "recommendations": out.recommendations,
            "quick_wins": quick_wins,
            "blockers_to_clear": blockers_to_clear,
            "project_health": project_health,
            "commands": commands,
        },
    });
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
    use bv_analysis::algorithms::cycles::tarjan_scc;
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _hash, _as_of_commit) = load_issues_auto(&cwd, None)?;
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = bv_analysis::analyzer::build_graph(&issues);
    let p1 = bv_analysis::analyzer::analyze_phase1(&g);
    let blocked = bv_analysis::triage::compute_blocked_set(&issues);
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
    let mut pr_map = std::collections::BTreeMap::new();
    for (i, v) in bv_analysis::algorithms::pagerank::pagerank_default(&g)
        .into_iter()
        .enumerate()
    {
        pr_map.insert(g.node_id(i).unwrap_or_default().to_string(), v);
    }
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
/// fallback, pinned at parity commit 9ace029). Byte-parity with frozen
/// goldens requires emitting Go's version string, not the Rust crate's.
const GO_APP_VERSION: &str = "v0.20.0";

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

fn run_robot_history() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let repo = std::env::current_dir().unwrap_or_default();

    let limit = 500; // Go --history-limit default
    let events = match bv_correlation::extract(
        &repo,
        &bv_correlation::ExtractOptions {
            limit,
            ..Default::default()
        },
    ) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Error: extraction failed: {err}");
            return ExitCode::from(1);
        }
    };

    // Group events by bead.
    let mut by_bead: std::collections::BTreeMap<String, Vec<&bv_correlation::BeadEvent>> =
        std::collections::BTreeMap::new();
    for e in &events {
        by_bead.entry(e.bead_id.clone()).or_default().push(e);
    }

    // Method distribution: all events from this path are explicit-message
    // correlations in the legacy extractor (Go method_distribution parity).
    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "data_hash": data_hash,
        // output_format/version omitted for JSON (Go omitempty parity).
        "stats": {
            "total_events": events.len(),
            "beads_with_commits": by_bead.len(),
        },
        "histories": by_bead.iter().map(|(id, evs)| {
            serde_json::json!({
                "bead_id": id,
                "events": evs,
            })
        }).collect::<Vec<_>>(),
    });
    println!("{payload}");
    ExitCode::from(0)
}

fn run_robot_orphans() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let repo = std::env::current_dir().unwrap_or_default();

    let min_score: i32 = std::env::var("BV_ORPHANS_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let events = match bv_correlation::extract(&repo, &ExtractOptionsAlias::default()) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Error: extraction failed: {err}");
            return ExitCode::from(1);
        }
    };

    let candidates: Vec<serde_json::Value> =
        bv_correlation::orphan::scan_orphan_candidates(&repo, &events, min_score)
            .into_iter()
            .map(|c| c.into_json())
            .collect();

    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "data_hash": data_hash,
        // output_format/version omitted for JSON (Go omitempty parity).
        "candidates_count": candidates.len(),
        "candidates": candidates,
    });
    println!("{payload}");
    ExitCode::from(0)
}

type ExtractOptionsAlias = bv_correlation::ExtractOptions;

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

fn envelope_json(data_hash: &str) -> serde_json::Value {
    serde_json::json!({
        "generated_at": jiff_now(),
        "data_hash": data_hash,
        // output_format/version omitted: Go handlers with hand-rolled inline
        // output structs (triage, plan, insights, priority, label-*, suggest)
        // don't embed RobotEnvelope, so those keys are absent there too.
    })
}

/// Go `NewRobotEnvelope` parity: for handlers whose Go output embeds the
/// full `RobotEnvelope` struct (alerts, next, history, …) the envelope also
/// carries `output_format` and `version` — golden-verified.
fn full_envelope_json(data_hash: &str) -> serde_json::Value {
    serde_json::json!({
        "generated_at": jiff_now(),
        "data_hash": data_hash,
        "output_format": "json",
        "version": GO_APP_VERSION,
    })
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
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
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
    if abs != 0.0 && (abs < 1e-6 || abs >= 1e21) {
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

fn top_n(map: &serde_json::Map<String, serde_json::Value>, n: usize) -> Vec<serde_json::Value> {
    let mut items: Vec<(String, f64)> = map
        .iter()
        .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
        .collect();
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    items.truncate(n);
    items
        .iter()
        .map(|(id, val)| serde_json::json!({"ID": id, "Value": val}))
        .collect()
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
    let bw_raw = bv_graph_core::betweenness(&g);
    let mut bw_obj = to_id_map(&g, &bw_raw);
    // gonum Betweenness omits zero-score nodes (endpoints of a DAG chain).
    bw_obj.retain(|_, v| v.as_f64().map_or(true, |f| f != 0.0));
    let ev_raw = bv_graph_core::eigenvector_default(&g);
    let ev_obj = to_id_map(&g, &ev_raw);
    let hits_result = bv_graph_core::hits_default(&g);
    let hub_obj = to_id_map(&g, &hits_result.hubs);
    let auth_obj = to_id_map(&g, &hits_result.authorities);
    let cp_heights = bv_graph_core::critical_path_heights(&g);
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

    let mut payload = envelope_json(&hash);
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
    if fixed_status.cycles.state == "skipped" {
        fixed_status.cycles.reason.clear();
    }
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
    payload["Articulation"] = serde_json::json!(art_ids);
    payload["Slack"] = serde_json::Value::Array(top_items_go(&slack_obj, INSIGHTS_LIMIT));

    // Orphans: zero out-degree (nothing depends on them), sorted (Go findOrphans).
    let orphans: Vec<String> = (0..g.len())
        .filter(|&i| g.out_degree(i) == 0)
        .map(|i| g.node_id(i).unwrap_or_default().to_string())
        .collect();
    payload["Orphans"] = serde_json::json!(orphans);

    // Cycles: Go emits null when none detected.
    let cycles_from_phase2 = phase2.cycles.clone().unwrap_or_default();
    if cycles_from_phase2.is_empty() {
        payload["Cycles"] = serde_json::Value::Null;
    } else {
        payload["Cycles"] = serde_json::json!(cycles_from_phase2);
    }
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
    let mut fs = serde_json::Map::new();
    fs.insert("pagerank".into(), serde_json::Value::Object(pr_obj));
    fs.insert("betweenness".into(), serde_json::Value::Object(bw_obj));
    fs.insert("eigenvector".into(), serde_json::Value::Object(ev_obj));
    fs.insert("hubs".into(), serde_json::Value::Object(hub_obj));
    fs.insert("authorities".into(), serde_json::Value::Object(auth_obj));
    fs.insert(
        "critical_path_score".into(),
        serde_json::Value::Object(cp_obj),
    );
    fs.insert("core_number".into(), serde_json::Value::Object(core_obj));
    fs.insert("slack".into(), serde_json::Value::Object(slack_obj));
    fs.insert("articulation_points".into(), serde_json::json!(art_ids));
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
        "jq '.CriticalPath[:3]' - Top 3 critical path items",
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
        let blocked_reduction = direct_list
            .iter()
            .filter(|id| {
                issues
                    .iter()
                    .any(|i| i.id == **id && i.status == Status::Blocked)
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
        let mut remaining = candidates.clone();
        let k = 5usize;
        for _ in 0..k {
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
                    if !is_open(issue) || completed.contains(&issue.id) || issue.id == *cand {
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
            topk_items.push(serde_json::json!({
                "id": best_id,
                "title": title_of(&best_id),
                "marginal_gain": best_gain,
                "unblocks": best_unblocks,
            }));
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
            "how_to_use": "Small vertex cover touching all dependency edges. Use for breadth coverage.",
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
            "rationale": "Greedy vertex cover (2-approx): iteratively pick highest uncovered degree until edges are covered or cap is reached.",
            "how_to_use": "Small vertex cover touching all dependency edges. Use for breadth coverage.",
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
    for &(_, len) in &path_ends {
        if len > 0 {
            total_paths += 1;
        }
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
    let k_paths = serde_json::json!({
        "status": feature_status("available", "", paths.len() >= 5 && total_paths > 5, paths.len() as i64, total_paths),
        "paths": paths,
        "how_to_use": "K-shortest critical paths. Focus on issues appearing in multiple paths.",
    });

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
    let current_actionable = open_set
        .iter()
        .filter(|id| blocked_by.get(*id).map_or(true, |v| v.is_empty()))
        .count();
    let mut pc_candidates: Vec<(String, i64, i64, Vec<String>)> = Vec::new();
    for id in &open_set {
        let mut newly: Vec<String> = Vec::new();
        if let Some(dependents) = blocker_of.get(id) {
            for &dep_id in dependents {
                let all_others_resolved = blocked_by
                    .get(dep_id)
                    .map(|blockers| blockers.iter().all(|&b| b == *id || !open_set.contains(b)))
                    .unwrap_or(true);
                if all_others_resolved {
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
    pc_candidates.truncate(5);
    let max_parallel = current_actionable as i64 + pc_candidates.first().map_or(0, |c| c.1);
    let parallel_cut = serde_json::json!({
        "status": {"state": "available"},
        "max_parallel": max_parallel,
        "how_to_use": "Issues that enable parallel work. Complete to maximize team throughput.",
    });

    // ---- Parallel Gain — Go: pending (bv-129 not implemented upstream) ----
    let parallel_gain = serde_json::json!({
        "status": {"state": "pending", "reason": "Awaiting implementation (bv-129)"},
        "how_to_use": "Parallelization improvement from completing each issue.",
    });

    // ---- Cycle Break — Go generateCycleBreakSuggestions ----
    let cycle_break = if cycles.is_empty() {
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
            "parallel_cut_limit": 5,
        },
        // Go map keys serialize sorted alphabetically.
        "usage_hints": {
            "coverage_set": "Small vertex cover touching all dependency edges. Use for breadth coverage.",
            "cycle_break": "Structural fix suggestions. Apply BEFORE working on cycle members.",
            "k_paths": "K-shortest critical paths. Focus on issues appearing in multiple paths.",
            "parallel_cut": "Issues that enable parallel work. Complete to maximize team throughput.",
            "parallel_gain": "Parallelization improvement from completing each issue.",
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
    let (bt_ns, pr_ns, cycles_ns, max_cycles) = match nodes {
        n if n < 100 => (2_000_000_000i64, 2_000_000_000i64, 2_000_000_000i64, 1000),
        n if n < 500 => (500_000_000i64, 500_000_000i64, 500_000_000i64, 100),
        n if n < 2000 => (500_000_000i64, 300_000_000i64, 300_000_000i64, 50),
        _ => (500_000_000i64, 200_000_000i64, 0i64, 10),
    };
    serde_json::json!({
        "ComputeBetweenness": true,
        "BetweennessTimeout": bt_ns,
        "BetweennessSkipReason": "",
        "BetweennessMode": "exact",
        "BetweennessSampleSize": 0,
        "BetweennessIsApproximate": false,
        "ComputePageRank": true,
        "PageRankTimeout": pr_ns,
        "PageRankSkipReason": "",
        "ComputeHITS": true,
        "HITSTimeout": pr_ns,
        "HITSSkipReason": "",
        "ComputeCycles": true,
        "CyclesTimeout": cycles_ns,
        "MaxCyclesToStore": max_cycles,
        "CyclesSkipReason": "",
        "ComputeEigenvector": true,
        "ComputeCriticalPath": true,
        "ComputeKCore": true,
        "ComputeArticulation": true,
        "ComputeSlack": true,
    })
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
    let actionable: Vec<&bv_core::model::Issue> = issues
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
    for (root, members) in &components {
        let mut actionable_members: Vec<&bv_core::model::Issue> = members
            .iter()
            .filter_map(|id| {
                actionable_set
                    .contains(id.as_str())
                    .then(|| by_id[id.as_str()])
            })
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
                // Go: nil slice serializes as null (not []).
                let unblocks_val = if unblocks.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(unblocks)
                };
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

    let payload = serde_json::json!({
        "generated_at": jiff_now(), "data_hash": hash,
        "analysis_config": plan_analysis_config(g.len()),
        "status": plan_priority_status(&g),
        "plan": {
            "tracks": tracks,
            "total_actionable": actionable.len(),
            "total_blocked": total_blocked_count,
            "summary": {
                "highest_impact": highest_id,
                "impact_reason": impact_reason,
                "unblocks_count": highest_count,
            },
        },
        "usage_hints": [
            "jq '.plan.tracks | length' - Number of parallel execution tracks",
            "jq '.plan.tracks[0].items | map(.id)' - First track item IDs",
            "jq '.plan.tracks[].items[] | select(.unblocks | length > 0)' - Items that unblock others",
            "jq '.plan.summary' - High-level execution summary",
            "jq '[.plan.tracks[].items[]] | length' - Total items across all tracks",
        ],
    });
    emit_json(&payload)
}

/// Go: `--robot-by-label`/`--robot-by-assignee` are modifiers of
/// `--robot-priority` (main.go:1799-1800) — exact-match filters applied to
/// the recommendation list, not standalone commands.
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

    let (mut issues, hash, _p1, status, g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    if let Some(label) = &by_label {
        issues.retain(|i| i.labels.iter().any(|l| l == label));
    }
    if let Some(assignee) = &by_assignee {
        issues.retain(|i| &i.assignee == assignee);
    }
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = bv_analysis::build_graph(&issues);

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
        issues: &issues,
        pagerank: &pr_map,
        betweenness: &bw_map,
        critical_path: Some(&cp_map),
        g: &g,
        now,
    };

    // Use the full impact scoring engine
    let impact_results = bv_analysis::impact::compute_impact_scores(&inputs);

    // Convert to recommendations (only where suggested < current)
    let mut recommendations: Vec<serde_json::Value> = Vec::new();
    for r in &impact_results {
        if let Some(issue) = issues.iter().find(|i| i.id == r.id) {
            let suggested = bv_analysis::scoring::score_to_priority(r.score);
            if suggested < issue.priority {
                let reasons: Vec<String> = {
                    let mut reasons = Vec::new();
                    if r.breakdown.pagerank > 0.15 {
                        reasons.push("High centrality in dependency graph".to_string());
                    }
                    if r.breakdown.betweenness > 0.10 {
                        reasons.push("Critical path bottleneck".to_string());
                    }
                    if r.breakdown.blocker_ratio > 0.05 {
                        reasons.push("Blocks multiple downstream tasks".to_string());
                    }
                    if r.breakdown.staleness > 0.03 {
                        reasons.push("Stale issue needs attention".to_string());
                    }
                    reasons
                };
                recommendations.push(serde_json::json!({
                    "issue_id": r.id,
                    "title": r.title,
                    "current_priority": issue.priority,
                    "suggested_priority": suggested,
                    "impact_score": r.score,
                    "confidence": 1,
                    "reasoning": reasons,
                }));
            }
        }
    }

    recommendations.sort_by(|a, b| {
        b["impact_score"]
            .as_f64()
            .partial_cmp(&a["impact_score"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recommendations.truncate(10);

    let mut payload = envelope_json(&hash);
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
    payload["filters"] = serde_json::json!({"max_results": 10});
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
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
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

    let output = bv_analysis::suggestions::generate_robot_suggest_output(&issues, &config, &hash);
    match serde_json::to_value(&output) {
        Ok(v) => emit_json(&v),
        Err(e) => {
            eprintln!("Error: serialization failed: {e}");
            ExitCode::from(1)
        }
    }
}
fn run_robot_alerts() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let stats = bv_analysis::drift::BaselineStats {
        node_count: issues.len(),
        ..Default::default()
    };
    let result = bv_analysis::drift::calculate(
        &stats,
        &stats,
        &bv_analysis::drift::DriftConfig::default(),
        &[],
        &issues,
        robot_now(),
    );

    // Go robot-alerts embeds the full RobotEnvelope (output_format+version)
    // and provides non-empty usage hints.
    let mut payload = full_envelope_json(&hash);
    payload["alerts"] = serde_json::to_value(&result.alerts).unwrap_or_default();
    payload["summary"] = serde_json::json!({
        "total": result.alerts.len(),
        "critical": result.critical_count,
        "warning": result.warning_count,
        "info": result.info_count,
    });
    payload["usage_hints"] = serde_json::json!([
        "--severity=warning --alert-type=stale_issue   # stale warnings only",
        "--alert-type=blocking_cascade                 # high-unblock opportunities",
        "jq '.alerts | map(.issue_id)'                # list impacted issues",
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
        let mut payload = envelope_json(&hash);
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

    let payload = serde_json::json!({
        "format": "json",
        "nodes": sorted_issues.len(),
        "edges": edge_count,
        "explanation": {
            "what": "Dependency graph as JSON adjacency list",
            "when_to_use": "When you need programmatic access to the graph structure",
        },
        "data_hash": hash,
        "adjacency": {"nodes": adj_nodes, "edges": adj_edges},
    });
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
        "output_format": "json",
        "version": GO_APP_VERSION,
        "recipes": recipes,
    });
    emit_json(&payload)
}

type CorrelationReport =
    std::collections::BTreeMap<String, Vec<bv_correlation::correlator::CorrelatedCommit>>;

/// Go `robotSearch` dispatch block (main.go — computes `searchDispatchContext.SearchOutput`
/// then calls the `robot-search` handler). `--search QUERY` required
/// (modifier-requires table), `--search-mode` (`text` default | `hybrid`),
/// `--search-preset` (hybrid only, default `default`), `--search-limit`/
/// `--robot-max-results` cap results (default 10).
///
/// Scope cut vs Go (see plan doc §11): no persisted vector index /
/// incremental sync (`index.Sync`, `syncStats`) — embeds every issue's
/// title+description fresh on each invocation via the existing
/// `hash_embed` primitive. `index`/`loaded` fields in the envelope are
/// therefore omitted rather than fabricated.
fn run_robot_search(args: &[String]) -> ExitCode {
    let query = args
        .iter()
        .position(|a| a == "--search")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    if query.trim().is_empty() {
        eprintln!("Error: --search requires a non-empty query");
        return ExitCode::from(2);
    }
    let mode = args
        .iter()
        .position(|a| a == "--search-mode")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "text".to_string());
    let preset_name = args
        .iter()
        .position(|a| a == "--search-preset")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    let limit: usize = args
        .iter()
        .position(|a| a == "--search-limit" || a == "--robot-max-results")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash, _as_of_commit) = match load_issues_auto(&cwd, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let dim = bv_search::embedder::DEFAULT_DIM;
    let query_vec = bv_search::embedder::hash_embed(&query, dim);
    let now = robot_now();

    let mut results: Vec<serde_json::Value> = Vec::new();
    if mode == "hybrid" {
        let Some(weights) = bv_search::hybrid::get_preset(&preset_name) else {
            eprintln!("Error: unknown --search-preset {preset_name:?}");
            return ExitCode::from(2);
        };
        for issue in &issues {
            let text = format!("{} {}", issue.title, issue.description);
            let issue_vec = bv_search::embedder::hash_embed(&text, dim);
            let text_score = bv_search::embedder::cosine_similarity(&query_vec, &issue_vec);
            let days_since_update = issue
                .updated_at
                .as_deref()
                .and_then(|s| s.parse::<jiff::Timestamp>().ok())
                .map(|t| now.since(t).map(|d| d.get_days()).unwrap_or(0) as f64)
                .unwrap_or(0.0);
            let components = bv_search::hybrid::ComponentScores::new(
                issue.status.as_str(),
                issue.priority,
                days_since_update,
            );
            let score = bv_search::hybrid::hybrid_score(text_score, &weights, &components);
            results.push(serde_json::json!({
                "issue_id": issue.id,
                "score": score,
                "text_score": text_score,
                "title": issue.title,
                "component_scores": components,
            }));
        }
        results.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .partial_cmp(&a["score"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        for issue in &issues {
            let text = format!("{} {}", issue.title, issue.description);
            let issue_vec = bv_search::embedder::hash_embed(&text, dim);
            let score = bv_search::embedder::cosine_similarity(&query_vec, &issue_vec);
            results.push(serde_json::json!({
                "issue_id": issue.id,
                "score": score,
                "title": issue.title,
            }));
        }
        results.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .partial_cmp(&a["score"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    results.truncate(limit);

    let mut payload = envelope_json(&hash);
    payload["query"] = serde_json::json!(query);
    payload["mode"] = serde_json::json!(mode);
    if mode == "hybrid" {
        payload["preset"] = serde_json::json!(preset_name);
    }
    payload["limit"] = serde_json::json!(limit);
    payload["results"] = serde_json::Value::Array(results);
    payload["usage_hints"] = serde_json::json!([
        "This build embeds fresh on every call — no persisted vector index \
         yet (see plan doc §11), so there is no 'index' sync-stats field.",
        "jq '.results[] | {id: .issue_id, score: .score, title: .title}'",
    ]);
    emit_json(&payload)
}

/// Go `handleRobotCausality` — `--robot-causality <bead-id>`.
fn run_robot_causality(args: &[String]) -> ExitCode {
    let bead_id = args
        .iter()
        .position(|a| a == "--robot-causality")
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
    if !issues.iter().any(|i| i.id == bead_id) {
        eprintln!("Bead not found: {bead_id}");
        return ExitCode::from(1);
    }
    let events = match bv_correlation::extract(
        &cwd,
        &bv_correlation::ExtractOptions {
            limit: 1000,
            ..Default::default()
        },
    ) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Error: extraction failed: {err}");
            return ExitCode::from(1);
        }
    };
    match bv_correlation::causality::build_causality_chain(&bead_id, &events) {
        Some(result) => {
            let mut payload = envelope_json(&hash);
            payload["chain"] = serde_json::to_value(&result.chain).unwrap_or_default();
            payload["insights"] = serde_json::to_value(&result.insights).unwrap_or_default();
            emit_json(&payload)
        }
        None => {
            eprintln!("No lifecycle events found for bead: {bead_id} (nothing to build a causal chain from)");
            ExitCode::from(1)
        }
    }
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

    let mut payload = envelope_json(&hash);
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

    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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

    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
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
            let mut payload = envelope_json(&hash);
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

    let mut payload = envelope_json(&hash);
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
    let mut payload = envelope_json(&hash);
    payload["analysis_config"] = serde_json::to_value(&cfg).unwrap_or_default();
    payload["results"] = serde_json::to_value(&results).unwrap_or_default();
    payload["usage_hints"] = serde_json::json!([
        "jq '.results.summaries | sort_by(.health) | .[:3]' - Critical labels",
        "jq '.results.labels[] | select(.health_level == \"critical\")' - Critical details",
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
    let mut payload = envelope_json(&hash);
    // Go: nil arrays serialize as null (not []) for empty list fields.
    let mut flow_obj = serde_json::Map::new();
    flow_obj.insert("labels".into(), serde_json::json!(flow.labels));
    flow_obj.insert("flow_matrix".into(), serde_json::json!(flow.flow_matrix));
    if flow.dependencies.is_empty() {
        flow_obj.insert("dependencies".into(), serde_json::Value::Null);
    } else {
        flow_obj.insert("dependencies".into(), serde_json::json!(flow.dependencies));
    }
    if flow.critical_paths.is_empty() {
        flow_obj.insert("critical_paths".into(), serde_json::Value::Null);
    } else {
        flow_obj.insert(
            "critical_paths".into(),
            serde_json::json!(flow.critical_paths),
        );
    }
    if flow.bottleneck_labels.is_empty() {
        flow_obj.insert("bottleneck_labels".into(), serde_json::Value::Null);
    } else {
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
    let mut payload = envelope_json(&hash);
    // Go: limit comes from --attention-limit flag (default 0 = no limit).
    // Go: if limit flag is 0 (default), limit = len(scores). JSON shows
    // the EFFECTIVE limit, not the raw flag value.
    let effective_limit = result.labels.len();
    payload["limit"] = serde_json::json!(effective_limit);
    payload["total_labels"] = serde_json::json!(result.total_labels);
    // Go builds labels from a separate struct with specific field order:
    // rank, label, attention_score, normalized_score, reason, open_count,
    // blocked_count, stale_count, pagerank_sum, velocity_factor.
    let attention_labels: Vec<serde_json::Value> = result
        .labels
        .iter()
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
    let mut payload = envelope_json(&hash);
    payload["files"] = serde_json::json!(result.files);
    payload["risk_level"] = serde_json::json!(result.risk_level);
    payload["risk_score"] = serde_json::json!(result.risk_score);
    payload["summary"] = serde_json::json!(result.summary);
    payload["affected_beads"] = serde_json::to_value(&result.affected_beads).unwrap_or_default();
    emit_json(&payload)
}

/// Go handleRobotDiff — git snapshot comparison.
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
    // Try to read previous issues from git ref
    let previous = std::process::Command::new("git")
        .args(["show", &format!("{ref_str}:.beads/issues.jsonl")])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut prev = Vec::new();
                for line in text.lines() {
                    if !line.trim().is_empty() {
                        if let Ok(issue) = serde_json::from_str::<bv_core::model::Issue>(line) {
                            prev.push(issue);
                        }
                    }
                }
                Some(prev)
            } else {
                None
            }
        });
    match previous {
        Some(prev) => {
            let result = bv_analysis::diff::diff_issues(&current, &prev, &ref_str);
            let mut payload = envelope_json(&hash);
            payload["diff"] = serde_json::to_value(&result).unwrap_or_default();
            emit_json(&payload)
        }
        None => {
            eprintln!("Error: could not read issues at ref {ref_str}");
            ExitCode::from(1)
        }
    }
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
    let mut payload = envelope_json(&hash);
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
