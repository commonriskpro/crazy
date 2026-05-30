use super::*;

// ── helpers ───────────────────────────────────────────────────────────

fn node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), kind, name)
}

fn edge(source: u32, target: u32, kind: EdgeKind) -> GraphEdge {
    GraphEdge::new(NodeRef(source), NodeRef(target), kind)
}

mod cbor_core;
mod cbor_extended;
mod refs_constraints;
mod validation_basic;
mod validation_full;
