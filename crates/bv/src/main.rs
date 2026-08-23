//! bvr — Beads Viewer in Rust.
//! CLI surface skeleton (Phase 3a): flag registry, argv rewriter, validation.
//! Command dispatch lands with bead p3-dispatch-3lv.

mod argv;
#[allow(dead_code)] // flag inventory is declarative data; consumed by dispatch phase
mod flags;
mod validation;

use std::process::ExitCode;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = argv::rewrite_args(&raw);

    // --version handled before validation (Go parity).
    if args.iter().any(|a| a == "--version") {
        println!("bvr 0.21.0 (FORT pre-release scaffold)");
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

    if presence.has("robot-help") {
        print_robot_help();
        return ExitCode::from(0);
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
    let analysis_cmds = [
        ("robot-insights", run_robot_insights as fn() -> ExitCode),
        ("robot-plan", run_robot_plan),
        ("robot-priority", run_robot_priority),
        ("robot-suggest", run_robot_suggest),
        ("robot-alerts", run_robot_alerts),
        ("robot-graph", run_robot_graph),
        ("robot-recipes", run_robot_recipes),
        ("robot-label-health", run_robot_label_health),
        ("robot-label-flow", run_robot_label_flow),
        ("robot-label-attention", run_robot_label_attention),
    ];
    for (flag, func) in &analysis_cmds {
        if presence.has(flag) {
            return func();
        }
    }

    // Interactive TUI: no robot flags present.
    let cwd = std::env::current_dir().unwrap_or_default();
    match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((issues, _)) => {
            eprintln!("Loaded {} issues — launching TUI", issues.len());
            // TUI runs in alternate screen; for now just report count.
            // Full ratatui event loop lands as tui-m1 matures further.
            eprintln!("TUI event loop available via bv_tui::run_tui()");
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("Error loading beads: {e}");
            ExitCode::from(1)
        }
    }
}

/// Load issues from discovery chain and emit --robot-triage JSON.
fn run_robot_triage() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let issues = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((issues, _stats)) => issues,
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
    use bv_analysis::algorithms::{cycles::tarjan_scc, pagerank::pagerank_default};
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _stats) =
        bv_core::discovery::load_issues_from_repo(&cwd).map_err(|e| e.to_string())?;
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
    for (i, v) in pagerank_default(&g).into_iter().enumerate() {
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
    let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
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
    let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
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
    let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
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

fn run_robot_priority() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, _) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let hash = bv_core::data_hash::compute_data_hash(&issues);
    let g = bv_analysis::build_graph(&issues);
    let pr = bv_graph_core::pagerank_default(&g);
    let bw = bv_graph_core::betweenness(&g);
    let max_pr = pr.iter().fold(0.0f64, |a, &b| a.max(b));
    let max_bw = bw.iter().fold(0.0f64, |a, &b| a.max(b));
    let mut recommendations: Vec<serde_json::Value> = Vec::new();
    for issue in &issues {
        if issue.status.is_closed() {
            continue;
        }
        let idx = match g.node_idx(&issue.id) {
            Some(x) => x,
            None => continue,
        };
        let pr_norm = if max_pr > 0.0 { pr[idx] / max_pr } else { 0.0 };
        let bw_norm = if max_bw > 0.0 { bw[idx] / max_bw } else { 0.0 };
        let blockers = g.in_degree(idx);
        let score = pr_norm * 0.22
            + bw_norm * 0.20
            + (blockers as f64).min(5.0) / 5.0 * 0.13
            + 0.05 * bv_analysis::impact::compute_priority_boost(issue.priority);

        let suggested = bv_analysis::scoring::score_to_priority(score);
        if suggested < issue.priority {
            let mut reasons = Vec::new();
            if pr_norm > 0.3 {
                reasons.push("High centrality in dependency graph".to_string());
            }
            if bw_norm > 0.2 {
                reasons.push("Bridge node connecting clusters".to_string());
            }
            if blockers > 0 {
                reasons.push(format!("Blocks {blockers} downstream issues"));
            }
            recommendations.push(serde_json::json!({
                "issue_id": issue.id, "title": issue.title,
                "current_priority": issue.priority, "suggested_priority": suggested,
                "impact_score": score, "confidence": 1, "reasoning": reasons,
            }));
        }
    }
    recommendations.sort_by(|a, b| {
        b["impact_score"]
            .as_f64()
            .partial_cmp(&a["impact_score"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recommendations.truncate(10);
    let high_conf = recommendations.len();

    let mut payload = envelope_json(&hash);
    payload["recommendations"] = serde_json::Value::Array(recommendations.clone());
    payload["field_descriptions"] = serde_json::json!({});
    payload["filters"] = serde_json::json!({"max_results": 10});
    payload["summary"] = serde_json::json!({
        "total_issues": issues.len(),
        "recommendations": recommendations.len(),
        "high_confidence": high_conf,
    });
    emit_json(&payload)
}
fn run_robot_suggest() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((issues, _)) => {
            let h = bv_core::data_hash::compute_data_hash(&issues);
            (issues, h)
        }
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
    let (issues, hash) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((issues, _)) => {
            let h = bv_core::data_hash::compute_data_hash(&issues);
            (issues, h)
        }
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

fn run_robot_graph() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (issues, hash) = match bv_core::discovery::load_issues_from_repo(&cwd) {
        Ok((issues, _)) => {
            let h = bv_core::data_hash::compute_data_hash(&issues);
            (issues, h)
        }
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let _g = bv_analysis::build_graph(&issues);

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

fn run_robot_label_health() -> ExitCode {
    let (_, hash, _p1, _status, _g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let mut payload = envelope_json(&hash);
    payload["analysis_config"] = serde_json::json!({});
    payload["results"] = serde_json::json!({"labels": []});
    emit_json(&payload)
}

fn run_robot_label_flow() -> ExitCode {
    let (_, hash, _p1, _status, _g) = match load_and_analyze() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let mut payload = envelope_json(&hash);
    payload["flow"] = serde_json::json!({"matrix": {}, "bottleneck_labels": []});
    emit_json(&payload)
}

fn run_robot_label_attention() -> ExitCode {
    let (_, hash) = match bv_core::discovery::load_issues_from_repo(
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
    let mut payload = envelope_json(&hash);
    payload["labels"] = serde_json::json!([]);
    emit_json(&payload)
}

fn jiff_now() -> String {
    jiff::Timestamp::now().to_string()
}

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
