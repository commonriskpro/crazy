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

use ail_change::canonical::CanonicalChangeSet;
use ail_core::semantic_graph::SemanticGraph;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ail_storage::{
    GraphStore, ObjectBackedGraphStore, PostgresGraphStore, SnapshotEnvelope,
    backends::memory::MemoryObjectStore,
    codec::{CborCodec, ContentCodec},
    error::StorageError,
    error::StorageResult,
    graph::ChangeSetLogEntry,
    object::{ObjectId, ObjectStore, RawObject},
};
use serde::{Deserialize, Serialize};

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

    /// Store the raw CBOR bytes of a `CanonicalChangeSet` under its change-id.
    ///
    /// The `change_id_hex` MUST be the 64-char hex encoding of `blake3(cbor_bytes)`,
    /// which is how `cmd_change` derives it.  This invariant lets `load_changeset_by_id`
    /// retrieve the bytes by decoding the hex back to the content-addressed key.
    ///
    /// Postgres: no-op (changeset retrieval not supported by that backend).
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
            // Postgres: changeset payload storage not supported; return Ok silently.
            StoreHandle::Postgres(_) => {
                let _ = change_id_hex;
                Ok(())
            }
        }
    }

    /// Load the raw CBOR bytes for a `CanonicalChangeSet` by its change-id and decode it.
    ///
    /// Returns `Ok(None)` when the change-id is not found in the store (triggers fallback
    /// in `cmd_verify`).  Returns `Ok(None)` for Postgres (not supported).
    ///
    /// # Errors
    ///
    /// Returns `Err` if the change-id hex is malformed (not 64-char lowercase hex) or
    /// if CBOR decoding fails (corrupt stored object).
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
            StoreHandle::Postgres(_) => return Ok(None),
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

    /// Persist a compiled WASM artifact and its sidecars under `.ail/wasm/`.
    ///
    /// Writes four files keyed by `hash`:
    ///   - `<hash>.wasm`              — raw WASM bytes
    ///   - `<hash>.source_map.json`   — source map JSON
    ///   - `<hash>.manifest.json`     — artifact manifest JSON
    ///   - `<hash>.capabilities.json` — capabilities manifest JSON
    ///
    /// Also updates `.ail/wasm/artifact-index.json` with the new entry so
    /// `load_wasm_artifact` can find the artifact by name or hash later.
    ///
    /// Returns `Ok(Some(paths))` for file-backed stores.
    /// Returns `Ok(None)` for in-memory and Postgres backends (no-op).
    pub fn save_wasm_artifact(
        &self,
        hash: &str,
        profile: &str,
        target: &str,
        bytes: WasmArtifactBytes<'_>,
    ) -> Result<Option<WasmArtifactPaths>, CliError> {
        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let dir = wasm_dir(ail_dir);
        std::fs::create_dir_all(&dir)?;

        let wasm_path = dir.join(format!("{hash}.wasm"));
        let source_map_path = dir.join(format!("{hash}.source_map.json"));
        let manifest_path = dir.join(format!("{hash}.manifest.json"));
        let capabilities_path = dir.join(format!("{hash}.capabilities.json"));

        atomic_write(&wasm_path, bytes.wasm)?;
        atomic_write(&source_map_path, bytes.source_map_json)?;
        atomic_write(&manifest_path, bytes.artifact_manifest_json)?;
        atomic_write(&capabilities_path, bytes.capabilities_manifest_json)?;

        // Update the artifact index.
        let mut entries = read_wasm_artifact_index(ail_dir).unwrap_or_default();
        let stored_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(existing) = entries.iter_mut().find(|e| e.hash == hash) {
            existing.stored_at = stored_at;
            existing.profile = profile.to_string();
        } else {
            entries.push(WasmArtifactIndexEntry {
                hash: hash.to_string(),
                profile: profile.to_string(),
                target: target.to_string(),
                stored_at,
            });
        }
        write_wasm_artifact_index(ail_dir, &entries)?;

        Ok(Some(WasmArtifactPaths {
            wasm_path,
            source_map_path,
            manifest_path,
            capabilities_path,
        }))
    }

    /// Load a persisted WASM artifact by name or hash.
    ///
    /// Lookup order:
    /// 1. If `name` is a 64-char hex string → exact hash match in index.
    /// 2. Strip `.wasm` suffix → match against `profile` in index (latest by `stored_at`).
    /// 3. Return the latest entry overall.
    ///
    /// Returns `Ok(None)` when no persisted artifact is found or when using a
    /// non-file-backed backend.
    pub fn load_wasm_artifact(
        &self,
        name: &str,
    ) -> Result<Option<PersistedWasmArtifact>, CliError> {
        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let entries = read_wasm_artifact_index(ail_dir).unwrap_or_default();
        if entries.is_empty() {
            return Ok(None);
        }

        let entry = if is_object_file_name(name) {
            // Exact hash lookup.
            entries.iter().find(|e| e.hash == name)
        } else {
            // Profile-name match (strip optional .wasm suffix), then fall back to latest.
            let profile_guess = name.strip_suffix(".wasm").unwrap_or(name);
            entries
                .iter()
                .filter(|e| e.profile == profile_guess)
                .max_by_key(|e| e.stored_at)
                .or_else(|| entries.iter().max_by_key(|e| e.stored_at))
        };

        let Some(entry) = entry else {
            return Ok(None);
        };

        let dir = wasm_dir(ail_dir);
        let wasm_path = dir.join(format!("{}.wasm", entry.hash));
        let source_map_path = dir.join(format!("{}.source_map.json", entry.hash));
        let manifest_path = dir.join(format!("{}.manifest.json", entry.hash));
        let capabilities_path = dir.join(format!("{}.capabilities.json", entry.hash));

        if !wasm_path.exists() {
            return Ok(None);
        }

        let wasm_bytes = std::fs::read(&wasm_path)?;
        let source_map_json = if source_map_path.exists() {
            std::fs::read(&source_map_path)?
        } else {
            b"{}".to_vec()
        };
        let artifact_manifest_json = if manifest_path.exists() {
            std::fs::read(&manifest_path)?
        } else {
            b"{}".to_vec()
        };
        let capabilities_manifest_json = if capabilities_path.exists() {
            std::fs::read(&capabilities_path)?
        } else {
            b"{\"entries\":[]}".to_vec()
        };

        Ok(Some(PersistedWasmArtifact {
            hash: entry.hash.clone(),
            profile: entry.profile.clone(),
            target: entry.target.clone(),
            wasm_bytes,
            source_map_json,
            artifact_manifest_json,
            capabilities_manifest_json,
            paths: WasmArtifactPaths {
                wasm_path,
                source_map_path,
                manifest_path,
                capabilities_path,
            },
        }))
    }
}

// ── WASM artifact persistence types ──────────────────────────────────────

/// Raw bytes passed to `save_wasm_artifact`.
///
/// Groups the four sidecar payloads so the method signature stays under the
/// clippy `too_many_arguments` threshold.
pub struct WasmArtifactBytes<'a> {
    /// Compiled WASM module bytes.
    pub wasm: &'a [u8],
    /// Source map JSON bytes.
    pub source_map_json: &'a [u8],
    /// Artifact manifest JSON bytes.
    pub artifact_manifest_json: &'a [u8],
    /// Capabilities manifest JSON bytes.
    pub capabilities_manifest_json: &'a [u8],
}

/// Index entry recorded in `.ail/wasm/artifact-index.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WasmArtifactIndexEntry {
    /// Blake3 hex hash of the raw WASM bytes (64-char lowercase hex).
    pub hash: String,
    /// Compilation profile (e.g. `"dev"`, `"prod"`).
    pub profile: String,
    /// Compilation target (e.g. `"wasm"`).
    pub target: String,
    /// Unix epoch seconds when the artifact was persisted.
    pub stored_at: u64,
}

/// On-disk paths for the four sidecar files written by `save_wasm_artifact`.
pub struct WasmArtifactPaths {
    pub wasm_path: PathBuf,
    pub source_map_path: PathBuf,
    pub manifest_path: PathBuf,
    pub capabilities_path: PathBuf,
}

/// A fully loaded persisted WASM artifact.
pub struct PersistedWasmArtifact {
    /// Blake3 hex hash of the raw WASM bytes.
    pub hash: String,
    /// Compilation profile.
    pub profile: String,
    /// Compilation target.
    pub target: String,
    /// Raw WASM bytes.
    pub wasm_bytes: Vec<u8>,
    /// Source map JSON bytes.
    pub source_map_json: Vec<u8>,
    /// Artifact manifest JSON bytes.
    pub artifact_manifest_json: Vec<u8>,
    /// Capabilities manifest JSON bytes.
    pub capabilities_manifest_json: Vec<u8>,
    /// On-disk file paths.
    pub paths: WasmArtifactPaths,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreDoctorReport {
    pub total_objects: usize,
    pub valid_objects: usize,
    pub corrupted_objects: usize,
    pub unreachable_objects: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreGcReport {
    pub objects_before: usize,
    pub objects_after: usize,
    pub bytes_freed: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SnapshotIndexEntry {
    id: ObjectId,
    created_at: u64,
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

    /// Expose construction for test helpers (e.g. doctor unit tests).
    #[cfg(test)]
    pub fn new_for_test(ail_dir: &Path) -> Self {
        Self::new(ail_dir)
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
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_object_file_name(file_name) {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            verify_object_bytes(file_name, &bytes)?;
            let Ok(snapshot) = CborCodec.decode::<SnapshotEnvelope>(&bytes) else {
                continue;
            };
            if snapshot.id == *id {
                return Ok(Some(snapshot));
            }
        }
        Ok(None)
    }

    fn list_snapshots_from_index(&self, ail_dir: &Path) -> StorageResult<Vec<SnapshotEnvelope>> {
        let entries = read_snapshot_index(ail_dir)?;
        if entries.is_empty() {
            return Ok(vec![]);
        }

        let mut snapshots = Vec::new();
        for entry in entries {
            let Some(snapshot) = self.find_snapshot(&entry.id)? else {
                return Err(StorageError::NotFound);
            };
            snapshots.push(snapshot);
        }
        Ok(snapshots)
    }
}

impl ObjectStore for FileObjectStore {
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        std::fs::create_dir_all(&self.objects_dir)?;
        let id = ObjectId::from_bytes(&object.0);
        let path = self.object_path(&id);
        if !path.exists() {
            atomic_write(&path, &object.0)?;
        }
        Ok(id)
    }

    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        let path = self.object_path(id);
        if path.exists() {
            let bytes = std::fs::read(path)?;
            if ObjectId::from_bytes(&bytes) != *id {
                return Err(StorageError::Codec(format!(
                    "object hash mismatch: expected {}",
                    id.to_hex()
                )));
            }
            Ok(Some(RawObject(bytes)))
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
    }
}

pub fn file_store(ail_dir: PathBuf) -> StoreHandle {
    file_handle(ail_dir)
}

#[cfg(test)]
pub fn init_file_layout(ail_dir: &Path) -> Result<(), CliError> {
    init_file_layout_with_branch(ail_dir, "main")
}

pub fn init_file_layout_with_branch(ail_dir: &Path, branch: &str) -> Result<(), CliError> {
    validate_branch_name(branch).map_err(CliError::Storage)?;
    std::fs::create_dir_all(ail_dir.join("refs").join("branches"))?;
    std::fs::create_dir_all(ail_dir.join("store").join("objects"))?;
    std::fs::create_dir_all(ail_dir.join("index"))?;
    write_head(ail_dir, branch).map_err(CliError::Storage)?;
    let branch_ref = branch_ref_path(ail_dir, branch).map_err(CliError::Storage)?;
    if !branch_ref.exists() {
        atomic_write_text(&branch_ref, "").map_err(CliError::Storage)?;
    }
    Ok(())
}

pub fn doctor(ail_dir: &Path) -> StorageResult<StoreDoctorReport> {
    let objects_dir = ail_dir.join("store").join("objects");
    let mut object_ids = Vec::new();
    let mut corrupted_objects = 0usize;

    if objects_dir.exists() {
        for entry in std::fs::read_dir(&objects_dir)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_object_file_name(file_name) {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            if verify_object_bytes(file_name, &bytes).is_ok() {
                object_ids.push(hex_to_object_id(file_name)?);
            } else {
                corrupted_objects += 1;
            }
        }
    }

    let total_objects = object_ids.len() + corrupted_objects;
    let reachable = reachable_objects(ail_dir)?;
    let unreachable_objects = object_ids
        .iter()
        .filter(|id| !reachable.contains(id))
        .count();

    Ok(StoreDoctorReport {
        total_objects,
        valid_objects: object_ids.len(),
        corrupted_objects,
        unreachable_objects,
    })
}

pub fn gc(ail_dir: &Path) -> StorageResult<StoreGcReport> {
    let objects_dir = ail_dir.join("store").join("objects");
    let reachable = reachable_objects(ail_dir)?;
    let mut objects_before = 0usize;
    let mut objects_after = 0usize;
    let mut bytes_freed = 0u64;

    if objects_dir.exists() {
        for entry in std::fs::read_dir(&objects_dir)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_object_file_name(file_name) {
                continue;
            }
            objects_before += 1;
            let id = hex_to_object_id(file_name)?;
            if reachable.contains(&id) {
                objects_after += 1;
            } else {
                bytes_freed += std::fs::metadata(&path)?.len();
                std::fs::remove_file(&path)?;
            }
        }
    }

    Ok(StoreGcReport {
        objects_before,
        objects_after,
        bytes_freed,
    })
}

fn file_handle(ail_dir: PathBuf) -> StoreHandle {
    let objects = FileObjectStore::new(&ail_dir);
    StoreHandle::File {
        graph: ObjectBackedGraphStore::new(objects.clone()),
        objects,
        ail_dir,
    }
}

// ── WASM artifact index helpers ───────────────────────────────────────────

fn wasm_dir(ail_dir: &Path) -> PathBuf {
    ail_dir.join("wasm")
}

fn wasm_artifact_index_path(ail_dir: &Path) -> PathBuf {
    wasm_dir(ail_dir).join("artifact-index.json")
}

fn read_wasm_artifact_index(ail_dir: &Path) -> Result<Vec<WasmArtifactIndexEntry>, CliError> {
    let path = wasm_artifact_index_path(ail_dir);
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(&path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Domain(format!("wasm artifact index decode: {e}")))
}

fn write_wasm_artifact_index(
    ail_dir: &Path,
    entries: &[WasmArtifactIndexEntry],
) -> Result<(), CliError> {
    let bytes = serde_json::to_vec(entries)
        .map_err(|e| CliError::Domain(format!("wasm artifact index encode: {e}")))?;
    atomic_write(&wasm_artifact_index_path(ail_dir), &bytes)?;
    Ok(())
}

fn write_object_ref(path: &Path, id: &ObjectId) -> StorageResult<()> {
    atomic_write_text(path, &format!("{}\n", id.to_hex()))
}

fn write_head(ail_dir: &Path, branch: &str) -> StorageResult<()> {
    atomic_write_text(
        &ail_dir.join("HEAD"),
        &format!("ref: refs/branches/{branch}\n"),
    )
}

fn atomic_write_text(path: &Path, content: &str) -> StorageResult<()> {
    atomic_write(path, content.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> StorageResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

fn current_branch(ail_dir: &Path) -> StorageResult<String> {
    let head_path = ail_dir.join("HEAD");
    if !head_path.exists() {
        return Ok("main".to_string());
    }
    let head = std::fs::read_to_string(head_path)?;
    let head = head.trim();
    let Some(branch) = head.strip_prefix("ref: refs/branches/") else {
        return Ok("main".to_string());
    };
    validate_branch_name(branch)?;
    Ok(branch.to_string())
}

fn branch_ref_path(ail_dir: &Path, branch: &str) -> StorageResult<PathBuf> {
    validate_branch_name(branch)?;
    Ok(ail_dir.join("refs").join("branches").join(branch))
}

fn read_branch_ref(path: &Path) -> StorageResult<Option<ObjectId>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(hex_to_object_id(trimmed)?))
}

fn read_snapshot_index(ail_dir: &Path) -> StorageResult<Vec<SnapshotIndexEntry>> {
    let path = ail_dir.join("index").join("snapshots.cbor");
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(path)?;
    CborCodec.decode::<Vec<SnapshotIndexEntry>>(&bytes)
}

fn update_snapshot_index(ail_dir: &Path, snapshot: &SnapshotEnvelope) -> StorageResult<()> {
    let mut entries = read_snapshot_index(ail_dir)?;
    if let Some(existing) = entries.iter_mut().find(|entry| entry.id == snapshot.id) {
        existing.created_at = snapshot.created_at;
    } else {
        entries.push(SnapshotIndexEntry {
            id: snapshot.id,
            created_at: snapshot.created_at,
        });
    }
    entries.sort_by_key(|entry| entry.created_at);

    let bytes = CborCodec.encode(&entries)?;
    atomic_write(&ail_dir.join("index").join("snapshots.cbor"), &bytes)
}

fn reachable_objects(ail_dir: &Path) -> StorageResult<std::collections::BTreeSet<ObjectId>> {
    let objects = FileObjectStore::new(ail_dir);
    let mut reachable = std::collections::BTreeSet::new();
    let branches_dir = ail_dir.join("refs").join("branches");

    if branches_dir.exists() {
        for entry in std::fs::read_dir(branches_dir)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let mut next = read_branch_ref(&path)?;
            while let Some(snapshot_id) = next {
                let Some(snapshot) = objects.find_snapshot(&snapshot_id)? else {
                    break;
                };
                let snapshot_cas = ObjectId::from_bytes(&CborCodec.encode(&snapshot)?);
                if !reachable.insert(snapshot_cas) {
                    break;
                }
                reachable.insert(snapshot.graph_root_hash);
                if let Some(change_id) = snapshot.applied_change_id {
                    reachable.insert(change_id);
                }
                if let Some(report_hash) = snapshot.verification_report_hash {
                    reachable.insert(ObjectId::from(report_hash));
                }
                next = snapshot.parent_id;
            }
        }
    }

    Ok(reachable)
}

fn verify_object_bytes(file_name: &str, bytes: &[u8]) -> StorageResult<()> {
    let expected = hex_to_object_id(file_name)?;
    let actual = ObjectId::from_bytes(bytes);
    if expected != actual {
        return Err(StorageError::Codec(format!(
            "object hash mismatch: expected {file_name}, actual {}",
            actual.to_hex()
        )));
    }
    Ok(())
}

fn is_object_file_name(name: &str) -> bool {
    name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_branch_name(branch: &str) -> StorageResult<()> {
    if branch.is_empty()
        || branch.starts_with('/')
        || branch.contains('/')
        || branch.contains("..")
        || branch.contains('\\')
        || branch.chars().any(char::is_whitespace)
    {
        return Err(StorageError::Codec(format!(
            "invalid branch name: {branch}"
        )));
    }
    Ok(())
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

    // ── T4: WASM artifact persistence ─────────────────────────────────────

    // Scenario: save_wasm_artifact returns None for memory backend (no-op).
    //   GIVEN a memory StoreHandle
    //   WHEN save_wasm_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn save_wasm_artifact_memory_is_noop() {
        let store = memory_store();
        let result = store.save_wasm_artifact(
            &"a".repeat(64),
            "dev",
            "wasm",
            WasmArtifactBytes {
                wasm: b"fake-wasm",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        );
        assert!(result.is_ok(), "save_wasm_artifact must not error");
        assert!(
            result.unwrap().is_none(),
            "memory store must return None (no-op)"
        );
    }

    // Scenario: save_wasm_artifact writes four sidecar files for file backend.
    //   GIVEN a file StoreHandle
    //   WHEN save_wasm_artifact is called
    //   THEN .ail/wasm/<hash>.wasm and the three sidecar files are on disk
    #[test]
    fn save_wasm_artifact_file_writes_sidecars() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());

        let fake_hash = "c".repeat(64);
        let result = store.save_wasm_artifact(
            &fake_hash,
            "dev",
            "wasm",
            WasmArtifactBytes {
                wasm: b"fake-wasm-bytes",
                source_map_json: b"{\"mappings\":[]}",
                artifact_manifest_json: b"{\"profile\":\"dev\"}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        );
        let paths = result
            .expect("save_wasm_artifact must succeed")
            .expect("file store must return Some(paths)");

        assert!(paths.wasm_path.exists(), "wasm file must exist");
        assert!(paths.source_map_path.exists(), "source_map file must exist");
        assert!(paths.manifest_path.exists(), "manifest file must exist");
        assert!(
            paths.capabilities_path.exists(),
            "capabilities file must exist"
        );

        let wasm_bytes = std::fs::read(&paths.wasm_path).expect("read wasm");
        assert_eq!(wasm_bytes, b"fake-wasm-bytes", "wasm bytes must match");

        // Index must also be written.
        let index_path = ail_dir.join("wasm").join("artifact-index.json");
        assert!(index_path.exists(), "artifact-index.json must be written");
        let index_bytes = std::fs::read(&index_path).expect("read index");
        let index: Vec<WasmArtifactIndexEntry> =
            serde_json::from_slice(&index_bytes).expect("parse index");
        assert_eq!(index.len(), 1, "index must have one entry");
        assert_eq!(index[0].hash, fake_hash, "index hash must match");
        assert_eq!(index[0].profile, "dev");
        assert_eq!(index[0].target, "wasm");
    }

    // Scenario: load_wasm_artifact returns None for memory backend.
    //   GIVEN a memory StoreHandle
    //   WHEN load_wasm_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn load_wasm_artifact_memory_returns_none() {
        let store = memory_store();
        let result = store.load_wasm_artifact("dev.wasm");
        assert!(result.is_ok(), "load_wasm_artifact must not error");
        assert!(result.unwrap().is_none(), "memory store must return None");
    }

    // Scenario: save + load roundtrip for file backend.
    //   GIVEN a file StoreHandle with a saved artifact
    //   WHEN load_wasm_artifact is called with a matching profile name
    //   THEN the loaded artifact bytes match the saved bytes
    #[test]
    fn wasm_artifact_save_load_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());

        let fake_hash = "d".repeat(64);
        store
            .save_wasm_artifact(
                &fake_hash,
                "dev",
                "wasm",
                WasmArtifactBytes {
                    wasm: b"wasm-roundtrip",
                    source_map_json: b"{\"sm\":1}",
                    artifact_manifest_json: b"{\"mf\":1}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        // Load by profile name.
        let loaded = store
            .load_wasm_artifact("dev.wasm")
            .expect("load must not error")
            .expect("artifact must be found");

        assert_eq!(loaded.hash, fake_hash, "hash must match");
        assert_eq!(loaded.profile, "dev", "profile must match");
        assert_eq!(
            loaded.wasm_bytes, b"wasm-roundtrip",
            "wasm bytes must match"
        );
        assert_eq!(loaded.source_map_json, b"{\"sm\":1}");
        assert_eq!(loaded.artifact_manifest_json, b"{\"mf\":1}");
        assert_eq!(loaded.capabilities_manifest_json, b"{\"entries\":[]}");
    }

    // TRIANGULATE: load_wasm_artifact returns None when no artifact persisted.
    //   GIVEN a file StoreHandle with no saved artifact
    //   WHEN load_wasm_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn load_wasm_artifact_file_returns_none_when_empty() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let result = store.load_wasm_artifact("program.wasm");
        assert!(result.is_ok(), "must not error");
        assert!(
            result.unwrap().is_none(),
            "no persisted artifact must return None"
        );
    }

    // TRIANGULATE: exact hash lookup in load_wasm_artifact.
    //   GIVEN a file store with a saved artifact
    //   WHEN load_wasm_artifact is called with the 64-char hash as name
    //   THEN the artifact is returned
    #[test]
    fn load_wasm_artifact_by_exact_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let fake_hash = "e".repeat(64);
        store
            .save_wasm_artifact(
                &fake_hash,
                "prod",
                "wasm",
                WasmArtifactBytes {
                    wasm: b"prod-wasm",
                    source_map_json: b"{}",
                    artifact_manifest_json: b"{}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        let loaded = store
            .load_wasm_artifact(&fake_hash)
            .expect("load must not error")
            .expect("artifact must be found by hash");

        assert_eq!(loaded.hash, fake_hash);
        assert_eq!(loaded.profile, "prod");
    }
}
