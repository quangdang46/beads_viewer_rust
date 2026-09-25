//! Byte-exact assertions for the priority brief writer.
//!
//! Every expected document below was produced by running Go
//! `export.GeneratePriorityBriefFromTriageJSON` from parity commit
//! `18afafa` over the matching fixture. They are checked byte-for-byte
//! (not normalised, not line-counted) so a paraphrase of the Go writer
//! cannot pass review: changing the document requires re-deriving it
//! against the Go source in the same commit that changes this file.

use bv_export::priority_brief::{generate_priority_brief_from_triage_json, PriorityBriefConfig};

fn read_fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

///
/// `fixtures/triage_brief_edge.json`, MaxRecommendations/MaxQuickWins/MaxBlockers left at their defaults, DataHash set to `deadbeefcafe`. Exercises bucket boundaries (0/0.25/0.5/0.75/1), out-of-range gauges, multi-byte truncation, an unknown issue type, an empty `reasons` list, and the 5/3/3 limits (six recommendations, four quick wins, four blockers).
#[test]
fn edge_matches_go_byte_for_byte() {
    let config = PriorityBriefConfig {
        max_recommendations: 5,
        max_quick_wins: 3,
        max_blockers: 3,
        include_what_if: true,
        include_legend: true,
        data_hash: "deadbeefcafe".to_string(),
    };
    let triage = read_fixture("triage_brief_edge.json");
    let got = generate_priority_brief_from_triage_json(&triage, &config).expect("brief renders");
    assert_eq!(got, EXPECTED_EDGE);
}

///
/// `fixtures/triage_brief_edge.json`, MaxRecommendations=2, MaxQuickWins=1, MaxBlockers=2, IncludeLegend=false, DataHash=`h1`. Exercises every limit the config actually gates, plus the legend switch.
#[test]
fn edge_config_matches_go_byte_for_byte() {
    let config = PriorityBriefConfig {
        max_recommendations: 2,
        max_quick_wins: 1,
        max_blockers: 2,
        include_what_if: false,
        include_legend: false,
        data_hash: "h1".to_string(),
    };
    let triage = read_fixture("triage_brief_edge.json");
    let got = generate_priority_brief_from_triage_json(&triage, &config).expect("brief renders");
    assert_eq!(got, EXPECTED_EDGE_CONFIG);
}

///
/// `fixtures/triage_brief_empty.json`, all defaults, no data hash. Exercises the three empty-section strings, and a `+05:30` `generated_at` that proves the header renders the source offset rather than UTC.
#[test]
fn empty_matches_go_byte_for_byte() {
    let config = PriorityBriefConfig {
        max_recommendations: 5,
        max_quick_wins: 3,
        max_blockers: 3,
        include_what_if: true,
        include_legend: true,
        data_hash: "".to_string(),
    };
    let triage = read_fixture("triage_brief_empty.json");
    let got = generate_priority_brief_from_triage_json(&triage, &config).expect("brief renders");
    assert_eq!(got, EXPECTED_EMPTY);
}

///
/// `fixtures/triage_small_chain.json`, all defaults, no data hash. Exercises a real captured triage result (ten recommendations, five blockers, one quick win).
#[test]
fn small_chain_matches_go_byte_for_byte() {
    let config = PriorityBriefConfig {
        max_recommendations: 5,
        max_quick_wins: 3,
        max_blockers: 3,
        include_what_if: true,
        include_legend: true,
        data_hash: "".to_string(),
    };
    let triage = read_fixture("triage_small_chain.json");
    let got = generate_priority_brief_from_triage_json(&triage, &config).expect("brief renders");
    assert_eq!(got, EXPECTED_SMALL_CHAIN);
}

///
/// `fixtures/triage_medium_tree.json`, all defaults, no data hash. Exercises a second captured triage result, so a fixture-specific paraphrase cannot pass.
#[test]
fn medium_tree_matches_go_byte_for_byte() {
    let config = PriorityBriefConfig {
        max_recommendations: 5,
        max_quick_wins: 3,
        max_blockers: 3,
        include_what_if: true,
        include_legend: true,
        data_hash: "".to_string(),
    };
    let triage = read_fixture("triage_medium_tree.json");
    let got = generate_priority_brief_from_triage_json(&triage, &config).expect("brief renders");
    assert_eq!(got, EXPECTED_MEDIUM_TREE);
}

///
/// `fixtures/triage_selfrepo.json`, all defaults, no data hash. Exercises a captured triage result with three recommendations, three quick wins and no blockers at all.
#[test]
fn selfrepo_matches_go_byte_for_byte() {
    let config = PriorityBriefConfig {
        max_recommendations: 5,
        max_quick_wins: 3,
        max_blockers: 3,
        include_what_if: true,
        include_legend: true,
        data_hash: "".to_string(),
    };
    let triage = read_fixture("triage_selfrepo.json");
    let got = generate_priority_brief_from_triage_json(&triage, &config).expect("brief renders");
    assert_eq!(got, EXPECTED_SELFREPO);
}

// ---- Expected documents, captured from Go 18afafa ---------------------------

// fixtures/triage_brief_edge.json | MaxRecommendations/MaxQuickWins/MaxBlockers left at their defaults, DataHash set to `deadbeefcafe`
const EXPECTED_EDGE: &str = r####"# 📊 Priority Brief

*Generated: 2026-08-22 14:06*  
*Version: 1.0.0 | Issues: 7*

**Hash:** `deadbeefcafe`

## 📈 Summary

| Open | In Progress | Blocked | Actionable |
|:----:|:-----------:|:-------:|:----------:|
| 7 | 1 | 2 | 4 |

---

## 🎯 Top Recommendations

| # | Issue | Type | P | Score | PR | BW | TI | Top Reason |
|:-:|-------|:----:|:-:|:-----:|:--:|:--:|:--:|------------|
| 1 | **E-1** exact bucket boundaries | 🐛 | P0 | 0.12 | ░░░░ | █░░░ | ████ | - |
| 2 | **E-2** héllo wörld — emojies 🐛🐛… | ✨ | P1 | 0.01 | ░░░░ | ██░░ | ██░░ | exactly-thirty-runes: héllo w… |
| 3 | **E-3** A title that is definite… | 📋 | P2 | 0.99 | ███░ | ███░ | ░░░░ | a reason that runs well past … |
| 4 | **E-4** out of range gauges | 🚀 | P3 | 1.00 | ████ | ░░░░ | ███░ | short |
| 5 | **E-5** unknown type falls throu… | • | P4 | 123.46 | █░░░ | █░░░ | ██░░ | first reason wins |

## ⚡ Quick Wins

| Issue | Reason |
|-------|--------|
| **W-1** one liner | tiny |
| **W-2** a quick win title that exceed… | a quick win reason that also exceeds fo… |
| **W-3** emoji 🐛 win | another reason that is definitely longe… |

## 🚧 Blockers to Clear

| Issue | Unblocks | Ready? |
|-------|:--------:|:------:|
| **B-1** actionable blocker | 7 | ✅ |
| **B-2** a blocker title longer than t… | 0 | ❌ |
| **B-3** third | -3 | ✅ |

---

## 📖 Legend

| Symbol | Meaning |
|:------:|:--------|
| **PR** | PageRank - dependency importance |
| **BW** | Betweenness - critical path frequency |
| **TI** | Time-to-Impact - urgency factor |
| █░░░ | Low (0-25%) |
| ██░░ | Medium (25-50%) |
| ███░ | High (50-75%) |
| ████ | Very High (75-100%) |
"####;

// fixtures/triage_brief_edge.json | MaxRecommendations=2, MaxQuickWins=1, MaxBlockers=2, IncludeLegend=false, DataHash=`h1`
const EXPECTED_EDGE_CONFIG: &str = r####"# 📊 Priority Brief

*Generated: 2026-08-22 14:06*  
*Version: 1.0.0 | Issues: 7*

**Hash:** `h1`

## 📈 Summary

| Open | In Progress | Blocked | Actionable |
|:----:|:-----------:|:-------:|:----------:|
| 7 | 1 | 2 | 4 |

---

## 🎯 Top Recommendations

| # | Issue | Type | P | Score | PR | BW | TI | Top Reason |
|:-:|-------|:----:|:-:|:-----:|:--:|:--:|:--:|------------|
| 1 | **E-1** exact bucket boundaries | 🐛 | P0 | 0.12 | ░░░░ | █░░░ | ████ | - |
| 2 | **E-2** héllo wörld — emojies 🐛🐛… | ✨ | P1 | 0.01 | ░░░░ | ██░░ | ██░░ | exactly-thirty-runes: héllo w… |

## ⚡ Quick Wins

| Issue | Reason |
|-------|--------|
| **W-1** one liner | tiny |

## 🚧 Blockers to Clear

| Issue | Unblocks | Ready? |
|-------|:--------:|:------:|
| **B-1** actionable blocker | 7 | ✅ |
| **B-2** a blocker title longer than t… | 0 | ❌ |

"####;

// fixtures/triage_brief_empty.json | all defaults, no data hash
const EXPECTED_EMPTY: &str = r####"# 📊 Priority Brief

*Generated: 2020-01-02 03:04*  
*Version: 9.9.9 | Issues: 0*

## 📈 Summary

| Open | In Progress | Blocked | Actionable |
|:----:|:-----------:|:-------:|:----------:|
| 0 | 0 | 0 | 0 |

---

## 🎯 Top Recommendations

*No recommendations available.*

## ⚡ Quick Wins

*No quick wins identified.*

## 🚧 Blockers to Clear

*No critical blockers.*

---

## 📖 Legend

| Symbol | Meaning |
|:------:|:--------|
| **PR** | PageRank - dependency importance |
| **BW** | Betweenness - critical path frequency |
| **TI** | Time-to-Impact - urgency factor |
| █░░░ | Low (0-25%) |
| ██░░ | Medium (25-50%) |
| ███░ | High (50-75%) |
| ████ | Very High (75-100%) |
"####;

// fixtures/triage_small_chain.json | all defaults, no data hash
const EXPECTED_SMALL_CHAIN: &str = r####"# 📊 Priority Brief

*Generated: 2026-08-22 14:06*  
*Version: 1.0.0 | Issues: 12*

## 📈 Summary

| Open | In Progress | Blocked | Actionable |
|:----:|:-----------:|:-------:|:----------:|
| 12 | 0 | 0 | 1 |

---

## 🎯 Top Recommendations

| # | Issue | Type | P | Score | PR | BW | TI | Top Reason |
|:-:|-------|:----:|:-:|:-----:|:--:|:--:|:--:|------------|
| 1 | **FIX-6** Chain step 6 | 📋 | P2 | 0.51 | ███░ | ████ | █░░░ | 🔓 Unblocks 1 item(s): FIX-7 |
| 2 | **FIX-5** Chain step 5 | 📋 | P2 | 0.51 | ███░ | ███░ | █░░░ | 🔓 Unblocks 1 item(s): FIX-6 |
| 3 | **FIX-3** Chain step 3 | 📋 | P2 | 0.51 | ███░ | ██░░ | █░░░ | 🔓 Unblocks 1 item(s): FIX-4 |
| 4 | **FIX-2** Chain step 2 | 📋 | P2 | 0.50 | ███░ | █░░░ | █░░░ | 🔓 Unblocks 1 item(s): FIX-3 |
| 5 | **FIX-7** Chain step 7 | 📋 | P2 | 0.50 | ██░░ | ████ | █░░░ | 🔓 Unblocks 1 item(s): FIX-8 |

## ⚡ Quick Wins

| Issue | Reason |
|-------|--------|
| **FIX-1** Chain step 1 | Unblocks 1 items |

## 🚧 Blockers to Clear

| Issue | Unblocks | Ready? |
|-------|:--------:|:------:|
| **FIX-1** Chain step 1 | 1 | ✅ |
| **FIX-10** Chain step 10 | 1 | ❌ |
| **FIX-11** Chain step 11 | 1 | ❌ |

---

## 📖 Legend

| Symbol | Meaning |
|:------:|:--------|
| **PR** | PageRank - dependency importance |
| **BW** | Betweenness - critical path frequency |
| **TI** | Time-to-Impact - urgency factor |
| █░░░ | Low (0-25%) |
| ██░░ | Medium (25-50%) |
| ███░ | High (50-75%) |
| ████ | Very High (75-100%) |
"####;

// fixtures/triage_medium_tree.json | all defaults, no data hash
const EXPECTED_MEDIUM_TREE: &str = r####"# 📊 Priority Brief

*Generated: 2026-08-22 14:06*  
*Version: 1.0.0 | Issues: 121*

## 📈 Summary

| Open | In Progress | Blocked | Actionable |
|:----:|:-----------:|:-------:|:----------:|
| 121 | 0 | 0 | 1 |

---

## 🎯 Top Recommendations

| # | Issue | Type | P | Score | PR | BW | TI | Top Reason |
|:-:|-------|:----:|:-:|:-----:|:--:|:--:|:--:|------------|
| 1 | **TREE-3** Depth-1 node | 📋 | P2 | 0.57 | █░░░ | ████ | █░░░ | 🎯 Completing this unblocks 3 … |
| 2 | **TREE-1** Tree root | 📋 | P2 | 0.54 | ████ | ░░░░ | █░░░ | 🎯 Completing this unblocks 3 … |
| 3 | **TREE-2** Depth-1 node | 📋 | P2 | 0.51 | █░░░ | ██░░ | █░░░ | 🎯 Completing this unblocks 3 … |
| 4 | **TREE-4** Depth-1 node | 📋 | P2 | 0.50 | █░░░ | ██░░ | █░░░ | 🎯 Completing this unblocks 3 … |
| 5 | **TREE-10** Depth-2 node | 📋 | P2 | 0.48 | ░░░░ | ███░ | █░░░ | 🎯 Completing this unblocks 3 … |

## ⚡ Quick Wins

| Issue | Reason |
|-------|--------|
| **TREE-1** Tree root | Unblocks 3 items |

## 🚧 Blockers to Clear

| Issue | Unblocks | Ready? |
|-------|:--------:|:------:|
| **TREE-1** Tree root | 3 | ✅ |
| **TREE-10** Depth-2 node | 3 | ❌ |
| **TREE-11** Depth-2 node | 3 | ❌ |

---

## 📖 Legend

| Symbol | Meaning |
|:------:|:--------|
| **PR** | PageRank - dependency importance |
| **BW** | Betweenness - critical path frequency |
| **TI** | Time-to-Impact - urgency factor |
| █░░░ | Low (0-25%) |
| ██░░ | Medium (25-50%) |
| ███░ | High (50-75%) |
| ████ | Very High (75-100%) |
"####;

// fixtures/triage_selfrepo.json | all defaults, no data hash
const EXPECTED_SELFREPO: &str = r####"# 📊 Priority Brief

*Generated: 2026-08-22 14:06*  
*Version: 1.0.0 | Issues: 46*

## 📈 Summary

| Open | In Progress | Blocked | Actionable |
|:----:|:-----------:|:-------:|:----------:|
| 3 | 0 | 0 | 3 |

---

## 🎯 Top Recommendations

| # | Issue | Type | P | Score | PR | BW | TI | Top Reason |
|:-:|-------|:----:|:-:|:-----:|:--:|:--:|:--:|------------|
| 1 | **beads_viewer_rust-lw1** Port Go self-update (pkg… | 📋 | P1 | 0.10 | ░░░░ | ░░░░ | █░░░ | ✅ Currently unclaimed - avail… |
| 2 | **beads_viewer_rust-tui-history-view-p8n** TUI: implement History/t… | 📋 | P2 | 0.08 | ░░░░ | ░░░░ | █░░░ | ✅ Currently unclaimed - avail… |
| 3 | **beads_viewer_rust-tui-pickers-modals-j5t** TUI: implement remaining… | 📋 | P3 | 0.06 | ░░░░ | ░░░░ | █░░░ | ✅ Currently unclaimed - avail… |

## ⚡ Quick Wins

| Issue | Reason |
|-------|--------|
| **beads_viewer_rust-lw1** Port Go self-update (pkg/upda… | Low complexity, high priority |
| **beads_viewer_rust-tui-history-view-p8n** TUI: implement History/time-t… | Low complexity |
| **beads_viewer_rust-tui-pickers-modals-j5t** TUI: implement remaining pick… | Low complexity |

## 🚧 Blockers to Clear

*No critical blockers.*

---

## 📖 Legend

| Symbol | Meaning |
|:------:|:--------|
| **PR** | PageRank - dependency importance |
| **BW** | Betweenness - critical path frequency |
| **TI** | Time-to-Impact - urgency factor |
| █░░░ | Low (0-25%) |
| ██░░ | Medium (25-50%) |
| ███░ | High (50-75%) |
| ████ | Very High (75-100%) |
"####;
