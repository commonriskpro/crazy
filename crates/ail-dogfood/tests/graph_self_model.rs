// ── ail-dogfood::tests::graph_self_model ──────────────────────────────────
//
// Integration tests for `build_graph_self_model()`.
//
// # Coverage
//
// - Structural validation passes (`validate() == Ok(())`)
// - CBOR round-trip determinism (encode → decode → re-encode → byte-equal)
// - Exact node count (10 nodes: 1 Module + 9 Type)
// - Meta module node present with correct kind and name
// - Triangulation: two distinct graphs produce different CBOR bytes

use ail_core::semantic_graph::{NodeKind, NodeRef};
use ail_dogfood::graph_self_model::build_graph_self_model;
use ail_storage::codec::{CborCodec, ContentCodec};

// ── self_model_passes_validate ────────────────────────────────────────────
// Spec scenario: "Self-model graph passes structural validation"
//   GIVEN build_graph_self_model() is called
//   WHEN validate() is called on the result
//   THEN validation returns Ok(())
#[test]
fn self_model_passes_validate() {
    let graph = build_graph_self_model();
    assert_eq!(
        graph.validate(),
        Ok(()),
        "self-model graph must pass structural validation"
    );
}

// ── self_model_cbor_round_trip_is_deterministic ───────────────────────────
// Spec scenario: "Self-model CBOR round-trip is deterministic"
//   GIVEN the self-model graph is encoded to CBOR bytes
//   WHEN the bytes are decoded and re-encoded
//   THEN the second encoding is byte-identical to the first
#[test]
fn self_model_cbor_round_trip_is_deterministic() {
    use ail_core::semantic_graph::SemanticGraph;

    let codec = CborCodec;
    let graph = build_graph_self_model();

    let bytes_a = codec.encode(&graph).expect("first encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes_a).expect("decode must succeed");
    let bytes_b = codec.encode(&decoded).expect("re-encode must succeed");

    assert_eq!(
        bytes_a, bytes_b,
        "CBOR round-trip must produce byte-identical output"
    );
}

// ── self_model_has_expected_node_count ────────────────────────────────────
// Spec: build_graph_self_model() produces exactly 10 nodes (1 Module + 9 Type)
#[test]
fn self_model_has_expected_node_count() {
    let graph = build_graph_self_model();
    assert_eq!(
        graph.nodes.len(),
        10,
        "self-model must contain exactly 10 nodes; got {}",
        graph.nodes.len()
    );
}

// ── self_model_has_meta_module_node ───────────────────────────────────────
// Spec: graph contains at least one NodeKind::Module node named "meta"
//   AND all type nodes have meta. prefix
#[test]
fn self_model_has_meta_module_node() {
    let graph = build_graph_self_model();

    let meta_node = graph
        .nodes
        .iter()
        .find(|n| n.id == NodeRef(0))
        .expect("NodeRef(0) must exist");

    assert_eq!(
        meta_node.kind,
        NodeKind::Module,
        "NodeRef(0) must be NodeKind::Module"
    );
    assert_eq!(meta_node.name, "meta", "NodeRef(0) must be named \"meta\"");

    // All other nodes must be Type nodes with meta. prefix
    let type_nodes: Vec<_> = graph.nodes.iter().filter(|n| n.id != NodeRef(0)).collect();
    assert_eq!(type_nodes.len(), 9, "must have 9 type nodes");
    for node in &type_nodes {
        assert_eq!(
            node.kind,
            NodeKind::Type,
            "node {} must be NodeKind::Type",
            node.name
        );
        assert!(
            node.name.starts_with("meta."),
            "type node name must start with 'meta.'; got '{}'",
            node.name
        );
    }
}

// ── different_graphs_differ_in_cbor ──────────────────────────────────────
// Triangulation: two structurally distinct graphs must NOT produce the
// same CBOR bytes.  Catches a trivially constant encoder.
#[test]
fn different_graphs_differ_in_cbor() {
    use ail_core::semantic_graph::{GraphNode, SemanticGraph};

    let codec = CborCodec;
    let self_model = build_graph_self_model();

    // A minimal, structurally different graph (single node, no edges)
    let other = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "other")],
        edges: vec![],
    };

    let bytes_self = codec.encode(&self_model).expect("encode self_model");
    let bytes_other = codec.encode(&other).expect("encode other");

    assert_ne!(
        bytes_self, bytes_other,
        "distinct graphs must produce different CBOR bytes"
    );
}
