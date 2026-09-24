//! Dependency-readiness index — port of Go `pkg/model/readiness.go`
//! (`NewReadinessIndex` + `compute` + `DependencyState`).
//!
//! `causalSnapshotState` is the only caller today: it evaluates one committed
//! snapshot of the beads file and needs to know whether the target's
//! dependencies *prove* readiness or merely fail to disprove it. Unknown data
//! withholds readiness exactly like unsatisfied data, so the three states stay
//! distinct rather than collapsing into a bool.
//!
//! Go builds the index from `[]model.Issue`, but the fields `compute` reads are
//! only `ID`, `Status` and `Dependencies`; `IssueType`, `Assignee` and
//! `DeferUntil` are consulted by `Ready`/`ReadyNow`, which causal analysis never
//! calls. This port therefore takes exactly those fields.

use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

/// Go `model.DependencyState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyState {
    /// Proven: every dependency resolves to a closed issue.
    Satisfied,
    /// Proven negative: at least one dependency resolves to a live issue.
    Unsatisfied,
    /// Incomplete data: a missing edge, or a parent cycle.
    Unknown,
}

impl DependencyState {
    /// Go `combineDependencyState` — unknown dominates, then unsatisfied.
    fn combine(self, other: Self) -> Self {
        if self == Self::Unknown || other == Self::Unknown {
            return Self::Unknown;
        }
        if self == Self::Unsatisfied || other == Self::Unsatisfied {
            return Self::Unsatisfied;
        }
        Self::Satisfied
    }
}

/// Go `model.DependencyType.IsValid` — any nonblank type is retained, so
/// informational and custom relationships survive a round trip.
pub fn dep_type_is_valid(dep_type: &str) -> bool {
    !dep_type.trim().is_empty()
}

/// Go `model.DependencyType.IsBlocking`. Empty means blocking: legacy beads data
/// predates typed edges, so an untyped dependency blocks by default.
pub fn dep_type_is_blocking(dep_type: &str) -> bool {
    matches!(dep_type, "" | "blocks" | "conditional-blocks" | "waits-for")
}

/// Go `model.DepParentChild`.
pub const DEP_PARENT_CHILD: &str = "parent-child";

/// Go `closedForReadiness` — `closed` and `tombstone` both release dependants.
/// A tombstone is a permanent removal, not outstanding work.
pub fn closed_for_readiness(status: &str) -> bool {
    status == "closed" || status == "tombstone"
}

/// The decision inputs `compute` reads, keyed by issue id (Go `readinessIssue`
/// plus the map key it is stored under). `dependencies` keeps the committed
/// record's own edge order; the result is order-independent, but reading in
/// source order keeps the port close to the original.
#[derive(Debug, Clone)]
pub struct ReadinessIssue {
    pub id: String,
    pub status: String,
    /// `(depends_on_id, type)` pairs.
    pub dependencies: Vec<(String, String)>,
}

/// Go `ReadinessIndex`. Construction is O(issues + edges); `dependency_state`
/// is O(1).
#[derive(Debug, Default)]
pub struct ReadinessIndex {
    issues: BTreeMap<String, ReadinessIssue>,
    states: BTreeMap<String, DependencyState>,
}

impl ReadinessIndex {
    /// Go `NewReadinessIndex` + `compute`.
    ///
    /// Go iterates its issue maps in randomized order. `compute` is
    /// order-independent — the first pass only folds a node's own edges into
    /// that node's own state, and the parent walk combines commutatively — so
    /// this iterates by sorted id and lands on Go's exact result.
    pub fn new(issues: &[ReadinessIssue]) -> Self {
        let mut r = Self::default();
        for issue in issues {
            r.issues.insert(issue.id.clone(), issue.clone());
        }
        r.compute();
        r
    }

    fn compute(&mut self) {
        // Split the two maps so the walk can read issues while writing states.
        let Self { issues, states } = self;
        let mut pending: BTreeMap<&str, i64> = BTreeMap::new();
        // parent id -> children still waiting on it (Go `r.children`).
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

        for (id, issue) in issues.iter() {
            states.insert(id.clone(), DependencyState::Satisfied);
            pending.insert(id.as_str(), 0);
            for (depends_on_id, dep_type) in &issue.dependencies {
                if dep_type == DEP_PARENT_CHILD {
                    children
                        .entry(depends_on_id.as_str())
                        .or_default()
                        .push(id.as_str());
                }
                // A closed issue's edges cannot gate anything, so they never
                // contribute state.
                if closed_for_readiness(&issue.status) {
                    continue;
                }
                let other = issues.get(depends_on_id);
                let current = || states.get(id).copied().unwrap_or(DependencyState::Unknown);
                if dep_type_is_blocking(dep_type) {
                    match other {
                        None => {
                            states.insert(id.clone(), DependencyState::Unknown);
                        }
                        Some(o) if !closed_for_readiness(&o.status) => {
                            states.insert(
                                id.clone(),
                                current().combine(DependencyState::Unsatisfied),
                            );
                        }
                        Some(_) => {}
                    }
                } else if dep_type == DEP_PARENT_CHILD {
                    match other {
                        None => {
                            states.insert(id.clone(), DependencyState::Unknown);
                        }
                        Some(o) if !closed_for_readiness(&o.status) => {
                            *pending.entry(id.as_str()).or_default() += 1;
                        }
                        Some(_) => {}
                    }
                }
            }
        }

        // Parents before children. There is no depth cutoff: a blocked ancestor
        // still gates a descendant any number of hops away. A parent cycle (and
        // everything behind it) stays unresolved.
        let mut queue: VecDeque<&str> = VecDeque::new();
        for (id, issue) in issues.iter() {
            if !closed_for_readiness(&issue.status) && pending.get(id.as_str()) == Some(&0) {
                queue.push_back(id.as_str());
            }
        }
        while let Some(id) = queue.pop_front() {
            let Some(kids) = children.get(id) else {
                continue;
            };
            let parent_state = states.get(id).copied().unwrap_or(DependencyState::Unknown);
            for child in kids.clone() {
                let Some(child_issue) = issues.get(child) else {
                    continue;
                };
                if closed_for_readiness(&child_issue.status) {
                    continue;
                }
                let current = states
                    .get(child)
                    .copied()
                    .unwrap_or(DependencyState::Unknown);
                states.insert(child.to_string(), current.combine(parent_state));
                if let Some(count) = pending.get_mut(child) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }

        for (id, remaining) in &pending {
            if *remaining > 0 {
                states.insert((*id).to_string(), DependencyState::Unknown);
            }
        }
    }

    /// Go `ReadinessIndex.DependencyState` — unknown for any id the index never
    /// saw, which is how a target missing from the snapshot reads.
    pub fn dependency_state(&self, id: &str) -> DependencyState {
        self.states
            .get(id)
            .copied()
            .unwrap_or(DependencyState::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &str, status: &str, deps: &[(&str, &str)]) -> ReadinessIssue {
        ReadinessIssue {
            id: id.into(),
            status: status.into(),
            dependencies: deps
                .iter()
                .map(|(d, t)| ((*d).to_string(), (*t).to_string()))
                .collect(),
        }
    }

    #[test]
    fn no_dependencies_is_satisfied() {
        let r = ReadinessIndex::new(&[issue("A", "open", &[])]);
        assert_eq!(r.dependency_state("A"), DependencyState::Satisfied);
    }

    #[test]
    fn open_blocker_is_unsatisfied_and_closed_blocker_is_satisfied() {
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", "blocks")]),
            issue("B", "open", &[]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Unsatisfied);
        // A's state does not leak onto its blocker.
        assert_eq!(r.dependency_state("B"), DependencyState::Satisfied);

        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", "blocks")]),
            issue("B", "closed", &[]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Satisfied);
    }

    #[test]
    fn missing_blocker_is_unknown() {
        let r = ReadinessIndex::new(&[issue("A", "open", &[("GONE", "blocks")])]);
        assert_eq!(r.dependency_state("A"), DependencyState::Unknown);
    }

    #[test]
    fn untyped_edge_blocks() {
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", "")]),
            issue("B", "in_progress", &[]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Unsatisfied);
    }

    #[test]
    fn non_blocking_edge_does_not_gate() {
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", "related")]),
            issue("B", "open", &[]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Satisfied);
    }

    #[test]
    fn parent_child_chain_of_ready_ancestors_stays_satisfied() {
        // Every ancestor is itself ready, so the whole chain is ready. A
        // `parent-child` edge defers to the parent, it does not block on it.
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", DEP_PARENT_CHILD)]),
            issue("B", "open", &[("C", DEP_PARENT_CHILD)]),
            issue("C", "open", &[("D", DEP_PARENT_CHILD)]),
            issue("D", "open", &[]),
        ]);
        for id in ["A", "B", "C", "D"] {
            assert_eq!(r.dependency_state(id), DependencyState::Satisfied, "{id}");
        }
    }

    #[test]
    fn parent_child_chain_propagates_from_any_depth() {
        // D is blocked by a live issue, and that block reaches three
        // parent-child hops up. There is no depth cutoff: an ancestor blocked
        // fifty hops away still gates the descendant.
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", DEP_PARENT_CHILD)]),
            issue("B", "open", &[("C", DEP_PARENT_CHILD)]),
            issue("C", "open", &[("D", DEP_PARENT_CHILD)]),
            issue("D", "open", &[("E", "blocks")]),
            issue("E", "open", &[]),
        ]);
        assert_eq!(r.dependency_state("E"), DependencyState::Satisfied);
        assert_eq!(r.dependency_state("D"), DependencyState::Unsatisfied);
        assert_eq!(r.dependency_state("C"), DependencyState::Unsatisfied);
        assert_eq!(r.dependency_state("B"), DependencyState::Unsatisfied);
        assert_eq!(r.dependency_state("A"), DependencyState::Unsatisfied);
    }

    #[test]
    fn closed_issue_does_not_wait_on_its_own_parents() {
        let r = ReadinessIndex::new(&[
            issue("A", "closed", &[("B", DEP_PARENT_CHILD)]),
            issue("B", "open", &[]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Satisfied);
    }

    #[test]
    fn parent_cycle_is_unknown() {
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", DEP_PARENT_CHILD)]),
            issue("B", "open", &[("A", DEP_PARENT_CHILD)]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Unknown);
        assert_eq!(r.dependency_state("B"), DependencyState::Unknown);
    }

    #[test]
    fn unknown_dominates_unsatisfied() {
        // A waits on a live blocker (unsatisfied) and a missing one (unknown).
        let r = ReadinessIndex::new(&[
            issue("A", "open", &[("B", "blocks"), ("GONE", "blocks")]),
            issue("B", "open", &[]),
        ]);
        assert_eq!(r.dependency_state("A"), DependencyState::Unknown);
    }

    #[test]
    fn absent_id_reads_unknown() {
        let r = ReadinessIndex::new(&[issue("A", "open", &[])]);
        assert_eq!(r.dependency_state("NOPE"), DependencyState::Unknown);
    }

    #[test]
    fn dep_type_validity_matches_go() {
        assert!(dep_type_is_valid("blocks"));
        assert!(dep_type_is_valid(" custom "));
        assert!(!dep_type_is_valid("   "));
        assert!(!dep_type_is_valid(""));
    }
}
