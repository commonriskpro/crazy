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
//
// File-store implementation details live in `store_file`.
// Doctor / GC report types and logic live in `store_doctor`.

use ail_change::canonical::CanonicalChangeSet;
use ail_core::semantic_graph::SemanticGraph;
use ail_verify::report::VerificationReport;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ail_storage::{
    GraphStore, ObjectBackedGraphStore, PostgresGraphStore, SnapshotEnvelope,
    backends::memory::MemoryObjectStore,
    error::StorageResult,
    graph::ChangeSetLogEntry,
    object::{ObjectId, ObjectStore, RawObject},
};

use crate::error::CliError;
// ── Re-exports (keep `crate::store::X` stable for all callers) ───────────

pub use crate::store_doctor::{doctor, gc};
#[cfg(test)]
pub use crate::store_file::init_file_layout;
pub use crate::store_file::{FileObjectStore, init_file_layout_with_branch};
pub(crate) use crate::store_file::{atomic_write, is_object_file_name};

use crate::store_file::{
    branch_ref_path, current_branch, hex_to_object_id, read_branch_ref, update_snapshot_index,
    validate_branch_name, write_object_ref,
};

// ── StoreHandle ───────────────────────────────────────────────────────────

/// Enum over the supported backing stores.
///
/// Dispatch is via `match` rather than `dyn` to keep concrete types and avoid
/// heap allocation overhead in a short-lived CLI process.
pub enum StoreHandle {
    /// In-memory store — no persistence across invocations.
    ///
    /// `report_index` is an in-process sidecar that maps a change-id hex string
    /// to the `(ObjectId, profile)` pair written by `save_verification_report`.
    /// This lets `load_verification_report_by_change_id` and the apply gate
    /// function identically to the file-backed backend within a single process.
    Memory {
        graph: ObjectBackedGraphStore<MemoryObjectStore>,
        objects: MemoryObjectStore,
        /// Maps `change_id_hex → (report_hash, verified_profile)`.
        report_index: Arc<Mutex<HashMap<String, (ObjectId, String)>>>,
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
                let branch = current_branch(ail_dir)?;
                write_object_ref(&branch_ref_path(ail_dir, &branch)?, &id)?;
                update_snapshot_index(ail_dir, env)?;
                Ok(id)
            }
            StoreHandle::Postgres(s) => s.save_snapshot(env).await,
        }
    }

    /// Save a snapshot to a specific branch when supported by the backend.
    pub async fn save_snapshot_on_branch(
        &self,
        env: &SnapshotEnvelope,
        branch: Option<&str>,
    ) -> StorageResult<ObjectId> {
        match (self, branch) {
            (StoreHandle::File { graph, ail_dir, .. }, Some(branch)) => {
                validate_branch_name(branch)?;
                let id = graph.save_snapshot(env).await?;
                write_object_ref(&branch_ref_path(ail_dir, branch)?, &id)?;
                update_snapshot_index(ail_dir, env)?;
                Ok(id)
            }
            _ => self.save_snapshot(env).await,
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
            } => objects.list_snapshots_from_index(ail_dir),
            StoreHandle::Postgres(s) => s.list_snapshots().await,
        }
    }

    /// Return the selected branch for file-backed stores.
    pub fn current_branch(&self) -> StorageResult<Option<String>> {
        match self {
            StoreHandle::File { ail_dir, .. } => Ok(Some(current_branch(ail_dir)?)),
            _ => Ok(None),
        }
    }

    /// Load the current branch HEAD snapshot when the backend tracks one.
    pub async fn head_snapshot(&self) -> StorageResult<Option<SnapshotEnvelope>> {
        match self {
            StoreHandle::File { ail_dir, .. } => {
                let branch = current_branch(ail_dir)?;
                let Some(id) = read_branch_ref(&branch_ref_path(ail_dir, &branch)?)? else {
                    return Ok(None);
                };
                self.load_snapshot(&id).await
            }
            _ => Ok(None),
        }
    }

    /// Return true when the active backend is a persisted project store.
    pub fn has_persistent_project(&self) -> bool {
        matches!(self, StoreHandle::File { .. } | StoreHandle::Postgres(_))
    }

    /// Return true when the store can resolve a `VerificationReport` by
    /// change-id via an index.
    ///
    /// File-backed stores write a `.ail/reports/<change_id>` sidecar during
    /// `ail verify`.  Memory stores maintain an equivalent in-process
    /// `change_id → (hash, profile)` index in `report_index`.  Both enforce
    /// the verification gate in `ail apply`.
    ///
    /// Postgres stores now also support report lookup via the `report_index`
    /// table (added in Wave 10C).
    pub fn supports_report_lookup_by_change_id(&self) -> bool {
        matches!(
            self,
            StoreHandle::File { .. } | StoreHandle::Memory { .. } | StoreHandle::Postgres(_)
        )
    }

    /// Return the file-backed context index cache path when available.
    pub fn context_index_path(&self) -> Option<PathBuf> {
        match self {
            StoreHandle::File { ail_dir, .. } => {
                Some(ail_dir.join("index").join("context-indexes.cbor"))
            }
            _ => None,
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
            StoreHandle::Postgres(_) => Err(CliError::Domain(
                "save_graph is not supported for the Postgres backend".to_string(),
            )),
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
            StoreHandle::Postgres(_) => Err(CliError::Domain(
                "load_graph is not supported for the Postgres backend".to_string(),
            )),
        }
    }

    /// Store the raw CBOR bytes of a `CanonicalChangeSet` under its change-id.
    ///
    /// The `change_id_hex` MUST be the 64-char hex encoding of `blake3(cbor_bytes)`,
    /// which is how `cmd_change` derives it.  This invariant lets `load_changeset_by_id`
    /// retrieve the bytes by decoding the hex back to the content-addressed key.
    pub async fn save_changeset_payload(
        &self,
        change_id_hex: &str,
        cbor_bytes: &[u8],
    ) -> Result<(), CliError> {
        match self {
            StoreHandle::Memory { objects, .. } => {
                objects.put(RawObject(cbor_bytes.to_vec())).await?;
                Ok(())
            }
            StoreHandle::File { objects, .. } => {
                objects.put(RawObject(cbor_bytes.to_vec())).await?;
                Ok(())
            }
            StoreHandle::Postgres(_) => Err(CliError::Domain(format!(
                "save_changeset_payload({change_id_hex}) is not supported for the Postgres backend"
            ))),
        }
    }

    /// Load the raw CBOR bytes for a `CanonicalChangeSet` by its change-id and decode it.
    ///
    /// Returns `Ok(None)` when the change-id is not found in the store (triggers fallback
    /// in `cmd_verify`).
    ///
    /// # Errors
    ///
    /// Returns `Err` if the change-id hex is malformed (not 64-char lowercase hex),
    /// if CBOR decoding fails (corrupt stored object), or if the active backend is Postgres
    /// (not supported).
    pub async fn load_changeset_by_id(
        &self,
        change_id_hex: &str,
    ) -> Result<Option<CanonicalChangeSet>, CliError> {
        if change_id_hex.len() != 64 || !change_id_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CliError::Domain(format!(
                "invalid change-id: {change_id_hex}"
            )));
        }
        // Decode the 64-char hex string to a 32-byte content-addressed ObjectId.
        let mut bytes = [0u8; 32];
        for (i, chunk) in change_id_hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk)
                .map_err(|_| CliError::Domain("invalid change-id: non-UTF8".to_string()))?;
            bytes[i] = u8::from_str_radix(s, 16)
                .map_err(|_| CliError::Domain(format!("invalid change-id hex: {change_id_hex}")))?;
        }
        let oid = ObjectId::from(bytes);

        let raw = match self {
            StoreHandle::Memory { objects, .. } => objects.get(&oid).await?,
            StoreHandle::File { objects, .. } => objects.get(&oid).await?,
            StoreHandle::Postgres(_) => {
                return Err(CliError::Domain(
                    "load_changeset_by_id is not supported for the Postgres backend".to_string(),
                ));
            }
        };

        let Some(raw) = raw else {
            return Ok(None);
        };

        ciborium::from_reader(raw.0.as_slice())
            .map(Some)
            .map_err(|e| CliError::Domain(format!("changeset decoding failed: {e}")))
    }

    /// Store raw content-addressed object bytes, asserting the expected id.
    pub async fn save_raw_object(
        &self,
        expected_id: &ObjectId,
        bytes: Vec<u8>,
    ) -> Result<(), CliError> {
        let stored_id = match self {
            StoreHandle::Memory { objects, .. } => objects.put(RawObject(bytes)).await?,
            StoreHandle::File { objects, .. } => objects.put(RawObject(bytes)).await?,
            StoreHandle::Postgres(_) => {
                return Err(CliError::Domain(
                    "remote pull cannot write raw objects to the Postgres backend yet".to_string(),
                ));
            }
        };
        if stored_id != *expected_id {
            return Err(CliError::Domain(format!(
                "pulled object id mismatch: expected {expected_id}, stored {stored_id}"
            )));
        }
        Ok(())
    }

    /// Persist a `VerificationReport` as a CBOR-encoded content-addressed object.
    ///
    /// The report is CBOR-encoded with `ciborium` and stored under its BLAKE3 hash.
    /// For file-backed stores a sidecar at `.ail/reports/<change_id>` is also written
    /// so the report can later be resolved by the originating change-id.  The sidecar
    /// format is `<hash> <profile>\n` so that `ail apply` can enforce profile matching.
    ///
    /// Returns the `ObjectId` (BLAKE3 hash of the CBOR bytes).
    ///
    /// All three backends are now supported.  Postgres persists via the
    /// `report_index` table added in Wave 10C.
    pub async fn save_verification_report(
        &self,
        change_id: &str,
        profile: &str,
        report: &VerificationReport,
    ) -> Result<ObjectId, CliError> {
        // Embed the profile into the stored object so hash-addressed lookup
        // (`inspect report <hash>`) can surface the profile without the sidecar.
        // The caller's `report` is not mutated; only the persisted bytes carry the field.
        let mut enriched = report.clone();
        enriched.verified_profile = Some(profile.to_string());
        let mut bytes = Vec::new();
        ciborium::into_writer(&enriched, &mut bytes)
            .map_err(|e| CliError::Domain(format!("report encoding failed: {e}")))?;

        match self {
            StoreHandle::Memory {
                objects,
                report_index,
                ..
            } => {
                let id = objects.put(RawObject(bytes)).await?;
                report_index
                    .lock()
                    .expect("report_index lock must not be poisoned")
                    .insert(change_id.to_string(), (id, profile.to_string()));
                Ok(id)
            }
            StoreHandle::File {
                objects, ail_dir, ..
            } => {
                let id = objects.put(RawObject(bytes)).await?;
                // Sidecar: .ail/reports/<change_id> → "<hash> <profile>\n"
                // The profile field enables per-profile gate enforcement in `ail apply`.
                let sidecar = ail_dir.join("reports").join(change_id);
                atomic_write(
                    &sidecar,
                    format!("{} {}\n", id.to_hex(), profile).as_bytes(),
                )
                .map_err(CliError::Storage)?;
                Ok(id)
            }
            StoreHandle::Postgres(s) => s
                .save_report(change_id, profile, bytes)
                .await
                .map_err(CliError::Storage),
        }
    }

    /// Load a `VerificationReport` by its BLAKE3 content-addressed hash.
    ///
    /// Returns `Ok(None)` when the object is absent from the store.
    /// All three backends are supported; Postgres uses `cas_objects`.
    pub async fn load_verification_report_by_hash(
        &self,
        hash: &ObjectId,
    ) -> Result<Option<VerificationReport>, CliError> {
        let raw = match self {
            StoreHandle::Memory { objects, .. } => objects.get(hash).await?,
            StoreHandle::File { objects, .. } => objects.get(hash).await?,
            StoreHandle::Postgres(s) => s
                .get_report_bytes(hash)
                .await
                .map_err(CliError::Storage)?
                .map(RawObject),
        };
        let Some(raw) = raw else {
            return Ok(None);
        };
        ciborium::from_reader(raw.0.as_slice())
            .map(Some)
            .map_err(|e| CliError::Domain(format!("report decoding failed: {e}")))
    }

    /// Load a `VerificationReport` by the `change_id` of the verify run that produced it.
    ///
    /// File-backed stores resolve the sidecar at `.ail/reports/<change_id>` to obtain
    /// the report hash and profile, then load the object.
    /// Memory stores resolve via the in-process `report_index` sidecar (Wave 9D).
    /// Postgres stores resolve via the `report_index` table (Wave 10C).
    ///
    /// On success returns `Some((report, hash, profile))` where:
    /// - `hash` is the BLAKE3 hash of the CBOR-encoded report.
    /// - `profile` is the verification profile recorded at `ail verify` time, or `"dev"`
    ///   for legacy sidecars that predate profile tracking (migration fallback).
    pub async fn load_verification_report_by_change_id(
        &self,
        change_id: &str,
    ) -> Result<Option<(VerificationReport, ObjectId, String)>, CliError> {
        // Memory backend: resolve via in-process report_index.
        if let StoreHandle::Memory { report_index, .. } = self {
            let entry = report_index
                .lock()
                .expect("report_index lock must not be poisoned")
                .get(change_id)
                .cloned();
            let Some((hash, verified_profile)) = entry else {
                return Ok(None);
            };
            let report = self.load_verification_report_by_hash(&hash).await?;
            return Ok(report.map(|r| (r, hash, verified_profile)));
        }

        // Postgres backend: resolve via the report_index table (Wave 10C).
        if let StoreHandle::Postgres(s) = self {
            let Some((hash, verified_profile)) = s
                .load_report_by_change_id(change_id)
                .await
                .map_err(CliError::Storage)?
            else {
                return Ok(None);
            };
            let report = self.load_verification_report_by_hash(&hash).await?;
            return Ok(report.map(|r| (r, hash, verified_profile)));
        }

        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let sidecar = ail_dir.join("reports").join(change_id);
        if !sidecar.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&sidecar).map_err(CliError::Io)?;
        let line = content.trim();
        // Sidecar format: "<hash> <profile>\n" (current) or "<hash>\n" (legacy).
        // Legacy sidecars (no space) are treated as profile "dev" for migration compat.
        let (hex, verified_profile) = if let Some((h, p)) = line.split_once(' ') {
            (h, p.trim().to_string())
        } else {
            (line, "dev".to_string())
        };
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CliError::Domain(format!(
                "corrupt report sidecar for {change_id}: bad hash '{hex}'"
            )));
        }
        let hash = hex_to_object_id(hex).map_err(CliError::Storage)?;
        let report = self.load_verification_report_by_hash(&hash).await?;
        Ok(report.map(|r| (r, hash, verified_profile)))
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
    if ail_dir.join("HEAD").exists() && ail_dir.join("store").join("objects").exists() {
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
        report_index: Arc::new(Mutex::new(HashMap::new())),
    }
}

pub fn file_store(ail_dir: PathBuf) -> StoreHandle {
    file_handle(ail_dir)
}

fn file_handle(ail_dir: PathBuf) -> StoreHandle {
    let objects = FileObjectStore::new(&ail_dir);
    StoreHandle::File {
        graph: ObjectBackedGraphStore::new(objects.clone()),
        objects,
        ail_dir,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_change::canonical::CanonicalChangeSet;
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
            ..Default::default()
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

    fn test_snapshot(
        id_seed: &[u8],
        root: ObjectId,
        parent_id: Option<ObjectId>,
    ) -> SnapshotEnvelope {
        SnapshotEnvelope {
            id: ObjectId::from_bytes(id_seed),
            graph_root_hash: root,
            parent_id,
            applied_change_id: None,
            created_at: id_seed.len() as u64,
            verification_report_hash: None,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn file_store_writes_objects_atomically_and_verifies_hash_on_load() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = FileObjectStore::new(&ail_dir);

        let id = store
            .put(RawObject(b"atomic-object".to_vec()))
            .await
            .expect("put object");
        let path = store.object_path(&id);

        assert!(path.exists(), "final object file must exist");
        assert!(
            !path.with_extension("tmp").exists(),
            "temporary object file must not remain after rename"
        );
        assert!(
            store.get(&id).await.expect("get object").is_some(),
            "valid object must load"
        );

        std::fs::write(&path, b"corrupted").expect("corrupt object");
        assert!(
            store.get(&id).await.is_err(),
            "hash mismatch must fail on load"
        );
    }

    #[tokio::test]
    async fn file_store_uses_indirect_head_and_named_branch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout_with_branch(&ail_dir, "feature").expect("init layout");
        let store = file_store(ail_dir.clone());
        let root = store
            .save_graph(&SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
            .await
            .expect("save graph");
        let snapshot = test_snapshot(b"feature-snapshot", root, None);

        store.save_snapshot(&snapshot).await.expect("save snapshot");

        assert_eq!(
            std::fs::read_to_string(ail_dir.join("HEAD")).expect("read HEAD"),
            "ref: refs/branches/feature\n"
        );
        assert_eq!(
            std::fs::read_to_string(ail_dir.join("refs").join("branches").join("feature"))
                .expect("read branch ref"),
            format!("{}\n", snapshot.id.to_hex())
        );
    }

    #[tokio::test]
    async fn snapshot_index_is_updated_and_used_for_listing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());
        let root = store
            .save_graph(&SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
            .await
            .expect("save graph");
        let first = test_snapshot(b"first", root, None);
        let second = test_snapshot(b"second-snapshot", root, Some(first.id));

        store.save_snapshot(&second).await.expect("save second");
        store.save_snapshot(&first).await.expect("save first");

        assert!(
            ail_dir.join("index").join("snapshots.cbor").exists(),
            "snapshot index must be written"
        );
        let listed = store.list_snapshots().await.expect("list snapshots");
        assert_eq!(
            listed,
            vec![first, second],
            "index order must be by timestamp"
        );
    }

    #[tokio::test]
    async fn doctor_reports_corrupted_and_unreachable_objects() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());
        let root = store
            .save_graph(&SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
            .await
            .expect("save graph");
        let snapshot = test_snapshot(b"reachable", root, None);
        store.save_snapshot(&snapshot).await.expect("save snapshot");
        let orphan = FileObjectStore::new(&ail_dir)
            .put(RawObject(b"orphan".to_vec()))
            .await
            .expect("put orphan");
        std::fs::write(
            ail_dir.join("store").join("objects").join("0".repeat(64)),
            b"bad",
        )
        .expect("write corrupt object");

        let report = doctor(&ail_dir).expect("doctor");

        assert_eq!(report.corrupted_objects, 1);
        assert!(
            report.unreachable_objects >= 1,
            "orphan object {} must be unreachable",
            orphan
        );
    }

    #[tokio::test]
    async fn gc_deletes_unreachable_objects_and_keeps_branch_tip_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());
        let root = store
            .save_graph(&SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
            .await
            .expect("save graph");
        let snapshot = test_snapshot(b"reachable", root, None);
        store.save_snapshot(&snapshot).await.expect("save snapshot");
        let object_store = FileObjectStore::new(&ail_dir);
        let orphan = object_store
            .put(RawObject(b"delete-me".to_vec()))
            .await
            .expect("put orphan");

        let report = gc(&ail_dir).expect("gc");

        assert!(report.bytes_freed > 0, "gc must free orphan bytes");
        assert!(
            !object_store.object_path(&orphan).exists(),
            "orphan must be deleted"
        );
        assert!(
            object_store.object_path(&root).exists(),
            "reachable graph root must be kept"
        );
    }

    // ── T3: save_changeset_payload + load_changeset_by_id ─────────────────

    /// Build a minimal CanonicalChangeSet and return its CBOR bytes + change_id hex.
    fn minimal_canonical() -> (CanonicalChangeSet, Vec<u8>, String) {
        let canonical = CanonicalChangeSet::default();
        let mut cbor_bytes = Vec::new();
        ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode must succeed");
        // change_id = content-addressed ObjectId expressed as hex
        let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
        (canonical, cbor_bytes, change_id)
    }

    // Scenario: memory store roundtrip — save then load returns same changeset.
    //   GIVEN a memory StoreHandle and a CanonicalChangeSet encoded as CBOR
    //   WHEN save_changeset_payload then load_changeset_by_id with same change_id
    //   THEN Some(canonical) is returned and equals the original
    #[tokio::test]
    async fn save_load_changeset_payload_roundtrip_memory() {
        let store = memory_store();
        let (canonical, cbor_bytes, change_id) = minimal_canonical();

        store
            .save_changeset_payload(&change_id, &cbor_bytes)
            .await
            .expect("save_changeset_payload must succeed");

        let loaded = store
            .load_changeset_by_id(&change_id)
            .await
            .expect("load_changeset_by_id must succeed");

        assert_eq!(
            loaded,
            Some(canonical),
            "loaded changeset must equal the saved canonical"
        );
    }

    // TRIANGULATE: file store roundtrip — save then load returns same changeset.
    //   GIVEN a file StoreHandle backed by a TempDir
    //   WHEN save_changeset_payload then load_changeset_by_id
    //   THEN Some(canonical) is returned and equals the original
    #[tokio::test]
    async fn save_load_changeset_payload_roundtrip_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);
        let (canonical, cbor_bytes, change_id) = minimal_canonical();

        store
            .save_changeset_payload(&change_id, &cbor_bytes)
            .await
            .expect("save_changeset_payload must succeed for file store");

        let loaded = store
            .load_changeset_by_id(&change_id)
            .await
            .expect("load_changeset_by_id must succeed for file store");

        assert_eq!(
            loaded,
            Some(canonical),
            "file store: loaded changeset must equal the saved canonical"
        );
    }

    // TRIANGULATE: unknown change-id returns None (fallback behavior).
    //   GIVEN a memory StoreHandle with no saved changeset
    //   WHEN load_changeset_by_id is called with a valid 64-char hex id
    //   THEN Ok(None) is returned — no error, no panic
    #[tokio::test]
    async fn load_changeset_by_id_unknown_returns_none() {
        let store = memory_store();
        // A valid 64-char hex id that was never stored.
        let unknown_id = "b".repeat(64);

        let result = store
            .load_changeset_by_id(&unknown_id)
            .await
            .expect("load_changeset_by_id must not error for unknown id");

        assert_eq!(
            result, None,
            "unknown change-id must return None (fallback)"
        );
    }

    // ── Postgres backend: explicit unsupported errors ──────────────────────
    //
    // These tests require a live Postgres instance and are gated with #[ignore].
    // Run with: cargo test -p ail-cli -- --include-ignored
    // Requires: AIL_TEST_DB_URL env var pointing to a Postgres instance.
    //
    // NOTE: Constructing `StoreHandle::Postgres` requires `PostgresGraphStore::connect()`
    // which performs a real TCP handshake and schema setup.  Live-DB-free unit tests
    // for these arms are not feasible without a significant trait-object refactor of
    // `StoreHandle`.  The integration tests below verify the explicit error contract.

    // Scenario: Postgres backend save_graph returns an explicit unsupported error.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN save_graph is called
    //   THEN Err(CliError::Domain(_)) is returned containing "not supported for the Postgres backend"
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_save_graph_returns_unsupported_error() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let err = store
            .save_graph(&graph)
            .await
            .expect_err("save_graph must fail for Postgres");

        let msg = format!("{err}");
        assert!(
            matches!(err, CliError::Domain(_)),
            "must be CliError::Domain; got: {msg}"
        );
        assert!(
            msg.contains("not supported for the Postgres backend"),
            "error must mention unsupported backend; got: {msg}"
        );
    }

    // Scenario: Postgres backend load_graph returns an explicit unsupported error.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN load_graph is called
    //   THEN Err(CliError::Domain(_)) is returned containing "not supported for the Postgres backend"
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_load_graph_returns_unsupported_error() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        let dummy_root = ObjectId::from_bytes(b"postgres-load-graph-test");

        let err = store
            .load_graph(&dummy_root)
            .await
            .expect_err("load_graph must fail for Postgres");

        let msg = format!("{err}");
        assert!(
            matches!(err, CliError::Domain(_)),
            "must be CliError::Domain; got: {msg}"
        );
        assert!(
            msg.contains("not supported for the Postgres backend"),
            "error must mention unsupported backend; got: {msg}"
        );
    }

    // Scenario: Postgres backend save_changeset_payload returns an explicit unsupported error.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN save_changeset_payload is called
    //   THEN Err(CliError::Domain(_)) is returned containing "not supported for the Postgres backend"
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_save_changeset_payload_returns_unsupported_error() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        let (_, cbor_bytes, change_id) = minimal_canonical();

        let err = store
            .save_changeset_payload(&change_id, &cbor_bytes)
            .await
            .expect_err("save_changeset_payload must fail for Postgres");

        let msg = format!("{err}");
        assert!(
            matches!(err, CliError::Domain(_)),
            "must be CliError::Domain; got: {msg}"
        );
        assert!(
            msg.contains("not supported for the Postgres backend"),
            "error must mention unsupported backend; got: {msg}"
        );
    }

    // Scenario: Postgres backend load_changeset_by_id returns an explicit unsupported error.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN load_changeset_by_id is called
    //   THEN Err(CliError::Domain(_)) is returned containing "not supported for the Postgres backend"
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_load_changeset_by_id_returns_unsupported_error() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        let unknown_id = "a".repeat(64);

        let err = store
            .load_changeset_by_id(&unknown_id)
            .await
            .expect_err("load_changeset_by_id must fail for Postgres");

        let msg = format!("{err}");
        assert!(
            matches!(err, CliError::Domain(_)),
            "must be CliError::Domain; got: {msg}"
        );
        assert!(
            msg.contains("not supported for the Postgres backend"),
            "error must mention unsupported backend; got: {msg}"
        );
    }

    // ── T4: save/load verification report ────────────────────────────────

    fn minimal_report() -> VerificationReport {
        VerificationReport::default()
    }

    // Scenario: memory store save + load by hash roundtrip.
    //   GIVEN a memory StoreHandle and a VerificationReport
    //   WHEN save_verification_report then load_verification_report_by_hash
    //   THEN the loaded report equals the original
    #[tokio::test]
    async fn save_load_verification_report_by_hash_memory() {
        let store = memory_store();
        let change_id = "c".repeat(64);
        let report = minimal_report();

        let hash = store
            .save_verification_report(&change_id, "dev", &report)
            .await
            .expect("save_verification_report must succeed for memory store");

        let loaded = store
            .load_verification_report_by_hash(&hash)
            .await
            .expect("load_verification_report_by_hash must succeed");

        // save_verification_report embeds the profile in the stored CBOR, so the
        // loaded report has verified_profile set even though the original did not.
        let expected = VerificationReport {
            verified_profile: Some("dev".to_string()),
            ..report.clone()
        };
        assert_eq!(
            loaded,
            Some(expected),
            "loaded report must match the enriched (profile-embedded) saved report"
        );
    }

    // TRIANGULATE: file store roundtrip — save + load by hash + sidecar.
    //   GIVEN a file StoreHandle backed by a TempDir
    //   WHEN save_verification_report then load by hash AND by change_id
    //   THEN both load paths return the same report
    #[tokio::test]
    async fn save_load_verification_report_file_store_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        // Create the reports subdirectory (normally created by `ail init`).
        std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");
        let store = file_store(ail_dir.clone());
        let change_id = "d".repeat(64);
        let report = minimal_report();

        let hash = store
            .save_verification_report(&change_id, "dev", &report)
            .await
            .expect("save_verification_report must succeed for file store");

        // save_verification_report embeds the profile in the stored CBOR.
        let expected = VerificationReport {
            verified_profile: Some("dev".to_string()),
            ..report.clone()
        };

        // Load by hash.
        let by_hash = store
            .load_verification_report_by_hash(&hash)
            .await
            .expect("load_verification_report_by_hash must succeed")
            .expect("report must be present when loaded by hash");
        assert_eq!(
            by_hash, expected,
            "hash-loaded report must match enriched report (profile embedded)"
        );

        // Load by change_id via sidecar.
        let by_change_id = store
            .load_verification_report_by_change_id(&change_id)
            .await
            .expect("load_verification_report_by_change_id must succeed")
            .expect("report must be present when loaded by change_id");
        assert_eq!(
            by_change_id.0, expected,
            "change-id-loaded report must match enriched report (profile embedded)"
        );
        assert_eq!(
            by_change_id.1, hash,
            "change-id-loaded hash must match the stored hash"
        );
        assert_eq!(
            by_change_id.2, "dev",
            "change-id-loaded profile must match the saved profile"
        );

        // Sidecar file exists at the expected path.
        assert!(
            ail_dir.join("reports").join(&change_id).exists(),
            "sidecar file must exist at .ail/reports/<change_id>"
        );
    }

    // TRIANGULATE: memory store load by change_id resolves via in-process index.
    //   GIVEN a memory StoreHandle with a saved report for "dev"
    //   WHEN load_verification_report_by_change_id is called with the same change_id
    //   THEN Some((report, hash, "dev")) is returned — index enforces the gate
    #[tokio::test]
    async fn load_verification_report_by_change_id_memory_uses_index() {
        let store = memory_store();
        let change_id = "e".repeat(64);
        let report = minimal_report();

        let hash = store
            .save_verification_report(&change_id, "dev", &report)
            .await
            .expect("save must succeed");

        let result = store
            .load_verification_report_by_change_id(&change_id)
            .await
            .expect("must not error")
            .expect("memory store must resolve by change_id via in-process index");

        let expected = VerificationReport {
            verified_profile: Some("dev".to_string()),
            ..report.clone()
        };
        assert_eq!(
            result.0, expected,
            "resolved report must match enriched (profile-embedded) saved report"
        );
        assert_eq!(result.1, hash, "resolved hash must match the stored hash");
        assert_eq!(
            result.2, "dev",
            "resolved profile must match the saved profile"
        );
    }

    // Scenario: memory store load by change_id returns None for unknown change_id.
    //   GIVEN a memory StoreHandle with no saved reports
    //   WHEN load_verification_report_by_change_id is called
    //   THEN Ok(None) is returned
    #[tokio::test]
    async fn load_verification_report_by_change_id_memory_unknown_returns_none() {
        let store = memory_store();
        let unknown = "f".repeat(64);

        let result = store
            .load_verification_report_by_change_id(&unknown)
            .await
            .expect("must not error");

        assert_eq!(result, None, "unknown change-id must return None");
    }

    // Scenario: memory store supports_report_lookup_by_change_id returns true.
    //   GIVEN a memory StoreHandle
    //   WHEN supports_report_lookup_by_change_id is called
    //   THEN true is returned — gate can be enforced
    #[test]
    fn memory_supports_report_lookup_by_change_id() {
        let store = memory_store();
        assert!(
            store.supports_report_lookup_by_change_id(),
            "memory store must report gate support after Wave 9D"
        );
    }

    // Scenario: memory index enforces profile matching across two saves.
    //   GIVEN a memory StoreHandle with reports saved for two different profiles
    //   WHEN each is loaded by its change_id
    //   THEN each returns the correct profile — gate can enforce profile mismatch
    #[tokio::test]
    async fn memory_report_index_records_profile_per_change_id() {
        let store = memory_store();
        let change_dev = "1".repeat(64);
        let change_prod = "2".repeat(64);
        let report = minimal_report();

        store
            .save_verification_report(&change_dev, "dev", &report)
            .await
            .expect("save dev must succeed");
        store
            .save_verification_report(&change_prod, "prod", &report)
            .await
            .expect("save prod must succeed");

        let dev_result = store
            .load_verification_report_by_change_id(&change_dev)
            .await
            .expect("must not error")
            .expect("dev report must resolve");
        let prod_result = store
            .load_verification_report_by_change_id(&change_prod)
            .await
            .expect("must not error")
            .expect("prod report must resolve");

        assert_eq!(dev_result.2, "dev", "dev change_id must carry dev profile");
        assert_eq!(
            prod_result.2, "prod",
            "prod change_id must carry prod profile"
        );
    }

    // TRIANGULATE: load by hash on unknown returns None.
    //   GIVEN a memory store with no reports
    //   WHEN load_verification_report_by_hash is called with an unknown hash
    //   THEN Ok(None) is returned
    #[tokio::test]
    async fn load_verification_report_by_hash_unknown_returns_none() {
        let store = memory_store();
        let unknown = ObjectId::from([0xffu8; 32]);

        let result = store
            .load_verification_report_by_hash(&unknown)
            .await
            .expect("must not error");

        assert_eq!(result, None, "unknown hash must return None");
    }

    // ── T4b: Wave 8A — verified_profile embedded in hash-addressed object ────

    // Scenario: save embeds profile in the CBOR object; hash-load reflects it.
    //   GIVEN a memory StoreHandle and a VerificationReport without verified_profile
    //   WHEN save_verification_report("dev") is called
    //   THEN load_by_hash returns a report with verified_profile = Some("dev")
    #[tokio::test]
    async fn save_verification_report_embeds_profile_in_object() {
        let store = memory_store();
        let change_id = "f".repeat(64);
        let report = minimal_report(); // verified_profile: None

        let hash = store
            .save_verification_report(&change_id, "dev", &report)
            .await
            .expect("save must succeed");

        let loaded = store
            .load_verification_report_by_hash(&hash)
            .await
            .expect("load must succeed")
            .expect("report must exist");

        assert_eq!(
            loaded.verified_profile,
            Some("dev".to_string()),
            "loaded report must carry the profile that was passed to save"
        );
        // The caller's original report is NOT mutated.
        assert_eq!(
            report.verified_profile, None,
            "save must not mutate the caller's report"
        );
    }

    // Scenario: different profiles are embedded distinctly.
    //   GIVEN two save calls with profiles "dev" and "prod"
    //   WHEN each report is loaded by its distinct hash
    //   THEN each carries the correct profile
    #[tokio::test]
    async fn save_verification_report_distinct_profiles_stored_distinctly() {
        let store = memory_store();
        let change_dev = "a".repeat(64);
        let change_prod = "b".repeat(64);
        let report = minimal_report();

        let hash_dev = store
            .save_verification_report(&change_dev, "dev", &report)
            .await
            .expect("save dev must succeed");
        let hash_prod = store
            .save_verification_report(&change_prod, "prod", &report)
            .await
            .expect("save prod must succeed");

        // Different profiles produce different CBOR → different hashes.
        assert_ne!(hash_dev, hash_prod, "dev and prod hashes must differ");

        let loaded_dev = store
            .load_verification_report_by_hash(&hash_dev)
            .await
            .expect("load dev must succeed")
            .expect("dev report must exist");
        let loaded_prod = store
            .load_verification_report_by_hash(&hash_prod)
            .await
            .expect("load prod must succeed")
            .expect("prod report must exist");

        assert_eq!(loaded_dev.verified_profile, Some("dev".to_string()));
        assert_eq!(loaded_prod.verified_profile, Some("prod".to_string()));
    }

    // Scenario: old CBOR without verified_profile field still deserializes.
    //   GIVEN CBOR bytes of a VerificationReport without the verified_profile field
    //   WHEN deserialized into VerificationReport
    //   THEN verified_profile defaults to None (backward compat)
    #[test]
    fn old_report_cbor_without_verified_profile_defaults_to_none() {
        // Encode a report that has verified_profile = None (as if from Wave 7).
        let old_report = VerificationReport {
            verified_profile: None,
            ..Default::default()
        };
        let mut buf = Vec::new();
        ciborium::into_writer(&old_report, &mut buf).expect("CBOR encode must succeed");

        // Decode — must succeed with verified_profile = None.
        let decoded: VerificationReport =
            ciborium::from_reader(buf.as_slice()).expect("CBOR decode must succeed");
        assert_eq!(
            decoded.verified_profile, None,
            "old reports without verified_profile must deserialize to None"
        );
    }

    // (T4 WASM artifact and T5 native artifact tests moved to store_artifacts.rs)

    // ── Postgres report index: Wave 10C integration tests ─────────────────
    //
    // These tests require a live Postgres instance and are gated with #[ignore].
    // Run with: cargo test -p ail-cli -- --include-ignored
    // Requires: AIL_TEST_DB_URL env var pointing to a Postgres instance.

    // Scenario: Postgres supports_report_lookup_by_change_id returns true.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN supports_report_lookup_by_change_id is called
    //   THEN true is returned — gate is enforced for all backends
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_supports_report_lookup_by_change_id() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");

        assert!(
            store.supports_report_lookup_by_change_id(),
            "Postgres backend must support report lookup after Wave 10C"
        );
    }

    // Scenario: Postgres save + load by change_id roundtrip.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN save_verification_report then load_verification_report_by_change_id
    //   THEN Some((report, hash, profile)) is returned with correct values
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_save_load_verification_report_by_change_id() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        let change_id = "0".repeat(64);
        let report = minimal_report();

        let hash = store
            .save_verification_report(&change_id, "dev", &report)
            .await
            .expect("save_verification_report must succeed for Postgres");

        let result = store
            .load_verification_report_by_change_id(&change_id)
            .await
            .expect("load must not error")
            .expect("report must resolve via report_index table");

        let expected = VerificationReport {
            verified_profile: Some("dev".to_string()),
            ..report
        };
        assert_eq!(
            result.0, expected,
            "loaded report must match the enriched saved report"
        );
        assert_eq!(result.1, hash, "loaded hash must match the stored hash");
        assert_eq!(result.2, "dev", "loaded profile must match saved profile");
    }

    // Scenario: Postgres load by hash roundtrip.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN save_verification_report then load_verification_report_by_hash
    //   THEN Some(report) is returned with profile embedded
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_save_load_verification_report_by_hash() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        let change_id = "1".repeat(64);
        let report = minimal_report();

        let hash = store
            .save_verification_report(&change_id, "prod", &report)
            .await
            .expect("save_verification_report must succeed for Postgres");

        let loaded = store
            .load_verification_report_by_hash(&hash)
            .await
            .expect("load_verification_report_by_hash must succeed")
            .expect("report bytes must be in cas_objects after save");

        assert_eq!(
            loaded.verified_profile,
            Some("prod".to_string()),
            "report loaded by hash must carry the embedded profile"
        );
    }

    // Scenario: Postgres load by change_id returns None when not saved.
    //   GIVEN a Postgres StoreHandle connected to a live DB
    //   WHEN load_verification_report_by_change_id is called with unknown change_id
    //   THEN Ok(None) is returned
    #[tokio::test]
    #[ignore = "requires AIL_TEST_DB_URL pointing to a live Postgres instance"]
    async fn postgres_load_verification_report_unknown_change_id_returns_none() {
        let url = std::env::var("AIL_TEST_DB_URL")
            .expect("AIL_TEST_DB_URL must be set for Postgres integration tests");
        let store = connect_postgres(&url).await.expect("connect must succeed");
        // A change_id that was never stored.
        let unknown = format!("unknown-change-{}", "z".repeat(48));

        let result = store
            .load_verification_report_by_change_id(&unknown)
            .await
            .expect("must not error for unknown change_id");

        assert_eq!(
            result, None,
            "unknown change_id must return None for Postgres"
        );
    }
}
