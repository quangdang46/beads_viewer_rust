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
fn undispatched_robot_command_fails_fast_instead_of_launching_tui() {
    // --robot-search is a registered primary (flags::ROBOT_PRIMARIES) with no
    // dispatch handler yet; it must error with exit 2, not fall through to
    // the interactive TUI (which would hang this test / any CI runner).
    let (code, _, stderr) = run(&["--robot-search", "--search", "foo"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("not yet implemented"), "{stderr}");
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
