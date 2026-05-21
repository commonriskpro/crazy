// ── ail-dogfood::tests::stdlib_projection ────────────────────────────────
//
// Integration tests for `project_stdlib_to_graph()`.
//
// # Coverage
//
// - Projected node count equals the registry entry count (9 nodes)
// - Projected graph passes structural validation
// - CBOR-encoded graph is BLAKE3-hash-stable across two independent calls
// - Projected node names match the registry entry `name` fields in order

use ail_dogfood::stdlib_projection::project_stdlib_to_graph;
use ail_stdlib::v1::v1_registry;
use ail_storage::codec::{CborCodec, ContentCodec};

// ── projected_node_count_equals_registry_entry_count ─────────────────────
// Spec scenario: "Projected graph node count equals registry entry count"
//   GIVEN v1_registry() is called (returns 9 entries)
//   WHEN project_stdlib_to_graph(registry) is called
//   THEN the resulting SemanticGraph contains exactly 9 module nodes
#[test]
fn projected_node_count_equals_registry_entry_count() {
    let registry = v1_registry();
    let expected_count = registry.entries.len();
    let graph = project_stdlib_to_graph(&registry);

    assert_eq!(
        graph.nodes.len(),
        expected_count,
        "projected node count must equal registry entry count; expected {expected_count}, got {}",
        graph.nodes.len()
    );
}

// ── projected_graph_passes_validate ──────────────────────────────────────
// Spec scenario: "Projected graph passes structural validation"
//   GIVEN the stdlib projection graph is built
//   WHEN validate() is called on it
//   THEN validation returns Ok(())
#[test]
fn projected_graph_passes_validate() {
    let registry = v1_registry();
    let graph = project_stdlib_to_graph(&registry);

    assert_eq!(
        graph.validate(),
        Ok(()),
        "projected stdlib graph must pass structural validation"
    );
}

// ── projected_graph_cbor_hash_is_stable ──────────────────────────────────
// Spec scenario: "Projected graph CBOR round-trip is hash-stable"
//   GIVEN the stdlib projection graph is encoded to CBOR
//   WHEN BLAKE3-hashed and re-encoded
//   THEN the hash is identical across two runs with no intervening mutations
#[test]
fn projected_graph_cbor_hash_is_stable() {
    let codec = CborCodec;

    let hash_a = {
        let registry = v1_registry();
        let graph = project_stdlib_to_graph(&registry);
        let bytes = codec.encode(&graph).expect("first encode must succeed");
        *blake3::hash(&bytes).as_bytes()
    };

    let hash_b = {
        let registry = v1_registry();
        let graph = project_stdlib_to_graph(&registry);
        let bytes = codec.encode(&graph).expect("second encode must succeed");
        *blake3::hash(&bytes).as_bytes()
    };

    assert_eq!(
        hash_a, hash_b,
        "BLAKE3 hash of CBOR-encoded stdlib projection must be identical across two independent calls"
    );
}

// ── projected_node_names_match_registry_entries ───────────────────────────
// Spec scenario: "node names match the name fields of the registry entries
//   in insertion order"
//   GIVEN the stdlib projection graph is built
//   WHEN node names are collected in NodeRef order
//   THEN they match registry entry `name` fields in insertion order
#[test]
fn projected_node_names_match_registry_entries() {
    let registry = v1_registry();
    let graph = project_stdlib_to_graph(&registry);

    let expected_names: Vec<&str> = registry.entries.iter().map(|e| e.name.as_str()).collect();

    // Nodes are inserted in registry order; sort by NodeRef to guarantee order
    let mut nodes = graph.nodes.clone();
    nodes.sort_by_key(|n| n.id);

    let actual_names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();

    assert_eq!(
        actual_names, expected_names,
        "projected node names must match registry entry `name` fields in insertion order;\n\
         expected: {expected_names:?}\n\
         actual:   {actual_names:?}"
    );
}
