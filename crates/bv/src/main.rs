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

    eprintln!("bvr: remaining commands arrive with later dispatch slices.");
    ExitCode::from(2)
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
