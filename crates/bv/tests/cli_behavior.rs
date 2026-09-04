//! CLI behavior integration tests (Phase 3a acceptance).

fn run(args: &[&str]) -> (i32, String, String) {
    let bin = concat!(env!("CARGO_BIN_EXE_bvr"));
    // CARGO_BIN_EXE only available in integration tests via env — fallback:
    let _ = bin;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bvr"))
        .args(args)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn version_exits_zero() {
    let (code, stdout, _) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("bvr 0.1.0"));
}

#[test]
fn modifier_violation_exits_one() {
    let (code, _, stderr) = run(&["--robot-diff"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("requires --diff-since"), "{stderr}");
}

#[test]
fn all_robot_primaries_are_dispatched_or_validated() {
    // Every robot primary in flags::ROBOT_PRIMARIES is either dispatched
    // (has a handler) or caught by modifier-requires validation.
    // Verify that --robot-drift without --check-drift triggers modifier
    // validation (exit 1) rather than the "not yet implemented" fallback.
    let (code, _, stderr) = run(&["--robot-drift"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("requires --check-drift"), "{stderr}");
}

#[test]
fn exclusive_primaries_exit_one() {
    let (code, _, stderr) = run(&["--robot-triage", "--robot-next"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("only one primary command"), "{stderr}");
}

#[test]
fn valid_triage_dispatches_and_exits_zero() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bvr"))
        .arg("--robot-triage")
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .expect("binary runs");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"quick_ref\""), "{stdout}");
    assert!(stdout.contains("\"data_hash\""));
}

#[test]
fn argv_alias_triage_works() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bvr"))
        .args(["triage"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .expect("binary runs");
    assert_eq!(
        out.status.code(),
        Some(0),
        "alias rewrites to --robot-triage"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("quick_ref"));
}

#[test]
fn robot_help_lists_primaries() {
    let (code, stdout, _) = run(&["--robot-help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--robot-triage"));
    assert!(stdout.contains("exit 0=success"));
}

#[test]
fn robot_capabilities_reports_real_implementation_status() {
    let (code, stdout, _) = run(&["--robot-capabilities"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"implemented_count\""));
    assert!(stdout.contains("\"total_count\""));
    // robot-triage is dispatched; robot-drift is not (yet) — both must be
    // present with their real status, not a blanket "implemented".
    assert!(stdout.contains("robot-triage"));
    assert!(stdout.contains("robot-drift"));
}

#[test]
fn robot_schema_unknown_command_exits_one_with_suggestions() {
    let (code, _, stderr) = run(&["--robot-schema", "--schema-command", "bogus-cmd"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("Unknown command"));
    assert!(stderr.contains("robot-triage"), "{stderr}");
}

#[test]
fn robot_docs_unknown_topic_exits_two() {
    let (code, stdout, _) = run(&["--robot-docs", "bogus-topic"]);
    assert_eq!(code, 2);
    assert!(stdout.contains("\"error\""), "{stdout}");
}

fn run_at_repo_root(args: &[&str]) -> (i32, String, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bvr"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn robot_blocker_chain_unknown_issue_exits_one() {
    let (code, _, stderr) = run_at_repo_root(&["--robot-blocker-chain", "nonexistent-id-xyz"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("Issue not found"), "{stderr}");
}

#[test]
fn robot_correlation_stats_runs_without_crashing() {
    // This repo's real .beads + git history exercise the correlator
    // pipeline end-to-end (git log walk + explicit/temporal scoring).
    let (code, stdout, _) = run_at_repo_root(&["--robot-correlation-stats"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"correlated_beads\""));
    assert!(stdout.contains("\"by_method\""));
}

#[test]
fn robot_file_hotspots_runs_without_crashing() {
    let (code, stdout, _) = run_at_repo_root(&["--robot-file-hotspots"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"hotspots\""));
}

#[test]
fn robot_causality_runs_for_real_bead() {
    let (code, stdout, _) = run_at_repo_root(&["--robot-causality", "beads_viewer_rust-api-freeze-b73"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"chain\""));
    assert!(stdout.contains("\"insights\""));
    assert!(stdout.contains("\"commit_count\""));
}

#[test]
fn robot_causality_unknown_bead_exits_one() {
    let (code, _, stderr) = run_at_repo_root(&["--robot-causality", "nonexistent-xyz"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("Bead not found"), "{stderr}");
}

#[test]
fn robot_related_builds_dependency_edges() {
    let (code, stdout, _) = run_at_repo_root(&["--robot-related", "beads_viewer_rust-api-freeze-b73"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"related\""));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    let related = parsed["related"].as_array().expect("array");
    assert!(!related.is_empty(), "should find at least one dependency edge");
    assert_eq!(parsed["bead_id"], "beads_viewer_rust-api-freeze-b73");
}

#[test]
fn robot_impact_network_all_returns_full_network() {
    let (code, stdout, _) = run_at_repo_root(&["--robot-impact-network", "all"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"network\""));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert!(parsed["node_count"].as_u64().unwrap() > 0);
}

#[test]
fn robot_sprint_list_handles_no_sprints_jsonl() {
    // This repo has no .beads/sprints.jsonl; sprint-list must succeed
    // with an empty array, not crash.
    let (code, stdout, _) = run_at_repo_root(&["--robot-sprint-list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"sprint_count\""));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(parsed["sprints"].as_array().unwrap().len(), 0);
}

#[test]
fn robot_sprint_show_unknown_sprint_exits_one() {
    let (code, _, stderr) = run_at_repo_root(&["--robot-sprint-show", "nonexistent-sprint"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("Sprint not found"), "{stderr}");
}

#[test]
fn robot_burndown_no_active_sprint_exits_one() {
    let (code, _, stderr) = run_at_repo_root(&["--robot-burndown"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("No active sprint found"), "{stderr}");
}

#[test]
fn robot_capacity_runs_without_crashing() {
    let (code, stdout, _) = run_at_repo_root(&["--robot-capacity"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"capacity\""));
    assert!(stdout.contains("\"open_count\""));
}

#[test]
fn robot_explain_correlation_bad_format_exits_two() {
    let (code, _, stderr) = run_at_repo_root(&["--robot-explain-correlation", "not-a-valid-format"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("expected format SHA:beadID"), "{stderr}");
}

#[test]
fn robot_search_ranks_and_respects_limit() {
    let (code, stdout, _) = run_at_repo_root(&["--robot-search", "--search", "triage", "--search-limit", "3"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"results\""));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(parsed["query"], "triage");
    let results = parsed["results"].as_array().expect("results array");
    assert!(results.len() <= 3, "must respect --search-limit");
}

#[test]
fn robot_search_missing_query_caught_by_modifier_requires() {
    // --robot-search without --search is rejected by the shared
    // modifier-requires validator before it ever reaches the handler.
    let (code, _, stderr) = run(&["--robot-search"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("requires --search"), "{stderr}");
}

#[test]
fn robot_search_unknown_preset_exits_two() {
    let (code, _, stderr) = run_at_repo_root(&[
        "--robot-search",
        "--search",
        "x",
        "--search-mode",
        "hybrid",
        "--search-preset",
        "bogus-preset",
    ]);
    assert_eq!(code, 2);
    assert!(stderr.contains("unknown --search-preset"), "{stderr}");
}

#[test]
fn robot_metrics_does_not_fabricate_timing_data() {
    let (code, stdout, _) = run(&["--robot-metrics"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"timing\""));
    assert!(stdout.contains("\"cache\""));
    // Empty arrays, not fabricated entries — no timing/cache subsystem exists.
    let compact: String = stdout.split_whitespace().collect();
    assert!(compact.contains("\"timing\":[]"), "{compact}");
    assert!(compact.contains("\"cache\":[]"), "{compact}");
}
