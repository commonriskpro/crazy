// Postgres-backed content-addressed object store.
//
// # Schema
//
// A single table holds all CAS objects:
//
//   CREATE TABLE IF NOT EXISTS cas_objects (
//       id   BYTEA PRIMARY KEY,   -- 32-byte BLAKE3 hash
//       data BYTEA NOT NULL       -- raw object bytes (schema-agnostic)
//   );
//
// The table is created on `connect()` so callers need not manage DDL separately.
//
// # Connection model
//
// `tokio-postgres` separates the `Client` from the `Connection` object that
// drives the socket.  `connect()` spawns the connection future onto the current
// Tokio runtime and returns a `PostgresObjectStore` holding only the `Client`.
//
// # Idempotency
//
// `put` uses `ON CONFLICT (id) DO NOTHING` so storing the same bytes twice is
// safe and returns the same `ObjectId` without error.

use std::sync::Arc;

use tokio_postgres::NoTls;

use crate::error::StorageResult;
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
