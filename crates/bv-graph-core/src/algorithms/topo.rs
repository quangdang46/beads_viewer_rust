//! Topological sort: Kahn's algorithm plus a gonum-compatible variant.
//!
//! Both order nodes such that for every edge u→v, u comes before v.
//! Kahn's serves execution planning and critical path analysis; the gonum
//! variant reproduces `gonum topo.Sort` so `--robot-insights` can match the
//! Go oracle byte-for-byte.

use crate::graph::DiGraph;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Topological sort result.
pub struct TopoSortResult {
    /// Sorted node indices (valid only if is_dag is true)
    pub order: Vec<usize>,
    /// Whether the graph is a DAG
    pub is_dag: bool,
}

/// Topological sort using Kahn's algorithm with deterministic ordering.
///
/// Uses a min-heap to ensure consistent output across runs.
/// Returns None if the graph contains cycles.
///
/// # Arguments
/// * `graph` - The directed graph to sort
///
/// # Returns
/// * `Some(order)` - Vector of node indices in topological order
/// * `None` - If the graph contains cycles
pub fn topological_sort(graph: &DiGraph) -> Option<Vec<usize>> {
    let n = graph.len();
    if n == 0 {
        return Some(Vec::new());
    }

    // Compute in-degrees
    let mut in_degree: Vec<usize> = (0..n).map(|i| graph.in_degree(i)).collect();

    // Min-heap for deterministic ordering (process lowest index first)
    let mut heap: BinaryHeap<Reverse<usize>> =
        (0..n).filter(|&i| in_degree[i] == 0).map(Reverse).collect();

    let mut order = Vec::with_capacity(n);

    while let Some(Reverse(u)) = heap.pop() {
        order.push(u);

        for &v in graph.successors_slice(u) {
            in_degree[v] -= 1;
            if in_degree[v] == 0 {
                heap.push(Reverse(v));
            }
        }
    }

    if order.len() == n {
        Some(order)
    } else {
        None // Cycle detected
    }
}

/// Check if the graph is a DAG (directed acyclic graph).
///
/// A graph is a DAG if and only if it has a valid topological order.
pub fn is_dag(graph: &DiGraph) -> bool {
    topological_sort(graph).is_some()
}

/// Strongly connected components in Tarjan completion order.
///
/// Mirrors `gonum topo.TarjanSCC` (vendor/gonum.org/v1/gonum/graph/topo/
/// tarjan.go:89). That calls `tarjanSCCstabilized(g, nil)`, so the sort
/// function is nil and nothing is reordered by the call: nodes are visited in
/// graph-node order and each node's successors in the graph's own adjacency
/// order. Components are appended as their root completes, i.e. in reverse
/// topological order.
fn tarjan_sccs(graph: &DiGraph) -> Vec<Vec<usize>> {
    let n = graph.len();

    // Go builds the analysis graph with `slices.Sort(blockingTargets)` before
    // adding each edge (pkg/analysis/graph.go:1636), so the adjacency list
    // gonum walks is already in ascending node-index order. Reproduce that
    // here rather than trust `DiGraph::add_edge` insertion order.
    let succs: Vec<Vec<usize>> = (0..n)
        .map(|u| {
            let mut out = graph.successors_slice(u).to_vec();
            out.sort_unstable();
            out.dedup();
            out
        })
        .collect();

    // index 0 means "unvisited", matching gonum's `indexTable[wID] == 0` test.
    let mut index = 0usize;
    let mut index_table = vec![0usize; n];
    let mut low_link = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut sccs: Vec<Vec<usize>> = Vec::new();

    // (node, next successor cursor) replaces gonum's recursive strongconnect
    // (tarjan.go:146) so a deep chain cannot overflow the stack. The visit
    // order is identical: a successor is descended into before the next one is
    // looked at, and the parent's low-link is folded in on the way back up.
    let mut frames: Vec<(usize, usize)> = Vec::new();

    for start in 0..n {
        if index_table[start] != 0 {
            continue;
        }
        index += 1;
        index_table[start] = index;
        low_link[start] = index;
        stack.push(start);
        on_stack[start] = true;
        frames.push((start, 0));

        while let Some(frame) = frames.last_mut() {
            let v = frame.0;
            if frame.1 < succs[v].len() {
                let w = succs[v][frame.1];
                frame.1 += 1;
                if index_table[w] == 0 {
                    index += 1;
                    index_table[w] = index;
                    low_link[w] = index;
                    stack.push(w);
                    on_stack[w] = true;
                    frames.push((w, 0));
                } else if on_stack[w] {
                    low_link[v] = low_link[v].min(index_table[w]);
                }
                continue;
            }

            // v is done: emit its component if v is a root, then return.
            if low_link[v] == index_table[v] {
                let mut scc = Vec::new();
                loop {
                    let w = stack.pop().expect("tarjan stack holds v");
                    on_stack[w] = false;
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
                sccs.push(scc);
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low_link[parent] = low_link[parent].min(low_link[v]);
            }
        }
    }

    sccs
}

/// Topological sort matching `gonum topo.Sort` exactly.
///
/// gonum's `Sort` is `TarjanSCC` followed by `sortedFrom(sccs, lexical)`
/// (tarjan.go:40-42): each component contributes its single node, then
/// `slices.Reverse` flips the whole list (tarjan.go:77), turning Tarjan's
/// reverse-topological component order into a "from → to" ordering. Any
/// multi-node component makes gonum return an `Unorderable` error, so this
/// returns `None` for such graphs.
///
/// `lexical` is `order.ByID`, and callers build node indices in sorted-id
/// order, so ascending index is exactly lexical order here.
///
/// Callers that need a different (or cheaper) valid order — critical path,
/// slack, k-paths — should keep using [`topological_sort`].
pub fn topological_sort_gonum(graph: &DiGraph) -> Option<Vec<usize>> {
    let sccs = tarjan_sccs(graph);
    if sccs.iter().any(|s| s.len() != 1) {
        return None; // gonum: Unorderable
    }
    let mut sorted: Vec<usize> = sccs.iter().map(|s| s[0]).collect();
    sorted.reverse(); // sortedFrom: slices.Reverse(sorted)
    Some(sorted)
}

/// Compute topological sort with detailed result.
pub fn topological_sort_result(graph: &DiGraph) -> TopoSortResult {
    match topological_sort(graph) {
        Some(order) => TopoSortResult {
            order,
            is_dag: true,
        },
        None => TopoSortResult {
            order: Vec::new(),
            is_dag: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_graph() {
        let g = DiGraph::new();
        let result = topological_sort(&g);
        assert_eq!(result, Some(vec![]));
    }

    #[test]
    fn test_single_node() {
        let mut g = DiGraph::new();
        g.add_node("a");
        let result = topological_sort(&g);
        assert_eq!(result, Some(vec![0]));
    }

    #[test]
    fn test_linear_chain() {
        // a -> b -> c
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        g.add_edge(a, b);
        g.add_edge(b, c);

        let result = topological_sort(&g).unwrap();
        assert_eq!(result, vec![0, 1, 2]); // a, b, c
    }

    #[test]
    fn test_diamond() {
        //     a
        //    / \
        //   b   c
        //    \ /
        //     d
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        let d = g.add_node("d");
        g.add_edge(a, b);
        g.add_edge(a, c);
        g.add_edge(b, d);
        g.add_edge(c, d);

        let result = topological_sort(&g).unwrap();
        // Valid orders: [a, b, c, d] or [a, c, b, d]
        // With min-heap, should be [a, b, c, d]
        assert_eq!(result[0], a); // a first
        assert_eq!(result[3], d); // d last
                                  // b before c due to min-heap ordering
        assert_eq!(result, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_cycle_detection() {
        // a -> b -> c -> a
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        g.add_edge(a, b);
        g.add_edge(b, c);
        g.add_edge(c, a);

        let result = topological_sort(&g);
        assert!(result.is_none());
    }

    #[test]
    fn test_self_loop() {
        // a -> a
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        g.add_edge(a, a);

        let result = topological_sort(&g);
        assert!(result.is_none());
    }

    #[test]
    fn test_disconnected() {
        // Two separate components: a->b, c->d
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        let d = g.add_node("d");
        g.add_edge(a, b);
        g.add_edge(c, d);

        let result = topological_sort(&g).unwrap();
        // With min-heap: [0, 2, 1, 3] - process by index
        // a (0) and c (2) have in-degree 0, min-heap pops 0 first
        assert_eq!(result.len(), 4);
        // a comes before b
        let a_pos = result.iter().position(|&x| x == a).unwrap();
        let b_pos = result.iter().position(|&x| x == b).unwrap();
        assert!(a_pos < b_pos);
        // c comes before d
        let c_pos = result.iter().position(|&x| x == c).unwrap();
        let d_pos = result.iter().position(|&x| x == d).unwrap();
        assert!(c_pos < d_pos);
    }

    #[test]
    fn test_is_dag() {
        let mut dag = DiGraph::new();
        let a = dag.add_node("a");
        let b = dag.add_node("b");
        dag.add_edge(a, b);
        assert!(is_dag(&dag));

        let mut cyclic = DiGraph::new();
        let x = cyclic.add_node("x");
        let y = cyclic.add_node("y");
        cyclic.add_edge(x, y);
        cyclic.add_edge(y, x);
        assert!(!is_dag(&cyclic));
    }

    #[test]
    fn test_deterministic() {
        // Run multiple times, should always get same result
        let mut g = DiGraph::new();
        for i in 0..10 {
            g.add_node(&format!("node{}", i));
        }
        // Add some edges
        g.add_edge(0, 5);
        g.add_edge(1, 5);
        g.add_edge(2, 6);
        g.add_edge(3, 7);
        g.add_edge(5, 8);
        g.add_edge(6, 8);
        g.add_edge(7, 9);
        g.add_edge(8, 9);

        let result1 = topological_sort(&g).unwrap();
        let result2 = topological_sort(&g).unwrap();
        let result3 = topological_sort(&g).unwrap();

        assert_eq!(result1, result2);
        assert_eq!(result2, result3);
    }

    /// Diamond `a->b, a->c, b->d, c->d` (indices 0=a .. 3=d). Both sorts are
    /// valid, and they disagree on the b/c tie: Kahn emits the lower index
    /// first, Tarjan emits the one its DFS reached last. gonum's `topo.Sort`
    /// on this graph is [a, c, b, d].
    fn diamond() -> DiGraph {
        let mut g = DiGraph::new();
        for id in ["a", "b", "c", "d"] {
            g.add_node(id);
        }
        g.add_edge(0, 1);
        g.add_edge(0, 2);
        g.add_edge(1, 3);
        g.add_edge(2, 3);
        g
    }

    #[test]
    fn gonum_matches_tarjan_not_kahn_on_diamond() {
        let g = diamond();
        assert_eq!(topological_sort_gonum(&g), Some(vec![0, 2, 1, 3]));
        // Same graph, same correctness, different tie-break: this is why the
        // insights field cannot reuse the Kahn result.
        assert_eq!(topological_sort(&g), Some(vec![0, 1, 2, 3]));
    }

    #[test]
    fn gonum_is_independent_of_edge_insertion_order() {
        let mut g = DiGraph::new();
        for id in ["a", "b", "c", "d"] {
            g.add_node(id);
        }
        // Same diamond, edges inserted in the opposite order.
        g.add_edge(0, 2);
        g.add_edge(0, 1);
        g.add_edge(2, 3);
        g.add_edge(1, 3);
        assert_eq!(topological_sort_gonum(&g), Some(vec![0, 2, 1, 3]));
    }

    #[test]
    fn gonum_orders_chain_from_to() {
        // a -> b -> c -> d
        let mut g = DiGraph::new();
        for id in ["a", "b", "c", "d"] {
            g.add_node(id);
        }
        g.add_edge(0, 1);
        g.add_edge(1, 2);
        g.add_edge(2, 3);
        assert_eq!(topological_sort_gonum(&g), Some(vec![0, 1, 2, 3]));
    }

    #[test]
    fn gonum_rejects_cycles() {
        // a -> b -> c -> a
        let mut g = DiGraph::new();
        for id in ["a", "b", "c"] {
            g.add_node(id);
        }
        g.add_edge(0, 1);
        g.add_edge(1, 2);
        g.add_edge(2, 0);
        assert_eq!(topological_sort_gonum(&g), None);
    }

    #[test]
    fn gonum_tolerates_self_loop() {
        // A one-node component is still a valid singleton, so gonum returns
        // an ordering where Kahn reports a cycle.
        let mut g = DiGraph::new();
        g.add_node("a");
        g.add_node("b");
        g.add_edge(0, 0);
        g.add_edge(1, 0);
        assert_eq!(topological_sort_gonum(&g), Some(vec![1, 0]));
        assert_eq!(topological_sort(&g), None);
    }

    #[test]
    fn gonum_empty_graph() {
        assert_eq!(topological_sort_gonum(&DiGraph::new()), Some(vec![]));
    }

    #[test]
    fn gonum_is_a_valid_topological_order() {
        // Random-ish DAG: gonum may pick any valid order, but never an
        // invalid one — every edge u->v keeps u before v.
        let mut g = DiGraph::new();
        for i in 0..40 {
            g.add_node(&format!("n{i:02}"));
        }
        for u in 0..40 {
            for v in (u + 1)..40 {
                if (u * 7 + v * 3) % 5 == 0 {
                    g.add_edge(u, v);
                }
            }
        }
        let order = topological_sort_gonum(&g).unwrap();
        assert_eq!(order.len(), 40);
        let mut pos = vec![usize::MAX; 40];
        for (rank, &u) in order.iter().enumerate() {
            pos[u] = rank;
        }
        for u in 0..40 {
            for &v in g.successors_slice(u) {
                assert!(pos[u] < pos[v], "{u} must precede {v}");
            }
        }
    }
}
