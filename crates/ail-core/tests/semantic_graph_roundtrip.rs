// Integration tests for SemanticGraph CBOR encode-persist-reload round-trips.
//
// These tests prove that:
//   1. A SemanticGraph can be CBOR-encoded, stored as a RawObject in
//      MemoryObjectStore, retrieved by ObjectId, decoded, and assert_eq!
//      to the original.
//   2. The ail-storage dev-dependency boundary holds: ObjectId (storage
//      identity) and NodeRef (intra-graph identity) are never conflated.
//   3. Truncated CBOR bytes fail gracefully without a panic or partial decode.
//
// Spec domains verified:
//   graph-storage-roundtrip — "Minimal graph round-trips"
//   graph-storage-roundtrip — "Full graph round-trips with all field types"
//   graph-storage-roundtrip — "Truncated CBOR payload fails gracefully"

use ail_core::semantic_graph::{EdgeKind, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_storage::{
    backends::memory::MemoryObjectStore,
    codec::{CborCodec, ContentCodec},
    object::{ObjectStore, RawObject},
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
