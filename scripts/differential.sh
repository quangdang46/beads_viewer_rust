#!/usr/bin/env bash
# Differential comparison: runs Go bv and Rust bvr side-by-side, normalizes
# nondeterministic fields (timestamps/timings), diffs outputs.
#
# Usage: scripts/differential.sh [--fixture DIR] [--command "ARGS"]
set -euo pipefail

GO_BV="${GO_BV:-beads_viewer/.bv-go}"
RUST_BIN="${RUST_BIN:-./target/debug/bvr}"
FIXTURES=(tests/fixtures/small_chain tests/fixtures/medium_tree)
COMMANDS=("--robot-triage" "--robot-insights" "--robot-plan")

PASS=0; FAIL=0; SKIP=0

for fixture in "${FIXTURES[@]}"; do
    for cmd in "${COMMANDS[@]}"; do
        slug=$(basename "$fixture")__$(echo "$cmd" | tr ' -' '__')

        # Go output
        go_out=$(cd "$fixture" && BV_TEST_MODE=1 "$OLDPWD/$GO_BV" $cmd 2>/dev/null) || { echo "SKIP(go-fail): $slug"; SKIP=$((SKIP+1)); continue; }
        # Rust output
        rust_out=$(cd "$fixture" && BV_ROBOT=1 "$OLDPWD/$RUST_BIN" $cmd 2>/dev/null) || { echo "FAIL(rust-error): $slug"; FAIL=$((FAIL+1)); continue; }

        # Normalize: strip timestamps and ms fields
        norm() {
            python3 -c "
import json, sys
def scrub(obj):
    if isinstance(obj, dict):
        return {k: ('<TIMESTAMP>' if k in ('generated_at','timestamp') and isinstance(v,str) else scrub(v)) for k,v in obj.items() if k not in ('ms','compute_time_ms')}
    if isinstance(obj, list): return [scrub(x) for x in obj]
    return obj
print(json.dumps(scrub(json.loads(sys.stdin.read())), sort_keys=True))
"
        }
        go_norm=$(echo "$go_out" | norm)
        rust_norm=$(echo "$rust_out" | norm)

        if [ "$go_norm" = "$rust_norm" ]; then
            echo "PASS: $slug"
            PASS=$((PASS+1))
        else
            echo "FAIL: $slug"
            FAIL=$((FAIL+1))
        fi
    done
done

echo ""
echo "Differential results: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
[ "$FAIL" -eq 0 ]
