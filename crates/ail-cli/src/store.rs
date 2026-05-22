// ── ail-cli::store ────────────────────────────────────────────────────────
//
// `StoreHandle` abstracts over the supported storage backends:
//
//   * `Memory` — in-process `ObjectBackedGraphStore<MemoryObjectStore>`.
//     Data is lost when the process exits. Used when no `--database-url`
//     or `AIL_DATABASE_URL` is configured.
//
//   * `File` — local `.ail/` object store backed by content-addressed files.
//     Used when `.ail/` exists in the current working directory.
//
//   * `Postgres` — durable `PostgresGraphStore` backed by a Postgres database.
//     Data persists across invocations. Used when a DB URL is configured.
//
// `build_store` constructs the appropriate variant from the optional URL and
// is the sole entry-point for store creation in the CLI.

use ail_core::semantic_graph::SemanticGraph;
use std::path::{Path, PathBuf};

use ail_storage::{
    GraphStore, ObjectBackedGraphStore, PostgresGraphStore, SnapshotEnvelope,
    backends::memory::MemoryObjectStore,
    codec::{CborCodec, ContentCodec},
    error::StorageError,
    error::StorageResult,
    graph::ChangeSetLogEntry,
    object::{ObjectId, ObjectStore, RawObject},
};

use crate::error::CliError;

// ── StoreHandle ───────────────────────────────────────────────────────────

/// Enum over the supported backing stores.
///
/// Dispatch is via `match` rather than `dyn` to keep concrete types and avoid
/// heap allocation overhead in a short-lived CLI process.
pub enum StoreHandle {
    /// In-memory store — no persistence across invocations.
    Memory {
        graph: ObjectBackedGraphStore<MemoryObjectStore>,
        objects: MemoryObjectStore,
    },
    /// File-backed durable store under `.ail/`.
    File {
        graph: ObjectBackedGraphStore<FileObjectStore>,
        objects: FileObjectStore,
        ail_dir: PathBuf,
    },
    /// Postgres-backed durable store.
    Postgres(PostgresGraphStore),
}

impl StoreHandle {
    /// Save a snapshot envelope; delegates to the active backend.
    pub async fn save_snapshot(&self, env: &SnapshotEnvelope) -> StorageResult<ObjectId> {
        match self {
            StoreHandle::Memory { graph, .. } => graph.save_snapshot(env).await,
            StoreHandle::File { graph, ail_dir, .. } => {
                let id = graph.save_snapshot(env).await?;
                write_ref(&ail_dir.join("HEAD"), &id)?;
                write_ref(&ail_dir.join("refs").join("branches").join("main"), &id)?;
                Ok(id)
            }
            StoreHandle::Postgres(s) => s.save_snapshot(env).await,
        }
    }

    /// Load a snapshot envelope by its id; delegates to the active backend.
    pub async fn load_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
        match self {
            StoreHandle::Memory { graph, .. } => graph.load_snapshot(id).await,
            StoreHandle::File { graph, objects, .. } => match graph.load_snapshot(id).await? {
                Some(snapshot) => Ok(Some(snapshot)),
                None => objects.find_snapshot(id),
            },
            StoreHandle::Postgres(s) => s.load_snapshot(id).await,
        }
    }

    /// Append a changeset log entry; delegates to the active backend.
    pub async fn append_changeset_log(&self, entry: &ChangeSetLogEntry) -> StorageResult<ObjectId> {
        match self {
            StoreHandle::Memory { graph, .. } => graph.append_changeset_log(entry).await,
            StoreHandle::File { graph, .. } => graph.append_changeset_log(entry).await,
            StoreHandle::Postgres(s) => s.append_changeset_log(entry).await,
        }
    }

    /// List all saved snapshot envelopes; delegates to the active backend.
    pub async fn list_snapshots(&self) -> StorageResult<Vec<SnapshotEnvelope>> {
        match self {
            StoreHandle::Memory { graph, .. } => graph.list_snapshots().await,
            StoreHandle::File {
                objects, ail_dir, ..
            } => objects.list_snapshots_from_head(ail_dir),
            StoreHandle::Postgres(s) => s.list_snapshots().await,
        }
    }

    /// Store a semantic graph as a content-addressed object and return its root hash.
    pub async fn save_graph(&self, graph: &SemanticGraph) -> Result<ObjectId, CliError> {
        let mut bytes = Vec::new();
        ciborium::into_writer(graph, &mut bytes)
            .map_err(|e| CliError::Domain(format!("graph encoding failed: {e}")))?;

        match self {
            StoreHandle::Memory { objects, .. } => Ok(objects.put(RawObject(bytes)).await?),
            StoreHandle::File { objects, .. } => Ok(objects.put(RawObject(bytes)).await?),
            StoreHandle::Postgres(_) => Ok(ObjectId::from_bytes(&bytes)),
        }
    }

    /// Load a semantic graph object by its content-addressed root hash.
    pub async fn load_graph(&self, root: &ObjectId) -> Result<Option<SemanticGraph>, CliError> {
        match self {
            StoreHandle::Memory { objects, .. } => {
                let Some(raw) = objects.get(root).await? else {
                    return Ok(None);
                };
                ciborium::from_reader(raw.0.as_slice())
                    .map(Some)
                    .map_err(|e| CliError::Domain(format!("graph decoding failed: {e}")))
            }
            StoreHandle::File { objects, .. } => {
                let Some(raw) = objects.get(root).await? else {
                    return Ok(None);
                };
                ciborium::from_reader(raw.0.as_slice())
                    .map(Some)
                    .map_err(|e| CliError::Domain(format!("graph decoding failed: {e}")))
            }
            StoreHandle::Postgres(_) => Ok(None),
        }
    }
}

// ── FileObjectStore ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct FileObjectStore {
    objects_dir: PathBuf,
}

impl FileObjectStore {
    fn new(ail_dir: &Path) -> Self {
        Self {
            objects_dir: ail_dir.join("store").join("objects"),
        }
    }

    fn object_path(&self, id: &ObjectId) -> PathBuf {
        self.objects_dir.join(id.to_hex())
    }

    fn find_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
        if !self.objects_dir.exists() {
            return Ok(None);
        }

        for entry in std::fs::read_dir(&self.objects_dir)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let Ok(snapshot) = CborCodec.decode::<SnapshotEnvelope>(&bytes) else {
                continue;
            };
            if snapshot.id == *id {
                return Ok(Some(snapshot));
            }
        }
        Ok(None)
    }

    fn list_snapshots_from_head(&self, ail_dir: &Path) -> StorageResult<Vec<SnapshotEnvelope>> {
        let head_path = ail_dir.join("HEAD");
        if !head_path.exists() {
            return Ok(vec![]);
        }

        let mut snapshots = Vec::new();
        let mut next = Some(read_ref(&head_path)?);
        while let Some(id) = next {
            let Some(snapshot) = self.find_snapshot(&id)? else {
                return Err(StorageError::NotFound);
            };
            next = snapshot.parent_id;
            snapshots.push(snapshot);
        }
        snapshots.reverse();
        Ok(snapshots)
    }
}

impl ObjectStore for FileObjectStore {
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        std::fs::create_dir_all(&self.objects_dir)?;
        let id = ObjectId::from_bytes(&object.0);
        let path = self.object_path(&id);
        if !path.exists() {
            std::fs::write(&path, &object.0)?;
        }
        Ok(id)
    }

    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        let path = self.object_path(id);
        if path.exists() {
            Ok(Some(RawObject(std::fs::read(path)?)))
        } else {
            Ok(None)
        }
    }

    async fn exists(&self, id: &ObjectId) -> StorageResult<bool> {
        Ok(self.object_path(id).exists())
    }
}

// ── build_store ───────────────────────────────────────────────────────────

/// Construct the appropriate `StoreHandle` from an optional database URL.
///
/// Resolution order:
/// 1. `db_url` argument (from `--database-url` flag).
/// 2. `AIL_DATABASE_URL` environment variable.
/// 3. Local file store if `.ail/` exists in the current directory.
/// 4. In-memory fallback.
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
    // 3. Local file store when the project has been initialized.
    let ail_dir = std::env::current_dir()?.join(".ail");
    if ail_dir.exists() {
        return Ok(file_handle(ail_dir));
    }
    // 4. In-memory fallback.
    Ok(memory_handle())
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
    memory_handle()
}

fn memory_handle() -> StoreHandle {
    let objects = MemoryObjectStore::new();
    StoreHandle::Memory {
        graph: ObjectBackedGraphStore::new(objects.clone()),
        objects,
    }
}

pub fn file_store(ail_dir: PathBuf) -> StoreHandle {
    file_handle(ail_dir)
}

pub fn init_file_layout(ail_dir: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(ail_dir.join("refs").join("branches"))?;
    std::fs::create_dir_all(ail_dir.join("store").join("objects"))?;
    Ok(())
}

fn file_handle(ail_dir: PathBuf) -> StoreHandle {
    let objects = FileObjectStore::new(&ail_dir);
    StoreHandle::File {
        graph: ObjectBackedGraphStore::new(objects.clone()),
        objects,
        ail_dir,
    }
}

fn write_ref(path: &Path, id: &ObjectId) -> StorageResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", id.to_hex()))?;
    Ok(())
}

fn read_ref(path: &Path) -> StorageResult<ObjectId> {
    hex_to_object_id(std::fs::read_to_string(path)?.trim())
}

fn hex_to_object_id(hex: &str) -> StorageResult<ObjectId> {
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StorageError::Codec(format!("invalid object id: {hex}")));
    }

    let mut bytes = [0u8; 32];
    for (idx, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let s = std::str::from_utf8(chunk)
            .map_err(|e| StorageError::Codec(format!("invalid object id: {e}")))?;
        bytes[idx] = u8::from_str_radix(s, 16)
            .map_err(|e| StorageError::Codec(format!("invalid object id: {e}")))?;
    }
    Ok(ObjectId::from(bytes))
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
            matches!(store, StoreHandle::Memory { .. }),
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

    // Scenario: Semantic graph object roundtrips through memory storage.
    #[tokio::test]
    async fn store_handle_saves_and_loads_graph() {
        let store = memory_store();
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let root = store.save_graph(&graph).await.expect("save graph");
        let loaded = store
            .load_graph(&root)
            .await
            .expect("load graph")
            .expect("graph object must exist");

        assert_eq!(loaded, graph, "loaded graph must match saved graph");
    }
}
