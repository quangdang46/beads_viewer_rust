//! Port of Go `filterByRepo` (`cmd/bv/main.go:5196-5239`) — the issue-narrowing
//! half of the `--repo` scope modifier.
//!
//! Go applies this inside `scopeLoadedIssues` (`main.go:4874-4885`) *before*
//! label and recipe selection, rebuilds `CandidateIDs` from the survivors, and
//! recomputes `DataHash` over the filtered set (`main.go:4883-4885`). The repo
//! scope therefore narrows the corpus every downstream metric sees; it is not
//! merely an annotation on the emitted envelope.

use crate::model::Issue;

/// Go `filterByRepo` (`main.go:5196-5239`). Keeps every issue whose ID — or,
/// failing that, whose `SourceRepo` — carries `filter` as a case-insensitive
/// prefix. An empty filter keeps the whole set, matching Go's early return at
/// `main.go:5197-5199`.
///
/// Go's "flexible separator" branch (`main.go:5220-5227`) is reproduced
/// verbatim even though it can never fire: prefix-matching `filterLower + "-"`
/// implies prefix-matching `filterLower`, so the plain-prefix branch's
/// `continue` (`main.go:5214-5217`) always fires first. The structure is kept
/// rather than "simplified" so this stays line-for-line auditable against the
/// oracle — the two forms select the same issues either way.
pub fn filter_by_repo(issues: &[Issue], filter: &str) -> Vec<Issue> {
    if filter.is_empty() {
        return issues.to_vec();
    }

    // Go main.go:5202-5203 — the filter is lowercased but never trimmed and
    // never separator-stripped, so `--repo " api"` matches nothing.
    let filter_lower = filter.to_lowercase();
    // Go main.go:5205-5207 tests the *raw* filter's suffix, not the lowercased
    // one. `-`, `:` and `_` are case-invariant, so the two forms agree.
    let needs_flexible_match =
        !filter.ends_with('-') && !filter.ends_with(':') && !filter.ends_with('_');

    // Hoisted out of the loop Go runs per issue (`main.go:5221-5223`); the
    // filter is loop-invariant, so this allocates three strings in total.
    let dash = format!("{filter_lower}-");
    let colon = format!("{filter_lower}:");
    let underscore = format!("{filter_lower}_");

    let mut result = Vec::new();
    for issue in issues {
        // Go main.go:5211.
        let id_lower = issue.id.to_lowercase();

        // Go main.go:5214-5217. Note this is a raw prefix compare, not a
        // separator-boundary compare: filter "api" also matches "apiary-1".
        if id_lower.starts_with(&filter_lower) {
            result.push(issue.clone());
            continue;
        }

        // Go main.go:5220-5227.
        if needs_flexible_match
            && (id_lower.starts_with(&dash)
                || id_lower.starts_with(&colon)
                || id_lower.starts_with(&underscore))
        {
            result.push(issue.clone());
            continue;
        }

        // Go main.go:5230-5235 — the `SourceRepo` fallback. Empty and `"."`
        // are both treated as "unset". `workspace::source_repo_key_from_prefix`
        // stores the value separator-free and lowercased, so this prefix
        // compare is what makes the fallback fire in workspace mode.
        if !issue.source_repo.is_empty()
            && issue.source_repo != "."
            && issue.source_repo.to_lowercase().starts_with(&filter_lower)
        {
            result.push(issue.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::filter_by_repo;
    use crate::model::{Issue, Status};

    fn issue(id: &str, source_repo: &str) -> Issue {
        Issue {
            id: id.into(),
            content_hash: String::new(),
            title: "T".into(),
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
            defer_until: None,
            closed_at: None,
            external_ref: None,
            compaction_level: 0,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: 0,
            labels: vec![],
            dependencies: vec![],
            comments: vec![],
            source_repo: source_repo.into(),
        }
    }

    fn ids(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.id.as_str()).collect()
    }

    /// Go main.go:5197-5199 returns the input untouched for an empty filter.
    #[test]
    fn empty_filter_keeps_everything() {
        let src = vec![issue("api-1", ""), issue("web-1", "")];
        assert_eq!(ids(&filter_by_repo(&src, "")), ["api-1", "web-1"]);
    }

    /// Go main.go:5211 + 5214 — the ID compare is case-insensitive on both sides.
    #[test]
    fn id_prefix_match_is_case_insensitive() {
        let src = vec![issue("API-1", ""), issue("web-1", "")];
        assert_eq!(ids(&filter_by_repo(&src, "api")), ["API-1"]);
    }

    /// Go main.go:5214 is a raw `strings.HasPrefix`, not a boundary compare, so
    /// a filter also matches IDs that merely continue the same letters.
    #[test]
    fn id_prefix_is_not_a_separator_boundary() {
        let src = vec![issue("apiary-1", ""), issue("web-1", "")];
        assert_eq!(ids(&filter_by_repo(&src, "api")), ["apiary-1"]);
    }

    /// Go main.go:5205-5207 + 5214-5217 — all three separators match a filter
    /// that carries none of them, and an exact (separator-free) ID matches too.
    #[test]
    fn all_three_separators_and_exact_ids_match_a_bare_filter() {
        let src = vec![
            issue("api-1", ""),
            issue("api:2", ""),
            issue("api_3", ""),
            issue("api", ""),
            issue("apix", ""),
            issue("web-1", ""),
        ];
        assert_eq!(
            ids(&filter_by_repo(&src, "api")),
            ["api-1", "api:2", "api_3", "api", "apix"]
        );
    }

    /// Go main.go:5205-5207 — a filter that already ends in a separator does not
    /// get the *other* separators appended, so `api-` matches anything starting
    /// with `api-` (including `api--1`, via the plain-prefix branch at
    /// `main.go:5214`) but never `api:1` or `api_1`.
    #[test]
    fn separator_terminated_filter_tries_no_other_separator() {
        let src = vec![
            issue("api-1", ""),
            issue("api--1", ""),
            issue("api:1", ""),
            issue("api_1", ""),
        ];
        assert_eq!(ids(&filter_by_repo(&src, "api-")), ["api-1", "api--1"]);
    }

    /// Go main.go:5230-5235 — the `SourceRepo` fallback rescues an issue whose
    /// ID does not carry the prefix, and is itself case-insensitive.
    #[test]
    fn source_repo_fallback_matches_when_id_does_not() {
        let src = vec![
            issue("bd-42", "api"),
            issue("bd-43", "API"),
            issue("bd-44", ""),
        ];
        assert_eq!(ids(&filter_by_repo(&src, "api")), ["bd-42", "bd-43"]);
    }

    /// Go main.go:5230 guards the fallback with `SourceRepo != "" && != "."`.
    #[test]
    fn source_repo_fallback_ignores_empty_and_dot() {
        let src = vec![
            issue("bd-42", ""),
            issue("bd-43", "."),
            issue("bd-44", "api"),
        ];
        assert_eq!(ids(&filter_by_repo(&src, "api")), ["bd-44"]);
    }

    /// Go main.go:4883-4885 recomputes the data hash over the *survivors*, so a
    /// caller that rehashes the returned slice must see a smaller corpus. This
    /// is the A/B the `--repo` wiring depends on.
    #[test]
    fn non_matching_filter_narrows_to_empty() {
        let src = vec![issue("api-1", ""), issue("web-1", "")];
        assert!(filter_by_repo(&src, "zzznope").is_empty());
    }

    /// Go appends in input order (`main.go:5210-5236`); downstream stable sorts
    /// depend on that starting order.
    #[test]
    fn survivors_keep_input_order() {
        let src = vec![
            issue("api-3", ""),
            issue("web-1", ""),
            issue("api-1", ""),
            issue("api-2", ""),
        ];
        assert_eq!(
            ids(&filter_by_repo(&src, "api")),
            ["api-3", "api-1", "api-2"]
        );
    }

    /// The `continue`s in Go's three branches mean an issue is appended at most
    /// once even when its ID *and* its `SourceRepo` both match.
    #[test]
    fn an_issue_matching_both_id_and_source_repo_is_not_duplicated() {
        let src = vec![issue("api-1", "api")];
        assert_eq!(ids(&filter_by_repo(&src, "api")), ["api-1"]);
    }

    /// Go main.go:5202-5203 lowercases but does not trim, so a padded filter
    /// matches nothing — the envelope would still advertise the raw value.
    #[test]
    fn filter_is_not_trimmed() {
        let src = vec![issue("api-1", "")];
        assert!(filter_by_repo(&src, " api").is_empty());
    }
}
