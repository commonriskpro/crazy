pub(super) use std::collections::BTreeSet;

pub(super) use ail_storage::{
    BranchRegistry, BranchStore, EnumerableObjectStore, GraphStore, ObjectBackedGraphStore,
    SnapshotEnvelope, TagRegistry, TagStore,
    backends::memory::MemoryObjectStore,
    object::ObjectId,
    retention::{
        GcReport, RetentionPolicy, SnapshotHolds, collect_branch_holds,
        collect_branch_holds_with_ancestry, collect_reachable_object_ids_for_snapshots,
        collect_tag_holds, compact_snapshots, gc_unreferenced, run_gc,
    },
};
pub(super) use futures::executor::block_on;

// ── helpers ───────────────────────────────────────────────────────────────

/// Deterministic `ObjectId` from a short label.
pub(super) fn oid(label: &str) -> ObjectId {
    ObjectId::from_bytes(label.as_bytes())
}

/// Build an `ObjectBackedGraphStore` backed by a fresh `MemoryObjectStore`.
pub(super) fn make_store() -> ObjectBackedGraphStore<MemoryObjectStore> {
    ObjectBackedGraphStore::new(MemoryObjectStore::new())
}

/// Build a snapshot at `created_at` with optional parent and change.
pub(super) fn snapshot(
    label: &str,
    created_at: u64,
    parent: Option<&str>,
    change: Option<&str>,
) -> SnapshotEnvelope {
    SnapshotEnvelope {
        id: oid(label),
        graph_root_hash: oid(&format!("root-{label}")),
        parent_id: parent.map(oid),
        applied_change_id: change.map(oid),
        created_at,
        verification_report_hash: None,
        ..Default::default()
    }
}

/// Save a slice of snapshots to `store` in order.
pub(super) fn save_all(
    store: &ObjectBackedGraphStore<MemoryObjectStore>,
    snaps: &[SnapshotEnvelope],
) {
    for s in snaps {
        block_on(store.save_snapshot(s)).expect("save must succeed");
    }
}
