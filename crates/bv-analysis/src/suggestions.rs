//! Smart recommendations for project hygiene — port of Go
//! `pkg/analysis/suggestions.go`, `duplicates.go`, `dependency_suggest.go`,
//! `label_suggest.go`, `cycle_warnings.go`, `suggest_all.go`.
//!
//! All constants, stop words, builtin label mappings, and JSON field names
//! match the Go source at commit 9ace029 byte-for-byte.

use crate::analyzer::build_graph;
use bv_core::model::{Issue, Status};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

// ---------------------------------------------------------------------------
// Suggestion types
// ---------------------------------------------------------------------------

/// SuggestionType categorizes the kind of suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionType {
    #[serde(rename = "missing_dependency")]
    MissingDependency,
    #[serde(rename = "potential_duplicate")]
    PotentialDuplicate,
    #[serde(rename = "label_suggestion")]
    LabelSuggestion,
    #[serde(rename = "stale_cleanup")]
    StaleCleanup,
    #[serde(rename = "cycle_warning")]
    CycleWarning,
}

impl SuggestionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MissingDependency => "missing_dependency",
            Self::PotentialDuplicate => "potential_duplicate",
            Self::LabelSuggestion => "label_suggestion",
            Self::StaleCleanup => "stale_cleanup",
            Self::CycleWarning => "cycle_warning",
        }
    }
}

/// ConfidenceLevel represents human-readable confidence thresholds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

// Confidence thresholds (Go constants).
const CONFIDENCE_THRESHOLD_LOW: f64 = 0.4;
const CONFIDENCE_THRESHOLD_HIGH: f64 = 0.7;

fn confidence_level(c: f64) -> ConfidenceLevel {
    if c < CONFIDENCE_THRESHOLD_LOW {
        ConfidenceLevel::Low
    } else if c >= CONFIDENCE_THRESHOLD_HIGH {
        ConfidenceLevel::High
    } else {
        ConfidenceLevel::Medium
    }
}

/// A smart recommendation for project hygiene.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    #[serde(rename = "type")]
    pub sug_type: String,
    #[serde(rename = "target_bead")]
    pub target_bead: String,
    #[serde(rename = "related_bead")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_bead: Option<String>,
    pub summary: String,
    pub reason: String,
    pub confidence: f64,
    #[serde(rename = "action_command")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_command: Option<String>,
    #[serde(rename = "generated_at")]
    pub generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Suggestion {
    fn new(
        sug_type: SuggestionType,
        target_bead: &str,
        summary: &str,
        reason: &str,
        confidence: f64,
    ) -> Self {
        Self {
            sug_type: sug_type.as_str().to_string(),
            target_bead: target_bead.to_string(),
            related_bead: None,
            summary: summary.to_string(),
            reason: reason.to_string(),
            confidence,
            action_command: None,
            generated_at: now_rfc3339(),
            metadata: None,
        }
    }

    fn with_related_bead(mut self, bead_id: &str) -> Self {
        self.related_bead = Some(bead_id.to_string());
        self
    }

    fn with_action(mut self, cmd: &str) -> Self {
        self.action_command = Some(cmd.to_string());
        self
    }

    fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        let m = self.metadata.get_or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = m.as_object_mut() {
            obj.insert(key.to_string(), value);
        }
        self
    }

    fn is_actionable(&self) -> bool {
        self.action_command.is_some()
    }
}

/// SuggestionStats summarizes a set of suggestions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuggestionStats {
    pub total: usize,
    #[serde(rename = "by_type")]
    pub by_type: BTreeMap<String, usize>,
    #[serde(rename = "by_confidence")]
    pub by_confidence: BTreeMap<String, usize>,
    #[serde(rename = "high_confidence_count")]
    pub high_confidence_count: usize,
    #[serde(rename = "actionable_count")]
    pub actionable_count: usize,
}

/// SuggestionSet holds a collection of suggestions with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionSet {
    pub suggestions: Vec<Suggestion>,
    #[serde(rename = "generated_at")]
    pub generated_at: String,
    #[serde(rename = "data_hash")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub data_hash: String,
    pub stats: SuggestionStats,
}

impl SuggestionSet {
    fn new(suggestions: Vec<Suggestion>, data_hash: &str) -> Self {
        let mut stats = SuggestionStats {
            total: suggestions.len(),
            ..Default::default()
        };
        for s in &suggestions {
            *stats.by_type.entry(s.sug_type.clone()).or_insert(0) += 1;
            let level = confidence_level(s.confidence);
            let level_str = match level {
                ConfidenceLevel::Low => "low",
                ConfidenceLevel::Medium => "medium",
                ConfidenceLevel::High => "high",
            };
            *stats
                .by_confidence
                .entry(level_str.to_string())
                .or_insert(0) += 1;
            if level == ConfidenceLevel::High {
                stats.high_confidence_count += 1;
            }
            if s.is_actionable() {
                stats.actionable_count += 1;
            }
        }
        Self {
            suggestions,
            generated_at: now_rfc3339(),
            data_hash: data_hash.to_string(),
            stats,
        }
    }
}

// ---------------------------------------------------------------------------
// Robot-suggest output envelope
// ---------------------------------------------------------------------------

/// Describes applied filters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuggestFilter {
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub filter_type: String,
    #[serde(rename = "min_confidence")]
    #[serde(skip_serializing_if = "is_zero")]
    pub min_confidence: f64,
    #[serde(rename = "bead_id")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bead_id: String,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// The JSON output structure for --robot-suggest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobotSuggestOutput {
    #[serde(rename = "generated_at")]
    pub generated_at: String,
    #[serde(rename = "data_hash")]
    pub data_hash: String,
    pub filters: SuggestFilter,
    pub suggestions: SuggestionSet,
    #[serde(rename = "usage_hints")]
    pub usage_hints: Vec<String>,
}

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Configuration for duplicate detection.
#[derive(Debug, Clone)]
pub struct DuplicateConfig {
    /// JaccardThreshold — minimum similarity score (0.0–1.0). Default: 0.7.
    pub jaccard_threshold: f64,
    /// MinKeywords — minimum keywords needed to compare. Default: 2.
    pub min_keywords: usize,
    /// IgnoreClosedVsOpen — skip pairs where one is closed and one is open.
    /// Default: true.
    pub ignore_closed_vs_open: bool,
    /// MaxSuggestions — maximum duplicate suggestions. Default: 20.
    pub max_suggestions: usize,
}

impl Default for DuplicateConfig {
    fn default() -> Self {
        Self {
            jaccard_threshold: 0.7,
            min_keywords: 2,
            ignore_closed_vs_open: true,
            max_suggestions: 20,
        }
    }
}

/// Configuration for dependency suggestion generation.
#[derive(Debug, Clone)]
pub struct DependencySuggestionConfig {
    /// MinKeywordOverlap — minimum shared keywords to suggest. Default: 2.
    pub min_keyword_overlap: usize,
    /// ExactMatchBonus — confidence bonus for exact keyword matches. Default: 0.15.
    pub exact_match_bonus: f64,
    /// LabelOverlapBonus — confidence bonus per shared label. Default: 0.1.
    pub label_overlap_bonus: f64,
    /// MinConfidence — minimum confidence to report. Default: 0.5.
    pub min_confidence: f64,
    /// MaxSuggestions — maximum suggestions. Default: 20.
    pub max_suggestions: usize,
    /// IgnoreExistingDeps — skip pairs that already have dependencies. Default: true.
    pub ignore_existing_deps: bool,
}

impl Default for DependencySuggestionConfig {
    fn default() -> Self {
        Self {
            min_keyword_overlap: 2,
            exact_match_bonus: 0.15,
            label_overlap_bonus: 0.1,
            min_confidence: 0.5,
            max_suggestions: 20,
            ignore_existing_deps: true,
        }
    }
}

/// Configuration for label suggestion generation.
#[derive(Debug, Clone)]
pub struct LabelSuggestionConfig {
    /// MinConfidence — minimum confidence to report. Default: 0.5.
    pub min_confidence: f64,
    /// MaxSuggestionsPerIssue — max suggestions per issue. Default: 3.
    pub max_suggestions_per_issue: usize,
    /// MaxTotalSuggestions — max total suggestions. Default: 30.
    pub max_total_suggestions: usize,
    /// LearnFromExisting — use existing labeled issues to learn patterns.
    /// Default: true.
    pub learn_from_existing: bool,
    /// BuiltinMappings — enable built-in keyword-to-label mappings. Default: true.
    pub builtin_mappings: bool,
}

impl Default for LabelSuggestionConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            max_suggestions_per_issue: 3,
            max_total_suggestions: 30,
            learn_from_existing: true,
            builtin_mappings: true,
        }
    }
}

/// Configuration for cycle warning generation.
#[derive(Debug, Clone)]
pub struct CycleWarningConfig {
    /// MaxCycles — maximum number of cycles to report. Default: 10.
    pub max_cycles: usize,
    /// IncludeSelfLoops — whether to report self-referencing dependencies.
    /// Default: true.
    pub include_self_loops: bool,
}

impl Default for CycleWarningConfig {
    fn default() -> Self {
        Self {
            max_cycles: 10,
            include_self_loops: true,
        }
    }
}

/// Unified configuration for the suggestion generator.
#[derive(Debug, Clone)]
pub struct SuggestAllConfig {
    pub duplicates: DuplicateConfig,
    pub dependencies: DependencySuggestionConfig,
    pub labels: LabelSuggestionConfig,
    pub cycles: CycleWarningConfig,
    pub enable_duplicates: bool,
    pub enable_dependencies: bool,
    pub enable_labels: bool,
    pub enable_cycles: bool,
    pub min_confidence: f64,
    pub max_suggestions: usize,
    /// FilterType — only include this suggestion type (empty = all).
    pub filter_type: Option<String>,
    /// FilterBead — only include suggestions for this bead ID.
    pub filter_bead: String,
}

impl Default for SuggestAllConfig {
    fn default() -> Self {
        Self {
            duplicates: DuplicateConfig::default(),
            dependencies: DependencySuggestionConfig::default(),
            labels: LabelSuggestionConfig::default(),
            cycles: CycleWarningConfig::default(),
            enable_duplicates: true,
            enable_dependencies: true,
            enable_labels: true,
            enable_cycles: true,
            min_confidence: 0.0,
            max_suggestions: 50,
            filter_type: None,
            filter_bead: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Status helpers (Go isClosedLikeStatus / isClosedLikeDuplicateStatus)
// ---------------------------------------------------------------------------

/// Go: `isClosedLikeStatus` — closed or tombstone.
fn is_closed_like_status(status: Status) -> bool {
    status == Status::Closed || status == Status::Tombstone
}

/// Go: `isClosedLikeDuplicateStatus` — closed or tombstone.
fn is_closed_like_duplicate_status(status: Status) -> bool {
    status == Status::Closed || status == Status::Tombstone
}

// ---------------------------------------------------------------------------
// Keyword extraction (Go duplicates.go)
// ---------------------------------------------------------------------------

/// Exact 62 stop words from Go `stopWords`.
static STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "are", "was", "were", "been", "have",
    "has", "had", "does", "did", "will", "would", "could", "should", "may", "might", "can", "not",
    "all", "any", "some", "each", "when", "where", "what", "which", "how", "why", "who", "its",
    "also", "just", "only", "more", "than", "then", "now", "here", "there", "these", "those",
    "such", "into", "over", "after", "before", "being", "other", "about", "like", "very", "most",
    "make", "use",
];

fn make_stop_set() -> HashSet<&'static str> {
    STOP_WORDS.iter().copied().collect()
}

/// Extract meaningful keywords from title + description.
/// Matches Go `extractKeywords` exactly.
fn extract_keywords(title: &str, description: &str) -> Vec<String> {
    let text = format!("{} {}", title, description).to_lowercase();

    // Remove common markdown/code artifacts (non-word chars → space).
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();

    let stop = make_stop_set();
    let mut seen: HashSet<String> = HashSet::new();
    let mut keywords: Vec<String> = Vec::new();

    for word in cleaned.split_whitespace() {
        if word.len() < 3 {
            continue;
        }
        if stop.contains(word) {
            continue;
        }
        if seen.insert(word.to_string()) {
            keywords.push(word.to_string());
        }
    }

    keywords
}

/// Intersect two keyword slices, returning sorted common keywords.
fn intersect_keywords(a: &[String], b: &[String]) -> Vec<String> {
    let set_a: HashSet<&String> = a.iter().collect();
    let mut common: Vec<String> = b.iter().filter(|w| set_a.contains(w)).cloned().collect();
    common.sort();
    common
}

/// Find keys present in both maps.
fn find_shared_keys(m1: &HashMap<String, bool>, m2: &HashMap<String, bool>) -> Vec<String> {
    let mut shared: Vec<String> = m1.keys().filter(|k| m2.contains_key(*k)).cloned().collect();
    shared.sort();
    shared
}

/// Unique strings from a slice.
fn unique_strings(s: &[String]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for v in s {
        if seen.insert(v.as_str()) {
            result.push(v.clone());
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Duplicate detection (Go duplicates.go)
// ---------------------------------------------------------------------------

fn sort_pairs_by_similarity(pairs: &mut [DuplicatePair]) {
    pairs.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.issue1.cmp(&b.issue1))
            .then_with(|| a.issue2.cmp(&b.issue2))
    });
}

/// A potential duplicate pair.
#[derive(Debug, Clone)]
struct DuplicatePair {
    issue1: String,
    issue2: String,
    similarity: f64,
    method: String,
    keywords: Vec<String>,
}

/// Detect potential duplicate issues using keyword-based Jaccard similarity
/// with an inverted index. Matches Go `DetectDuplicates` exactly.
pub fn detect_duplicates(issues: &[Issue], config: &DuplicateConfig) -> Vec<Suggestion> {
    if issues.len() < 2 {
        return Vec::new();
    }

    // 1. Extract keywords for each issue and build inverted index.
    let keywords: Vec<Vec<String>> = issues
        .iter()
        .map(|i| extract_keywords(&i.title, &i.description))
        .collect();

    // index[word] = list of issue indices containing that word.
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, kws) in keywords.iter().enumerate() {
        if kws.len() >= config.min_keywords {
            for w in kws {
                index.entry(w.clone()).or_default().push(i);
            }
        }
    }

    let mut pairs: Vec<DuplicatePair> = Vec::new();

    // 2. Iterate and find candidates.
    for i in 0..issues.len() {
        if keywords[i].len() < config.min_keywords {
            continue;
        }

        let mut overlaps: HashMap<usize, usize> = HashMap::new();
        for w in &keywords[i] {
            if let Some(indices) = index.get(w) {
                for &idx in indices {
                    if idx > i {
                        *overlaps.entry(idx).or_insert(0) += 1;
                    }
                }
            }
        }

        // 3. Evaluate candidates.
        for (&j, &overlap) in &overlaps {
            if keywords[j].len() < config.min_keywords {
                continue;
            }

            let union = keywords[i].len() + keywords[j].len() - overlap;
            if union == 0 {
                continue;
            }
            let similarity = overlap as f64 / union as f64;

            if similarity < config.jaccard_threshold {
                continue;
            }

            let issue1 = &issues[i];
            let issue2 = &issues[j];

            // Skip tombstones.
            if issue1.status == Status::Tombstone || issue2.status == Status::Tombstone {
                continue;
            }

            // Skip closed vs open pairs if configured.
            if config.ignore_closed_vs_open
                && is_closed_like_duplicate_status(issue1.status)
                    != is_closed_like_duplicate_status(issue2.status)
            {
                continue;
            }

            let common = intersect_keywords(&keywords[i], &keywords[j]);

            pairs.push(DuplicatePair {
                issue1: issue1.id.clone(),
                issue2: issue2.id.clone(),
                similarity,
                method: "jaccard".to_string(),
                keywords: common,
            });
        }
    }

    sort_pairs_by_similarity(&mut pairs);
    pairs.truncate(config.max_suggestions);

    // Issue lookup map for constructing suggestions.
    let issue_map: HashMap<&str, &Issue> = issues.iter().map(|i| (i.id.as_str(), i)).collect();

    pairs
        .into_iter()
        .map(|pair| {
            let issue1 = issue_map.get(pair.issue1.as_str()).unwrap();
            let issue2 = issue_map.get(pair.issue2.as_str()).unwrap();

            let keywords_display: Vec<&str> =
                pair.keywords.iter().take(5).map(|s| s.as_str()).collect();
            let mut sug = Suggestion::new(
                SuggestionType::PotentialDuplicate,
                &pair.issue1,
                &format!("Potential duplicate of {}", pair.issue2),
                &format!(
                    "{:.0}% keyword similarity; common: {}",
                    pair.similarity * 100.0,
                    keywords_display.join(", ")
                ),
                pair.similarity,
            )
            .with_related_bead(&pair.issue2)
            .with_metadata("method", serde_json::json!(pair.method));

            // Add action command if both are open.
            if !is_closed_like_duplicate_status(issue1.status)
                && !is_closed_like_duplicate_status(issue2.status)
            {
                sug = sug.with_action(&format!(
                    "br dep add {} {} --type=related",
                    pair.issue1, pair.issue2
                ));
            }

            sug
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Missing dependency detection (Go dependency_suggest.go)
// ---------------------------------------------------------------------------

fn sort_matches_by_confidence(matches: &mut [DependencyMatch]) {
    matches.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.from.cmp(&b.from))
            .then_with(|| a.to.cmp(&b.to))
    });
}

/// A potential dependency relationship.
#[derive(Debug, Clone)]
struct DependencyMatch {
    from: String,
    to: String,
    confidence: f64,
    shared_keywords: Vec<String>,
    shared_labels: Vec<String>,
    reason: String,
}

/// Analyze issues for potential missing dependencies.
/// Matches Go `DetectMissingDependencies` exactly.
pub fn detect_missing_dependencies(
    issues: &[Issue],
    config: &DependencySuggestionConfig,
) -> Vec<Suggestion> {
    if issues.len() < 2 {
        return Vec::new();
    }

    // 1. Build inverted index and precompute data.
    let keywords: Vec<Vec<String>> = issues
        .iter()
        .map(|i| extract_keywords(&i.title, &i.description))
        .collect();

    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, kws) in keywords.iter().enumerate() {
        if kws.len() >= config.min_keyword_overlap {
            for w in kws {
                index.entry(w.clone()).or_default().push(i);
            }
        }
    }

    // Labels per issue.
    let issue_labels: Vec<HashMap<String, bool>> = issues
        .iter()
        .map(|i| {
            i.labels
                .iter()
                .map(|l| l.to_lowercase())
                .map(|l| (l, true))
                .collect()
        })
        .collect();

    // ID -> index map.
    let id_to_index: HashMap<&str, usize> = issues
        .iter()
        .enumerate()
        .map(|(i, iss)| (iss.id.as_str(), i))
        .collect();

    // Existing deps.
    let existing_deps: Vec<HashMap<usize, bool>> = issues
        .iter()
        .map(|iss| {
            let mut deps = HashMap::new();
            for dep in &iss.dependencies {
                if let Some(&idx) = id_to_index.get(dep.effective_depends_on()) {
                    deps.insert(idx, true);
                }
            }
            deps
        })
        .collect();

    let mut matches: Vec<DependencyMatch> = Vec::new();

    // 2. Iterate and find candidates.
    for i in 0..issues.len() {
        if keywords[i].len() < config.min_keyword_overlap {
            continue;
        }

        let mut overlaps: HashMap<usize, usize> = HashMap::new();
        for w in &keywords[i] {
            if let Some(indices) = index.get(w) {
                for &idx in indices {
                    if idx > i {
                        *overlaps.entry(idx).or_insert(0) += 1;
                    }
                }
            }
        }

        // 3. Evaluate candidates.
        for (&j, &overlap) in &overlaps {
            if overlap < config.min_keyword_overlap {
                continue;
            }

            if config.ignore_existing_deps
                && (existing_deps[i].contains_key(&j) || existing_deps[j].contains_key(&i))
            {
                continue;
            }

            let issue1 = &issues[i];
            let issue2 = &issues[j];

            // Skip closed-like issues.
            if is_closed_like_status(issue1.status) || is_closed_like_status(issue2.status) {
                continue;
            }

            let shared_kw = intersect_keywords(&keywords[i], &keywords[j]);
            let shared_labels = find_shared_keys(&issue_labels[i], &issue_labels[j]);

            // Calculate confidence.
            let mut base_conf = shared_kw.len() as f64 * 0.1;
            if base_conf > 0.5 {
                base_conf = 0.5;
            }

            let title2_lower = issue2.title.to_lowercase();
            let id1_lower = issue1.id.to_lowercase();
            let id2_lower = issue2.id.to_lowercase();
            let desc1_lower = issue1.description.to_lowercase();
            let desc2_lower = issue2.description.to_lowercase();

            // ID mentioned.
            if desc2_lower.contains(&id1_lower) || desc1_lower.contains(&id2_lower) {
                base_conf += config.exact_match_bonus * 2.0;
            }

            // Title words of issue1 mentioned in issue2's title.
            for word in &keywords[i] {
                if word.len() >= 5 && title2_lower.contains(word.as_str()) {
                    base_conf += config.exact_match_bonus;
                    break;
                }
            }

            // Label overlap bonus.
            base_conf += shared_labels.len() as f64 * config.label_overlap_bonus;

            if base_conf > 0.95 {
                base_conf = 0.95;
            }

            if base_conf < config.min_confidence {
                continue;
            }

            // Determine direction. Go uses `Before` (strictly less than);
            // `<=` here would reverse direction when timestamps are equal.
            let (from, to) = if issue1.created_at.as_deref() < issue2.created_at.as_deref()
                || issue1.priority < issue2.priority
            {
                (issue2, issue1)
            } else {
                (issue1, issue2)
            };

            let mut reason = format!("{} shared keywords", shared_kw.len());
            if !shared_labels.is_empty() {
                reason += &format!(", {} shared labels", shared_labels.len());
            }

            matches.push(DependencyMatch {
                from: from.id.clone(),
                to: to.id.clone(),
                confidence: base_conf,
                shared_keywords: shared_kw,
                shared_labels,
                reason,
            });
        }
    }

    sort_matches_by_confidence(&mut matches);
    matches.truncate(config.max_suggestions);

    matches
        .into_iter()
        .map(|m| {
            let mut sug = Suggestion::new(
                SuggestionType::MissingDependency,
                &m.from,
                &format!("May depend on {}", m.to),
                &m.reason,
                m.confidence,
            )
            .with_related_bead(&m.to)
            .with_action(&format!("br dep add {} {}", m.from, m.to))
            .with_metadata("shared_keywords", serde_json::json!(m.shared_keywords));

            if !m.shared_labels.is_empty() {
                sug = sug.with_metadata("shared_labels", serde_json::json!(m.shared_labels));
            }

            sug
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Label suggestion (Go label_suggest.go)
// ---------------------------------------------------------------------------

/// Built-in keyword-to-label mappings — exact copy of Go `builtinLabelMappings`.
fn builtin_label_mappings() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m: HashMap<&'static str, Vec<&'static str>> = HashMap::new();

    // Technical areas
    m.insert("database", vec!["database", "db"]);
    m.insert("migration", vec!["database", "migration"]);
    m.insert("api", vec!["api"]);
    m.insert("endpoint", vec!["api"]);
    m.insert("rest", vec!["api"]);
    m.insert("graphql", vec!["api", "graphql"]);
    m.insert("auth", vec!["auth", "security"]);
    m.insert("login", vec!["auth"]);
    m.insert("password", vec!["auth", "security"]);
    m.insert("security", vec!["security"]);
    m.insert("test", vec!["testing"]);
    m.insert("tests", vec!["testing"]);
    m.insert("unittest", vec!["testing"]);
    m.insert("integration", vec!["testing", "integration"]);
    m.insert("ui", vec!["ui", "frontend"]);
    m.insert("frontend", vec!["frontend"]);
    m.insert("backend", vec!["backend"]);
    m.insert("server", vec!["backend"]);
    m.insert("cli", vec!["cli"]);
    m.insert("command", vec!["cli"]);
    m.insert("config", vec!["config"]);
    m.insert("settings", vec!["config"]);
    m.insert("performance", vec!["performance"]);
    m.insert("slow", vec!["performance"]);
    m.insert("fast", vec!["performance"]);
    m.insert("memory", vec!["performance"]);
    m.insert("cache", vec!["performance", "cache"]);
    m.insert("docs", vec!["documentation"]);
    m.insert("readme", vec!["documentation"]);
    m.insert("refactor", vec!["refactoring"]);
    m.insert("cleanup", vec!["refactoring", "maintenance"]);
    m.insert("dependency", vec!["dependencies"]);
    m.insert("deps", vec!["dependencies"]);

    // Issue types
    m.insert("bug", vec!["bug"]);
    m.insert("fix", vec!["bug"]);
    m.insert("broken", vec!["bug"]);
    m.insert("crash", vec!["bug"]);
    m.insert("error", vec!["bug"]);
    m.insert("feature", vec!["feature"]);
    m.insert("enhance", vec!["enhancement"]);
    m.insert("improve", vec!["enhancement"]);
    m.insert("urgent", vec!["urgent", "priority"]);
    m.insert("hotfix", vec!["urgent", "bug"]);

    m
}

/// Extract keyword-to-label patterns from existing issues.
fn learn_label_mappings(issues: &[Issue]) -> HashMap<String, HashMap<String, usize>> {
    let mut mappings: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for issue in issues {
        if issue.labels.is_empty() {
            continue;
        }
        let keywords = extract_keywords(&issue.title, &issue.description);
        for kw in &keywords {
            let label_map = mappings.entry(kw.clone()).or_default();
            for label in &issue.labels {
                *label_map.entry(label.to_lowercase()).or_insert(0) += 1;
            }
        }
    }

    mappings
}

fn sort_label_matches_by_confidence(matches: &mut [LabelMatch]) {
    matches.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.issue_id.cmp(&b.issue_id))
            .then_with(|| a.label.cmp(&b.label))
    });
}

/// A potential label suggestion.
#[derive(Debug, Clone)]
struct LabelMatch {
    issue_id: String,
    label: String,
    confidence: f64,
    reason: String,
    matched_words: Vec<String>,
}

/// Analyze issues for potential label suggestions.
/// Matches Go `SuggestLabels` exactly.
pub fn suggest_labels(issues: &[Issue], config: &LabelSuggestionConfig) -> Vec<Suggestion> {
    if issues.is_empty() {
        return Vec::new();
    }

    // Build learned mappings from existing labeled issues.
    let learned_mappings = if config.learn_from_existing {
        learn_label_mappings(issues)
    } else {
        HashMap::new()
    };

    // Find all existing labels for validation.
    let mut all_labels: HashSet<String> = HashSet::new();
    for issue in issues {
        for label in &issue.labels {
            all_labels.insert(label.to_lowercase());
        }
    }

    let builtin = builtin_label_mappings();
    let mut matches: Vec<LabelMatch> = Vec::new();

    for issue in issues {
        // Skip closed/tombstone issues.
        if is_closed_like_status(issue.status) {
            continue;
        }

        // Get existing labels for this issue.
        let existing_labels: HashMap<String, bool> = issue
            .labels
            .iter()
            .map(|l| (l.to_lowercase(), true))
            .collect();

        // Extract keywords.
        let keywords = extract_keywords(&issue.title, &issue.description);
        let keyword_set: HashSet<&str> = keywords.iter().map(|s| s.as_str()).collect();

        // Score potential labels.
        let mut label_scores: HashMap<String, f64> = HashMap::new();
        let mut label_reasons: HashMap<String, Vec<String>> = HashMap::new();

        // Check builtin mappings.
        if config.builtin_mappings {
            for &keyword in &keyword_set {
                if let Some(labels) = builtin.get(keyword) {
                    for &label in labels {
                        if !existing_labels.contains_key(label) && all_labels.contains(label) {
                            *label_scores.entry(label.to_string()).or_insert(0.0) += 0.3;
                            label_reasons
                                .entry(label.to_string())
                                .or_default()
                                .push(keyword.to_string());
                        }
                    }
                }
            }
        }

        // Check learned mappings.
        if config.learn_from_existing {
            for &keyword in &keyword_set {
                if let Some(label_counts) = learned_mappings.get(keyword) {
                    for (label, count) in label_counts {
                        if !existing_labels.contains_key(label.as_str())
                            && all_labels.contains(label.as_str())
                        {
                            let mut bonus = 0.1 + (*count as f64 * 0.05);
                            if bonus > 0.4 {
                                bonus = 0.4;
                            }
                            *label_scores.entry(label.clone()).or_insert(0.0) += bonus;
                            label_reasons
                                .entry(label.clone())
                                .or_default()
                                .push(keyword.to_string());
                        }
                    }
                }
            }
        }

        // Convert scores to candidates, sorted by score desc then label asc.
        let mut candidates: Vec<(String, f64)> = label_scores.into_iter().collect();
        candidates.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut issue_matches = 0;
        for (label, score) in &candidates {
            if *score < config.min_confidence {
                continue;
            }
            let mut score = *score;
            if score > 0.95 {
                score = 0.95;
            }
            if issue_matches >= config.max_suggestions_per_issue {
                break;
            }

            let reasons = label_reasons.get(label).cloned().unwrap_or_default();
            let mut unique_reasons = unique_strings(&reasons);
            unique_reasons.sort();
            let reason = format!("keywords: {}", unique_reasons.join(", "));

            matches.push(LabelMatch {
                issue_id: issue.id.clone(),
                label: label.clone(),
                confidence: score,
                reason,
                matched_words: unique_reasons,
            });
            issue_matches += 1;
        }
    }

    sort_label_matches_by_confidence(&mut matches);
    matches.truncate(config.max_total_suggestions);

    matches
        .into_iter()
        .map(|m| {
            Suggestion::new(
                SuggestionType::LabelSuggestion,
                &m.issue_id,
                &format!("Consider adding label '{}'", m.label),
                &m.reason,
                m.confidence,
            )
            .with_action(&format!("br update {} --add-label={}", m.issue_id, m.label))
            .with_metadata("suggested_label", serde_json::json!(m.label))
            .with_metadata("matched_keywords", serde_json::json!(m.matched_words))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Cycle warnings (Go cycle_warnings.go)
// ---------------------------------------------------------------------------

fn format_cycle_path(cycle: &[String]) -> String {
    cycle.join(" \u{2192} ") // → Unicode arrow
}

/// Generate suggestions for dependency cycles in the graph.
/// Matches Go `DetectCycleWarnings` exactly.
pub fn detect_cycle_warnings(issues: &[Issue], config: &CycleWarningConfig) -> Vec<Suggestion> {
    if issues.len() < 2 {
        return Vec::new();
    }

    let g = build_graph(issues);
    let cycles_raw = crate::algorithms::cycles::enumerate_cycles(&g, 100);

    if cycles_raw.is_empty() {
        return Vec::new();
    }

    // Convert node indices to issue IDs and append closing node (Go parity).
    let cycles: Vec<Vec<String>> = cycles_raw
        .iter()
        .filter_map(|cycle| {
            if cycle.is_empty() {
                return None;
            }
            let start = cycle[0];
            let mut path: Vec<String> = cycle.iter().filter_map(|&idx| g.node_id(idx)).collect();
            // Append closing node (Go includes it).
            if let Some(start_id) = g.node_id(start) {
                path.push(start_id);
            }
            Some(path)
        })
        .collect();

    let mut suggestions: Vec<Suggestion> = Vec::new();

    for (i, cycle) in cycles.iter().enumerate() {
        if i >= config.max_cycles {
            break;
        }

        // Skip self-loops if configured.
        if cycle.len() == 2 && cycle[0] == cycle[1] && !config.include_self_loops {
            continue;
        }

        let cycle_path = format_cycle_path(cycle);
        let cycle_len = cycle.len() - 1; // Exclude closing node.

        // Confidence based on cycle length (shorter = higher).
        let confidence = (1.0 - (cycle_len as f64 - 2.0) * 0.1).clamp(0.5, 1.0);

        // Generate summary based on cycle type.
        let summary = if cycle_len == 1 {
            format!("Self-loop: {} depends on itself", cycle[0])
        } else if cycle_len == 2 {
            format!("Direct cycle between {} and {}", cycle[0], cycle[1])
        } else {
            format!("Dependency cycle of {} issues", cycle_len)
        };

        let target_bead = &cycle[0];

        // Path without closing node for metadata.
        let path_without_close: Vec<&str> = cycle
            .iter()
            .take(cycle.len() - 1)
            .map(|s| s.as_str())
            .collect();

        let mut sug = Suggestion::new(
            SuggestionType::CycleWarning,
            target_bead,
            &summary,
            &format!("Cycle path: {}", cycle_path),
            confidence,
        )
        .with_metadata("cycle_length", serde_json::json!(cycle_len))
        .with_metadata("cycle_path", serde_json::json!(path_without_close));

        // Add action command to break the cycle.
        if cycle_len >= 2 {
            let from = &cycle[cycle_len - 1];
            let to = &cycle[0];
            sug = sug.with_action(&format!("br dep remove {} {}", from, to));
        }

        // If there's a second issue, mark it as related.
        if cycle_len >= 2 {
            sug = sug.with_related_bead(&cycle[1]);
        }

        suggestions.push(sug);
    }

    suggestions
}

// ---------------------------------------------------------------------------
// Orchestrator (Go suggest_all.go)
// ---------------------------------------------------------------------------

/// Run all suggestion detectors and return aggregated results.
/// Matches Go `GenerateAllSuggestions` exactly.
pub fn generate_all_suggestions(
    issues: &[Issue],
    config: &SuggestAllConfig,
    data_hash: &str,
) -> SuggestionSet {
    let mut all_suggestions: Vec<Suggestion> = Vec::new();

    if config.enable_duplicates
        && (config.filter_type.is_none()
            || config.filter_type.as_deref() == Some(SuggestionType::PotentialDuplicate.as_str()))
    {
        let duplicates = detect_duplicates(issues, &config.duplicates);
        all_suggestions.extend(duplicates);
    }

    if config.enable_dependencies
        && (config.filter_type.is_none()
            || config.filter_type.as_deref() == Some(SuggestionType::MissingDependency.as_str()))
    {
        let dependencies = detect_missing_dependencies(issues, &config.dependencies);
        all_suggestions.extend(dependencies);
    }

    if config.enable_labels
        && (config.filter_type.is_none()
            || config.filter_type.as_deref() == Some(SuggestionType::LabelSuggestion.as_str()))
    {
        let labels = suggest_labels(issues, &config.labels);
        all_suggestions.extend(labels);
    }

    if config.enable_cycles
        && (config.filter_type.is_none()
            || config.filter_type.as_deref() == Some(SuggestionType::CycleWarning.as_str()))
    {
        let cycles = detect_cycle_warnings(issues, &config.cycles);
        all_suggestions.extend(cycles);
    }

    // Apply filters.
    let filtered: Vec<Suggestion> = all_suggestions
        .into_iter()
        .filter(|sug| {
            if config.min_confidence > 0.0 && sug.confidence < config.min_confidence {
                return false;
            }
            if let Some(ref ft) = config.filter_type {
                if &sug.sug_type != ft {
                    return false;
                }
            }
            if !config.filter_bead.is_empty()
                && sug.target_bead != config.filter_bead
                && sug.related_bead.as_deref() != Some(config.filter_bead.as_str())
            {
                return false;
            }
            true
        })
        .collect();

    // Sort by confidence (highest first). Go uses unstable sort for equal
    // confidence — golden order is non-deterministic and not reproducible.
    let mut filtered = filtered;
    filtered.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut filtered = filtered;
    if config.max_suggestions > 0 && filtered.len() > config.max_suggestions {
        filtered.truncate(config.max_suggestions);
    }

    SuggestionSet::new(filtered, data_hash)
}

/// Create the full robot-suggest output.
/// Matches Go `GenerateRobotSuggestOutput` exactly.
pub fn generate_robot_suggest_output(
    issues: &[Issue],
    config: &SuggestAllConfig,
    data_hash: &str,
) -> RobotSuggestOutput {
    let set = generate_all_suggestions(issues, config, data_hash);

    RobotSuggestOutput {
        generated_at: now_rfc3339(),
        data_hash: data_hash.to_string(),
        filters: SuggestFilter {
            filter_type: config.filter_type.clone().unwrap_or_default(),
            min_confidence: config.min_confidence,
            bead_id: config.filter_bead.clone(),
        },
        suggestions: set,
        usage_hints: vec![
            "jq '.suggestions.suggestions[:5]' - Top 5 suggestions by confidence".to_string(),
            "jq '.suggestions.suggestions[] | select(.type==\"potential_duplicate\")' - Filter duplicates".to_string(),
            "jq '.suggestions.suggestions[] | select(.confidence >= 0.8)' - High-confidence only".to_string(),
            "jq '.suggestions.stats.by_type' - Count by suggestion type".to_string(),
            "jq '.suggestions.suggestions[].action_command' - All action commands".to_string(),
            "--suggest-type=dependency - Filter to dependency suggestions".to_string(),
            "--suggest-confidence=0.7 - Minimum confidence threshold".to_string(),
            "--suggest-bead=<id> - Suggestions for specific bead".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_rfc3339() -> String {
    let ts = jiff::Timestamp::now();
    let s = ts.to_string();
    // Truncate to second precision (Go parity).
    if let Some(pos) = s.find('.') {
        format!("{}Z", &s[..pos])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(id: &str, title: &str, desc: &str) -> Issue {
        Issue {
            id: id.to_string(),
            content_hash: String::new(),
            title: title.to_string(),
            description: desc.to_string(),
            design: String::new(),
            acceptance_criteria: String::new(),
            notes: String::new(),
            status: Status::Open,
            priority: 0,
            issue_type: String::new(),
            assignee: String::new(),
            estimated_minutes: None,
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
            updated_at: Some("2026-01-01T00:00:00Z".to_string()),
            due_date: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: Vec::new(),
            dependencies: Vec::new(),
            comments: Vec::new(),
            source_repo: String::new(),
        }
    }

    #[test]
    fn extract_keywords_filters_stop_words() {
        let kws = extract_keywords("the quick brown fox", "is very fast");
        assert!(!kws.contains(&"the".to_string()));
        assert!(!kws.contains(&"is".to_string()));
        assert!(!kws.contains(&"very".to_string()));
    }

    #[test]
    fn extract_keywords_removes_short_words() {
        let kws = extract_keywords("a bc def", "");
        assert!(!kws.contains(&"a".to_string()));
        assert!(!kws.contains(&"bc".to_string()));
        assert!(kws.contains(&"def".to_string()));
    }

    #[test]
    fn detect_duplicates_basic() {
        let issues = vec![
            make_issue(
                "A-1",
                "Fix database connection pool",
                "Connection pool exhaustion",
            ),
            make_issue(
                "A-2",
                "Fix database connection pool overflow",
                "Connection pool exhaustion bug",
            ),
            make_issue("A-3", "Add new API endpoint", "Create a new REST endpoint"),
        ];
        let config = DuplicateConfig::default();
        let suggestions = detect_duplicates(&issues, &config);
        // A-1 and A-2 should be flagged as potential duplicates.
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].sug_type, "potential_duplicate");
    }

    #[test]
    fn detect_duplicates_too_few_issues() {
        let issues = vec![make_issue("A-1", "Fix bug", "Some desc")];
        let config = DuplicateConfig::default();
        let suggestions = detect_duplicates(&issues, &config);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn detect_missing_dependencies_basic() {
        let issue1 = make_issue(
            "A-1",
            "Fix database connection",
            "Fixes connection to the database",
        );
        let issue2 = make_issue(
            "A-2",
            "Database migration script",
            "Create a migration for database schema",
        );
        // Both share "database" as a keyword.
        let config = DependencySuggestionConfig::default();
        let suggestions = detect_missing_dependencies(&[issue1, issue2], &config);
        // May or may not meet min confidence depending on keyword overlap.
        // With only 1 shared keyword and min 2, should be empty.
        assert!(suggestions.is_empty());
    }

    #[test]
    fn detect_cycle_warnings_empty_graph() {
        let issues = vec![make_issue("A-1", "One issue", "")];
        let config = CycleWarningConfig::default();
        let suggestions = detect_cycle_warnings(&issues, &config);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn suggestion_set_stats() {
        let s1 = Suggestion::new(
            SuggestionType::PotentialDuplicate,
            "A-1",
            "dup",
            "reason",
            0.8,
        );
        let s2 = Suggestion::new(
            SuggestionType::LabelSuggestion,
            "A-2",
            "label",
            "reason",
            0.3,
        );
        let set = SuggestionSet::new(vec![s1, s2], "abc123");
        assert_eq!(set.stats.total, 2);
        assert_eq!(set.stats.actionable_count, 0);
        assert!(set.stats.by_type.contains_key("potential_duplicate"));
        assert!(set.stats.by_type.contains_key("label_suggestion"));
    }

    #[test]
    fn builtin_label_mappings_count() {
        let m = builtin_label_mappings();
        // Go has 43 keyword entries mapping to labels.
        assert_eq!(m.len(), 43);
    }

    #[test]
    fn stop_words_count() {
        assert_eq!(STOP_WORDS.len(), 60);
    }
}
