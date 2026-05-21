// ── ail-core: semantic facts CBOR round-trip tests ───────────────────────
//
// TDD approval (task 1.1): backward-compat golden-bytes check.
//   Compiles and passes BEFORE new optional fields are added; continues to
//   pass after adding them with `#[serde(default, skip_serializing_if = …)]`.
//
// TDD RED (task 1.4): populated TypeFacts/EffectRow/CapabilityReqs tests are
//   added below after tasks 1.2 + 1.3 introduce those types.

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_storage::codec::{CborCodec, ContentCodec};

// ── GOLDEN BYTES ──────────────────────────────────────────────────────────
//
// Captured from CborCodec::encode on:
//   SemanticGraph {
//     nodes: [GraphNode { id: NodeRef(0), kind: Module, name: "root" }],
//     edges: [],
//   }
//
// CBOR structure (human-readable):
//   {
//     "nodes": [ {"id": 0, "kind": "Module", "name": "root"} ],
//     "edges": []
//   }
//
// DO NOT change these bytes unless you are intentionally making a backward-
// incompatible Phase 1–4 wire-format change.  Adding new `Option<T>` fields
// with `skip_serializing_if = "Option::is_none"` MUST NOT change these bytes.
#[rustfmt::skip]
const GOLDEN_PHASE_14_BYTES: [u8; 42] = [
    0xa2,                               // map(2)
      0x65, 0x6e, 0x6f, 0x64, 0x65, 0x73, // "nodes"
      0x81,                             //   array(1)
        0xa3,                           //     map(3)
          0x62, 0x69, 0x64,             //       "id"
            0x00,                       //         0
          0x64, 0x6b, 0x69, 0x6e, 0x64, //       "kind"
            0x66, 0x4d, 0x6f, 0x64, 0x75, 0x6c, 0x65, // "Module"
          0x64, 0x6e, 0x61, 0x6d, 0x65, //       "name"
            0x64, 0x72, 0x6f, 0x6f, 0x74, // "root"
      0x65, 0x65, 0x64, 0x67, 0x65, 0x73, // "edges"
      0x80,                             //   array(0)
];

// ── helpers ───────────────────────────────────────────────────────────────

/// Build a minimal Phase 1–4 SemanticGraph using only the original fields.
/// After tasks 1.2+1.3 add optional fields, this helper will need to be
/// updated to use `GraphNode::new(…)` or set new fields explicitly to None.
fn minimal_phase_14_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "root")],
        edges: vec![],
    }
}

// ── Task 1.1: approval tests (GREEN before AND after adding new fields) ───

// ── backward_compat_old_graph_roundtrips ─────────────────────────────────
// Spec scenario: "Existing node construction unchanged"
//   GIVEN a SemanticGraph built with Phase 1–4 struct literals
//   WHEN serialized and deserialized via CBOR
//   THEN the round-trip is lossless and byte-identical
//
// Approval test: ensures adding new serde fields doesn't break existing graphs.
#[test]
fn backward_compat_old_graph_roundtrips() {
    let codec = CborCodec;
    let original = minimal_phase_14_graph();

    let bytes = codec.encode(&original).expect("encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

    assert_eq!(
        decoded, original,
        "Phase 1–4 graph must round-trip losslessly through CBOR"
    );

    // TRIANGULATE: determinism — encode the same graph twice → identical bytes.
    let bytes2 = codec.encode(&original).expect("re-encode must succeed");
    assert_eq!(
        bytes, bytes2,
        "Phase 1–4 graph CBOR encoding must be deterministic"
    );
}

// ── golden_bytes_verify_no_cbor_drift ────────────────────────────────────
// Spec scenario: "Existing node construction unchanged" — byte-level
//   GIVEN the hard-coded GOLDEN_PHASE_14_BYTES constant
//   WHEN decoded and re-encoded with the current serde impl
//   THEN the re-encoded bytes MUST equal the original golden bytes
//
// This catches any serde attribute change that would alter the Phase 1–4
// wire format (e.g., accidentally removing `skip_serializing_if` so that
// `None` fields emit null bytes and increase the payload size).
#[test]
fn golden_bytes_verify_no_cbor_drift() {
    let codec = CborCodec;
    let golden = &GOLDEN_PHASE_14_BYTES[..];

    // Decode golden bytes into the current struct (with new optional fields
    // defaulting to None via `#[serde(default)]`).
    let decoded: SemanticGraph = codec
        .decode(golden)
        .expect("golden bytes must decode into SemanticGraph without error");

    // Structural assertions.
    assert_eq!(decoded.nodes.len(), 1, "golden graph must have 1 node");
    assert_eq!(decoded.edges.len(), 0, "golden graph must have 0 edges");
    assert_eq!(
        decoded.nodes[0].id,
        NodeRef(0),
        "node id must be NodeRef(0)"
    );
    assert_eq!(
        decoded.nodes[0].kind,
        NodeKind::Module,
        "node kind must be Module"
    );
    assert_eq!(decoded.nodes[0].name, "root", "node name must be 'root'");

    // Re-encode and compare to golden bytes — proves no CBOR drift.
    let re_encoded = codec
        .encode(&decoded)
        .expect("re-encode of decoded graph must succeed");
    assert_eq!(
        re_encoded.as_slice(),
        golden,
        "re-encoding a decoded Phase 1–4 graph must produce bytes identical \
         to the original golden baseline (no CBOR drift after adding optional fields)"
    );
}

// ── Task 1.4 RED tests ────────────────────────────────────────────────────
// These tests reference `TypeFacts`, `EffectRow`, `CapabilityReqs`, and
// `GraphNode::new(…)` which do NOT exist yet (tasks 1.2 + 1.3).
// Adding them here is the RED phase — the file will NOT compile until the
// production types are introduced.

use ail_core::semantic_graph::{CapabilityReqs, EffectRow, TypeFacts};

// ── type_facts_roundtrips_via_cbor ────────────────────────────────────────
// Spec scenario: "Optional field attached and retrievable"
//   GIVEN a GraphNode built with GraphNode::new(id, kind, name)
//   WHEN type_facts is set to Some(TypeFacts { nominal: "Int", generics: [] })
//   THEN the node round-trips via CBOR and the populated field is preserved
//
// RED: TypeFacts and GraphNode::new do not exist → compile failure is expected.
// GREEN: tasks 1.2 + 1.3 introduce the types and constructor.
// TRIANGULATE: second case with generics populated forces non-trivial encoding.
#[test]
fn type_facts_roundtrips_via_cbor() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "add");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".to_string(),
        generics: vec![],
    });

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes = codec.encode(&graph).expect("encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

    let tf = decoded.nodes[0]
        .type_facts
        .as_ref()
        .expect("type_facts must survive round-trip");
    assert_eq!(tf.nominal, "Int", "nominal must round-trip as 'Int'");
    assert!(tf.generics.is_empty(), "generics must remain empty");
}

// ── TRIANGULATE: type_facts_with_generics_roundtrips ─────────────────────
// Spec scenario: "Optional field attached and retrievable" — with generics
//   GIVEN a TypeFacts with non-empty generics vec
//   WHEN encoded and decoded via CBOR
//   THEN all generics are preserved in order
#[test]
fn type_facts_with_generics_roundtrips() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(1), NodeKind::Type, "Map");
    node.type_facts = Some(TypeFacts {
        nominal: "Map".to_string(),
        generics: vec!["Key".to_string(), "Value".to_string()],
    });

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes = codec.encode(&graph).expect("encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

    let tf = decoded.nodes[0]
        .type_facts
        .as_ref()
        .expect("type_facts with generics must survive round-trip");
    assert_eq!(tf.nominal, "Map", "nominal must be 'Map'");
    assert_eq!(
        tf.generics,
        &["Key", "Value"],
        "generics must preserve order"
    );
}

// ── all_three_facts_deterministic ─────────────────────────────────────────
// Spec scenario: "Deterministic serialization"
//   GIVEN a GraphNode with all three optional fields populated
//   WHEN CBOR-serialized twice in sequence
//   THEN both byte sequences MUST be identical
#[test]
fn all_three_facts_deterministic() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "run");
    node.type_facts = Some(TypeFacts {
        nominal: "Unit".to_string(),
        generics: vec![],
    });
    node.effect_row = Some(EffectRow {
        effects: vec!["IO".to_string()],
    });
    node.capability_reqs = Some(CapabilityReqs {
        caps: vec!["net:read".to_string()],
    });

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes_a = codec.encode(&graph).expect("first encode must succeed");
    let bytes_b = codec.encode(&graph).expect("second encode must succeed");

    assert_eq!(
        bytes_a, bytes_b,
        "serializing the same graph with all three facts twice must produce identical bytes"
    );
}

// ── none_facts_produce_golden_bytes ──────────────────────────────────────
// Spec scenario: "Existing node construction unchanged"
//   GIVEN a GraphNode created with GraphNode::new(…) (all optional fields None)
//   WHEN serialized to CBOR
//   THEN the bytes MUST be identical to GOLDEN_PHASE_14_BYTES
//
// This is the critical backward-compat assertion: the new constructor must
// produce EXACTLY the same bytes as the old struct literal construction.
#[test]
fn none_facts_produce_golden_bytes() {
    let codec = CborCodec;

    // New-style construction — optional fields default to None.
    let node = GraphNode::new(NodeRef(0), NodeKind::Module, "root");
    let graph_new = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes_new = codec
        .encode(&graph_new)
        .expect("encode new-style must succeed");

    assert_eq!(
        bytes_new.as_slice(),
        &GOLDEN_PHASE_14_BYTES[..],
        "GraphNode::new with all-None optional fields must produce the same CBOR \
         bytes as the Phase 1–4 struct literal (backward-compat proof)"
    );
}
