// Structural diff storage.
//
// # Design
//
// A `StructuralDiff` is a first-class stored artifact that records the
// semantic changes introduced by a single `ChangeSet`.  Unlike text diffs,
// structural diffs are graph-level: they list which nodes were created,
// modified, or deleted, and which edges were added or removed.
//
// # Doc spec
//
// ```txt
// structural_diff change.add_checkout
//   creates fn.checkout
//   modifies module.checkout
//   connects fn.checkout uses cap.payment.charge
//   exposes api.checkout
// end
// ```
//
// # Index
//
// `ObjectBackedStructuralDiffStore` keeps an in-memory `diff_id → CAS id`
// index (same pattern as `ObjectBackedGraphStore::snapshot_index`) so that
// diffs can be retrieved by their caller-assigned identity rather than by
// the CAS hash of their CBOR bytes.
//
// # Determinism
//
// `StructuralDiff` uses `Vec` for ordered, deterministic CBOR serialization.
// No `HashMap` fields are permitted per the workspace determinism contract.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::{StorageError, StorageResult};
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── StructuralDiff ────────────────────────────────────────────────────────

/// A first-class stored artifact recording the semantic changes introduced
/// by a single change-set.
///
/// Unlike text diffs, `StructuralDiff` is a graph-level description: it
/// tracks which nodes were created, modified, or deleted, and which edges
/// were added or removed.  This enables audit queries and change-impact
/// analysis without replaying the full changeset history.
///
/// # Determinism contract
///
/// All collection fields use `Vec` for ordered, deterministic CBOR.
/// Callers must sort entries before construction if canonical order matters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructuralDiff {
    /// Caller-assigned identity of this diff record.
    ///
    /// Typically derived from the `ObjectId` of the associated change-set.
    /// Stored in the index so callers can retrieve the diff by this id.
    pub id: ObjectId,
    /// Identity of the change-set that produced this diff.
    pub change_id: ObjectId,
    /// `ObjectId`s of nodes created by this change (new nodes in the graph).
    pub created_nodes: Vec<ObjectId>,
    /// `ObjectId`s of nodes modified by this change (their content changed).
    pub modified_nodes: Vec<ObjectId>,
    /// `ObjectId`s of nodes deleted by this change (removed from the graph).
    pub deleted_nodes: Vec<ObjectId>,
    /// Edges added by this change as `(source, target)` pairs.
    pub added_edges: Vec<(ObjectId, ObjectId)>,
    /// Edges removed by this change as `(source, target)` pairs.
    pub removed_edges: Vec<(ObjectId, ObjectId)>,
    /// `ObjectId`s of nodes newly exposed to the public API by this change.
    pub exposed_nodes: Vec<ObjectId>,
    /// Unix timestamp in milliseconds when this diff was recorded.
    pub created_at: u64,
}

// ── StructuralDiffStore trait ─────────────────────────────────────────────

/// Async storage contract for `StructuralDiff` records.
///
/// Diffs are stored as content-addressed objects indexed by `diff.id`.
/// Retrieving a diff by its caller-assigned `id` is always supported
/// (unlike a raw CAS store which would only support retrieval by hash).
pub trait StructuralDiffStore {
    /// Store `diff` and return its caller-assigned `diff.id`.
    ///
    /// If a diff with the same `id` already exists in the index, the index
    /// entry is updated (idempotent for re-writes of identical diffs).
    fn store_diff(
        &self,
        diff: &StructuralDiff,
    ) -> impl Future<Output = StorageResult<ObjectId>> + Send;

    /// Retrieve the `StructuralDiff` whose `id` field equals `id`, or `None`
    /// if no such diff has been stored.
    fn load_diff(
        &self,
        id: &ObjectId,
    ) -> impl Future<Output = StorageResult<Option<StructuralDiff>>> + Send;

    /// List all stored `StructuralDiff`s in unspecified order.
    ///
    /// Returns an empty `Vec` when no diffs have been stored.
    fn list_diffs(&self) -> impl Future<Output = StorageResult<Vec<StructuralDiff>>> + Send;
}

// ── ObjectBackedStructuralDiffStore ──────────────────────────────────────

/// `StructuralDiffStore` implementation backed by any `ObjectStore`.
///
/// Diffs are serialized with `CborCodec` and stored as raw CAS bytes.
/// An internal `diff_index` maps `diff.id → CAS id` so that
/// `load_diff(diff.id)` retrieves the correct record.
pub struct ObjectBackedStructuralDiffStore<S> {
    store: S,
    codec: CborCodec,
    /// Maps `diff.id → CAS id` of the stored diff.
    ///
    /// `Arc<Mutex<_>>` keeps the store usable across `&self` async calls.
    diff_index: Arc<Mutex<HashMap<ObjectId, ObjectId>>>,
}

impl<S: ObjectStore + Send + Sync> ObjectBackedStructuralDiffStore<S> {
    /// Wrap `store` in an `ObjectBackedStructuralDiffStore`.
    pub fn new(store: S) -> Self {
        Self {
            store,
            codec: CborCodec,
            diff_index: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S: ObjectStore + Send + Sync> StructuralDiffStore for ObjectBackedStructuralDiffStore<S> {
    async fn store_diff(&self, diff: &StructuralDiff) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(diff)?;
        let cas_id = self.store.put(RawObject(bytes)).await?;
        let mut guard = self
            .diff_index
            .lock()
            .expect("diff_index lock must not be poisoned");
        guard.insert(diff.id, cas_id);
        Ok(diff.id)
    }

    async fn load_diff(&self, id: &ObjectId) -> StorageResult<Option<StructuralDiff>> {
        let cas_id = {
            let guard = self
                .diff_index
                .lock()
                .expect("diff_index lock must not be poisoned");
            guard.get(id).copied()
        };
        match cas_id {
            None => Ok(None),
            Some(cas_id) => match self.store.get(&cas_id).await? {
                None => Err(StorageError::NotFound),
                Some(raw) => {
                    let diff = self.codec.decode(&raw.0)?;
                    Ok(Some(diff))
                }
            },
        }
    }

    async fn list_diffs(&self) -> StorageResult<Vec<StructuralDiff>> {
        let pairs: Vec<(ObjectId, ObjectId)> = {
            let guard = self
                .diff_index
                .lock()
                .expect("diff_index lock must not be poisoned");
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };

        let mut result = Vec::with_capacity(pairs.len());
        for (_diff_id, cas_id) in pairs {
            match self.store.get(&cas_id).await? {
                None => {
                    return Err(StorageError::NotFound);
                }
                Some(raw) => {
                    let diff: StructuralDiff = self.codec.decode(&raw.0)?;
                    result.push(diff);
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

    fn make_diff(seed: &[u8]) -> StructuralDiff {
        let id = ObjectId::from_bytes(seed);
        let change_id = ObjectId::from_bytes(&[seed[0]; 32]);
        StructuralDiff {
            id,
            change_id,
            created_nodes: Vec::new(),
            modified_nodes: Vec::new(),
            deleted_nodes: Vec::new(),
            added_edges: Vec::new(),
            removed_edges: Vec::new(),
            exposed_nodes: Vec::new(),
            created_at: 1_000,
        }
    }

    // Scenario: store then load a StructuralDiff.
    //   GIVEN store_diff(diff) was called
    //   WHEN load_diff(diff.id) is called
    //   THEN the returned diff equals the original
    #[test]
    fn store_and_load_diff_roundtrip() {
        let store = ObjectBackedStructuralDiffStore::new(MemoryObjectStore::new());
        let diff = make_diff(b"diff-add-checkout");

        block_on(async {
            let returned_id = store.store_diff(&diff).await.expect("store_diff");
            assert_eq!(returned_id, diff.id, "store_diff must return diff.id");

            let loaded = store.load_diff(&diff.id).await.expect("load_diff");
            assert_eq!(loaded, Some(diff.clone()), "loaded diff must equal original");
        });
    }

    // Scenario: load_diff for unknown id returns None.
    //   GIVEN an empty store
    //   WHEN load_diff(&unknown) is called
    //   THEN None is returned
    #[test]
    fn load_diff_missing_returns_none() {
        let store = ObjectBackedStructuralDiffStore::new(MemoryObjectStore::new());
        let id = ObjectId::from_bytes(b"not-stored");

        block_on(async {
            let result = store.load_diff(&id).await.expect("load_diff");
            assert_eq!(result, None, "missing diff must return None");
        });
    }

    // Scenario: list_diffs returns empty vec on fresh store.
    #[test]
    fn list_diffs_empty_on_fresh_store() {
        let store = ObjectBackedStructuralDiffStore::new(MemoryObjectStore::new());
        block_on(async {
            let list = store.list_diffs().await.expect("list_diffs");
            assert!(list.is_empty(), "fresh store must return empty list");
        });
    }

    // Scenario: list_diffs returns all stored diffs.
    //   GIVEN two diffs stored
    //   WHEN list_diffs is called
    //   THEN both are present
    #[test]
    fn list_diffs_returns_all_stored() {
        let store = ObjectBackedStructuralDiffStore::new(MemoryObjectStore::new());
        let d1 = make_diff(b"diff-1");
        let d2 = make_diff(b"diff-2");

        block_on(async {
            store.store_diff(&d1).await.expect("store d1");
            store.store_diff(&d2).await.expect("store d2");

            let list = store.list_diffs().await.expect("list");
            assert_eq!(list.len(), 2, "must return exactly two diffs");

            let ids: Vec<ObjectId> = list.iter().map(|d| d.id).collect();
            assert!(ids.contains(&d1.id), "d1 must be in list");
            assert!(ids.contains(&d2.id), "d2 must be in list");
        });
    }

    // TRIANGULATE: diff with populated fields roundtrips correctly.
    #[test]
    fn diff_with_populated_fields_roundtrip() {
        let store = ObjectBackedStructuralDiffStore::new(MemoryObjectStore::new());
        let id = ObjectId::from_bytes(b"diff.add_checkout");
        let change_id = ObjectId::from_bytes(b"change.add_checkout");
        let node_checkout = ObjectId::from_bytes(b"fn.checkout");
        let cap_payment = ObjectId::from_bytes(b"cap.payment.charge");
        let api_checkout = ObjectId::from_bytes(b"api.checkout");
        let module_checkout = ObjectId::from_bytes(b"module.checkout");

        let diff = StructuralDiff {
            id,
            change_id,
            created_nodes: vec![node_checkout],
            modified_nodes: vec![module_checkout],
            deleted_nodes: Vec::new(),
            added_edges: vec![(node_checkout, cap_payment)],
            removed_edges: Vec::new(),
            exposed_nodes: vec![api_checkout],
            created_at: 1_234_567,
        };

        block_on(async {
            store.store_diff(&diff).await.expect("store");
            let loaded = store.load_diff(&id).await.expect("load");
            assert_eq!(loaded, Some(diff), "populated diff must roundtrip");
        });
    }
}
