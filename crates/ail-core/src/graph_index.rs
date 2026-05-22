// ── ail-core::graph_index ─────────────────────────────────────────────────
//
// Derived adjacency index over a `SemanticGraph` for O(1) neighbor lookups.
//
// # Design
//
// `GraphIndex` is built once from a `SemanticGraph` by scanning all edges in a
// single O(E) pass. The resulting `forward` (callees) and `backward` (callers)
// maps each contain at most one entry per `NodeRef`, with the neighbor list
// stored as a `Vec<NodeRef>` for deterministic iteration order.
//
// All semantic edge kinds (`Calls`, `DependsOn`, `Emits`, `Reads`, `Writes`,
// `Proves`, `BreaksIfChanged`) are indexed — they all participate in forward
// and backward adjacency, consistent with the spec requirement to propagate
// dirtiness through every semantic relationship.
//
// # Complexity
//
// - `build`: O(E)
// - `callees` / `callers`: O(1) map lookup, O(1) slice return

use std::collections::BTreeMap;

use crate::semantic_graph::{NodeRef, SemanticGraph};

// ── GraphIndex ────────────────────────────────────────────────────────────

/// Precomputed adjacency index over a [`SemanticGraph`].
///
/// Provides O(1) lookup of outbound (callees) and inbound (callers)
/// neighbors for any [`NodeRef`] in the graph.
///
/// Built with [`GraphIndex::build`]; the struct itself is immutable after
/// construction.
pub struct GraphIndex {
    /// Forward adjacency: node → its outbound neighbors (callees).
    forward: BTreeMap<NodeRef, Vec<NodeRef>>,
    /// Backward adjacency: node → its inbound neighbors (callers).
    backward: BTreeMap<NodeRef, Vec<NodeRef>>,
}

impl GraphIndex {
    /// Build a `GraphIndex` from `graph` by scanning all edges once.
    ///
    /// Every edge `source → target` is recorded in both directions:
    /// - `forward[source]` gets `target` appended.
    /// - `backward[target]` gets `source` appended.
    ///
    /// All [`EdgeKind`](crate::semantic_graph::EdgeKind) variants are treated
    /// uniformly — they all participate in forward and backward adjacency.
    ///
    /// # Complexity
    ///
    /// O(E) where E is the number of edges.
    pub fn build(graph: &SemanticGraph) -> Self {
        let mut forward: BTreeMap<NodeRef, Vec<NodeRef>> = BTreeMap::new();
        let mut backward: BTreeMap<NodeRef, Vec<NodeRef>> = BTreeMap::new();

        for edge in &graph.edges {
            forward.entry(edge.source).or_default().push(edge.target);
            backward.entry(edge.target).or_default().push(edge.source);
        }

        Self { forward, backward }
    }

    /// Return the outbound neighbors (callees) of `r`.
    ///
    /// Returns an empty slice if `r` has no outbound edges or is not present
    /// in the graph.
    pub fn callees(&self, r: NodeRef) -> &[NodeRef] {
        self.forward.get(&r).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Return the inbound neighbors (callers) of `r`.
    ///
    /// Returns an empty slice if `r` has no inbound edges or is not present
    /// in the graph.
    pub fn callers(&self, r: NodeRef) -> &[NodeRef] {
        self.backward.get(&r).map(Vec::as_slice).unwrap_or(&[])
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, SemanticGraph};

    use super::*;

    fn node(id: u32) -> GraphNode {
        GraphNode::new(NodeRef(id), NodeKind::Function, format!("fn_{id}"))
    }

    fn edge(source: u32, target: u32, kind: EdgeKind) -> GraphEdge {
        GraphEdge::new(NodeRef(source), NodeRef(target), kind)
    }

    // ── Spec scenario: Forward adjacency for a calling node ───────────────
    // GIVEN a SemanticGraph with edge NodeRef(0) → NodeRef(1) kind Calls
    // WHEN GraphIndex::build(graph) is called
    // THEN index.callees(NodeRef(0)) contains NodeRef(1)
    #[test]
    fn callees_returns_target_for_calls_edge() {
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1)],
            edges: vec![edge(0, 1, EdgeKind::Calls)],
        };
        let index = GraphIndex::build(&graph);
        assert!(
            index.callees(NodeRef(0)).contains(&NodeRef(1)),
            "callees(0) must contain NodeRef(1)"
        );
    }

    // ── Spec scenario: Backward adjacency for a called node ──────────────
    // GIVEN the same graph
    // WHEN GraphIndex::build(graph) is called
    // THEN index.callers(NodeRef(1)) contains NodeRef(0)
    #[test]
    fn callers_returns_source_for_calls_edge() {
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1)],
            edges: vec![edge(0, 1, EdgeKind::Calls)],
        };
        let index = GraphIndex::build(&graph);
        assert!(
            index.callers(NodeRef(1)).contains(&NodeRef(0)),
            "callers(1) must contain NodeRef(0)"
        );
    }

    // ── Spec scenario: Empty adjacency for isolated node ─────────────────
    // GIVEN a SemanticGraph with NodeRef(2) having no edges
    // WHEN GraphIndex::build(graph) is called
    // THEN index.callees(NodeRef(2)) is empty AND index.callers(NodeRef(2)) is empty
    #[test]
    fn isolated_node_has_empty_adjacency() {
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1), node(2)],
            edges: vec![edge(0, 1, EdgeKind::Calls)],
        };
        let index = GraphIndex::build(&graph);
        assert!(
            index.callees(NodeRef(2)).is_empty(),
            "isolated node must have no callees"
        );
        assert!(
            index.callers(NodeRef(2)).is_empty(),
            "isolated node must have no callers"
        );
    }

    // ── Spec scenario: DependsOn edge appears in callees ─────────────────
    // GIVEN a graph with edge NodeRef(0) → NodeRef(1) kind DependsOn
    // WHEN GraphIndex::build(graph) is called
    // THEN index.callees(NodeRef(0)) contains NodeRef(1)
    #[test]
    fn depends_on_edge_is_indexed_as_callee() {
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1)],
            edges: vec![edge(0, 1, EdgeKind::DependsOn)],
        };
        let index = GraphIndex::build(&graph);
        assert!(
            index.callees(NodeRef(0)).contains(&NodeRef(1)),
            "DependsOn edge must appear in callees"
        );
    }

    // ── TRIANGULATE: empty graph produces no entries ──────────────────────
    #[test]
    fn empty_graph_produces_no_adjacency() {
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        let index = GraphIndex::build(&graph);
        assert!(index.callees(NodeRef(0)).is_empty());
        assert!(index.callers(NodeRef(0)).is_empty());
    }

    // ── TRIANGULATE: multiple edges from same source ──────────────────────
    #[test]
    fn multiple_outbound_edges_are_all_indexed() {
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1), node(2)],
            edges: vec![edge(0, 1, EdgeKind::Calls), edge(0, 2, EdgeKind::DependsOn)],
        };
        let index = GraphIndex::build(&graph);
        let callees = index.callees(NodeRef(0));
        assert!(callees.contains(&NodeRef(1)));
        assert!(callees.contains(&NodeRef(2)));
    }

    // ── TRIANGULATE: all edge kinds are indexed ───────────────────────────
    #[test]
    fn all_edge_kinds_are_indexed() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0),
                node(1),
                node(2),
                node(3),
                node(4),
                node(5),
                node(6),
            ],
            edges: vec![
                edge(0, 1, EdgeKind::Calls),
                edge(0, 2, EdgeKind::DependsOn),
                edge(0, 3, EdgeKind::Emits),
                edge(0, 4, EdgeKind::Reads),
                edge(0, 5, EdgeKind::Writes),
                edge(0, 6, EdgeKind::Proves),
            ],
        };
        let index = GraphIndex::build(&graph);
        let callees = index.callees(NodeRef(0));
        for target in 1..=6u32 {
            assert!(
                callees.contains(&NodeRef(target)),
                "EdgeKind for target {target} must appear in callees"
            );
        }
    }
}
