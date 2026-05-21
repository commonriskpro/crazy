// ── tests/hash_tests.rs — Task 1.4 TDD cycle ─────────────────────────────
//
// RED: written before hash.rs production code existed.
// Tests call ail_compiler::hash::{hash_with_parent, stable_cbor_bytes}.

use ail_compiler::hash::{hash_with_parent, stable_cbor_bytes};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};

// Scenario: same inputs to hash_with_parent produce the same [u8; 32].
// Spec: "same inputs → same core_ir_hash"
#[test]
fn hash_with_parent_is_deterministic() {
    let parent = b"parent-bytes-for-test-seeding-ab";
    let content = b"some content to hash deterministically";
    let h1 = hash_with_parent(parent, content);
    let h2 = hash_with_parent(parent, content);
    assert_eq!(
        h1, h2,
        "hash_with_parent must return identical results for identical inputs"
    );
}

// TRIANGULATE: different content bytes produce different hashes.
// Proves the function is not a constant/trivial implementation.
#[test]
fn different_content_produces_different_hash() {
    let parent = b"same-parent-seed-value-for-test!";
    let h1 = hash_with_parent(parent, b"content-alpha");
    let h2 = hash_with_parent(parent, b"content-beta");
    assert_ne!(
        h1, h2,
        "distinct content bytes must produce distinct hashes"
    );
}

// TRIANGULATE: different parent bytes produce different hashes even with same content.
#[test]
fn different_parent_produces_different_hash() {
    let content = b"same-content-bytes";
    let h1 = hash_with_parent(b"parent-one", content);
    let h2 = hash_with_parent(b"parent-two", content);
    assert_ne!(h1, h2, "distinct parent bytes must produce distinct hashes");
}

// Scenario: stable_cbor_bytes is deterministic across two independent calls.
// Spec: "Re-serialization produces identical bytes"
#[test]
fn stable_cbor_bytes_is_deterministic() {
    let node = GraphNode::new(NodeRef(1), NodeKind::Function, "test_fn");
    let b1 = stable_cbor_bytes(&node).expect("first encode must succeed");
    let b2 = stable_cbor_bytes(&node).expect("second encode must succeed");
    assert_eq!(b1, b2, "identical inputs must produce identical CBOR bytes");
}

// TRIANGULATE: stable_cbor_bytes for different values are not equal.
#[test]
fn stable_cbor_bytes_differ_for_different_inputs() {
    let node_a = GraphNode::new(NodeRef(0), NodeKind::Module, "mod_a");
    let node_b = GraphNode::new(NodeRef(0), NodeKind::Module, "mod_b"); // different name
    let b_a = stable_cbor_bytes(&node_a).expect("encode a");
    let b_b = stable_cbor_bytes(&node_b).expect("encode b");
    assert_ne!(
        b_a, b_b,
        "different inputs must produce different CBOR bytes"
    );
}

// Scenario: hash_with_parent output is exactly 32 bytes.
#[test]
fn hash_output_is_32_bytes() {
    let h = hash_with_parent(b"p", b"c");
    assert_eq!(h.len(), 32, "BLAKE3 hash must be 32 bytes");
}
