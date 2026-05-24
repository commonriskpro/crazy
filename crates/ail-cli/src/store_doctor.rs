// ── ail-cli::store_doctor ─────────────────────────────────────────────────
//
// Store integrity and garbage-collection reports for the file-backed store.
//
// `doctor` — walk `.ail/store/objects`, hash-verify every object, and count
//            corrupted and unreachable objects.
// `gc`     — delete unreachable objects; return bytes reclaimed.
//
// Both operate on the `.ail/` directory path and are backend-agnostic at the
// call site; callers dispatch to them only when the active backend is `File`.

use std::path::Path;

use ail_storage::error::StorageResult;

use crate::store_file::{
    hex_to_object_id, is_object_file_name, reachable_objects, verify_object_bytes,
};

// ── Reports ───────────────────────────────────────────────────────────────

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

// ── doctor ────────────────────────────────────────────────────────────────

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

// ── gc ────────────────────────────────────────────────────────────────────

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
