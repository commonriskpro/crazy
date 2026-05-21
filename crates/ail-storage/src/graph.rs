// Snapshot / log envelopes, GraphStore trait, and ObjectBackedGraphStore.
//
// # Determinism contract
//
// `SnapshotEnvelope` and `ChangeSetLogEntry` are serialized via `CborCodec`
// before being stored as raw content-addressed objects.  They MUST satisfy
// the codec's determinism invariants:
//
// - No `HashMap` fields — use ordered collections or flat fields only.
// - No floating-point values.
// - Integer timestamps as `u64` Unix milliseconds.
//
// `graph_root_hash` is an opaque `ObjectId`; no Semantic Graph node/edge
// model is introduced in this crate.
//
// # Async trait syntax
//
// `GraphStore` uses Return-Position Impl Trait In Traits (RPITIT) with an
// explicit `+ Send` bound rather than `async fn` syntax.  This is intentional:
// native `async fn` in traits (Rust 1.75+) does not automatically add a `Send`
// bound to the returned future, which would prevent using the trait in
// multi-threaded contexts (e.g., `T: GraphStore + Send + Sync`).  Clippy does
// not flag RPITIT-style trait declarations.  `ObjectStore` follows the same
// convention for the same reason.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── SnapshotEnvelope ──────────────────────────────────────────────────────

/// Envelope that captures the state of the graph at a single point in time.
///
/// All five spec-required fields are present:
/// `id`, `graph_root_hash`, `parent_id`, `applied_change_id`, `created_at`.
///
/// `graph_root_hash` is the opaque `ObjectId` of the root graph object stored
/// in the backing `ObjectStore`.  `parent_id` links to the preceding snapshot,
/// or is `None` for a genesis (first) snapshot.  `applied_change_id` records
/// the change-set that produced this snapshot, or `None` for genesis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    /// Envelope identity: the `ObjectId` assigned by the caller (not the CAS
    /// id of the encoded bytes).  `GraphStore::load_snapshot` looks up by
    /// this field, not by the raw-bytes hash.
    pub id: ObjectId,
    /// Content-addressed root of the graph captured by this snapshot.
    pub graph_root_hash: ObjectId,
    /// Parent snapshot, or `None` for a genesis snapshot.
    pub parent_id: Option<ObjectId>,
    /// The change-set that produced this snapshot, or `None` for genesis.
    pub applied_change_id: Option<ObjectId>,
    /// Unix timestamp in milliseconds when this snapshot was created.
    pub created_at: u64,
}

// ── ChangeSetLogEntry ─────────────────────────────────────────────────────

/// A log entry recording one change-set applied on top of a snapshot.
///
/// All four spec-required fields are present:
/// `id`, `base_snapshot_id`, `payload_hash`, `created_at`.
///
/// `id` is the opaque identity of the change-set itself (not the CAS id of
/// this log object — that is returned by `GraphStore::append_changeset_log`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeSetLogEntry {
    /// Opaque identity of the change-set (e.g. the `ObjectId` of its data).
    pub id: ObjectId,
    /// The snapshot this log entry was applied on top of.
    pub base_snapshot_id: ObjectId,
    /// Content-addressed hash of the change-set payload bytes.
    pub payload_hash: ObjectId,
    /// Unix timestamp in milliseconds when the change-set was recorded.
    pub created_at: u64,
}

// ── GraphStore trait ──────────────────────────────────────────────────────

/// Async storage contract for snapshot envelopes and change-set log entries.
///
/// Implementations encode domain values via `CborCodec`, store the resulting
/// bytes as `RawObject`s in an `ObjectStore`, and decode on retrieval.
///
/// # Load semantics
///
/// `save_snapshot` returns `envelope.id` (the identity pre-assigned in the
/// envelope, **not** the CAS hash of the encoded bytes).  `load_snapshot`
/// looks up by the same `envelope.id`, so the spec scenario
/// `save_snapshot(e)` → `load_snapshot(e.id)` is always satisfied.
pub trait GraphStore {
    /// Encode and store `value`; return `value.id` (the envelope's own
    /// identity, not the raw-bytes CAS hash).
    fn save_snapshot(
        &self,
        value: &SnapshotEnvelope,
    ) -> impl Future<Output = StorageResult<ObjectId>> + Send;

    /// Load and decode the `SnapshotEnvelope` whose `id` field equals `id`,
    /// or `None` if no such snapshot has been saved.
    fn load_snapshot(
        &self,
        id: &ObjectId,
    ) -> impl Future<Output = StorageResult<Option<SnapshotEnvelope>>> + Send;

    /// Encode and store `entry` as a log object; return its CAS `ObjectId`.
    fn append_changeset_log(
        &self,
        entry: &ChangeSetLogEntry,
    ) -> impl Future<Output = StorageResult<ObjectId>> + Send;
}

// ── ObjectBackedGraphStore ────────────────────────────────────────────────

/// `GraphStore` implementation that delegates persistence to any `ObjectStore`.
///
/// Values are serialized with `CborCodec` before being stored as raw bytes,
/// and deserialized on load.  The store is generic so both `MemoryObjectStore`
/// and future production backends can be used without code changes.
///
/// # Index
///
/// An internal `snapshot_index` maps `envelope.id → CAS id` so that
/// `load_snapshot(envelope.id)` retrieves the correct object regardless of
/// the relationship between the caller-chosen envelope identity and the
/// content-hash of the encoded bytes.  This index is not serialized and is
/// scoped to a single `ObjectBackedGraphStore` instance.
pub struct ObjectBackedGraphStore<S> {
    store: S,
    codec: CborCodec,
    /// Maps `SnapshotEnvelope.id` → the CAS `ObjectId` returned by `store.put`.
    ///
    /// `Arc<Mutex<_>>` is used so the store remains `Clone` and can be used
    /// across `&self` async calls without ownership transfer.
    snapshot_index: Arc<Mutex<HashMap<ObjectId, ObjectId>>>,
}

impl<S: ObjectStore + Send + Sync> ObjectBackedGraphStore<S> {
    /// Wrap `store` in an `ObjectBackedGraphStore`.
    pub fn new(store: S) -> Self {
        Self {
            store,
            codec: CborCodec,
            snapshot_index: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S: ObjectStore + Send + Sync> GraphStore for ObjectBackedGraphStore<S> {
    async fn save_snapshot(&self, value: &SnapshotEnvelope) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(value)?;
        let cas_id = self.store.put(RawObject(bytes)).await?;
        // Register envelope.id → CAS id so load_snapshot can retrieve it.
        let mut guard = self
            .snapshot_index
            .lock()
            .expect("snapshot_index lock must not be poisoned");
        guard.insert(value.id, cas_id);
        // Return envelope.id per spec: callers use it with load_snapshot.
        Ok(value.id)
    }

    async fn load_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
        let cas_id = {
            let guard = self
                .snapshot_index
                .lock()
                .expect("snapshot_index lock must not be poisoned");
            guard.get(id).copied()
        };
        match cas_id {
            None => Ok(None),
            Some(cas_id) => match self.store.get(&cas_id).await? {
                None => Ok(None),
                Some(raw) => {
                    let snap = self.codec.decode(&raw.0)?;
                    Ok(Some(snap))
                }
            },
        }
    }

    async fn append_changeset_log(&self, entry: &ChangeSetLogEntry) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(entry)?;
        self.store.put(RawObject(bytes)).await
    }
}
