# Vendored assets

Three files in this directory are copied rather than written. Go holds the same
three files, in `pkg/export/` at parity commit `18afafa`; this crate is a port,
so the bytes are ported too. `crates/bv-export/tests/graph_interactive_parity.rs`
pins the rendered document against Go's own output byte-for-byte, which is what
keeps these copies honest.

| File | Source | Licence | Notes |
|---|---|---|---|
| `force-graph.min.js` | `pkg/export/force-graph.min.js` — [vasturiano/force-graph](https://github.com/vasturiano/force-graph) v1.43.5 | MIT | Embedded so the exported page works with no network. |
| `marked.min.js` | `pkg/export/marked.min.js` — [markedjs/marked](https://github.com/markedjs/marked) v14.1.0 | MIT (Copyright 2011-2024 Christopher Jeffrey) | Embedded so issue descriptions render as Markdown with no network. |
| `graph_interactive.html` | the `fmt.Sprintf` template in `pkg/export/graph_render_beautiful.go:15` | same as this repository | The page's markup, stylesheet and UI script. |

## The one edit to `graph_interactive.html`

Go's template is a format string. Its thirteen positional verbs —

```
%s title, %s title, %d nodes, %d edges, %d nodes, %d nodes, %d edges,
%s timestamp, %s hash, %s project, %s force-graph, %s marked, %s data
```

— were replaced, positionally and one for one, with the named sentinels
`@@BV_TITLE@@`, `@@BV_NODES@@`, `@@BV_EDGES@@`, `@@BV_TIMESTAMP@@`,
`@@BV_HASH@@`, `@@BV_PROJECT@@`, `@@BV_FORCE_GRAPH_JS@@`, `@@BV_MARKED_JS@@`
and `@@BV_GRAPH_DATA_JSON@@`.

The reason is that Rust's `format!` would otherwise require doubling all 74
literal percent signs *and* every `{` and `}` in the stylesheet — a
transformation nobody could review. Named sentinels keep the template bytes
verbatim and put the substitution in ordinary Rust, where it is testable. The
37 `%%` escapes are deliberately left in place: `render_html` collapses them,
reproducing what Go's `Sprintf` does, and it does so before substitution so
the graph JSON and the two bundles keep their own percent signs.

## Line endings

The two vendored bundles ship with CRLF line endings (8,684 and 1,675 carriage
returns). **Both were converted to LF.** `rustc` normalises CRLF to LF when it
reads any source file, `include_str!` included, so an LF-normalised copy is
what actually reaches the binary — storing the CRs would leave the file on disk
disagreeing with the compiled artefact. The consequence is that the exported
page differs from Go's by those 10,359 carriage returns, all of them inside
third-party JavaScript where line endings are not significant. This is the one
known byte-level divergence, and it is unavoidable in Rust.
