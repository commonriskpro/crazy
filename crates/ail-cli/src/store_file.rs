// ── ail-cli::store_file ──────────────────────────────────────────────────
//
// File-backed `ObjectStore` implementation and low-level file system helpers
// for the `.ail/` directory layout.
//
// Public surface:
//   `FileObjectStore`             — content-addressed file object store.
//   `init_file_layout_with_branch` — initialise a fresh `.ail/` tree.
//   `init_file_layout`            — convenience wrapper (test only).
//
// `pub(crate)` helpers consumed by `store.rs` and `store_doctor.rs`:
//   atomic_write, write_object_ref, write_head, current_branch,
//   branch_ref_path, read_branch_ref, update_snapshot_index,
//   reachable_objects, is_object_file_name, validate_branch_name,
//   hex_to_object_id, verify_object_bytes.

use std::path::{Path, PathBuf};

use ail_storage::{
    SnapshotEnvelope,
    codec::{CborCodec, ContentCodec},
    error::{StorageError, StorageResult},
    object::{ObjectId, ObjectStore, RawObject},
};
use serde::{Deserialize, Serialize};

use crate::error::CliError;

// ── SnapshotIndexEntry ────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SnapshotIndexEntry {
    pub(crate) id: ObjectId,
    pub(crate) created_at: u64,
}

// ── FileObjectStore ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct FileObjectStore {
    objects_dir: PathBuf,
}

impl FileObjectStore {
    pub(crate) fn new(ail_dir: &Path) -> Self {
        Self {
            objects_dir: ail_dir.join("store").join("objects"),
        }
    }

    /// Expose construction for test helpers (e.g. doctor unit tests).
    #[cfg(test)]
    pub fn new_for_test(ail_dir: &Path) -> Self {
        Self::new(ail_dir)
    }

    pub(crate) fn object_path(&self, id: &ObjectId) -> PathBuf {
        self.objects_dir.join(id.to_hex())
    }

    pub(crate) fn find_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
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

    pub(crate) fn list_snapshots_from_index(
        &self,
        ail_dir: &Path,
    ) -> StorageResult<Vec<SnapshotEnvelope>> {
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

// ── Layout helpers ────────────────────────────────────────────────────────

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

// ── File system helpers ───────────────────────────────────────────────────

pub(crate) fn write_object_ref(path: &Path, id: &ObjectId) -> StorageResult<()> {
    atomic_write_text(path, &format!("{}\n", id.to_hex()))
}

pub(crate) fn write_head(ail_dir: &Path, branch: &str) -> StorageResult<()> {
    atomic_write_text(
        &ail_dir.join("HEAD"),
        &format!("ref: refs/branches/{branch}\n"),
    )
}

fn atomic_write_text(path: &Path, content: &str) -> StorageResult<()> {
    atomic_write(path, content.as_bytes())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> StorageResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

pub(crate) fn current_branch(ail_dir: &Path) -> StorageResult<String> {
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

pub(crate) fn branch_ref_path(ail_dir: &Path, branch: &str) -> StorageResult<PathBuf> {
    validate_branch_name(branch)?;
    Ok(ail_dir.join("refs").join("branches").join(branch))
}

pub(crate) fn read_branch_ref(path: &Path) -> StorageResult<Option<ObjectId>> {
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

pub(crate) fn read_snapshot_index(ail_dir: &Path) -> StorageResult<Vec<SnapshotIndexEntry>> {
    let path = ail_dir.join("index").join("snapshots.cbor");
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(path)?;
    CborCodec.decode::<Vec<SnapshotIndexEntry>>(&bytes)
}

pub(crate) fn update_snapshot_index(
    ail_dir: &Path,
    snapshot: &SnapshotEnvelope,
) -> StorageResult<()> {
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

pub(crate) fn reachable_objects(
    ail_dir: &Path,
) -> StorageResult<std::collections::BTreeSet<ObjectId>> {
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

pub(crate) fn verify_object_bytes(file_name: &str, bytes: &[u8]) -> StorageResult<()> {
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

pub fn is_object_file_name(name: &str) -> bool {
    name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn validate_branch_name(branch: &str) -> StorageResult<()> {
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

pub(crate) fn hex_to_object_id(hex: &str) -> StorageResult<ObjectId> {
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
