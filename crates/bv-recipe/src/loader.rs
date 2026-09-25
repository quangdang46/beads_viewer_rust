//! Recipe discovery and merging — port of Go `pkg/recipe/loader.go` at commit
//! `18afafa`.
//!
//! [`Loader::load`] merges four sources in ascending precedence: the embedded
//! [`BUILTIN_RECIPES_YAML`], `~/.config/bv/recipes.yaml`, `<project>/.bv/
//! recipes.yaml`, then one file per recipe under `<project>/.beads/recipes/`.
//! Missing optional files are fine; an unreadable or invalid one becomes a
//! [`Loader::warnings`] entry and is skipped, so a broken user file never costs
//! you the builtins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::Recipe;

/// The embedded builtin set, a verbatim copy of Go's
/// `pkg/recipe/defaults/recipes.yaml` at `18afafa`. The descriptions are echoed
/// by `--robot-recipes`, so they are byte-for-byte load-bearing.
pub const BUILTIN_RECIPES_YAML: &str = include_str!("../defaults/recipes.yaml");

/// Recipe sources, in ascending precedence: a later source overrides an earlier
/// one that defines the same name.
pub const SOURCE_BUILTIN: &str = "builtin";
pub const SOURCE_USER: &str = "user";
pub const SOURCE_PROJECT: &str = "project";
pub const SOURCE_PROJECT_FILE: &str = "project-file";

/// The `recipes:` map form. A `null` value disables a name, which is why the
/// value is optional all the way down.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeFile {
    #[serde(default)]
    recipes: BTreeMap<String, Option<Recipe>>,
}

/// A lightweight recipe description for discovery (`--robot-recipes`).
///
/// Field order is the JSON key order Go emits, and `path` is omitted when empty
/// exactly as `json:"path,omitempty"` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeSummary {
    pub name: String,
    pub description: String,
    /// One of the [`SOURCE_BUILTIN`] family.
    pub source: String,
    /// File that defined a project-file recipe.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

/// Go `UnknownRecipeError` — the argument names neither a recipe file nor a
/// loaded recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRecipeError {
    pub name: String,
    /// Every name that does resolve, sorted — the caller prints these under
    /// "Available recipes:".
    pub available: Vec<String>,
}

impl std::fmt::Display for UnknownRecipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Go's `fmt.Sprintf("unknown recipe %q", e.Name)`.
        write!(f, "unknown recipe {:?}", self.name)
    }
}

impl std::error::Error for UnknownRecipeError {}

/// What [`Loader::resolve`] can fail with. Go returns `*UnknownRecipeError` or a
/// plain error from [`load_file`]; the two are told apart so a caller can decide
/// whether to print the available-names table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    Unknown(UnknownRecipeError),
    /// A `.yaml`/`.yml` argument that could not be loaded.
    Load(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Unknown(e) => e.fmt(f),
            ResolveError::Load(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Why one optional source failed. Go distinguishes "absent" (silent) from
/// "present but bad" (a warning) via `os.IsNotExist`.
#[derive(Debug, Clone)]
enum SourceError {
    Absent,
    Bad(String),
}

/// Loads and merges recipes from multiple sources.
#[derive(Debug, Clone)]
pub struct Loader {
    recipes: BTreeMap<String, Recipe>,
    /// Recipe name -> source.
    sources: BTreeMap<String, String>,
    /// Recipe name -> defining file (project-file only).
    paths: BTreeMap<String, String>,
    user_path: String,
    project_dir: String,
    warnings: Vec<String>,
}

impl Loader {
    /// Go `NewLoader()` — user config at `~/.config/bv/recipes.yaml`, project
    /// config at the working directory.
    pub fn new() -> Self {
        let mut l = Self {
            recipes: BTreeMap::new(),
            sources: BTreeMap::new(),
            paths: BTreeMap::new(),
            user_path: String::new(),
            project_dir: String::new(),
            warnings: Vec::new(),
        };
        // Go fills in a default only for an empty override, so passing an empty
        // path means "use the default", not "disable this source".
        if l.user_path.is_empty() {
            l.user_path = home_dir()
                .map(|h| h.join(".config").join("bv").join("recipes.yaml"))
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
        }
        if l.project_dir.is_empty() {
            l.project_dir = std::env::current_dir()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        l
    }

    /// Go `WithUserPath` — override `~/.config/bv/recipes.yaml`.
    pub fn with_user_path(mut self, path: impl Into<String>) -> Self {
        self.user_path = path.into();
        self
    }

    /// Go `WithProjectDir` — override the directory holding `.bv/` and
    /// `.beads/recipes/`.
    pub fn with_project_dir(mut self, dir: impl Into<String>) -> Self {
        self.project_dir = dir.into();
        self
    }

    /// Go `Load`. Fails only when the embedded builtins cannot be parsed, which
    /// cannot happen for a compiled-in constant; every other source degrades to
    /// a warning.
    pub fn load(&mut self) -> Result<(), String> {
        self.load_builtin()
            .map_err(|e| format!("loading builtin recipes: {e}"))?;

        let user_path = self.user_path.clone();
        if !user_path.is_empty() {
            if let Err(SourceError::Bad(message)) =
                self.load_from_file(Path::new(&user_path), SOURCE_USER)
            {
                self.warnings.push(format!("user config: {message}"));
            }
        }

        let project_dir = self.project_dir.clone();
        if !project_dir.is_empty() {
            let project_path = Path::new(&project_dir).join(".bv").join("recipes.yaml");
            if let Err(SourceError::Bad(message)) =
                self.load_from_file(&project_path, SOURCE_PROJECT)
            {
                self.warnings.push(format!("project config: {message}"));
            }

            self.load_project_files(&Path::new(&project_dir).join(".beads").join("recipes"));
        }

        Ok(())
    }

    /// Go `loadBuiltin` — parse the embedded defaults. Unlike a user file, an
    /// invalid builtin is fatal: the binary would be shipping a broken recipe.
    fn load_builtin(&mut self) -> Result<(), String> {
        let file: RecipeFile = decode_strict(BUILTIN_RECIPES_YAML)
            .map_err(|e| format!("parsing embedded defaults: {e}"))?;
        for (name, maybe_recipe) in file.recipes {
            let Some(mut recipe) = maybe_recipe else {
                continue;
            };
            recipe.name = name.clone();
            if let Err(e) = recipe.validate() {
                return Err(format!("builtin recipe {name}: {e}"));
            }
            self.set(&name, recipe, SOURCE_BUILTIN, "");
        }
        Ok(())
    }

    /// Go `loadFromFile` — merge a `recipes:` map. An individual recipe that
    /// fails validation is skipped with a warning; an explicit `null` disables
    /// the name.
    fn load_from_file(&mut self, path: &Path, source: &str) -> Result<(), SourceError> {
        let data = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SourceError::Absent
            } else {
                SourceError::Bad(format!("open {}: {e}", path.display()))
            }
        })?;
        let file: RecipeFile = decode_strict(&data)
            .map_err(|e| SourceError::Bad(format!("parsing {}: {e}", path.display())))?;

        for (name, maybe_recipe) in file.recipes {
            let Some(mut recipe) = maybe_recipe else {
                self.remove(&name);
                continue;
            };
            recipe.name = name.clone();
            if let Err(e) = recipe.validate() {
                self.warnings
                    .push(format!("recipe {name} in {}: {e}", path.display()));
                continue;
            }
            self.set(&name, recipe, source, "");
        }
        Ok(())
    }

    /// Go `loadProjectFiles` — register every `*.yaml`/`*.yml` under `dir` as
    /// one recipe. Files are visited in name order so a duplicate name resolves
    /// deterministically (later file wins, with a warning).
    fn load_project_files(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.warnings
                        .push(format!("project recipes dir {}: {e}", dir.display()));
                }
                return;
            }
        };
        // Go keeps any non-directory entry, so a symlinked recipe file is
        // still loaded; `is_file()` resolves the link, matching that.
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_path_argument)
            })
            .collect();
        // Every entry shares this directory, so ordering by path component is
        // the same name ordering as Go's `sort.Strings` on the full paths.
        paths.sort();

        for path in paths {
            let recipe = match load_file(&path) {
                Ok(r) => r,
                Err(e) => {
                    self.warnings.push(e);
                    continue;
                }
            };
            let path_text = path.to_string_lossy().into_owned();
            let name = recipe.name.clone();
            match self.paths.get(&name) {
                Some(prev) if prev != &path_text => self.warnings.push(format!(
                    "project recipe file {path_text}: recipe {name:?} already defined by {prev}; using {path_text}"
                )),
                _ => {}
            }
            self.set(&name, recipe, SOURCE_PROJECT_FILE, &path_text);
        }
    }

    /// Go `set` — a map source clears any recorded path.
    fn set(&mut self, name: &str, recipe: Recipe, source: &str, path: &str) {
        self.recipes.insert(name.to_string(), recipe);
        self.sources.insert(name.to_string(), source.to_string());
        if path.is_empty() {
            self.paths.remove(name);
        } else {
            self.paths.insert(name.to_string(), path.to_string());
        }
    }

    /// Go `remove`.
    fn remove(&mut self, name: &str) {
        self.recipes.remove(name);
        self.sources.remove(name);
        self.paths.remove(name);
    }

    /// Go `Get` — the recipe registered under `name`, if any.
    pub fn get(&self, name: &str) -> Option<&Recipe> {
        self.recipes.get(name)
    }

    /// Go `Resolve` — turn a `--recipe` argument into a recipe. A `.yaml`/`.yml`
    /// argument is loaded from that path; anything else is looked up by name.
    ///
    /// The returned recipe is a copy, matching Go's `*Recipe` return: a
    /// path-loaded recipe is never registered in the loader's table.
    pub fn resolve(&self, arg: &str) -> Result<Recipe, ResolveError> {
        if is_path_argument(arg) {
            return load_file(Path::new(arg.trim())).map_err(ResolveError::Load);
        }
        match self.get(arg) {
            Some(r) => Ok(r.clone()),
            None => Err(ResolveError::Unknown(UnknownRecipeError {
                name: arg.to_string(),
                available: self.names(),
            })),
        }
    }

    /// Go `List` — every available recipe, sorted by name.
    pub fn list(&self) -> Vec<&Recipe> {
        self.recipes.values().collect()
    }

    /// Go `ListSummaries` — lightweight recipe summaries, sorted by name.
    pub fn list_summaries(&self) -> Vec<RecipeSummary> {
        self.recipes
            .iter()
            .map(|(name, recipe)| RecipeSummary {
                name: name.clone(),
                description: recipe.description.clone(),
                source: self.source(name),
                path: self.path(name),
            })
            .collect()
    }

    /// Go `Names` — every recipe name, sorted.
    pub fn names(&self) -> Vec<String> {
        self.recipes.keys().cloned().collect()
    }

    /// Go `Warnings` — everything skipped while loading, in discovery order.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Go `Source` — the `Source*` constant that last defined `name`, or `""`.
    pub fn source(&self, name: &str) -> String {
        self.sources.get(name).cloned().unwrap_or_default()
    }

    /// Go `Path` — the file that defined a project-file recipe, or `""` for
    /// recipes from map sources.
    pub fn path(&self, name: &str) -> String {
        self.paths.get(name).cloned().unwrap_or_default()
    }
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}

/// Go `LoadDefault` — a loader over the default user and project locations.
pub fn load_default() -> Result<Loader, String> {
    let mut loader = Loader::new();
    loader.load()?;
    Ok(loader)
}

/// Go `IsPathArgument` — whether a `--recipe` argument names a YAML file rather
/// than a recipe name.
pub fn is_path_argument(arg: &str) -> bool {
    let lower = arg.trim().to_lowercase();
    lower.ends_with(".yaml") || lower.ends_with(".yml")
}

/// Go `LoadFile` — parse a single-recipe YAML file such as
/// `.beads/recipes/sprint-review.yaml`. The recipe's name comes from its `name`
/// field, defaulting to the file stem. Unknown keys and invalid values are
/// errors that name the offending field.
pub fn load_file(path: &Path) -> Result<Recipe, String> {
    let data = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("recipe file not found: {}", path.display())
        } else {
            format!("reading recipe file {}: {e}", path.display())
        }
    })?;
    let mut recipe: Recipe =
        decode_strict(&data).map_err(|e| format!("parsing recipe file {}: {e}", path.display()))?;
    if recipe.name.trim().is_empty() {
        recipe.name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    recipe
        .validate()
        .map_err(|e| format!("recipe file {}: {e}", path.display()))?;
    Ok(recipe)
}

/// Go `decodeStrict` — unmarshal YAML rejecting keys the target type does not
/// declare, so a misspelt filter is reported (with its name) instead of being
/// silently dropped. `#[serde(deny_unknown_fields)]` on every recipe struct is
/// what does the rejecting. An empty document leaves `T` at its default, as
/// Go's `io.EOF` tolerance does.
fn decode_strict<T: serde::de::DeserializeOwned + Default>(data: &str) -> Result<T, String> {
    if data.trim().is_empty() {
        return Ok(T::default());
    }
    serde_yaml_ng::from_str(data).map_err(|e| e.to_string())
}

/// Go's `os.UserHomeDir`: `%USERPROFILE%` on Windows, `$HOME` elsewhere.
fn home_dir() -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{issue, IssueExt};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh directory under the OS temp dir. This crate never removes
    /// anything (the repository forbids file deletion), so each call takes a new
    /// name and the tag keeps parallel tests apart.
    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bv-recipe-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, content).expect("write fixture");
    }

    /// Builtins only: both optional sources point somewhere that does not exist.
    fn builtins_only(tag: &str) -> Loader {
        let root = scratch(tag);
        let mut l = Loader::new()
            .with_user_path(
                root.join("no-user-config.yaml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .with_project_dir(root.join("no-project").to_string_lossy().into_owned());
        l.load().expect("load builtins");
        l
    }

    const BUILTIN_NAMES: [&str; 11] = [
        "actionable",
        "blocked",
        "bottlenecks",
        "closed",
        "default",
        "high-impact",
        "quick-wins",
        "recent",
        "release-cut",
        "stale",
        "triage",
    ];

    #[test]
    fn builtins_load_from_the_embedded_yaml() {
        let loader = builtins_only("builtin");
        assert_eq!(loader.names(), BUILTIN_NAMES);
        for name in BUILTIN_NAMES {
            assert!(loader.get(name).is_some(), "missing builtin {name}");
            assert_eq!(loader.source(name), SOURCE_BUILTIN);
            assert_eq!(loader.path(name), "");
        }
        let default = loader.get("default").expect("default");
        assert_eq!(default.name, "default");
        assert_eq!(
            default.description,
            "Default view showing all open issues sorted by priority"
        );
        assert!(loader.get("nonexistent").is_none());
        assert_eq!(loader.list().len(), loader.names().len());
    }

    #[test]
    fn every_builtin_validates_and_declares_the_metrics_it_needs() {
        let loader = builtins_only("validate");
        for r in loader.list() {
            assert_eq!(r.validate(), Ok(()), "builtin {}", r.name);
            assert!(
                crate::apply::apply(&[], &crate::apply::Metrics::default(), r, None).is_ok(),
                "builtin {} does not apply",
                r.name
            );
        }
        for (name, want) in [
            ("high-impact", true),
            ("bottlenecks", true),
            ("triage", false),
            ("actionable", false),
        ] {
            assert_eq!(
                loader.get(name).expect(name).needs_graph_metrics(),
                want,
                "{name} needs_graph_metrics"
            );
        }
        assert!(loader.get("triage").expect("triage").needs_triage_scores());
        assert!(!loader
            .get("high-impact")
            .expect("high-impact")
            .needs_triage_scores());
    }

    #[test]
    fn the_user_file_overrides_a_builtin_and_adds_names() {
        let dir = scratch("user");
        let user_path = dir.join("recipes.yaml");
        write(
            &user_path,
            r#"
recipes:
  custom:
    description: "Custom user recipe"
    filters:
      status: [open]
    sort:
      field: title
  default:
    description: "Overridden default"
    filters:
      status: [closed]
"#,
        );
        let mut loader = Loader::new()
            .with_user_path(user_path.to_string_lossy().into_owned())
            .with_project_dir(dir.join("no-project").to_string_lossy().into_owned());
        loader.load().expect("load");

        let custom = loader.get("custom").expect("custom");
        assert_eq!(custom.description, "Custom user recipe");
        assert_eq!(loader.source("custom"), SOURCE_USER);
        assert_eq!(loader.path("custom"), "");

        let default = loader.get("default").expect("default still present");
        assert_eq!(default.description, "Overridden default");
        assert_eq!(loader.source("default"), SOURCE_USER);
        // A map source carries no path, and never inherited the builtin's.
        assert_eq!(loader.path("default"), "");
    }

    #[test]
    fn a_project_map_defines_project_scoped_recipes() {
        let dir = scratch("project");
        write(
            &dir.join(".bv").join("recipes.yaml"),
            r#"
recipes:
  project-local:
    description: "Project-specific recipe"
    filters:
      id_prefix: "proj-"
"#,
        );
        let mut loader = Loader::new()
            .with_user_path(dir.join("none.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.to_string_lossy().into_owned());
        loader.load().expect("load");
        assert!(loader.get("project-local").is_some());
        assert_eq!(loader.source("project-local"), SOURCE_PROJECT);
    }

    #[test]
    fn a_null_value_disables_a_builtin() {
        let dir = scratch("disable");
        write(&dir.join("recipes.yaml"), "recipes:\n  stale: null\n");
        let mut loader = Loader::new()
            .with_user_path(dir.join("recipes.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.join("no-project").to_string_lossy().into_owned());
        loader.load().expect("load");
        assert!(loader.get("stale").is_none(), "stale should be disabled");
        assert!(loader.get("default").is_some(), "builtins survive");
        assert!(loader.warnings().is_empty());
    }

    #[test]
    fn summaries_carry_a_source_and_a_path() {
        let loader = builtins_only("summaries");
        let summaries = loader.list_summaries();
        assert_eq!(summaries.len(), 11);
        for s in &summaries {
            assert!(!s.name.is_empty());
            assert!(!s.source.is_empty());
            assert!(s.path.is_empty());
        }
        // Alphabetical, as Go's `sort.Strings` and `ListSummaries` both imply.
        let names: Vec<&str> = summaries.iter().map(|s| s.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn missing_optional_files_are_silent() {
        let dir = scratch("missing");
        let mut loader = Loader::new()
            .with_user_path(dir.join("nope.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.join("nope-project").to_string_lossy().into_owned());
        loader
            .load()
            .expect("a missing optional file is not an error");
        assert!(loader.get("default").is_some());
        assert!(loader.warnings().is_empty(), "{:?}", loader.warnings());
    }

    #[test]
    fn an_unparseable_user_file_warns_but_keeps_the_builtins() {
        let dir = scratch("badyaml");
        write(&dir.join("recipes.yaml"), "invalid: [yaml: {");
        let mut loader = Loader::new()
            .with_user_path(dir.join("recipes.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.join("no-project").to_string_lossy().into_owned());
        loader.load().expect("load still succeeds");
        assert!(!loader.warnings().is_empty());
        assert!(loader.warnings().iter().any(|w| w.contains("user config")));
        assert!(loader.get("default").is_some());
    }

    #[test]
    fn an_unknown_key_rejects_the_whole_document() {
        let dir = scratch("unknownkey");
        write(
            &dir.join("recipes.yaml"),
            r#"
recipes:
  good:
    description: "fine"
    filters:
      status: [open]
  typo:
    description: "misspelt key"
    filter:
      status: [open]
"#,
        );
        let mut loader = Loader::new()
            .with_user_path(dir.join("recipes.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.join("no-project").to_string_lossy().into_owned());
        loader.load().expect("load");
        // Strict decoding rejects the whole document, so neither recipe loads
        // and the warning names the offending key.
        assert!(loader.get("typo").is_none() && loader.get("good").is_none());
        assert!(
            loader
                .warnings()
                .iter()
                .any(|w| w.contains("user config") && w.contains("filter")),
            "{:?}",
            loader.warnings()
        );
        assert!(
            loader.get("default").is_some(),
            "builtins survive a bad user file"
        );
    }

    #[test]
    fn a_recipe_failing_validation_is_skipped_individually() {
        let dir = scratch("skipone");
        write(
            &dir.join(".bv").join("recipes.yaml"),
            "recipes:\n  ok:\n    filters:\n      status: [open]\n  broken:\n    filters:\n      updated_after: \"last tuesday\"\n",
        );
        let mut loader = Loader::new()
            .with_user_path(dir.join("none.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.to_string_lossy().into_owned());
        loader.load().expect("load");
        assert!(loader.get("ok").is_some());
        assert!(loader.get("broken").is_none());
        assert!(
            loader
                .warnings()
                .iter()
                .any(|w| w.contains("recipe broken") && w.contains("filters.updated_after")),
            "{:?}",
            loader.warnings()
        );
    }

    #[test]
    fn later_sources_win_in_the_documented_order() {
        let dir = scratch("precedence");
        write(
            &dir.join("user.yaml"),
            "recipes:\n  default:\n    description: user\n  user-only:\n    description: user\n",
        );
        write(
            &dir.join(".bv").join("recipes.yaml"),
            "recipes:\n  default:\n    description: project\n  user-only:\n    description: project\n  project-only:\n    description: project\n",
        );
        write(
            &dir.join(".beads").join("recipes").join("default.yaml"),
            "description: project-file\n",
        );
        write(
            &dir.join(".beads").join("recipes").join("project-only.yaml"),
            "description: project-file\n",
        );
        let mut loader = Loader::new()
            .with_user_path(dir.join("user.yaml").to_string_lossy().into_owned())
            .with_project_dir(dir.to_string_lossy().into_owned());
        loader.load().expect("load");

        for (name, source) in [
            ("default", SOURCE_PROJECT_FILE),
            ("user-only", SOURCE_PROJECT),
            ("project-only", SOURCE_PROJECT_FILE),
            ("actionable", SOURCE_BUILTIN),
        ] {
            let r = loader.get(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(loader.source(name), source, "{name} source");
            if source != SOURCE_BUILTIN {
                assert_eq!(r.description, source, "{name} description");
            }
        }
        assert_eq!(loader.path("user-only"), "", "a map recipe has no path");
        assert_eq!(
            loader.path("default"),
            dir.join(".beads")
                .join("recipes")
                .join("default.yaml")
                .to_string_lossy()
        );
    }

    #[test]
    fn project_recipe_files_register_one_recipe_each() {
        let dir = scratch("projectfiles");
        let recipes_dir = dir.join(".beads").join("recipes");
        // One recipe per file; the stem names it unless the file says otherwise.
        write(
            &recipes_dir.join("sprint.yaml"),
            "description: Current sprint work\nfilters:\n  status: [open, in_progress]\n  tags: [sprint]\nsort:\n  field: priority\n  secondary:\n    field: updated\n    direction: desc\nview:\n  max_items: 10\n",
        );
        write(
            &recipes_dir.join("review.yml"),
            "name: sprint-review\ndescription: Named by the file, not the stem\nfilters:\n  status: [closed]\n",
        );
        // A misspelt filter key must be reported by name, never silently dropped.
        write(
            &recipes_dir.join("bad.yaml"),
            "description: typo\nfilters:\n  statuses: [open]\n",
        );
        // Non-recipe entries are ignored.
        write(&recipes_dir.join("README.md"), "# not a recipe\n");
        std::fs::create_dir_all(recipes_dir.join("archive")).expect("archive dir");
        // The project map is lower precedence than a project file of the same name.
        write(
            &dir.join(".bv").join("recipes.yaml"),
            "recipes:\n  sprint:\n    description: From the project map\n  map-only:\n    description: Only in the map\n",
        );

        let mut loader = Loader::new()
            .with_user_path(
                dir.join("no-user-config.yaml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .with_project_dir(dir.to_string_lossy().into_owned());
        loader.load().expect("load");

        let sprint = loader.get("sprint").expect("sprint from the project file");
        assert_eq!(sprint.name, "sprint");
        assert_eq!(sprint.description, "Current sprint work");
        assert_eq!(loader.source("sprint"), SOURCE_PROJECT_FILE);
        assert_eq!(
            loader.path("sprint"),
            recipes_dir.join("sprint.yaml").to_string_lossy()
        );
        let secondary = sprint
            .sort
            .secondary
            .as_deref()
            .expect("secondary survived");
        assert_eq!(secondary.field, "updated");
        assert_eq!(sprint.view.max_items, 10);

        let review = loader.get("sprint-review").expect("named by the file");
        assert_eq!(review.description, "Named by the file, not the stem");
        assert!(
            loader.get("review").is_none(),
            "the stem is not registered when the file names itself"
        );
        assert_eq!(loader.source("map-only"), SOURCE_PROJECT);
        assert!(loader.get("bad").is_none(), "unknown key must not load");
        assert!(
            loader
                .warnings()
                .iter()
                .any(|w| w.contains("bad.yaml") && w.contains("statuses")),
            "{:?}",
            loader.warnings()
        );

        let summary = loader
            .list_summaries()
            .into_iter()
            .find(|s| s.name == "sprint")
            .expect("sprint in summaries");
        assert_eq!(summary.source, SOURCE_PROJECT_FILE);
        assert_eq!(
            summary.path,
            recipes_dir.join("sprint.yaml").to_string_lossy()
        );

        // And the recipe actually applies.
        let issues = vec![
            issue("s-1").priority(2).labels(&["sprint"]),
            issue("s-2").priority(1).labels(&["sprint"]),
            issue("s-3")
                .status(bv_core::model::Status::Closed)
                .priority(0)
                .labels(&["sprint"]),
            issue("s-4").priority(0),
        ];
        let got = crate::apply::apply(&issues, &crate::apply::Metrics::default(), sprint, None)
            .expect("apply sprint");
        assert_eq!(
            got.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["s-2", "s-1"]
        );
    }

    #[test]
    fn path_arguments_are_recognised_by_extension() {
        for arg in [
            "sprint.yaml",
            "SPRINT.YML",
            "./x.yaml",
            "/abs/path/x.yml",
            " a.yaml ",
        ] {
            assert!(is_path_argument(arg), "{arg} should be a path");
        }
        for arg in ["actionable", "high-impact", "sprint.yaml.bak", "yaml", ""] {
            assert!(!is_path_argument(arg), "{arg} should be a name");
        }
    }

    #[test]
    fn resolve_loads_a_path_or_looks_up_a_name() {
        let dir = scratch("resolvepath");
        let mut loader = Loader::new()
            .with_user_path(
                dir.join("no-user-config.yaml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .with_project_dir(dir.to_string_lossy().into_owned());
        loader.load().expect("load");

        // A path outside any recipes dir, unnamed: the stem names it.
        let unnamed = dir.join("somewhere").join("x.yaml");
        write(
            &unnamed,
            "filters:\n  status: [open]\nsort:\n  field: updated\n  direction: desc\n",
        );
        let r = loader.resolve(&unnamed.to_string_lossy()).expect("resolve");
        assert_eq!(r.name, "x");
        assert_eq!(r.filters.status, ["open"]);
        assert_eq!(r.sort.field, "updated");

        // The file's own name wins over the stem; .yml is accepted.
        let named = dir.join("named.yml");
        write(&named, "name: custom-name\ndescription: d\n");
        assert_eq!(
            loader
                .resolve(&named.to_string_lossy())
                .expect("resolve")
                .name,
            "custom-name"
        );

        // Names still resolve.
        let r = loader.resolve("actionable").expect("resolve by name");
        assert_eq!(r.name, "actionable");

        // A missing path is a clear error naming the path.
        let missing = dir.join("missing.yaml");
        let err = loader
            .resolve(&missing.to_string_lossy())
            .expect_err("missing path");
        let text = err.to_string();
        assert!(text.contains("recipe file not found"), "{text}");
        assert!(
            text.contains(&missing.to_string_lossy().into_owned()),
            "{text}"
        );

        // An unknown name carries the available names.
        let err = loader.resolve("nope").expect_err("unknown name");
        let ResolveError::Unknown(unknown) = err else {
            panic!("expected UnknownRecipeError");
        };
        assert_eq!(unknown.name, "nope");
        assert!(!unknown.available.is_empty());
        assert_eq!(unknown.to_string(), "unknown recipe \"nope\"");

        // Unknown keys fail with the key named.
        let bad = dir.join("bad.yaml");
        write(&bad, "filters:\n  statuses: [open]\n");
        let text = loader
            .resolve(&bad.to_string_lossy())
            .expect_err("unknown key")
            .to_string();
        assert!(text.contains("statuses"), "{text}");
        assert!(text.contains(&bad.to_string_lossy().into_owned()), "{text}");

        // Unusable values fail through validation.
        let karma = dir.join("karma.yaml");
        write(&karma, "sort:\n  field: karma\n");
        assert!(loader
            .resolve(&karma.to_string_lossy())
            .expect_err("bad sort field")
            .to_string()
            .contains("sort.field \"karma\""));

        // A recipes: map handed in as a path is rejected (it is not a single recipe).
        let map_file = dir.join("map.yaml");
        write(&map_file, "recipes:\n  a:\n    description: x\n");
        assert!(loader
            .resolve(&map_file.to_string_lossy())
            .expect_err("map file")
            .to_string()
            .contains("recipes"));

        // Path recipes are not registered in the loader.
        assert!(loader.get("x").is_none() && loader.get("custom-name").is_none());
    }

    #[test]
    fn a_discovered_recipe_with_invalid_values_is_skipped_with_a_warning() {
        for (field, yaml) in [
            ("view.columns", "view:\n  columns: [assignee_typo]\n"),
            ("view.group_by", "view:\n  group_by: guessed\n"),
            ("view.truncate_title", "view:\n  truncate_title: -1\n"),
            ("view.collapsed", "view:\n  collapsed: true\n"),
            ("metrics", "metrics: [pagerank_typo]\n"),
        ] {
            let dir = scratch("invalidpres");
            let path = dir.join("invalid.yaml");
            write(&path, &format!("name: invalid\n{yaml}"));
            let mut loader = Loader::new()
                .with_user_path(dir.join("none.yaml").to_string_lossy().into_owned())
                .with_project_dir(dir.to_string_lossy().into_owned());
            loader.load().expect("load");
            assert!(
                loader.resolve(&path.to_string_lossy()).is_err(),
                "explicit invalid recipe must fail with field {field}"
            );

            write(
                &dir.join(".beads").join("recipes").join("invalid.yaml"),
                &format!("name: invalid\n{yaml}"),
            );
            let mut loader = Loader::new()
                .with_user_path(dir.join("none2.yaml").to_string_lossy().into_owned())
                .with_project_dir(dir.to_string_lossy().into_owned());
            loader.load().expect("load");
            assert!(
                loader.get("invalid").is_none()
                    && loader.warnings().iter().any(|w| w.contains(field)),
                "discovered invalid recipe was accepted or silently ignored: {:?}",
                loader.warnings()
            );
        }
    }

    #[test]
    fn load_default_reads_the_ambient_locations() {
        // Whatever the machine has, the builtins are always present and the
        // call must not fail on a missing user or project file.
        let loader = load_default().expect("load_default");
        assert!(loader.get("default").is_some());
    }
}
