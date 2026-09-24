#!/usr/bin/env bash
# Parity matrix — run a curated set of invocations through the Go oracle and
# the Rust build and report, per invocation, whether they agree.
#
# This exists because `golden_comparison` cannot currently answer the question.
# The frozen corpus is stale (it holds old-Rust output, not Go's), so the gate
# is red for reasons unrelated to the port. Issue #5's checkboxes are claims
# with no automated witness; this is the witness.
#
#   pass    stdout byte-identical
#   timing  differs only in `*.ms` measurement fields (nondeterministic)
#   DIFFER  a real divergence — this is what needs fixing
#   ERROR   one side failed or produced non-JSON
#
# Usage:
#   scripts/parity_matrix.sh                 # full matrix
#   scripts/parity_matrix.sh --robot-next    # only invocations matching a filter
#   RS_BIN=/path/to/bvr scripts/parity_matrix.sh
#
# Requires a Go oracle at scripts/.bv-go.exe (see capture_goldens.sh) and a
# built Rust binary.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GO_BIN="${GO_BIN:-$ROOT/scripts/.bv-go.exe}"
RS_BIN="${RS_BIN:-$ROOT/target/debug/bvr.exe}"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1787407612}"
export BV_NO_CACHE=1
export BV_TEST_MODE=1

FILTER="${1:-}"

for bin in "$GO_BIN" "$RS_BIN"; do
  if [ ! -x "$bin" ]; then
    echo "MISSING BINARY: $bin" >&2
    echo "  Go:    (cd ../beads_viewer && go build -o ../beads_viewer_rust/scripts/.bv-go.exe ./cmd/bv)" >&2
    echo "  Rust:  cargo build -p bv" >&2
    exit 3
  fi
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# A bead id that exists in the selfrepo fixture, for invocations that need one.
BEAD=$(grep -o '"id":"[^"]*"' "$ROOT/.beads/issues.jsonl" 2>/dev/null | head -1 | sed 's/.*:"//;s/"//')
BEAD="${BEAD:-beads_viewer_rust-api-freeze-b73}"

# invocations are '|' separated so the matrix survives shell quoting
MATRIX=(
  # --- core robot surface, no modifiers ---
  "--robot-triage"
  "--robot-next"
  "--robot-plan"
  "--robot-insights"
  "--robot-priority"
  "--robot-alerts"
  "--robot-suggest"
  "--robot-graph"
  "--robot-label-health"
  "--robot-label-flow"
  "--robot-recipes"
  "--robot-schema"
  "--robot-metrics"
  "--robot-capabilities"
  # --- causal/history (needs a git repo: selfrepo only) ---
  "--robot-causality|$BEAD"
  "--robot-causality|$BEAD|--history-limit|10"
  "--robot-causality|$BEAD|--history-since|90d"
  # --- triage family modifiers ---
  "--robot-triage|--brief"
  "--robot-triage-by-track"
  "--robot-triage-by-label"
  "--robot-not-ready-labels|blocked"
  "--robot-triage|--graph-root|$BEAD"
  # --- search (the query arrives via --search, or positionally after the
  #     `search` alias — Go accepts both) ---
  "--robot-search|--search|deploy"
  "search|deploy"
  "--robot-search|--search|deploy|--search-limit|5"
  "--robot-search|--search|deploy|--search-mode|semantic"
  "--robot-search|--search|deploy|--search-preset|balanced"
  # --- priority scoping ---
  "--robot-priority|--robot-by-label|phase"
  "--robot-priority|--robot-by-assignee|someone"
  "--robot-priority|--robot-min-confidence|0.5"
  # --- attention / alerts / suggest ---
  "--robot-label-attention|--attention-limit|3"
  "--robot-alerts|--severity|high"
  "--robot-alerts|--alert-type|blocked"
  "--robot-suggest|--suggest-type|dependency"
  "--robot-suggest|--suggest-confidence|0.5"
  # --- graph / impact (selfrepo: needs git + files) ---
  "--robot-blocker-chain|$BEAD"
  "--robot-impact|README.md"
  "--robot-impact-network|$BEAD"
  "--robot-causality|$BEAD|--robot-history-timeout-ms|5000"
  # --- drift / baseline ---
  "--robot-diff|--diff-since|HEAD~3"
  # --- output format surface ---
  "--robot-triage|--format|json"
  "--robot-triage|--format|toon"
  "--robot-next|--toon"
  "--robot-next|--output|json"
  # --- argv normalisation ---
  "--json"
  "--toon"
  "triage"
  "next"
  "insights"
  # --- global / meta ---
  "--version"
  "--robot-help"
  "--robot-docs|guide"
  # --- error paths (exit code + stderr are the contract) ---
  "--robot-diff"
  "--robot-triage|--format|xml"
  "--robot-causality|NO-SUCH-BEAD-xyz"
  "--robot-causality|$BEAD|--history-limit|abc"
  "--robot-triage|--brief|--robot-next"
)

pass=0; timing=0; differ=0; error=0; skipped=0
failed_list=()

printf '%-52s %s\n' "INVOCATION" "VERDICT"
printf '%.0s-' {1..74}; echo

for entry in "${MATRIX[@]}"; do
  IFS='|' read -r -a argv <<< "$entry"
  label="${argv[*]}"
  if [ -n "$FILTER" ] && [[ "$label" != *"$FILTER"* ]]; then
    skipped=$((skipped + 1))
    continue
  fi

  "$GO_BIN" "${argv[@]}" >"$TMP/go.json" 2>"$TMP/go.err"; ge=$?
  "$RS_BIN" "${argv[@]}" >"$TMP/rs.json" 2>"$TMP/rs.err"; re=$?

  if [ "$ge" != "$re" ]; then
    verdict="DIFFER(exit go=$ge rs=$re)"; differ=$((differ + 1)); failed_list+=("$label")
  elif [ "$ge" != 0 ]; then
    # Both failed the same way: compare the message, not the payload.
    if cmp -s "$TMP/go.err" "$TMP/rs.err"; then verdict="pass(err)"; pass=$((pass + 1))
    else verdict="DIFFER(stderr)"; differ=$((differ + 1)); failed_list+=("$label"); fi
  elif cmp -s "$TMP/go.json" "$TMP/rs.json"; then
    verdict="pass"; pass=$((pass + 1))
  else
    # Are the only differing leaves nondeterministic timings?
    shape=$(node -e '
      const fs = require("fs");
      const dir = process.argv[1];
      const leaves = (o, p, out) => {
        if (Array.isArray(o)) { o.forEach((v, i) => leaves(v, `${p}[${i}]`, out)); return; }
        if (o && typeof o === "object") { for (const k of Object.keys(o).sort()) leaves(o[k], `${p}.${k}`, out); return; }
        out.push([p, JSON.stringify(o)]);
      };
      const read = (f) => { try { return JSON.parse(fs.readFileSync(`${dir}/${f}`, "utf8")); } catch { return null; } };
      const go = read("go.json"), rs = read("rs.json");
      if (!go || !rs) { console.log("NONJSON"); process.exit(0); }
      const a = [], b = [];
      leaves(go, "", a); leaves(rs, "", b);
      const ma = new Map(a), mb = new Map(b);
      const diffs = [];
      for (const [k, v] of a) if (mb.get(k) !== v) diffs.push(k);
      for (const [k, v] of b) if (ma.get(k) !== v) diffs.push(k);
      const onlyTiming = diffs.length > 0 && diffs.every((k) => /\.(ms|compute_time_ms)$/.test(k));
      if (onlyTiming) { console.log("TIMING"); process.exit(0); }
      console.log("DIFFER:" + [...new Set(diffs)].slice(0, 6).join(" | "));
    ' "$TMP")
    case "$shape" in
      TIMING) verdict="timing"; timing=$((timing + 1)) ;;
      NONJSON) verdict="DIFFER(non-json)"; differ=$((differ + 1)); failed_list+=("$label") ;;
      *) verdict="DIFFER"; differ=$((differ + 1)); failed_list+=("$label") ;;
    esac
  fi
  printf '%-52s %s\n' "${label:0:52}" "$verdict"
done

echo
echo "pass=$pass  timing-only=$timing  DIFFER=$differ  skipped=$skipped"
if [ "$differ" -gt 0 ]; then
  echo
  echo "Divergences to fix:"
  for f in "${failed_list[@]}"; do echo "  - $f"; done
fi
[ "$differ" -eq 0 ]
