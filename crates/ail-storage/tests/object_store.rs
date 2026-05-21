// Integration tests: ObjectStore contract.
//
// Spec scenarios tested for BOTH MemoryObjectStore and TempfileObjectStore:
//   - "Put-get roundtrip": put a RawObject, get it back — bytes are identical.
//   - "Exists after put": exists() returns true after put.
//   - "Missing object returns None": get/exists on unknown ObjectId return None/false.
//
// The test helpers accept any type implementing ObjectStore, so both backends
// are exercised by the same scenario bodies.

use ail_storage::{
    backends::{memory::MemoryObjectStore, tempfile::TempfileObjectStore},
    object::{ObjectId, ObjectStore, RawObject},
};
use futures::executor::block_on;

// ── shared helpers ────────────────────────────────────────────────────────────

fn sample_raw_object() -> RawObject {
    RawObject(b"hello storage".to_vec())
}

fn unknown_id() -> ObjectId {
    ObjectId::from_bytes(b"definitely not stored anywhere 123")
}

// ── memory backend ────────────────────────────────────────────────────────────

// Spec: "Put-get roundtrip" — MemoryObjectStore
#[test]
fn memory_put_get_roundtrip() {
    block_on(async {
        let store = MemoryObjectStore::new();
        let obj = sample_raw_object();
        let id = store.put(obj.clone()).await.expect("put must succeed");
        let retrieved = store.get(&id).await.expect("get must succeed");
        assert_eq!(
            retrieved,
            Some(obj),
            "retrieved bytes must match what was stored"
        );
    });
}

// Spec: "Exists after put" — MemoryObjectStore
#[test]
fn memory_exists_after_put() {
    block_on(async {
        let store = MemoryObjectStore::new();
        let obj = sample_raw_object();
        let id = store.put(obj).await.expect("put must succeed");
        let exists = store.exists(&id).await.expect("exists must succeed");
        assert!(exists, "exists must return true after a successful put");
    });
}

// Spec: "Missing object returns None" — MemoryObjectStore
#[test]
fn memory_missing_object_returns_none() {
    block_on(async {
        let store = MemoryObjectStore::new();
        let id = unknown_id();
        let retrieved = store.get(&id).await.expect("get on missing must not error");
        assert_eq!(retrieved, None, "get on unknown id must return None");
        let exists = store
            .exists(&id)
            .await
            .expect("exists on missing must not error");
        assert!(!exists, "exists on unknown id must return false");
    });
}

// ── tempfile backend ──────────────────────────────────────────────────────────

// Spec: "Put-get roundtrip" — TempfileObjectStore
#[test]
fn tempfile_put_get_roundtrip() {
    block_on(async {
        let store = TempfileObjectStore::new().expect("tempdir creation must succeed");
        let obj = sample_raw_object();
        let id = store.put(obj.clone()).await.expect("put must succeed");
        let retrieved = store.get(&id).await.expect("get must succeed");
        assert_eq!(
            retrieved,
            Some(obj),
            "retrieved bytes must match what was stored"
        );
    });
}

// Spec: "Exists after put" — TempfileObjectStore
#[test]
fn tempfile_exists_after_put() {
    block_on(async {
        let store = TempfileObjectStore::new().expect("tempdir creation must succeed");
        let obj = sample_raw_object();
        let id = store.put(obj).await.expect("put must succeed");
        let exists = store.exists(&id).await.expect("exists must succeed");
        assert!(exists, "exists must return true after a successful put");
    });
}

// Spec: "Missing object returns None" — TempfileObjectStore
#[test]
fn tempfile_missing_object_returns_none() {
    block_on(async {
        let store = TempfileObjectStore::new().expect("tempdir creation must succeed");
        let id = unknown_id();
        let retrieved = store.get(&id).await.expect("get on missing must not error");
        assert_eq!(retrieved, None, "get on unknown id must return None");
        let exists = store
            .exists(&id)
            .await
            .expect("exists on missing must not error");
        assert!(!exists, "exists on unknown id must return false");
    });
}

// ── Spec: "MemoryObjectStore per-instance isolation" ─────────────────────────
// Two separate MemoryObjectStore instances MUST NOT share state.
// An object stored in instance A must be invisible to instance B.
//
// TRIANGULATE: also proves instance A can still see its own object after the
// isolation check (rules out a bug where put fails silently).
#[test]
fn memory_instance_isolation() {
    block_on(async {
        let store_a = MemoryObjectStore::new();
        let store_b = MemoryObjectStore::new();
        let obj = sample_raw_object();

        let id = store_a.put(obj).await.expect("put in store_a must succeed");

        let exists_in_b = store_b
            .exists(&id)
            .await
            .expect("exists in store_b must not error");
        assert!(
            !exists_in_b,
            "store_b must not see objects stored only in store_a"
        );

        // TRIANGULATE: store_a can still see its own object
        let exists_in_a = store_a
            .exists(&id)
            .await
            .expect("exists in store_a must not error");
        assert!(
            exists_in_a,
            "store_a must still see its own object after isolation check"
        );
    });
}

// ── TRIANGULATE: content-addressed identity ───────────────────────────────────
// Two objects with the same bytes must share the same ObjectId (CAS property).
// Verifies that put returns a deterministic id — not a random one.
#[test]
fn memory_same_bytes_same_id() {
    block_on(async {
        let store = MemoryObjectStore::new();
        let obj = sample_raw_object();
        let id1 = store.put(obj.clone()).await.expect("first put");
        let id2 = store.put(obj).await.expect("second put");
        assert_eq!(
            id1, id2,
            "identical bytes must always map to the same ObjectId"
        );
    });
}
