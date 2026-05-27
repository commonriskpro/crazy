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
use crate::store_file::{
    FileObjectStore, branch_ref_path, current_branch, hex_to_object_id, read_branch_ref,
    update_snapshot_index, validate_branch_name, write_object_ref,
};

use super::atomic_write;
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
