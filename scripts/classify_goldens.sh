#!/usr/bin/env bash
# Classify the golden gate: for every case, compare the STORED golden against
# the live Go oracle, and the Rust build against the same oracle.
#
# This separates three very different situations that all show up as
# "golden_comparison is RED":
#
#   stale   the stored golden no longer matches Go itself -> the corpus is
#           out of date and the gate is measuring nothing useful
#   path    golden and Go agree once absolute paths are normalised -> the corpus
#           was captured on a different machine, so path-bearing fields differ
#           forever on this host
#   real    golden == Go, but Rust differs -> a genuine port gap
#
# Usage: scripts/classify_goldens.sh [outdir]

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/golden}"
GO_BIN="$ROOT/scripts/.bv-go.exe"
RS_BIN="$ROOT/target/debug/bvr.exe"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1787407612}"
# The capture script pins these; without them the oracle takes a different
# code path (cache bypass, test-mode output) and every case reads as stale.
export BV_NO_CACHE=1
export BV_TEST_MODE=1

[ -x "$GO_BIN" ] || { echo "missing $GO_BIN" >&2; exit 3; }
[ -x "$RS_BIN" ] || { echo "missing $RS_BIN" >&2; exit 3; }

FIXTURES=("selfrepo:$ROOT" "small_chain:$ROOT/tests/fixtures/small_chain" \
          "medium_tree:$ROOT/tests/fixtures/medium_tree" \
          "large_cyclic_600:$ROOT/tests/fixtures/large_cyclic_600" \
          "xl_2500:$ROOT/tests/fixtures/xl_2500")

COMMANDS=("--robot-triage" "--robot-next" "--robot-plan" "--robot-insights" \
          "--robot-priority" "--robot-suggest" "--robot-alerts" "--robot-graph" \
          "--robot-label-health" "--robot-label-flow" "--robot-label-attention" \
          "--robot-history" "--robot-diff|--diff-since|HEAD~5" "--robot-recipes" \
          "--robot-schema")

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Normalise anything host-specific so a path difference is not mistaken for a
# semantic one: absolute repo paths, Windows separators, and the username.
norm() {
  sed -e "s#${ROOT//\//\\\\}#<REPO>#g" \
      -e "s#${ROOT}#<REPO>#g" \
      -e "s#/Users/[A-Za-z0-9_.-]*/Projects/beads_viewer_rust#<REPO>#g" \
      -e 's#\\\\#/#g' \
      -e 's#C:\\\\Users\\\\[A-Za-z0-9_.-]*#<HOME>#g'
}

stale=0; real=0; pass=0; skipped=0; rust_beats_stale=0
printf '%-46s %s\n' "CASE" "VERDICT"
for fx in "${FIXTURES[@]}"; do
  name="${fx%%:*}"; dir="${fx#*:}"
  for cmd in "${COMMANDS[@]}"; do
    IFS='|' read -r -a parts <<< "$cmd"
    args=("${parts[@]}")
    # Same slug rule as scripts/capture_goldens.sh: every space and hyphen
    # becomes an underscore.
    slug="$(printf '%s' "$cmd" | tr '|' ' ' | tr ' -' '__')"
    golden="$OUT/${name}__${slug}.json"
    [ -f "$golden" ] || { skipped=$((skipped+1)); continue; }

    ( cd "$dir" && "$GO_BIN" "${args[@]}" >"$TMP/go.json" 2>/dev/null ); ge=$?
    if [ $ge -ne 0 ] || [ ! -s "$TMP/go.json" ]; then
      skipped=$((skipped+1)); continue
    fi
    ( cd "$dir" && "$RS_BIN" "${args[@]}" >"$TMP/rs.json" 2>/dev/null )

    norm <"$golden"      >"$TMP/golden.norm"
    norm <"$TMP/go.json" >"$TMP/go.norm"
    norm <"$TMP/rs.json" >"$TMP/rs.norm"

    golden_valid=0; rust_matches=0
    cmp -s "$TMP/golden.norm" "$TMP/go.norm"     && golden_valid=1
    cmp -s "$TMP/rs.norm"    "$TMP/go.norm"     && rust_matches=1

    if [ $golden_valid -eq 1 ]; then
      if [ $rust_matches -eq 1 ]; then
        verdict="PASS"; pass=$((pass+1))
      else
        verdict="REAL-GAP"; real=$((real+1))
      fi
    else
      stale=$((stale+1))
      if [ $rust_matches -eq 1 ]; then
        verdict="STALE-golden (rust==go)"; rust_beats_stale=$((rust_beats_stale+1))
      else
        verdict="STALE-golden (both differ)"
      fi
    fi
    printf '%-46s %s\n' "${name}__${slug}" "$verdict"
  done
done

echo
echo "pass=$pass  real-gaps=$real  stale-golden=$stale (of which rust already matches go: $rust_beats_stale)  skipped=$skipped"
