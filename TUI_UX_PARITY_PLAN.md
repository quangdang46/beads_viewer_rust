# TUI UX Parity Plan — closing the gap to Go `bv`

> **STATUS UPDATE (2026-09-07):** All six phases (A–F) plus the four
> polish-tier leftovers (G13–G15, AgentPromptModal, velocity_comparison)
> have shipped — see §7–§12. Nothing from §2's scope remains open except
> the byte-parity threshold re-audit note at the end of §11 (auditing,
> not UX).

> Scope: **interactive TUI only** (`crates/bv-tui` + the TUI-launch path in `crates/bv`). Robot/CLI JSON parity is tracked separately in `COMPREHENSIVE_PLAN_FOR_FORT_BEADS_VIEWER.md` (~33/47 `--robot-*` primaries real as of that doc).
>
> **Methodology:** every claim below is verified directly against `crates/bv-tui/src/**/*.rs` as of 2026-09-07, plus the upstream Go source (`Dicklesworthstone/beads_viewer`, `pkg/ui/`) via DeepWiki. **This supersedes the "Confirmed remaining gaps" section of `COMPREHENSIVE_PLAN_FOR_FORT_BEADS_VIEWER.md`**, which was last edited 2026-09-04 17:58 — most `bv-tui` view files were written or rewritten *after* that timestamp (2026-09-04 22:43 through 2026-09-06 11:21) and the doc's line-count claim ("~2,940 lines vs Go's ~24,000+") is stale; actual current size is **7,211 lines**. Do not trust that section going forward — trust this one, and re-verify against source before relying on any single line here after further changes land.
>
> **Non-goal of this document:** re-litigate robot CLI gaps, or attempt byte-identical Go feature-parity in every internal system (cass integration, bubbletea widget internals, etc.). The goal is **1:1 *user-facing* UX** — every view, keybinding, and interaction a person driving the TUI would notice — not internal architecture parity.

---

## 1. Verified current state (2026-09-07)

### 1.1 What's real and working

All 12 `ViewMode` variants (`List, Board, Tree, Graph, FlowMatrix, Attention, Insights, Alerts, TimeTravel, Sprint, Tutorial, Actionable`) have a live render arm in `lib.rs` — **none fall through to a blank/List placeholder**. This directly contradicts the stale doc's claim that Graph and TimeTravel "render nothing." Verified real, with genuine data wiring:

| View | File | Notes |
|---|---|---|
| List | `lib.rs` (default render path) | filter modes, sort modes, split view |
| Detail | `detail.rs` (344 lines) | split-pane + full-screen |
| Board (Kanban) | `views/board.rs` | swimlane grouping (status/priority/type), cycled with `s` — **but see §2.3, key collides with sort-mode `s`** |
| Tree | `views/tree.rs` + `crate::views::tree::build_tree_nodes` | real parent-child builder, matches Go `tree.go` semantics per prior fix |
| Graph | `views/graph.rs` (554 lines) | ASCII ego-graph, PageRank/Betweenness/HITS panel, node list |
| FlowMatrix | `views/flow_matrix.rs` (167 lines) | backed by real `bv_analysis::label_health::CrossLabelFlow` |
| Attention | `views/attention.rs` (125 lines) | backed by real `LabelAttentionScore` |
| Insights | `views/insights.rs` | 6-panel metric view |
| Alerts | `views/alerts.rs` | drift severity list |
| Sprint | `views/sprint.rs` (450 lines) | burndown, at-risk items, from `.beads/sprints.jsonl` |
| Tutorial | `tutorial.rs` (279 lines) | multi-page walkthrough |
| Actionable | `actionable.rs` (275 lines) | — |
| History (rendered under `ViewMode::TimeTravel`, see §2.1) | `views/history.rs` (446 lines) | bead↔commit correlation, 2 modes, confidence filter |

Correlation, search-scoring, and label-health *algorithms* backing these views are real (already tracked in the other plan doc); this document only tracks the **rendering/interaction layer**.

### 1.2 Confirmed gaps (code-verified, not guessed)

| # | Gap | Evidence | Go behavior (target) |
|---|---|---|---|
| G1 | **`ViewMode::TimeTravel` is not Time-Travel — it's an alias for the History view.** `t` toggles a view that renders `views::history::render_history`, the same bead↔commit correlation view as intended for a "History" key. Go's actual Time-Travel (`focusTimeTravelInput`) is a *revision-input prompt* → `SnapshotDiff` (new/closed/modified issues at a past git rev). There is no revision prompt, no diff computation, no `T` "quick HEAD~5" anywhere in `bv-tui`. | `lib.rs:1267-1276`, `lib.rs:779-786` | `t` → prompt for revision → Enter submits → diff view (new/closed/modified issues); `T` → instant `HEAD~5` diff |
| G2 | **Label Dashboard doesn't exist as a view.** Go's `focusLabelDashboard` (`[` key) shows per-label health scores (Healthy/Warning/Critical) computed from velocity, staleness, blocked-ratio, work-distribution, with a health-detail modal (`h`) and drilldown overlay (`d`) listing that label's issues. `bv-tui`'s Attention view is a *different* Go view (`pkg/ui/attention.go`) — label ranking by attention score, not label health. Both back onto the same `bv_analysis::label_health` module already, so this is render-layer-only work. | No `LabelDashboard` variant in `ViewMode` enum (`lib.rs:181-194`); no `label_dashboard.rs` in `views/` | New view, `[` key, health badge per label, `h` detail modal, `d` drilldown |
| G3 | **Pickers are dead code.** `views/pickers.rs` defines `LabelPicker`, `RecipePicker`, `RepoPicker` — real logic, unit-tested (`label_picker_navigation`, `label_picker_filter`, `recipe_picker_navigation`). None are reachable at runtime: `App.label_picker: Option<LabelPicker>` is declared and initialized to `None` and never set to `Some(..)` anywhere; `RecipePicker`/`RepoPicker` aren't referenced in `lib.rs` at all. Current substitutes are `cycle_label_filter()`/`cycle_repo_filter()` — one-key cycle-through, not the list+fuzzy-filter popup Go has. Recipe application (`'` key, apply a saved filter/action recipe) has **no substitute at all** — entirely unreachable. | `lib.rs:150,413`; `grep` shows zero other references | `l`/`[label]` → filterable popup list, Enter applies; `'` → recipe popup, Enter applies recipe; `w`/repo picker → same pattern (Go), vs bvr's plain cycle |
| G4 | **Priority Hints don't exist at runtime.** Documented in `keybindings.rs`'s registry (`("p", "Toggle priority hints")`, `lib.rs`'s own comment header) but there is no `KeyCode::Char('p')` arm anywhere in `handle_key`, no `PriorityHints` state, no render path. Purely aspirational documentation. | `keybindings.rs:114`; absent from `lib.rs` `match code` arms (full list checked: `q,j,k,o,c,r,/,b,i,` `` ` ``,`;,t,!,f,A,E,G,P,F,a,s,Enter,x,C,O,?,S,L,w`) | Overlay/status message: count of priority-vs-graph-metric misalignments, or "no misalignments" |
| G5 | **No live/auto reload.** Go watches `.beads/issues.jsonl` via `fsnotify` and refreshes automatically on change. `bv-tui` only has `Ctrl+R` → `reload_from_disk()`, a manual pull. No file watcher exists in the crate (`notify` is not a dependency of `bv-tui`). | `lib.rs:929-933` (`handle_ctrl_key`); no `notify`/watcher usage in `bv-tui/src/**` | Background `notify` watcher (already a planned dep per the workspace-level plan doc's crate-mapping table) triggers the same reload path `Ctrl+R` already calls |
| G6 | **No search-mode split.** `/` performs a hardcoded case-insensitive **substring** match on `id`+`title` only (`apply_search`, `lib.rs:562-575`). Go has two distinct entry points: `/` fuzzy search (sahilm/fuzzy → Rust target `nucleo-matcher`, already the planned dependency) and `Ctrl+S` semantic search (vector similarity — and `bv-search`'s `cosine_similarity`/hybrid scorer is *already implemented and tested*, just wired only to the `--robot-search` CLI path, not the TUI). Neither fuzzy ranking nor semantic search is reachable from the TUI. | `lib.rs:562-575`; `grep -n "SearchMode\|fuzzy\|semantic"` → zero hits in `bv-tui` | `/` ranks by fuzzy score (not just filters by substring); `Ctrl+S` opens semantic search using the existing `bv-search` engine |
| G7 | **`agent_prompt_modal.rs`, `update_modal.rs`, `views/velocity_comparison.rs` are unreferenced dead code.** Each defines a real `struct` (`AgentPromptModal`, `UpdateModal`, velocity comparison rendering) but `grep` across `bv-tui/src` shows **zero usages outside their own definition file** — not constructed, not stored on `App`, no keybinding reaches them, no render arm calls them. | `grep -rn "AgentPromptModal\|UpdateModal" crates/bv-tui/src` → only definition sites; `velocity_comparison` not in `ViewMode` enum | Update-available banner/modal (`bvr --version` vs latest release) with agent-prompt copy-to-clipboard shortcut; velocity trend view reachable from Sprint |
| G8 | **`keybindings.rs` registry has drifted from runtime `handle_key` and is incomplete.** Concrete mismatches: <br>• Registry documents `"g" → "Toggle graph view"`; runtime actually binds **uppercase `G`** (`lib.rs:819`). Lowercase `g` is unbound. <br>• Registry has no entry at all for Sprint-toggle (`P`, `lib.rs:835`) or Actionable-toggle (`F`, `lib.rs:843`) under `Focus::List`. <br>• `Focus` enum (`keybindings.rs:10-24`) has no `TimeTravel` or `Actionable` variant, so `bindings_for()` returns empty for those contexts even once G1 is fixed. <br>• `build_default_registry()` only populates `Focus::List, Detail, Sprint, Graph, History` — **Board, Tree, Insights, Alerts, FlowMatrix, Attention, Tutorial have zero registered bindings** despite being real, working views with their own internal navigation. This directly degrades the in-app `?` help overlay and `shortcuts_sidebar.rs`, which both read from this registry — a user pressing `?` inside Board/Tree/Insights/etc. today sees nothing or stale info. | `keybindings.rs:10-24, 70-211`; cross-checked against `lib.rs` `match code` arms | Every reachable view has an accurate, complete keybinding entry; help overlay always matches runtime behavior |
| G9 | **No keybinding customization.** No config-driven remap exists in either engine. Matches Go, which the original plan doc already flagged as "low priority" / no Rust counterpart planned. | — | *(carried forward as an explicit non-goal, not a gap — see §4)* |

### 1.3 Key finding: most remaining work is wiring, not new algorithms

Every backing data structure for G2–G4 and G6–G7 already exists and is tested (`bv_analysis::label_health`, `bv-search::cosine_similarity`, `views::pickers::{LabelPicker,RecipePicker,RepoPicker}`, `AgentPromptModal`, `UpdateModal`, `velocity_comparison`). This changes the shape of the plan: **Phase A below is connective work with almost no new algorithm risk**, and should be sequenced first — it's the highest ROI in the whole list.

---

## 2. Phased plan

Ordered by (a) dependency, (b) ROI (dead-code wiring first — no new logic, lowest risk), (c) user-visible severity.

### Phase A — Wire the dead code (no new logic; pure connection work)

Goal: every struct/module that already exists and is tested becomes reachable from the running TUI.

1. **A1 — Pickers (G3).** Add `LabelPicker`/`RecipePicker`/`RepoPicker` as real `Option<T>` overlay state on `App` (pattern already exists for `label_picker`, just needs an actual `Some(..)` assignment + render arm + key handler). Bind: `l` → label picker (replace `cycle_label_filter` call site or keep as a fallback), `'` → recipe picker (currently completely unbound), consider keeping `w`'s cycle behavior for repo filter *or* upgrading to `RepoPicker` — user call, see §5 Q1.
2. **A2 — Update modal + agent-prompt modal (G7 half).** Wire `UpdateModal` behind whatever "check for update" trigger Go uses (version check against latest release; reuse whatever the `bv` CLI updater logic already does, if any exists in `crates/bv`). Wire `AgentPromptModal` to its trigger key (check Go docs for the binding — likely a copy-agent-prompt action from Detail or List focus).
3. **A3 — Velocity comparison (G7 half).** Add a way to reach it from Sprint view (Go: likely a sub-toggle inside Sprint, not a separate `ViewMode` — verify against `pkg/ui/velocity_comparison.go` call sites before deciding whether it's a `ViewMode` variant or a Sprint sub-state).
4. **A4 — Keybindings registry drift (G8).** Fix `"g"` → `"G"` in the registry text (or, better, generate the registry from the same match arms `handle_key` uses, so drift becomes structurally impossible — see §5 Q2). Add missing `Focus::TimeTravel`/`Focus::Actionable` variants. Populate bindings for Board, Tree, Insights, Alerts, FlowMatrix, Attention, Tutorial by reading their actual `handle_key`-equivalent logic in each view file.

**Acceptance:** `cargo clippy --workspace --all-targets -- -D warnings` clean (dead-code warnings for these structs disappear naturally once wired — don't just `#[allow(dead_code)]` them). New integration tests in `bv-tui`'s existing test module style (see `quit_sets_flag`, `sort_mode_changes_order` pattern) for each newly-reachable interaction. Manually drive `bvr` against `.beads/` and confirm each key reaches its view.

### Phase B — Time-Travel, for real (G1)

This is currently the most user-visible bug: pressing `t` looks like it works (view changes) but shows the *wrong feature*. Two sub-decisions needed before implementation — see §5 Q3.

1. Rename the current History-aliased-as-TimeTravel path back to a correctly-labeled `ViewMode::History` (or confirm one already exists distinct from `TimeTravel` — currently `TimeTravel` is the *only* way to reach `render_history`, so a plain History key/view may not exist standalone either; audit `lib.rs` once more before starting).
2. Build the real Time-Travel: revision-input prompt (`focusTimeTravelInput` equivalent — reuse whatever text-input pattern the label/recipe pickers from Phase A establish), `t` enters prompt, `T` is instant `HEAD~5`, `Enter` submits, `Esc` cancels.
3. Backing diff logic: check whether `bv-core`/`bv-analysis` already has a `SnapshotDiff`-equivalent (search for "diff" in those crates — `--robot-diff` is listed as an *undispatched* robot primary in the other plan doc, so this may need the same underlying diff engine work as that CLI gap; **coordinate so the algorithm is built once and shared** between `--robot-diff` and the TUI Time-Travel view rather than duplicated).

**Acceptance:** `t` + revision → correct new/closed/modified diff, golden-tested against a known git history fixture the way other components are goldened.

### Phase C — Label Dashboard (G2)

1. New `ViewMode::LabelDashboard`, `[` key (currently unbound — confirmed no collision).
2. Render: per-label health badge (Healthy/Warning/Critical) computed from the same `bv_analysis::label_health` module already backing Attention/FlowMatrix — check whether a composite health-score function already exists there (the other plan doc mentions "composite per-label health" was ported) or needs a small render-side mapping.
3. `h` → detail modal (velocity/staleness/blocked-ratio/work-distribution breakdown for one label).
4. `d` → drilldown overlay (issue list for that label, filterable, `Enter` jumps to List filtered by that label).

**Acceptance:** matches the health-score thresholds/labels the Go docs describe (Healthy/Warning/Critical) — pull exact thresholds from `pkg/analysis/label_health.go` if not already ported per the correlation plan doc's note that "composite per-label health" is done.

### Phase D — Search upgrade (G6)

1. **Fuzzy `/`:** swap `apply_search`'s `.contains()` substring check for `nucleo-matcher` scoring (already the planned dependency per the workspace crate-mapping table — not a new decision, just not yet pulled into `bv-tui`'s `Cargo.toml`). Rank results by score instead of filtering to substring matches only.
2. **Semantic `Ctrl+S`:** new key handler, new lightweight overlay/mode, calls into `bv-search`'s existing `cosine_similarity`/hybrid scorer (already used by `--robot-search`) against the loaded issue set. No new algorithm — this is Phase A-grade wiring, just listed here because it's naturally paired with fuzzy search UX work.

**Acceptance:** existing `search_filters_by_title` test still passes with fuzzy ranking (contains-match issues should still surface, just possibly reordered by score); new test for semantic search returning issues by embedding similarity.

### Phase E — Priority Hints (G4)

1. Small: `p` key, computes priority-vs-graph-metric misalignment count (check whether `bv-analysis` already exposes this — the robot CLI likely has an equivalent signal already, e.g. via triage recommendations; reuse rather than reimplement).
2. Status-line message or small overlay per Go's minimal behavior (Go itself is described as just a status message, not a full view — keep it that lightweight).

### Phase F — Live reload (G5)

1. Add `notify` (already the workspace's planned dependency for this per the top-level crate-mapping table — not a new choice) to `bv-tui`, watch `.beads/issues.jsonl` (and `sprints.jsonl` if Sprint view should also live-update).
2. On event, call the same `reload_from_disk()` path `Ctrl+R` already uses — this is the smallest-risk item in the whole plan since the reload logic itself is proven; only the trigger is new.
3. `PollWatcher` fallback for network filesystems, matching the workspace plan's existing NFS/SMB/SSHFS/FUSE caveat.

**Acceptance:** editing `.beads/issues.jsonl` externally while `bvr` is running updates the visible list without a manual keypress, within a reasonable debounce window (avoid thrashing on rapid writes — debounce like Go's watcher does, check its debounce interval if documented).

---

## 3. Suggested sequencing

```
Phase A (wire dead code) ─┬─→ Phase B (Time-Travel) ──→ golden diff tests
                          ├─→ Phase C (Label Dashboard)
                          ├─→ Phase D (search upgrade)
                          ├─→ Phase E (priority hints)
                          └─→ Phase F (live reload)
```

A has no dependents blocking it and touches the least risky code (assembling already-tested pieces) — do it first regardless of what else gets prioritized. B, C, D, E, F are independent of each other and of A's completion in principle, but A should land first because it's the cheapest confidence-builder and because A4 (fixing the keybindings registry) makes every subsequent phase's key-choice decisions auditable against a trustworthy source instead of the currently-drifted one.

---

## 4. Explicit non-goals (carried forward, not overlooked)

- **Keybinding customization** (G9) — Go itself has minimal support for this per the original port plan; not worth building beyond Go's own bar.
- **cass session integration** inside History view — explicitly scope-cut in `views/history.rs`'s own doc comment; no reason to revisit unless a user specifically needs it.
- **File tree panel + transition animations** in History view — same scope cut, cosmetic/secondary.
- **PNG/SVG graph export** — belongs to `bv-export`, not the TUI Graph *view*; already tracked (or explicitly deferred) in the other plan doc.
- **Byte-identical bubbletea/lipgloss visual styling** — ratatui will never pixel-match a different TUI framework's rendering; "1:1 UX" here means *feature and interaction* parity, not identical ANSI output.

---

## 5. Open questions before implementation starts

1. **Q1 (Phase A1):** For the repo filter, should `w` keep its current one-key cycle behavior *and* gain a `RepoPicker` overlay as an additional entry point, or should the picker fully replace the cycle? (Go has a picker; bvr's cycle is a "documented-simplified" substitute per the existing scope-cut convention — decide whether to keep both or retire the simpler one.)
2. **Q2 (Phase A4):** Should `keybindings.rs`'s registry be generated/derived from `handle_key`'s match arms (single source of truth, prevents future drift) or kept hand-maintained but disciplined? Given G8 shows hand-maintenance has already drifted once, a derived/macro or test-enforced approach (e.g., a test that walks both and asserts they agree) is recommended — but it's a real design choice, not just a fix.
3. **Q3 (Phase B):** Does a plain "History" view/key need to exist separately once Time-Travel is corrected, or was `t`-showing-History always meant as a temporary stand-in with no separate History keybinding intended? Needs one more read of `lib.rs`'s full key list (done above) cross-referenced with whether users currently rely on `t` for what is *actually* History-view access — changing this is a behavior change for anyone already using `t` today, not just a bug fix.
4. **Q4 (Phase B3):** Should the Time-Travel diff engine be built once and shared with the still-undispatched `--robot-diff` CLI command (noted as a gap in `COMPREHENSIVE_PLAN_FOR_FORT_BEADS_VIEWER.md`), or built independently? Sharing avoids duplicating diff logic but couples this plan's timeline to that CLI gap's.

---

## 6. Testing/acceptance strategy (applies to every phase)

- Every wired interaction gets a `#[test]` in the same style as existing ones (`quit_sets_flag`, `sort_mode_changes_order`, `search_filters_by_title` in `crates/bv-tui/src/lib.rs`'s test module) — construct an `App`, drive `handle_key`, assert state.
- Anything touching git history (Time-Travel diff) needs a fixture repo + golden output, matching the project's existing golden-testing convention (`golden/` at repo root) rather than ad hoc assertions.
- After each phase: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`, then a manual smoke pass with `bvr` against the real `.beads/` repo (41-issue fixture already in this repo) — same verification loop already used for the install/lint pass.
- No phase should introduce new dead code — if a struct is added, it must be reachable by the end of that same phase's commit(s).

---

## 7. Phase A results (shipped 2026-09-07)

All items verified with `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` (green — 55 bv-tui unit tests + 7 integration tests, workspace total unaffected elsewhere) and a manual `bvr` install + `--robot-triage` smoke check against the real `.beads/` repo (identical `data_hash`, confirming zero robot/CLI regression from TUI-only changes).

### Shipped

- **A1 (pickers):** `LabelPicker` wired to `l` (builds real label→count from loaded issues; `L` one-key cycle kept as a fast-path alternative, not removed). `RepoPicker` wired to `w`, **replacing** the old one-key `cycle_repo_filter` per Q1's resolution (bv has a real picker; bvr should too). `RecipePicker` wired to `'`, backed by the 6 real built-in recipes Go ships (`triage`, `release-cut`, `blocked-review`, `dependency-risk`, `quick-wins`, `stale` — confirmed via DeepWiki against `defaults/recipes.yaml`), each implemented with real semantics using existing analysis primitives (`bv_analysis::blocker_chain::{is_actionable,open_blockers}`, `graph_metrics.betweenness`, `updated_at` staleness) — no new algorithms, all wiring. User/project YAML recipe loading remains a documented scope cut (matches the existing convention already used elsewhere in this port).
- **A2 (modals):** `UpdateModal` wired to `U` (only opens when `update_tag` is `Some`, matching Go's dual trigger — auto-detection already existed and populated `update_tag`; the modal itself was the missing half). `AgentPromptModal` **not** wired this pass — Go shows it automatically on `AGENTS.md` detection with no dedicated keybinding, which needs a touch more design than the other items; deferred, not forgotten.
- **A3 (velocity comparison):** **not** wired this pass — deferred with the others below.
- **A4 (registry drift, `keybindings.rs`):** Fixed `"g"` -> `"G"` (the exact drift found in the audit). Added `Focus::TimeTravel` and `Focus::Actionable` variants. Populated real bindings for every previously-empty Focus (Board, Tree, Insights, Alerts, FlowMatrix, Attention, Tutorial, Actionable, TimeTravel, History) — each entry describes what's *actually* wired today, not aspirational Go behavior (e.g. Board's entry says "shared list cursor" rather than claiming column-switching that doesn't exist — see G14 below). Removed the `p` "priority hints" line since G4 (priority hints) still has zero runtime implementation — leaving it would have re-introduced exactly the kind of drift this phase exists to fix. Added two regression tests: `every_focus_has_at_least_one_binding` (structural guard — any future `ViewMode`/`Focus` added without registry entries now fails a test instead of silently breaking `?`) and `graph_toggle_is_uppercase_g_matching_runtime` (pins the specific G8 finding so it can't drift back). This resolves Q2 in favor of "hand-maintained but test-enforced" rather than full codegen.
- **Dynamic help overlay:** the `?` overlay was a *third*, separately hand-maintained hardcoded `Vec<Line>` (on top of `keybindings.rs`'s registry and the runtime `handle_key` match arms) — a third source the same drift could hide in. It's now generated live from `app.key_registry.bindings_for(focus_for_view(app.current_view))`, so there's exactly one source of truth for "what does `?` show."
- **Overlay visibility bug (found mid-implementation, not in original audit):** `render()`'s help/sidebar overlay code lived *after* a giant `match app.current_view { ... return; ... }` — meaning every non-List view (`Board`, `Tree`, `Graph`, `Insights`, `Alerts`, `FlowMatrix`, `Attention`, `Actionable`, `Sprint`, `History`, `TimeTravel` — i.e. everything except `List`) `return`ed *before* that code ever ran. Pressing `?` while inside any of those 11 views silently did nothing. Fixed by extracting a `render_overlays()` function (help + update modal + all 3 pickers) and calling it from every branch instead of only the List fallthrough path.

### Real bugs found and fixed mid-implementation (not in the original §1.2 audit)

- **G11 — History's j/k navigation was unreachable dead code.** The `else if self.current_view == ViewMode::TimeTravel { history.move_bead_down() }` branch was nested *inside* the `ViewMode::Graph` arm's inner if/else — meaning it could only execute when `current_view` was simultaneously `Graph` and `TimeTravel`, which is impossible. In practice, pressing `j`/`k` while viewing history silently fell through to the *last* `else if` clause and moved the hidden main-list cursor instead of the bead cursor actually on screen. Fixed by moving both to a correctly-scoped top-level `else if current_view == ViewMode::History` branch (in both the `j` and `k` handlers — the bug only existed in `j`'s branch; `k`'s branch had no History case at all, same root problem). Covered by a new regression test, `history_j_k_does_not_move_main_list_cursor`.
- **G12 — the History view's data was permanently empty.** `App::new` set `app.history = Some(HistoryState::build_from_beads(vec![]))` — an empty `Vec` — and *nothing else in the codebase ever populated it*. The 446-line `views/history.rs` renderer was fully real, but it was rendering a view that could never have data. This is why the original audit's claim "History view: real, working" was itself only half-true — the render layer was real; the data layer was silently disconnected. Fixed with `load_history_if_needed()`, called lazily on first `h` press (matching the original — never-implemented — code comment's stated intent), reusing the exact same `bv_correlation::correlator::{walk_commits, correlate}` pipeline `main.rs` already uses for `robot-history`/`robot-file-hotspots` etc. (`bv-correlation` added as a `bv-tui` dependency). One real bug caught and fixed in this new code during implementation: `CorrelatedCommit` carries a correlation `reason` string, not the original commit message — the first draft used `reason` as the displayed commit message by mistake; fixed by building a `sha -> message` lookup from the raw `CommitInfo` list before consuming it into `correlate`.
- **`t`/`T` no longer silently mislabel History as "Time-Travel".** Since History now has its own correct `h` binding with real data, `t`/`T` (Go's actual Time-Travel trigger) was repointed to an honest placeholder view ("Not yet implemented — see Phase B") instead of continuing to alias to History under the wrong name. This directly resolves the "one real bug, not just a gap" finding from the original audit (the "Key finding" note) — no interaction now silently does the wrong thing; anything not yet real says so.

### New History interactions wired (using already-existing, already-tested `HistoryState` methods that simply had no keybinding before)

`J`/`K` (commit up/down, via existing `move_commit_up`/`move_commit_down`), `v` (bead/git mode toggle, via existing `toggle_mode`), `c` (confidence-threshold cycle 0%->50%->80%, new inline logic since `HistoryState` had no dedicated cycle method — just a field), `y` (copy selected commit SHA to clipboard, via a new shared `copy_to_clipboard()` helper factored out of the pre-existing `copy_issue_to_clipboard`, so both call sites share one implementation).

### Newly found, NOT fixed this pass (deliberately out of scope — tracked for later)

- **G13 — clipboard copy has no Windows branch.** `copy_to_clipboard()` (and the pre-existing `copy_issue_to_clipboard` it was factored from) shells out to `pbcopy`/`wl-copy`/`xclip` — none of which exist on Windows, so `C` and the new `y` both silently fail with "Clipboard failed" on Windows (confirmed: this is the exact OS this repo's dev environment runs). The original architecture plan (`COMPREHENSIVE_PLAN_FOR_FORT_BEADS_VIEWER.md` section 2's crate-mapping table) already specifies `arboard` as the intended cross-platform clipboard crate for this port — this was never a case of "no plan," just not-yet-executed. Not fixed here to keep this pass's diff scoped to view/keybinding wiring rather than a dependency swap; tracked as a follow-up.
- **G14 — Board view has no column navigation, jump-to-column, or grouping cycle.** It currently only shares the List view's plain j/k cursor; Go's `h`/`l` column switching, `1`-`4` jump-to-column, and `s` grouping cycle (Status<->Priority<->Type — `SwimlaneMode` already exists in `views/board.rs` and is cycle-able in principle, it's just never wired to a key; render always hardcodes `SwimlaneMode::Status`) are all unimplemented. Documented honestly in the registry now rather than claimed.
- **G15 — Actionable and Tutorial views render their own selection cursor but have no j/k handler that moves it.** `app.actionable`'s state and equivalent Tutorial paging exist as real state but the global j/k handler doesn't special-case either view the way it does for FlowMatrix/Attention/Alerts/Graph/History, so their internal cursors are currently stuck. `tutorial.rs` does have its own `next_page`/`prev_page` — check whether those are reachable via some other key before assuming this needs new logic vs just a missing match arm.
- **AgentPromptModal, velocity_comparison** — still fully unreferenced (Phase A2/A3 originally scoped these in; deferred as noted above).
- **Phase B (real Time-Travel), C (Label Dashboard), D (search upgrade), E (priority hints), F (live reload)** — unchanged from the original plan above, **except** one load-bearing discovery for Phase B: `bv_analysis::diff::diff_issues(current, previous, diff_ref) -> DiffResult` **already exists** (it's the same algorithm `--robot-diff` needs, per the still-undispatched-CLI-primaries list in `COMPREHENSIVE_PLAN_FOR_FORT_BEADS_VIEWER.md`). Phase B's remaining work is materially smaller than originally scoped: no algorithm to design, just (1) fetch `issues.jsonl` content at a revision via `git show <rev>:.beads/issues.jsonl` piped through `bv_core::loader::parse_issues_with_options` (accepts any `impl Read`, so a `Cursor<Vec<u8>>` over the git-show output works directly), (2) a revision-input prompt UI (the label/recipe picker text-input pattern from this pass is a ready-made template), (3) a result-rendering view for `DiffResult`'s added/removed/changed lists. Recommend sharing this exact plumbing with `--robot-diff`'s dispatch (resolves Q4 in favor of "share") rather than building it twice.

### Q1-Q4 resolutions (for the record)

- **Q1:** Replaced `w`'s cycle with the real `RepoPicker`, per "bv behavior = source of truth."
- **Q2:** Hand-maintained registry, but now test-enforced (`every_focus_has_at_least_one_binding`, `graph_toggle_is_uppercase_g_matching_runtime`) — not full codegen, but drift can no longer land silently.
- **Q3:** Resolved as "yes, they're genuinely separate" — `h`/History and `t`/`TimeTravel` are now two independent, correctly-labeled views instead of one aliasing the other.
- **Q4:** Resolved as "share" — Phase B should reuse `bv_analysis::diff::diff_issues` for both the TUI and `--robot-diff` rather than building the diff logic twice.

---

## 8. Phase B results (shipped 2026-09-07)

Real Time-Travel, verified with `cargo fmt --all && cargo clippy
--workspace --all-targets -- -D warnings && cargo test --workspace` (green —
81 bv-tui unit tests incl. 7 new Time-Travel tests, 6 discovery_git tests
incl. 1 new hermetic git fixture test, golden gate still passing) plus a
manual `--robot-diff` smoke check (identical `data_hash`, zero-count diff vs
`HEAD~5` confirmed genuine via empty `git diff HEAD~5 -- .beads/issues.jsonl`,
non-zero diff confirmed vs `4aed089~1` with a real `open → closed` transition).

### Shipped

- **B1 (prompt + instant keys):** `t` opens a revision-input prompt
(`time_travel_prompt: Option<String>` on `App`, Go `focusTimeTravelInput`;
typing/Backspace edit, Enter submits, Esc cancels — same modal-intercept
pattern as the Phase A pickers). `T` diffs instantly vs `HEAD~5` with no
prompt. Empty-submit is a no-op (prompt stays open, no git call).
- **B2 (shared plumbing, Q4):** both the TUI (`App::run_time_travel`) and
`--robot-diff` (`run_robot_diff`) go through `bv_core::discovery::GitLoader::load_at`
(revision resolution + `git show` + tolerant JSONL parse — the same loader
`--as-of` already uses). The robot refactor maps failures back to the exact
legacy message (`Error: could not read issues at ref <ref>`, exit 1), so the
golden-covered CLI surface is byte-identical. No new algorithm anywhere:
`diff_issues` and `GitLoader` both pre-existed and were already tested.
- **B3 (render):** `render_time_travel` shows the prompt echo, then `+N/-N/~N`
summary with per-entry lines (`+ id`, `- id`, `~ id [old → new] [title]
[priority]`), an empty-diff message when the working set matches the ref,
and the error text on bad refs. Long diffs are windowed around the cursor to
fit the view height. `j`/`k` move a dedicated `time_travel_cursor` over the
flattened entry list (added, removed, changed); `esc` in the view returns to
List instead of quitting.
- **B4 (registry):** `Focus::TimeTravel` documents the real bindings
(`t`/`T`/`enter`/`esc`/`j`/`k`); the "not yet implemented" placeholder text
and enum comments are gone, so `?` inside the view is accurate.

### Tests

- 7 TUI tests: prompt open on `t`, typing/Backspace/Esc-cancel, empty-Enter
keeps prompt open, pure `apply_time_travel_result` added/removed/changed
counts, `j`/`k` moves diff cursor (not list cursor), `esc` returns to List
without quitting, `T` sets view + status without prompt. The old
`time_travel_toggle_shows_placeholder_not_history` test was replaced (behavior
contract changed, not re-pinned wording).
- 1 hermetic git test (`git_loader_load_at_sees_committed_snapshot_diff`):
builds a temp repo with two commits (A open → A closed + B new), asserts
`load_at("HEAD~1")`/`load_at("HEAD")` see the right snapshots and unknown
refs error. The `diff_issues` half over those snapshots is pinned on the
consumer side (TUI tests) to avoid a bv-core→bv-analysis dev-dependency.

### Still open (unchanged)

- Phase C (Label Dashboard), D (search upgrade), E (priority hints),
F (live reload) — as originally scoped.
- G13 (Windows clipboard), G14 (Board columns), G15 (Actionable/Tutorial j/k),
AgentPromptModal, velocity_comparison — untouched by this pass.

---

## 9. Phase C results (shipped 2026-09-07)

Label Dashboard, verified with `cargo fmt --all && cargo clippy
--workspace --all-targets -- -D warnings && cargo test --workspace` (green —
67 bv-tui unit tests incl. 6 new dashboard tests). TUI-only changes; no
robot/CLI surface touched.

### Shipped

- **C1 (view + key):** new `ViewMode::LabelDashboard` on `[` (verified
unbound — no `[`, `d`, or `LabelDashboard` references existed in `bv-tui`
before this pass), lazy-loaded on first open like History
(`load_label_health_if_needed`, deferred analysis cost). Worst-health-first
display order, dedicated `label_dashboard_cursor` on `j`/`k`, `esc` returns
to List instead of quitting.
- **C2 (badges, no new thresholds):** rows render badge + score + open/total
+ blocked-ratio from `bv_analysis::label_health::{compute_all_label_health,
health_level_from_score}` — the same module + Healthy ≥ 70 / Warning ≥ 40 /
Critical thresholds `--robot-label-health` uses. Zero new constants.
- **C3 (`h` detail modal):** velocity (closed 7d/30d, avg days to close,
score, trend), freshness (stale count, avg days since update, score), flow
(in/out deps, external blocked/blocking, score), work distribution
(open/closed/blocked + blocked-ratio). View-scoped `h` arm precedes the
global History toggle, so `h` inside the dashboard never leaves the view.
- **C4 (`d` drilldown):** picker-convention overlay (`j`/`k` navigate, other
chars substring-filter id+title, Backspace clears, `Enter` jumps to List
with `label_filter` set, `Esc` closes). Opening it closes the detail modal
(one overlay at a time).
- **C5 (registry):** new `Focus::LabelDashboard` with the real bindings, so
`?` inside the view is accurate; the existing
`every_focus_has_at_least_one_binding` guard covers the new variant.

### Tests

- 6 TUI tests: lazy open/load + toggle, `j`/`k` moves dashboard cursor (not
list cursor), `h` toggles detail without entering History, drilldown
filter-narrow + Enter-applies-filter, drilldown Esc closes in-view, view Esc
returns to List without quitting.

### Still open (unchanged)

- Phase D (search upgrade), E (priority hints), F (live reload) — as
originally scoped.
- G13 (Windows clipboard), G14 (Board columns), G15 (Actionable/Tutorial j/k),
AgentPromptModal, velocity_comparison — untouched by this pass.

---

## 10. Phase D results (shipped 2026-09-07)

Search upgrade, verified with `cargo fmt --all && cargo clippy --workspace
--all-targets -- -D warnings && cargo test --workspace` (green — 73 bv-tui
unit tests incl. 6 new search tests) plus a manual `--robot-triage` /
`--robot-diff` smoke check (identical `data_hash`, confirming zero
robot/CLI regression from TUI+dependency changes).

### Shipped

- **D1 (fuzzy `/`):** `apply_search` now ranks by `nucleo-matcher`
(`nucleo-matcher = "0.3"`, new `bv-tui` dependency — the planned crate, just
not previously pulled in) over `id + title` haystacks, score desc with id
asc tiebreak. Empty query still restores natural row order.
- **D2 (semantic Ctrl+S):** new `semantic_searching`/`semantic_query` mode
behind the existing `handle_ctrl_key` plumbing (`Ctrl+S`), ranking by
`bv_search::embedder::{hash_embed, cosine_similarity}` over
title+description — the exact text-mode engine `--robot-search` uses, no new
algorithm. Zero-score rows hidden, id asc tiebreak. `s/` status bar mirrors
the `/` bar shape with a distinct prefix. Esc clears+exits, Enter accepts
(back to standard filter order, same as fuzzy).
- **D3 (registry):** stale `"/"` text ("fuzzy ranking not yet implemented")
fixed; new `ctrl+s` entry under List plus a `Focus::Search` section for the
in-input keys, so `?` stays accurate.

### Real bug found mid-implementation (not in the original audit)

- **G16 — nucleo-matcher 0.3.1 panics on mixed-case needles.** An uppercase
needle char that can only match via case folding (e.g. `"Is"` vs `"issue"`)
passes the crate's prefilter but trips the optimal matcher's reject assert
(`fuzzy_optimal.rs`: "should have been caught by prefilter") — a hard panic
on a one-character-then-lowercase keystroke path, i.e. reachable by ordinary
typing. Workaround on our side: pre-fold only the needle to lowercase and
let `Config::DEFAULT` (ignore_case) fold the haystack, which avoids the
defective path while staying case-insensitive. Pinned by
`fuzzy_search_mixed_case_prefixes_never_panic` (12 ASCII prefixes/queries
must rank without panicking; `"ISSUE 2"` must still find exactly `T-2`).

### Tests

- 6 TUI tests: substring match surfaces ranked (`"issue 1"` → exactly
`[T-1]`), empty query restores all rows, semantic ranks token-overlapping
rows and hides the disjoint one, semantic Esc clears+exits, semantic Enter
accepts, mixed-case regression above.

### Still open (unchanged)

- Phase E (priority hints), F (live reload) — as originally scoped.
- G13 (Windows clipboard), G14 (Board columns), G15 (Actionable/Tutorial j/k),
AgentPromptModal, velocity_comparison — untouched by this pass.

---

## 11. Phase E + F results (shipped 2026-09-07, same day)

Verified the same way as every prior phase: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` green (76 bv-tui unit tests, up from 74, + 7 integration tests), `bvr` reinstalled and `--robot-triage` re-run against the real `.beads/` repo with an identical `data_hash` (no CLI regression).

### Phase E — Priority Hints (G4), shipped

`p` toggles `show_priority_hints`; on first toggle-on, lazily computes a suggested-priority map via `bv_analysis::impact::compute_impact_scores` + `bv_analysis::scoring::score_to_priority` — the exact same engine `--robot-priority` already uses (confirmed via `run_robot_priority` in `crates/bv/src/main.rs`), so this was wiring, not a new scoring algorithm. Status message matches Go's copy: `"Priority hints: ↑ increase ↓ decrease (N suggestions)"` / `"Priority hints: No misalignments detected"`. Per-row ↑/↓ arrows render next to the priority badge in the list (Go's `delegate.go` behavior), not just the status line. One deliberate scope note: the existing `--robot-priority` CLI path only ever surfaced *under*-prioritized issues (`suggested < current`); this counts *both* directions to match Go's bidirectional hint UI — same underlying primitives, just not filtered to one direction.

### Phase F — Live reload (G5), shipped — with one deliberate deviation from the original plan

The original plan (§2, Phase F) called for adding the `notify` crate as a background-thread file watcher. Implemented instead as **mtime polling piggybacked on the TUI's existing ~500ms terminal-event-poll tick** (`tui_event_loop`'s `event::poll(Duration::from_millis(500))`), via `App::check_for_reload()`. Rationale: same user-visible acceptance criterion ("editing `.beads/issues.jsonl` externally updates the TUI without a manual keypress") is satisfied with zero new dependencies, no background-thread lifecycle to manage, and no `PollWatcher`-vs-`RecommendedWatcher` fallback complexity — the tick already exists and already drives the freshness badge, so this is strictly fewer moving parts for the same result. `App::refresh_watch_target()` resolves the actual `.beads/*.jsonl` path via the existing `bv_core::discovery::{get_beads_dir, find_jsonl_path_with_warnings}` (same discovery Go's watcher targets — `.beads/issues.jsonl` specifically, not a generalized any-datasource watcher, matching Go's own documented scope); a changed mtime triggers the exact same `reload_from_disk()` path `Ctrl+R` already calls, so behavior on reload (full state reset) is unchanged and already-accepted, just now reachable automatically as well as manually. If discovery fails (no `.beads` dir, or the active datasource is SQLite rather than JSONL — out of scope, matching Go), watching is silently a no-op rather than a startup error.

If a "real" `notify`-based watcher (instant reload vs up-to-500ms latency, and coverage of SQLite-backed setups) turns out to matter later, revisit — but the 500ms-polling version is functionally equivalent for the stated acceptance criterion and was lower-risk to ship today.

### Everything from §2's six phases is now shipped except

- **AgentPromptModal, velocity_comparison** — still fully unreferenced (deferred twice now, from Phase A2/A3).
- **G13** (Windows clipboard — `arboard` migration), **G14** (Board column nav/grouping cycle), **G15** (Actionable/Tutorial internal j/k) — smaller polish items found during Phase A review, still open.
- Label Dashboard's Go-side `d` drilldown / `h` detail modal are real per Phase C, but re-verify against Go's exact health-score thresholds if byte-parity with `--robot-label-health` ever needs auditing (not done as part of this TUI-focused pass).
---

## 12. Cleanup pass results (shipped 2026-09-07, same day)

Closed the four polish-tier leftovers from §7/§11, verified with
`cargo fmt --all --check` + `cargo clippy --workspace --all-targets --
-D warnings` + `cargo test --workspace` green (bv-tui 76 → **85 unit** +
7 integration; zero failures workspace-wide) plus a `--robot-triage`
smoke run (exit 0, `data_hash` present — TUI-only diff, no CLI impact).

- **G13 (Windows clipboard):** `copy_to_clipboard` now tries `arboard = "3"`
  (new `bv-tui` dep — Win32 on Windows, X11/Wayland on Linux, pbcopy on
  macOS) first, falling back to the legacy `pbcopy`/`wl-copy`/`xclip`
  shell-out when arboard has no display to talk to (headless/SSH). Same
  call sites (`C`, `y`), same status messages.
- **G14 (Board columns):** real grouping behind all three `SwimlaneMode`s —
  Status keeps its 4 fixed columns, Priority is fixed P0–P4, Type is the
  sorted distinct `issue_type` values present in the rows (`ALL` fallback).
  `h`/`l` move a highlighted selected column, `1`–`9` jump, `s` cycles the
  grouping (resets column, status message names the mode). Board-scoped
  guarded arms precede the global `h`/`l`/`s` meanings, same pattern as the
  dashboard `h`. Render takes `(mode, selected)` instead of hardcoded
  `Status`.
- **G15 (Actionable/Tutorial j/k):** both route through the global `j`/`k`
  chain now (`ActionableState::move_up/down`, `TutorialState::next/prev_page`)
  without touching the shared list cursor. Tutorial renders its real pages
  full-area (the old static help-text arm — including its stale "g/G jump"
  line — is gone); backtick re-entry restarts at page 0. Two adjacent
  same-class bugs fixed in the same stroke: `F` lazy-builds
  `ActionableState` and `P` lazy-loads `SprintState` from
  `.beads/sprints.jsonl` (both were `None` forever, so neither view could
  ever show data — the G12 pattern a third and fourth time).
- **AgentPromptModal:** auto-shows once at real TUI startup when AGENTS.md
  is detected (cwd or repo root above `.beads/` — Go's trigger), `g`
  reopens manually (lowercase `g` was the only free mnemonic key; the G8
  regression test now pins `g` = prompts, `G` = graph). `j`/`k` navigate,
  `Enter` copies the command via the new clipboard path, `Esc` closes.
  Detection lives in `tui_event_loop`, not `App::new`, so unit tests keep a
  clean slate (constructor detection broke 20 tests mid-pass — caught by
  the suite, fixed same pass).
- **velocity_comparison:** `v` sub-toggle inside Sprint (free there; `v` is
  History-scoped elsewhere), rendered as an overlay over the dashboard from
  per-sprint planned/completed counts; `Esc` closes the overlay before the
  view. Sprint `j`/`k` now moves `selected_idx` (previously fell through to
  the hidden list cursor despite the registry claiming otherwise).
- **Esc honesty:** `Esc` now returns to List from every view whose registry
  entry claims `esc` closes it (Board, Tree, Insights, Alerts, FlowMatrix,
  Attention, Tutorial, Actionable, Sprint — plus the existing TimeTravel,
  LabelDashboard). Graph/History keep prior behavior since their registry
  entries never claimed `esc`.

Nothing from §2 remains open. The drilldown/detail threshold re-audit note
at the end of §11 still stands (byte-parity auditing, not UX).
