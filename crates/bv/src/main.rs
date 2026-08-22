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

    // Drift / baseline dispatch (Phase 3d).
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
