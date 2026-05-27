use std::path::PathBuf;

use ail_storage::ObjectBackedGraphStore;

use super::{FileObjectStore, StoreHandle};

pub fn file_store(ail_dir: PathBuf) -> StoreHandle {
    file_handle(ail_dir)
}

pub(super) fn file_handle(ail_dir: PathBuf) -> StoreHandle {
    let objects = FileObjectStore::new(&ail_dir);
    StoreHandle::File {
        graph: ObjectBackedGraphStore::new(objects.clone()),
        objects,
        ail_dir,
    }
}
