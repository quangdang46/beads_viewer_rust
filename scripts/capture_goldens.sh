#!/usr/bin/env bash
# Capture frozen Go-oracle goldens from the reference clone.
# Usage: scripts/capture_goldens.sh [output_dir=golden]
set -euo pipefail

OUT="${1:-golden}"
UPSTREAM="beads_viewer"
COMMIT="9ace029f1b141c4843a1fbd2c4a365888ef734a5"

# Pin the clock for capture. Go honors SOURCE_DATE_EPOCH (main.go:1165), so
# time-dependent output (staleness day counts, velocity week buckets) is
# reproducible. Without this, goldens are stamped at wall-clock time and
# drift out of step with the epoch golden_comparison pins, which showed up as
# ~32-day "No activity in N days" divergences across the whole corpus.
# Override with: SOURCE_DATE_EPOCH=<unix-seconds> scripts/capture_goldens.sh
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1787407612}"

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
        # Capture to a temp file first: a bare `> "$f"` redirect truncates the
        # golden BEFORE the command runs, so a command that fails (e.g.
        # --robot-history outside a git repo) silently leaves a 0-byte golden
        # that looks like a legitimate capture. Only replace the golden on
        # success, so a failed command leaves the previous corpus intact.
        tmp="$f.tmp"
        rm -f "$tmp"
        # `set -e` is disabled around the capture so a command that legitimately
        # fails (e.g. --robot-history outside a git repo) is reported as a SKIP
        # rather than aborting the whole run before the status is inspected.
        if [ "$fixture" != "." ]; then
            set +e
            (cd "$fixture" && BV_NO_CACHE=1 BV_TEST_MODE=1 "$OLDPWD/$BV" $cmd > "$OLDPWD/$tmp" 2>/dev/null)
            cmd_status=$?
            set -e
        else
            set +e
            (BV_NO_CACHE=1 BV_TEST_MODE=1 $BV $cmd > "$tmp" 2>/dev/null)
            cmd_status=$?
            set -e
        fi
        if [ "$cmd_status" -ne 0 ] || [ ! -s "$tmp" ]; then
            echo "SKIP (cmd failed, golden left unchanged): $name $cmd"
            rm -f "$tmp"
        else
            mv "$tmp" "$f"
        fi
    done
done

# TOON corpus for the commands that support it
mkdir -p "$OUT/toon"
for cmd in "--robot-triage" "--robot-next" "--robot-plan" "--robot-insights" "--robot-graph" "--robot-history"; do
    slug=$(echo "$cmd" | tr ' -' '__')
    (BV_NO_CACHE=1 BV_TEST_MODE=1 $BV $cmd --format toon > "$OUT/toon/selfrepo${slug}.toon" 2>/dev/null) || echo "SKIP toon: $cmd"
done

# Record provenance of the run. `source_date_epoch` is the instant the
# capture was pinned to; golden_comparison reads it so the test clock can
# never drift from the corpus.
{
    echo "captured_at: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "go_commit: $current"
    echo "expected_commit: $COMMIT"
    echo "source_date_epoch: $SOURCE_DATE_EPOCH"
} > "$OUT/METADATA.txt"

rm -f scripts/.bv-go
echo "Goldens written to $OUT/"
