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

    let covering = SnapshotEnvelope {
        id: covering_id,
        graph_root_hash: last.graph_root_hash,
        parent_id: first.parent_id,
        applied_change_id: last.applied_change_id,
        created_at: last.created_at,
        verification_report_hash: last.verification_report_hash,
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
