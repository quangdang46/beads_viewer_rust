#!/usr/bin/env bash
# Capture frozen Go-oracle goldens from the reference clone.
# Usage: scripts/capture_goldens.sh [output_dir=golden]
set -euo pipefail

OUT="${1:-golden}"
UPSTREAM="beads_viewer"
COMMIT="9ace029f1b141c4843a1fbd2c4a365888ef734a5"

[ -d "$UPSTREAM" ] || { echo "ERROR: $UPSTREAM/ clone not found"; exit 1; }

cd "$UPSTREAM"
current=$(git rev-parse HEAD)
[ "$current" = "$COMMIT" ] || echo "WARN: upstream at $current, expected $COMMIT (proceeding — goldens record actual SHA)"

echo "Building Go bv..."
go build -o ../scripts/.bv-go ./cmd/bv
cd ..

BV=scripts/.bv-go

# Fixture classes: real repo + synthetic fixtures
declare -a FIXTURES=(
    "."
    "tests/fixtures/small_chain"
    "tests/fixtures/medium_tree"
    "tests/fixtures/large_cyclic_600"
    "tests/fixtures/xl_2500"
)

declare -a COMMANDS=(
    "--robot-triage"
    "--robot-next"
    "--robot-plan"
    "--robot-insights"
    "--robot-priority"
    "--robot-suggest"
    "--robot-alerts"
    "--robot-graph"
    "--robot-label-health"
    "--robot-label-flow"
    "--robot-label-attention"
    "--robot-history"
    "--robot-diff --diff-since HEAD~5"
    "--robot-recipes"
    "--robot-schema"
)

mkdir -p "$OUT"

for fixture in "${FIXTURES[@]}"; do
    name=$(basename "$fixture")
    [ "$name" = "." ] && name="selfrepo"
    for cmd in "${COMMANDS[@]}"; do
        slug=$(echo "$cmd" | tr ' -' '__')
        f="$OUT/${name}__${slug}.json"
        # Run from a copy of the fixture so .bv state doesn't leak between runs
        if [ "$fixture" != "." ]; then
            (cd "$fixture" && BV_NO_CACHE=1 BV_TEST_MODE=1 "$OLDPWD/$BV" $cmd > "$OLDPWD/$f" 2>/dev/null) || echo "SKIP (cmd failed): $name $cmd"
        else
            (BV_NO_CACHE=1 BV_TEST_MODE=1 $BV $cmd > "$f" 2>/dev/null) || echo "SKIP (cmd failed): selfrepo $cmd"
        fi
    done
done

# TOON corpus for the commands that support it
mkdir -p "$OUT/toon"
for cmd in "--robot-triage" "--robot-next" "--robot-plan" "--robot-insights" "--robot-graph" "--robot-history"; do
    slug=$(echo "$cmd" | tr ' -' '__')
    (BV_NO_CACHE=1 BV_TEST_MODE=1 $BV $cmd --format toon > "$OUT/toon/selfrepo${slug}.toon" 2>/dev/null) || echo "SKIP toon: $cmd"
done

# Record provenance of the run
{
    echo "captured_at: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "go_commit: $current"
    echo "expected_commit: $COMMIT"
} > "$OUT/METADATA.txt"

rm -f scripts/.bv-go
echo "Goldens written to $OUT/"
