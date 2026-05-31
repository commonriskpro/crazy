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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::{StorageError, StorageResult};
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── SnapshotEnvelope ──────────────────────────────────────────────────────

/// Envelope that captures the state of the graph at a single point in time.
///
/// All six spec-required fields are present:
/// `id`, `graph_root_hash`, `parent_id`, `applied_change_id`, `created_at`,
/// and `verification_report_hash`.
///
/// `graph_root_hash` is the opaque `ObjectId` of the root graph object stored
/// in the backing `ObjectStore`.  `parent_id` links to the preceding snapshot,
/// or is `None` for a genesis (first) snapshot.  `applied_change_id` records
/// the change-set that produced this snapshot, or `None` for genesis.
/// `verification_report_hash` is the BLAKE3 hash of the verification report
/// associated with this snapshot, or `None` when no report has been produced
/// (e.g. genesis snapshots or snapshots that pre-date the verification pipeline).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// BLAKE3 hash of the verification report linked to this snapshot.
    ///
    /// `None` when no verification report has been produced (genesis, or
    /// snapshots that pre-date the verification pipeline).  Serialized only
    /// when `Some` to keep the CBOR representation backward-compatible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_report_hash: Option<[u8; 32]>,
    /// `ObjectId`s of approval and audit records associated with this snapshot
    /// or preserved from collapsed snapshots during compaction.
    ///
    /// For non-compacted snapshots this is typically empty — approval records
    /// are stored separately in the approval layer and linked here during
    /// compaction so they survive the covering-snapshot boundary.
    ///
    /// Serialized only when non-empty for backward compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit_record_ids: Vec<ObjectId>,
    /// `ObjectId`s of schema migration metadata records associated with this
    /// snapshot or preserved from collapsed snapshots during compaction.
    ///
    /// Migration reports (recording equivalence proofs after schema changes)
    /// are attached here so that compacted snapshots remain migration-history
    /// traceable without walking the full parent chain.
    ///
    /// Serialized only when non-empty for backward compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migration_metadata_ids: Vec<ObjectId>,
}

// ── Snapshot diagnostics ─────────────────────────────────────────────────

/// Stable, deterministic, redacted diagnostic for snapshot storage issues.
///
/// Descriptors are safe for production logs and health endpoints: they expose
/// short fingerprints for correlation without emitting full object ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotIssueDescriptor {
    /// Stable machine-readable code.
    pub code: String,
    /// Coarse issue category.
    pub category: String,
    /// Severity suitable for health reporting.
    pub severity: String,
    /// Redacted subject kind.
    pub subject: String,
    /// Stable redacted snapshot/object fingerprint.
    pub fingerprint: String,
    /// Stable machine-readable reason.
    pub reason: String,
    /// Human-readable message with no full object id.
    pub message: String,
}

/// Snapshot manifest entry used by production diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifestEntry {
    /// Snapshot identity recorded by the manifest.
    pub snapshot_id: ObjectId,
    /// Root object hash the manifest believes the snapshot has.
    pub graph_root_hash: ObjectId,
}

/// Snapshot index entry used by production diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotIndexEntry {
    /// Snapshot identity recorded by the index.
    pub snapshot_id: ObjectId,
    /// Root object hash the index believes the snapshot has.
    pub graph_root_hash: ObjectId,
}

/// Optional external metadata to validate against the stored snapshots.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDiagnosticsInput {
    /// Snapshot ids and roots recorded in a manifest.
    #[serde(default)]
    pub manifest_entries: Vec<SnapshotManifestEntry>,
    /// Snapshot ids and roots recorded in a lookup index.
    #[serde(default)]
    pub index_entries: Vec<SnapshotIndexEntry>,
}

/// A single snapshot storage issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotDiagnosticsIssue {
    /// A snapshot's graph root points to a missing object.
    MissingRootObject {
        /// Snapshot that referenced the missing root object.
        snapshot_id: ObjectId,
        /// Missing root object id.
        root_id: ObjectId,
    },
    /// The internal snapshot index points to a missing encoded snapshot object.
    MissingSnapshotObject {
        /// Snapshot identity recorded by the index.
        snapshot_id: ObjectId,
        /// Missing encoded snapshot object id.
        object_id: ObjectId,
    },
    /// Manifest metadata no longer matches the stored snapshot set.
    StaleManifestEntry {
        /// Snapshot identity recorded by the manifest.
        snapshot_id: ObjectId,
    },
    /// Index metadata no longer matches the stored snapshot set.
    StaleIndexEntry {
        /// Snapshot identity recorded by the index.
        snapshot_id: ObjectId,
    },
    /// The snapshot listing contains the same snapshot id more than once.
    DuplicateSnapshotId {
        /// Duplicated snapshot identity.
        snapshot_id: ObjectId,
    },
    /// A parent link points to a snapshot that is not present.
    ParentChainGap {
        /// Snapshot with the dangling parent link.
        snapshot_id: ObjectId,
        /// Missing parent snapshot id.
        parent_id: ObjectId,
    },
    /// Following parent links reaches a repeated snapshot id.
    ParentChainCycle {
        /// Snapshot whose ancestry enters a cycle.
        snapshot_id: ObjectId,
    },
}

impl SnapshotDiagnosticsIssue {
    fn kind_ord(&self) -> u8 {
        match self {
            SnapshotDiagnosticsIssue::MissingRootObject { .. } => 0,
            SnapshotDiagnosticsIssue::MissingSnapshotObject { .. } => 1,
            SnapshotDiagnosticsIssue::StaleManifestEntry { .. } => 2,
            SnapshotDiagnosticsIssue::StaleIndexEntry { .. } => 3,
            SnapshotDiagnosticsIssue::DuplicateSnapshotId { .. } => 4,
            SnapshotDiagnosticsIssue::ParentChainGap { .. } => 5,
            SnapshotDiagnosticsIssue::ParentChainCycle { .. } => 6,
        }
    }

    fn primary_id(&self) -> &ObjectId {
        match self {
            SnapshotDiagnosticsIssue::MissingRootObject { snapshot_id, .. }
            | SnapshotDiagnosticsIssue::MissingSnapshotObject { snapshot_id, .. }
            | SnapshotDiagnosticsIssue::StaleManifestEntry { snapshot_id }
            | SnapshotDiagnosticsIssue::StaleIndexEntry { snapshot_id }
            | SnapshotDiagnosticsIssue::DuplicateSnapshotId { snapshot_id }
            | SnapshotDiagnosticsIssue::ParentChainGap { snapshot_id, .. }
            | SnapshotDiagnosticsIssue::ParentChainCycle { snapshot_id } => snapshot_id,
        }
    }

    fn secondary_id(&self) -> Option<&ObjectId> {
        match self {
            SnapshotDiagnosticsIssue::MissingRootObject { root_id, .. } => Some(root_id),
            SnapshotDiagnosticsIssue::MissingSnapshotObject { object_id, .. } => Some(object_id),
            SnapshotDiagnosticsIssue::ParentChainGap { parent_id, .. } => Some(parent_id),
            SnapshotDiagnosticsIssue::StaleManifestEntry { .. }
            | SnapshotDiagnosticsIssue::StaleIndexEntry { .. }
            | SnapshotDiagnosticsIssue::DuplicateSnapshotId { .. }
            | SnapshotDiagnosticsIssue::ParentChainCycle { .. } => None,
        }
    }

    fn descriptor(&self) -> SnapshotIssueDescriptor {
        let (code, category, subject, reason, message) = match self {
            SnapshotDiagnosticsIssue::MissingRootObject { .. } => (
                "storage.snapshot.missing_root_object",
                "storage.snapshot.reachability",
                "snapshot_root",
                "missing_root_object",
                "snapshot graph root points to a missing object",
            ),
            SnapshotDiagnosticsIssue::MissingSnapshotObject { .. } => (
                "storage.snapshot.missing_snapshot_object",
                "storage.snapshot.index",
                "snapshot_object",
                "missing_snapshot_object",
                "snapshot index points to a missing encoded snapshot object",
            ),
            SnapshotDiagnosticsIssue::StaleManifestEntry { .. } => (
                "storage.snapshot.stale_manifest_entry",
                "storage.snapshot.manifest",
                "snapshot_manifest",
                "stale_manifest_entry",
                "snapshot manifest entry does not match stored snapshot metadata",
            ),
            SnapshotDiagnosticsIssue::StaleIndexEntry { .. } => (
                "storage.snapshot.stale_index_entry",
                "storage.snapshot.index",
                "snapshot_index",
                "stale_index_entry",
                "snapshot index entry does not match stored snapshot metadata",
            ),
            SnapshotDiagnosticsIssue::DuplicateSnapshotId { .. } => (
                "storage.snapshot.duplicate_id",
                "storage.snapshot.manifest",
                "snapshot",
                "duplicate_snapshot_id",
                "snapshot listing contains a duplicate snapshot id",
            ),
            SnapshotDiagnosticsIssue::ParentChainGap { .. } => (
                "storage.snapshot.parent_gap",
                "storage.snapshot.chain",
                "snapshot_parent",
                "parent_chain_gap",
                "snapshot parent link points to a missing snapshot",
            ),
            SnapshotDiagnosticsIssue::ParentChainCycle { .. } => (
                "storage.snapshot.parent_cycle",
                "storage.snapshot.chain",
                "snapshot_parent",
                "parent_chain_cycle",
                "snapshot parent chain contains a cycle",
            ),
        };

        SnapshotIssueDescriptor {
            code: code.to_owned(),
            category: category.to_owned(),
            severity: "error".to_owned(),
            subject: subject.to_owned(),
            fingerprint: snapshot_redacted_fingerprint(self.primary_id()),
            reason: reason.to_owned(),
            message: message.to_owned(),
        }
    }
}

/// Summary of a snapshot diagnostics run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDiagnosticsReport {
    /// All detected issues, sorted for determinism.
    pub issues: Vec<SnapshotDiagnosticsIssue>,
    /// Redacted, stable diagnostics derived from `issues`.
    #[serde(default)]
    pub diagnostics: Vec<SnapshotIssueDescriptor>,
    /// Number of decoded snapshot envelopes examined.
    pub snapshots_checked: u64,
    /// `true` iff no issues were detected.
    pub passed: bool,
}

/// Run read-only snapshot diagnostics against a graph store and object store.
///
/// This validates snapshot roots, duplicate snapshot ids, parent-chain gaps and
/// cycles, plus optional external manifest/index metadata.
///
/// For [`ObjectBackedGraphStore`], prefer
/// [`ObjectBackedGraphStore::diagnose_snapshots`] when internal index/object
/// drift also needs to be reported instead of returned as `list_snapshots`
/// failure.
pub async fn diagnose_snapshot_store<G, O>(
    graph_store: &G,
    object_store: &O,
    input: SnapshotDiagnosticsInput,
) -> StorageResult<SnapshotDiagnosticsReport>
where
    G: GraphStore + Send + Sync,
    O: ObjectStore + Send + Sync,
{
    let snapshots = graph_store.list_snapshots().await?;
    diagnose_snapshot_envelopes(&snapshots, object_store, input, Vec::new()).await
}

fn sort_snapshot_issues(issues: &mut [SnapshotDiagnosticsIssue]) {
    issues.sort_by(|a, b| {
        a.kind_ord()
            .cmp(&b.kind_ord())
            .then(a.primary_id().as_bytes().cmp(b.primary_id().as_bytes()))
            .then(
                a.secondary_id()
                    .map(ObjectId::as_bytes)
                    .cmp(&b.secondary_id().map(ObjectId::as_bytes)),
            )
    });
}

async fn diagnose_snapshot_envelopes<O>(
    snapshots: &[SnapshotEnvelope],
    object_store: &O,
    input: SnapshotDiagnosticsInput,
    mut issues: Vec<SnapshotDiagnosticsIssue>,
) -> StorageResult<SnapshotDiagnosticsReport>
where
    O: ObjectStore + Send + Sync,
{
    let mut snapshot_by_id = BTreeMap::new();
    let mut seen_snapshot_ids = BTreeSet::new();
    let mut duplicated_snapshot_ids = BTreeSet::new();

    for snapshot in snapshots {
        if !seen_snapshot_ids.insert(snapshot.id) {
            duplicated_snapshot_ids.insert(snapshot.id);
        }
        snapshot_by_id.entry(snapshot.id).or_insert(snapshot);
    }

    for snapshot_id in duplicated_snapshot_ids {
        issues.push(SnapshotDiagnosticsIssue::DuplicateSnapshotId { snapshot_id });
    }

    for snapshot in snapshots {
        if !object_store.exists(&snapshot.graph_root_hash).await? {
            issues.push(SnapshotDiagnosticsIssue::MissingRootObject {
                snapshot_id: snapshot.id,
                root_id: snapshot.graph_root_hash,
            });
        }

        if let Some(parent_id) = snapshot.parent_id
            && !snapshot_by_id.contains_key(&parent_id)
        {
            issues.push(SnapshotDiagnosticsIssue::ParentChainGap {
                snapshot_id: snapshot.id,
                parent_id,
            });
        }
    }

    for entry in input.manifest_entries {
        match snapshot_by_id.get(&entry.snapshot_id) {
            Some(snapshot) if snapshot.graph_root_hash == entry.graph_root_hash => {}
            _ => issues.push(SnapshotDiagnosticsIssue::StaleManifestEntry {
                snapshot_id: entry.snapshot_id,
            }),
        }
    }

    for entry in input.index_entries {
        match snapshot_by_id.get(&entry.snapshot_id) {
            Some(snapshot) if snapshot.graph_root_hash == entry.graph_root_hash => {}
            _ => issues.push(SnapshotDiagnosticsIssue::StaleIndexEntry {
                snapshot_id: entry.snapshot_id,
            }),
        }
    }

    for snapshot in snapshots {
        let mut visited = BTreeSet::new();
        let mut current_id = Some(snapshot.id);

        while let Some(id) = current_id {
            if !visited.insert(id) {
                issues.push(SnapshotDiagnosticsIssue::ParentChainCycle {
                    snapshot_id: snapshot.id,
                });
                break;
            }

            current_id = match snapshot_by_id.get(&id) {
                Some(current) => current.parent_id,
                None => None,
            };
        }
    }

    sort_snapshot_issues(&mut issues);
    let diagnostics = issues
        .iter()
        .map(SnapshotDiagnosticsIssue::descriptor)
        .collect::<Vec<_>>();
    let passed = issues.is_empty();

    Ok(SnapshotDiagnosticsReport {
        issues,
        diagnostics,
        snapshots_checked: snapshots.len() as u64,
        passed,
    })
}

fn snapshot_redacted_fingerprint(id: &ObjectId) -> String {
    let hex = id.to_hex();
    format!("blake3:{}…", &hex[..12])
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

    /// List all saved `SnapshotEnvelope`s in insertion order.
    ///
    /// Returns an empty `Vec` when no snapshots have been saved.
    fn list_snapshots(&self) -> impl Future<Output = StorageResult<Vec<SnapshotEnvelope>>> + Send;
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

impl<S: ObjectStore + Send + Sync> ObjectBackedGraphStore<S> {
    /// Run read-only diagnostics over this store's snapshot index.
    ///
    /// Unlike `list_snapshots`, this reports an index entry that points to a
    /// missing encoded snapshot object as a redacted diagnostic instead of
    /// returning `StorageError::NotFound`.
    pub async fn diagnose_snapshots(
        &self,
        input: SnapshotDiagnosticsInput,
    ) -> StorageResult<SnapshotDiagnosticsReport> {
        let mut pairs: Vec<(ObjectId, ObjectId)> = {
            let guard = self
                .snapshot_index
                .lock()
                .expect("snapshot_index lock must not be poisoned");
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };

        pairs.sort_by(|(a_snapshot, a_object), (b_snapshot, b_object)| {
            a_snapshot
                .as_bytes()
                .cmp(b_snapshot.as_bytes())
                .then(a_object.as_bytes().cmp(b_object.as_bytes()))
        });

        let mut snapshots = Vec::with_capacity(pairs.len());
        let mut issues = Vec::new();

        for (indexed_snapshot_id, object_id) in pairs {
            match self.store.get(&object_id).await? {
                None => issues.push(SnapshotDiagnosticsIssue::MissingSnapshotObject {
                    snapshot_id: indexed_snapshot_id,
                    object_id,
                }),
                Some(raw) => {
                    let snapshot: SnapshotEnvelope = self.codec.decode(&raw.0)?;
                    if snapshot.id != indexed_snapshot_id {
                        issues.push(SnapshotDiagnosticsIssue::StaleIndexEntry {
                            snapshot_id: indexed_snapshot_id,
                        });
                    }
                    snapshots.push(snapshot);
                }
            }
        }

        diagnose_snapshot_envelopes(&snapshots, &self.store, input, issues).await
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

    async fn list_snapshots(&self) -> StorageResult<Vec<SnapshotEnvelope>> {
        // Collect all (envelope_id → cas_id) pairs from the index.
        let pairs: Vec<(ObjectId, ObjectId)> = {
            let guard = self
                .snapshot_index
                .lock()
                .expect("snapshot_index lock must not be poisoned");
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };

        let mut result = Vec::with_capacity(pairs.len());
        for (_envelope_id, cas_id) in pairs {
            match self.store.get(&cas_id).await? {
                None => {
                    // Index points to a missing CAS object — treat as corruption.
                    return Err(StorageError::NotFound);
                }
                Some(raw) => {
                    let snap: SnapshotEnvelope = self.codec.decode(&raw.0)?;
                    result.push(snap);
                }
            }
        }
        Ok(result)
    }
}

// ── MutableObjectBackedGraphStore deletion ───────────────────────────────

impl<S: ObjectStore + Send + Sync> ObjectBackedGraphStore<S> {
    /// Remove the snapshot identified by `id` from the internal index.
    ///
    /// The underlying raw object bytes are not erased from the `ObjectStore`
    /// (CAS stores are typically append-only); the index entry that maps
    /// `envelope.id → CAS id` is removed so that `load_snapshot` and
    /// `list_snapshots` will no longer return this snapshot.
    pub(crate) fn remove_snapshot_from_index(&self, id: &ObjectId) {
        let mut guard = self
            .snapshot_index
            .lock()
            .expect("snapshot_index lock must not be poisoned");
        guard.remove(id);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::memory::MemoryObjectStore;

    fn make_envelope(seed: &[u8]) -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(seed);
        let root = ObjectId::from_bytes(&[seed[0]; 32]);
        SnapshotEnvelope {
            id,
            graph_root_hash: root,
            parent_id: None,
            applied_change_id: None,
            created_at: 0,
            verification_report_hash: None,
            audit_record_ids: Vec::new(),
            migration_metadata_ids: Vec::new(),
        }
    }

    #[derive(Clone)]
    struct StaticGraphStore {
        snapshots: Vec<SnapshotEnvelope>,
    }

    impl GraphStore for StaticGraphStore {
        async fn save_snapshot(&self, value: &SnapshotEnvelope) -> StorageResult<ObjectId> {
            Ok(value.id)
        }

        async fn load_snapshot(&self, id: &ObjectId) -> StorageResult<Option<SnapshotEnvelope>> {
            Ok(self
                .snapshots
                .iter()
                .find(|snapshot| snapshot.id == *id)
                .cloned())
        }

        async fn append_changeset_log(&self, entry: &ChangeSetLogEntry) -> StorageResult<ObjectId> {
            Ok(entry.id)
        }

        async fn list_snapshots(&self) -> StorageResult<Vec<SnapshotEnvelope>> {
            Ok(self.snapshots.clone())
        }
    }

    async fn store_root(object_store: &MemoryObjectStore, label: &[u8]) -> ObjectId {
        object_store
            .put(RawObject(label.to_vec()))
            .await
            .expect("root object must store")
    }

    fn snapshot_with(id_seed: &[u8], root: ObjectId, parent: Option<ObjectId>) -> SnapshotEnvelope {
        let mut snapshot = make_envelope(id_seed);
        snapshot.graph_root_hash = root;
        snapshot.parent_id = parent;
        snapshot
    }

    // Scenario: production snapshot diagnostics surface stable redacted issues.
    //   GIVEN snapshots with a missing root, stale manifest/index metadata,
    //         a duplicate id, a parent gap, and a parent cycle
    //   WHEN snapshot diagnostics are run over differently ordered inputs
    //   THEN diagnostics are redacted and deterministically ordered
    #[tokio::test]
    async fn snapshot_diagnostics_are_redacted_and_deterministic() {
        let object_store = MemoryObjectStore::new();
        let present_root = store_root(&object_store, b"present-root").await;
        let missing_root = ObjectId::from_bytes(b"missing-root");

        let genesis = snapshot_with(b"snapshot-genesis", present_root, None);
        let missing_parent_id = ObjectId::from_bytes(b"missing-parent");
        let orphan = snapshot_with(b"snapshot-orphan", missing_root, Some(missing_parent_id));

        let mut cycle_a = snapshot_with(b"snapshot-cycle-a", present_root, None);
        let mut cycle_b = snapshot_with(b"snapshot-cycle-b", present_root, Some(cycle_a.id));
        cycle_a.parent_id = Some(cycle_b.id);

        let mut duplicate = orphan.clone();
        duplicate.graph_root_hash = present_root;

        let stale_input = SnapshotDiagnosticsInput {
            manifest_entries: vec![SnapshotManifestEntry {
                snapshot_id: genesis.id,
                graph_root_hash: missing_root,
            }],
            index_entries: vec![SnapshotIndexEntry {
                snapshot_id: genesis.id,
                graph_root_hash: missing_root,
            }],
        };

        let first_store = StaticGraphStore {
            snapshots: vec![
                cycle_b.clone(),
                orphan.clone(),
                genesis.clone(),
                cycle_a.clone(),
                duplicate.clone(),
            ],
        };
        let second_store = StaticGraphStore {
            snapshots: vec![duplicate, cycle_a, genesis.clone(), orphan, cycle_b],
        };

        let first = diagnose_snapshot_store(&first_store, &object_store, stale_input.clone())
            .await
            .expect("diagnostics must succeed");
        let second = diagnose_snapshot_store(&second_store, &object_store, stale_input)
            .await
            .expect("diagnostics must succeed");

        assert_eq!(first.diagnostics, second.diagnostics);
        assert!(!first.passed, "issues must fail diagnostics");
        assert_eq!(first.snapshots_checked, 5);

        let codes: Vec<&str> = first
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect();
        assert!(codes.contains(&"storage.snapshot.missing_root_object"));
        assert!(codes.contains(&"storage.snapshot.stale_manifest_entry"));
        assert!(codes.contains(&"storage.snapshot.stale_index_entry"));
        assert!(codes.contains(&"storage.snapshot.duplicate_id"));
        assert!(codes.contains(&"storage.snapshot.parent_gap"));
        assert!(codes.contains(&"storage.snapshot.parent_cycle"));

        let rendered = format!("{:?}", first.diagnostics);
        assert!(
            !rendered.contains(&genesis.id.to_hex()),
            "diagnostics must not expose full snapshot ids"
        );
        assert!(
            !rendered.contains(&missing_root.to_hex()),
            "diagnostics must not expose full object ids"
        );
    }

    // Scenario: object-backed diagnostics report index entries whose objects vanished.
    //   GIVEN the internal snapshot index points to a missing encoded snapshot object
    //   WHEN ObjectBackedGraphStore::diagnose_snapshots is called
    //   THEN a missing snapshot object diagnostic is returned instead of NotFound
    #[tokio::test]
    async fn object_backed_snapshot_diagnostics_reports_missing_index_object() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let snapshot_id = ObjectId::from_bytes(b"indexed-snapshot");
        let object_id = ObjectId::from_bytes(b"missing-snapshot-object");

        store
            .snapshot_index
            .lock()
            .expect("snapshot_index lock must not be poisoned")
            .insert(snapshot_id, object_id);

        let report = store
            .diagnose_snapshots(SnapshotDiagnosticsInput::default())
            .await
            .expect("diagnostics must not fail on missing indexed object");

        assert_eq!(report.snapshots_checked, 0);
        assert!(matches!(
            report.issues.as_slice(),
            [SnapshotDiagnosticsIssue::MissingSnapshotObject { snapshot_id: got_snapshot_id, object_id: got_object_id }]
            if *got_snapshot_id == snapshot_id && *got_object_id == object_id
        ));
        assert_eq!(
            report.diagnostics[0].code,
            "storage.snapshot.missing_snapshot_object"
        );
    }

    // Scenario: object-backed diagnostics report stale internal index entries.
    //   GIVEN an index key points to an encoded snapshot with a different id
    //   WHEN ObjectBackedGraphStore::diagnose_snapshots is called
    //   THEN the stale index diagnostic uses the index key, redacted
    #[tokio::test]
    async fn object_backed_snapshot_diagnostics_reports_stale_index_key() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let root = store_root(&store.store, b"root-for-stale-index").await;
        let actual = snapshot_with(b"actual-snapshot", root, None);
        let indexed_snapshot_id = ObjectId::from_bytes(b"stale-index-key");
        let bytes = store.codec.encode(&actual).expect("snapshot must encode");
        let object_id = store
            .store
            .put(RawObject(bytes))
            .await
            .expect("encoded snapshot must store");

        store
            .snapshot_index
            .lock()
            .expect("snapshot_index lock must not be poisoned")
            .insert(indexed_snapshot_id, object_id);

        let report = store
            .diagnose_snapshots(SnapshotDiagnosticsInput::default())
            .await
            .expect("diagnostics must succeed");

        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            SnapshotDiagnosticsIssue::StaleIndexEntry { snapshot_id }
            if *snapshot_id == indexed_snapshot_id
        )));
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "storage.snapshot.stale_index_entry")
        );
    }

    // Scenario: list_snapshots returns empty vec when no snapshots saved.
    //   GIVEN a fresh ObjectBackedGraphStore
    //   WHEN list_snapshots is called
    //   THEN an empty vec is returned
    #[tokio::test]
    async fn list_snapshots_empty_on_fresh_store() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let list = store
            .list_snapshots()
            .await
            .expect("list_snapshots must succeed");
        assert!(list.is_empty(), "fresh store must return empty list");
    }

    // Scenario: list_snapshots returns saved envelopes.
    //   GIVEN save_snapshot was called with two envelopes
    //   WHEN list_snapshots is called
    //   THEN both envelopes are present in the result
    #[tokio::test]
    async fn list_snapshots_returns_saved_envelopes() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e1 = make_envelope(b"envelope-one");
        let e2 = make_envelope(b"envelope-two");

        store.save_snapshot(&e1).await.expect("save e1");
        store.save_snapshot(&e2).await.expect("save e2");

        let list = store
            .list_snapshots()
            .await
            .expect("list_snapshots must succeed");
        assert_eq!(list.len(), 2, "must return exactly two envelopes");

        // Both ids must be present (order may vary).
        let ids: Vec<ObjectId> = list.iter().map(|s| s.id).collect();
        assert!(ids.contains(&e1.id), "e1 must be in list");
        assert!(ids.contains(&e2.id), "e2 must be in list");
    }

    // TRIANGULATE: save + load roundtrip for SnapshotEnvelope.
    //   GIVEN save_snapshot(e) was called
    //   WHEN load_snapshot(e.id) is called
    //   THEN the returned envelope equals e
    #[tokio::test]
    async fn save_and_load_snapshot_roundtrip() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(b"roundtrip-test");
        store.save_snapshot(&e).await.expect("save must succeed");
        let loaded = store.load_snapshot(&e.id).await.expect("load must succeed");
        assert_eq!(loaded, Some(e), "loaded envelope must equal original");
    }
}
