use super::*;
use crate::dto::{QueryBudget, QueryScope};
use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_storage::graph::SnapshotEnvelope;
use ail_storage::object::ObjectId;

// ── helpers ───────────────────────────────────────────────────────────

fn make_snapshot() -> SnapshotEnvelope {
    let id = ObjectId::from_bytes(b"builder-snap");
    SnapshotEnvelope {
        id,
        graph_root_hash: id,
        parent_id: None,
        applied_change_id: None,
        created_at: 1_000,
        verification_report_hash: None,
        ..Default::default()
    }
}

fn make_graph() -> SemanticGraph {
    // 3 nodes: 0 → 1 → 2 (chain)
    SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "core"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "run"),
            GraphNode::new(NodeRef(2), NodeKind::Effect, "io"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Emits),
        ],
    }
}

fn no_redactions() -> BTreeSet<NodeRef> {
    BTreeSet::new()
}

mod core;
mod history_provenance;
mod r2_attributes;
mod r2_queries;
mod traversal;
