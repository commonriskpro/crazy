// Filesystem-backed content-addressed object store using a temporary directory.
//
// Each object is stored as a file whose name is the lower-hex `ObjectId`.
// The directory is owned by a `TempDir` and is deleted when the store is dropped.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};
use crate::retention::{EnumerableObjectStore, MutableObjectStore};

/// A filesystem-backed `ObjectStore` rooted in a temporary directory.
///
/// Files are named by the lower-hex `ObjectId` of their contents.
/// The backing directory is removed when the store is dropped.
#[derive(Debug)]
pub struct TempfileObjectStore {
    /// Holds the `TempDir` alive for the lifetime of the store.
    _dir: Arc<TempDir>,
    /// Path to the directory (derived once to avoid repeated `path()` calls).
    root: PathBuf,
}

impl TempfileObjectStore {
    /// Create a new `TempfileObjectStore` backed by a fresh temporary directory.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if the temporary directory cannot be created.
    pub fn new() -> std::io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().to_path_buf();
        Ok(Self {
            _dir: Arc::new(dir),
            root,
        })
    }

    fn object_path(&self, id: &ObjectId) -> PathBuf {
        self.root.join(id.to_hex())
    }
}

fn object_id_from_hex(hex: &str) -> Option<ObjectId> {
    if hex.len() != 64 {
        return None;
    }

    let mut bytes = [0u8; 32];
    for (idx, byte) in bytes.iter_mut().enumerate() {
        let start = idx * 2;
        *byte = u8::from_str_radix(&hex[start..start + 2], 16).ok()?;
    }

    Some(ObjectId::from(bytes))
}

impl ObjectStore for TempfileObjectStore {
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        let id = ObjectId::from_bytes(&object.0);
        let path = self.object_path(&id);
        // Idempotent: do not rewrite if the file already exists.
        if !path.exists() {
            std::fs::write(&path, &object.0)?;
        }
        Ok(id)
    }

    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        let path = self.object_path(id);
        if path.exists() {
            let bytes = std::fs::read(&path)?;
            Ok(Some(RawObject(bytes)))
        } else {
            Ok(None)
        }
    }

    async fn exists(&self, id: &ObjectId) -> StorageResult<bool> {
        Ok(self.object_path(id).exists())
    }
}

impl EnumerableObjectStore for TempfileObjectStore {
    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        ObjectStore::get(self, id).await
    }

    async fn list_object_ids(&self) -> StorageResult<Vec<ObjectId>> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            if let Some(name) = entry.file_name().to_str()
                && let Some(id) = object_id_from_hex(name)
            {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }
}

impl MutableObjectStore for TempfileObjectStore {
    async fn delete_object(&self, id: &ObjectId) -> StorageResult<Option<u64>> {
        let path = self.object_path(id);
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        std::fs::remove_file(path)?;
        Ok(Some(metadata.len()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::retention::run_gc;

    #[tokio::test]
    async fn tempfile_list_object_ids_returns_stored_objects() {
        let store = TempfileObjectStore::new().expect("tempdir creation must succeed");
        let id = store
            .put(RawObject(b"tempfile object".to_vec()))
            .await
            .expect("put must succeed");

        let ids = store.list_object_ids().await.expect("list must succeed");

        assert_eq!(ids, vec![id]);
    }

    #[tokio::test]
    async fn tempfile_run_gc_deletes_unreachable_object() {
        let store = TempfileObjectStore::new().expect("tempdir creation must succeed");
        let keep_id = store
            .put(RawObject(b"keep".to_vec()))
            .await
            .expect("put keep");
        let drop_id = store
            .put(RawObject(b"drop".to_vec()))
            .await
            .expect("put drop");

        let report = run_gc(&store, &BTreeSet::from([keep_id]))
            .await
            .expect("gc must succeed");

        assert_eq!(report.objects_examined, 2);
        assert_eq!(report.objects_deleted, 1);
        assert!(store.exists(&keep_id).await.expect("exists keep"));
        assert!(!store.exists(&drop_id).await.expect("exists drop"));
    }
}
