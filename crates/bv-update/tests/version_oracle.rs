//! Differential test: Rust version handling vs the Go oracle.
//!
//! `tests/data/version_oracle.tsv` was produced by running Go's verbatim
//! `parseVersion` / `compareVersions` / `isNewerVersion` / `isDevelopmentVersion`
//! (beads_viewer `pkg/updater/updater.go:389-613`, commit 18afafa) over every
//! pair in `tests/data/version_corpus.tsv`. Each row is:
//!
//! ```text
//! compareVersions(a,b) \t parseVersion(a).ok \t parseVersion(a).err \t
//! parseVersion(b).err \t coreComponents \t isDev(a)|isDev(b) \t isNewerVersion(a,b)
//! ```
//!
//! Do not hand-edit either file. To regenerate the oracle, build a throwaway
//! `package main` that `include`s lines 389-613 of `updater.go` verbatim (they
//! are self-contained — no `version.Version` or other package reference), add
//! a driver that reads `a\tb` rows on stdin and prints the seven fields above,
//! and pipe the corpus through it. The committed oracle was verified
//! byte-identical to a fresh run of exactly that recipe.

use bv_update::version::{
    compare_versions, is_development_version, is_newer_version, parse_version,
};

const CORPUS: &str = include_str!("data/version_corpus.tsv");
const ORACLE: &str = include_str!("data/version_oracle.tsv");

fn parse_ok(raw: &str) -> (bool, String, usize) {
    match parse_version(raw) {
        Ok(p) => (true, "ok".to_string(), p.core_components()),
        Err(e) => (false, e, 0),
    }
}

fn rust_row(a: &str, b: &str) -> String {
    let (a_ok, a_err, a_components) = parse_ok(a);
    let (_b_ok, b_err, _b_components) = parse_ok(b);
    let dev = format!(
        "{}|{}",
        is_development_version(a),
        is_development_version(b)
    );
    let newer = match is_newer_version(a, b) {
        Ok(v) => v.to_string(),
        Err(e) => format!("err:{e}"),
    };
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        compare_versions(a, b),
        u8::from(a_ok),
        a_err,
        b_err,
        a_components,
        dev,
        newer
    )
}

#[test]
fn matches_go_version_oracle() {
    let corpus: Vec<&str> = CORPUS.lines().filter(|l| !l.is_empty()).collect();
    let oracle: Vec<&str> = ORACLE.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        corpus.len(),
        oracle.len(),
        "corpus and oracle row counts differ"
    );
    assert!(
        corpus.len() >= 4000,
        "corpus shrank to {} rows; regenerate both files together",
        corpus.len()
    );

    let mut mismatches = Vec::new();
    for (pair, want) in corpus.iter().zip(oracle.iter()) {
        let (a, b) = pair.split_once('\t').expect("corpus row must be `a\\tb`");
        let got = rust_row(a, b);
        if got != *want {
            mismatches.push(format!(
                "compare({a:?}, {b:?})\n  go:   {want}\n  rust: {got}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} / {} rows diverge from the Go oracle:\n{}",
        mismatches.len(),
        corpus.len(),
        mismatches
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
