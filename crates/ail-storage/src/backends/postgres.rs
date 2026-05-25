// Postgres-backed content-addressed object store and graph store.
//
// # Schema
//
// cas_objects: stores all CAS objects (shared by ObjectStore and GraphStore).
//
//   CREATE TABLE IF NOT EXISTS cas_objects (
//       id   BYTEA PRIMARY KEY,   -- 32-byte BLAKE3 hash
//       data BYTEA NOT NULL       -- raw object bytes (schema-agnostic)
//   );
//
// snapshots_index: maps snapshot envelope_id → cas_id for listing.
//
//   CREATE TABLE IF NOT EXISTS snapshots_index (
//       envelope_id BYTEA PRIMARY KEY,
//       cas_id      BYTEA NOT NULL
//   );
//
// report_index: maps change_id → (report_hash, profile) for apply-gate lookup.
//
//   CREATE TABLE IF NOT EXISTS report_index (
//       change_id   TEXT  PRIMARY KEY,  -- canonical change-id hex string
//       report_hash BYTEA NOT NULL,     -- 32-byte BLAKE3 hash of report CBOR
//       profile     TEXT  NOT NULL      -- verification profile (e.g. "dev", "prod")
//   );
//
// All three tables are created on `connect()` so callers need not manage DDL.
//
// # Connection model
//
// `tokio-postgres` separates the `Client` from the `Connection` object that
// drives the socket.  `connect()` spawns the connection future onto the current
// Tokio runtime and returns the store holding only the `Client`.
//
// # Idempotency
//
// `put` uses `ON CONFLICT (id) DO NOTHING` so storing the same bytes twice is
// safe and returns the same `ObjectId` without error.
//
// `save_report` uses `ON CONFLICT (change_id) DO UPDATE` so re-running
// `ail verify` on the same change updates the index to the latest result.

use std::sync::Arc;

use tokio_postgres::NoTls;

use crate::codec::{CborCodec, ContentCodec};
use crate::error::{StorageError, StorageResult};
use crate::graph::{ChangeSetLogEntry, GraphStore, SnapshotEnvelope};
use crate::object::{ObjectId, ObjectStore, RawObject};

/// A production `ObjectStore` backed by a Postgres database.
///
/// Objects are stored in the `cas_objects` table: `id BYTEA PRIMARY KEY` holds
/// the 32-byte BLAKE3 hash; `data BYTEA` holds the raw bytes.  The table is
/// created automatically on [`connect`](PostgresObjectStore::connect).
///
/// The inner [`tokio_postgres::Client`] is wrapped in an [`Arc`] so the store
/// can be cloned and shared across async tasks without ownership transfer.
#[derive(Clone)]
pub struct PostgresObjectStore {
    client: Arc<tokio_postgres::Client>,
}

impl PostgresObjectStore {
    /// Connect to Postgres at `url` and return a ready `PostgresObjectStore`.
    ///
    /// The `cas_objects` table is created if it does not already exist.
    /// The background connection task is spawned onto the current Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::StorageError::Postgres`] if the connection fails
    /// or if the DDL statement cannot be executed.
    pub async fn connect(url: &str) -> StorageResult<Self> {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
        // Spawn the connection driver; its lifetime is independent of the client.
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                // Log to stderr; we have no logger dependency in this crate.
                eprintln!("ail-storage postgres connection error: {e}");
            }
        });

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS cas_objects \
                 (id BYTEA PRIMARY KEY, data BYTEA NOT NULL)",
                &[],
            )
            .await?;

        Ok(Self {
            client: Arc::new(client),
        })
    }
}

impl ObjectStore for PostgresObjectStore {
    /// Store `object` and return its BLAKE3-derived `ObjectId`.
    ///
    /// Idempotent: if the object already exists the existing row is kept and
    /// the same `ObjectId` is returned without error.
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        let id = ObjectId::from_bytes(&object.0);
        self.client
            .execute(
                "INSERT INTO cas_objects (id, data) VALUES ($1, $2) \
                 ON CONFLICT (id) DO NOTHING",
                &[&id.as_bytes().as_slice(), &object.0.as_slice()],
            )
            .await?;
        Ok(id)
    }

    /// Retrieve the object identified by `id`, or `None` if absent.
    ///
    /// Never returns [`crate::error::StorageError::NotFound`]; a missing object
    /// is represented as `None` per the `ObjectStore` contract.
    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        let row = self
            .client
            .query_opt(
                "SELECT data FROM cas_objects WHERE id = $1",
                &[&id.as_bytes().as_slice()],
            )
            .await?;
        Ok(row.map(|r| RawObject(r.get::<_, Vec<u8>>(0))))
    }

    /// Return `true` if an object with the given `id` exists in the store.
    async fn exists(&self, id: &ObjectId) -> StorageResult<bool> {
        let row = self
            .client
            .query_opt(
                "SELECT 1 FROM cas_objects WHERE id = $1",
                &[&id.as_bytes().as_slice()],
            )
            .await?;
        Ok(row.is_some())
    }
}

// ── PostgresGraphStore ────────────────────────────────────────────────────

/// A production `GraphStore` backed by a Postgres database.
///
/// Stores `SnapshotEnvelope`s and `ChangeSetLogEntry`s as CBOR objects in the
/// `cas_objects` table.  A separate `snapshots_index` table maps each
/// `envelope.id` to its CAS object id so that snapshots can be listed across
/// process invocations without an in-memory index.
///
/// The inner `Client` is shared with `PostgresObjectStore` via `Arc` when both
/// are constructed from the same connection; or it may be a dedicated connection.
#[derive(Clone)]
pub struct PostgresGraphStore {
    client: Arc<tokio_postgres::Client>,
    codec: CborCodec,
}

impl PostgresGraphStore {
    /// Connect to Postgres at `url` and return a ready `PostgresGraphStore`.
    ///
    /// Creates `cas_objects` and `snapshots_index` tables if they do not exist.
    /// Spawns the connection driver onto the current Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Postgres`] if the connection fails or if any
    /// DDL statement cannot be executed.
    pub async fn connect(url: &str) -> StorageResult<Self> {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("ail-storage postgres (graph) connection error: {e}");
            }
        });

        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS cas_objects \
                     (id BYTEA PRIMARY KEY, data BYTEA NOT NULL); \
                 CREATE TABLE IF NOT EXISTS snapshots_index \
                     (envelope_id BYTEA PRIMARY KEY, cas_id BYTEA NOT NULL); \
                 CREATE TABLE IF NOT EXISTS report_index \
                     (change_id TEXT PRIMARY KEY, \
                      report_hash BYTEA NOT NULL, \
                      profile TEXT NOT NULL);",
            )
            .await?;

        Ok(Self {
            client: Arc::new(client),
            codec: CborCodec,
        })
    }

    /// Store raw CBOR bytes in `cas_objects` and return the content-addressed id.
    async fn put_raw(&self, bytes: Vec<u8>) -> StorageResult<ObjectId> {
        let id = ObjectId::from_bytes(&bytes);
        self.client
            .execute(
                "INSERT INTO cas_objects (id, data) VALUES ($1, $2) \
                 ON CONFLICT (id) DO NOTHING",
                &[&id.as_bytes().as_slice(), &bytes.as_slice()],
            )
            .await?;
        Ok(id)
    }

    /// Load raw bytes from `cas_objects` by CAS id.
    async fn get_raw(&self, cas_id: &ObjectId) -> StorageResult<Option<Vec<u8>>> {
        let row = self
            .client
            .query_opt(
                "SELECT data FROM cas_objects WHERE id = $1",
                &[&cas_id.as_bytes().as_slice()],
            )
            .await?;
        Ok(row.map(|r| r.get::<_, Vec<u8>>(0)))
    }

    // ── Report index ──────────────────────────────────────────────────────

    /// Store `cbor_bytes` in `cas_objects` and record `change_id → (hash, profile)`
    /// in `report_index`.
    ///
    /// Idempotent per change-id: if an entry already exists for `change_id` the
    /// row is updated to the new hash and profile (covers re-running `ail verify`).
    ///
    /// Returns the BLAKE3 `ObjectId` of the stored CBOR bytes.
    pub async fn save_report(
        &self,
        change_id: &str,
        profile: &str,
        cbor_bytes: Vec<u8>,
    ) -> StorageResult<ObjectId> {
        let hash = self.put_raw(cbor_bytes).await?;
        self.client
            .execute(
                "INSERT INTO report_index (change_id, report_hash, profile) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT (change_id) DO UPDATE \
                     SET report_hash = EXCLUDED.report_hash, \
                         profile     = EXCLUDED.profile",
                &[&change_id, &hash.as_bytes().as_slice(), &profile],
            )
            .await?;
        Ok(hash)
    }

    /// Look up `change_id` in `report_index` and return `(report_hash, profile)`.
    ///
    /// Returns `Ok(None)` when no entry exists (report not yet persisted).
    pub async fn load_report_by_change_id(
        &self,
        change_id: &str,
    ) -> StorageResult<Option<(ObjectId, String)>> {
        let row = self
            .client
            .query_opt(
                "SELECT report_hash, profile FROM report_index WHERE change_id = $1",
                &[&change_id],
            )
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let hash_bytes: Vec<u8> = row.get(0);
        let profile: String = row.get(1);
        if hash_bytes.len() != 32 {
            return Err(StorageError::Codec(
                "invalid report_hash length in report_index".into(),
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash_bytes);
        Ok(Some((ObjectId::from(arr), profile)))
    }

    /// Return the raw CBOR bytes of a report by its BLAKE3 content-addressed hash.
    ///
    /// Returns `Ok(None)` when the object is absent from `cas_objects`.
    pub async fn get_report_bytes(&self, hash: &ObjectId) -> StorageResult<Option<Vec<u8>>> {
        self.get_raw(hash).await
    }
}

impl GraphStore for PostgresGraphStore {
    /// Encode and persist a `SnapshotEnvelope`; return `envelope.id`.
    ///
    /// Also records the mapping in `snapshots_index` for cross-invocation listing.
    async fn save_snapshot(&self, value: &SnapshotEnvelope) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(value)?;
        let cas_id = self.put_raw(bytes).await?;
        // Record envelope_id → cas_id for listing.
        self.client
            .execute(
                "INSERT INTO snapshots_index (envelope_id, cas_id) VALUES ($1, $2) \
                 ON CONFLICT (envelope_id) DO NOTHING",
                &[
                    &value.id.as_bytes().as_slice(),
                    &cas_id.as_bytes().as_slice(),
                ],
            )
            .await?;
        Ok(value.id)
    }

    /// Load and decode the `SnapshotEnvelope` whose `id` field equals `id`.
    async fn load_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
        // Look up CAS id from the index.
        let row = self
            .client
            .query_opt(
                "SELECT cas_id FROM snapshots_index WHERE envelope_id = $1",
                &[&id.as_bytes().as_slice()],
            )
            .await?;
        let cas_id_bytes: Vec<u8> = match row {
            None => return Ok(None),
            Some(r) => r.get::<_, Vec<u8>>(0),
        };
        let mut arr = [0u8; 32];
        if cas_id_bytes.len() != 32 {
            return Err(StorageError::Codec(
                "invalid cas_id length in snapshots_index".into(),
            ));
        }
        arr.copy_from_slice(&cas_id_bytes);
        let cas_id = ObjectId::from(arr);

        match self.get_raw(&cas_id).await? {
            None => Ok(None),
            Some(raw) => {
                let snap = self.codec.decode(&raw)?;
                Ok(Some(snap))
            }
        }
    }

    /// Encode and persist a `ChangeSetLogEntry`; return its CAS `ObjectId`.
    async fn append_changeset_log(&self, entry: &ChangeSetLogEntry) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(entry)?;
        self.put_raw(bytes).await
    }

    /// List all saved `SnapshotEnvelope`s by scanning `snapshots_index`.
    async fn list_snapshots(&self) -> StorageResult<Vec<SnapshotEnvelope>> {
        let rows = self
            .client
            .query("SELECT cas_id FROM snapshots_index", &[])
            .await?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let cas_id_bytes: Vec<u8> = row.get(0);
            if cas_id_bytes.len() != 32 {
                return Err(StorageError::Codec(
                    "invalid cas_id in snapshots_index".into(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&cas_id_bytes);
            let cas_id = ObjectId::from(arr);

            match self.get_raw(&cas_id).await? {
                None => return Err(StorageError::NotFound),
                Some(raw) => {
                    let snap = self.codec.decode(&raw)?;
                    result.push(snap);
                }
            }
        }
        Ok(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Postgres integration tests require a live database — gate with #[ignore].
    // Run with: cargo test -- --include-ignored
    // Requires: AIL_TEST_DB_URL env var pointing to a Postgres instance.

    /// GIVEN a live Postgres instance
    /// WHEN save_report / load_report_by_change_id / get_report_bytes are called
    /// THEN the report index roundtrips correctly
    #[tokio::test]
    #[ignore]
    async fn postgres_report_index_roundtrip() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = PostgresGraphStore::connect(&url)
            .await
            .expect("connect must succeed");

        let change_id = "test-change-report-roundtrip";
        let profile = "dev";
        let cbor_bytes = b"fake-report-cbor".to_vec();

        // Save should succeed and return a hash.
        let hash = store
            .save_report(change_id, profile, cbor_bytes.clone())
            .await
            .expect("save_report must succeed");

        // Load by change_id must return the same hash and profile.
        let result = store
            .load_report_by_change_id(change_id)
            .await
            .expect("load_report_by_change_id must not error")
            .expect("entry must exist after save");
        assert_eq!(result.0, hash, "loaded hash must match saved hash");
        assert_eq!(result.1, profile, "loaded profile must match saved profile");

        // Load bytes by hash must return the original CBOR bytes.
        let loaded_bytes = store
            .get_report_bytes(&hash)
            .await
            .expect("get_report_bytes must not error")
            .expect("bytes must exist after save");
        assert_eq!(
            loaded_bytes, cbor_bytes,
            "loaded bytes must match saved bytes"
        );

        // Re-run with a different profile — ON CONFLICT DO UPDATE must apply.
        let hash2 = store
            .save_report(change_id, "prod", b"updated-cbor".to_vec())
            .await
            .expect("second save_report must succeed");
        let result2 = store
            .load_report_by_change_id(change_id)
            .await
            .expect("second load must not error")
            .expect("entry must exist after update");
        assert_eq!(result2.0, hash2, "updated hash must match second save");
        assert_eq!(result2.1, "prod", "profile must reflect update");
    }

    /// GIVEN a live Postgres instance
    /// WHEN PostgresGraphStore::connect is called
    /// THEN the store is ready and tables exist
    #[tokio::test]
    #[ignore]
    async fn postgres_graph_store_connect_creates_tables() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = PostgresGraphStore::connect(&url)
            .await
            .expect("connect must succeed");

        // Save a snapshot and list it back.
        let dummy_id = ObjectId::from_bytes(b"test-envelope");
        let dummy_root = ObjectId::from_bytes(b"test-root");
        let envelope = SnapshotEnvelope {
            id: dummy_id,
            graph_root_hash: dummy_root,
            parent_id: None,
            applied_change_id: None,
            created_at: 0,
            verification_report_hash: None,
            ..Default::default()
        };
        store
            .save_snapshot(&envelope)
            .await
            .expect("save_snapshot must succeed");

        let loaded = store
            .load_snapshot(&dummy_id)
            .await
            .expect("load_snapshot must succeed");
        assert!(loaded.is_some(), "saved snapshot must be loadable");

        let list = store
            .list_snapshots()
            .await
            .expect("list_snapshots must succeed");
        assert!(
            !list.is_empty(),
            "list_snapshots must return saved snapshot"
        );
    }
}
