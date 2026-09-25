//! Dependency readiness for the recipe filters — the slice of Go
//! `pkg/model/readiness.go` that `pkg/recipe` actually calls.
//!
//! Go hands `recipe.Metrics` a `*model.ReadinessIndex` built by the caller from
//! the *full* source, so a blocker outside the display scope still counts. The
//! graph walk itself already lives in
//! [`bv_correlation::readiness::ReadinessIndex`] (a port of Go's `compute`), and
//! this type reuses it verbatim rather than forking the algorithm. What is added
//! is only what `ReadinessIndex.Ready` reads on top of the walk: each issue's
//! own status and deferral instant. `bv-correlation` omits both because causal
//! analysis never calls `Ready`.

use std::collections::BTreeMap;

use bv_core::model::{Issue, Status};
use bv_correlation::readiness::{
    closed_for_readiness, DependencyState, ReadinessIndex, ReadinessIssue,
};

/// Go `model.ReadinessIndex`, restricted to `DependencyState` and `Ready`.
///
/// Build it from the full issue set, then filter the issues you pass to
/// [`crate::apply`]: the index is the authority, the input slice is the view.
#[derive(Debug)]
pub struct Readiness {
    states: ReadinessIndex,
    /// The `Ready` inputs `compute` does not read, keyed by issue id.
    own: BTreeMap<String, (Status, Option<jiff::Timestamp>)>,
}

impl Readiness {
    /// Go `model.NewReadinessIndex` — indexes every dependency edge in
    /// `issues`, which is O(issues + edges); lookups are O(1).
    pub fn new(issues: &[Issue]) -> Self {
        let mut states = Vec::with_capacity(issues.len());
        let mut own = BTreeMap::new();
        for issue in issues {
            states.push(ReadinessIssue {
                id: issue.id.clone(),
                status: issue.status.as_str().to_string(),
                dependencies: issue
                    .dependencies
                    .iter()
                    .map(|d| {
                        (
                            d.effective_depends_on().to_string(),
                            d.r#type.as_str().to_string(),
                        )
                    })
                    .collect(),
            });
            // A deferral instant that does not parse is treated as absent, the
            // same as an unset column; Go cannot reach that state because it
            // unmarshals `DeferUntil` into a typed `*time.Time`.
            let defer_until = issue
                .defer_until
                .as_deref()
                .and_then(|s| s.parse::<jiff::Timestamp>().ok());
            own.insert(issue.id.clone(), (issue.status, defer_until));
        }
        Self {
            states: ReadinessIndex::new(&states),
            own,
        }
    }

    /// Go `ReadinessIndex.DependencyState` — unknown for any id the index never
    /// saw, which is how a target outside the authority reads.
    pub fn dependency_state(&self, id: &str) -> DependencyState {
        self.states.dependency_state(id)
    }

    /// Go `readinessIssue.isDeferredAt` — the scheduler defers work until this
    /// instant passes.
    fn is_deferred_at(&self, id: &str, now: jiff::Timestamp) -> bool {
        self.own
            .get(id)
            .and_then(|(_, defer_until)| *defer_until)
            .is_some_and(|until| until > now)
    }

    /// Go `ReadinessIndex.Ready` — includes ongoing work for planning. An issue
    /// is ready when it is open or in progress, not deferred past `now`, and
    /// its dependencies are proven satisfied. Both unsatisfied and unknown
    /// dependencies withhold readiness.
    pub fn ready(&self, id: &str, now: jiff::Timestamp) -> bool {
        let Some(&(status, _)) = self.own.get(id) else {
            return false;
        };
        if !matches!(status, Status::Open | Status::InProgress) {
            return false;
        }
        if self.is_deferred_at(id, now) {
            return false;
        }
        self.dependency_state(id) == DependencyState::Satisfied
    }

    /// Go `closedForReadiness`, re-exported so callers can reason about the
    /// same closed/tombstone rule the walk uses.
    pub fn closed_for_readiness(status: Status) -> bool {
        closed_for_readiness(status.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{blocking, issue, ts, with_status};
    use bv_core::model::DependencyType;

    #[test]
    fn ready_needs_open_status_no_deferral_and_satisfied_deps() {
        let now = ts("2026-09-01T12:00:00Z");
        let mut deferred = issue("later");
        deferred.defer_until = Some("2026-10-01T00:00:00Z".into());
        let mut elapsed = issue("now-ok");
        elapsed.defer_until = Some("2026-08-01T00:00:00Z".into());
        let issues = vec![
            issue("root"),
            blocking("blocked", "root"),
            with_status("parked", Status::Blocked),
            with_status("done", Status::Closed),
            with_status("tomb", Status::Tombstone),
            deferred,
            elapsed,
        ];

        let r = Readiness::new(&issues);
        assert!(r.ready("root", now));
        assert!(!r.ready("blocked", now));
        assert!(!r.ready("parked", now));
        assert!(!r.ready("done", now));
        assert!(!r.ready("tomb", now));
        assert!(!r.ready("later", now));
        assert!(r.ready("now-ok", now));
        // An id the index never saw is unknown, so never ready.
        assert!(!r.ready("absent", now));
        assert_eq!(r.dependency_state("blocked"), DependencyState::Unsatisfied);
        assert_eq!(r.dependency_state("absent"), DependencyState::Unknown);
    }

    #[test]
    fn a_closed_blocker_satisfies_and_a_missing_one_is_unknown() {
        let now = ts("2026-09-01T12:00:00Z");
        let issues = vec![
            blocking("a", "done"),
            blocking("b", "missing"),
            with_status("done", Status::Closed),
        ];
        assert_eq!(issues[0].dependencies[0].r#type, DependencyType::Blocks);
        let r = Readiness::new(&issues);
        assert!(r.ready("a", now));
        assert!(!r.ready("b", now));
    }

    #[test]
    fn an_unparseable_defer_until_reads_as_unset() {
        let now = ts("2026-09-01T12:00:00Z");
        let mut broken = issue("broken");
        broken.defer_until = Some("not a timestamp".into());
        let r = Readiness::new(&[broken]);
        assert!(r.ready("broken", now));
    }
}
