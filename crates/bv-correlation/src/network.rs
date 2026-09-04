//! Bead impact/relation network — port of Go `pkg/correlation/network.go`
//! (904 lines), backing `robot-related` and `robot-impact-network`.
//!
//! Documented scope cut (see plan doc §11): builds the same three edge
//! types as Go (shared-commit, shared-file, dependency) from the existing
//! `correlator::correlate` report + `Issue.dependencies`, and supports
//! depth-limited sub-network extraction (`sub_network`) matching Go's
//! `GetSubNetwork`. Not ported: cluster detection (`detectClusters` —
//! connected-components grouping with generated human-readable labels)
//! and network-wide stats beyond edge/node counts. A caller wanting
//! "what beads are related to X" gets a real answer; a caller wanting
//! named clusters across the whole graph does not yet.

use crate::correlator::CorrelatedCommit;
use bv_core::model::Issue;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    SharedCommit,
    SharedFile,
    Dependency,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct NetworkEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub weight: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct NetworkNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub degree: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ImpactNetwork {
    pub nodes: BTreeMap<String, NetworkNode>,
    pub edges: Vec<NetworkEdge>,
}

/// Build the full network: shared-commit + shared-file edges from the
/// correlation report, plus blocking/parent-child dependency edges from
/// the issue set itself.
pub fn build_network(
    issues: &[Issue],
    report: &BTreeMap<String, Vec<CorrelatedCommit>>,
) -> ImpactNetwork {
    let mut nodes: BTreeMap<String, NetworkNode> = issues
        .iter()
        .map(|i| {
            (
                i.id.clone(),
                NetworkNode {
                    id: i.id.clone(),
                    title: i.title.clone(),
                    status: i.status.as_str().to_string(),
                    degree: 0,
                },
            )
        })
        .collect();

    let mut edge_map: BTreeMap<(String, String, EdgeType), NetworkEdge> = BTreeMap::new();
    let bump = |a: &str,
                b: &str,
                ty: EdgeType,
                shared_item: &str,
                edge_map: &mut BTreeMap<(String, String, EdgeType), NetworkEdge>| {
        if a == b {
            return;
        }
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let key = (lo.to_string(), hi.to_string(), ty);
        let entry = edge_map.entry(key).or_insert_with(|| NetworkEdge {
            from: lo.to_string(),
            to: hi.to_string(),
            edge_type: ty,
            weight: 0,
            shared: Vec::new(),
        });
        entry.weight += 1;
        if !shared_item.is_empty() && !entry.shared.contains(&shared_item.to_string()) {
            entry.shared.push(shared_item.to_string());
        }
    };

    // Shared-commit edges: two beads correlated to the same commit sha.
    let mut by_sha: HashMap<&str, Vec<&str>> = HashMap::new();
    for (bead_id, commits) in report {
        for c in commits {
            by_sha
                .entry(c.sha.as_str())
                .or_default()
                .push(bead_id.as_str());
        }
    }
    for (sha, beads) in &by_sha {
        for i in 0..beads.len() {
            for j in (i + 1)..beads.len() {
                bump(
                    beads[i],
                    beads[j],
                    EdgeType::SharedCommit,
                    sha,
                    &mut edge_map,
                );
            }
        }
    }

    // Shared-file edges: two beads whose correlated commits touch an
    // overlapping file (across any commit, not just the same one).
    let mut files_by_bead: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (bead_id, commits) in report {
        let set = files_by_bead.entry(bead_id.as_str()).or_default();
        for c in commits {
            for f in &c.files {
                set.insert(f.as_str());
            }
        }
    }
    let bead_ids: Vec<&str> = files_by_bead.keys().copied().collect();
    for i in 0..bead_ids.len() {
        for j in (i + 1)..bead_ids.len() {
            let a = &files_by_bead[bead_ids[i]];
            let b = &files_by_bead[bead_ids[j]];
            for f in a.intersection(b) {
                bump(
                    bead_ids[i],
                    bead_ids[j],
                    EdgeType::SharedFile,
                    f,
                    &mut edge_map,
                );
            }
        }
    }

    // Dependency edges: direct `Issue.dependencies` links (any type).
    for issue in issues {
        for dep in &issue.dependencies {
            let target = dep.effective_depends_on();
            if !target.is_empty() && nodes.contains_key(target) {
                bump(&issue.id, target, EdgeType::Dependency, "", &mut edge_map);
            }
        }
    }

    let edges: Vec<NetworkEdge> = edge_map.into_values().collect();
    for e in &edges {
        if let Some(n) = nodes.get_mut(&e.from) {
            n.degree += 1;
        }
        if let Some(n) = nodes.get_mut(&e.to) {
            n.degree += 1;
        }
    }

    ImpactNetwork { nodes, edges }
}

/// BFS out to `depth` hops from `bead_id` (Go's `GetSubNetwork`). Depth is
/// clamped to `[1, 3]` to match Go's `handleRobotImpactNetwork` behavior.
pub fn sub_network(network: &ImpactNetwork, bead_id: &str, depth: usize) -> ImpactNetwork {
    let depth = depth.clamp(1, 3);
    if !network.nodes.contains_key(bead_id) {
        return ImpactNetwork {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        };
    }
    let mut adjacency: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, e) in network.edges.iter().enumerate() {
        adjacency.entry(e.from.as_str()).or_default().push(idx);
        adjacency.entry(e.to.as_str()).or_default().push(idx);
    }

    let mut included: BTreeSet<String> = BTreeSet::new();
    included.insert(bead_id.to_string());
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();
    frontier.push_back((bead_id.to_string(), 0));
    let mut edge_indices: BTreeSet<usize> = BTreeSet::new();

    while let Some((current, d)) = frontier.pop_front() {
        if d >= depth {
            continue;
        }
        let Some(edge_idxs) = adjacency.get(current.as_str()) else {
            continue;
        };
        for &idx in edge_idxs {
            edge_indices.insert(idx);
            let e = &network.edges[idx];
            let other = if e.from == current { &e.to } else { &e.from };
            if included.insert(other.clone()) {
                frontier.push_back((other.clone(), d + 1));
            }
        }
    }

    let nodes: BTreeMap<String, NetworkNode> = included
        .iter()
        .filter_map(|id| network.nodes.get(id).map(|n| (id.clone(), n.clone())))
        .collect();
    let edges: Vec<NetworkEdge> = edge_indices
        .into_iter()
        .map(|i| network.edges[i].clone())
        .collect();
    ImpactNetwork { nodes, edges }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bv_core::model::{Dependency, DependencyType, Status};

    fn issue(id: &str) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: id.to_string(),
            description: String::new(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status: Status::Open,
            priority: 2,
            issue_type: "task".into(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: None,
            updated_at: None,
            due_date: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: vec![],
            dependencies: vec![],
            comments: vec![],
            source_repo: String::new(),
        }
    }

    fn commit(sha: &str, bead: &str, files: &[&str]) -> CorrelatedCommit {
        CorrelatedCommit {
            sha: sha.to_string(),
            bead_id: bead.to_string(),
            confidence: 0.9,
            methods: vec!["explicit_id"],
            reason: String::new(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            author: "alice".to_string(),
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn shared_commit_creates_an_edge() {
        let issues = vec![issue("A"), issue("B")];
        let mut report: BTreeMap<String, Vec<CorrelatedCommit>> = BTreeMap::new();
        report.insert("A".into(), vec![commit("sha1", "A", &["x.rs"])]);
        report.insert("B".into(), vec![commit("sha1", "B", &["x.rs"])]);
        let net = build_network(&issues, &report);
        assert!(net
            .edges
            .iter()
            .any(|e| e.edge_type == EdgeType::SharedCommit));
    }

    #[test]
    fn shared_file_across_different_commits_creates_an_edge() {
        let issues = vec![issue("A"), issue("B")];
        let mut report: BTreeMap<String, Vec<CorrelatedCommit>> = BTreeMap::new();
        report.insert("A".into(), vec![commit("sha1", "A", &["shared.rs"])]);
        report.insert("B".into(), vec![commit("sha2", "B", &["shared.rs"])]);
        let net = build_network(&issues, &report);
        assert!(net
            .edges
            .iter()
            .any(|e| e.edge_type == EdgeType::SharedFile));
    }

    #[test]
    fn dependency_edge_from_issue_deps() {
        let mut a = issue("A");
        a.dependencies.push(Dependency {
            issue_id: "A".into(),
            depends_on_id: "B".into(),
            depends_on_legacy: String::new(),
            target_id_legacy: String::new(),
            r#type: DependencyType::Blocks,
            created_at: None,
            created_by: String::new(),
        });
        let issues = vec![a, issue("B")];
        let net = build_network(&issues, &BTreeMap::new());
        assert!(net
            .edges
            .iter()
            .any(|e| e.edge_type == EdgeType::Dependency));
    }

    #[test]
    fn sub_network_respects_depth() {
        let issues = vec![issue("A"), issue("B"), issue("C")];
        let mut report: BTreeMap<String, Vec<CorrelatedCommit>> = BTreeMap::new();
        report.insert("A".into(), vec![commit("sha1", "A", &["x.rs"])]);
        report.insert("B".into(), vec![commit("sha1", "B", &["x.rs"])]);
        report.insert("C".into(), vec![commit("sha2", "C", &["y.rs"])]);
        let net = build_network(&issues, &report);
        let sub = sub_network(&net, "A", 1);
        assert!(sub.nodes.contains_key("A"));
        assert!(sub.nodes.contains_key("B"));
        // C is 2 hops from A via shared-commit(A,B) then would need B-C edge,
        // which doesn't exist here, so this just confirms depth-1 doesn't
        // pull in unrelated nodes.
        assert!(!sub.nodes.contains_key("C"));
    }

    #[test]
    fn sub_network_unknown_bead_is_empty() {
        let net = ImpactNetwork {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        };
        let sub = sub_network(&net, "nope", 1);
        assert!(sub.nodes.is_empty());
    }
}
