// Filesystem-backed content-addressed object store using a temporary directory.
//
// Each object is stored as a file whose name is the lower-hex `ObjectId`.
// The directory is owned by a `TempDir` and is deleted when the store is dropped.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};

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
