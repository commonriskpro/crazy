use super::*;

#[test]
fn valid_graph_passes_validation() {
    let graph = SemanticGraph {
        nodes: vec![
            node(0, NodeKind::Module, "core"),
            node(1, NodeKind::Function, "run"),
            node(2, NodeKind::Type, "Config"),
        ],
        edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Reads)],
    };
    assert_eq!(graph.validate(), Ok(()));
}

// ── duplicate_node_ref_is_rejected ────────────────────────────────────
// Spec scenario: "Duplicate NodeRef is rejected"
//   GIVEN a graph builder that inserts two nodes both with NodeRef(0)
//   WHEN validate() is called
//   THEN validation returns Err identifying the duplicate ref
#[test]
fn duplicate_node_ref_is_rejected() {
    let graph = SemanticGraph {
        nodes: vec![
            node(0, NodeKind::Module, "a"),
            node(0, NodeKind::Function, "b"), // duplicate!
        ],
        edges: vec![],
    };
    assert_eq!(
        graph.validate(),
        Err(GraphValidationError::DuplicateRef(NodeRef(0)))
    );
}

// ── dangling_edge_source_is_rejected ──────────────────────────────────
// Spec scenario: "Edge with missing source is rejected"
//   GIVEN a graph containing NodeRef(1) but not NodeRef(99)
//   WHEN an edge (NodeRef(99) → NodeRef(1)) is added and validate() called
//   THEN validation returns Err naming the missing source ref
#[test]
fn dangling_edge_source_is_rejected() {
    let graph = SemanticGraph {
        nodes: vec![node(1, NodeKind::Function, "target_fn")],
        edges: vec![edge(99, 1, EdgeKind::Calls)], // source 99 is missing
    };
    assert_eq!(
        graph.validate(),
        Err(GraphValidationError::DanglingEdge {
            r#ref: NodeRef(99),
            role: DanglingRole::Source,
        })
    );
}

// ── dangling_edge_target_is_rejected ──────────────────────────────────
// Spec scenario: "Edge with missing target"
//   GIVEN a graph containing NodeRef(0) but not NodeRef(77)
//   WHEN an edge (NodeRef(0) → NodeRef(77)) is added and validate() called
//   THEN validation returns Err naming the missing target ref
#[test]
fn dangling_edge_target_is_rejected() {
    let graph = SemanticGraph {
        nodes: vec![node(0, NodeKind::Module, "source_mod")],
        edges: vec![edge(0, 77, EdgeKind::DependsOn)], // target 77 is missing
    };
    assert_eq!(
        graph.validate(),
        Err(GraphValidationError::DanglingEdge {
            r#ref: NodeRef(77),
            role: DanglingRole::Target,
        })
    );
}

// ── TRIANGULATE: edge_with_present_endpoints_passes ───────────────────
// Spec scenario: "Edge with present endpoints passes"
//   GIVEN a graph with NodeRef(0) and NodeRef(1)
//   WHEN an edge (NodeRef(0) → NodeRef(1)) is added and validate() called
//   THEN validation returns Ok(())
//
// Different from valid_graph_passes_validation: single edge, minimal setup.
#[test]
fn edge_with_present_endpoints_passes() {
    let graph = SemanticGraph {
        nodes: vec![
            node(0, NodeKind::Module, "src"),
            node(1, NodeKind::Module, "dst"),
        ],
        edges: vec![edge(0, 1, EdgeKind::DependsOn)],
    };
    assert_eq!(graph.validate(), Ok(()));
}

// ── TRIANGULATE: empty_graph_passes_validation ────────────────────────
// Edge case: a graph with no nodes and no edges is structurally valid.
#[test]
fn empty_graph_passes_validation() {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    assert_eq!(graph.validate(), Ok(()));
}

// ── cbor_encodes_deterministically ────────────────────────────────────
// Spec scenario: "Re-serialization produces identical bytes"
//   GIVEN a SemanticGraph serialized to CBOR
//   WHEN the bytes are deserialized and re-serialized
//   THEN the output bytes are identical to the original
//
// Uses ail_storage::codec::CborCodec — added as dev-dependency.
