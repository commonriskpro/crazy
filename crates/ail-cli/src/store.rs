// ── ail-cli::store ────────────────────────────────────────────────────────
//
// `StoreHandle` abstracts over the two supported storage backends:
//
//   * `Memory` — in-process `ObjectBackedGraphStore<MemoryObjectStore>`.
//     Data is lost when the process exits. Used when no `--database-url`
//     or `AIL_DATABASE_URL` is configured.
//
//   * `Postgres` — durable `PostgresGraphStore` backed by a Postgres database.
//     Data persists across invocations. Used when a DB URL is configured.
//
// `build_store` constructs the appropriate variant from the optional URL and
// is the sole entry-point for store creation in the CLI.

use ail_storage::{
    GraphStore, ObjectBackedGraphStore, PostgresGraphStore, SnapshotEnvelope,
    backends::memory::MemoryObjectStore, error::StorageResult, graph::ChangeSetLogEntry,
    object::ObjectId,
};

use crate::error::CliError;

// ── StoreHandle ───────────────────────────────────────────────────────────

/// Enum over the two supported backing stores.
///
/// Dispatch is via `match` rather than `dyn` to keep concrete types and avoid
/// heap allocation overhead in a short-lived CLI process.
pub enum StoreHandle {
    /// In-memory store — no persistence across invocations.
    Memory(ObjectBackedGraphStore<MemoryObjectStore>),
    /// Postgres-backed durable store.
    Postgres(PostgresGraphStore),
}

impl StoreHandle {
    /// Save a snapshot envelope; delegates to the active backend.
    pub async fn save_snapshot(&self, env: &SnapshotEnvelope) -> StorageResult<ObjectId> {
        match self {
            StoreHandle::Memory(s) => s.save_snapshot(env).await,
            StoreHandle::Postgres(s) => s.save_snapshot(env).await,
        }
    }

    /// Load a snapshot envelope by its id; delegates to the active backend.
    pub async fn load_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
        match self {
            StoreHandle::Memory(s) => s.load_snapshot(id).await,
            StoreHandle::Postgres(s) => s.load_snapshot(id).await,
        }
    }

    /// Append a changeset log entry; delegates to the active backend.
    pub async fn append_changeset_log(&self, entry: &ChangeSetLogEntry) -> StorageResult<ObjectId> {
        match self {
            StoreHandle::Memory(s) => s.append_changeset_log(entry).await,
            StoreHandle::Postgres(s) => s.append_changeset_log(entry).await,
        }
    }

    /// List all saved snapshot envelopes; delegates to the active backend.
    pub async fn list_snapshots(&self) -> StorageResult<Vec<SnapshotEnvelope>> {
        match self {
            StoreHandle::Memory(s) => s.list_snapshots().await,
            StoreHandle::Postgres(s) => s.list_snapshots().await,
        }
    }
}

// ── build_store ───────────────────────────────────────────────────────────

/// Construct the appropriate `StoreHandle` from an optional database URL.
///
/// Resolution order:
/// 1. `db_url` argument (from `--database-url` flag).
/// 2. `AIL_DATABASE_URL` environment variable.
/// 3. In-memory fallback.
///
/// # Errors
///
/// Returns `Err(CliError::Storage(_))` if a DB URL is provided but the
/// connection fails.
pub async fn build_store(db_url: Option<&str>) -> Result<StoreHandle, CliError> {
    // 1. Explicit flag.
    if let Some(url) = db_url {
        return connect_postgres(url).await;
    }
    // 2. Environment variable.
    if let Ok(url) = std::env::var("AIL_DATABASE_URL") {
        return connect_postgres(&url).await;
    }
    // 3. In-memory fallback.
    Ok(StoreHandle::Memory(ObjectBackedGraphStore::new(
        MemoryObjectStore::new(),
    )))
}

async fn connect_postgres(url: &str) -> Result<StoreHandle, CliError> {
    let store = PostgresGraphStore::connect(url).await?;
    Ok(StoreHandle::Postgres(store))
}

/// Construct a fresh in-memory `StoreHandle` without checking env vars.
///
/// Intended for tests that need a hermetic memory store without touching the
/// environment. Not part of the public production API.
#[cfg(test)]
pub fn memory_store() -> StoreHandle {
    StoreHandle::Memory(ObjectBackedGraphStore::new(MemoryObjectStore::new()))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_storage::object::ObjectId;

    // Scenario: memory_store returns Memory variant without touching env.
    //   GIVEN no external dependencies
    //   WHEN memory_store() is called
    //   THEN StoreHandle::Memory is returned
    #[test]
    fn memory_store_returns_memory_variant() {
        let store = memory_store();
        assert!(
            matches!(store, StoreHandle::Memory(_)),
            "memory_store must produce Memory backend"
        );
    }

    // Scenario: Memory store list_snapshots returns empty initially.
    //   GIVEN a fresh Memory StoreHandle
    //   WHEN list_snapshots is called
    //   THEN empty vec is returned; no error
    #[tokio::test]
    async fn store_handle_memory_list_snapshots_empty() {
        let store = memory_store();
        let list = store.list_snapshots().await.expect("list must succeed");
        assert!(list.is_empty(), "fresh memory store must return empty list");
    }

    // Scenario: Memory store save + list roundtrip.
    //   GIVEN a Memory StoreHandle and a SnapshotEnvelope
    //   WHEN save_snapshot then list_snapshots
    //   THEN the saved envelope is present in the list
    #[tokio::test]
    async fn store_handle_dispatches_list_snapshots() {
        let store = memory_store();

        let id = ObjectId::from_bytes(b"store-handle-test-envelope");
        let root = ObjectId::from_bytes(b"store-handle-test-root");
        let env = SnapshotEnvelope {
            id,
            graph_root_hash: root,
            parent_id: None,
            applied_change_id: None,
            created_at: 42,
            verification_report_hash: None,
        };

        store
            .save_snapshot(&env)
            .await
            .expect("save_snapshot must succeed");

        let list = store.list_snapshots().await.expect("list must succeed");
        assert_eq!(list.len(), 1, "exactly one snapshot must be listed");
        assert_eq!(list[0].id, id, "listed snapshot must match saved id");
    }
}
