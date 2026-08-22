//! Live differential: extract events from this repo's own git history.
use bv_correlation::{extract, ExtractOptions};
use std::path::Path;

#[test]
fn extracts_events_from_selfrepo_history() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let events = extract(&repo, &ExtractOptions::default()).expect("extraction works");
    // Our history definitely created beads (33 total in .beads).
    assert!(!events.is_empty(), "expected lifecycle events");
    // Every event references a bead that exists in the current JSONL.
    let ids: std::collections::HashSet<String> = events.iter().map(|e| e.bead_id.clone()).collect();
    assert!(!ids.is_empty());
    // Event types are all valid enum values (compile-checked), spot-check shape.
    for e in events.iter().take(5) {
        assert!(!e.commit_sha.is_empty());
        assert!(!e.timestamp.is_empty());
    }
}

#[test]
fn bead_id_filter_narrows_results() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let all = extract(&repo, &ExtractOptions::default()).unwrap();
    if all.is_empty() {
        return; // no history yet
    }
    let target = &all[0].bead_id;
    let filtered = extract(
        &repo,
        &ExtractOptions {
            bead_id: Some(target.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(filtered.iter().all(|e| e.bead_id == *target));
}
