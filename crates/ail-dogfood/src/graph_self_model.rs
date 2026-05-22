// ── ail-dogfood::graph_self_model ─────────────────────────────────────────
//
// Constructs a `SemanticGraph` whose nodes represent the toolchain's own
// core types, proving the graph model can describe its own schema.
//
// # Node layout
//
// | NodeRef | Kind   | Name                  |
// |---------|--------|-----------------------|
// | 0       | Module | "meta"                |
// | 1       | Type   | "meta.SemanticGraph"  |
// | 2       | Type   | "meta.GraphNode"      |
// | 3       | Type   | "meta.NodeRef"        |
// | 4       | Type   | "meta.NodeKind"       |
// | 5       | Type   | "meta.EdgeKind"       |
// | 6       | Type   | "meta.GraphEdge"      |
// | 7       | Type   | "meta.ChangeSet"      |
// | 8       | Type   | "meta.ChangeSetOp"    |
// | 9       | Type   | "meta.SnapshotId"     |
//
// Edges: NodeRef(0) --DependsOn--> NodeRef(1..=9) (9 edges total).
//
// # Postconditions
//
// - `result.validate() == Ok(())`
// - Exactly 10 nodes, 9 edges
// - One `NodeKind::Module` node named `"meta"`
// - Nine `NodeKind::Type` nodes with `meta.` prefixed names

use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};

/// Build a `SemanticGraph` that models the toolchain's own core types.
///
/// # Postconditions
///
/// - `result.validate() == Ok(())` (no duplicate refs, no dangling edges)
/// - `result.nodes.len() == 10`
/// - `result.edges.len() == 9`
/// - The first node is `NodeKind::Module` with name `"meta"`
/// - Nodes 1–9 are `NodeKind::Type` with `meta.`-prefixed names
pub fn build_graph_self_model() -> SemanticGraph {
    // ── Nodes ─────────────────────────────────────────────────────────────

    let nodes = vec![
        // NodeRef(0) — the meta module boundary
        GraphNode::new(NodeRef(0), NodeKind::Module, "meta"),
        // NodeRef(1..=9) — one Type node per core toolchain type
        GraphNode::new(NodeRef(1), NodeKind::Type, "meta.SemanticGraph"),
        GraphNode::new(NodeRef(2), NodeKind::Type, "meta.GraphNode"),
        GraphNode::new(NodeRef(3), NodeKind::Type, "meta.NodeRef"),
        GraphNode::new(NodeRef(4), NodeKind::Type, "meta.NodeKind"),
        GraphNode::new(NodeRef(5), NodeKind::Type, "meta.EdgeKind"),
        GraphNode::new(NodeRef(6), NodeKind::Type, "meta.GraphEdge"),
        GraphNode::new(NodeRef(7), NodeKind::Type, "meta.ChangeSet"),
        GraphNode::new(NodeRef(8), NodeKind::Type, "meta.ChangeSetOp"),
        GraphNode::new(NodeRef(9), NodeKind::Type, "meta.SnapshotId"),
    ];

    // ── Edges ─────────────────────────────────────────────────────────────
    // meta module --DependsOn--> each type node

    let edges = (1u32..=9)
        .map(|i| GraphEdge::new(NodeRef(0), NodeRef(i), EdgeKind::DependsOn))
        .collect();

    SemanticGraph { nodes, edges }
}
