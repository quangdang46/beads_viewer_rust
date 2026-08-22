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
    assert!(stdout.starts_with("bvr 0.21.0"));
}

#[test]
fn modifier_violation_exits_one() {
    let (code, _, stderr) = run(&["--robot-diff"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("requires --diff-since"), "{stderr}");
}

#[test]
fn exclusive_primaries_exit_one() {
    let (code, _, stderr) = run(&["--robot-triage", "--robot-next"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("only one primary command"), "{stderr}");
}

#[test]
fn valid_invocation_reaches_dispatch_notice() {
    let (code, _, stderr) = run(&["--robot-triage"]);
    assert_eq!(code, 2); // dispatch not yet implemented
    assert!(stderr.contains("Phase 3c"), "{stderr}");
}

#[test]
fn argv_alias_triage_works() {
    let (code, _, stderr) = run(&["triage"]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("Phase 3c"),
        "alias must rewrite to --robot-triage: {stderr}"
    );
}

#[test]
fn robot_help_lists_primaries() {
    let (code, stdout, _) = run(&["--robot-help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--robot-triage"));
    assert!(stdout.contains("exit 0=success"));
}
