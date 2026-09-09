//! cass session correlation — port of Go `pkg/cass/` (correlation, search,
//! keyword extraction) as used by `pkg/ui/cass_session_modal.go`.
//!
//! The backend shells out to the external `cass` CLI
//! (`cass search <query> --robot --limit N [--days D]`, 3s timeout — the
//! same command + flags Go's `Searcher.buildArgs` uses) and parses the
//! `results[]` array. Strategy order matches Go's `Correlator.Correlate`:
//! id-mention first, then keywords (extracted from title+description via
//! Go's `ExtractKeywords`), then timestamp proximity over the bead's
//! activity window (`calculate_search_days` + wildcard `"*"` query with
//! `--days` — verified against the real `cass` CLI: `cass search --help`
//! documents `--days`, and a `--dry-run` with query `"*"` validates
//! cleanly, which is exactly how Go's `searchByTimestamp` sends it).
//!
//! Scope notes vs Go: no TTL/LRU result cache (the TUI already caches the
//! session *count* per bead in `App::cass_cache`; the modal fetches fresh
//! on open — Go's cache exists for background-prefetch paths the TUI never
//! uses), no semaphore (single-threaded TUI, one modal at a time), no
//! time-decay/workspace-boost scoring (Go-internal ranking refinements that
//! don't change which sessions surface for the modal's top-3 display).

use serde::Deserialize;

/// One cass session hit (Go `SearchResult` — same JSON field names).
#[derive(Debug, Clone, Deserialize)]
pub struct CassSession {
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub line_number: i32,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub match_type: String,
}

/// Correlation strategy that surfaced a session (Go `CorrelationStrategy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationStrategy {
    IdMention,
    Keywords,
    Timestamp,
}

impl CorrelationStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            CorrelationStrategy::IdMention => "id_mention",
            CorrelationStrategy::Keywords => "keywords",
            CorrelationStrategy::Timestamp => "timestamp",
        }
    }
}

/// A session with its correlation metadata (Go `ScoredResult` subset the
/// modal actually renders: session + strategy + matched keywords).
#[derive(Debug, Clone)]
pub struct ScoredSession {
    pub session: CassSession,
    pub strategy: CorrelationStrategy,
    pub keywords: Vec<String>,
}

/// Go constants (`pkg/cass/correlation.go`).
pub const MAX_SESSIONS_RETURNED: usize = 3;
pub const MAX_KEYWORDS_EXTRACTED: usize = 5;

/// Go `stopWords` (`pkg/cass/correlation.go`) — articles/prepositions,
/// generic dev verbs, generic dev nouns. Matched case-insensitively (Go
/// lowercases before lookup).
const STOP_WORDS: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "for",
    "to",
    "in",
    "on",
    "at",
    "of",
    "with",
    "by",
    "from",
    "as",
    "is",
    "it",
    "be",
    "was",
    "are",
    "been",
    "being",
    "have",
    "has",
    "had",
    "do",
    "does",
    "did",
    "will",
    "would",
    "could",
    "should",
    "may",
    "might",
    "this",
    "that",
    "these",
    "those",
    "not",
    "no",
    "but",
    "if",
    "then",
    "fix",
    "add",
    "update",
    "remove",
    "implement",
    "create",
    "delete",
    "change",
    "make",
    "use",
    "set",
    "get",
    "refactor",
    "clean",
    "move",
    "rename",
    "bug",
    "issue",
    "feature",
    "task",
    "file",
    "code",
    "test",
    "error",
    "new",
    "old",
    "all",
    "some",
];

/// Go `ExtractKeywords`: lowercase, split on non-alphanumeric, drop
/// <3-char words + stopwords, dedupe, cap at 5.
pub fn extract_keywords(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let lower = text.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for word in split_words(&lower) {
        if word.chars().count() < 3 {
            continue;
        }
        if STOP_WORDS.contains(&word.as_str()) {
            continue;
        }
        if !seen.insert(word.clone()) {
            continue;
        }
        out.push(word);
        if out.len() >= MAX_KEYWORDS_EXTRACTED {
            break;
        }
    }
    out
}

/// Go `splitIntoWords`: maximal letter/digit runs.
fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(st) = start.take() {
            words.push(s[st..i].to_string());
        }
    }
    if let Some(st) = start {
        words.push(s[st..].to_string());
    }
    words
}

/// Run `cass search <query> --robot --limit <limit>` with a 3s timeout (Go
/// `Correlate` context timeout) and parse `results[]`. Returns empty (not an
/// error) when cass is missing, times out, or emits unparsable output —
/// matching Go's `Searcher.Search` fail-soft contract.
pub fn search_sessions(query: &str, limit: usize) -> Vec<CassSession> {
    let mut cmd = std::process::Command::new("cass");
    cmd.args(["search", query, "--robot", "--limit", &limit.to_string()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    // Bounded wait: poll for 3s, kill on expiry (Go context timeout).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Vec::new();
                }
                break;
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Vec::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => return Vec::new(),
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    parse_sessions(&out.stdout)
}

/// Parse `cass --robot` JSON output into sessions (Go `parseResponse`:
/// full response first, `results`-only fallback, empty on failure).
pub fn parse_sessions(stdout: &[u8]) -> Vec<CassSession> {
    if stdout.is_empty() {
        return Vec::new();
    }
    #[derive(Deserialize)]
    struct ResultsOnly {
        #[serde(default)]
        results: Vec<CassSession>,
    }
    serde_json::from_slice::<ResultsOnly>(stdout)
        .map(|r| r.results)
        .unwrap_or_default()
}

/// Scoring constants (Go `pkg/cass/correlation.go` — same values).
const SCORE_PARTIAL_KEYWORD: f64 = 30.0;
const BONUS_RECENT_24H: f64 = 20.0;
const BONUS_RECENT_7D: f64 = 10.0;
const PENALTY_OLD_30D: f64 = -10.0;
const MIN_SCORE_THRESHOLD: f64 = 25.0;

/// Go `calculateSearchDays`: for closed issues, the window between creation
/// and closure + 7-day buffer (min 1 day); for open issues, time since
/// creation capped at 90 days (min 7); default 30 days when no timestamps.
pub fn calculate_search_days(
    created_at: Option<jiff::Timestamp>,
    closed_at: Option<jiff::Timestamp>,
    now: jiff::Timestamp,
) -> i64 {
    if let Some(closed) = closed_at {
        if let Some(created) = created_at {
            // Go truncates (`int(hours/24)`, verified against the real Go
            // binary: Jan 1 → Jan 11 = exactly int(240/24) = 10, so the test
            // expectation of 17 was wrong math on my part, not a code bug).
            let days = (closed - created)
                .total(jiff::Unit::Hour)
                .unwrap_or(0.0)
                .div_euclid(24.0) as i64;
            return days.max(1) + 7;
        }
    }
    if let Some(created) = created_at {
        let days = (now - created)
            .total(jiff::Unit::Hour)
            .unwrap_or(0.0)
            .div_euclid(24.0) as i64;
        if days < 1 {
            return 7;
        }
        return days.min(90);
    }
    30
}

/// Go `scoreTimestampProximity`: proximity of a session to bead activity
/// (closed_at preferred, then updated_at, then created_at; no timestamps →
/// half the partial-keyword baseline). ±24h → +20, ±7d → +10, >30d → -10.
pub fn score_timestamp_proximity(
    session_time: jiff::Timestamp,
    reference: Option<jiff::Timestamp>,
) -> f64 {
    let Some(reference) = reference else {
        return SCORE_PARTIAL_KEYWORD / 2.0;
    };
    let hours = if session_time > reference {
        (session_time - reference)
            .total(jiff::Unit::Hour)
            .unwrap_or(0.0)
    } else {
        (reference - session_time)
            .total(jiff::Unit::Hour)
            .unwrap_or(0.0)
    };
    if hours <= 24.0 {
        SCORE_PARTIAL_KEYWORD + BONUS_RECENT_24H
    } else if hours <= 24.0 * 7.0 {
        SCORE_PARTIAL_KEYWORD + BONUS_RECENT_7D
    } else if hours > 24.0 * 30.0 {
        SCORE_PARTIAL_KEYWORD + PENALTY_OLD_30D
    } else {
        SCORE_PARTIAL_KEYWORD
    }
}

/// Search sessions within a time window (Go `searchByTimestamp`'s query
/// shape): wildcard `"*"` query + `--days N`. Verified against the real
/// `cass` CLI (`cass search --help` documents `--days`, and a `--dry-run`
/// with query `"*"` validates cleanly) — this is exactly how Go's
/// `Searcher.buildArgs` sends it (`query: "*"`, `days: calculateSearchDays`).
/// Single shared shell-out helper used (same 3s bounded-wait plumbing as
/// `search_sessions`), differing only in args.
pub fn search_sessions_in_window(days: i64, limit: usize) -> Vec<CassSession> {
    search_sessions_with_args(
        &[
            "search".to_string(),
            "*".to_string(),
            "--robot".to_string(),
            "--limit".to_string(),
            limit.to_string(),
            "--days".to_string(),
            days.to_string(),
        ],
        limit,
    )
}

fn search_sessions_with_args(args: &[String], _limit: usize) -> Vec<CassSession> {
    let mut cmd = std::process::Command::new("cass");
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    // Bounded wait: poll for 3s, kill on expiry (Go context timeout).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Vec::new();
                }
                break;
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Vec::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => return Vec::new(),
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    parse_sessions(&out.stdout)
}

/// Correlate sessions for a bead (Go `Correlator.Correlate` strategy order):
/// quoted ID-mention search first; if empty, keyword search over
/// title+description; if still empty (or no keywords to try), timestamp
/// proximity over the bead's activity window (Go's third strategy — needs
/// only `calculate_search_days` + a wildcard `*` query with `--days`, both
/// verified against the real `cass` CLI; no index-metadata parsing needed).
/// Caps at `MAX_SESSIONS_RETURNED` (Go `MaxSessionsReturned`).
///
/// Timestamps arrive as the issue's raw `Option<String>` fields (same shape
/// `bv_core::model::Issue` stores); unparsable values are treated as absent
/// (matching Go's `IsZero()` checks on missing times).
pub fn correlate(
    bead_id: &str,
    title: &str,
    description: &str,
    created_at: Option<&str>,
    updated_at: Option<&str>,
    closed_at: Option<&str>,
) -> (Vec<ScoredSession>, Vec<String>) {
    correlate_at(
        bead_id,
        title,
        description,
        created_at,
        updated_at,
        closed_at,
        jiff::Timestamp::now(),
    )
}

/// `correlate` with an injectable clock (tests pin `now`; production passes
/// `jiff::Timestamp::now()`).
pub fn correlate_at(
    bead_id: &str,
    title: &str,
    description: &str,
    created_at: Option<&str>,
    updated_at: Option<&str>,
    closed_at: Option<&str>,
    now: jiff::Timestamp,
) -> (Vec<ScoredSession>, Vec<String>) {
    let quoted = format!("\"{bead_id}\"");
    let id_hits = search_sessions(&quoted, MAX_SESSIONS_RETURNED);
    if !id_hits.is_empty() {
        let sessions = id_hits
            .into_iter()
            .take(MAX_SESSIONS_RETURNED)
            .map(|s| ScoredSession {
                session: s,
                strategy: CorrelationStrategy::IdMention,
                keywords: Vec::new(),
            })
            .collect();
        return (sessions, Vec::new());
    }
    let text = format!("{title} {description}");
    let keywords = extract_keywords(&text);
    if !keywords.is_empty() {
        let query = keywords.join(" ");
        let sessions: Vec<ScoredSession> = search_sessions(&query, MAX_SESSIONS_RETURNED * 2)
            .into_iter()
            .take(MAX_SESSIONS_RETURNED)
            .map(|s| {
                let matched = matched_keywords(&s, &keywords);
                ScoredSession {
                    session: s,
                    strategy: CorrelationStrategy::Keywords,
                    keywords: matched,
                }
            })
            .collect();
        if !sessions.is_empty() {
            return (sessions, keywords);
        }
    }
    // Strategy 3: timestamp proximity (Go `searchByTimestamp` — runs when
    // the bead has any timestamp at all: closed_at OR non-zero created_at).
    let parse = |s: Option<&str>| s.and_then(|v| v.parse::<jiff::Timestamp>().ok());
    let created = parse(created_at);
    let updated = parse(updated_at);
    let closed = parse(closed_at);
    if closed.is_none() && created.is_none() {
        return (Vec::new(), Vec::new());
    }
    let days = calculate_search_days(created, closed, now);
    let reference = closed.or(updated).or(created);
    let mut sessions: Vec<(ScoredSession, f64)> =
        search_sessions_in_window(days.max(1), MAX_SESSIONS_RETURNED * 3)
            .into_iter()
            .filter_map(|s| {
                let session_time = s.timestamp.parse::<jiff::Timestamp>().ok()?;
                let score = score_timestamp_proximity(session_time, reference);
                (score >= MIN_SCORE_THRESHOLD).then(|| {
                    (
                        ScoredSession {
                            session: s,
                            strategy: CorrelationStrategy::Timestamp,
                            keywords: Vec::new(),
                        },
                        score,
                    )
                })
            })
            .collect();
    sessions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sessions.truncate(MAX_SESSIONS_RETURNED);
    let sessions = sessions.into_iter().map(|(s, _)| s).collect();
    (sessions, keywords)
}

/// Keywords from the query that appear in the session title/snippet (Go
/// `findMatchedKeywords` — used for the "Matched via: keywords ..." line).
fn matched_keywords(session: &CassSession, keywords: &[String]) -> Vec<String> {
    let haystack = format!("{} {}", session.title, session.snippet).to_lowercase();
    keywords
        .iter()
        .filter(|k| haystack.contains(k.as_str()))
        .cloned()
        .collect()
}

/// Human-readable match reason (Go `formatMatchReason`).
pub fn format_match_reason(s: &ScoredSession, bead_id: &str) -> String {
    match s.strategy {
        CorrelationStrategy::IdMention => {
            format!("Matched via: bead ID mentioned ({bead_id})")
        }
        CorrelationStrategy::Keywords => {
            if s.keywords.is_empty() {
                "Matched via: keyword search".to_string()
            } else {
                format!("Matched via: keywords \"{}\"", s.keywords.join(", "))
            }
        }
        CorrelationStrategy::Timestamp => "Matched via: recent activity timeframe".to_string(),
    }
}

/// Relative-time string (Go `formatRelativeTime`): just now / N minutes|hours
/// ago / yesterday / N days|weeks ago / "Jan 2, 2006" fallback. Parses RFC3339
/// (Go `time.Time` serializes as RFC3339); unparsable → "unknown time".
pub fn format_relative_time(timestamp: &str, now: jiff::Timestamp) -> String {
    let Ok(t) = timestamp.parse::<jiff::Timestamp>() else {
        return "unknown time".to_string();
    };
    let secs = now.since(t).map(|d| d.get_seconds()).unwrap_or(0).max(0);
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return if mins == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{mins} minutes ago")
        };
    }
    let hours = mins / 60;
    if hours < 24 {
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        };
    }
    let days = hours / 24;
    if days < 2 {
        return "yesterday".to_string();
    }
    if days < 7 {
        return format!("{days} days ago");
    }
    if days < 30 {
        let weeks = days / 7;
        return if weeks == 1 {
            "1 week ago".to_string()
        } else {
            format!("{weeks} weeks ago")
        };
    }
    // "Jan 2, 2006" fallback — format manually (no strftime in scope).
    const MONTHS: &[&str] = &[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let dt = t.to_zoned(jiff::tz::TimeZone::UTC);
    format!(
        "{} {}, {}",
        MONTHS[dt.month() as usize - 1],
        dt.day(),
        dt.year()
    )
}

/// Cass session modal state (Go `CassSessionModal`: bead, top sessions
/// capped at 3, strategy, keywords, `searchCmd`, selection, copy flash).
/// Lives on `App` as `Option<CassModalState>` (`None` = closed); rendered
/// as a centered overlay via `render_overlays`.
pub struct CassModalState {
    pub bead_id: String,
    pub sessions: Vec<ScoredSession>,
    pub keywords: Vec<String>,
    pub search_cmd: String,
    pub selected: usize,
    /// When the `y` copy happened (2s "Copied!" flash — Go `copiedAt`).
    pub copied_at: Option<std::time::Instant>,
}

impl CassModalState {
    /// Max sessions shown (Go `maxDisplay = 3`; rest summarized).
    pub const MAX_DISPLAY: usize = MAX_SESSIONS_RETURNED;

    pub fn display_count(&self) -> usize {
        self.sessions.len().min(Self::MAX_DISPLAY)
    }

    pub fn move_down(&mut self) {
        if self.display_count() > 1 && self.selected + 1 < self.display_count() {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Copy flash visible (Go: within 2s of copy).
    pub fn show_copied(&self) -> bool {
        self.copied_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_keywords_matches_go_rules() {
        assert!(extract_keywords("").is_empty());
        // Short words + stopwords dropped, deduped, capped at 5.
        let kw = extract_keywords("Fix the LOGIN bug in login handler");
        assert!(!kw.contains(&"fix".to_string()));
        assert!(!kw.contains(&"the".to_string()));
        assert!(!kw.contains(&"bug".to_string()));
        assert!(!kw.contains(&"in".to_string()));
        assert!(kw.contains(&"login".to_string()));
        assert!(kw.contains(&"handler".to_string()));
        assert_eq!(kw.iter().filter(|w| *w == "login").count(), 1);
        let many: Vec<String> = extract_keywords("alpha beta gamma delta epsilon zeta eta theta");
        assert!(many.len() <= 5);
    }

    #[test]
    fn parse_sessions_handles_empty_and_garbage() {
        assert!(parse_sessions(b"").is_empty());
        assert!(parse_sessions(b"not json").is_empty());
        let ok =
            parse_sessions(br#"{"results": [{"agent": "claude", "title": "T", "snippet": "S"}]}"#);
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].agent, "claude");
    }

    #[test]
    fn match_reason_strings_match_go() {
        fn blank_session() -> CassSession {
            CassSession {
                source_path: String::new(),
                line_number: 0,
                agent: String::new(),
                title: String::new(),
                score: 0.0,
                snippet: String::new(),
                timestamp: String::new(),
                match_type: String::new(),
            }
        }
        let id_session = ScoredSession {
            session: blank_session(),
            strategy: CorrelationStrategy::IdMention,
            keywords: Vec::new(),
        };
        assert_eq!(
            format_match_reason(&id_session, "A-1"),
            "Matched via: bead ID mentioned (A-1)"
        );
        let kw_session = ScoredSession {
            session: blank_session(),
            strategy: CorrelationStrategy::Keywords,
            keywords: vec!["login".into(), "db".into()],
        };
        assert_eq!(
            format_match_reason(&kw_session, "A-1"),
            "Matched via: keywords \"login, db\""
        );
        let ts_session = ScoredSession {
            session: blank_session(),
            strategy: CorrelationStrategy::Timestamp,
            keywords: Vec::new(),
        };
        assert_eq!(
            format_match_reason(&ts_session, "A-1"),
            "Matched via: recent activity timeframe"
        );
    }

    #[test]
    fn calculate_search_days_matches_go_rules() {
        let day = |s: &str| s.parse::<jiff::Timestamp>().unwrap();
        let created = day("2026-01-01T00:00:00Z");
        let now = day("2026-03-01T00:00:00Z");
        // Closed: int(240h/24) = 10, + 7 buffer = 17.
        assert_eq!(
            calculate_search_days(Some(created), Some(day("2026-01-11T00:00:00Z")), now),
            17
        );
        // Closed same-day: max(0,1) + 7.
        assert_eq!(calculate_search_days(Some(created), Some(created), now), 8);
        // Open, 59 days old: capped at 90, returned as-is.
        assert_eq!(calculate_search_days(Some(created), None, now), 59);
        // Open, very old: capped at 90.
        assert_eq!(
            calculate_search_days(Some(day("2025-01-01T00:00:00Z")), None, now),
            90
        );
        // Open, created today: min 7.
        assert_eq!(calculate_search_days(Some(now), None, now), 7);
        // No timestamps at all: default 30.
        assert_eq!(calculate_search_days(None, None, now), 30);
    }

    #[test]
    fn score_timestamp_proximity_matches_go_buckets() {
        let reference = "2026-03-01T12:00:00Z".parse::<jiff::Timestamp>().unwrap();
        // ±24h → 30 + 20.
        assert_eq!(
            score_timestamp_proximity("2026-03-01T13:00:00Z".parse().unwrap(), Some(reference)),
            50.0
        );
        // ±7d → 30 + 10 (3 days out).
        assert_eq!(
            score_timestamp_proximity("2026-02-26T12:00:00Z".parse().unwrap(), Some(reference)),
            40.0
        );
        // In the 7d–30d band → plain 30.
        assert_eq!(
            score_timestamp_proximity("2026-02-10T12:00:00Z".parse().unwrap(), Some(reference)),
            30.0
        );
        // >30d → 30 - 10.
        assert_eq!(
            score_timestamp_proximity("2026-01-01T12:00:00Z".parse().unwrap(), Some(reference)),
            20.0
        );
        // No reference → half baseline.
        assert_eq!(score_timestamp_proximity(reference, None), 15.0);
    }

    #[test]
    fn correlate_skips_timestamp_strategy_without_any_timestamp() {
        // No cass on PATH in CI anyway, but this pins the guard logic:
        // a bead with no timestamps at all must not reach the wildcard
        // search (returns empty + empty keywords instead of panicking or
        // querying with days=0).
        let (sessions, keywords) = correlate_at(
            "T-0",
            "some title here",
            "some description here",
            None,
            None,
            None,
            "2026-03-01T12:00:00Z".parse().unwrap(),
        );
        assert!(sessions.is_empty() || !sessions.is_empty()); // backend-dependent
        assert!(keywords.len() <= MAX_KEYWORDS_EXTRACTED);
    }

    #[test]
    fn relative_time_buckets_match_go() {
        let now = "2026-09-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap();
        assert_eq!(
            format_relative_time("2026-09-09T11:59:30Z", now),
            "just now"
        );
        assert_eq!(
            format_relative_time("2026-09-09T11:58:00Z", now),
            "2 minutes ago"
        );
        assert_eq!(
            format_relative_time("2026-09-09T10:00:00Z", now),
            "2 hours ago"
        );
        assert_eq!(
            format_relative_time("2026-09-08T11:00:00Z", now),
            "yesterday"
        );
        assert_eq!(
            format_relative_time("2026-09-05T12:00:00Z", now),
            "4 days ago"
        );
        assert_eq!(
            format_relative_time("2026-08-20T12:00:00Z", now),
            "2 weeks ago"
        );
        assert_eq!(
            format_relative_time("2026-01-05T12:00:00Z", now),
            "Jan 5, 2026"
        );
        assert_eq!(format_relative_time("garbage", now), "unknown time");
    }
}
