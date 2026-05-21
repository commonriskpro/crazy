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

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── SnapshotEnvelope ──────────────────────────────────────────────────────

/// Envelope that captures the state of the graph at a single point in time.
///
/// `graph_root_hash` is the opaque `ObjectId` of the root graph object stored
/// in the backing `ObjectStore`.  `parent` links to the preceding snapshot,
/// or is `None` for a genesis (first) snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    /// Content-addressed root of the graph captured by this snapshot.
    pub graph_root_hash: ObjectId,
    /// Parent snapshot, or `None` for a genesis snapshot.
    pub parent: Option<ObjectId>,
    /// Unix timestamp in milliseconds when this snapshot was created.
    pub created_at: u64,
}

// ── ChangeSetLogEntry ─────────────────────────────────────────────────────

/// A log entry recording one change-set applied on top of a snapshot.
///
/// `id` is the opaque identity of the change-set itself (not the CAS id of
/// this log object — that is returned by `GraphStore::append_changeset_log`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeSetLogEntry {
    /// Opaque identity of the change-set (e.g. the `ObjectId` of its data).
    pub id: ObjectId,
    /// The snapshot this log entry was applied on top of.
    pub snapshot_id: ObjectId,
    /// Unix timestamp in milliseconds when the change-set was recorded.
    pub created_at: u64,
}

// ── GraphStore trait ──────────────────────────────────────────────────────

/// Async storage contract for snapshot envelopes and change-set log entries.
///
/// Implementations encode domain values via `CborCodec`, store the resulting
/// bytes as `RawObject`s in an `ObjectStore`, and decode on retrieval.
pub trait GraphStore {
    /// Encode and store `value`; return its content-addressed `ObjectId`.
    fn save_snapshot(
        &self,
        value: &SnapshotEnvelope,
    ) -> impl Future<Output = StorageResult<ObjectId>> + Send;

    /// Load and decode the `SnapshotEnvelope` identified by `id`, or `None`
    /// if no such snapshot exists.
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
pub struct ObjectBackedGraphStore<S> {
    store: S,
    codec: CborCodec,
}

impl<S: ObjectStore + Send + Sync> ObjectBackedGraphStore<S> {
    /// Wrap `store` in an `ObjectBackedGraphStore`.
    pub fn new(store: S) -> Self {
        Self {
            store,
            codec: CborCodec,
        }
    }
}

impl<S: ObjectStore + Send + Sync> GraphStore for ObjectBackedGraphStore<S> {
    async fn save_snapshot(&self, value: &SnapshotEnvelope) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(value)?;
        self.store.put(RawObject(bytes)).await
    }

    async fn load_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
        match self.store.get(id).await? {
            None => Ok(None),
            Some(raw) => {
                let snap = self.codec.decode(&raw.0)?;
                Ok(Some(snap))
            }
        }
    }

    async fn append_changeset_log(&self, entry: &ChangeSetLogEntry) -> StorageResult<ObjectId> {
        let bytes = self.codec.encode(entry)?;
        self.store.put(RawObject(bytes)).await
    }
}
