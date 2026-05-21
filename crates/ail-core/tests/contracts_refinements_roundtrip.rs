// ── ail-core: contracts & refinements CBOR round-trip tests ─────────────
//
// TDD RED (task 1.1): Written BEFORE ContractClauses / RuntimeCheckMeta exist
// in semantic_graph.rs.  This file WILL NOT compile until tasks 1.2–1.4
// introduce the types and fields.
//
// Spec domains verified:
//   CRM-1 — ContractClauses uses Vec<String> (no HashMap) for requires/ensures
//   CRM-2 — RuntimeCheckMeta { predicate, hash } is serde-serializable
//   CRM-3 — contract_clauses uses skip_serializing_if = "Option::is_none"
//   CRM-4 — GraphNode::new sets contract_clauses: None (no source-compat break)

use ail_core::semantic_graph::{
    ContractClauses, GraphNode, NodeKind, NodeRef, RuntimeCheckMeta, SemanticGraph,
};
use ail_storage::codec::{CborCodec, ContentCodec};

// ── GOLDEN BYTES (Phase 1–5 baseline) ────────────────────────────────────
//
// A single-node, no-edges graph built with GraphNode::new + all optional fields
// absent.  Adding Phase 6 fields with `skip_serializing_if = "Option::is_none"`
// MUST NOT change these bytes — otherwise old readers would see a different
// payload and backward compatibility would be broken.
//
// CBOR structure (human-readable):
//   { "nodes": [ {"id": 0, "kind": "Module", "name": "root"} ], "edges": [] }
//
// DO NOT change these bytes unless you intend a deliberate wire-format break.
#[rustfmt::skip]
const GOLDEN_PHASE5_BYTES: [u8; 42] = [
    0xa2,                                           // map(2)
      0x65, 0x6e, 0x6f, 0x64, 0x65, 0x73,           // "nodes"
      0x81,                                         //   array(1)
        0xa3,                                       //     map(3)
          0x62, 0x69, 0x64,                         //       "id"
            0x00,                                   //         0
          0x64, 0x6b, 0x69, 0x6e, 0x64,             //       "kind"
            0x66, 0x4d, 0x6f, 0x64, 0x75, 0x6c, 0x65, //  "Module"
          0x64, 0x6e, 0x61, 0x6d, 0x65,             //       "name"
            0x64, 0x72, 0x6f, 0x6f, 0x74,           //       "root"
      0x65, 0x65, 0x64, 0x67, 0x65, 0x73,           // "edges"
      0x80,                                         //   array(0)
];

// ── contract_clauses_roundtrip ────────────────────────────────────────────
// Spec scenario: CRM-3 — Contract field round-trips correctly
//   GIVEN a GraphNode with contract_clauses: Some(ContractClauses {
//             requires: ["x > 0"], ensures: ["result >= 0"] })
//   WHEN CBOR-encoded with CborCodec and decoded
//   THEN the decoded contract_clauses equals the original value
//
// RED: ContractClauses type and GraphNode.contract_clauses field do not exist.
// GREEN: tasks 1.2 + 1.3 introduce ContractClauses and the field.
// TRIANGULATE: see empty_requires_vec_preserved below.
#[test]
fn contract_clauses_roundtrip() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Contract, "invariant_fn");
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["x > 0".to_string()],
        ensures: vec!["result >= 0".to_string()],
    });

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes = codec.encode(&graph).expect("encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

    let cc = decoded.nodes[0]
        .contract_clauses
        .as_ref()
        .expect("contract_clauses must survive CBOR round-trip");

    assert_eq!(
        cc.requires,
        vec!["x > 0"],
        "requires clause must round-trip exactly"
    );
    assert_eq!(
        cc.ensures,
        vec!["result >= 0"],
        "ensures clause must round-trip exactly"
    );
}

// ── TRIANGULATE: empty_requires_vec_preserved ────────────────────────────
// Spec scenario: CRM-1 — Empty clause lists are preserved
//   GIVEN ContractClauses { requires: [], ensures: ["result >= 0"] }
//   WHEN round-tripped through CBOR
//   THEN requires is an empty Vec and ensures contains exactly one entry
//
// Forces non-trivial encoding: empty Vec is NOT the same as absent Option.
#[test]
fn empty_requires_vec_preserved() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "safe_fn");
    node.contract_clauses = Some(ContractClauses {
        requires: vec![],
        ensures: vec!["result >= 0".to_string()],
    });

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes = codec.encode(&graph).expect("encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

    let cc = decoded.nodes[0]
        .contract_clauses
        .as_ref()
        .expect("contract_clauses must survive round-trip");

    assert!(
        cc.requires.is_empty(),
        "empty requires Vec must round-trip as empty (not None)"
    );
    assert_eq!(
        cc.ensures,
        vec!["result >= 0"],
        "ensures must contain exactly one entry after round-trip"
    );
}

// ── no_contract_node_matches_phase5_golden ────────────────────────────────
// Spec scenario: CRM-3 — No-contract node is byte-identical to Phase 5 output
//   GIVEN a GraphNode constructed via GraphNode::new (contract_clauses = None)
//   WHEN CBOR-encoded by Phase 6 code
//   THEN the output bytes are IDENTICAL to the Phase 5 golden baseline
//
// This is the critical backward-compat assertion: Phase 6 MUST NOT produce a
// larger payload for nodes that do not use contract fields.
#[test]
fn no_contract_node_matches_phase5_golden() {
    let codec = CborCodec;

    let node = GraphNode::new(NodeRef(0), NodeKind::Module, "root");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes = codec.encode(&graph).expect("encode must succeed");

    assert_eq!(
        bytes.as_slice(),
        &GOLDEN_PHASE5_BYTES[..],
        "GraphNode::new with no contract fields must produce byte-identical CBOR to the \
         Phase 5 golden baseline (no CBOR drift)"
    );
}

// ── TRIANGULATE: runtime_check_meta_roundtrips ────────────────────────────
// Spec scenario: CRM-2 — RuntimeCheckMeta is serde-serializable
//   GIVEN a GraphNode with runtime_checks: Some([RuntimeCheckMeta { predicate, hash }])
//   WHEN CBOR round-tripped
//   THEN the predicate and hash are preserved exactly
//
// RED: RuntimeCheckMeta type and GraphNode.runtime_checks field do not exist.
// GREEN: task 1.2 adds RuntimeCheckMeta; task 1.3 adds the field.
#[test]
fn runtime_check_meta_roundtrips() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "checked_fn");
    node.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "x != null".to_string(),
        hash: "abc123def456".to_string(),
    }]);

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes = codec.encode(&graph).expect("encode must succeed");
    let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

    let checks = decoded.nodes[0]
        .runtime_checks
        .as_ref()
        .expect("runtime_checks must survive CBOR round-trip");

    assert_eq!(
        checks.len(),
        1,
        "must have exactly one RuntimeCheckMeta after round-trip"
    );
    assert_eq!(
        checks[0].predicate, "x != null",
        "predicate must round-trip exactly"
    );
    assert_eq!(
        checks[0].hash, "abc123def456",
        "hash must round-trip exactly"
    );
}

// ── TRIANGULATE: contract_and_runtime_together_deterministic ─────────────
// Spec: CRM-1/CRM-2 combined — Both fields populated → deterministic encoding
//   GIVEN a GraphNode with both contract_clauses and runtime_checks populated
//   WHEN CBOR-encoded twice in sequence
//   THEN both byte sequences are identical (determinism guarantee)
#[test]
fn contract_and_runtime_together_deterministic() {
    let codec = CborCodec;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Contract, "dual_node");
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["a > 0".to_string(), "b > 0".to_string()],
        ensures: vec!["a + b > 0".to_string()],
    });
    node.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "a != null".to_string(),
        hash: "deadbeef".to_string(),
    }]);

    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let bytes_a = codec.encode(&graph).expect("first encode must succeed");
    let bytes_b = codec.encode(&graph).expect("second encode must succeed");

    assert_eq!(
        bytes_a, bytes_b,
        "encoding the same contract+runtime graph twice must produce identical bytes"
    );
}
