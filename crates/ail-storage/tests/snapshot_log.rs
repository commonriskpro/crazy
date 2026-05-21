// Integration tests for SnapshotEnvelope, ChangeSetLogEntry, and GraphStore.
//
// These tests are the acceptance criteria for Phase 4 (PR 3).  They drive
// the design of `graph.rs` and `ObjectBackedGraphStore`.
//
// Test layout
// ──────────────────────────────────────────────────────────────────────────
// snapshot_full_roundtrip   – save + load preserves all fields
// genesis_no_parent         – snapshot with parent=None survives round-trip
// entry_roundtrip           – ChangeSetLogEntry codec round-trip via CborCodec
// save_then_load            – save_snapshot returns a valid ObjectId; load succeeds
// append_log_succeeds       – append_changeset_log returns a real, non-zero ObjectId

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

/// A `SnapshotEnvelope` with a parent, used for round-trip assertions.
fn snapshot_with_parent() -> SnapshotEnvelope {
    SnapshotEnvelope {
        graph_root_hash: oid("root-data"),
        parent: Some(oid("parent-snapshot")),
        created_at: 1_700_000_000_000_u64,
    }
}

// ── snapshot_full_roundtrip ───────────────────────────────────────────────
// Spec scenario: "Full round-trip"
//   GIVEN a SnapshotEnvelope with all fields populated
//   WHEN saved via GraphStore and loaded back by the returned ObjectId
//   THEN all fields are preserved exactly
#[test]
fn snapshot_full_roundtrip() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = snapshot_with_parent();

    let saved_id = block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    let loaded = block_on(graph_store.load_snapshot(&saved_id))
        .expect("load must succeed")
        .expect("snapshot must be present after save");

    assert_eq!(
        loaded.graph_root_hash, snap.graph_root_hash,
        "graph_root_hash must survive the round-trip"
    );
    assert_eq!(
        loaded.parent, snap.parent,
        "parent must survive the round-trip"
    );
    assert_eq!(
        loaded.created_at, snap.created_at,
        "created_at must survive the round-trip"
    );
}

// ── genesis_no_parent ────────────────────────────────────────────────────
// Spec scenario: "Genesis snapshot"
//   GIVEN a SnapshotEnvelope with parent = None
//   WHEN saved and loaded back
//   THEN parent is still None (genesis semantics preserved)
#[test]
fn genesis_no_parent() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = SnapshotEnvelope {
        graph_root_hash: oid("genesis-root"),
        parent: None,
        created_at: 0_u64,
    };

    let saved_id = block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    let loaded = block_on(graph_store.load_snapshot(&saved_id))
        .expect("load must succeed")
        .expect("genesis snapshot must be present");

    assert!(
        loaded.parent.is_none(),
        "genesis snapshot must have no parent"
    );
    assert_eq!(loaded.graph_root_hash, snap.graph_root_hash);
}

// ── entry_roundtrip ───────────────────────────────────────────────────────
// Spec scenario: "ChangeSetLogEntry codec round-trip"
//   GIVEN a ChangeSetLogEntry
//   WHEN encoded with CborCodec and decoded back
//   THEN all fields equal the original (verifies Serde derives)
#[test]
fn entry_roundtrip() {
    let codec = CborCodec;
    let entry = ChangeSetLogEntry {
        id: oid("changeset-42"),
        snapshot_id: oid("snapshot-7"),
        created_at: 1_700_000_000_001_u64,
    };

    let bytes = codec.encode(&entry).expect("encode must succeed");
    let decoded: ChangeSetLogEntry = codec.decode(&bytes).expect("decode must succeed");

    assert_eq!(
        decoded.id, entry.id,
        "ChangeSetLogEntry.id must survive codec round-trip"
    );
    assert_eq!(
        decoded.snapshot_id, entry.snapshot_id,
        "snapshot_id must survive codec round-trip"
    );
    assert_eq!(
        decoded.created_at, entry.created_at,
        "created_at must survive codec round-trip"
    );
}

// ── save_then_load ────────────────────────────────────────────────────────
// Spec scenario: "Save-then-load consistency"
//   GIVEN a SnapshotEnvelope saved to an ObjectBackedGraphStore
//   WHEN the returned ObjectId is used to load
//   THEN the result is Some and created_at matches (TRIANGULATE: different value)
#[test]
fn save_then_load() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let snap = SnapshotEnvelope {
        graph_root_hash: oid("another-root"),
        parent: None,
        created_at: 42_000_u64,
    };

    let id = block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");
    let result = block_on(graph_store.load_snapshot(&id)).expect("load must not error");

    assert!(
        result.is_some(),
        "saved snapshot must be loadable by its id"
    );
    assert_eq!(
        result.unwrap().created_at,
        42_000_u64,
        "created_at 42_000 must be preserved"
    );
}

// ── append_log_succeeds ───────────────────────────────────────────────────
// Spec scenario: "Append changeset log"
//   GIVEN a ChangeSetLogEntry
//   WHEN appended via GraphStore
//   THEN the returned CAS ObjectId is non-zero (real hash, not a stub)
#[test]
fn append_log_succeeds() {
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let entry = ChangeSetLogEntry {
        id: oid("cs-1"),
        snapshot_id: oid("snap-1"),
        created_at: 9_999_u64,
    };

    let cas_id = block_on(graph_store.append_changeset_log(&entry)).expect("append must succeed");

    assert_ne!(
        cas_id.as_bytes(),
        &[0u8; 32],
        "CAS id must be a real content hash, not all-zero"
    );
}
