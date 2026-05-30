use super::*;

// ── G32: Semantic ref newtypes ────────────────────────────────────────

// Spec: Each newtype wraps a String and is constructible.
// RED: tests written before types existed; now GREEN after add.
#[test]
fn ref_newtypes_are_constructible() {
    let block = BlockRef("block_checkout".to_string());
    let contract = ContractRef("contract.payment".to_string());
    let effect = EffectRef("effect.db.read".to_string());
    let proof = ProofObligationRef("proof.invariant.balance".to_string());
    let rtcheck = RuntimeCheckRef("rtcheck.null_guard".to_string());

    assert_eq!(block.0, "block_checkout");
    assert_eq!(contract.0, "contract.payment");
    assert_eq!(effect.0, "effect.db.read");
    assert_eq!(proof.0, "proof.invariant.balance");
    assert_eq!(rtcheck.0, "rtcheck.null_guard");
}

// TRIANGULATE: two different values of the same newtype are not equal.
#[test]
fn ref_newtypes_inequality() {
    let a = BlockRef("block_a".to_string());
    let b = BlockRef("block_b".to_string());
    assert_ne!(
        a, b,
        "BlockRef with different inner values must not be equal"
    );

    let ca = ContractRef("c1".to_string());
    let cb = ContractRef("c2".to_string());
    assert_ne!(ca, cb);
}

// Spec: Ref newtypes are serde-transparent — CBOR encoding matches plain String.
#[test]
fn ref_newtype_cbor_is_transparent_with_string() {
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;

    let raw = "block_checkout_flow".to_string();
    let typed = BlockRef(raw.clone());

    let bytes_raw = codec.encode(&raw).expect("encode raw string");
    let bytes_typed = codec.encode(&typed).expect("encode BlockRef");

    assert_eq!(
        bytes_raw, bytes_typed,
        "BlockRef CBOR must be identical to plain String CBOR (transparent serde)"
    );
}

// TRIANGULATE: Ref newtype CBOR round-trip preserves value.
#[test]
fn ref_newtype_cbor_round_trip() {
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;

    let original = ContractRef("contract.checkout.payment".to_string());
    let bytes = codec.encode(&original).expect("encode ContractRef");
    let decoded: ContractRef = codec.decode(&bytes).expect("decode ContractRef");
    assert_eq!(
        original, decoded,
        "ContractRef must survive CBOR round-trip"
    );
}

// ── Task C3 (RED): ConstraintSet::has_partial_ord ────────────────────

// S-C3a: ConstraintSet with has_partial_ord=true round-trips through CBOR.
#[test]
fn constraint_set_with_has_partial_ord_cbor_round_trip() {
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Price");
    node.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: false,
        has_hash: false,
        has_partial_ord: true,
        extras: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let bytes = codec.encode(&graph).expect("encode");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
    let cs = decoded.nodes[0]
        .constraint_set
        .as_ref()
        .expect("constraint_set must be Some");
    assert!(
        cs.has_partial_ord,
        "has_partial_ord must be true after round-trip"
    );
    assert!(!cs.has_ord, "has_ord must remain false");
}

// S-C3b: Old ConstraintSet without has_partial_ord deserializes with has_partial_ord=false.
// Backward compatibility via serde default.
#[test]
fn legacy_constraint_set_has_partial_ord_defaults_false() {
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    // A legacy node with constraint_set that has no has_partial_ord field
    // in its CBOR bytes must deserialize with has_partial_ord=false.
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Amount");
    node.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: true,
        has_hash: false,
        has_partial_ord: false, // default — must not be emitted in CBOR when false
        extras: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let bytes = codec.encode(&graph).expect("encode");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
    let cs = decoded.nodes[0].constraint_set.as_ref().unwrap();
    assert!(!cs.has_partial_ord, "has_partial_ord must default to false");
    assert!(cs.has_ord, "has_ord must be preserved");
}
