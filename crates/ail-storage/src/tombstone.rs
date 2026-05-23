// Tombstone records for logical deletes.
//
// # Design
//
// The storage model uses logical deletes: removing a node from the active
// graph does not erase the underlying CAS objects — it writes an immutable
// tombstone record that marks the node as deleted, preserving the deletion
// provenance (which change caused it and what replaced it).
//
// # Doc spec
//
// ```txt
// tombstone fn.old_checkout
//   deleted_by change.remove_old_checkout
//   replacement fn.checkout_v2
// end
// ```
//
// # Index
//
// `ObjectBackedTombstoneStore` keeps an in-memory `deleted_id → CAS id`
// index (same pattern as `ObjectBackedGraphStore::snapshot_index`) so that
// tombstones can be retrieved by the identity of the deleted object rather
// than by the CAS hash of the tombstone bytes.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── Tombstone ─────────────────────────────────────────────────────────────

/// An immutable record written when a graph node or snapshot is logically
/// deleted.
///
/// Tombstones implement the "logical delete" principle: the object's identity
/// (`deleted_id`) is removed from the active index, but the tombstone is
/// retained so that:
///
/// - Audit queries can determine *when* and *why* the deletion occurred.
/// - Prior snapshots that still reference the deleted object remain readable.
/// - Replacement information links the old object to its successor (if any).
///
/// # Determinism contract
///
/// `Tombstone` is serialized via `CborCodec` — no floating-point values,
/// no `HashMap` fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tombstone {
    /// `ObjectId` of the change-set (or change event) that caused this
    /// deletion (e.g., `change.remove_old_checkout`).
    pub deleted_by: ObjectId,
    /// Optional `ObjectId` of the replacement node that supersedes the
    /// deleted object.  `None` when there is no direct replacement.
    pub replacement: Option<ObjectId>,
    /// Unix timestamp in milliseconds when this tombstone was written.
    pub timestamp: u64,
}

// ── TombstoneStore trait ──────────────────────────────────────────────────

/// Async storage contract for tombstone records.
///
/// Implementations store tombstones as content-addressed objects keyed by
/// the `ObjectId` of the deleted item, not by the tombstone's own CAS hash.
pub trait TombstoneStore {
    /// Write a tombstone for `deleted_id` and return the CAS `ObjectId` of
    /// the stored tombstone record.
    ///
    /// If a tombstone for `deleted_id` already exists it is overwritten
    /// (idempotent for repeated deletions of the same object).
    fn write_tombstone(
        &self,
        deleted_id: ObjectId,
        tombstone: Tombstone,
    ) -> impl Future<Output = StorageResult<ObjectId>> + Send;

    /// Retrieve the tombstone written for `deleted_id`, or `None` if the
    /// object was never tombstoned.
    fn read_tombstone(
        &self,
        deleted_id: &ObjectId,
    ) -> impl Future<Output = StorageResult<Option<Tombstone>>> + Send;

    /// List all tombstone records held in this store.
    ///
    /// Returns `(deleted_id, Tombstone)` pairs in unspecified order.
    /// Returns an empty `Vec` when no tombstones have been written.
    fn list_tombstones(
        &self,
    ) -> impl Future<Output = StorageResult<Vec<(ObjectId, Tombstone)>>> + Send;
}

// ── ObjectBackedTombstoneStore ────────────────────────────────────────────

/// `TombstoneStore` implementation that delegates persistence to any
/// `ObjectStore`.
///
/// Tombstones are serialized with `CborCodec` and stored as raw CAS bytes.
/// An internal `index` maps `deleted_id → CAS id` so that
/// `read_tombstone(deleted_id)` can retrieve the correct record.
pub struct ObjectBackedTombstoneStore<S> {
    store: S,
    codec: CborCodec,
    /// Maps `deleted_id → CAS id` of the stored tombstone record.
    ///
    /// `Arc<Mutex<_>>` keeps the store `Clone`-able and usable across
    /// `&self` async calls without ownership transfer.
    index: Arc<Mutex<HashMap<ObjectId, ObjectId>>>,
}

impl<S: ObjectStore + Send + Sync> ObjectBackedTombstoneStore<S> {
    /// Wrap `store` in an `ObjectBackedTombstoneStore`.
    pub fn new(store: S) -> Self {
        Self {
            store,
            codec: CborCodec,
            index: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S: ObjectStore + Send + Sync> TombstoneStore for ObjectBackedTombstoneStore<S> {
    async fn write_tombstone(
        &self,
        deleted_id: ObjectId,
        tombstone: Tombstone,
    ) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(&tombstone)?;
        let cas_id = self.store.put(RawObject(bytes)).await?;
        let mut guard = self
            .index
            .lock()
            .expect("tombstone index lock must not be poisoned");
        guard.insert(deleted_id, cas_id);
        Ok(cas_id)
    }

    async fn read_tombstone(&self, deleted_id: &ObjectId) -> StorageResult<Option<Tombstone>> {
        let cas_id = {
            let guard = self
                .index
                .lock()
                .expect("tombstone index lock must not be poisoned");
            guard.get(deleted_id).copied()
        };
        match cas_id {
            None => Ok(None),
            Some(cas_id) => match self.store.get(&cas_id).await? {
                None => Ok(None),
                Some(raw) => {
                    let tombstone = self.codec.decode(&raw.0)?;
                    Ok(Some(tombstone))
                }
            },
        }
    }

    async fn list_tombstones(&self) -> StorageResult<Vec<(ObjectId, Tombstone)>> {
        let pairs: Vec<(ObjectId, ObjectId)> = {
            let guard = self
                .index
                .lock()
                .expect("tombstone index lock must not be poisoned");
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };

        let mut result = Vec::with_capacity(pairs.len());
        for (deleted_id, cas_id) in pairs {
            match self.store.get(&cas_id).await? {
                None => {
                    // CAS object missing — skip gracefully (possible if the
                    // underlying store was compacted without the tombstone index).
                }
                Some(raw) => {
                    let tombstone: Tombstone = self.codec.decode(&raw.0)?;
                    result.push((deleted_id, tombstone));
                }
            }
        }
        Ok(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;
    use crate::backends::memory::MemoryObjectStore;

    fn make_tombstone(change_seed: &[u8]) -> Tombstone {
        Tombstone {
            deleted_by: ObjectId::from_bytes(change_seed),
            replacement: None,
            timestamp: 42_000,
        }
    }

    fn deleted_id(seed: &[u8]) -> ObjectId {
        ObjectId::from_bytes(seed)
    }

    // Scenario: write then read a tombstone — happy path.
    //   GIVEN write_tombstone(id, t) was called
    //   WHEN read_tombstone(&id) is called
    //   THEN the returned tombstone equals t
    #[test]
    fn write_and_read_tombstone_roundtrip() {
        let store = ObjectBackedTombstoneStore::new(MemoryObjectStore::new());
        let id = deleted_id(b"node.old_checkout");
        let t = make_tombstone(b"change.remove_old_checkout");

        block_on(async {
            store
                .write_tombstone(id, t.clone())
                .await
                .expect("write_tombstone must succeed");

            let loaded = store
                .read_tombstone(&id)
                .await
                .expect("read_tombstone must succeed");
            assert_eq!(loaded, Some(t), "loaded tombstone must equal original");
        });
    }

    // Scenario: read_tombstone for unknown id returns None.
    //   GIVEN an empty store
    //   WHEN read_tombstone(&unknown) is called
    //   THEN None is returned
    #[test]
    fn read_tombstone_missing_returns_none() {
        let store = ObjectBackedTombstoneStore::new(MemoryObjectStore::new());
        let id = deleted_id(b"not-deleted");

        block_on(async {
            let result = store
                .read_tombstone(&id)
                .await
                .expect("read_tombstone must not error");
            assert_eq!(result, None, "missing tombstone must return None");
        });
    }

    // Scenario: list_tombstones returns all written tombstones.
    //   GIVEN two tombstones written
    //   WHEN list_tombstones is called
    //   THEN both (deleted_id, tombstone) pairs are present
    #[test]
    fn list_tombstones_returns_all_written() {
        let store = ObjectBackedTombstoneStore::new(MemoryObjectStore::new());
        let id1 = deleted_id(b"node-alpha");
        let id2 = deleted_id(b"node-beta");
        let t1 = make_tombstone(b"change-x");
        let t2 = Tombstone {
            deleted_by: ObjectId::from_bytes(b"change-y"),
            replacement: Some(ObjectId::from_bytes(b"node-beta-v2")),
            timestamp: 99_000,
        };

        block_on(async {
            store
                .write_tombstone(id1, t1.clone())
                .await
                .expect("write t1");
            store
                .write_tombstone(id2, t2.clone())
                .await
                .expect("write t2");

            let list = store.list_tombstones().await.expect("list must succeed");
            assert_eq!(list.len(), 2, "must return exactly two tombstones");

            let ids: Vec<ObjectId> = list.iter().map(|(id, _)| *id).collect();
            assert!(ids.contains(&id1), "id1 must be in list");
            assert!(ids.contains(&id2), "id2 must be in list");
        });
    }

    // TRIANGULATE: tombstone with replacement roundtrips correctly.
    #[test]
    fn tombstone_with_replacement_roundtrip() {
        let store = ObjectBackedTombstoneStore::new(MemoryObjectStore::new());
        let id = deleted_id(b"fn.old_checkout");
        let replacement_id = ObjectId::from_bytes(b"fn.checkout_v2");
        let t = Tombstone {
            deleted_by: ObjectId::from_bytes(b"change.remove_old_checkout"),
            replacement: Some(replacement_id),
            timestamp: 1_000_000,
        };

        block_on(async {
            store.write_tombstone(id, t.clone()).await.expect("write");
            let loaded = store.read_tombstone(&id).await.expect("read");
            assert_eq!(loaded, Some(t.clone()), "tombstone must roundtrip");
            assert_eq!(
                loaded.unwrap().replacement,
                Some(replacement_id),
                "replacement must be preserved"
            );
        });
    }

    // TRIANGULATE: fresh store returns empty list.
    #[test]
    fn list_tombstones_empty_on_fresh_store() {
        let store = ObjectBackedTombstoneStore::new(MemoryObjectStore::new());
        block_on(async {
            let list = store.list_tombstones().await.expect("list must succeed");
            assert!(list.is_empty(), "fresh store must return empty list");
        });
    }
}
