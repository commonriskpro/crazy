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

use crate::branch::BranchStore;
use crate::codec::{CborCodec, ContentCodec};
use crate::error::{StorageError, StorageResult};
use crate::graph::{GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
use crate::object::{ObjectId, ObjectStore, RawObject};
use crate::tag::TagStore;

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

// ── SnapshotHolds ─────────────────────────────────────────────────────────

/// Snapshot IDs that must survive GC regardless of `RetentionPolicy` rules.
///
/// Holds complement `RetentionPolicy`: the policy guards snapshots by
/// age/type heuristics; holds guard by explicit live reference — an active
/// branch head, an immutable tag lock, or an audit/legal hold.
///
/// Build holds via [`collect_branch_holds`] / [`collect_tag_holds`] from
/// their respective registries, merge with any explicit audit/legal holds,
/// then pass to [`gc_unreferenced`].
///
/// # Determinism
///
/// `Vec<ObjectId>` fields use sorted (or deduped) inputs; the
/// [`as_set`](SnapshotHolds::as_set) method converts to a `BTreeSet` for
/// O(log n) GC lookup in deterministic iteration order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapshotHolds {
    /// Snapshot IDs currently pointed to by active branch heads.
    ///
    /// A branch head snapshot must never be garbage-collected while the branch
    /// is live, even if the snapshot is older than `max_age_days`.
    pub branch_heads: Vec<ObjectId>,
    /// Snapshot IDs locked by immutable tags (release or named tags).
    ///
    /// Tags are append-only pointers; a tagged snapshot must survive for as
    /// long as the tag exists.
    pub tag_locks: Vec<ObjectId>,
    /// Snapshot IDs under an explicit audit or legal hold.
    ///
    /// Legal and compliance requirements may mandate that specific snapshots
    /// be retained beyond what the retention policy alone would protect.
    pub audit_holds: Vec<ObjectId>,
}

impl SnapshotHolds {
    /// Return `true` if `id` is protected by any hold category.
    ///
    /// # Performance
    ///
    /// Each call scans up to three `Vec`s linearly — `O(n)` per call where `n`
    /// is the total number of held IDs.  Do **not** call this in a loop over
    /// many snapshot IDs; instead call [`as_set`](SnapshotHolds::as_set) once
    /// and use the returned `BTreeSet` for `O(log n)` per-snapshot lookup.
    /// [`gc_unreferenced`] already follows this pattern internally.
    pub fn is_held(&self, id: &ObjectId) -> bool {
        self.branch_heads.contains(id)
            || self.tag_locks.contains(id)
            || self.audit_holds.contains(id)
    }

    /// Merge all held IDs into a `BTreeSet` for efficient O(log n) lookup.
    ///
    /// Called once per GC run; the set is used for every snapshot check in
    /// `gc_unreferenced`.
    pub fn as_set(&self) -> BTreeSet<ObjectId> {
        let mut set = BTreeSet::new();
        set.extend(self.branch_heads.iter().copied());
        set.extend(self.tag_locks.iter().copied());
        set.extend(self.audit_holds.iter().copied());
        set
    }
}

// ── collect_branch_holds ──────────────────────────────────────────────────

/// Collect the snapshot IDs currently pointed to by all active branch heads.
///
/// Call this before [`gc_unreferenced`] and populate
/// `SnapshotHolds::branch_heads` with the result to prevent GC from
/// deleting snapshots that are reachable from any live branch.
///
/// # HEAD-only constraint
///
/// This function returns **only** the HEAD snapshot of each branch — the
/// snapshot the branch pointer currently references.  Intermediate snapshots
/// in the branch's parent chain are **not** added to the hold set.  If the
/// retention policy does not independently protect those ancestors (e.g. via
/// `max_age_days` or `keep_tagged`), they may be GC'd while the branch is
/// live, breaking history traversal.
///
/// Use [`collect_branch_holds_with_ancestry`] when full parent-chain
/// protection is required.
///
/// # Errors
///
/// Propagates any [`StorageError`] from `branch_store.list_branches()`.
pub async fn collect_branch_holds<B>(branch_store: &B) -> StorageResult<Vec<ObjectId>>
where
    B: BranchStore + Send + Sync,
{
    let branches = branch_store.list_branches().await?;
    Ok(branches.into_iter().map(|b| b.target_snapshot_id).collect())
}

// ── collect_branch_holds_with_ancestry ───────────────────────────────────

/// Collect snapshot IDs for all active branch heads **and their full ancestor
/// chains**.
///
/// For each branch returned by `branch_store.list_branches()`, this function
/// traverses the `parent_id` chain in `graph_store` until a genesis snapshot
/// (`parent_id == None`) or an already-visited node is reached.  All visited
/// snapshot IDs — heads and every ancestor — are returned as a deduplicated,
/// sorted `Vec<ObjectId>`.
///
/// # When to prefer this over [`collect_branch_holds`]
///
/// [`collect_branch_holds`] protects only the HEAD snapshot of each branch.
/// If intermediate ancestors are not independently retained by policy (e.g.
/// `max_age_days` or `keep_tagged`), GC can delete them while the branch is
/// live, breaking history traversal.  Use this function when the full parent
/// chain of every active branch must survive GC.
///
/// # Cost
///
/// Makes one `list_snapshots` call to build an in-memory parent-link index,
/// then performs `O(depth)` map lookups per branch.  Total cost is
/// `O(total_snapshots)` — the same order as [`gc_unreferenced`] itself, which
/// also calls `list_snapshots`.
///
/// # Errors
///
/// Propagates any [`StorageError`] from `branch_store.list_branches()` or
/// `graph_store.list_snapshots()`.
pub async fn collect_branch_holds_with_ancestry<B, G>(
    branch_store: &B,
    graph_store: &G,
) -> StorageResult<Vec<ObjectId>>
where
    B: BranchStore + Send + Sync,
    G: GraphStore + Send + Sync,
{
    let branches = branch_store.list_branches().await?;
    if branches.is_empty() {
        return Ok(Vec::new());
    }

    // Build id → parent_id map from all snapshots.
    // Using HashMap for internal computation is fine — only serializable
    // types must avoid HashMap per the project's determinism contract.
    let all_snapshots = graph_store.list_snapshots().await?;
    let parent_map: std::collections::HashMap<ObjectId, Option<ObjectId>> = all_snapshots
        .into_iter()
        .map(|s| (s.id, s.parent_id))
        .collect();

    // Walk from each branch head up through the parent chain.
    // BTreeSet gives deterministic iteration order and O(log n) deduplication.
    let mut held: BTreeSet<ObjectId> = BTreeSet::new();
    for branch in branches {
        let mut current = Some(branch.target_snapshot_id);
        while let Some(id) = current {
            if !held.insert(id) {
                // Already visited — convergent chains share a common ancestor;
                // no need to re-walk the shared suffix.
                break;
            }
            // Flatten Option<&Option<ObjectId>> → Option<ObjectId>:
            //   - Not in map (snapshot compacted or not yet saved): stop.
            //   - parent_id == None (genesis): stop.
            //   - parent_id == Some(pid): advance to pid.
            current = parent_map.get(&id).copied().flatten();
        }
    }

    Ok(held.into_iter().collect())
}

// ── collect_tag_holds ─────────────────────────────────────────────────────

/// Collect the snapshot IDs locked by all tags in `tag_store`.
///
/// Call this before [`gc_unreferenced`] and populate
/// `SnapshotHolds::tag_locks` with the result to prevent GC from deleting
/// any snapshot that a tag still points to.
///
/// # Errors
///
/// Propagates any [`StorageError`] from `tag_store.list_tags()`.
pub async fn collect_tag_holds<T>(tag_store: &T) -> StorageResult<Vec<ObjectId>>
where
    T: TagStore + Send + Sync,
{
    let tags = tag_store.list_tags().await?;
    Ok(tags.into_iter().map(|t| t.snapshot_id).collect())
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

// ── EnumerableObjectStore ─────────────────────────────────────────────────

/// Read-only contract for loading and enumerating content-addressed objects.
///
/// Integrity verification can depend on this narrower interface instead of the
/// write/delete-capable object store traits.
pub trait EnumerableObjectStore {
    /// Retrieve the object identified by `id`, or `None` if absent.
    fn get(&self, id: &ObjectId) -> impl Future<Output = StorageResult<Option<RawObject>>> + Send;

    /// Return all `ObjectId`s currently present in the store.
    fn list_object_ids(&self) -> impl Future<Output = StorageResult<Vec<ObjectId>>> + Send;
}

// ── MutableObjectStore ────────────────────────────────────────────────────

/// Extension of `EnumerableObjectStore` that supports physical deletion of
/// content-addressed objects.
///
/// GC requires both enumeration and deletion, while read-only integrity checks
/// should depend on `EnumerableObjectStore` instead.
pub trait MutableObjectStore: ObjectStore + EnumerableObjectStore {
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

// ── collect_reachable_object_ids_for_snapshots ───────────────────────────

/// Collect the set of CAS `ObjectId`s that must be retained when running
/// [`run_gc`] on an `ObjectStore` shared with an [`ObjectBackedGraphStore`].
///
/// For each [`SnapshotEnvelope`] in `retained`, this function adds **two**
/// CAS object IDs to the returned set:
///
/// 1. `envelope.graph_root_hash` — the CAS id of the graph content root
///    object written by the graph layer before the snapshot was saved.
/// 2. The CAS id of the **serialized snapshot envelope itself** — the BLAKE3
///    hash of the CBOR bytes that [`GraphStore::save_snapshot`] writes to the
///    backing `ObjectStore`.
///
/// # Why both IDs are required
///
/// [`ObjectBackedGraphStore::save_snapshot`] encodes each [`SnapshotEnvelope`]
/// with [`CborCodec`] and stores the result as a `RawObject`.  The content-
/// addressed id of those bytes (`cas_id`) is what the internal
/// `snapshot_index` maps to for `load_snapshot` and `list_snapshots`.  That
/// `cas_id` is **distinct** from `envelope.graph_root_hash`.
///
/// If [`run_gc`] is called with a `reachable` set that contains only
/// `graph_root_hash` values, the stored envelope bytes are treated as
/// unreachable and deleted.  The snapshot index still references those CAS
/// ids, so subsequent calls to `list_snapshots` return
/// [`StorageError::NotFound`] — a silent data-corruption scenario with no
/// error at GC time.
///
/// # Encoding contract
///
/// The envelope CAS id is recomputed by encoding each [`SnapshotEnvelope`]
/// with the same [`CborCodec`] used by
/// [`ObjectBackedGraphStore::save_snapshot`].  This is correct as long as:
/// - The codec is deterministic (guaranteed by [`CborCodec`]'s invariants).
/// - The envelope struct has not been mutated between `save_snapshot` and
///   the call to this function.  Always load envelopes from the live store
///   immediately before calling this function.
///
/// # Usage
///
/// Call this after [`gc_unreferenced`] has removed unreachable *snapshots*,
/// then pass the result as `reachable` to [`run_gc`]:
///
/// ```ignore
/// // 1. Remove unreachable snapshot index entries.
/// gc_unreferenced(&graph_store, &policy, &holds, now_ms).await?;
/// // 2. Enumerate the retained snapshots.
/// let retained = graph_store.list_snapshots().await?;
/// // 3. Build the CAS reachable set (graph roots + envelope bytes).
/// let reachable = collect_reachable_object_ids_for_snapshots(&retained)?;
/// // 4. Delete unreachable raw CAS objects.
/// run_gc(&object_store, &reachable).await?;
/// ```
///
/// # Errors
///
/// Returns [`StorageError::Codec`] if CBOR encoding fails.  This should not
/// occur for well-formed [`SnapshotEnvelope`] values.
pub fn collect_reachable_object_ids_for_snapshots(
    retained: &[SnapshotEnvelope],
) -> StorageResult<BTreeSet<ObjectId>> {
    let codec = CborCodec;
    let mut reachable = BTreeSet::new();
    for envelope in retained {
        // The CAS id of the CBOR-encoded envelope bytes written by save_snapshot.
        let bytes = codec.encode(envelope)?;
        reachable.insert(ObjectId::from_bytes(&bytes));
        // The graph content root referenced by this snapshot.
        reachable.insert(envelope.graph_root_hash);
    }
    Ok(reachable)
}

// ── run_gc ────────────────────────────────────────────────────────────────

/// Physical garbage-collect unreachable CAS objects.
///
/// # Parameters
/// - `store`     — a `MutableObjectStore` (list + delete).
/// - `reachable` — the set of `ObjectId`s that must be kept.  Objects NOT in
///   this set are considered unreachable and will be deleted.
///
/// # Returns
/// An `ObjectGcReport` counting examined, deleted, and bytes freed.
///
/// # Design
///
/// The caller is responsible for computing `reachable` from the graph store.
/// Call [`gc_unreferenced`] first to remove unreachable snapshot index entries,
/// then call [`collect_reachable_object_ids_for_snapshots`] on the surviving
/// snapshots to build the `reachable` set — that function adds **both** the
/// `graph_root_hash` and the envelope CAS id for every retained snapshot,
/// preventing the silent data-corruption that occurs when only `graph_root_hash`
/// fields are collected.  This separation keeps `run_gc` independent of the
/// snapshot/graph layer and fully testable with a plain `MemoryObjectStore`.
///
/// # Warning: include snapshot envelope CAS IDs
///
/// When `store` is the same `ObjectStore` that backs an
/// `ObjectBackedGraphStore`, the `reachable` set must include **both**:
/// - The `graph_root_hash` of each retained snapshot (graph content objects).
/// - The CAS `ObjectId` of each retained snapshot *envelope* — the CBOR
///   bytes written to the `ObjectStore` by `GraphStore::save_snapshot`.
///
/// Omitting the envelope CAS IDs causes `run_gc` to treat the serialized
/// snapshot envelopes as unreachable and delete them, corrupting the store
/// even though the logical snapshot index still references those objects.
/// To obtain an envelope CAS ID, re-encode the `SnapshotEnvelope` with the
/// same codec used by the graph store (`CborCodec`) and hash the result with
/// `ObjectId::from_bytes`.
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

/// Identify and remove snapshots that are not retained by `policy` or `holds`.
///
/// A snapshot survives GC when **either** of these conditions holds:
/// - `policy.is_retained(snapshot, now_ms)` returns `true` (age/type rules), or
/// - `holds.is_held(&snapshot.id)` returns `true` (branch head, tag lock, or
///   audit/legal hold).
///
/// This two-layer check ensures that live branch heads and tag-locked
/// snapshots are never collected even if the retention policy alone would
/// not protect them (e.g. an old snapshot pointed to by an active branch).
///
/// # Parameters
/// - `store`  — a `MutableGraphStore` (supports both read and delete).
/// - `policy` — the retention policy to apply.
/// - `holds`  — explicit holds from branch heads, tags, and audit requirements.
///   Pass `&SnapshotHolds::default()` when no holds are registered.
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
    holds: &SnapshotHolds,
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

    // Materialise holds into a BTreeSet once for O(log n) per-snapshot lookup.
    let hold_set = holds.as_set();

    // Build the set of retained snapshot ids first, so that we can skip
    // deletion for anything that is retained by policy or by a hold.
    let retained_ids: BTreeSet<ObjectId> = snapshots
        .iter()
        .filter(|s| policy.is_retained(s, now_ms) || hold_set.contains(&s.id))
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
