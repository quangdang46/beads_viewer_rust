//! Differential test for the hooks loader against the Go reference.
//!
//! `pkg/hooks/config.go` is almost entirely defaulting and validation, and
//! every one of its rules is observable only through a normalised hook: the
//! phase-dependent `on_error` default, the zero-means-unset timeout, the
//! generated names, the dropped empty commands, the accumulated warnings. The
//! expected strings below were produced by running Go's `hooks.Loader` over
//! the same `.bv/hooks.yaml` files at parity commit `18afafa` and rendering
//! the result in a fixed shape, so a change to any of those rules shows up as
//! a diff rather than as a plausible-looking document.
//!
//! The one thing not compared literally is the YAML *parse* error: Go reports
//! it through `gopkg.in/yaml.v3` and this crate through `serde_yaml_ng`, so
//! the wording differs. The test asserts that a malformed file is rejected,
//! not how.

use bv_export::hooks::{go_duration_string, HookPhase, HooksError, Loader};

/// Render a loaded config the way the Go harness does, so the two outputs can
/// be compared line for line.
fn render(dir: &std::path::Path) -> String {
    let mut loader = Loader::new(dir);
    match loader.load() {
        Ok(()) => {}
        Err(e) => {
            // A Go-side error is a single `ERROR ...` line whose text after
            // the path is contractual; the path is machine-specific.
            let text = e.to_string();
            if let HooksError::Parse { source, .. } = &e {
                assert!(
                    !source.to_string().is_empty(),
                    "a parse error must carry the YAML cause"
                );
            }
            let rendered = match &e {
                HooksError::Timeout { raw, reason } => {
                    format!("invalid timeout {raw:?}: {reason}")
                }
                _ => text,
            };
            return format!("ERROR {rendered}\n");
        }
    }

    let mut out = format!("HAS {}\n", loader.has_hooks());
    for warning in loader.warnings() {
        out.push_str(&format!("WARN {warning}\n"));
    }
    for phase in [HookPhase::PreExport, HookPhase::PostExport] {
        for hook in loader.hooks(phase) {
            out.push_str(&format!(
                "HOOK {phase} name={:?} timeout={} on_error={:?} command={:?}\n",
                hook.name,
                go_duration_string(hook.timeout),
                hook.on_error.as_str(),
                hook.command,
            ));
            for (key, value) in &hook.env {
                out.push_str(&format!("ENV {phase} {key}={value:?}\n"));
            }
        }
    }
    out
}

fn check(fixture: &str, expected: &str) {
    let dir = fixture_dir(fixture);
    assert_eq!(render(&dir), expected, "fixture {fixture}");
}

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("hooks")
        .join(name)
}

#[test]
fn a_directory_without_hooks_yaml_has_no_hooks() {
    // config.go:114-119 — a missing file is not an error.
    check("01_missing_dir", "HAS false\n");
}

#[test]
fn a_config_without_a_hooks_key_has_no_hooks() {
    check("02_hooks_absent", "HAS false\n");
}

#[test]
fn defaults_are_applied_per_phase() {
    // config.go:132-159 — the 30s timeout, the generated `{phase}-{n}` name
    // and the phase-dependent on_error default.
    check(
        "03_defaults",
        concat!(
            "HAS true\n",
            "HOOK pre-export name=\"pre-export-1\" timeout=30s on_error=\"fail\" command=\"echo pre\"\n",
            "HOOK post-export name=\"post-export-1\" timeout=30s on_error=\"continue\" command=\"echo post\"\n",
        ),
    );
}

#[test]
fn every_timeout_spelling_go_accepts() {
    // config.go:208-237 — duration strings, bare numbers, YAML scalars that
    // are not strings, and the zero that means "unset".
    check(
        "04_timeouts",
        concat!(
            "HAS true\n",
            "HOOK pre-export name=\"pre-export-1\" timeout=30s on_error=\"fail\" command=\"a\"\n",
            "HOOK pre-export name=\"pre-export-2\" timeout=1m30s on_error=\"fail\" command=\"b\"\n",
            "HOOK pre-export name=\"pre-export-3\" timeout=45s on_error=\"fail\" command=\"c\"\n",
            "HOOK pre-export name=\"pre-export-4\" timeout=500ms on_error=\"fail\" command=\"d\"\n",
            "HOOK pre-export name=\"pre-export-5\" timeout=30s on_error=\"fail\" command=\"e\"\n",
            "HOOK pre-export name=\"pre-export-6\" timeout=250ms on_error=\"fail\" command=\"f\"\n",
            "HOOK pre-export name=\"pre-export-7\" timeout=2h0m0s on_error=\"fail\" command=\"g\"\n",
            "HOOK pre-export name=\"pre-export-8\" timeout=1.5µs on_error=\"fail\" command=\"h\"\n",
            "HOOK pre-export name=\"pre-export-9\" timeout=1µs on_error=\"fail\" command=\"i\"\n",
            "HOOK pre-export name=\"pre-export-10\" timeout=45s on_error=\"fail\" command=\"j\"\n",
        ),
    );
}

#[test]
fn invalid_timeouts_report_go_parse_duration_errors() {
    // config.go:213 / :222 — each Go failure shape, in the order the
    // fixtures were captured.
    check(
        "05_bad_timeouts",
        "ERROR invalid timeout \"-5s\": must be non-negative\n",
    );
    check(
        "06_bad_timeout_bare",
        "ERROR invalid timeout \"-5\": time: missing unit in duration \"-5\"\n",
    );
    check(
        "07_bad_timeout_junk",
        "ERROR invalid timeout \"soon\": time: invalid duration \"soon\"\n",
    );
    check(
        "08_bad_timeout_unit",
        "ERROR invalid timeout \"5x\": time: unknown unit \"x\" in duration \"5x\"\n",
    );
    // A YAML boolean reaches the parser as its text.
    check(
        "15_bool_timeout",
        "ERROR invalid timeout \"true\": time: invalid duration \"true\"\n",
    );
}

#[test]
fn on_error_is_trimmed_lowercased_and_validated() {
    // config.go:142-148 — including the warning text and its phase-specific
    // fallback, which differ per phase.
    check(
        "09_on_error_variants",
        concat!(
            "HAS true\n",
            "WARN pre-export hook 4 has invalid on_error \"retry\"; using \"fail\"\n",
            "WARN post-export hook 2 has invalid on_error \"nope\"; using \"continue\"\n",
            "HOOK pre-export name=\"pre-export-1\" timeout=30s on_error=\"fail\" command=\"a\"\n",
            "HOOK pre-export name=\"pre-export-2\" timeout=30s on_error=\"fail\" command=\"b\"\n",
            "HOOK pre-export name=\"pre-export-3\" timeout=30s on_error=\"continue\" command=\"c\"\n",
            "HOOK pre-export name=\"pre-export-4\" timeout=30s on_error=\"fail\" command=\"d\"\n",
            "HOOK pre-export name=\"pre-export-5\" timeout=30s on_error=\"fail\" command=\"e\"\n",
            "HOOK post-export name=\"post-export-1\" timeout=30s on_error=\"continue\" command=\"f\"\n",
            "HOOK post-export name=\"post-export-2\" timeout=30s on_error=\"continue\" command=\"g\"\n",
        ),
    );
}

#[test]
fn empty_commands_are_dropped_and_counted_by_file_position() {
    // config.go:133-136 — the warning counts the 1-based position in the
    // file, so skipping entry 1 does not renumber entry 3.
    check(
        "10_empty_commands",
        concat!(
            "HAS true\n",
            "WARN pre-export hook 1 has empty command; skipping\n",
            "WARN pre-export hook 3 has empty command; skipping\n",
            "WARN post-export hook 1 has empty command; skipping\n",
            "HOOK pre-export name=\"real\" timeout=30s on_error=\"fail\" command=\"echo hi\"\n",
            "HOOK pre-export name=\"after\" timeout=30s on_error=\"fail\" command=\"echo after\"\n",
        ),
    );
}

#[test]
fn explicit_names_win_and_env_is_sorted() {
    // config.go:149-151 — env keys are rendered in sorted order, matching the
    // order the executor also applies them in.
    check(
        "11_names_and_env",
        concat!(
            "HAS true\n",
            "HOOK pre-export name=\"first\" timeout=30s on_error=\"fail\" command=\"echo 1\"\n",
            "ENV pre-export ALPHA=\"a\"\n",
            "ENV pre-export ZED=\"z\"\n",
            "HOOK pre-export name=\"second\" timeout=30s on_error=\"fail\" command=\"echo 2\"\n",
        ),
    );
}

#[test]
fn generated_names_are_sequential_per_phase() {
    // config.go:149-151.
    check(
        "12_all_defaults_named",
        concat!(
            "HAS true\n",
            "HOOK post-export name=\"post-export-1\" timeout=30s on_error=\"continue\" command=\"a\"\n",
            "HOOK post-export name=\"post-export-2\" timeout=30s on_error=\"continue\" command=\"b\"\n",
            "HOOK post-export name=\"post-export-3\" timeout=30s on_error=\"continue\" command=\"c\"\n",
        ),
    );
}

#[test]
fn a_null_or_empty_timeout_is_unset() {
    // config.go:208-210 — both spellings of "no timeout" take the default.
    check(
        "14_null_timeout",
        concat!(
            "HAS true\n",
            "HOOK pre-export name=\"pre-export-1\" timeout=30s on_error=\"fail\" command=\"a\"\n",
            "HOOK pre-export name=\"pre-export-2\" timeout=30s on_error=\"fail\" command=\"b\"\n",
        ),
    );
}

#[test]
fn malformed_yaml_is_rejected() {
    // config.go:123 — the wording comes from the YAML library, which differs
    // between Go and Rust, so only the rejection is contractual.
    let dir = fixture_dir("13_malformed");
    let mut loader = Loader::new(&dir);
    let err = loader.load().unwrap_err();
    assert!(
        matches!(err, HooksError::Parse { .. }),
        "expected a parse error, got {err:?}"
    );
}
