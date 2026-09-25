//! End-to-end assertions for the export-hook executor.
//!
//! These tests really spawn processes. That is the point: the previous
//! revision of `hooks.rs` had no executor at all, so the closest thing to a
//! test was an assertion that a hard-coded `Vec` had four elements. What the
//! tests below pin is the behaviour a project actually depends on — the
//! `BV_*` contract, `${VAR}` expansion in a hook's own `env:` block, the
//! `on_error` policy deciding whether a failure propagates, the timeout, and
//! the summary text Go prints.

use bv_export::hooks::{
    run_hooks, Executor, ExportContext, HookPhase, HooksConfig, HooksError, Loader, OnError,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn context() -> ExportContext {
    ExportContext {
        export_path: "/tmp/report.md".into(),
        export_format: "markdown".into(),
        issue_count: 7,
        timestamp: "2024-01-15T10:00:00Z".parse().unwrap(),
    }
}

/// A command that succeeds, prints a marker, and is valid under both `cmd /C`
/// and `sh -c`.
fn echo_command(text: &str) -> String {
    format!("echo {text}")
}

/// A command that exits non-zero.
fn fail_command() -> String {
    "exit 1".to_string()
}

/// A command that outlives any deadline the tests hand it.
fn slow_command() -> String {
    if cfg!(target_os = "windows") {
        // `timeout /t` needs a console; pinging the loopback does not.
        "ping -n 4 127.0.0.1 > nul".to_string()
    } else {
        "sleep 4".to_string()
    }
}

fn config_with(hooks: &[(HookPhase, &str, OnError, Duration)]) -> HooksConfig {
    let build = |phase: HookPhase| -> Vec<bv_export::hooks::Hook> {
        hooks
            .iter()
            .filter(|(p, _, _, _)| *p == phase)
            .map(|(_, command, on_error, timeout)| bv_export::hooks::Hook {
                name: command.to_string(),
                command: (*command).to_string(),
                timeout: *timeout,
                env: Default::default(),
                on_error: *on_error,
            })
            .collect()
    };
    let mut config = HooksConfig::default();
    config.hooks.pre_export = build(HookPhase::PreExport);
    config.hooks.post_export = build(HookPhase::PostExport);
    config
}

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bvr-hooks-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".bv")).expect("create temp dir");
    dir
}

fn write_hooks(dir: &Path, body: &str) {
    std::fs::write(dir.join(".bv").join("hooks.yaml"), body).expect("write hooks.yaml");
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn pre_export_hook_sees_the_context_environment() {
    // executor.go:138-141 — the four BV_* variables reach the child process.
    let command = if cfg!(target_os = "windows") {
        "echo %BV_EXPORT_FORMAT%-%BV_ISSUE_COUNT%".to_string()
    } else {
        "echo $BV_EXPORT_FORMAT-$BV_ISSUE_COUNT".to_string()
    };
    let config = config_with(&[(
        HookPhase::PreExport,
        &command,
        OnError::Fail,
        Duration::from_secs(30),
    )]);
    let mut executor = Executor::new(config, context());
    executor.run_pre_export().expect("hook succeeds");

    let results = executor.results();
    assert_eq!(results.len(), 1);
    assert!(results[0].success, "hook failed: {:?}", results[0].error);
    assert_eq!(results[0].stdout, "markdown-7");
    assert_eq!(results[0].phase, HookPhase::PreExport);
}

#[test]
fn hook_env_expands_context_variables() {
    // executor.go:153-161 — `${VAR}` sees both the OS environment and the
    // export context, and a later key can read an earlier one.
    //
    // Two different expansions are in play and both are checked: Rust expands
    // `${...}` inside the `env:` *values* (a `${GREETING}!` that reads the
    // `GREETING` declared above it), while the *command* is expanded by
    // whichever shell `getShellCommand` selected.
    let (command, expected) = if cfg!(target_os = "windows") {
        ("echo %GREETING%/%SECOND%", "markdown-world/markdown-world!")
    } else {
        ("echo $GREETING/$SECOND", "markdown-world/markdown-world!")
    };
    let dir = tempdir("expand");
    write_hooks(
        &dir,
        &format!(
            r#"
hooks:
  post-export:
    - name: expand
      command: {command:?}
      on_error: continue
      env:
        GREETING: "${{BV_EXPORT_FORMAT}}-world"
        SECOND: "${{GREETING}}!"
"#
        ),
    );
    let mut loader = Loader::new(&dir);
    loader.load().unwrap();
    // The raw declaration is untouched — expansion happens at spawn time.
    assert_eq!(
        loader.hooks(HookPhase::PostExport)[0].env["GREETING"],
        "${BV_EXPORT_FORMAT}-world"
    );

    let config = loader.config();
    let mut executor = Executor::new(config, context());
    executor.run_post_export().unwrap();
    let result = &executor.results()[0];
    assert!(result.success, "hook failed: {:?}", result.error);
    assert_eq!(result.stdout, expected);
}

#[test]
fn pre_export_failure_stops_the_remaining_hooks() {
    // executor.go:63-68 — a `fail` policy aborts the phase on first failure.
    let dir = tempdir("abort");
    let marker = dir.join("second-ran.txt");
    let marker_arg = marker.display().to_string();
    let second = format!("echo ran > {marker_arg}");
    let config = config_with(&[
        (
            HookPhase::PreExport,
            &fail_command(),
            OnError::Fail,
            Duration::from_secs(30),
        ),
        (
            HookPhase::PreExport,
            &second,
            OnError::Fail,
            Duration::from_secs(30),
        ),
    ]);
    let mut executor = Executor::new(config, context());
    let err = executor.run_pre_export().unwrap_err();
    assert!(matches!(err, HooksError::PreExportFailed { .. }));
    assert_eq!(
        err.to_string(),
        "pre-export hook \"exit 1\" failed: exit status 1"
    );
    assert_eq!(executor.results().len(), 1, "the second hook must not run");
    assert!(read(&marker).is_empty(), "the second hook must not run");
}

#[test]
fn post_export_continue_swallows_the_failure() {
    // executor.go:81-89 + config.go:154-159 — a post-export hook that fails
    // under the default `continue` policy reports but does not propagate.
    let config = config_with(&[(
        HookPhase::PostExport,
        &fail_command(),
        OnError::Continue,
        Duration::from_secs(30),
    )]);
    let mut executor = Executor::new(config, context());
    executor
        .run_post_export()
        .expect("continue policy does not propagate");
    let results = executor.results();
    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert_eq!(
        results[0].error.as_ref().unwrap().to_string(),
        "exit status 1"
    );
}

#[test]
fn post_export_fail_reports_the_first_failure_only() {
    // executor.go:81-89 — every hook still runs; only the first is returned.
    let config = config_with(&[
        (
            HookPhase::PostExport,
            &fail_command(),
            OnError::Fail,
            Duration::from_secs(30),
        ),
        (
            HookPhase::PostExport,
            &echo_command("second"),
            OnError::Fail,
            Duration::from_secs(30),
        ),
    ]);
    let mut executor = Executor::new(config, context());
    assert!(executor.run_post_export().is_err());
    assert_eq!(executor.results().len(), 2, "post-export runs every hook");
    assert!(!executor.results()[0].success);
    assert!(executor.results()[1].success);
}

#[test]
fn a_hook_that_outlives_its_timeout_is_killed() {
    // executor.go:143-151.
    let config = config_with(&[(
        HookPhase::PreExport,
        &slow_command(),
        OnError::Fail,
        Duration::from_millis(250),
    )]);
    let mut executor = Executor::new(config, context());
    let err = executor.run_pre_export().unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("timeout after 250ms"),
        "unexpected error: {rendered}"
    );
    assert_eq!(executor.results().len(), 1);
    assert!(!executor.results()[0].success);
}

#[test]
fn summary_matches_the_go_shape() {
    // executor.go:249-271.
    let config = config_with(&[
        (
            HookPhase::PostExport,
            &echo_command("ok"),
            OnError::Continue,
            Duration::from_secs(30),
        ),
        (
            HookPhase::PostExport,
            &fail_command(),
            OnError::Continue,
            Duration::from_secs(30),
        ),
    ]);
    let mut executor = Executor::new(config, context());
    executor.run_post_export().unwrap();
    let summary = executor.summary();
    let mut lines = summary.lines();
    assert_eq!(
        lines.next().unwrap(),
        "Hook execution: 1 succeeded, 1 failed"
    );
    assert!(lines.next().unwrap().starts_with("  [OK] echo ok ("));
    assert_eq!(lines.next().unwrap(), "  [FAIL] exit 1: exit status 1");
    assert!(lines.next().is_none());
}

#[test]
fn summary_of_no_hooks_is_the_go_sentinel() {
    // executor.go:250-251.
    let executor = Executor::new(HooksConfig::default(), context());
    assert_eq!(executor.summary(), "No hooks executed");
}

#[test]
fn run_hooks_is_none_when_disabled_or_unconfigured() {
    // executor.go:287-301.
    let dir = tempdir("run-hooks");
    assert!(run_hooks(&dir, context(), true).unwrap().is_none());

    // A config file with only empty commands leaves nothing to run.
    write_hooks(&dir, "hooks:\n  pre-export:\n    - command: \"  \"\n");
    assert!(run_hooks(&dir, context(), false).unwrap().is_none());

    write_hooks(
        &dir,
        "hooks:\n  pre-export:\n    - name: real\n      command: \"echo hi\"\n",
    );
    let executor = run_hooks(&dir, context(), false)
        .expect("hooks load")
        .expect("hooks configured");
    assert_eq!(
        executor.results().len(),
        0,
        "RunHooks only builds the executor"
    );
}

#[test]
fn loader_defaults_and_warnings_survive_a_real_file() {
    // config.go:132-159 end to end.
    let dir = tempdir("defaults");
    write_hooks(
        &dir,
        r#"
hooks:
  pre-export:
    - command: "echo one"
    - command: "echo two"
      timeout: "45s"
  post-export:
    - command: "echo three"
      on_error: "fail"
"#,
    );
    let mut loader = Loader::new(&dir);
    loader.load().unwrap();
    assert!(loader.warnings().is_empty(), "{:?}", loader.warnings());

    let pre = loader.hooks(HookPhase::PreExport);
    assert_eq!(pre[0].name, "pre-export-1");
    assert_eq!(pre[0].timeout, Duration::from_secs(30));
    assert_eq!(pre[1].timeout, Duration::from_secs(45));

    let post = loader.hooks(HookPhase::PostExport);
    assert_eq!(post[0].name, "post-export-1");
    // The phase default is overridden by an explicit `fail`, not by a guess.
    assert_eq!(post[0].on_error, OnError::Fail);
}
