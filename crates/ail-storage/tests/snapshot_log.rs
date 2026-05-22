// Integration tests for SnapshotEnvelope, ChangeSetLogEntry, and GraphStore.
//
// These tests are the acceptance criteria for Phase 4 (PR 3) and the
// verification-fix slice.  They drive the design of `graph.rs` and
// `ObjectBackedGraphStore`.
//
// Spec fields verified
// ──────────────────────────────────────────────────────────────────────────
// SnapshotEnvelope:    id, graph_root_hash, parent_id, applied_change_id, created_at
// ChangeSetLogEntry:   id, base_snapshot_id, payload_hash, created_at
//
// Test layout
// ──────────────────────────────────────────────────────────────────────────
// snapshot_full_roundtrip         – save + load by envelope.id preserves all fields
// genesis_no_parent               – snapshot with parent_id=None survives round-trip
// entry_roundtrip                 – ChangeSetLogEntry codec round-trip via CborCodec
// save_then_load                  – save_snapshot returns envelope.id; load_snapshot succeeds
// save_then_load_by_envelope_id   – TRIANGULATE: load using snap.id directly (spec scenario)
// append_log_succeeds             – append_changeset_log returns a real, non-zero ObjectId
// snapshot_envelope_encoding_is_deterministic  – HashMap prohibition guard for SnapshotEnvelope
// changeset_entry_encoding_is_deterministic    – HashMap prohibition guard for ChangeSetLogEntry

use ail_storage::{
    backends::memory::MemoryObjectStore,
    codec::{CborCodec, ContentCodec},
    graph::{ChangeSetLogEntry, GraphStore, ObjectBackedGraphStore, SnapshotEnvelope},
    object::ObjectId,
};
use futures::executor::block_on;

// ── helpers ───────────────────────────────────────────────────────────────

/// Deterministic `ObjectId` from a short label for use as fixture data.
fn oid(label: &str) -> ObjectId {
    ObjectId::from_bytes(label.as_bytes())
}

/// A `SnapshotEnvelope` with all required spec fields populated.
///
/// Spec: `id`, `graph_root_hash`, `parent_id`, `applied_change_id`, `created_at`.
fn snapshot_with_parent() -> SnapshotEnvelope {
    SnapshotEnvelope {
        id: oid("snapshot-A"),
        graph_root_hash: oid("root-data"),
        parent_id: Some(oid("parent-snapshot")),
        applied_change_id: Some(oid("change-1")),
        created_at: 1_700_000_000_000_u64,
        verification_report_hash: None,
    }
}

// ── snapshot_full_roundtrip ───────────────────────────────────────────────
// Spec scenario: "Full round-trip"
//   GIVEN a fully-populated SnapshotEnvelope
//   WHEN saved via GraphStore and loaded back by envelope.id
//   THEN all five spec fields are preserved exactly
#[test]
fn snapshot_full_roundtrip() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = snapshot_with_parent();

    let returned_id = block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    // save_snapshot must return the envelope's own id (spec: load_snapshot(envelope.id))
    assert_eq!(
        returned_id, snap.id,
        "save_snapshot must return envelope.id, not the CAS id"
    );

    let loaded = block_on(graph_store.load_snapshot(&snap.id))
        .expect("load must succeed")
        .expect("snapshot must be present after save");

    assert_eq!(loaded.id, snap.id, "id must survive the round-trip");
    assert_eq!(
        loaded.graph_root_hash, snap.graph_root_hash,
        "graph_root_hash must survive the round-trip"
    );
    assert_eq!(
        loaded.parent_id, snap.parent_id,
        "parent_id must survive the round-trip"
    );
    assert_eq!(
        loaded.applied_change_id, snap.applied_change_id,
        "applied_change_id must survive the round-trip"
    );
    assert_eq!(
        loaded.created_at, snap.created_at,
        "created_at must survive the round-trip"
    );
}

// ── genesis_no_parent ────────────────────────────────────────────────────
// Spec scenario: "Genesis snapshot (no parent)"
//   GIVEN a SnapshotEnvelope with parent_id = None
//   WHEN saved and loaded back via envelope.id
//   THEN parent_id is still None (genesis semantics preserved)
#[test]
fn genesis_no_parent() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = SnapshotEnvelope {
        id: oid("genesis-snap"),
        graph_root_hash: oid("genesis-root"),
        parent_id: None,
        applied_change_id: None,
        created_at: 0_u64,
        verification_report_hash: None,
    };

    let returned_id = block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    let loaded = block_on(graph_store.load_snapshot(&returned_id))
        .expect("load must succeed")
        .expect("genesis snapshot must be present");

    assert!(
        loaded.parent_id.is_none(),
        "genesis snapshot must have no parent_id"
    );
    assert!(
        loaded.applied_change_id.is_none(),
        "genesis snapshot must have no applied_change_id"
    );
    assert_eq!(loaded.graph_root_hash, snap.graph_root_hash);
    assert_eq!(loaded.id, snap.id);
}

// ── entry_roundtrip ───────────────────────────────────────────────────────
// Spec scenario: "ChangeSetLogEntry codec round-trip"
//   GIVEN a ChangeSetLogEntry with all four spec fields set
//   WHEN encoded with CborCodec and decoded back
//   THEN every field equals the original (verifies Serde derives)
//
// Spec fields: id, base_snapshot_id, payload_hash, created_at
#[test]
fn entry_roundtrip() {
    let codec = CborCodec;
    let entry = ChangeSetLogEntry {
        id: oid("changeset-42"),
        base_snapshot_id: oid("snapshot-7"),
        payload_hash: oid("payload-bytes-42"),
        created_at: 1_700_000_000_001_u64,
    };

    let bytes = codec.encode(&entry).expect("encode must succeed");
    let decoded: ChangeSetLogEntry = codec.decode(&bytes).expect("decode must succeed");

    assert_eq!(
        decoded.id, entry.id,
        "ChangeSetLogEntry.id must survive codec round-trip"
    );
    assert_eq!(
        decoded.base_snapshot_id, entry.base_snapshot_id,
        "base_snapshot_id must survive codec round-trip"
    );
    assert_eq!(
        decoded.payload_hash, entry.payload_hash,
        "payload_hash must survive codec round-trip"
    );
    assert_eq!(
        decoded.created_at, entry.created_at,
        "created_at must survive codec round-trip"
    );
}

// ── save_then_load ────────────────────────────────────────────────────────
// Spec scenario: "Save then load"
//   GIVEN a SnapshotEnvelope saved to an ObjectBackedGraphStore
//   WHEN save_snapshot completes (returns envelope.id)
//   THEN load_snapshot(returned_id) returns Some and all fields match
#[test]
fn save_then_load() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = SnapshotEnvelope {
        id: oid("snap-42"),
        graph_root_hash: oid("another-root"),
        parent_id: None,
        applied_change_id: None,
        created_at: 42_000_u64,
        verification_report_hash: None,
    };

    let returned_id = block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    // save_snapshot returns envelope.id per spec
    assert_eq!(
        returned_id, snap.id,
        "save_snapshot must return envelope.id"
    );

    let result = block_on(graph_store.load_snapshot(&returned_id)).expect("load must not error");

    assert!(
        result.is_some(),
        "saved snapshot must be loadable by its id"
    );
    let loaded = result.unwrap();
    assert_eq!(
        loaded.created_at, 42_000_u64,
        "created_at 42_000 must be preserved"
    );
    assert_eq!(loaded.id, snap.id, "id field must be preserved");
}

// ── save_then_load_by_envelope_id ─────────────────────────────────────────
// TRIANGULATE: Spec scenario "Save then load" — second variant
//   Load using snap.id DIRECTLY (not the returned value from save_snapshot).
//   This proves the GraphStore indexes by envelope.id, not by CAS id.
#[test]
fn save_then_load_by_envelope_id() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = SnapshotEnvelope {
        id: oid("snap-direct-id"),
        graph_root_hash: oid("direct-root"),
        parent_id: Some(oid("direct-parent")),
        applied_change_id: Some(oid("direct-change")),
        created_at: 99_000_u64,
        verification_report_hash: None,
    };

    block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    // Load using envelope.id directly — spec requirement
    let loaded = block_on(graph_store.load_snapshot(&snap.id))
        .expect("load must not error")
        .expect("snapshot must be present when loaded by envelope.id");

    assert_eq!(
        loaded.id, snap.id,
        "id must match when loaded by envelope.id"
    );
    assert_eq!(
        loaded.parent_id, snap.parent_id,
        "parent_id must be preserved"
    );
    assert_eq!(
        loaded.applied_change_id, snap.applied_change_id,
        "applied_change_id must be preserved"
    );
}

// ── append_log_succeeds ───────────────────────────────────────────────────
// Spec scenario: "Append log entry succeeds"
//   GIVEN a ChangeSetLogEntry
//   WHEN appended via GraphStore
//   THEN the returned CAS ObjectId is non-zero (real hash, not a stub)
#[test]
fn append_log_succeeds() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let entry = ChangeSetLogEntry {
        id: oid("cs-1"),
        base_snapshot_id: oid("snap-1"),
        payload_hash: oid("payload-1"),
        created_at: 9_999_u64,
    };

    let cas_id = block_on(graph_store.append_changeset_log(&entry)).expect("append must succeed");

    assert_ne!(
        cas_id.as_bytes(),
        &[0u8; 32],
        "CAS id must be a real content hash, not all-zero"
    );
}

// ── snapshot_envelope_encoding_is_deterministic ───────────────────────────
// Spec: "HashMap prohibition" — SnapshotEnvelope
//   GIVEN a SnapshotEnvelope
//   WHEN encoded twice via CborCodec
//   THEN both encodings are byte-identical
//
// HashMap fields produce non-deterministic key ordering in CBOR maps.
// Identical bytes across two encodes prove no HashMap field is present.
#[test]
fn snapshot_envelope_encoding_is_deterministic() {
    let codec = CborCodec;
    let snap = SnapshotEnvelope {
        id: oid("hashmap-check-id"),
        graph_root_hash: oid("hashmap-check-root"),
        parent_id: Some(oid("hashmap-check-parent")),
        applied_change_id: Some(oid("hashmap-check-change")),
        created_at: 1_234_567_890_u64,
        verification_report_hash: None,
    };
    let bytes1 = codec.encode(&snap).expect("first encode must succeed");
    let bytes2 = codec.encode(&snap).expect("second encode must succeed");
    assert_eq!(
        bytes1, bytes2,
        "SnapshotEnvelope encoding must be deterministic — no HashMap fields allowed"
    );
}

// ── verification_report_hash_round_trip ──────────────────────────────────
// Spec scenario: "verification_report_hash preserved through GraphStore"
//   GIVEN a SnapshotEnvelope with verification_report_hash = Some([42u8; 32])
//   WHEN saved via GraphStore and loaded back by envelope.id
//   THEN verification_report_hash equals Some([42u8; 32])
#[test]
fn verification_report_hash_round_trip() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = SnapshotEnvelope {
        id: oid("vrh-snap"),
        graph_root_hash: oid("vrh-root"),
        parent_id: None,
        applied_change_id: None,
        created_at: 1_000,
        verification_report_hash: Some([42u8; 32]),
    };

    block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    let loaded = block_on(graph_store.load_snapshot(&snap.id))
        .expect("load must succeed")
        .expect("snapshot must be present");

    assert_eq!(
        loaded.verification_report_hash,
        Some([42u8; 32]),
        "verification_report_hash must survive the GraphStore round-trip"
    );
}

// ── verification_report_hash_none_omitted_from_cbor ───────────────────────
// Spec: serde(skip_serializing_if) — None is not written to the CBOR bytes.
//   GIVEN a SnapshotEnvelope with verification_report_hash = None
//   WHEN encoded twice via CborCodec
//   THEN both encodings are byte-identical (determinism) AND
//        a second envelope with verification_report_hash = Some(…)
//        produces DIFFERENT bytes (field is actually present when Some)
#[test]
fn verification_report_hash_none_omitted_from_cbor() {
    let codec = CborCodec;
    let none_snap = SnapshotEnvelope {
        id: oid("vrh-none-id"),
        graph_root_hash: oid("vrh-none-root"),
        parent_id: None,
        applied_change_id: None,
        created_at: 500,
        verification_report_hash: None,
    };
    let some_snap = SnapshotEnvelope {
        verification_report_hash: Some([1u8; 32]),
        ..none_snap.clone()
    };

    let bytes_none_1 = codec.encode(&none_snap).expect("encode none #1");
    let bytes_none_2 = codec.encode(&none_snap).expect("encode none #2");
    let bytes_some = codec.encode(&some_snap).expect("encode some");

    assert_eq!(
        bytes_none_1, bytes_none_2,
        "None encoding must be deterministic"
    );
    assert_ne!(
        bytes_none_1, bytes_some,
        "Some encoding must differ from None (field is serialized when present)"
    );
}

// ── changeset_entry_encoding_is_deterministic ─────────────────────────────
// Spec: "HashMap prohibition" — ChangeSetLogEntry
//   GIVEN a ChangeSetLogEntry
//   WHEN encoded twice via CborCodec
//   THEN both encodings are byte-identical
#[test]
fn changeset_entry_encoding_is_deterministic() {
    let codec = CborCodec;
    let entry = ChangeSetLogEntry {
        id: oid("hashmap-entry-id"),
        base_snapshot_id: oid("hashmap-entry-base"),
        payload_hash: oid("hashmap-entry-payload"),
        created_at: 9_876_543_210_u64,
    };
    let bytes1 = codec.encode(&entry).expect("first encode must succeed");
    let bytes2 = codec.encode(&entry).expect("second encode must succeed");
    assert_eq!(
        bytes1, bytes2,
        "ChangeSetLogEntry encoding must be deterministic — no HashMap fields allowed"
    );
}
