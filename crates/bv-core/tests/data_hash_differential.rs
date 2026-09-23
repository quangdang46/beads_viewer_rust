//! Differential tests: our data_hash must equal the Go oracle's for every
//! fixture class.
//!
//! Oracle updated 2026-09-24 to go_commit 18afafa (bv v0.25.0), which replaced
//! the v0.20.0 flat NUL-joined encoding with a length-prefixed fingerprint
//! scheme and widened the digest from 16 to 64 hex chars. The previous
//! `compute_data_hash_v020` retains the old algorithm for historical reference.
use bv_core::data_hash::compute_data_hash;
use bv_core::model::Issue;

fn load_fixture(name: &str) -> Vec<Issue> {
    let path = format!(
        "{}/../../tests/fixtures/{}/.beads/issues.jsonl",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {name} readable: {e}"));
    let mut issues = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let issue: Issue = serde_json::from_str(line).expect("fixture line parses");
        if issue.validate().is_ok() {
            issues.push(issue);
        }
    }
    issues
}

#[test]
fn small_chain_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("small_chain")),
        "abed23dcb3ba317ee50db516b30891f85fae889bd62891f799b403c4ec7cc7be"
    );
}

#[test]
fn medium_tree_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("medium_tree")),
        "b5d952c60ab9b368611a4bdfd856e73f9c97f6a3abaf340e5293ffb5304f576d"
    );
}

#[test]
fn large_cyclic_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("large_cyclic_600")),
        "b1d36a4c4270b5beb0bf9aeb1aa48e842059ecb4323bd47ad0f5d143e09578c9"
    );
}

#[test]
fn xl_2500_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("xl_2500")),
        "ee56db4fb68f5132e96c98f8326282a2584fa0ffb8597ad2536590435d8c8566"
    );
}
