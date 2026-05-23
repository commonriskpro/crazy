// Retention policies, physical GC, and snapshot compaction.
//
// # Design
//
// `RetentionPolicy` is a pure-data struct that encodes which snapshots are
// protected from physical deletion.  The GC and compaction operations take a
// `GraphStore` reference and a policy and produce typed reports without
// mutating the store directly — the store's own mutation API (`save_snapshot`,
// etc.) is used so that both in-memory and future production stores work.
//
// # Snapshot reachability
//
// A snapshot is *retained* (protected) when any of the following hold:
// - `keep_tagged` is true and the snapshot's `applied_change_id` is `Some`
//   (interpreted as "was applied via an explicit change — might be a release").
// - `keep_releases` is true and the snapshot's `parent_id` is `None`
//   (genesis snapshots are treated as baseline releases).
// - `max_age_days` is `Some(n)` and the snapshot's `created_at` timestamp
//   falls within the last `n * 86_400_000` milliseconds relative to `now_ms`.
//
// Everything else is unreferenced and can be removed by GC.
//
// # Compaction
//
// `compact_snapshots` collapses a contiguous range (by index in `list_snapshots`
// order) into a single covering snapshot that records which snapshot ids it
// replaces.  The covering snapshot keeps the `graph_root_hash` of the last
// snapshot in the range (i.e. the most recent graph state).  All collapsed
// originals are removed from the store and the covering snapshot is saved.
//
// # Determinism
//
// No `HashMap` fields appear in serializable types per the project's
// determinism contract.

use std::collections::BTreeSet;
use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::error::{StorageError, StorageResult};
use crate::graph::{GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
use crate::object::{ObjectId, ObjectStore};

// ── RetentionPolicy ───────────────────────────────────────────────────────

/// Policy that governs which snapshots survive physical GC.
///
/// Fields follow the spec directly:
/// - `max_age_days` — keep snapshots younger than this many days (wall clock).
///   `None` means no age-based retention; all snapshots may be collected.
/// - `keep_releases` — when `true`, genesis snapshots (`parent_id == None`)
///   are always retained regardless of age.
/// - `keep_tagged` — when `true`, snapshots that carry an `applied_change_id`
///   are always retained (interpreted as "tagged/released" snapshots).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Maximum age in days a snapshot may reach before becoming eligible for GC.
    /// `None` disables age-based retention (all snapshots may be collected
    /// unless protected by another rule).
    pub max_age_days: Option<u64>,
    /// If `true`, genesis snapshots (`parent_id == None`) are always kept.
    pub keep_releases: bool,
    /// If `true`, snapshots that have an `applied_change_id` are always kept.
    pub keep_tagged: bool,
}

impl RetentionPolicy {
    /// Return `true` if `snapshot` must be retained under this policy.
    ///
    /// `now_ms` is the current time as Unix milliseconds; it is taken as a
    /// parameter so callers can inject a deterministic clock in tests.
    pub fn is_retained(&self, snapshot: &SnapshotEnvelope, now_ms: u64) -> bool {
        // Rule 1: keep_releases protects genesis snapshots.
        if self.keep_releases && snapshot.parent_id.is_none() {
            return true;
        }
        // Rule 2: keep_tagged protects snapshots with an applied change.
        if self.keep_tagged && snapshot.applied_change_id.is_some() {
            return true;
        }
        // Rule 3: max_age_days protects young snapshots.
        if let Some(max_days) = self.max_age_days {
            let max_age_ms = max_days.saturating_mul(86_400_000);
            let cutoff = now_ms.saturating_sub(max_age_ms);
            if snapshot.created_at >= cutoff {
                return true;
            }
        }
        false
    }
}

// ── GcReport ──────────────────────────────────────────────────────────────

/// Summary of a physical GC run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GcReport {
    /// Number of snapshots examined.
    pub snapshots_examined: u64,
    /// Number of snapshots removed as unreferenced.
    pub snapshots_removed: u64,
    /// Number of snapshots retained (protected by policy).
    pub snapshots_retained: u64,
}

// ── CompactionReport ──────────────────────────────────────────────────────

/// Summary of a snapshot compaction run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompactionReport {
    /// Number of snapshots collapsed into the covering snapshot.
    pub snapshots_merged: u64,
    /// The `ObjectId` of the covering snapshot that replaced the range.
    pub covering_snapshot_id: ObjectId,
}

// ── ObjectStore mutation helper ───────────────────────────────────────────

/// Marker trait: a `GraphStore` that also supports snapshot deletion.
///
/// Standard `GraphStore` is append-only; GC needs to remove objects.  Rather
/// than modifying the `GraphStore` trait (which would affect all implementations),
/// GC accepts a mutable reference to a concrete store type that exposes deletion
/// via this additional trait.
pub trait MutableGraphStore: GraphStore {
    /// Remove the snapshot with `id` from the store.
    ///
    /// If no snapshot with that `id` exists, this is a no-op (idempotent).
    fn delete_snapshot(&self, id: &ObjectId) -> impl Future<Output = StorageResult<()>> + Send;
}

// ── MutableObjectStore ────────────────────────────────────────────────────

/// Extension of `ObjectStore` that supports enumeration and physical deletion
/// of content-addressed objects.
///
/// `ObjectStore` is append-only by design; this trait adds the mutating
/// operations required for GC without changing the base trait interface.
pub trait MutableObjectStore: crate::object::ObjectStore {
    /// Return all `ObjectId`s currently present in the store.
    fn list_object_ids(&self) -> impl Future<Output = StorageResult<Vec<ObjectId>>> + Send;

    /// Remove the object identified by `id` and return the number of bytes
    /// freed.  Returns `None` if no object with that `id` was present
    /// (idempotent).
    fn delete_object(
        &self,
        id: &ObjectId,
    ) -> impl Future<Output = StorageResult<Option<u64>>> + Send;
}

// ── ObjectGcReport ────────────────────────────────────────────────────────

/// Summary of a physical object-level GC run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectGcReport {
    /// Total number of CAS objects examined.
    pub objects_examined: u64,
    /// Number of unreachable objects deleted.
    pub objects_deleted: u64,
    /// Total bytes reclaimed by deleting unreachable objects.
    pub bytes_freed: u64,
}

// ── run_gc ────────────────────────────────────────────────────────────────

/// Physical garbage-collect unreachable CAS objects.
///
/// # Parameters
/// - `store`     — a `MutableObjectStore` (list + delete).
/// - `reachable` — the set of `ObjectId`s that must be kept (e.g. the
///   `graph_root_hash` values of all retained snapshots).  Objects NOT in
///   this set are considered unreachable and will be deleted.
///
/// # Returns
/// An `ObjectGcReport` counting examined, deleted, and bytes freed.
///
/// # Design
///
/// The caller is responsible for computing `reachable` from the graph store
/// (e.g. by calling `gc_unreferenced` first to identify retained snapshots
/// and then collecting their `graph_root_hash` fields).  This separation keeps
/// `run_gc` independent of the snapshot/graph layer and fully testable with a
/// plain `MemoryObjectStore`.
pub async fn run_gc<S>(store: &S, reachable: &BTreeSet<ObjectId>) -> StorageResult<ObjectGcReport>
where
    S: MutableObjectStore + Send + Sync,
{
    let all_ids = store.list_object_ids().await?;
    let mut report = ObjectGcReport {
        objects_examined: all_ids.len() as u64,
        objects_deleted: 0,
        bytes_freed: 0,
    };

    for id in &all_ids {
        if !reachable.contains(id)
            && let Some(bytes) = store.delete_object(id).await?
        {
            report.objects_deleted += 1;
            report.bytes_freed += bytes;
        }
    }

    Ok(report)
}

// ── gc_unreferenced ───────────────────────────────────────────────────────

/// Identify and remove snapshots that are not retained by `policy`.
///
/// # Parameters
/// - `store`  — a `MutableGraphStore` (supports both read and delete).
/// - `policy` — the retention policy to apply.
/// - `now_ms` — current Unix time in milliseconds (injected for determinism).
///
/// # Returns
/// A `GcReport` counting examined, retained, and removed snapshots.
///
/// # Errors
/// Propagates any `StorageError` from `list_snapshots` or `delete_snapshot`.
pub async fn gc_unreferenced<S>(
    store: &S,
    policy: &RetentionPolicy,
    now_ms: u64,
) -> StorageResult<GcReport>
where
    S: MutableGraphStore + Send + Sync,
{
    let snapshots = store.list_snapshots().await?;
    let mut report = GcReport {
        snapshots_examined: snapshots.len() as u64,
        snapshots_removed: 0,
        snapshots_retained: 0,
    };

    // Build the set of retained snapshot ids first, so that we can skip
    // deletion for anything that is retained.
    let retained_ids: BTreeSet<ObjectId> = snapshots
        .iter()
        .filter(|s| policy.is_retained(s, now_ms))
        .map(|s| s.id)
        .collect();

    for snapshot in &snapshots {
        if retained_ids.contains(&snapshot.id) {
            report.snapshots_retained += 1;
        } else {
            store.delete_snapshot(&snapshot.id).await?;
            report.snapshots_removed += 1;
        }
    }

    Ok(report)
}

// ── compact_snapshots ─────────────────────────────────────────────────────

/// Merge snapshots in `range` (start..=end indices into `list_snapshots` order)
/// into a single covering snapshot.
///
/// The covering snapshot:
/// - Uses the `graph_root_hash` of the last snapshot in the range (most recent
///   graph state).
/// - Sets `parent_id` to the parent of the first snapshot in the range.
/// - Sets `applied_change_id` to the `applied_change_id` of the last snapshot
///   in the range.
/// - Sets `created_at` to the `created_at` of the last snapshot in the range.
/// - Gets an `id` derived from the BLAKE3 hash of all collapsed snapshot ids
///   concatenated in order.
///
/// All originals in the range are deleted; the covering snapshot is saved.
///
/// # Parameters
/// - `store`       — a `MutableGraphStore`.
/// - `range_start` — inclusive start index into `list_snapshots` order.
/// - `range_end`   — inclusive end index into `list_snapshots` order.
///
/// # Errors
/// - `StorageError::NotFound` if the range is empty or indices are out of bounds.
/// - Propagates any other `StorageError`.
pub async fn compact_snapshots<S>(
    store: &S,
    range_start: usize,
    range_end: usize,
) -> StorageResult<CompactionReport>
where
    S: MutableGraphStore + Send + Sync,
{
    if range_start > range_end {
        return Err(StorageError::NotFound);
    }

    let mut all_snapshots = store.list_snapshots().await?;

    // Sort by created_at so that range indices are stable and time-ordered.
    // Ties are broken by the snapshot id bytes for full determinism.
    all_snapshots.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.as_bytes().cmp(b.id.as_bytes()))
    });

    if range_end >= all_snapshots.len() {
        return Err(StorageError::NotFound);
    }

    let range_snapshots: Vec<&SnapshotEnvelope> =
        all_snapshots[range_start..=range_end].iter().collect();

    if range_snapshots.is_empty() {
        return Err(StorageError::NotFound);
    }

    let first = range_snapshots[0];
    let last = *range_snapshots.last().expect("non-empty range");

    // Derive a stable id by hashing all collapsed ids in order.
    let mut id_bytes: Vec<u8> = Vec::with_capacity(range_snapshots.len() * 32);
    for snap in &range_snapshots {
        id_bytes.extend_from_slice(snap.id.as_bytes());
    }
    let covering_id = ObjectId::from_bytes(&id_bytes);

    // Preserve audit records from all collapsed snapshots.
    // Deduplicate while preserving order (BTreeSet for determinism).
    let mut audit_set: std::collections::BTreeSet<ObjectId> = std::collections::BTreeSet::new();
    for snap in &range_snapshots {
        for id in &snap.audit_record_ids {
            audit_set.insert(*id);
        }
    }
    let audit_record_ids: Vec<ObjectId> = audit_set.into_iter().collect();

    // Preserve migration metadata from all collapsed snapshots.
    let mut migration_set: std::collections::BTreeSet<ObjectId> = std::collections::BTreeSet::new();
    for snap in &range_snapshots {
        for id in &snap.migration_metadata_ids {
            migration_set.insert(*id);
        }
    }
    let migration_metadata_ids: Vec<ObjectId> = migration_set.into_iter().collect();

    let covering = SnapshotEnvelope {
        id: covering_id,
        graph_root_hash: last.graph_root_hash,
        parent_id: first.parent_id,
        applied_change_id: last.applied_change_id,
        created_at: last.created_at,
        verification_report_hash: last.verification_report_hash,
        audit_record_ids,
        migration_metadata_ids,
    };

    // Save covering snapshot first, then remove originals.
    store.save_snapshot(&covering).await?;

    for snap in &range_snapshots {
        store.delete_snapshot(&snap.id).await?;
    }

    Ok(CompactionReport {
        snapshots_merged: range_snapshots.len() as u64,
        covering_snapshot_id: covering_id,
    })
}

// ── MutableGraphStore impl for ObjectBackedGraphStore ─────────────────────

impl<S: ObjectStore + Send + Sync> MutableGraphStore for ObjectBackedGraphStore<S> {
    async fn delete_snapshot(&self, id: &ObjectId) -> StorageResult<()> {
        self.remove_snapshot_from_index(id);
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod gc_tests {
    use futures::executor::block_on;

    use super::*;
    use crate::backends::memory::MemoryObjectStore;
    use crate::object::{ObjectStore, RawObject};

    fn make_store_with_objects(payloads: &[&[u8]]) -> MemoryObjectStore {
        let store = MemoryObjectStore::new();
        block_on(async {
            for payload in payloads {
                store
                    .put(RawObject(payload.to_vec()))
                    .await
                    .expect("put must succeed");
            }
        });
        store
    }

    fn id_of(payload: &[u8]) -> ObjectId {
        ObjectId::from_bytes(payload)
    }

    // Spec: unreachable objects are deleted; reachable objects survive.
    #[test]
    fn run_gc_deletes_unreachable_objects() {
        let store = make_store_with_objects(&[b"keep-me", b"remove-me"]);
        let keep_id = id_of(b"keep-me");
        let reachable = BTreeSet::from([keep_id]);

        let report = block_on(run_gc(&store, &reachable)).expect("run_gc must succeed");

        assert_eq!(report.objects_examined, 2);
        assert_eq!(report.objects_deleted, 1);
        assert!(report.bytes_freed > 0, "bytes_freed must be positive");

        // Reachable object still present; unreachable object gone.
        block_on(async {
            assert!(
                store.exists(&keep_id).await.expect("exists"),
                "reachable object must survive GC"
            );
            let remove_id = id_of(b"remove-me");
            assert!(
                !store.exists(&remove_id).await.expect("exists"),
                "unreachable object must be deleted"
            );
        });
    }

    // Spec: all objects reachable → nothing deleted.
    #[test]
    fn run_gc_with_all_reachable_deletes_nothing() {
        let store = make_store_with_objects(&[b"obj-a", b"obj-b"]);
        let reachable = BTreeSet::from([id_of(b"obj-a"), id_of(b"obj-b")]);

        let report = block_on(run_gc(&store, &reachable)).expect("run_gc");

        assert_eq!(report.objects_examined, 2);
        assert_eq!(report.objects_deleted, 0);
        assert_eq!(report.bytes_freed, 0);
    }

    // Spec: empty store → empty report.
    #[test]
    fn run_gc_on_empty_store_returns_zero_counts() {
        let store = MemoryObjectStore::new();
        let reachable = BTreeSet::new();

        let report = block_on(run_gc(&store, &reachable)).expect("run_gc");

        assert_eq!(report.objects_examined, 0);
        assert_eq!(report.objects_deleted, 0);
        assert_eq!(report.bytes_freed, 0);
    }

    // Spec: bytes_freed matches the actual payload size of deleted objects.
    #[test]
    fn run_gc_bytes_freed_matches_payload_size() {
        let payload = b"exactly-fourteen"; // 16 bytes
        let store = make_store_with_objects(&[payload]);
        let reachable = BTreeSet::new(); // nothing is reachable

        let report = block_on(run_gc(&store, &reachable)).expect("run_gc");

        assert_eq!(report.objects_deleted, 1);
        assert_eq!(
            report.bytes_freed,
            payload.len() as u64,
            "bytes_freed must equal the payload size"
        );
    }
}

#[cfg(test)]
mod compaction_tests {
    use futures::executor::block_on;

    use super::*;
    use crate::backends::memory::MemoryObjectStore;
    use crate::graph::{GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
    use crate::object::ObjectId;

    fn make_snapshot(seed: &[u8], ts: u64) -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(seed);
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: ts,
            verification_report_hash: None,
            audit_record_ids: Vec::new(),
            migration_metadata_ids: Vec::new(),
        }
    }

    fn make_store() -> ObjectBackedGraphStore<MemoryObjectStore> {
        ObjectBackedGraphStore::new(MemoryObjectStore::new())
    }

    // Scenario: compact_snapshots produces a covering snapshot.
    //   GIVEN two snapshots in the store
    //   WHEN compact_snapshots(0..=1) is called
    //   THEN one covering snapshot replaces both originals
    #[test]
    fn compact_produces_covering_snapshot() {
        let store = make_store();
        let s1 = make_snapshot(b"snap-1", 1_000);
        let s2 = make_snapshot(b"snap-2", 2_000);

        block_on(async {
            store.save_snapshot(&s1).await.expect("save s1");
            store.save_snapshot(&s2).await.expect("save s2");

            let report = compact_snapshots(&store, 0, 1)
                .await
                .expect("compact must succeed");
            assert_eq!(report.snapshots_merged, 2);

            let list = store.list_snapshots().await.expect("list after compact");
            assert_eq!(
                list.len(),
                1,
                "originals must be replaced by covering snapshot"
            );
            assert_eq!(list[0].id, report.covering_snapshot_id);
        });
    }

    // Scenario: compaction preserves audit_record_ids from collapsed snapshots.
    //   GIVEN two snapshots each with audit_record_ids
    //   WHEN compact_snapshots collapses them
    //   THEN the covering snapshot holds the union of all audit_record_ids
    #[test]
    fn compact_preserves_audit_record_ids() {
        let store = make_store();
        let audit_a = ObjectId::from_bytes(b"approval-record-a");
        let audit_b = ObjectId::from_bytes(b"approval-record-b");

        let mut s1 = make_snapshot(b"snap-a", 1_000);
        s1.audit_record_ids = vec![audit_a];
        let mut s2 = make_snapshot(b"snap-b", 2_000);
        s2.audit_record_ids = vec![audit_b];

        block_on(async {
            store.save_snapshot(&s1).await.expect("save s1");
            store.save_snapshot(&s2).await.expect("save s2");

            compact_snapshots(&store, 0, 1)
                .await
                .expect("compact must succeed");

            let list = store.list_snapshots().await.expect("list");
            assert_eq!(list.len(), 1, "must have one covering snapshot");
            let covering = &list[0];
            assert!(
                covering.audit_record_ids.contains(&audit_a),
                "covering snapshot must preserve audit_a"
            );
            assert!(
                covering.audit_record_ids.contains(&audit_b),
                "covering snapshot must preserve audit_b"
            );
        });
    }

    // Scenario: compaction preserves migration_metadata_ids from collapsed snapshots.
    //   GIVEN two snapshots each with migration_metadata_ids
    //   WHEN compact_snapshots collapses them
    //   THEN the covering snapshot holds the union of all migration_metadata_ids
    #[test]
    fn compact_preserves_migration_metadata_ids() {
        let store = make_store();
        let mig_1 = ObjectId::from_bytes(b"migration-report-v1-v2");
        let mig_2 = ObjectId::from_bytes(b"migration-report-v2-v3");

        let mut s1 = make_snapshot(b"snap-m1", 1_000);
        s1.migration_metadata_ids = vec![mig_1];
        let mut s2 = make_snapshot(b"snap-m2", 2_000);
        s2.migration_metadata_ids = vec![mig_2];

        block_on(async {
            store.save_snapshot(&s1).await.expect("save s1");
            store.save_snapshot(&s2).await.expect("save s2");

            compact_snapshots(&store, 0, 1)
                .await
                .expect("compact must succeed");

            let list = store.list_snapshots().await.expect("list");
            let covering = &list[0];
            assert!(
                covering.migration_metadata_ids.contains(&mig_1),
                "must preserve mig_1"
            );
            assert!(
                covering.migration_metadata_ids.contains(&mig_2),
                "must preserve mig_2"
            );
        });
    }

    // TRIANGULATE: empty audit/migration ids on input → empty on covering snapshot.
    #[test]
    fn compact_with_empty_metadata_stays_empty() {
        let store = make_store();
        let s1 = make_snapshot(b"snap-empty-1", 1_000);
        let s2 = make_snapshot(b"snap-empty-2", 2_000);

        block_on(async {
            store.save_snapshot(&s1).await.expect("save s1");
            store.save_snapshot(&s2).await.expect("save s2");

            compact_snapshots(&store, 0, 1)
                .await
                .expect("compact must succeed");

            let list = store.list_snapshots().await.expect("list");
            let covering = &list[0];
            assert!(
                covering.audit_record_ids.is_empty(),
                "audit_record_ids must be empty when all inputs are empty"
            );
            assert!(
                covering.migration_metadata_ids.is_empty(),
                "migration_metadata_ids must be empty when all inputs are empty"
            );
        });
    }
}
