# Provenance — copied assets

- `crates/bv-graph-wasm/` = verbatim copy of `beads_viewer/bv-graph-wasm/` from
  Dicklesworthstone/beads_viewer @ `9ace029f1b141c4843a1fbd2c4a365888ef734a5`
  (v0.20.0). No modifications yet. Verified `diff -r` identical.
  **Stale:** the reference clone has since moved to `18afafa` (v0.25.0). This
  vendored copy still reflects v0.20.0 and has not been re-diffed against the
  new parity target.
  Purpose: seed for the future shared graph core (plan §4.3 / Phase 2).
  Extraction into `bv-graph-core` + wasm thin wrapper happens in Phase 0/2,
  NOT now.
