use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ail_storage::{ObjectBackedGraphStore, backends::memory::MemoryObjectStore};

use super::StoreHandle;

/// Construct a fresh in-memory `StoreHandle` without checking env vars.
///
/// Intended for tests that need a hermetic memory store without touching the
/// environment. Not part of the public production API.
#[cfg(test)]
pub fn memory_store() -> StoreHandle {
    memory_handle()
}

pub(super) fn memory_handle() -> StoreHandle {
    let objects = MemoryObjectStore::new();
    StoreHandle::Memory {
        graph: ObjectBackedGraphStore::new(objects.clone()),
        objects,
        report_index: Arc::new(Mutex::new(HashMap::new())),
    }
}
