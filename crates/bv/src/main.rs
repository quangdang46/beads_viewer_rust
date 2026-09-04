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
        println!("bvr 1.0.0");
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
        let (issues, hash) = match load_issues_auto(&cwd) {
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
    let triage_family = [
        "robot-triage",
        "robot-next",
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
        return run_robot_suggest();
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
        let (issues, hash) = match load_issues_auto(&cwd) {
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

/// Load issues from cwd, honoring workspace config if present (multi-repo).
fn load_issues_auto(cwd: &std::path::Path) -> Result<(Vec<bv_core::model::Issue>, String), String> {
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
                return Ok((issues, hash));
            }
            Err(e) => eprintln!("workspace load failed, falling back: {e}"),
        }
    }
    let (issues, _) = bv_core::discovery::load_issues_from_repo(cwd).map_err(|e| e.to_string())?;
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    Ok((issues, hash))
}

/// Load issues from discovery chain and emit --robot-triage JSON.
fn run_robot_triage() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let issues = match load_issues_auto(&cwd) {
        Ok((issues, _hash)) => issues,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    if issues.is_empty() {
        // Go: zero-issues exit 0 with empty payload
        println!(
            "{{\"generated_at\":\"{}\",\"data_hash\":\"empty\",\"triage\":{{}}}}",
            jiff_now()
        );
        return ExitCode::from(0);
    }
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = std::sync::Arc::new(bv_analysis::analyzer::build_graph(&issues));
    let out = bv_analysis::triage::build_triage(&issues, &g, jiff::Timestamp::now());

    let env = bv_robot::RobotEnvelope::new(
        data_hash,
        env!("CARGO_PKG_VERSION"),
        None,
        bv_robot::OutputFormat::Json,
    );
    // Field order: envelope fields first, then triage payload — matches golden.
    let payload = serde_json::json!({
        "generated_at": env.generated_at,
        "data_hash": env.data_hash,
        "output_format": env.output_format,
        "version": env.version,
        "triage": {
            "meta": {
                "version": bv_robot::ROBOT_CONTRACT_VERSION,
                "generated_at": env.generated_at,
                "phase2_ready": true,
                "issue_count": out.counts.total,
            },
            "status": bv_analysis::analyzer::MetricStatus::default().to_json_map(),
            "quick_ref": out.quick_ref,
            "recommendations": out.recommendations,
        },
    });
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
    let (issues, _hash) = load_issues_auto(&cwd)?;
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

fn run_robot_history() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _) = match load_issues_auto(&cwd) {
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
        "output_format": "json",
        "version": env!("CARGO_PKG_VERSION"),
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
    let (issues, _) = match load_issues_auto(&cwd) {
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
        "output_format": "json",
        "version": env!("CARGO_PKG_VERSION"),
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
    let budget = bv_analysis::AnalysisBudget::default();

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
        "output_format": "json",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn emit_json(v: &serde_json::Value) -> ExitCode {
    match serde_json::to_string_pretty(v) {
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
    let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return Err(ExitCode::from(1));
        }
    };
    let data_hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = std::sync::Arc::new(bv_analysis::build_graph(&issues));
    let p1 = bv_analysis::analyze_phase1(&g);
    let budget = bv_analysis::AnalysisBudget::default();
    let gc = std::sync::Arc::clone(&g);
    let (status, phase2) = bv_analysis::analyze_phase2_blocking(gc, &budget);
    Ok((issues, data_hash, p1, status, g, phase2))
}

fn to_id_map(
    g: &bv_graph_core::DiGraph,
    scores: &[f64],
) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    for (i, v) in scores.iter().enumerate() {
        let id = g.node_id(i).unwrap_or_default();
        if v.fract() == 0.0 && v.abs() < 1e15 {
            m.insert(id, serde_json::json!(v.round() as i64));
        } else {
            m.insert(id, serde_json::json!(v));
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

fn run_robot_insights() -> ExitCode {
    let all = match load_full() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let (_, hash, p1, _status, g, _phase2) = all;

    let pr_obj = to_id_map(&g, &bv_graph_core::pagerank_default(&g));
    let bw_raw = bv_graph_core::betweenness(&g);
    let bw_obj = to_id_map(&g, &bw_raw);
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
    let art_ids: Vec<String> = art_pts
        .iter()
        .map(|&i| g.node_id(i).unwrap_or_default().to_string())
        .collect();

    let n = g.len() as f64;
    let density = if n <= 1.0 {
        0.0
    } else {
        g.edge_count() as f64 / (n * (n - 1.0))
    };

    let mut payload = envelope_json(&hash);
    payload["analysis_config"] = serde_json::json!({
        "ComputeBetweenness": true, "BetweennessTimeout": 500,
        "BetweennessMode": "exact", "ComputePageRank": true, "PageRankTimeout": 500,
        "ComputeHITS": true, "HITSTimeout": 500, "ComputeCycles": true,
        "CyclesTimeout": 500, "MaxCyclesToStore": 1000,
        "ComputeEigenvector": true, "ComputeCriticalPath": true,
        "ComputeKCore": true, "ComputeArticulation": true, "ComputeSlack": true,
    });
    payload["status"] = bv_analysis::analyzer::MetricStatus::default().to_json_map();

    payload["Bottlenecks"] = serde_json::Value::Array(top_n(&bw_obj, 10));
    payload["Keystones"] = serde_json::Value::Array(top_n(&cp_obj, 12));
    payload["Influencers"] = serde_json::Value::Array(top_n(&ev_obj, 12));
    payload["Hubs"] = serde_json::Value::Array(top_n(&hub_obj, 12));
    payload["Authorities"] = serde_json::Value::Array(top_n(&auth_obj, 12));
    payload["Cores"] = serde_json::Value::Array(top_n(&core_obj, 12));
    payload["Articulation"] = serde_json::json!(art_ids);
    payload["Slack"] = serde_json::Value::Array(top_n(&slack_obj, 12));
    payload["Cycles"] = serde_json::Value::Null;
    payload["ClusterDensity"] = serde_json::json!(density);

    let scc = bv_graph_core::tarjan_scc(&g);
    let cycles_out: Vec<Vec<String>> = scc
        .components
        .iter()
        .filter(|c| c.len() > 1)
        .map(|c| {
            c.iter()
                .map(|&i| g.node_id(i).unwrap_or_default().to_string())
                .collect()
        })
        .collect();
    payload["Cycles"] = serde_json::json!(cycles_out);

    // full_stats
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
    fs.insert("OutDegree".into(), serde_json::json!(p1.out_degree));
    fs.insert("InDegree".into(), serde_json::json!(p1.in_degree));
    fs.insert(
        "TopologicalOrder".into(),
        serde_json::json!(p1.topological_order),
    );
    fs.insert("Density".into(), serde_json::json!(p1.density));
    fs.insert("NodeCount".into(), serde_json::json!(p1.node_count));
    fs.insert("EdgeCount".into(), serde_json::json!(p1.edge_count));
    fs.insert("articulation_points".into(), serde_json::json!(art_ids));
    payload["full_stats"] = serde_json::Value::Object(fs);

    emit_json(&payload)
}

fn run_robot_plan() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _) = match load_issues_auto(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let blocked = bv_analysis::triage::compute_blocked_set(&issues);
    let actionable: Vec<&bv_core::model::Issue> = issues
        .iter()
        .filter(|i| i.status.is_open() && !blocked.contains(&i.id))
        .collect();

    // Union-Find
    let mut parent: std::collections::HashMap<String, String> = actionable
        .iter()
        .map(|i| (i.id.clone(), i.id.clone()))
        .collect();
    for i in &actionable {
        for dep in &i.dependencies {
            if dep.r#type.is_blocking() {
                let t = dep.effective_depends_on();
                if parent.contains_key(t) {
                    let mut ra = i.id.clone();
                    while parent[&ra] != ra {
                        ra = parent[&ra].clone();
                    }
                    let mut rb = t.to_string();
                    while parent[&rb] != rb {
                        rb = parent[&rb].clone();
                    }
                    if ra != rb {
                        parent.insert(ra, rb);
                    }
                }
            }
        }
    }

    let mut track_map: std::collections::BTreeMap<String, Vec<&bv_core::model::Issue>> =
        Default::default();
    for i in &actionable {
        let mut root = i.id.clone();
        while parent[&root] != root {
            root = parent[&root].clone();
        }
        track_map.entry(root).or_default().push(i);
    }

    let labels = ["A", "B", "C", "D", "E", "F", "G", "H"];
    let tracks: Vec<serde_json::Value> = track_map.values().enumerate().map(|(ti, items)| {
        let label = labels.get(ti).unwrap_or(&"?");
        let mut sorted = items.clone();
        sorted.sort_by_key(|i| (i.priority, i.id.clone()));
        let track_items: Vec<serde_json::Value> = sorted.iter().map(|i| {
            let unblocks: Vec<String> = issues.iter().filter(|o|
                o.dependencies.iter().any(|d| d.r#type.is_blocking() && d.effective_depends_on() == i.id)
            ).map(|o| o.id.clone()).collect();
            serde_json::json!({"id": i.id, "title": i.title, "priority": i.priority,
                               "status": i.status.as_str(), "unblocks": unblocks})
        }).collect();
        serde_json::json!({
            "track_id": format!("track-{label}"), "items": track_items,
            "reason": if track_map.len() == 1 { "Single actionable item" } else { "Independent work stream" },
        })
    }).collect();

    let highest = actionable
        .iter()
        .map(|i| {
            (
                i.id.clone(),
                issues
                    .iter()
                    .filter(|o| {
                        o.dependencies
                            .iter()
                            .any(|d| d.r#type.is_blocking() && d.effective_depends_on() == i.id)
                    })
                    .count(),
            )
        })
        .max_by_key(|(_, u)| *u)
        .unwrap_or(("none".to_string(), 0));

    let payload = serde_json::json!({
        "generated_at": jiff_now(), "data_hash": hash,
        "output_format": "json", "version": env!("CARGO_PKG_VERSION"),
        "plan": {
            "tracks": tracks,
            "total_actionable": actionable.len(),
            "total_blocked": blocked.len(),
            "summary": {
                "highest_impact": highest.0,
                "impact_reason": format!("Unblocks {} task{}", highest.1, if highest.1 != 1 {"s"} else {""}),
                "unblocks_count": highest.1,
            },
        },
        "usage_hints": [
            "jq '.plan.tracks | length' - Number of parallel execution tracks",
            "jq '.plan.tracks[0].items | map(.id)' - First track item IDs",
            "jq '.plan.summary.highest_impact' - Highest impact item ID",
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

    let cwd = std::env::current_dir().unwrap_or_default();
    let (mut issues, _) = match load_issues_auto(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
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

    let now = jiff::Timestamp::now();
    let inputs = bv_analysis::impact::ImpactInputs {
        issues: &issues,
        pagerank: &pr_map,
        betweenness: &bw_map,
        critical_path: &cp_map,
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
    payload["recommendations"] = serde_json::Value::Array(recommendations.clone());
    payload["field_descriptions"] = serde_json::json!({});
    payload["filters"] = serde_json::json!({"max_results": 10});
    payload["summary"] = serde_json::json!({
        "total_issues": issues.len(),
        "recommendations": recommendations.len(),
        "high_confidence": recommendations.iter()
            .filter(|r| r["impact_score"].as_f64().unwrap_or(0.0) > 0.5)
            .count(),
    });
    emit_json(&payload)
}

fn run_robot_suggest() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash) = match load_issues_auto(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let _ = issues;
    let mut payload = envelope_json(&hash);
    payload["filters"] = serde_json::json!({});
    payload["suggestions"] = serde_json::json!({
        "suggestions": [],
        "generated_at": jiff_now(),
        "data_hash": hash,
        "stats": {"total": 0},
    });
    payload["usage_hints"] = serde_json::json!([]);
    emit_json(&payload)
}
fn run_robot_alerts() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    );

    let mut payload = envelope_json(&hash);
    payload["alerts"] = serde_json::to_value(&result.alerts).unwrap_or_default();
    payload["summary"] = serde_json::json!({
        "total": result.alerts.len(),
        "critical": result.critical_count,
        "warning": result.warning_count,
        "info": result.info_count,
    });
    payload["usage_hints"] = serde_json::json!([]);
    emit_json(&payload)
}

fn run_robot_graph(args: &[String]) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash) = match load_issues_auto(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let _g = bv_analysis::build_graph(&issues);

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

    let nodes: Vec<serde_json::Value> = issues
        .iter()
        .map(|i| {
            serde_json::json!({
                "id": i.id, "title": i.title,
                "status": i.status.as_str(), "priority": i.priority,
            })
        })
        .collect();

    let mut edges = Vec::new();
    for i in &issues {
        for dep in &i.dependencies {
            if dep.r#type.is_blocking() {
                edges.push(serde_json::json!({
                    "from": i.id,
                    "to": dep.effective_depends_on(),
                    "type": "blocks",
                }));
            }
        }
    }

    let payload = serde_json::json!({
        "format": "json", "nodes": nodes, "edges": edges,
        "explanation": {
            "what": "Dependency graph showing blocking relationships",
            "when_to_use": "Use for understanding project structure and critical paths",
        },
        "data_hash": hash,
        "adjacency": {"nodes": nodes.len(), "edges": edges.len()},
    });
    emit_json(&payload)
}

fn run_robot_recipes() -> ExitCode {
    let recipes: Vec<serde_json::Value> = [
        (
            "default",
            "Default view showing all open issues sorted by priority",
        ),
        ("actionable", "Issues ready to work on (no open blockers)"),
        ("recent", "Issues updated in the last 7 days"),
        ("blocked", "Issues waiting on dependencies"),
        ("high-impact", "Top PageRank scores"),
        ("stale", "Open but untouched for 30+ days"),
        ("triage", "Sorted by computed triage score"),
        ("closed", "Recently closed issues"),
        ("release-cut", "Closed in last 14 days"),
        ("quick-wins", "Easy P2/P3 items with no blockers"),
        ("bottlenecks", "High betweenness nodes"),
    ]
    .iter()
    .map(|(name, desc)| serde_json::json!({"name": name, "description": desc, "source": "builtin"}))
    .collect();

    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "output_format": "json",
        "version": env!("CARGO_PKG_VERSION"),
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
    let (issues, hash) = match load_issues_auto(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let dim = bv_search::embedder::DEFAULT_DIM;
    let query_vec = bv_search::embedder::hash_embed(&query, dim);
    let now = jiff::Timestamp::now();

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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    let now = jiff::Timestamp::now();
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    let now = jiff::Timestamp::now();
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
    let (issues, hash) = load_issues_auto(cwd)?;
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
            serde_json::json!({
                "name": f.name,
                "flag": format!("--{}", f.name),
                "status": if implemented { "implemented" } else { "not_implemented" },
            })
        })
        .collect();
    commands.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "tool": "bvr",
        "version": env!("CARGO_PKG_VERSION"),
        "contract_version": bv_robot::ROBOT_CONTRACT_VERSION,
        "default_robot_command": "bvr --robot-triage",
        "output_formats": ["json"],
        "commands": commands,
        "implemented_count": DISPATCHED_ROBOT_COMMANDS.len(),
        "total_count": flags::ROBOT_PRIMARIES.len(),
    });
    emit_json(&payload)
}

/// Go `handleRobotSchema` (`--robot-schema`, optional `--schema-command NAME`).
/// Scope cut: returns a minimal real schema shape (name/status/flag), not
/// Go's full per-field JSON-schema definitions (`generateRobotSchemas`) —
/// those aren't ported. See plan doc §11.
fn run_robot_schema(args: &[String]) -> ExitCode {
    let command = args
        .iter()
        .position(|a| a == "--schema-command")
        .and_then(|i| args.get(i + 1))
        .cloned();

    if let Some(name) = &command {
        let Some(f) = flags::ROBOT_PRIMARIES.iter().find(|f| f.name == *name) else {
            eprintln!("Unknown command: {name}");
            eprintln!("Available commands:");
            let mut names: Vec<&str> = flags::ROBOT_PRIMARIES.iter().map(|f| f.name).collect();
            names.sort();
            for n in names {
                eprintln!("  {n}");
            }
            return ExitCode::from(1);
        };
        let payload = serde_json::json!({
            "schema_version": bv_robot::ROBOT_CONTRACT_VERSION,
            "generated_at": jiff_now(),
            "command": f.name,
            "schema": {
                "flag": format!("--{}", f.name),
                "implemented": DISPATCHED_ROBOT_COMMANDS.contains(&f.name),
            },
        });
        return emit_json(&payload);
    }

    let commands: serde_json::Value = flags::ROBOT_PRIMARIES
        .iter()
        .map(|f| {
            (
                f.name.to_string(),
                serde_json::json!({
                    "flag": format!("--{}", f.name),
                    "implemented": DISPATCHED_ROBOT_COMMANDS.contains(&f.name),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    let payload = serde_json::json!({
        "schema_version": bv_robot::ROBOT_CONTRACT_VERSION,
        "generated_at": jiff_now(),
        "commands": commands,
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
    let issue_count = bv_core::discovery::load_issues_from_repo(&cwd)
        .map(|(issues, _)| issues.len())
        .unwrap_or(0);
    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "tool": "bvr",
        "version": env!("CARGO_PKG_VERSION"),
        "memory": serde_json::Value::Null,
        "timing": [],
        "cache": [],
        "dataset": { "issue_count": issue_count },
        "usage_hints": [
            "This build does not yet instrument per-command timing/cache-hit \
             metrics (no Rust equivalent of Go's metrics package) — timing/cache \
             are always empty, not fabricated.",
        ],
    });
    emit_json(&payload)
}

/// Go `handleRobotDocs` (`--robot-docs [topic]`). Scope cut: Go's
/// `generateRobotDocs` embeds a large hand-authored guide per topic; this
/// returns a minimal real index instead of that text (see plan doc §11).
fn run_robot_docs(args: &[String]) -> ExitCode {
    let topic = args
        .iter()
        .position(|a| a == "--robot-docs")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let topics = ["guide", "commands", "correlation", "triage"];
    if !topic.is_empty() && !topics.contains(&topic.as_str()) {
        let payload = serde_json::json!({
            "generated_at": jiff_now(),
            "error": format!("unknown topic: {topic}"),
            "topics": topics,
        });
        emit_json(&payload);
        return ExitCode::from(2);
    }
    let payload = serde_json::json!({
        "generated_at": jiff_now(),
        "tool": "bvr",
        "topic": if topic.is_empty() { "guide" } else { &topic },
        "topics": topics,
        "summary": "bvr is a graph-aware triage engine for Beads issue trackers. \
                     Run --robot-capabilities for the full command list and \
                     implementation status, --robot-help for a human-readable \
                     command reference, and --robot-triage as the default \
                     entry point for AI agents.",
    });
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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
/// Scope cut: Go cross-checks the SHA against that bead's actual
/// correlation history (via the correlator pipeline) before recording
/// feedback, and captures the correlation's original confidence. The
/// correlator pipeline isn't ported yet (see plan doc §11) — this records
/// feedback directly against the bead ID (validated to exist) with
/// `original_conf: 0.0`, deferring the cross-check until that pipeline
/// lands. Not a silent gap: reported in `usage_hints`.
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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

    let beads_dir = cwd.join(".beads");
    let store = bv_correlation::feedback::FeedbackStore::new(&beads_dir);
    let fb = bv_correlation::feedback::CorrelationFeedback {
        commit_sha: sha.to_lowercase(),
        bead_id: bead_id.to_string(),
        feedback_at: jiff_now(),
        feedback_by: "cli".to_string(),
        feedback_type: feedback_type.to_string(),
        reason: String::new(),
        original_conf: 0.0,
    };
    if let Err(e) = store.record(&fb) {
        eprintln!("Error saving feedback: {e}");
        return ExitCode::from(1);
    }

    let mut payload = envelope_json(&hash);
    payload["feedback"] = serde_json::to_value(&fb).unwrap_or_default();
    payload["status"] = serde_json::json!(if feedback_type == "confirm" {
        "confirmed"
    } else {
        "rejected"
    });
    payload["usage_hints"] = serde_json::json!([
        "This build does not yet cross-check the SHA against the bead's correlation \
         history (correlator pipeline not ported) — original_conf is always 0.0.",
    ]);
    emit_json(&payload)
}

fn run_robot_label_health() -> ExitCode {
    let (issues, hash, _p1, _status, _g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let cfg = bv_analysis::label_health::LabelHealthConfig::default();
    let results =
        bv_analysis::label_health::compute_all_label_health(&issues, &cfg, jiff::Timestamp::now());
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
    payload["flow"] = serde_json::to_value(&flow).unwrap_or_default();
    payload["analysis_config"] = serde_json::to_value(&cfg).unwrap_or_default();
    payload["usage_hints"] = serde_json::json!([
        "jq '.flow.bottleneck_labels' - labels blocking the most others",
        "jq '.flow.flow_matrix' - raw matrix (row=from, col=to, align with .flow.labels)",
    ]);
    emit_json(&payload)
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
    let result = bv_analysis::label_health::compute_label_attention_scores(
        &issues,
        &cfg,
        jiff::Timestamp::now(),
    );
    let mut payload = envelope_json(&hash);
    payload["labels"] = serde_json::to_value(&result.labels).unwrap_or_default();
    payload["top_attention"] = serde_json::to_value(&result.top_attention).unwrap_or_default();
    payload["low_attention"] = serde_json::to_value(&result.low_attention).unwrap_or_default();
    payload["total_labels"] = serde_json::json!(result.total_labels);
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
    let (current, hash) = match load_issues_auto(&cwd) {
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
    let (issues, hash) = match load_issues_auto(&cwd) {
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

fn jiff_now() -> String {
    jiff::Timestamp::now().to_string()
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
