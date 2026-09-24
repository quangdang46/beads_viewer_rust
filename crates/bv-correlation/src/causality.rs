//! Temporal causality analysis for one bead — port of Go
//! `pkg/correlation/causality.go` (`BuildCausalityChainAt`,
//! `buildRecordedCausality` and the insight helpers), backing
//! `--robot-causality`.
//!
//! Two chain shapes exist. When the correlator retained committed dependency
//! snapshots for the requested target (`CausalHistory`), the chain is built from
//! those observations: every recorded transition, the constraint edges between
//! them, the blocked intervals they imply, and the longest evidenced path. When
//! no snapshots were retained, the chain is a lifecycle *chronology* only and
//! says so — blocking durations are reported as unknown, never as zero.
//!
//! Durations are integer nanoseconds throughout, because that is what Go's
//! `time.Duration` marshals to. Where Go's custom `MarshalJSON` replaces a
//! duration with a nullable pointer, the field here is an `Option<i64>` that
//! serializes as a number or `null`. Unknown measurement is never a
//! fabricated zero.

use crate::extractor::{BeadEvent, EventType, HistoricalIssueState};
use crate::history::{BeadHistory, HistoryCommit};
use crate::readiness::DependencyState;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Time and duration primitives
// ---------------------------------------------------------------------------

/// Go `time.Time` as this port carries it: the exact text for output, plus the
/// parsed instant for comparison and arithmetic.
///
/// Git's `%aI`/`%cI` are already strict ISO 8601 at second precision with an
/// explicit offset, so Go's `time.Parse` -> `RFC3339Nano` round trip is the
/// identity string. Keeping the original text therefore reproduces Go's output
/// byte for byte, which re-formatting a UTC-normalized `jiff::Timestamp` would
/// not: Go preserves the commit's own offset rather than converting to UTC.
///
/// Ordering deliberately compares `ts` and not `raw`. Go's `time.Time`
/// operators compare instants, and a commit's author offset varies per
/// repository: `2026-01-02T10:00:00+09:00` is 01:00Z and therefore *earlier*
/// than `2026-01-02T02:00:00Z`, even though its text sorts later. Deriving
/// `Ord` here would compare the text and silently invert every ordering and
/// equality check in this module for such a repository.
#[derive(Debug, Clone)]
pub struct GoTime {
    raw: String,
    ts: jiff::Timestamp,
}

impl PartialEq for GoTime {
    fn eq(&self, other: &Self) -> bool {
        self.ts == other.ts
    }
}

impl Eq for GoTime {}

impl PartialOrd for GoTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GoTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.ts.cmp(&other.ts)
    }
}

impl GoTime {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let ts = raw
            .parse::<jiff::Timestamp>()
            .map_err(|e| format!("parsing timestamp {raw:?}: {e}"))?;
        Ok(Self {
            raw: raw.to_string(),
            ts,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Whole nanoseconds since the Unix epoch. Exact via seconds + subsecond
    /// parts, so a multi-month difference keeps nanosecond resolution (an
    /// `f64` nanosecond total would start losing bits past ~104 days).
    fn epoch_nanos(&self) -> i128 {
        i128::from(self.ts.as_second()) * 1_000_000_000 + i128::from(self.ts.subsec_nanosecond())
    }

    /// Go `t2.Sub(t1)`, in nanoseconds.
    fn sub_nanos(&self, other: &Self) -> i64 {
        let delta = self.epoch_nanos() - other.epoch_nanos();
        delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }

    /// Go `a.Before(b)`.
    fn before(&self, other: &Self) -> bool {
        self.ts < other.ts
    }
}

impl Serialize for GoTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

/// Serialize a `GoTime` field, skipping when absent (Go's `omitempty`).
fn serialize_opt_time<S: Serializer>(
    value: &Option<GoTime>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(t) => serializer.serialize_str(&t.raw),
        None => serializer.serialize_none(),
    }
}

/// Nanoseconds in a second / hour / day, for the human-readable formatters.
const HOUR_NANOS: i64 = 3_600_000_000_000;
const DAY_NANOS: i64 = 24 * HOUR_NANOS;

// ---------------------------------------------------------------------------
// Retained committed state (Go `CausalHistory`)
// ---------------------------------------------------------------------------

/// Go `CausalState` — the target's constraint set as of one committed change.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CausalState {
    pub issue: Option<HistoricalIssueState>,
    /// False when any record in the snapshot was malformed, duplicated, or
    /// carried an invalid status or dependency type.
    pub known: bool,
    pub dependency_state: DependencyState,
    pub blockers: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// Go `CausalObservation` — the target's state across one commit that changed
/// it or one of the records that gate it.
#[derive(Debug, Clone, Serialize)]
pub struct CausalObservation {
    pub commit_sha: String,
    pub timestamp: GoTime,
    pub committed_at: GoTime,
    pub before: CausalState,
    pub after: CausalState,
    /// Target and relevant blocking/parent records only — not every issue in
    /// the commit.
    pub changes: Vec<BeadEvent>,
}

/// Go `CausalHistory` — retained only for an explicitly requested target, and
/// only in Git first-parent order. Order is never author-date sorted: the
/// commit graph is the authority for what was observed when.
#[derive(Debug, Clone, Serialize)]
pub struct CausalHistory {
    pub bead_id: String,
    pub observations: Vec<CausalObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<GoTime>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_time: Option<GoTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_committed_at: Option<GoTime>,
}

// ---------------------------------------------------------------------------
// Chain shapes
// ---------------------------------------------------------------------------

/// Go `CausalEventType`. `Observation` is the synthetic open-wait marker Go
/// adds with the literal type `"observation"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalEventType {
    Created,
    Claimed,
    Commit,
    Blocked,
    Unblocked,
    Closed,
    Reopened,
    Changed,
    Deleted,
    ConstraintChange,
    Observation,
}

impl CausalEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Claimed => "claimed",
            Self::Commit => "commit",
            Self::Blocked => "blocked",
            Self::Unblocked => "unblocked",
            Self::Closed => "closed",
            Self::Reopened => "reopened",
            Self::Changed => "changed",
            Self::Deleted => "deleted",
            Self::ConstraintChange => "constraint_change",
            Self::Observation => "observation",
        }
    }

    /// Go `causalEventOrder` — the tie-break applied to events that share an
    /// author timestamp in the chronology-only path.
    fn order(self) -> i32 {
        match self {
            Self::Created => 0,
            Self::Claimed => 1,
            Self::Blocked => 2,
            Self::Commit => 3,
            Self::Unblocked => 4,
            Self::Closed => 5,
            Self::Reopened => 6,
            _ => 7,
        }
    }

    /// Go's per-event `Type` switch in `buildRecordedCausality`.
    fn of_event_type(event: EventType) -> Option<Self> {
        match event {
            EventType::Created => Some(Self::Created),
            EventType::Claimed => Some(Self::Claimed),
            EventType::Closed => Some(Self::Closed),
            EventType::Reopened => Some(Self::Reopened),
            EventType::Deleted => Some(Self::Deleted),
            // Go's chronology path skips modified events outright.
            EventType::Modified => None,
        }
    }
}

/// Go `CausalEvent` — one entry in the chain. Field order below is Go's
/// emitted order, which the compatibility contract pins.
#[derive(Debug, Clone, Serialize)]
pub struct CausalEvent {
    pub id: usize,
    #[serde(rename = "type")]
    pub event_type: CausalEventType,
    pub timestamp: GoTime,
    pub description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub commit_sha: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub blocker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caused_by_id: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub enables_ids: Vec<usize>,
    /// Seconds until the next event, in nanoseconds (Go `time.Duration`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_next: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_bead_id: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_opt_time"
    )]
    pub committed_at: Option<GoTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<HistoricalIssueState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<HistoricalIssueState>,
    /// Whether the per-commit record transition parsed cleanly. Distinct from
    /// full-source authority, which lives on the observation's state.
    pub transition_observed: bool,
}

impl CausalEvent {
    fn new(id: usize, event_type: CausalEventType, timestamp: GoTime, description: String) -> Self {
        Self {
            id,
            event_type,
            timestamp,
            description,
            commit_sha: String::new(),
            blocker_id: String::new(),
            caused_by_id: None,
            enables_ids: Vec::new(),
            duration_next: None,
            source_bead_id: String::new(),
            committed_at: None,
            before: None,
            after: None,
            transition_observed: false,
        }
    }
}

impl Serialize for CausalEventType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Go `CausalLink` — an evidenced relation between two observed events.
/// Chronological proximity and commit correlation alone never create one.
#[derive(Debug, Clone, Serialize)]
pub struct CausalLink {
    pub from: usize,
    pub to: usize,
    pub kind: String,
    pub evidence: String,
    /// Observed wait in nanoseconds — never an estimate of task execution time
    /// or a counterfactual schedule.
    pub duration: i64,
}

/// Go `BlockedPeriod` — a contiguous observed interval in which a constraint
/// withheld readiness.
#[derive(Debug, Clone, Serialize)]
pub struct BlockedPeriod {
    pub start_time: GoTime,
    pub end_time: GoTime,
    /// Nanoseconds.
    pub duration: i64,
    /// Set only when exactly one blocker applied.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub blocker_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocker_ids: Vec<String>,
    /// `explicit_status`, `dependency`, or `union`.
    pub kind: String,
    pub ongoing: bool,
    pub start_observed: bool,
}

/// Go `CausalChain`. Field order is Go's emitted order. `links` is nullable
/// because Go's chronology-only path leaves the slice nil (JSON `null`) while
/// the recorded path always seeds it (`[]`).
#[derive(Debug, Clone, Serialize)]
pub struct CausalChain {
    pub bead_id: String,
    pub title: String,
    pub status: String,
    /// Recorded transitions in Git order; correlations are separate.
    pub events: Vec<CausalEvent>,
    /// Number of causal links.
    pub edge_count: usize,
    pub start_time: GoTime,
    pub end_time: GoTime,
    pub is_complete: bool,
    pub links: Option<Vec<CausalLink>>,
    /// Correlation is not causal evidence.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related_commits: Vec<HistoryCommit>,
    pub duration_known: bool,
    pub time_basis: String,
    /// Go's `MarshalJSON` shadows this with a nullable pointer, so the field is
    /// the serialized one and sits last. Always present, null when unknown.
    pub total_time: Option<i64>,
}

/// Go `CausalInsights`. Field order is Go's emitted order, which places the
/// seven durations Go's `MarshalJSON` shadows *after* the untouched fields
/// rather than at their declaration positions.
#[derive(Debug, Clone, Serialize)]
pub struct CausalInsights {
    pub blocked_periods: Option<Vec<BlockedPeriod>>,
    /// Event ids on the longest evidenced constraint path.
    pub critical_path: Option<Vec<usize>>,
    pub critical_path_desc: String,
    pub commit_count: usize,
    /// Go's tags for these three carry no `omitempty`, so each is always
    /// present and null when the measurement is unavailable.
    pub avg_time_between: Option<i64>,
    pub longest_gap: Option<i64>,
    pub longest_gap_desc: String,
    /// Declared by Go but never assigned. Go's tag carries no `omitempty`, so
    /// the key is always present and always null.
    pub estimated_without: Option<i64>,
    pub summary: String,
    pub recommendations: Vec<String>,
    /// `complete`, `partial`, `inconsistent`, or `unavailable`.
    pub coverage: String,
    pub limitations: Vec<String>,
    pub duration_known: bool,
    pub blocked_duration_known: bool,
    pub critical_path_duration_known: bool,
    pub explicit_blocked_periods: Option<Vec<BlockedPeriod>>,
    pub dependency_wait_periods: Option<Vec<BlockedPeriod>>,
    pub explicit_duration_known: bool,
    pub dependency_duration_known: bool,

    // --- the seven durations Go's MarshalJSON shadows ---
    pub total_duration: Option<i64>,
    pub blocked_duration: Option<i64>,
    /// Nonblocked elapsed, explicitly *not* execution effort.
    pub active_duration: Option<i64>,
    pub explicit_blocked_duration: Option<i64>,
    pub dependency_wait_duration: Option<i64>,
    pub blocked_percentage: Option<f64>,
    pub critical_path_duration: Option<i64>,
}

/// Go `CausalityResult` — the `--robot-causality` payload body.
#[derive(Debug, Clone, Serialize)]
pub struct CausalityResult {
    pub generated_at: GoTime,
    /// The report's bead fingerprint. The loader's file-level corpus hash is a
    /// different value; the envelope carries that one at the top level.
    pub data_hash: String,
    pub chain: CausalChain,
    pub insights: CausalInsights,
}

impl CausalityResult {
    /// The insights object as JSON, for direct embedding in an output payload.
    pub fn insights_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.insights).unwrap_or(serde_json::Value::Null)
    }
}

/// Go `CausalityOptions`.
#[derive(Debug, Clone, Default)]
pub struct CausalityOptions {
    /// Include correlated commits alongside the chain (Go's default is true;
    /// the CLI always sets it).
    pub include_commits: bool,
    /// Bead id -> title, used to name a dependency record in an event
    /// description when the record itself carries no title.
    pub blocker_titles: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Go `normalizeLifecycleStatus` / the correlation package's `normalizeStatus`.
pub fn normalize_status(status: &str) -> String {
    status.trim().to_lowercase()
}

/// Go `isClosedLifecycleStatus` — the status is already normalized.
pub fn is_closed_lifecycle_status(status: &str) -> bool {
    status == "closed" || status == "tombstone"
}

/// Go `isCompleteCausalStatus` — normalizes first.
pub fn is_complete_causal_status(status: &str) -> bool {
    is_closed_lifecycle_status(&normalize_status(status))
}

/// Go `causalStateWait` — `(explicit, dependency, known)`. A closed bead never
/// waits, and an unknown dependency state withholds both conclusions.
fn causal_state_wait(s: &CausalState) -> (bool, bool, bool) {
    let Some(issue) = s.issue.as_ref() else {
        return (false, false, false);
    };
    if !s.known {
        return (false, false, false);
    }
    if is_complete_causal_status(&issue.status) {
        return (false, false, true);
    }
    let status = normalize_status(&issue.status);
    (
        status == "blocked",
        s.dependency_state == DependencyState::Unsatisfied,
        s.dependency_state != DependencyState::Unknown,
    )
}

/// Go `sameCausalConstraints` — did the gate this bead is behind actually move?
/// Blocker identity and dependency state must match; the target's own status is
/// compared only when both sides carry it.
fn same_causal_constraints(a: &CausalState, b: &CausalState) -> bool {
    if a.known != b.known || a.dependency_state != b.dependency_state || a.blockers != b.blockers {
        return false;
    }
    match (&a.issue, &b.issue) {
        (None, None) => true,
        (Some(x), Some(y)) => x.status == y.status,
        _ => false,
    }
}

/// Go `appendBlockedPeriod` — merges into the previous period when it is
/// contiguous *and* the blocker set is unchanged, so one continuous wait does
/// not fragment per observation.
fn append_blocked_period(
    mut periods: Vec<BlockedPeriod>,
    start: &GoTime,
    end: &GoTime,
    blockers: &[String],
    kind: &str,
    observed: bool,
    ongoing: bool,
) -> Vec<BlockedPeriod> {
    if end.before(start) {
        return periods;
    }
    if let Some(last) = periods.last_mut() {
        if last.end_time == *start && last.blocker_ids == blockers {
            last.end_time = end.clone();
            last.duration += end.sub_nanos(start);
            last.ongoing = ongoing;
            return periods;
        }
    }
    let blocker_id = if blockers.len() == 1 {
        blockers[0].clone()
    } else {
        String::new()
    };
    periods.push(BlockedPeriod {
        start_time: start.clone(),
        end_time: end.clone(),
        duration: end.sub_nanos(start),
        blocker_id,
        blocker_ids: blockers.to_vec(),
        kind: kind.to_string(),
        ongoing,
        start_observed: observed,
    });
    periods
}

fn append_unique_int(ids: &mut Vec<usize>, id: usize) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// Go `formatInt` — a plain integer rendering, kept as an explicit helper so
/// the truncating casts at the call sites read the same way Go's do.
fn format_int(n: i64) -> String {
    n.to_string()
}

/// Go `formatDurationShort` — deliberately coarse, matching the wire format.
fn format_duration_short(d: i64) -> String {
    if d < HOUR_NANOS {
        return format_int(d / 60_000_000_000) + "m";
    }
    if d < DAY_NANOS {
        return format_int(d / HOUR_NANOS) + "h";
    }
    let days = d / DAY_NANOS;
    if days < 7 {
        return format_int(days) + "d";
    }
    let weeks = days / 7;
    if weeks < 4 {
        return format_int(weeks) + "w";
    }
    format_int(days / 30) + "mo"
}

/// Go `formatPercent` — truncates toward zero, it does not round.
fn format_percent(p: f64) -> String {
    format_int(p as i64) + "%"
}

fn format_gap_description(from: &CausalEvent, to: &CausalEvent, gap: i64) -> String {
    format!(
        "{} between {} and {}",
        format_duration_short(gap),
        from.event_type.as_str(),
        to.event_type.as_str()
    )
}

// ---------------------------------------------------------------------------
// Chain construction
// ---------------------------------------------------------------------------

/// Go `(*HistoryReport).BuildCausalityChainAt`.
///
/// Takes the target's `BeadHistory`, the retained committed constraints for
/// that target (if any), and a caller-owned reference instant. The reference
/// instant is the caller's, never the wall clock, so an open chain's duration
/// and the result's stamp cannot disagree; when it predates the first event the
/// open-chain duration is clamped rather than going negative.
pub fn build_causality_chain_at(
    history: &BeadHistory,
    causal_history: Option<&CausalHistory>,
    opts: &CausalityOptions,
    now: &GoTime,
) -> Option<CausalityResult> {
    match causal_history {
        Some(h) if h.bead_id == history.bead_id => {
            Some(build_recorded_causality(history, h, opts, now))
        }
        _ => Some(build_chronology_chain(history, opts, now)),
    }
}

/// Go's chronology-only path (`BuildCausalityChainAt` without retained
/// snapshots). Blocking durations are reported unavailable rather than zero,
/// because a commit log cannot prove what a bead was waiting on.
fn build_chronology_chain(
    history: &BeadHistory,
    opts: &CausalityOptions,
    now: &GoTime,
) -> CausalityResult {
    let zero = zero_time();
    let mut chain = CausalChain {
        bead_id: history.bead_id.clone(),
        title: history.title.clone(),
        status: history.status.clone(),
        events: Vec::new(),
        edge_count: 0,
        start_time: zero.clone(),
        end_time: zero.clone(),
        is_complete: is_complete_causal_status(&history.status),
        // Go leaves this slice nil here, so it serializes as null.
        links: None,
        related_commits: Vec::new(),
        duration_known: false,
        time_basis: "author timestamps; chronology only".to_string(),
        total_time: None,
    };

    struct RawEvent {
        timestamp: GoTime,
        event_type: CausalEventType,
        description: String,
        commit_sha: String,
    }
    let mut raw: Vec<RawEvent> = Vec::new();

    for event in &history.events {
        let Some(event_type) = CausalEventType::of_event_type(event.event_type) else {
            continue;
        };
        let description = match event.event_type {
            EventType::Created => "Bead created",
            EventType::Claimed => "Work started (claimed)",
            EventType::Closed => "Work completed (closed)",
            EventType::Reopened => "Bead reopened",
            _ => "Bead removed from the historical source",
        };
        let Ok(timestamp) = GoTime::parse(&event.timestamp) else {
            continue;
        };
        raw.push(RawEvent {
            timestamp,
            event_type,
            description: description.to_string(),
            commit_sha: String::new(),
        });
    }

    if opts.include_commits {
        for commit in history.commits.iter().flatten() {
            let Ok(timestamp) = GoTime::parse(&commit.timestamp) else {
                continue;
            };
            // Truncate by characters, not bytes, so a multi-byte message is not
            // cut mid-rune.
            let mut desc: String = commit.message.chars().take(47).collect();
            let message_len = commit.message.chars().count();
            if message_len > 50 {
                desc.push_str("...");
            } else {
                desc = commit.message.clone();
            }
            raw.push(RawEvent {
                timestamp,
                event_type: CausalEventType::Commit,
                description: format!("Commit: {desc}"),
                commit_sha: commit.short_sha.clone(),
            });
        }
    }

    raw.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.event_type.order().cmp(&b.event_type.order()))
            .then_with(|| a.commit_sha.cmp(&b.commit_sha))
            .then_with(|| a.description.cmp(&b.description))
    });

    for (i, r) in raw.into_iter().enumerate() {
        chain.events.push(CausalEvent {
            id: i,
            blocker_id: String::new(),
            caused_by_id: None,
            enables_ids: Vec::new(),
            duration_next: None,
            source_bead_id: String::new(),
            committed_at: None,
            before: None,
            after: None,
            transition_observed: false,
            ..CausalEvent::new(i, r.event_type, r.timestamp, r.description)
        });
        let event = chain.events.last_mut().expect("just pushed");
        event.commit_sha = r.commit_sha;
    }

    if !chain.events.is_empty() {
        chain.start_time = chain.events[0].timestamp.clone();
        chain.end_time = chain.events[chain.events.len() - 1].timestamp.clone();
        if !chain.is_complete {
            chain.end_time = now.clone();
            if chain.end_time.before(&chain.start_time) {
                chain.end_time = chain.start_time.clone();
            }
        }
        let total = chain.end_time.sub_nanos(&chain.start_time);
        // Only a recorded creation bounds a duration; a chain that merely
        // mentions a commit has no known start.
        chain.duration_known = chain.events[0].event_type == CausalEventType::Created;
        // Go's MarshalJSON nulls this unless DurationKnown, so the chain must
        // not claim a span it cannot vouch for.
        chain.total_time = chain.duration_known.then_some(total);
    }

    chain.edge_count = chain.events.iter().map(|e| e.enables_ids.len()).sum();

    let mut insights = build_insights(&mut chain);
    insights.duration_known = chain.duration_known;
    insights.coverage = "unavailable".to_string();
    insights.limitations = vec!["Committed dependency snapshots were not retained; chronology does not establish blocking or causation.".to_string()];
    insights.summary = "Lifecycle chronology only; blocking duration is unavailable".to_string();
    insights.recommendations = insights.limitations.clone();

    CausalityResult {
        generated_at: now.clone(),
        data_hash: String::new(),
        chain,
        insights,
    }
}

/// Go's zero `time.Time` — the chain's untouched start/end before any event is
/// seen, matching Go's `0001-01-01T00:00:00Z`.
fn zero_time() -> GoTime {
    GoTime::parse("0001-01-01T00:00:00Z").expect("the zero time is a valid RFC3339 instant")
}

/// Go `buildRecordedCausality` — the retained-snapshot path.
fn build_recorded_causality(
    history: &BeadHistory,
    causal_history: &CausalHistory,
    opts: &CausalityOptions,
    now: &GoTime,
) -> CausalityResult {
    let observations = &causal_history.observations;
    let mut chain = CausalChain {
        bead_id: history.bead_id.clone(),
        title: history.title.clone(),
        status: history.status.clone(),
        events: Vec::new(),
        edge_count: 0,
        start_time: zero_time(),
        end_time: zero_time(),
        is_complete: is_complete_causal_status(&history.status),
        links: Some(Vec::new()),
        related_commits: Vec::new(),
        duration_known: false,
        time_basis:
            "author timestamps of committed snapshots, in Git first-parent order; intervals are observed history, not execution effort"
                .to_string(),
        total_time: None,
    };
    let mut insights = CausalInsights {
        blocked_periods: Some(Vec::new()),
        critical_path: Some(Vec::new()),
        critical_path_desc: String::new(),
        commit_count: 0,
        avg_time_between: None,
        longest_gap: None,
        longest_gap_desc: String::new(),
        estimated_without: None,
        summary: String::new(),
        recommendations: Vec::new(),
        coverage: "complete".to_string(),
        limitations: Vec::new(),
        duration_known: true,
        blocked_duration_known: true,
        critical_path_duration_known: false,
        explicit_blocked_periods: Some(Vec::new()),
        dependency_wait_periods: Some(Vec::new()),
        explicit_duration_known: true,
        dependency_duration_known: true,
        total_duration: None,
        blocked_duration: None,
        active_duration: None,
        explicit_blocked_duration: None,
        dependency_wait_duration: None,
        blocked_percentage: None,
        critical_path_duration: None,
    };
    if opts.include_commits {
        chain.related_commits = history.commits.clone().unwrap_or_default();
        insights.commit_count = chain.related_commits.len();
    }

    // Register a limitation once, and demote `complete` to `partial`. An
    // already-inconsistent coverage is left alone.
    fn limit(insights: &mut CausalInsights, reason: &str) {
        if insights.limitations.iter().any(|l| l == reason) {
            return;
        }
        insights.limitations.push(reason.to_string());
        if insights.coverage == "complete" {
            insights.coverage = "partial".to_string();
        }
    }

    // `boundary[i]` is the id of the event that carries observation i's effect
    // on the target, or -1 when the target's own record did not change.
    let mut boundaries: Vec<isize> = vec![-1; observations.len()];
    let mut first: isize = -1;
    let mut clocks_valid = true;

    for (i, obs) in observations.iter().enumerate() {
        if i > 0 {
            let prev = &observations[i - 1];
            if obs.timestamp.before(&prev.timestamp) || obs.committed_at.before(&prev.committed_at)
            {
                clocks_valid = false;
                limit(
                    &mut insights,
                    "Git transition order contradicts author or committer clocks; elapsed measurements are unavailable.",
                );
                insights.coverage = "inconsistent".to_string();
            }
        }
        if obs.after.issue.is_some() && first < 0 {
            first = i as isize;
            chain.start_time = obs.timestamp.clone();
            if obs.before.issue.is_some() || !obs.before.known {
                insights.duration_known = false;
                insights.blocked_duration_known = false;
                insights.explicit_duration_known = false;
                insights.dependency_duration_known = false;
                limit(
                    &mut insights,
                    "Creation predates the retained window or the initial source is incomplete.",
                );
            }
        }

        let mut causes: Vec<usize> = Vec::new();
        for change in &obs.changes {
            let mut event_type = CausalEventType::ConstraintChange;
            let mut description = format!("Dependency record changed: {}", change.bead_id);
            let mut title = opts
                .blocker_titles
                .get(&change.bead_id)
                .cloned()
                .unwrap_or_default();
            if let Some(after) = &change.after {
                if !after.title.is_empty() {
                    title = after.title.clone();
                }
            } else if let Some(before) = &change.before {
                if !before.title.is_empty() {
                    title = before.title.clone();
                }
            }
            if !title.is_empty() {
                description.push_str(&format!(" ({title})"));
            }

            let is_target = change.bead_id == history.bead_id;
            if is_target {
                event_type = CausalEventType::Changed;
                description = "Bead record changed".to_string();
                match change.event_type {
                    EventType::Created => {
                        event_type = CausalEventType::Created;
                        description = "Bead first recorded".to_string();
                    }
                    EventType::Claimed => {
                        event_type = CausalEventType::Claimed;
                        description = "Work claimed".to_string();
                    }
                    EventType::Closed => {
                        event_type = CausalEventType::Closed;
                        description = "Bead closed".to_string();
                    }
                    EventType::Reopened => {
                        event_type = CausalEventType::Reopened;
                        description = "Bead reopened".to_string();
                    }
                    EventType::Deleted => {
                        event_type = CausalEventType::Deleted;
                        description = "Bead removed from the historical source".to_string();
                    }
                    EventType::Modified => {}
                }
                // An explicit "blocked" status is a stronger statement than a
                // dependency-derived wait, so it wins the type.
                if change
                    .after
                    .as_ref()
                    .is_some_and(|a| normalize_status(&a.status) == "blocked")
                    && change
                        .before
                        .as_ref()
                        .is_none_or(|b| normalize_status(&b.status) != "blocked")
                {
                    event_type = CausalEventType::Blocked;
                    description = "Explicit blocked status recorded".to_string();
                }
                if change
                    .before
                    .as_ref()
                    .is_some_and(|b| normalize_status(&b.status) == "blocked")
                    && change.after.as_ref().is_some_and(|a| {
                        !is_complete_causal_status(&a.status)
                            && normalize_status(&a.status) != "blocked"
                    })
                {
                    event_type = CausalEventType::Unblocked;
                    description = "Explicit blocked status cleared".to_string();
                }
            }
            if !change.transition_observed {
                description = format!(
                    "Incomplete source: record evidence for {}; transition is uncertain",
                    change.bead_id
                );
            }

            let id = chain.events.len();
            chain.events.push(CausalEvent {
                commit_sha: obs.commit_sha.clone(),
                committed_at: Some(obs.committed_at.clone()),
                source_bead_id: change.bead_id.clone(),
                before: change.before.clone(),
                after: change.after.clone(),
                transition_observed: change.transition_observed,
                ..CausalEvent::new(id, event_type, obs.timestamp.clone(), description)
            });
            if is_target {
                boundaries[i] = id as isize;
            } else {
                causes.push(id);
            }
        }

        // Dependency transitions can happen while the target record is
        // untouched, so the target's consequence is emitted after the source
        // changes of the same commit.
        let gate_changed = !same_causal_constraints(&obs.before, &obs.after);
        if gate_changed && (boundaries[i] < 0 || !causes.is_empty()) {
            let (explicit, dependency, known) = causal_state_wait(&obs.after);
            let (event_type, description) = if known {
                if explicit || dependency {
                    (
                        CausalEventType::Blocked,
                        "Recorded constraints prevent readiness",
                    )
                } else {
                    (
                        CausalEventType::Unblocked,
                        "Recorded constraints no longer prevent readiness",
                    )
                }
            } else {
                (
                    CausalEventType::ConstraintChange,
                    "Dependency state became unknown",
                )
            };
            let id = chain.events.len();
            chain.events.push(CausalEvent {
                commit_sha: obs.commit_sha.clone(),
                committed_at: Some(obs.committed_at.clone()),
                source_bead_id: history.bead_id.clone(),
                transition_observed: obs.before.known && obs.after.known,
                ..CausalEvent::new(
                    id,
                    event_type,
                    obs.timestamp.clone(),
                    description.to_string(),
                )
            });
            boundaries[i] = id as isize;
        }

        // A lifecycle change or a creation is not by itself evidence that
        // another record caused it: require a real, known dependency gate
        // transition while the target stayed nonterminal on both sides.
        let dependency_changed = obs.before.issue.is_some()
            && obs.after.issue.is_some()
            && obs
                .before
                .issue
                .as_ref()
                .is_some_and(|i| !is_complete_causal_status(&i.status))
            && obs
                .after
                .issue
                .as_ref()
                .is_some_and(|i| !is_complete_causal_status(&i.status))
            && obs.before.dependency_state != obs.after.dependency_state
            && obs.before.dependency_state != DependencyState::Unknown
            && obs.after.dependency_state != DependencyState::Unknown;
        if dependency_changed && obs.before.known && obs.after.known {
            for cause in causes {
                let e = &chain.events[cause];
                // Skip a record that changed for an unrelated reason.
                if let (Some(before), Some(after)) = (&e.before, &e.after) {
                    if is_complete_causal_status(&before.status)
                        == is_complete_causal_status(&after.status)
                        && before.dependencies == after.dependencies
                    {
                        continue;
                    }
                }
                link(
                    &mut chain,
                    cause as isize,
                    boundaries[i],
                    "dependency_transition",
                    "This committed dependency-state change accompanies the target gate transition; simultaneous changes are joint evidence, not isolated causal estimates.",
                    0,
                );
            }
        }
    }

    if first < 0 {
        insights.coverage = "unavailable".to_string();
        insights.duration_known = false;
        insights.blocked_duration_known = false;
        insights.explicit_duration_known = false;
        insights.dependency_duration_known = false;
        limit(&mut insights, "No retained committed state for this bead.");
    } else {
        // The measurement half can itself discover contradictory clocks (a
        // revision's own stamps, or a reference instant before the first
        // event), and Go shares one `clocksValid` flag across both halves.
        clocks_valid = measure_recorded(
            history,
            causal_history,
            &boundaries,
            &mut chain,
            &mut insights,
            first as usize,
            clocks_valid,
            now,
        );
    }

    chain.duration_known = insights.duration_known;
    chain.edge_count = chain.links.as_ref().map_or(0, Vec::len);
    build_constraint_path(&mut chain, &mut insights);
    insights.critical_path_duration_known = clocks_valid
        && insights
            .critical_path
            .as_ref()
            .is_some_and(|p| !p.is_empty());
    if !clocks_valid
        && insights
            .critical_path
            .as_ref()
            .is_some_and(|p| !p.is_empty())
    {
        insights.critical_path_desc =
            "Dependency transitions are retained, but inconsistent clocks prevent measuring the constraint path."
                .to_string();
    }
    if clocks_valid {
        populate_causal_gaps(&mut chain, &mut insights);
    }
    insights.summary = build_summary(&chain, &insights);
    if !insights.duration_known || !insights.blocked_duration_known {
        insights.summary =
            "Partial historical evidence; total or blocked duration is unavailable".to_string();
    }
    insights.recommendations = generate_recommendations(&chain, &insights);
    if !insights.duration_known || !insights.blocked_duration_known {
        let mut recs = vec![
            "Review the incomplete history or inconsistent clocks before drawing duration conclusions."
                .to_string(),
        ];
        recs.extend(insights.limitations.iter().cloned());
        insights.recommendations = recs;
    }
    insights.recommendations.push(
        "Nonblocked elapsed time is not execution effort or a minimum completion-time estimate."
            .to_string(),
    );

    CausalityResult {
        generated_at: now.clone(),
        data_hash: String::new(),
        chain,
        insights,
    }
}

/// Go's link closure: record the edge, note the forward `enables` on the source
/// and the first `caused_by` on the target.
#[allow(clippy::too_many_arguments)]
fn link(
    chain: &mut CausalChain,
    from: isize,
    to: isize,
    kind: &str,
    evidence: &str,
    duration: i64,
) {
    // Both endpoints are event ids that may be absent (-1). A link needs a
    // real source and a strictly later target; anything else is not an edge.
    // Taking `isize` for both keeps a missing id from wrapping to usize::MAX.
    if from < 0 || to < 0 || to <= from {
        return;
    }
    let from = from as usize;
    let to = to as usize;
    if let Some(links) = chain.links.as_mut() {
        links.push(CausalLink {
            from,
            to,
            kind: kind.to_string(),
            evidence: evidence.to_string(),
            duration,
        });
    }
    append_unique_int(&mut chain.events[from].enables_ids, to);
    if chain.events[to].caused_by_id.is_none() {
        chain.events[to].caused_by_id = Some(from);
    }
}

/// The measurement half of `buildRecordedCausality`: the open end, the
/// blocked-interval accumulation, and the totals.
#[allow(clippy::too_many_arguments)]
fn measure_recorded(
    history: &BeadHistory,
    causal_history: &CausalHistory,
    boundaries: &[isize],
    chain: &mut CausalChain,
    insights: &mut CausalInsights,
    first: usize,
    clocks_valid: bool,
    now: &GoTime,
) -> bool {
    let observations = &causal_history.observations;
    let mut clocks_valid = clocks_valid;

    let last = observations
        .last()
        .expect("the caller only reaches here when an observation exists");
    chain.end_time = now.clone();
    if let Some(reference_time) = &causal_history.reference_time {
        chain.end_time = reference_time.clone();
        let reference_ok = causal_history
            .reference_committed_at
            .as_ref()
            .is_some_and(|rc| {
                !reference_time.before(&last.timestamp) && !rc.before(&last.committed_at)
            });
        if !reference_ok {
            clocks_valid = false;
            add_limitation(
                insights,
                "The requested revision's clocks contradict the retained transition order; elapsed measurements are unavailable.",
            );
            insights.coverage = "inconsistent".to_string();
        }
    }
    if let Some(until) = &causal_history.until {
        if *until < chain.end_time {
            chain.end_time = until.clone();
            add_limitation(
                insights,
                "The history ends at the requested cutoff; later state is not observed.",
            );
        }
    }
    // The live issue set and the retained committed state must agree, or the
    // measurement describes a different bead state than the one reported.
    let status_agrees = last
        .after
        .issue
        .as_ref()
        .is_some_and(|i| normalize_status(&i.status) == normalize_status(&history.status));
    if !status_agrees {
        insights.blocked_duration_known = false;
        insights.duration_known = false;
        insights.explicit_duration_known = false;
        insights.dependency_duration_known = false;
        add_limitation(
            insights,
            "Current status differs from the retained committed state.",
        );
    }
    // A closed bead's clock stops at the commit that closed it, not at the
    // reference instant.
    if last
        .after
        .issue
        .as_ref()
        .is_some_and(|i| is_complete_causal_status(&i.status))
    {
        for obs in observations[first..].iter().rev() {
            let became_closed = obs
                .after
                .issue
                .as_ref()
                .is_some_and(|i| is_complete_causal_status(&i.status))
                && obs
                    .before
                    .issue
                    .as_ref()
                    .is_none_or(|i| !is_complete_causal_status(&i.status));
            if became_closed {
                chain.end_time = obs.timestamp.clone();
                break;
            }
        }
    }
    if chain.end_time.before(&chain.start_time) {
        clocks_valid = false;
        add_limitation(
            insights,
            "The reference clock precedes the observed lifecycle.",
        );
        insights.coverage = "inconsistent".to_string();
    }

    let mut wait_start_id: isize = -1;
    let mut wait_start: Option<&GoTime> = None;
    let mut blocked_duration: i64 = 0;
    let mut explicit_duration: i64 = 0;
    let mut dependency_duration: i64 = 0;

    for i in first..observations.len() {
        let obs = &observations[i];
        let (explicit, dependency, known) = causal_state_wait(&obs.after);
        if !known {
            insights.dependency_duration_known = false;
            if !explicit {
                insights.blocked_duration_known = false;
            }
            if !obs.after.known || obs.after.issue.is_none() {
                insights.explicit_duration_known = false;
            }
            add_limitation(insights, "Some dependency intervals are unknown because records are missing, invalid, deleted, or cyclic through parents.");
        }
        if i > first && !same_causal_constraints(&observations[i - 1].after, &obs.before) {
            insights.blocked_duration_known = false;
            insights.explicit_duration_known = false;
            insights.dependency_duration_known = false;
            add_limitation(insights, "The retained snapshots have a discontinuity.");
        }

        let blocked = explicit || dependency;
        if blocked && wait_start_id < 0 {
            wait_start_id = boundaries[i];
            wait_start = Some(&obs.timestamp);
        }
        if !blocked && wait_start_id >= 0 {
            if known && clocks_valid {
                let start = wait_start.expect("set whenever wait_start_id is");
                link(
                    chain,
                    wait_start_id,
                    boundaries[i],
                    "observed_wait",
                    "Recorded blocked interval ended at this observed state transition.",
                    obs.timestamp.sub_nanos(start),
                );
            }
            wait_start_id = -1;
        }

        // The interval runs to the next observation, or to the chain's end.
        let mut end = chain.end_time.clone();
        if let Some(next) = observations.get(i + 1) {
            if next.timestamp < end {
                end = next.timestamp.clone();
            }
        }
        if !clocks_valid || end.before(&obs.timestamp) || obs.after.issue.is_none() {
            continue;
        }
        if obs
            .after
            .issue
            .as_ref()
            .is_some_and(|i| is_complete_causal_status(&i.status))
        {
            continue;
        }
        let ongoing = i == observations.len() - 1 && !chain.is_complete;
        // The start of this interval is only *observed* when the previous
        // state positively showed no wait.
        let start_observed = obs.before.issue.is_none() || {
            let (e, d, k) = causal_state_wait(&obs.before);
            k && !e && !d
        };

        if blocked {
            let periods = insights.blocked_periods.take().unwrap_or_default();
            insights.blocked_periods = Some(append_blocked_period(
                periods,
                &obs.timestamp,
                &end,
                &obs.after.blockers,
                "union",
                start_observed,
                ongoing,
            ));
            blocked_duration += end.sub_nanos(&obs.timestamp);
        }
        if explicit {
            let periods = insights.explicit_blocked_periods.take().unwrap_or_default();
            insights.explicit_blocked_periods = Some(append_blocked_period(
                periods,
                &obs.timestamp,
                &end,
                &[],
                "explicit_status",
                start_observed,
                ongoing,
            ));
            explicit_duration += end.sub_nanos(&obs.timestamp);
        }
        if dependency {
            let periods = insights.dependency_wait_periods.take().unwrap_or_default();
            insights.dependency_wait_periods = Some(append_blocked_period(
                periods,
                &obs.timestamp,
                &end,
                &obs.after.blockers,
                "dependency",
                start_observed,
                ongoing,
            ));
            dependency_duration += end.sub_nanos(&obs.timestamp);
        }
    }

    if wait_start_id >= 0 && clocks_valid && !chain.is_complete && insights.blocked_duration_known {
        let start = wait_start.expect("set whenever wait_start_id is");
        let id = chain.events.len();
        chain.events.push(CausalEvent::new(
            id,
            CausalEventType::Observation,
            chain.end_time.clone(),
            "Open wait measured through the reference clock; no release event is inferred"
                .to_string(),
        ));
        chain.events[id].source_bead_id = history.bead_id.clone();
        link(
            chain,
            wait_start_id,
            id as isize,
            "ongoing_wait",
            "The last known blocked state remains observed through the caller's reference instant.",
            chain.end_time.sub_nanos(start),
        );
    }

    if clocks_valid {
        let total = chain.end_time.sub_nanos(&chain.start_time);
        // Go's chain MarshalJSON gates `total_time` on DurationKnown alone.
        chain.total_time = insights.duration_known.then_some(total);
        if insights.duration_known {
            insights.total_duration = Some(total);
        }
        if insights.blocked_duration_known {
            insights.blocked_duration = Some(blocked_duration);
        }
        if insights.duration_known && insights.blocked_duration_known {
            insights.active_duration = Some(total - blocked_duration);
            // Go skips the division when the total is zero but still emits the
            // field, so a zero-length window reports 0%, not null.
            insights.blocked_percentage = Some(if total > 0 {
                blocked_duration as f64 / total as f64 * 100.0
            } else {
                0.0
            });
        }
        if insights.explicit_duration_known {
            insights.explicit_blocked_duration = Some(explicit_duration);
        }
        if insights.dependency_duration_known {
            insights.dependency_wait_duration = Some(dependency_duration);
        }
    } else {
        insights.duration_known = false;
        insights.blocked_duration_known = false;
        insights.explicit_duration_known = false;
        insights.dependency_duration_known = false;
    }
    clocks_valid
}

/// The same one-shot `limit` closure `buildRecordedCausality` uses, for the
/// measurement half which runs in its own function.
fn add_limitation(insights: &mut CausalInsights, reason: &str) {
    if insights.limitations.iter().any(|l| l == reason) {
        return;
    }
    insights.limitations.push(reason.to_string());
    if insights.coverage == "complete" {
        insights.coverage = "partial".to_string();
    }
}

/// Go `buildConstraintPath` — the longest weighted path in the observed
/// constraint DAG. Events with no evidence link never join the path merely
/// because they happened in between.
fn build_constraint_path(chain: &mut CausalChain, insights: &mut CausalInsights) {
    let n = chain.events.len();
    let mut weights: Vec<i64> = vec![0; n];
    let mut paths: Vec<Vec<usize>> = vec![Vec::new(); n];
    let links = chain.links.clone().unwrap_or_default();

    for to in 0..n {
        for edge in &links {
            if edge.to != to {
                continue;
            }
            let weight = weights[edge.from] + edge.duration;
            if paths[to].is_empty() || weight > weights[to] {
                let mut path = paths[edge.from].clone();
                if path.is_empty() {
                    path.push(edge.from);
                }
                path.push(to);
                paths[to] = path;
                weights[to] = weight;
            }
        }
        if !paths[to].is_empty()
            && (insights.critical_path.as_ref().is_none_or(Vec::is_empty)
                || weights[to] > insights.critical_path_duration.unwrap_or(0))
        {
            insights.critical_path = Some(paths[to].clone());
            insights.critical_path_duration = Some(weights[to]);
        }
    }

    if insights
        .critical_path
        .as_ref()
        .is_some_and(|p| !p.is_empty())
    {
        insights.critical_path_desc = format!(
            "Longest evidenced constraint path: {} observed waiting; not a project schedule",
            format_duration_short(insights.critical_path_duration.unwrap_or(0))
        );
    } else {
        insights.critical_path_desc =
            "No evidence-supported constraint path in the retained history".to_string();
    }
}

/// Go `buildInsights` — the chronology path's insights.
fn build_insights(chain: &mut CausalChain) -> CausalInsights {
    let mut insights = CausalInsights {
        blocked_periods: Some(Vec::new()),
        critical_path: Some(Vec::new()),
        critical_path_desc: String::new(),
        commit_count: chain
            .events
            .iter()
            .filter(|e| e.event_type == CausalEventType::Commit)
            .count(),
        avg_time_between: None,
        longest_gap: None,
        longest_gap_desc: String::new(),
        estimated_without: None,
        summary: String::new(),
        recommendations: Vec::new(),
        coverage: String::new(),
        limitations: Vec::new(),
        duration_known: false,
        blocked_duration_known: false,
        critical_path_duration_known: false,
        // Go leaves these nil in the chronology path, so they serialize null.
        explicit_blocked_periods: None,
        dependency_wait_periods: None,
        explicit_duration_known: false,
        dependency_duration_known: false,
        // Go seeds this from the chain's own total; the nullable shadow in its
        // MarshalJSON drops it again when `duration_known` is false.
        total_duration: chain.total_time,
        blocked_duration: None,
        active_duration: None,
        explicit_blocked_duration: None,
        dependency_wait_duration: None,
        blocked_percentage: None,
        critical_path_duration: None,
    };
    populate_causal_gaps(chain, &mut insights);
    insights.summary = build_summary(chain, &insights);
    insights.recommendations = generate_recommendations(chain, &insights);
    insights
}

/// Go `populateCausalGaps` — observed transition chronology, not causation.
/// Contradictory clocks leave every gap field unavailable and preserve Git order.
fn populate_causal_gaps(chain: &mut CausalChain, insights: &mut CausalInsights) {
    for i in 1..chain.events.len() {
        if chain.events[i].timestamp < chain.events[i - 1].timestamp {
            return;
        }
    }
    if chain.events.len() > 1 {
        let mut total_gap: i64 = 0;
        let mut longest_gap: i64 = 0;
        // The first *valid* gap index, so the description never points at a
        // gap that was not measured.
        let mut longest_gap_idx: usize = 1;
        for i in 1..chain.events.len() {
            let gap = chain.events[i]
                .timestamp
                .sub_nanos(&chain.events[i - 1].timestamp);
            chain.events[i - 1].duration_next = Some(gap);
            total_gap += gap;
            if gap > longest_gap {
                longest_gap = gap;
                longest_gap_idx = i;
            }
        }
        // Integer division, as Go's Duration division truncates.
        insights.avg_time_between = Some(total_gap / (chain.events.len() as i64 - 1));
        insights.longest_gap = Some(longest_gap);
        insights.longest_gap_desc = format_gap_description(
            &chain.events[longest_gap_idx - 1],
            &chain.events[longest_gap_idx],
            longest_gap,
        );
    }
}

/// Go `buildSummary` — one line, describing elapsed time rather than effort.
fn build_summary(chain: &CausalChain, insights: &CausalInsights) -> String {
    let total = insights.total_duration.unwrap_or(0);
    let pct = insights.blocked_percentage.unwrap_or(0.0);
    if !chain.is_complete {
        if pct > 50.0 {
            return format!(
                "In progress, mostly blocked ({} total, {} blocked)",
                format_duration_short(total),
                format_percent(pct)
            );
        }
        return format!(
            "In progress for {} with {} commits",
            format_duration_short(total),
            format_int(insights.commit_count as i64)
        );
    }
    if pct > 30.0 {
        return format!(
            "Completed in {} ({} blocked)",
            format_duration_short(total),
            format_percent(pct)
        );
    }
    format!(
        "Completed in {} with {} commits",
        format_duration_short(total),
        format_int(insights.commit_count as i64)
    )
}

/// Go `generateRecommendations` — actionable observations, never a fabricated
/// diagnosis.
fn generate_recommendations(chain: &CausalChain, insights: &CausalInsights) -> Vec<String> {
    let mut recs: Vec<String> = Vec::new();
    let pct = insights.blocked_percentage.unwrap_or(0.0);
    let total = insights.total_duration.unwrap_or(0);

    if pct > 50.0 {
        recs.push(format!(
            "High blocked percentage ({}) - consider addressing blockers earlier in the process",
            format_percent(pct)
        ));
    }
    if let Some(gap) = insights.longest_gap {
        if gap > 7 * DAY_NANOS {
            recs.push(format!(
                "Longest gap of {} - consider breaking work into smaller pieces",
                format_duration_short(gap)
            ));
        }
    }
    if total > 7 * DAY_NANOS && insights.commit_count < 3 {
        recs.push(format!(
            "Few commits over {} - consider more frequent incremental commits",
            format_duration_short(total)
        ));
    }
    if !chain.is_complete && total > 14 * DAY_NANOS {
        recs.push(format!(
            "Open for {} - consider breaking into subtasks or closing if complete",
            format_duration_short(total)
        ));
    }
    if recs.is_empty() {
        recs.push("No significant issues detected in the causal flow".to_string());
    }
    recs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> GoTime {
        GoTime::parse(s).expect("valid timestamp")
    }

    fn event(bead_id: &str, ty: EventType, ts: &str) -> BeadEvent {
        BeadEvent {
            bead_id: bead_id.into(),
            event_type: ty,
            timestamp: ts.into(),
            commit_sha: "sha".into(),
            commit_msg: String::new(),
            author: "alice".into(),
            author_email: String::new(),
            before: None,
            after: None,
            transition_observed: true,
        }
    }

    fn history(status: &str) -> BeadHistory {
        BeadHistory {
            bead_id: "A-1".into(),
            title: "A bead".into(),
            status: status.into(),
            events: vec![event("A-1", EventType::Created, "2026-01-01T00:00:00Z")],
            milestones: Default::default(),
            commits: None,
            cycle_time: None,
            last_author: String::new(),
        }
    }

    fn state(status: &str, known: bool, dep: DependencyState) -> CausalState {
        CausalState {
            issue: Some(HistoricalIssueState {
                id: "A-1".into(),
                status: status.into(),
                title: "A bead".into(),
                dependencies: Vec::new(),
            }),
            known,
            dependency_state: dep,
            blockers: Vec::new(),
            reason: String::new(),
        }
    }

    fn observation(
        sha: &str,
        ts: &str,
        before: CausalState,
        after: CausalState,
    ) -> CausalObservation {
        CausalObservation {
            commit_sha: sha.into(),
            timestamp: t(ts),
            committed_at: t(ts),
            before,
            after,
            changes: Vec::new(),
        }
    }

    /// A snapshot in which the target is absent. `known = true` is the normal
    /// creation case (the file existed, the bead did not); `known = false`
    /// means the source itself is incomplete, which is what makes a duration
    /// unknowable.
    fn absent(known: bool) -> CausalState {
        CausalState {
            issue: None,
            known,
            dependency_state: DependencyState::Unknown,
            blockers: Vec::new(),
            reason: String::new(),
        }
    }

    fn history_of(obs: Vec<CausalObservation>) -> CausalHistory {
        CausalHistory {
            bead_id: "A-1".into(),
            observations: obs,
            until: None,
            revision: String::new(),
            reference_time: None,
            reference_committed_at: None,
        }
    }

    #[test]
    fn chronology_path_reports_blocking_as_unavailable() {
        let h = history("open");
        let r = build_causality_chain_at(
            &h,
            None,
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        assert_eq!(r.insights.coverage, "unavailable");
        // The chain does start at a recorded creation, so its own span is
        // known — but nothing about *blocking* is.
        assert!(r.insights.duration_known);
        assert!(r.insights.blocked_duration.is_none());
        assert!(r.insights.blocked_percentage.is_none());
        // Go's chronology path leaves the slice nil.
        assert!(r.chain.links.is_none());
        assert_eq!(r.chain.time_basis, "author timestamps; chronology only");
        assert_eq!(r.insights.total_duration, Some(86_400_000_000_000));
    }

    #[test]
    fn chronology_duration_is_unknown_without_a_recorded_creation() {
        // A chain that only mentions a commit has no known start, so the span
        // must stay null rather than become a zero.
        let mut h = history("open");
        h.events = vec![event("A-1", EventType::Modified, "2026-01-01T00:00:00Z")];
        let r = build_causality_chain_at(
            &h,
            None,
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        assert!(!r.chain.duration_known);
        assert!(!r.insights.duration_known);
        assert!(r.insights.total_duration.is_none());
    }

    #[test]
    fn recorded_path_seeds_links_and_reports_complete() {
        let obs = vec![observation(
            "s1",
            "2026-01-01T00:00:00Z",
            // A known snapshot in which the bead does not yet exist: the
            // creation is fully observed, so nothing is limited.
            absent(true),
            state("open", true, DependencyState::Satisfied),
        )];
        let h = history("open");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(obs)),
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        assert!(r.chain.links.is_some());
        assert_eq!(r.insights.coverage, "complete");
        assert_eq!(r.insights.total_duration, Some(86_400_000_000_000));
        assert_eq!(r.insights.blocked_duration, Some(0));
    }

    #[test]
    fn an_incomplete_source_demotes_coverage_to_partial() {
        let obs = vec![observation(
            "s1",
            "2026-01-01T00:00:00Z",
            absent(false),
            state("open", true, DependencyState::Satisfied),
        )];
        let h = history("open");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(obs)),
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        assert_eq!(r.insights.coverage, "partial");
        assert!(!r.insights.duration_known);
        assert!(r.insights.total_duration.is_none());
        assert!(r
            .insights
            .limitations
            .iter()
            .any(|l| l.contains("Creation predates the retained window")));
    }

    #[test]
    fn creation_before_the_window_makes_duration_unknown() {
        // The target already exists in `before`, so the retained window does
        // not contain its creation.
        let obs = vec![observation(
            "s1",
            "2026-01-01T00:00:00Z",
            state("open", true, DependencyState::Satisfied),
            state("in_progress", true, DependencyState::Satisfied),
        )];
        let h = history("in_progress");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(obs)),
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        assert!(!r.insights.duration_known);
        assert!(r.insights.total_duration.is_none());
        assert!(r
            .insights
            .limitations
            .iter()
            .any(|l| l.contains("Creation predates the retained window")));
    }

    #[test]
    fn no_retained_state_is_unavailable() {
        let h = history("open");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(Vec::new())),
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        assert_eq!(r.insights.coverage, "unavailable");
        assert!(r
            .insights
            .limitations
            .iter()
            .any(|l| l == "No retained committed state for this bead."));
    }

    #[test]
    fn blocked_dependency_yields_a_period_and_percentage() {
        // Created ready, then a live blocker appears, then it closes.
        let unsat = CausalState {
            blockers: vec!["B-1".into()],
            ..state("open", true, DependencyState::Unsatisfied)
        };
        let unsat_before = unsat.clone();
        let obs = vec![
            observation(
                "s1",
                "2026-01-01T00:00:00Z",
                absent(true),
                state("open", true, DependencyState::Satisfied),
            ),
            observation(
                "s2",
                "2026-01-02T00:00:00Z",
                state("open", true, DependencyState::Satisfied),
                unsat,
            ),
            observation(
                "s3",
                "2026-01-03T00:00:00Z",
                unsat_before,
                state("closed", true, DependencyState::Satisfied),
            ),
        ];
        let h = history("closed");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(obs)),
            &CausalityOptions::default(),
            &t("2026-01-04T00:00:00Z"),
        )
        .expect("history exists");
        let periods = r.insights.blocked_periods.as_ref().expect("array");
        assert_eq!(
            periods.len(),
            1,
            "one contiguous wait, not one per observation"
        );
        assert_eq!(periods[0].blocker_id, "B-1");
        assert_eq!(periods[0].duration, 86_400_000_000_000);
        assert_eq!(periods[0].kind, "union");
        // A closed bead ends at the closing commit, not the reference instant.
        assert_eq!(r.chain.end_time.raw(), "2026-01-03T00:00:00Z");
        assert_eq!(r.insights.total_duration, Some(2 * 86_400_000_000_000));
        assert_eq!(r.insights.blocked_duration, Some(86_400_000_000_000));
        let pct = r.insights.blocked_percentage.expect("known");
        assert!((pct - 50.0).abs() < 1e-9, "{pct}");
    }

    #[test]
    fn go_time_orders_by_instant_not_by_text() {
        // The corpus this port was written against is entirely +07:00, which
        // hides this. A repository whose commits carry mixed offsets breaks any
        // comparison that reads the timestamp as text: 10:00+09:00 is 01:00Z
        // and therefore EARLIER than 02:00Z, while its text sorts later.
        let early = t("2026-01-02T10:00:00+09:00"); // 01:00Z
        let late = t("2026-01-02T02:00:00Z");
        assert!(early.before(&late), "instant ordering");
        assert!(early < late, "Ord must agree with before()");
        assert!(late > early);
        assert_ne!(early, late);
        // Same instant written two ways must compare equal, as Go's Equal does.
        assert_eq!(early, t("2026-01-02T01:00:00Z"));
        // And the duration is the true elapsed time, not a text difference.
        assert_eq!(late.sub_nanos(&early), 3_600_000_000_000);
        // Serialization still preserves each commit's own offset.
        assert_eq!(early.raw(), "2026-01-02T10:00:00+09:00");
    }

    #[test]
    fn gaps_survive_a_mixed_offset_repository() {
        // The same trap reached through the public entry point: events ordered
        // by instant, not by text, so gap measurement still happens.
        let mut h = history("closed");
        h.events = vec![
            event("A-1", EventType::Created, "2026-01-02T10:00:00+09:00"),
            event("A-1", EventType::Claimed, "2026-01-02T02:00:00Z"),
            event("A-1", EventType::Closed, "2026-01-02T04:00:00Z"),
        ];
        let r = build_causality_chain_at(
            &h,
            None,
            &CausalityOptions::default(),
            &t("2026-01-03T00:00:00Z"),
        )
        .expect("history exists");
        assert_eq!(r.chain.events.len(), 3);
        // Instant order puts 01:00Z (the +09:00 creation) first, so the gaps
        // are one hour then two. Ordering by text would have produced none.
        let gaps: Vec<Option<i64>> = r.chain.events.iter().map(|e| e.duration_next).collect();
        assert_eq!(
            gaps,
            vec![Some(3_600_000_000_000), Some(7_200_000_000_000), None],
            "gap measurement must survive mixed offsets"
        );
        assert_eq!(r.insights.longest_gap, Some(7_200_000_000_000));
    }

    #[test]
    fn chain_total_time_is_null_when_the_duration_is_unknown() {
        // A chain whose first event is not a recorded creation has no known
        // start, so Go's MarshalJSON emits null. Printing the elapsed span
        // anyway would assert a number the evidence does not support.
        let mut h = history("open");
        h.events = vec![event("A-1", EventType::Modified, "2026-01-01T00:00:00Z")];
        let r = build_causality_chain_at(
            &h,
            None,
            &CausalityOptions::default(),
            &t("2026-06-01T00:00:00Z"),
        )
        .expect("history exists");
        assert!(!r.chain.duration_known);
        assert!(r.chain.total_time.is_none(), "{:?}", r.chain.total_time);
    }

    #[test]
    fn a_zero_length_window_reports_zero_percent_not_null() {
        // Created and closed in the same commit: the total is 0, so Go skips
        // the division but still emits the field, because its presence is gated
        // on the two "known" flags rather than on a non-zero total.
        let obs = vec![observation(
            "s1",
            "2026-01-01T00:00:00Z",
            absent(true),
            state("closed", true, DependencyState::Satisfied),
        )];
        let h = history("closed");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(obs)),
            &CausalityOptions::default(),
            &t("2026-06-01T00:00:00Z"),
        )
        .expect("history exists");
        assert_eq!(r.chain.end_time.raw(), "2026-01-01T00:00:00Z");
        assert_eq!(r.chain.total_time, Some(0));
        assert_eq!(r.insights.blocked_percentage, Some(0.0));
    }

    #[test]
    fn durations_serialize_as_nanosecond_integers() {
        // Go's ladder is coarse: anything from one hour up reports whole hours,
        // so 90 minutes is "1h", not "1m30s".
        assert_eq!(format_duration_short(59 * 60_000_000_000), "59m");
        // Exactly one hour is not "< 1 hour", so it rolls up to the hour rung.
        assert_eq!(format_duration_short(60 * 60_000_000_000), "1h");
        assert_eq!(format_duration_short(90 * 60_000_000_000), "1h");
        assert_eq!(format_duration_short(HOUR_NANOS), "1h");
        assert_eq!(format_duration_short(DAY_NANOS), "1d");
        assert_eq!(format_duration_short(7 * DAY_NANOS), "1w");
        assert_eq!(format_duration_short(30 * DAY_NANOS), "1mo");
        // Go truncates rather than rounds, at every rung.
        assert_eq!(format_duration_short(119 * 60_000_000_000), "1h");
        assert_eq!(format_duration_short(DAY_NANOS + HOUR_NANOS), "1d");
    }

    #[test]
    fn percent_truncates_toward_zero() {
        assert_eq!(format_percent(49.9), "49%");
        assert_eq!(format_percent(0.0), "0%");
    }

    #[test]
    fn causal_state_wait_requires_a_known_open_issue() {
        let closed = state("closed", true, DependencyState::Unsatisfied);
        assert_eq!(causal_state_wait(&closed), (false, false, true));

        let unknown = CausalState {
            issue: None,
            known: true,
            dependency_state: DependencyState::Satisfied,
            blockers: Vec::new(),
            reason: String::new(),
        };
        assert_eq!(causal_state_wait(&unknown), (false, false, false));

        let blocked = state("blocked", true, DependencyState::Satisfied);
        assert_eq!(causal_state_wait(&blocked), (true, false, true));

        let unknown_dep = state("open", true, DependencyState::Unknown);
        assert_eq!(causal_state_wait(&unknown_dep), (false, false, false));
    }

    #[test]
    fn constraint_comparison_tracks_the_gate_not_the_title() {
        let a = state("open", true, DependencyState::Unsatisfied);
        let mut b = a.clone();
        b.issue.as_mut().expect("issue").title = "renamed".into();
        assert!(
            same_causal_constraints(&a, &b),
            "title is not part of the gate"
        );

        let mut c = a.clone();
        c.blockers = vec!["B-1".into()];
        assert!(!same_causal_constraints(&a, &c));
    }

    #[test]
    fn inconsistent_clocks_are_reported_and_measure_nothing() {
        let obs = vec![
            observation(
                "s1",
                "2026-01-02T00:00:00Z",
                CausalState {
                    issue: None,
                    known: false,
                    dependency_state: DependencyState::Unknown,
                    blockers: Vec::new(),
                    reason: String::new(),
                },
                state("open", true, DependencyState::Satisfied),
            ),
            // Author time goes backwards even though Git order does not.
            observation(
                "s2",
                "2026-01-01T00:00:00Z",
                state("open", true, DependencyState::Satisfied),
                state("in_progress", true, DependencyState::Satisfied),
            ),
        ];
        let h = history("in_progress");
        let r = build_causality_chain_at(
            &h,
            Some(&history_of(obs)),
            &CausalityOptions::default(),
            &t("2026-01-03T00:00:00Z"),
        )
        .expect("history exists");
        assert_eq!(r.insights.coverage, "inconsistent");
        assert!(!r.insights.duration_known);
        assert!(r.insights.total_duration.is_none());
        assert!(r
            .insights
            .limitations
            .iter()
            .any(|l| l.contains("author or committer clocks")));
    }

    #[test]
    fn insights_field_order_matches_go() {
        // Go's custom MarshalJSON places the seven shadowed durations last.
        let h = history("open");
        let r = build_causality_chain_at(
            &h,
            None,
            &CausalityOptions::default(),
            &t("2026-01-02T00:00:00Z"),
        )
        .expect("history exists");
        let v = serde_json::to_value(&r.insights).expect("serializes");
        let keys: Vec<&str> = v
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.first(), Some(&"blocked_periods"));
        assert_eq!(keys.last(), Some(&"critical_path_duration"));
        let total = keys
            .iter()
            .position(|k| *k == "total_duration")
            .expect("present");
        let duration_known = keys
            .iter()
            .position(|k| *k == "duration_known")
            .expect("present");
        assert!(total > duration_known, "shadowed durations come last");
    }
}
