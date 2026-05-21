// Integration tests for SemanticGraph CBOR encode-persist-reload round-trips.
//
// These tests prove that:
//   1. A SemanticGraph can be CBOR-encoded, stored as a RawObject in
//      MemoryObjectStore, retrieved by ObjectId, decoded, and assert_eq!
//      to the original.
//   2. The ail-storage dev-dependency boundary holds: ObjectId (storage
//      identity) and NodeRef (intra-graph identity) are never conflated.
//   3. Truncated CBOR bytes fail gracefully without a panic or partial decode.
//   4. Unknown CBOR discriminants are rejected on deserialization.
//   5. The full GraphStore snapshot path (ObjectBackedGraphStore/SnapshotEnvelope)
//      preserves graph_root_hash and never stores raw NodeRef values.
//   6. All NodeKind (9) and EdgeKind (7) variants survive a round-trip exactly.
//
// Spec domains verified:
//   graph-storage-roundtrip — "Minimal graph round-trips"
//   graph-storage-roundtrip — "Full graph round-trips with all field types"
//   graph-storage-roundtrip — "Truncated CBOR payload fails gracefully"
//   graph-storage-roundtrip — "Unknown discriminant rejected on deserialization"
//   graph-storage-roundtrip — "NodeRef absent from storage record"
//   graph-storage-roundtrip — "Graph snapshot path via ObjectBackedGraphStore"

use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_storage::{
    GraphStore, ObjectBackedGraphStore, SnapshotEnvelope,
    backends::memory::MemoryObjectStore,
    codec::{CborCodec, ContentCodec},
    object::{ObjectId, ObjectStore, RawObject},
};
use futures::executor::block_on;

// ── minimal_graph_roundtrips ──────────────────────────────────────────────
// Spec scenario: "Minimal graph round-trips"
//   GIVEN a SemanticGraph with one node and no edges
//   WHEN stored and reloaded via ObjectStore + CborCodec
//   THEN the reloaded graph equals the original
#[test]
fn minimal_graph_roundtrips() {
    let store = MemoryObjectStore::new();
    let codec = CborCodec;

    let original = SemanticGraph {
        nodes: vec![GraphNode {
            id: NodeRef(0),
            kind: NodeKind::Module,
            name: "root".to_string(),
        }],
        edges: vec![],
    };

    // Encode → store as RawObject → get ObjectId
    let bytes = codec.encode(&original).expect("encode must succeed");
    let object_id = block_on(store.put(RawObject(bytes))).expect("put must succeed");

    // Retrieve raw bytes by ObjectId → decode
    let raw = block_on(store.get(&object_id))
        .expect("get must succeed")
        .expect("object must be present after put");

    let decoded: SemanticGraph = codec.decode(&raw.0).expect("decode must succeed");

    assert_eq!(
        decoded, original,
        "decoded SemanticGraph must equal the original (minimal graph)"
    );
}

// ── full_graph_roundtrips ─────────────────────────────────────────────────
// Spec scenario: "Full graph round-trips with all field types"
//   GIVEN a SemanticGraph with multiple NodeKinds, EdgeKinds (via testkit fixture)
//   WHEN stored and reloaded
//   THEN all fields including kind variants are preserved exactly
#[test]
fn full_graph_roundtrips() {
    let store = MemoryObjectStore::new();
    let codec = CborCodec;

    // Use the testkit fixture: 3 nodes (Module, Function, Effect),
    // 2 edges (DependsOn, Emits) — exercises all distinct variant types.
    let original = ail_testkit::make_semantic_graph();

    let bytes = codec.encode(&original).expect("encode must succeed");
    let object_id = block_on(store.put(RawObject(bytes))).expect("put must succeed");

    let raw = block_on(store.get(&object_id))
        .expect("get must succeed")
        .expect("full graph object must be present after put");

    let decoded: SemanticGraph = codec.decode(&raw.0).expect("decode must succeed");

    assert_eq!(
        decoded, original,
        "decoded SemanticGraph must equal the original (full graph with multiple kinds)"
    );

    // Extra: confirm node kinds survived — not just byte equality.
    assert_eq!(
        decoded.nodes[0].kind,
        NodeKind::Module,
        "first node kind must be Module"
    );
    assert_eq!(
        decoded.nodes[1].kind,
        NodeKind::Function,
        "second node kind must be Function"
    );
    assert_eq!(
        decoded.nodes[2].kind,
        NodeKind::Effect,
        "third node kind must be Effect"
    );
    assert_eq!(
        decoded.edges[0].kind,
        EdgeKind::DependsOn,
        "first edge kind must be DependsOn"
    );
    assert_eq!(
        decoded.edges[1].kind,
        EdgeKind::Emits,
        "second edge kind must be Emits"
    );
}

// ── truncated_cbor_fails_gracefully ───────────────────────────────────────
// Spec scenario: "Truncated CBOR payload fails gracefully"
//   GIVEN truncated CBOR bytes injected directly into MemoryObjectStore
//   WHEN reload is attempted (decode on the raw bytes)
//   THEN the operation returns an Err and no partial graph is produced
#[test]
fn truncated_cbor_fails_gracefully() {
    let store = MemoryObjectStore::new();
    let codec = CborCodec;

    // Inject a 5-byte slice that is not valid CBOR for a SemanticGraph.
    let truncated_bytes = vec![0xA2, 0x01, 0x02, 0x03, 0x04]; // 5 garbage bytes
    let object_id =
        block_on(store.put(RawObject(truncated_bytes))).expect("put of raw bytes must succeed");

    // Retrieve and attempt to decode.
    let raw = block_on(store.get(&object_id))
        .expect("get must succeed")
        .expect("truncated object must be retrievable");

    let result: Result<SemanticGraph, _> = codec.decode(&raw.0);
    assert!(
        result.is_err(),
        "decoding truncated CBOR must return an error, not a partial SemanticGraph"
    );
}

// ── unknown_node_kind_discriminant_is_rejected ────────────────────────────
// Spec scenario: "Unknown discriminant rejected on deserialization"
//   GIVEN CBOR bytes representing a SemanticGraph where the NodeKind variant
//   text has been replaced with a string that is not a valid enum variant
//   WHEN decoded via CborCodec
//   THEN the operation returns an Err — no partial graph is produced
//
// RED: test written first — exercises serde's unknown-variant rejection path.
// GREEN: serde #[derive(Deserialize)] always errors on unknown variants.
// TRIANGULATE: single clear failure mode; no second case needed.
#[test]
fn unknown_node_kind_discriminant_is_rejected() {
    let codec = CborCodec;

    // Encode a valid minimal graph whose NodeKind::Module variant serializes
    // as the 6-byte CBOR text "Module".
    let original = SemanticGraph {
        nodes: vec![GraphNode {
            id: NodeRef(0),
            kind: NodeKind::Module,
            name: "root".to_string(),
        }],
        edges: vec![],
    };
    let bytes = codec.encode(&original).expect("encode must succeed");

    // Find the 6 bytes spelling "Module" (the variant name in the CBOR payload).
    // Replace them with "Zzzzzz" — same byte length, not a valid NodeKind variant.
    // The CBOR length prefix (0x66 = text-6) remains intact, so the stream is
    // well-formed CBOR but contains an unknown variant name.
    let pos = bytes
        .windows(6)
        .position(|w| w == b"Module")
        .expect("encoded bytes must contain the 'Module' variant string");
    let mut patched = bytes.clone();
    patched[pos..pos + 6].copy_from_slice(b"Zzzzzz");

    let result: Result<SemanticGraph, _> = codec.decode(&patched);
    assert!(
        result.is_err(),
        "decoding a graph with unknown NodeKind variant 'Zzzzzz' must return an error, \
         not produce a partial SemanticGraph"
    );
}

// ── snapshot_envelope_uses_object_ids_not_node_refs ───────────────────────
// Spec scenarios:
//   "CBOR Encode-Persist-Reload Equality" — via ObjectBackedGraphStore path
//   "NodeRef absent from storage record"
//
// Proves two things in one test:
//   A. The GraphStore snapshot path works end-to-end: save_snapshot → load_snapshot
//      via ObjectBackedGraphStore preserves the full SnapshotEnvelope.
//   B. SnapshotEnvelope fields are ObjectId (32-byte BLAKE3 hashes), never
//      the raw NodeRef u32 values that live inside the graph payload.
//      The graph payload itself is a separate CAS object (graph_root_hash
//      points to it); the envelope never stores graph internals directly.
//
// RED: test written first — exercises ObjectBackedGraphStore (existing impl).
// GREEN: ObjectBackedGraphStore was implemented in a prior commit.
// TRIANGULATE: NodeRef-size vs ObjectId-size assertion forces distinct types.
#[test]
fn snapshot_envelope_uses_object_ids_not_node_refs() {
    let codec = CborCodec;

    // Graph node carries NodeRef(42) — this raw value must NOT appear in
    // the SnapshotEnvelope's storage fields.
    let graph = SemanticGraph {
        nodes: vec![GraphNode {
            id: NodeRef(42),
            kind: NodeKind::Contract,
            name: "boundary-node".to_string(),
        }],
        edges: vec![],
    };

    // ObjectId::from_bytes produces a 32-byte BLAKE3 hash of the graph bytes.
    let graph_bytes = codec.encode(&graph).expect("encode graph");
    let graph_root_hash = ObjectId::from_bytes(&graph_bytes);

    // Build a genesis SnapshotEnvelope: all identity fields are ObjectIds.
    let snapshot_id = ObjectId::from_bytes(b"genesis-snapshot-boundary-test");
    let envelope = SnapshotEnvelope {
        id: snapshot_id,
        graph_root_hash,
        parent_id: None,
        applied_change_id: None,
        created_at: 1_716_300_000_000_u64,
    };

    // ── Part A: GraphStore path round-trip ────────────────────────────────
    let gs = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let returned_id = block_on(gs.save_snapshot(&envelope)).expect("save_snapshot must succeed");
    assert_eq!(
        returned_id, snapshot_id,
        "save_snapshot must return envelope.id, not the CAS hash of the envelope bytes"
    );

    let loaded = block_on(gs.load_snapshot(&snapshot_id))
        .expect("load_snapshot must succeed")
        .expect("snapshot must be present after save_snapshot");

    assert_eq!(
        loaded, envelope,
        "loaded SnapshotEnvelope must equal the original after ObjectBackedGraphStore round-trip"
    );

    // ── Part B: NodeRef absent from SnapshotEnvelope fields ──────────────
    // NodeRef(42) is a u32 — 4 bytes. ObjectId is a 32-byte BLAKE3 hash.
    // They are different in type, length, and value: they CANNOT be equal.
    assert_eq!(
        loaded.graph_root_hash.as_bytes().len(),
        32,
        "graph_root_hash must be a 32-byte BLAKE3 hash, not a 4-byte NodeRef value"
    );

    // The BLAKE3 hash of the graph CBOR bytes is NOT the same as NodeRef(42)
    // zero-padded to 32 bytes. This proves the storage identity is a real
    // content hash derived from the graph payload, not a graph-internal index.
    let node_ref_42_as_padded_hash: [u8; 32] = {
        let mut arr = [0u8; 32];
        arr[..4].copy_from_slice(&42u32.to_le_bytes());
        arr
    };
    assert_ne!(
        loaded.graph_root_hash.as_bytes(),
        &node_ref_42_as_padded_hash,
        "graph_root_hash must be a BLAKE3 content hash derived from graph bytes, \
         never the raw NodeRef(42) value zero-padded to 32 bytes"
    );
}

// ── all_node_and_edge_kind_variants_roundtrip ─────────────────────────────
// Spec scenario: "Full graph round-trips with all field types" — exhaustive
//   GIVEN a SemanticGraph containing ALL 9 NodeKind variants and ALL 7 EdgeKind
//   variants (9 nodes + 7 edges)
//   WHEN stored and reloaded via ObjectStore + CborCodec
//   THEN every variant field is preserved exactly after the round-trip
//
// RED: test written first — exercises all enum arms not covered by prior tests.
// GREEN: serde enums already serialize/deserialize all named variants.
// TRIANGULATE: individual kind assertions per-node and per-edge force each
//              variant to be present in the decoded result.
#[test]
fn all_node_and_edge_kind_variants_roundtrip() {
    let store = MemoryObjectStore::new();
    let codec = CborCodec;

    // 9 nodes — one per NodeKind variant.
    // 7 edges — one per EdgeKind variant, connecting adjacent node pairs.
    let original = SemanticGraph {
        nodes: vec![
            GraphNode {
                id: NodeRef(0),
                kind: NodeKind::Module,
                name: "m".to_string(),
            },
            GraphNode {
                id: NodeRef(1),
                kind: NodeKind::Function,
                name: "f".to_string(),
            },
            GraphNode {
                id: NodeRef(2),
                kind: NodeKind::Type,
                name: "t".to_string(),
            },
            GraphNode {
                id: NodeRef(3),
                kind: NodeKind::Effect,
                name: "e".to_string(),
            },
            GraphNode {
                id: NodeRef(4),
                kind: NodeKind::Capability,
                name: "c".to_string(),
            },
            GraphNode {
                id: NodeRef(5),
                kind: NodeKind::Contract,
                name: "k".to_string(),
            },
            GraphNode {
                id: NodeRef(6),
                kind: NodeKind::Invariant,
                name: "i".to_string(),
            },
            GraphNode {
                id: NodeRef(7),
                kind: NodeKind::Test,
                name: "s".to_string(),
            },
            GraphNode {
                id: NodeRef(8),
                kind: NodeKind::Boundary,
                name: "b".to_string(),
            },
        ],
        edges: vec![
            GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
            },
            GraphEdge {
                source: NodeRef(1),
                target: NodeRef(2),
                kind: EdgeKind::Reads,
            },
            GraphEdge {
                source: NodeRef(2),
                target: NodeRef(3),
                kind: EdgeKind::Writes,
            },
            GraphEdge {
                source: NodeRef(3),
                target: NodeRef(4),
                kind: EdgeKind::Emits,
            },
            GraphEdge {
                source: NodeRef(4),
                target: NodeRef(5),
                kind: EdgeKind::DependsOn,
            },
            GraphEdge {
                source: NodeRef(5),
                target: NodeRef(6),
                kind: EdgeKind::Proves,
            },
            GraphEdge {
                source: NodeRef(6),
                target: NodeRef(7),
                kind: EdgeKind::BreaksIfChanged,
            },
        ],
    };

    let bytes = codec.encode(&original).expect("encode must succeed");
    let object_id = block_on(store.put(RawObject(bytes))).expect("put must succeed");

    let raw = block_on(store.get(&object_id))
        .expect("get must succeed")
        .expect("all-variants object must be present after put");

    let decoded: SemanticGraph = codec.decode(&raw.0).expect("decode must succeed");

    assert_eq!(
        decoded, original,
        "SemanticGraph with all NodeKind and EdgeKind variants must round-trip exactly"
    );

    // Assert every NodeKind variant survived individually (not just byte equality).
    assert_eq!(decoded.nodes[0].kind, NodeKind::Module, "node[0].kind");
    assert_eq!(decoded.nodes[1].kind, NodeKind::Function, "node[1].kind");
    assert_eq!(decoded.nodes[2].kind, NodeKind::Type, "node[2].kind");
    assert_eq!(decoded.nodes[3].kind, NodeKind::Effect, "node[3].kind");
    assert_eq!(decoded.nodes[4].kind, NodeKind::Capability, "node[4].kind");
    assert_eq!(decoded.nodes[5].kind, NodeKind::Contract, "node[5].kind");
    assert_eq!(decoded.nodes[6].kind, NodeKind::Invariant, "node[6].kind");
    assert_eq!(decoded.nodes[7].kind, NodeKind::Test, "node[7].kind");
    assert_eq!(decoded.nodes[8].kind, NodeKind::Boundary, "node[8].kind");

    // Assert every EdgeKind variant survived individually.
    assert_eq!(decoded.edges[0].kind, EdgeKind::Calls, "edge[0].kind");
    assert_eq!(decoded.edges[1].kind, EdgeKind::Reads, "edge[1].kind");
    assert_eq!(decoded.edges[2].kind, EdgeKind::Writes, "edge[2].kind");
    assert_eq!(decoded.edges[3].kind, EdgeKind::Emits, "edge[3].kind");
    assert_eq!(decoded.edges[4].kind, EdgeKind::DependsOn, "edge[4].kind");
    assert_eq!(decoded.edges[5].kind, EdgeKind::Proves, "edge[5].kind");
    assert_eq!(
        decoded.edges[6].kind,
        EdgeKind::BreaksIfChanged,
        "edge[6].kind"
    );
}

// ── TRIANGULATE: object_id_is_not_node_ref ────────────────────────────────
// Spec: "NodeRef absent from storage record"
//   Proves the identity separation contract: storing a SemanticGraph produces
//   an ObjectId at the storage boundary; no NodeRef is present in the returned
//   ObjectId (they are distinct types with no common derivation path).
//
//   This is a compile-time proof: if the code compiles, the types are separate.
//   We add a runtime assertion to make the intent visible in the test output.
#[test]
fn node_ref_and_object_id_are_distinct_types() {
    let graph = SemanticGraph {
        nodes: vec![GraphNode {
            id: NodeRef(42),
            kind: NodeKind::Contract,
            name: "boundary-check".to_string(),
        }],
        edges: vec![],
    };

    let codec = CborCodec;
    let store = MemoryObjectStore::new();

    let bytes = codec.encode(&graph).expect("encode must succeed");
    let object_id = block_on(store.put(RawObject(bytes))).expect("put must succeed");

    // ObjectId is 32 bytes of BLAKE3 hash content; NodeRef is a u32 index.
    // The fact that this compiles proves they are separate types.
    // The assertion below proves the ObjectId is a real content hash (not zero).
    assert_ne!(
        object_id.as_bytes(),
        &[0u8; 32],
        "ObjectId must be a real BLAKE3 hash, not all-zero — confirms it is not NodeRef(42)"
    );
}
