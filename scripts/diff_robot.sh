#!/usr/bin/env bash
# Differential check: run the same robot invocation through the Go oracle and
# the Rust build, then report how their JSON differs.
#
# Usage: scripts/diff_robot.sh --robot-triage [--as-of HEAD~3 ...]
#   Runs in the current directory (a beads repo) and prints:
#     - "MATCH" when stdout is byte-identical
#     - otherwise the per-leaf JSON differences, plus both exit codes
#
# Requires scripts/.bv-go.exe (go build -o ../beads_viewer_rust/scripts/.bv-go.exe
# ./cmd/bv) and a current `cargo build -p bv`. SOURCE_DATE_EPOCH is pinned so
# time-dependent fields are comparable.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GO_BIN="$ROOT/scripts/.bv-go.exe"
RS_BIN="$ROOT/target/debug/bvr.exe"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1787407612}"

for bin in "$GO_BIN" "$RS_BIN"; do
  if [ ! -x "$bin" ]; then
    echo "MISSING BINARY: $bin" >&2
    exit 3
  fi
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

"$GO_BIN" "$@" >"$TMP/go.json" 2>"$TMP/go.err"; GO_EXIT=$?
"$RS_BIN" "$@" >"$TMP/rs.json" 2>"$TMP/rs.err"; RS_EXIT=$?

echo "### args: $*"
echo "### exit: go=$GO_EXIT rust=$RS_EXIT"

if [ "$GO_EXIT" != "$RS_EXIT" ]; then
  echo "### EXIT CODE MISMATCH"
fi

if cmp -s "$TMP/go.json" "$TMP/rs.json"; then
  echo "### STDOUT: byte-identical"
else
  echo "### STDOUT: DIFFERS (go=$(wc -c <"$TMP/go.json")B rust=$(wc -c <"$TMP/rs.json")B)"
  if [ ! -s "$TMP/go.json" ] || [ ! -s "$TMP/rs.json" ]; then
    echo "--- go stdout ---"; head -c 800 "$TMP/go.json"; echo
    echo "--- rust stdout ---"; head -c 800 "$TMP/rs.json"; echo
  else
    node -e '
      const fs = require("fs");
      const dir = process.argv[1];
      const flat = (o, p, out) => {
        if (Array.isArray(o)) { o.forEach((v, i) => flat(v, `${p}[${i}]`, out)); return; }
        if (o && typeof o === "object") { for (const k of Object.keys(o).sort()) flat(o[k], `${p}.${k}`, out); return; }
        out.push(`${p} = ${JSON.stringify(o)}`);
      };
      const read = (f) => { try { return JSON.parse(fs.readFileSync(`${dir}/${f}`, "utf8")); } catch (e) { return null; } };
      const go = read("go.json"), rs = read("rs.json");
      if (!go || !rs) { console.log("  (one side is not valid JSON)"); process.exit(0); }
      const a = [], b = [];
      flat(go, "", a); flat(rs, "", b);
      const sa = new Set(a), sb = new Set(b);
      const onlyGo = a.filter((x) => !sb.has(x));
      const onlyRs = b.filter((x) => !sa.has(x));
      const cap = 40;
      console.log(`  leaves only in GO (${onlyGo.length}${onlyGo.length > cap ? ", showing " + cap : ""}):`);
      onlyGo.slice(0, cap).forEach((x) => console.log("    - " + x));
      console.log(`  leaves only in RUST (${onlyRs.length}${onlyRs.length > cap ? ", showing " + cap : ""}):`);
      onlyRs.slice(0, cap).forEach((x) => console.log("    + " + x));
    ' "$TMP"
  fi
fi

if ! cmp -s "$TMP/go.err" "$TMP/rs.err"; then
  echo "### STDERR DIFFERS"
  diff <(head -20 "$TMP/go.err") <(head -20 "$TMP/rs.err") | head -20
fi
