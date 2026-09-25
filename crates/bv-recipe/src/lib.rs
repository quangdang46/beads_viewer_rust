//! bv-recipe: the `--recipe` engine, a port of Go `pkg/recipe` at commit
//! `18afafa` (bv v0.25.0).
//!
//! A recipe is a reusable view configuration. [`apply::apply`] is the one
//! engine the TUI and the robot path share: it filters issues with the recipe's
//! filters, orders them by its sort chain and keeps at most `view.max_items`.
//! Until this crate is wired in, `--recipe` only reaches the envelope's
//! `scope.recipe` field and changes nothing about the issues that are reported.
//!
//! The load-bearing ports, in dependency order:
//!
//! - [`types`] — [`Recipe`] and friends, their validation, and relative/ISO
//!   time parsing (`pkg/recipe/types.go`).
//! - [`loader`] — builtin + user + project discovery, and `--recipe`
//!   argument resolution (`pkg/recipe/loader.go`).
//! - [`apply`] — the filter/sort/truncate engine (`pkg/recipe/apply.go`).
//! - [`readiness`] — the slice of `pkg/model/readiness.go` the filters consult.
//!
//! Two deliberate deviations from Go, both forced by this repository's model:
//!
//! - **Status is a closed enum.** Go's `model.Status` is an open string, so a
//!   custom workflow state such as `qa-review` can appear on an issue and a
//!   recipe can select it. `bv_core::model::Status` has exactly ten values, so
//!   such a filter validates but matches nothing.
//! - **Timestamps stay strings.** Go types `CreatedAt`/`UpdatedAt`/
//!   `DeferUntil` as `time.Time`; here they are the `Option<String>` the loader
//!   keeps for `data_hash` byte-parity, and this crate parses them. A stamp that
//!   does not parse reads as unset, which is the branch Go takes for an absent
//!   timestamp.
//!
//! Everything else — field names, defaults, sort directions, error wording,
//! source precedence — is copied from the Go source rather than invented.
//!
//! # Example
//!
//! ```
//! use bv_recipe::{apply, Loader};
//!
//! let mut loader = Loader::new();
//! loader.load().unwrap();
//! let blocked = loader.get("blocked").expect("builtin");
//!
//! assert!(blocked.needs_graph_metrics() == false);
//! // Applying with no metrics source reads every graph score as 0.
//! let applied = apply::apply(&[], &apply::Metrics::default(), blocked, None).unwrap();
//! assert!(applied.is_empty());
//! ```

pub mod apply;
pub mod loader;
pub mod readiness;
pub mod types;

#[cfg(test)]
mod testutil;

pub use apply::{apply, filter, sort_issues, GraphMetrics, Metrics};
pub use loader::{
    is_path_argument, load_default, load_file, Loader, RecipeSummary, ResolveError,
    UnknownRecipeError, BUILTIN_RECIPES_YAML, SOURCE_BUILTIN, SOURCE_PROJECT, SOURCE_PROJECT_FILE,
    SOURCE_USER,
};
pub use readiness::Readiness;
pub use types::{
    canonical_sort_field, parse_relative_time, ExportConfig, FilterConfig, Recipe, SortConfig,
    TimeParseError, ViewConfig, SORT_FIELD_BETWEENNESS, SORT_FIELD_CREATED, SORT_FIELD_ID,
    SORT_FIELD_IMPACT, SORT_FIELD_PAGERANK, SORT_FIELD_PRIORITY, SORT_FIELD_STATUS,
    SORT_FIELD_TITLE, SORT_FIELD_TRIAGE, SORT_FIELD_UPDATED, VIEW_COLUMNS, VIEW_METRICS,
};
