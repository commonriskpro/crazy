// In-memory content-addressed object store.
//
// Backed by an `Arc<Mutex<HashMap<ObjectId, RawObject>>>` so it can be
// shared across `put`/`get`/`exists` calls that require `&self`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};
use crate::retention::{EnumerableObjectStore, MutableObjectStore};

/// An in-memory `ObjectStore` suitable for tests and ephemeral workloads.
///
/// All data is lost when the store is dropped.
#[derive(Clone, Debug, Default)]
pub struct MemoryObjectStore {
    map: Arc<Mutex<HashMap<ObjectId, RawObject>>>,
}

impl MemoryObjectStore {
    /// Create a new, empty `MemoryObjectStore`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ObjectStore for MemoryObjectStore {
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        let id = ObjectId::from_bytes(&object.0);
        let mut guard = self.map.lock().expect("lock must not be poisoned");
        guard.entry(id).or_insert(object);
        Ok(id)
    }

    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        let guard = self.map.lock().expect("lock must not be poisoned");
        Ok(guard.get(id).cloned())
    }

    async fn exists(&self, id: &ObjectId) -> StorageResult<bool> {
        let guard = self.map.lock().expect("lock must not be poisoned");
        Ok(guard.contains_key(id))
    }
}

impl EnumerableObjectStore for MemoryObjectStore {
    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        ObjectStore::get(self, id).await
    }

    async fn list_object_ids(&self) -> StorageResult<Vec<ObjectId>> {
        let guard = self.map.lock().expect("lock must not be poisoned");
        Ok(guard.keys().cloned().collect())
    }
}

impl MutableObjectStore for MemoryObjectStore {
    async fn delete_object(&self, id: &ObjectId) -> StorageResult<Option<u64>> {
        let mut guard = self.map.lock().expect("lock must not be poisoned");
        Ok(guard.remove(id).map(|obj| obj.0.len() as u64))
    }
}
