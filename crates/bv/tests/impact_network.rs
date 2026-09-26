//! `--robot-impact-network` payload shape, ported from Go's
//! `correlation.NewNetworkBuilderWithIssues(report, issues).BuildAt` +
//! `ImpactNetwork.ToResult` (pkg/correlation/network.go:128/180/857), driven
//! by `handleRobotImpactNetwork` (cmd/bv/robot_registry.go:3330).
//!
//! The old Rust build assembled the network from `bv_correlation::network`,
//! a simplified model with a different wire shape: edges were `from` / `to` /
//! `type` / `shared`, dependency edges were emitted for EVERY dependency
//! rather than only blocking ones, shared-file edges needed a commit list Go
//! does not consult, and the command emitted a top-level `edge_count` and
//! `node_count` that Go has no equivalent for while omitting `depth`, `stats`,
//! `clusters` and `top_connected` entirely. These tests pin Go's shape.
//!
//! `git init` is required: `handleRobotImpactNetwork` calls
//! `correlation.ValidateRepository` before it reads anything. The commit walk
//! is what produces the shared-file edges, so the fixture has to be a real
//! repository with a real history.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

/// A four-bead graph whose git history produces one edge of every family Go
/// builds, so the whole shape is exercised:
///
/// * `NN-1` and `NN-2` are closed in the SAME commit, which also touches two
///   source files they both own — that is the `co_committed` correlation
///   (`ExtractAllCoCommits`, cocommit.go:644, only fires on claimed/closed
///   events). One shared commit gives a `shared_commit` edge; two shared files
///   give a `shared_file` edge of weight 2, which is exactly `detectClusters`'
///   `minWeight` (network.go:472), so the pair forms a cluster.
/// * `NN-3` is closed in a later commit, and depends on `NN-1` with a BLOCKING
///   type — a `dependency` edge.
/// * `NN-4` is closed in a last commit, and depends on `NN-1` with a
///   non-blocking type, which `addDependencyEdges` skips (`dep.Type.IsBlocking()`,
///   network.go:404). It is therefore an isolated node.
///
/// Bead IDs need two or more UPPERCASE letters: Go's generic explicit-ID
/// pattern is `\b([A-Z]{2,10}-\d+)\b` (explicit.go:84).
///
/// `tag` keeps the parallel fixtures off each other's directory.
fn fixture_repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bvr_impact_network_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".beads")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();

    // The four beads; `closed` lists the IDs this revision has closed, and the
    // two dependencies are present from the start so both are loaded.
    let write_beads = |closed: &[&str]| {
        let mut out = String::new();
        for (id, title, priority) in [
            ("NN-1", "Root", 1),
            ("NN-2", "Peer", 2),
            ("NN-3", "Blocked child", 2),
            ("NN-4", "Related child", 2),
        ] {
            let status = if closed.contains(&id) {
                "closed"
            } else {
                "open"
            };
            let deps = match id {
                // Go's Dependency unmarshals `depends_on_id` (model/types.go:352).
                "NN-3" => {
                    r#","dependencies":[{"issue_id":"NN-3","depends_on_id":"NN-1","type":"blocks"}]"#
                }
                "NN-4" => {
                    r#","dependencies":[{"issue_id":"NN-4","depends_on_id":"NN-1","type":"related"}]"#
                }
                _ => "",
            };
            out.push_str(&format!(
                r#"{{"id":"{id}","title":"{title}","status":"{status}","priority":{priority},"issue_type":"task","labels":["net"],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"{deps}}}"#
            ));
            out.push('\n');
        }
        std::fs::write(dir.join(".beads").join("issues.jsonl"), out).unwrap();
    };
    let commit = |dir: &PathBuf, msg: &str| {
        git(dir, &["add", "-A"]);
        git(
            dir,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ],
        );
    };

    write_beads(&[]);
    std::fs::write(dir.join("src").join("lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(dir.join("src").join("other.rs"), "pub fn b() {}\n").unwrap();
    git(&dir, &["init", "-q", "."]);
    commit(&dir, "seed");

    // Close NN-1 and NN-2 in one commit that also edits both shared files.
    std::fs::write(
        dir.join("src").join("lib.rs"),
        "pub fn a() {}\npub fn c() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src").join("other.rs"),
        "pub fn b() {}\npub fn d() {}\n",
    )
    .unwrap();
    write_beads(&["NN-1", "NN-2"]);
    commit(&dir, "close NN-1 and NN-2");

    std::fs::write(dir.join("src").join("child.rs"), "pub fn e() {}\n").unwrap();
    write_beads(&["NN-1", "NN-2", "NN-3"]);
    commit(&dir, "close NN-3");

    std::fs::write(dir.join("src").join("child4.rs"), "pub fn f() {}\n").unwrap();
    write_beads(&["NN-1", "NN-2", "NN-3", "NN-4"]);
    commit(&dir, "close NN-4");

    dir
}

fn git(dir: &PathBuf, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {args:?} failed in the fixture repo");
}

fn run(dir: &PathBuf, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bvr"))
        .current_dir(dir)
        .args(["--robot-impact-network"])
        .args(args)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn network(dir: &PathBuf, target: &str) -> Value {
    let (code, stdout, stderr) = run(dir, &[target]);
    assert_eq!(code, 0, "stderr: {stderr}");
    serde_json::from_str(&stdout).expect("stdout is JSON")
}

/// Go `NetworkEdge` (network.go:24) — `from_bead` / `to_bead` / `edge_type` /
/// `weight` / `details`. The old Rust build emitted `from` / `to` / `type` /
/// `shared`, so every one of these five names is load-bearing.
#[test]
fn edges_carry_go_field_names() {
    let dir = fixture_repo("fields");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    let edges = out["network"]["edges"]
        .as_array()
        .expect("network.edges is an array")
        .clone();
    assert!(!edges.is_empty(), "the fixture must produce edges");
    for edge in &edges {
        let keys: Vec<&str> = edge
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["from_bead", "to_bead", "edge_type", "weight", "details"],
            "edge key set and order"
        );
        assert!(edge["edge_type"].is_string());
        assert!(edge["weight"].is_i64());
        assert!(edge["details"].is_array());
        // Go's `makeNetworkEdgeKey` canonicalises the pair, so from <= to.
        assert!(edge["from_bead"].as_str().unwrap() <= edge["to_bead"].as_str().unwrap());
    }
}

/// Go's `details` are capped at 5 and sorted (`limitStrings(details, 5)` after
/// `sort.Strings`, network.go:330-331), and each family records a different
/// thing: a short SHA for shared commits, the path for shared files, and the
/// DECLARED direction `from -> to` for dependencies — which is not
/// necessarily the canonicalised key order.
#[test]
fn edge_details_are_sorted_capped_and_family_specific() {
    let dir = fixture_repo("details");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    for edge in out["network"]["edges"].as_array().unwrap() {
        let details: Vec<&str> = edge["details"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        assert!(details.len() <= 5, "limitStrings caps details at 5");
        let mut sorted = details.clone();
        sorted.sort_unstable();
        assert_eq!(details, sorted, "details are sorted before capping");

        match edge["edge_type"].as_str().unwrap() {
            "dependency" => {
                assert!(
                    details.iter().all(|d| d.contains(" -> ")),
                    "a dependency detail records `from -> to`: {details:?}"
                );
            }
            "shared_file" => {
                assert!(
                    details.iter().all(|d| d.contains('/')),
                    "a shared_file detail is a path: {details:?}"
                );
            }
            "shared_commit" => {
                assert!(
                    details.iter().all(|d| d.len() <= 7),
                    "a shared_commit detail is a short SHA: {details:?}"
                );
            }
            other => panic!("unknown edge_type {other:?}"),
        }
    }
}

/// `addDependencyEdges` (network.go:404) keeps only `dep.Type.IsBlocking()`.
/// The old build emitted an edge per dependency of any type, so `NN-4`'s
/// `related` link showed up as a spurious edge.
#[test]
fn only_blocking_dependencies_produce_dependency_edges() {
    let dir = fixture_repo("blocking");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    let dep_edges: Vec<&Value> = out["network"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["edge_type"] == "dependency")
        .collect();
    let pairs: Vec<(String, String)> = dep_edges
        .iter()
        .map(|e| {
            (
                e["from_bead"].as_str().unwrap().to_string(),
                e["to_bead"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(
        pairs.contains(&("NN-1".into(), "NN-3".into())),
        "the blocks dependency must be an edge: {pairs:?}"
    );
    assert!(
        !pairs.contains(&("NN-1".into(), "NN-4".into())),
        "a `related` dependency is not blocking (IsBlocking, model.go:149) and \
         must not become an edge: {pairs:?}"
    );
}

/// `BuildAt` derives nodes from the report's histories, and every one of them
/// carries the full `NetworkNode` (network.go:64). The old build emitted only
/// `id` / `title` / `status` / `degree`.
#[test]
fn nodes_carry_the_full_go_network_node() {
    let dir = fixture_repo("nodes");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    let nodes = out["network"]["nodes"]
        .as_object()
        .expect("nodes is an object");
    assert!(
        nodes.contains_key("NN-1"),
        "NN-1 has commits, so it is a node"
    );
    let n1 = &nodes["NN-1"];
    assert_eq!(n1["bead_id"], "NN-1");
    assert_eq!(n1["title"], "Root");
    // The node's status is the HISTORY's status, not the issue's.
    assert_eq!(n1["status"], "closed");
    // Go takes priority from the issue set, so NN-1's priority 1 wins.
    assert_eq!(n1["priority"], 1);
    // Not a tombstone, so it was eligible for the shared-file index.
    assert!(n1["cluster_id"].is_i64());
    assert!(n1["commit_count"].is_i64());
    assert!(n1["file_count"].is_i64());
    // Go never assigns `connectivity` in BuildAt, so it is always 0.
    assert_eq!(n1["connectivity"], 0);
    assert!(n1["last_activity"].is_string());
    // A node with no commits is absent: BuildAt iterates report.Histories.
    let keys: Vec<&str> = nodes.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["NN-1", "NN-2", "NN-3", "NN-4"],
        "one node per history"
    );
}

/// Go's `NetworkStats` (network.go:101) plus the two derived top-level lists
/// `ToResult` adds (network.go:875-901). The old build emitted none of these.
#[test]
fn result_carries_stats_clusters_and_top_lists() {
    let dir = fixture_repo("stats");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    let stats = &out["stats"];
    for key in [
        "total_nodes",
        "total_edges",
        "cluster_count",
        "avg_degree",
        "max_degree",
        "density",
        "isolated_nodes",
        "largest_cluster",
    ] {
        assert!(!stats[key].is_null(), "stats.{key} is missing");
    }
    assert_eq!(
        stats["total_nodes"],
        out["network"]["nodes"].as_object().unwrap().len()
    );
    assert_eq!(
        stats["total_edges"],
        out["network"]["edges"].as_array().unwrap().len()
    );
    // network.stats and the top-level stats are the same object for `all`.
    assert_eq!(stats, &out["network"]["stats"]);

    assert!(out["network"]["clusters"].is_array());
    // Go's BeadCluster: cluster_id, bead_ids, label, internal_edges,
    // external_edges, internal_connectivity, central_bead, shared_files,
    // total_commits (network.go:78).
    for cluster in out["network"]["clusters"].as_array().unwrap() {
        for key in [
            "cluster_id",
            "bead_ids",
            "label",
            "internal_edges",
            "external_edges",
            "internal_connectivity",
            "central_bead",
            "shared_files",
            "total_commits",
        ] {
            assert!(!cluster[key].is_null(), "cluster.{key} is missing");
        }
        assert!(cluster["bead_ids"].as_array().unwrap().len() >= 2);
    }

    // top_clusters is the first 5 of the full network's clusters.
    assert_eq!(
        out["top_clusters"].as_array().unwrap().len(),
        out["network"]["clusters"].as_array().unwrap().len().min(5)
    );
    // top_connected is the 10 highest-degree nodes, ties broken by bead_id.
    let top = out["top_connected"].as_array().unwrap();
    assert!(!top.is_empty() && top.len() <= 10);
    for w in top.windows(2) {
        let (d0, d1) = (
            w[0]["degree"].as_i64().unwrap(),
            w[1]["degree"].as_i64().unwrap(),
        );
        assert!(
            d0 > d1
                || (d0 == d1
                    && w[0]["bead_id"].as_str().unwrap() < w[1]["bead_id"].as_str().unwrap()),
            "top_connected is (degree desc, bead_id asc)"
        );
    }
}

/// `detectClusters` keeps only components of at least two beads joined by an
/// edge of `weight >= 2` (network.go:472, :526). NN-3's only edge is to NN-1
/// with weight 1, so the fixture's clusters must never contain a lone bead.
#[test]
fn clusters_only_group_beads_joined_by_a_weight_two_edge() {
    let dir = fixture_repo("clusters");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    for cluster in out["network"]["clusters"].as_array().unwrap() {
        let members: Vec<&str> = cluster["bead_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap())
            .collect();
        assert!(members.len() >= 2, "a single bead is not a cluster");
        let mut sorted = members.clone();
        sorted.sort_unstable();
        assert_eq!(members, sorted, "bead_ids are sorted");
        // Every member carries the cluster's id, and nothing else does.
        for (bead, node) in out["network"]["nodes"].as_object().unwrap() {
            let in_cluster = members.contains(&bead.as_str());
            let tagged = node["cluster_id"] == cluster["cluster_id"];
            assert_eq!(in_cluster, tagged, "{bead} cluster_id disagrees");
        }
        // A qualifying edge must exist inside the cluster.
        let strong = out["network"]["edges"].as_array().unwrap().iter().any(|e| {
            e["weight"].as_i64().unwrap() >= 2
                && members.contains(&e["from_bead"].as_str().unwrap())
                && members.contains(&e["to_bead"].as_str().unwrap())
        });
        assert!(strong, "no weight>=2 edge inside the cluster");
    }
}

/// Go's `GenerateReportCached` sees every correlated commit's files, so two
/// beads touching one file get a `shared_file` edge. The old build built its
/// file index from a report that carried no file data, so it emitted zero.
#[test]
fn shared_file_edges_exist_for_beads_touching_one_file() {
    let dir = fixture_repo("shared_file");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    let shared: Vec<&Value> = out["network"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["edge_type"] == "shared_file")
        .collect();
    assert!(
        !shared.is_empty(),
        "NN-1 and NN-2 both touch src/lib.rs; Go's BuildFileIndex links them"
    );
    for edge in &shared {
        let details: Vec<&str> = edge["details"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        assert!(
            details.contains(&"src/lib.rs"),
            "the shared path is the detail: {details:?}"
        );
    }
}

/// Go registers `--network-depth` with default 2 and clamps to 1..3
/// (main.go:1590, robot_registry.go:3386-3390). `depth` is `omitempty` on the
/// result, and `--robot-impact-network all` still carries it.
#[test]
fn depth_is_emitted_and_clamped_to_one_through_three() {
    let dir = fixture_repo("depth");
    for (args, want) in [
        (vec!["all"], 2),
        (vec!["all", "--network-depth", "1"], 1),
        (vec!["all", "--network-depth", "3"], 3),
        (vec!["all", "--network-depth", "0"], 1),
        (vec!["all", "--network-depth", "9"], 3),
    ] {
        let (code, stdout, stderr) = run(&dir, &args);
        assert_eq!(code, 0, "{args:?} stderr: {stderr}");
        let out: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(out["depth"], want, "{args:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The old build emitted `node_count` and `edge_count` at the top level.
/// Go has no equivalent — the counts live in `stats` — so both are gone.
#[test]
fn no_top_level_counts_that_go_does_not_emit() {
    let dir = fixture_repo("counts");
    let out = network(&dir, "all");
    let _ = std::fs::remove_dir_all(&dir);

    for key in ["node_count", "edge_count"] {
        assert!(out.get(key).is_none(), "`{key}` is not a Go field");
    }
    // The Go key set for `all`: the envelope plus the result payload.
    let mut keys: Vec<&str> = out
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "authority_hash",
            "data_hash",
            "depth",
            "generated_at",
            "network",
            "output_format",
            "scope_hash",
            "source_authority",
            "source_kind",
            "source_path",
            "stats",
            "top_clusters",
            "top_connected",
            "version",
        ]
    );
}

/// `bead_id` is `omitempty`: absent for `all`, present for a specific bead.
/// A named bead switches the payload to its sub-network and the stats follow
/// the sub-network, not the whole one.
#[test]
fn a_named_bead_yields_a_sub_network_and_omits_nothing_else() {
    let dir = fixture_repo("subnet");
    let all = network(&dir, "all");
    let sub = network(&dir, "NN-1");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        all.get("bead_id").is_none(),
        "bead_id is omitempty for `all`"
    );
    assert_eq!(sub["bead_id"], "NN-1");

    let sub_nodes = sub["network"]["nodes"].as_object().unwrap();
    assert!(
        sub_nodes.contains_key("NN-1"),
        "the target is always included"
    );
    for (bead, node) in sub_nodes {
        assert!(all["network"]["nodes"]
            .as_object()
            .unwrap()
            .contains_key(bead));
        // Node copies keep their identity, degree included.
        assert_eq!(node, &all["network"]["nodes"][bead], "{bead}");
    }
    // Every surviving edge has both endpoints inside the sub-network.
    for edge in sub["network"]["edges"].as_array().unwrap() {
        assert!(sub_nodes.contains_key(edge["from_bead"].as_str().unwrap()));
        assert!(sub_nodes.contains_key(edge["to_bead"].as_str().unwrap()));
    }
    // GetSubNetwork starts from an empty Clusters slice, so the sub-network
    // reports no clusters even though the full one has them.
    assert_eq!(
        sub["network"]["clusters"].as_array().unwrap().len(),
        0,
        "GetSubNetwork does not carry clusters across"
    );
    assert_eq!(sub["stats"]["total_nodes"], sub_nodes.len());
    assert_eq!(sub["stats"]["cluster_count"], 0);
    // top_clusters always comes from the FULL network, for context.
    assert_eq!(sub["top_clusters"], all["top_clusters"]);
}

/// A bead with no correlated commits is not a node, so naming it is the
/// "not found" path: Go prints the message to stderr and returns exit 1
/// (robot_registry.go:3373-3378).
#[test]
fn an_unknown_bead_reports_not_found_at_exit_one() {
    let dir = fixture_repo("missing");
    let (code, stdout, stderr) = run(&dir, &["NOPE-1"]);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(code, 1);
    assert_eq!(stderr, "Bead not found in network: NOPE-1\n");
    assert!(stdout.is_empty(), "no payload is emitted: {stdout}");
}

/// Outside a git repository Go refuses before reading anything
/// (`correlation.ValidateRepository`, robot_registry.go:3335).
#[test]
fn a_non_repository_is_refused() {
    let dir = std::env::temp_dir().join(format!("bvr_impact_network_nogit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".beads")).unwrap();
    std::fs::write(dir.join(".beads").join("issues.jsonl"), "").unwrap();

    let (code, stdout, stderr) = run(&dir, &["all"]);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(code, 1);
    assert!(stderr.contains("not a git repository"), "{stderr}");
    assert!(stdout.is_empty(), "no payload is emitted: {stdout}");
}
