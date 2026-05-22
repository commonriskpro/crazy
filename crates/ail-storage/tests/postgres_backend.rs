// Integration tests: PostgresObjectStore + GraphStore adapter.
//
// All tests that require a live Postgres instance are annotated with `#[ignore]`
// so they are skipped by default (`cargo test`).  Run them with:
//
//   DATABASE_URL=postgres://... cargo test -p ail-storage -- --ignored
//
// The one exception is `postgres_connect_error_on_bad_url`, which only needs
// the tokio runtime and exercises the error path — no DB required.
//
// Spec scenarios covered
// ──────────────────────────────────────────────────────────────────────────
//   S1  put-get roundtrip
//   S2  idempotent put
//   S3  exists after put
//   S4  missing object returns None / false
//   S5  content-addressed identity (same bytes → same ObjectId)
//   S6  GraphStore via ObjectBackedGraphStore<PostgresObjectStore>
//   S7  connect error on bad URL (no DB required)

use ail_storage::{
    backends::postgres::PostgresObjectStore,
    graph::{ChangeSetLogEntry, GraphStore, ObjectBackedGraphStore, SnapshotEnvelope},
    object::{ObjectId, ObjectStore, RawObject},
};

// ── helpers ───────────────────────────────────────────────────────────────

/// Read DATABASE_URL from the environment, returning None if absent.
fn db_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// Skip the test gracefully when DATABASE_URL is not set.
macro_rules! require_db {
    () => {
        match db_url() {
            Some(url) => url,
            None => {
                eprintln!("DATABASE_URL not set — skipping integration test");
                return;
            }
        }
    };
}

/// Connect to a fresh `PostgresObjectStore` for a single test.
async fn connect(url: &str) -> PostgresObjectStore {
    PostgresObjectStore::connect(url)
        .await
        .expect("PostgresObjectStore::connect must succeed with a valid DATABASE_URL")
}

/// A deterministic `ObjectId` from a short label.
fn oid(label: &str) -> ObjectId {
    ObjectId::from_bytes(label.as_bytes())
}

// ── S1: put-get roundtrip ────────────────────────────────────────────────

// Spec S1: "Put-get roundtrip"
//   GIVEN a connected PostgresObjectStore
//   WHEN put(RawObject) then get(id)
//   THEN returned bytes equal the original
#[tokio::test]
#[ignore]
async fn postgres_put_get_roundtrip() {
    let url = require_db!();
    let store = connect(&url).await;

    let obj = RawObject(b"hello postgres".to_vec());
    let id = store.put(obj.clone()).await.expect("put must succeed");
    let retrieved = store.get(&id).await.expect("get must succeed");

    assert_eq!(
        retrieved,
        Some(obj),
        "retrieved bytes must match what was stored"
    );
}

// ── S2: idempotent put ───────────────────────────────────────────────────

// Spec S2: "Idempotent put"
//   GIVEN a connected store with object X already stored
//   WHEN put(X) is called again
//   THEN same ObjectId returned, no error
#[tokio::test]
#[ignore]
async fn postgres_idempotent_put() {
    let url = require_db!();
    let store = connect(&url).await;

    let obj = RawObject(b"idempotent test bytes".to_vec());
    let id1 = store
        .put(obj.clone())
        .await
        .expect("first put must succeed");
    let id2 = store.put(obj).await.expect("second put must succeed");

    assert_eq!(id1, id2, "idempotent put must return the same ObjectId");
}

// ── S3: exists after put ─────────────────────────────────────────────────

// Spec S3: "Exists after put"
//   GIVEN a connected store
//   WHEN put then exists(id)
//   THEN true
#[tokio::test]
#[ignore]
async fn postgres_exists_after_put() {
    let url = require_db!();
    let store = connect(&url).await;

    let obj = RawObject(b"existence check".to_vec());
    let id = store.put(obj).await.expect("put must succeed");
    let exists = store.exists(&id).await.expect("exists must succeed");

    assert!(exists, "exists must return true after a successful put");
}

// ── S4: missing object returns None / false ───────────────────────────────

// Spec S4: "Missing object returns None"
//   GIVEN an unknown ObjectId
//   WHEN get or exists
//   THEN None / false
#[tokio::test]
#[ignore]
async fn postgres_missing_object_returns_none() {
    let url = require_db!();
    let store = connect(&url).await;

    let id = ObjectId::from_bytes(b"object that was never stored in postgres 42");
    let retrieved = store.get(&id).await.expect("get on missing must not error");
    assert_eq!(retrieved, None, "get on unknown id must return None");

    let exists = store
        .exists(&id)
        .await
        .expect("exists on missing must not error");
    assert!(!exists, "exists on unknown id must return false");
}

// ── S5: content-addressed identity ───────────────────────────────────────

// Spec S5: "Content-addressed identity"
//   GIVEN same bytes stored twice
//   THEN same ObjectId returned both times
#[tokio::test]
#[ignore]
async fn postgres_same_bytes_same_id() {
    let url = require_db!();
    let store = connect(&url).await;

    let obj = RawObject(b"cas identity bytes".to_vec());
    let id1 = store.put(obj.clone()).await.expect("first put");
    let id2 = store.put(obj).await.expect("second put");

    assert_eq!(
        id1, id2,
        "identical bytes must always map to the same ObjectId"
    );
}

// ── S6: GraphStore via adapter ────────────────────────────────────────────

// Spec S6: "GraphStore via ObjectBackedGraphStore<PostgresObjectStore>"
//   GIVEN ObjectBackedGraphStore::new(PostgresObjectStore::connect(url))
//   WHEN save_snapshot + load_snapshot
//   THEN roundtrip preserves all five spec fields
#[tokio::test]
#[ignore]
async fn postgres_graph_store_roundtrip() {
    let url = require_db!();
    let store = connect(&url).await;
    let graph = ObjectBackedGraphStore::new(store);

    let snap = SnapshotEnvelope {
        id: oid("postgres-graph-snap"),
        graph_root_hash: oid("postgres-graph-root"),
        parent_id: Some(oid("postgres-graph-parent")),
        applied_change_id: Some(oid("postgres-graph-change")),
        created_at: 1_700_000_000_000_u64,
        verification_report_hash: None,
    };

    let returned_id = graph.save_snapshot(&snap).await.expect("save must succeed");
    assert_eq!(
        returned_id, snap.id,
        "save_snapshot must return envelope.id"
    );

    let loaded = graph
        .load_snapshot(&snap.id)
        .await
        .expect("load must succeed")
        .expect("snapshot must be present after save");

    assert_eq!(loaded.id, snap.id);
    assert_eq!(loaded.graph_root_hash, snap.graph_root_hash);
    assert_eq!(loaded.parent_id, snap.parent_id);
    assert_eq!(loaded.applied_change_id, snap.applied_change_id);
    assert_eq!(loaded.created_at, snap.created_at);

    // Also verify append_changeset_log
    let entry = ChangeSetLogEntry {
        id: oid("postgres-cs-1"),
        base_snapshot_id: snap.id,
        payload_hash: oid("postgres-payload-1"),
        created_at: 9_000_u64,
    };
    let cas_id = graph
        .append_changeset_log(&entry)
        .await
        .expect("append must succeed");
    assert_ne!(
        cas_id.as_bytes(),
        &[0u8; 32],
        "CAS id must be a real hash, not all-zero"
    );
}

// ── S7: connect error on bad URL (no DB required) ─────────────────────────

// Spec S7: "Error on bad URL"
//   GIVEN an invalid postgres URL
//   WHEN connect is called
//   THEN StorageResult::Err(StorageError::Postgres(_))
//
// This test does NOT require a real database; it exercises the error path only.
// No #[ignore] annotation — runs unconditionally.
#[tokio::test]
async fn postgres_connect_error_on_bad_url() {
    let result = PostgresObjectStore::connect("postgres://invalid-host:5432/no_such_db").await;
    assert!(
        result.is_err(),
        "connect must return Err on an unreachable/invalid URL"
    );
}
