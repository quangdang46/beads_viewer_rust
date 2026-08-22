//! Differential tests: our data_hash must equal the Go oracle's for every
//! fixture class (hashes captured from go_commit 9ace029 via --robot-triage).
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
        "124ebe0f74ba42c8"
    );
}

#[test]
fn medium_tree_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("medium_tree")),
        "628d02d09a74a512"
    );
}

#[test]
fn large_cyclic_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("large_cyclic_600")),
        "ed0cdfdc8020ac1c"
    );
}

#[test]
fn xl_2500_matches_go() {
    assert_eq!(
        compute_data_hash(&load_fixture("xl_2500")),
        "0d6d9720762a2a57"
    );
}
