// Integration tests for retention policies, GC, and snapshot compaction.
//
// All tests use `MemoryObjectStore` (deterministic, no DB needed).
// Time is injected as `now_ms` so tests are deterministic and instant.
//
// Test layout
// ──────────────────────────────────────────────────────────────────────────
// RetentionPolicy::is_retained
//   retention_keeps_genesis_when_keep_releases_true
//   retention_releases_genesis_when_keep_releases_false
//   retention_keeps_tagged_when_keep_tagged_true
//   retention_releases_tagged_when_keep_tagged_false
//   retention_keeps_young_snapshot_within_max_age
//   retention_removes_old_snapshot_beyond_max_age
//   retention_none_max_age_does_not_protect_by_age
//
// gc_unreferenced
//   gc_empty_store_produces_zero_report
//   gc_retains_all_when_policy_keeps_all
//   gc_removes_all_when_policy_keeps_nothing
//   gc_partial_retention_removes_only_unreferenced
//   gc_report_counts_are_consistent
//
// compact_snapshots
//   compact_empty_range_returns_error
//   compact_out_of_bounds_returns_error
//   compact_single_snapshot_range
//   compact_multiple_snapshots_produces_covering
//   compact_covering_has_correct_graph_root_hash
//   compact_covering_has_correct_parent_id
//   compact_merged_count_matches_range
//   compact_originals_are_removed

use ail_storage::{
    GraphStore, ObjectBackedGraphStore, SnapshotEnvelope,
    backends::memory::MemoryObjectStore,
    object::ObjectId,
    retention::{GcReport, RetentionPolicy, compact_snapshots, gc_unreferenced},
};
use futures::executor::block_on;

// ── helpers ───────────────────────────────────────────────────────────────

/// Deterministic `ObjectId` from a short label.
fn oid(label: &str) -> ObjectId {
    ObjectId::from_bytes(label.as_bytes())
}

/// Build an `ObjectBackedGraphStore` backed by a fresh `MemoryObjectStore`.
fn make_store() -> ObjectBackedGraphStore<MemoryObjectStore> {
    ObjectBackedGraphStore::new(MemoryObjectStore::new())
}

/// Build a snapshot at `created_at` with optional parent and change.
fn snapshot(
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
    }
}

/// Save a slice of snapshots to `store` in order.
fn save_all(store: &ObjectBackedGraphStore<MemoryObjectStore>, snaps: &[SnapshotEnvelope]) {
    for s in snaps {
        block_on(store.save_snapshot(s)).expect("save must succeed");
    }
}

// ── RetentionPolicy::is_retained ──────────────────────────────────────────

// GIVEN keep_releases = true
// WHEN snapshot has parent_id = None (genesis)
// THEN is_retained returns true
#[test]
fn retention_keeps_genesis_when_keep_releases_true() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: true,
        keep_tagged: false,
    };
    let snap = snapshot("genesis", 0, None, None);
    assert!(
        policy.is_retained(&snap, 1_000_000_000_000),
        "genesis must be retained when keep_releases is true"
    );
}

// GIVEN keep_releases = false
// WHEN snapshot has parent_id = None (genesis)
// THEN is_retained returns false (no other rule applies)
#[test]
fn retention_releases_genesis_when_keep_releases_false() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let snap = snapshot("old-genesis", 0, None, None);
    assert!(
        !policy.is_retained(&snap, 1_000_000_000_000),
        "genesis must not be retained when keep_releases is false and no age rule applies"
    );
}

// GIVEN keep_tagged = true
// WHEN snapshot has applied_change_id = Some(_)
// THEN is_retained returns true
#[test]
fn retention_keeps_tagged_when_keep_tagged_true() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: true,
    };
    let snap = snapshot("tagged", 0, Some("parent"), Some("change-42"));
    assert!(
        policy.is_retained(&snap, 1_000_000_000_000),
        "snapshot with applied_change_id must be retained when keep_tagged is true"
    );
}

// GIVEN keep_tagged = false
// WHEN snapshot has applied_change_id = Some(_)
// THEN is_retained returns false (no other rule applies)
#[test]
fn retention_releases_tagged_when_keep_tagged_false() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let snap = snapshot("tagged-old", 0, Some("parent"), Some("change-42"));
    assert!(
        !policy.is_retained(&snap, 1_000_000_000_000),
        "snapshot must not be retained when keep_tagged is false and no age rule applies"
    );
}

// GIVEN max_age_days = Some(30)  and  now = 30 days since created_at
// WHEN snapshot created_at == now - 25 days (within 30 days)
// THEN is_retained returns true
#[test]
fn retention_keeps_young_snapshot_within_max_age() {
    let now_ms: u64 = 30 * 86_400_000; // 30 days in ms
    let policy = RetentionPolicy {
        max_age_days: Some(30),
        keep_releases: false,
        keep_tagged: false,
    };
    // Created 25 days ago — younger than 30 days
    let created_at = now_ms - 25 * 86_400_000;
    let snap = snapshot("young", created_at, Some("p"), None);
    assert!(
        policy.is_retained(&snap, now_ms),
        "snapshot younger than max_age_days must be retained"
    );
}

// GIVEN max_age_days = Some(30)
// WHEN snapshot created_at is older than 30 days
// THEN is_retained returns false
#[test]
fn retention_removes_old_snapshot_beyond_max_age() {
    let now_ms: u64 = 60 * 86_400_000; // 60 days in ms
    let policy = RetentionPolicy {
        max_age_days: Some(30),
        keep_releases: false,
        keep_tagged: false,
    };
    // Created 45 days ago — older than 30-day window
    let created_at = now_ms - 45 * 86_400_000;
    let snap = snapshot("old", created_at, Some("p"), None);
    assert!(
        !policy.is_retained(&snap, now_ms),
        "snapshot older than max_age_days must not be retained"
    );
}

// GIVEN max_age_days = None
// WHEN snapshot has any created_at
// THEN age rule does not protect it (returns false if no other rule matches)
#[test]
fn retention_none_max_age_does_not_protect_by_age() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let snap = snapshot("recent", 999_999_999_999, Some("p"), None);
    assert!(
        !policy.is_retained(&snap, 1_000_000_000_000),
        "None max_age_days must not protect a snapshot by age alone"
    );
}

// ── gc_unreferenced ───────────────────────────────────────────────────────

// GIVEN an empty store
// WHEN gc_unreferenced is called
// THEN report has all zero counts
#[test]
fn gc_empty_store_produces_zero_report() {
    let store = make_store();
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: true,
        keep_tagged: true,
    };
    let report: GcReport = block_on(gc_unreferenced(&store, &policy, 0)).expect("gc must succeed");
    assert_eq!(report.snapshots_examined, 0);
    assert_eq!(report.snapshots_retained, 0);
    assert_eq!(report.snapshots_removed, 0);
}

// GIVEN two snapshots both protected by policy
// WHEN gc_unreferenced is called
// THEN both are retained, none removed
#[test]
fn gc_retains_all_when_policy_keeps_all() {
    let store = make_store();
    let snaps = vec![
        snapshot("s1", 0, None, None), // genesis → retained by keep_releases
        snapshot("s2", 1, None, None), // also genesis
    ];
    save_all(&store, &snaps);

    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: true,
        keep_tagged: false,
    };
    let report = block_on(gc_unreferenced(&store, &policy, 0)).expect("gc must succeed");
    assert_eq!(report.snapshots_examined, 2);
    assert_eq!(report.snapshots_retained, 2);
    assert_eq!(report.snapshots_removed, 0);

    // Snapshots still present after GC.
    let after = block_on(store.list_snapshots()).expect("list must succeed");
    assert_eq!(after.len(), 2);
}

// GIVEN two snapshots both unprotected by policy
// WHEN gc_unreferenced is called
// THEN both are removed, none retained
#[test]
fn gc_removes_all_when_policy_keeps_nothing() {
    let store = make_store();
    let snaps = vec![
        snapshot("r1", 0, Some("parent"), None), // not genesis, no change, no age
        snapshot("r2", 1, Some("parent"), None),
    ];
    save_all(&store, &snaps);

    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let report = block_on(gc_unreferenced(&store, &policy, 0)).expect("gc must succeed");
    assert_eq!(report.snapshots_examined, 2);
    assert_eq!(report.snapshots_retained, 0);
    assert_eq!(report.snapshots_removed, 2);

    // Store is empty after GC.
    let after = block_on(store.list_snapshots()).expect("list must succeed");
    assert!(after.is_empty(), "all snapshots must be removed");
}

// GIVEN three snapshots: one tagged (kept), two untagged (removed)
// WHEN gc_unreferenced is called with keep_tagged = true
// THEN exactly one retained, two removed
#[test]
fn gc_partial_retention_removes_only_unreferenced() {
    let store = make_store();
    let snaps = vec![
        snapshot("keep-me", 0, Some("p"), Some("change-1")), // tagged → retained
        snapshot("drop-1", 0, Some("p"), None),
        snapshot("drop-2", 0, Some("p"), None),
    ];
    save_all(&store, &snaps);

    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: true,
    };
    let report = block_on(gc_unreferenced(&store, &policy, 0)).expect("gc must succeed");
    assert_eq!(report.snapshots_examined, 3);
    assert_eq!(report.snapshots_retained, 1);
    assert_eq!(report.snapshots_removed, 2);

    // Only the tagged snapshot survives.
    let after = block_on(store.list_snapshots()).expect("list must succeed");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, oid("keep-me"));
}

// GIVEN any store state
// WHEN gc runs
// THEN retained + removed == examined
#[test]
fn gc_report_counts_are_consistent() {
    let store = make_store();
    let snaps = vec![
        snapshot("a", 0, None, Some("c")),       // genesis + tagged
        snapshot("b", 0, Some("p"), None),       // neither
        snapshot("c", 0, Some("p"), Some("c2")), // tagged only
    ];
    save_all(&store, &snaps);

    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: true,
        keep_tagged: true,
    };
    let report = block_on(gc_unreferenced(&store, &policy, 0)).expect("gc must succeed");
    assert_eq!(
        report.snapshots_retained + report.snapshots_removed,
        report.snapshots_examined,
        "retained + removed must equal examined"
    );
}

// ── compact_snapshots ─────────────────────────────────────────────────────

// GIVEN range_start > range_end
// WHEN compact_snapshots is called
// THEN StorageError::NotFound is returned
#[test]
fn compact_empty_range_returns_error() {
    let store = make_store();
    let s = snapshot("x", 0, None, None);
    save_all(&store, &[s]);

    let result = block_on(compact_snapshots(&store, 2, 1));
    assert!(result.is_err(), "reversed range must return an error");
}

// GIVEN range_end >= number of snapshots
// WHEN compact_snapshots is called
// THEN StorageError::NotFound is returned
#[test]
fn compact_out_of_bounds_returns_error() {
    let store = make_store();
    let s = snapshot("only", 0, None, None);
    save_all(&store, &[s]);

    let result = block_on(compact_snapshots(&store, 0, 5));
    assert!(result.is_err(), "out-of-bounds range must return an error");
}

// GIVEN a store with exactly one snapshot, range [0..=0]
// WHEN compact_snapshots is called
// THEN report.snapshots_merged == 1 and a covering snapshot is saved
#[test]
fn compact_single_snapshot_range() {
    let store = make_store();
    let s = snapshot("solo", 100, None, Some("ch1"));
    save_all(&store, &[s.clone()]);

    let report = block_on(compact_snapshots(&store, 0, 0)).expect("compact must succeed");
    assert_eq!(report.snapshots_merged, 1);

    // Covering snapshot must exist.
    let loaded = block_on(store.load_snapshot(&report.covering_snapshot_id))
        .expect("load must succeed")
        .expect("covering snapshot must be present");
    assert_eq!(loaded.graph_root_hash, s.graph_root_hash);
}

// GIVEN three snapshots [s0, s1, s2], range [0..=2]
// WHEN compact_snapshots is called
// THEN report.snapshots_merged == 3 and covering snapshot exists
#[test]
fn compact_multiple_snapshots_produces_covering() {
    let store = make_store();
    let snaps = vec![
        snapshot("c0", 1000, None, None),
        snapshot("c1", 2000, Some("c0"), Some("ch1")),
        snapshot("c2", 3000, Some("c1"), Some("ch2")),
    ];
    save_all(&store, &snaps);

    let report = block_on(compact_snapshots(&store, 0, 2)).expect("compact must succeed");
    assert_eq!(report.snapshots_merged, 3);

    // Covering snapshot must exist.
    let loaded = block_on(store.load_snapshot(&report.covering_snapshot_id))
        .expect("load must succeed")
        .expect("covering snapshot must be present");

    // graph_root_hash must be from the last snapshot in the range.
    assert_eq!(loaded.graph_root_hash, oid("root-c2"));
}

// GIVEN three snapshots [s0, s1, s2], range [0..=2]
// WHEN compact_snapshots is called
// THEN covering snapshot's graph_root_hash equals last snapshot's graph_root_hash
#[test]
fn compact_covering_has_correct_graph_root_hash() {
    let store = make_store();
    let snaps = vec![
        snapshot("h0", 100, None, None),
        snapshot("h1", 200, Some("h0"), None),
        snapshot("h2", 300, Some("h1"), Some("c")),
    ];
    save_all(&store, &snaps);

    let report = block_on(compact_snapshots(&store, 0, 2)).expect("compact must succeed");
    let loaded = block_on(store.load_snapshot(&report.covering_snapshot_id))
        .expect("load")
        .expect("covering must exist");

    assert_eq!(
        loaded.graph_root_hash,
        oid("root-h2"),
        "graph_root_hash must come from the last snapshot in range"
    );
}

// GIVEN three snapshots [s0, s1, s2] where s0 is genesis (parent_id = None),
//   range [0..=2]
// WHEN compact_snapshots is called
// THEN covering snapshot's parent_id equals first snapshot's parent_id (None)
#[test]
fn compact_covering_has_correct_parent_id() {
    let store = make_store();
    let snaps = vec![
        snapshot("p0", 1, None, None), // genesis
        snapshot("p1", 2, Some("p0"), None),
        snapshot("p2", 3, Some("p1"), None),
    ];
    save_all(&store, &snaps);

    let report = block_on(compact_snapshots(&store, 0, 2)).expect("compact must succeed");
    let loaded = block_on(store.load_snapshot(&report.covering_snapshot_id))
        .expect("load")
        .expect("covering must exist");

    assert!(
        loaded.parent_id.is_none(),
        "covering must inherit genesis parent_id (None) from first snapshot"
    );
}

// GIVEN n snapshots in the range
// WHEN compact_snapshots is called
// THEN report.snapshots_merged == n
#[test]
fn compact_merged_count_matches_range() {
    let store = make_store();
    let snaps: Vec<SnapshotEnvelope> = (0u64..5)
        .map(|i| snapshot(&format!("m{i}"), i * 1000, Some("base"), None))
        .collect();
    save_all(&store, &snaps);

    let report = block_on(compact_snapshots(&store, 1, 3)).expect("compact must succeed");
    assert_eq!(
        report.snapshots_merged, 3,
        "merging range [1..=3] must produce snapshots_merged == 3"
    );
}

// GIVEN three snapshots [s0, s1, s2], range [0..=2]
// WHEN compact_snapshots is called
// THEN s0, s1, s2 are removed from the store
#[test]
fn compact_originals_are_removed() {
    let store = make_store();
    let snaps = vec![
        snapshot("orig0", 1, None, None),
        snapshot("orig1", 2, Some("orig0"), None),
        snapshot("orig2", 3, Some("orig1"), None),
    ];
    save_all(&store, &snaps);

    let report = block_on(compact_snapshots(&store, 0, 2)).expect("compact must succeed");

    // Originals must be gone.
    for label in ["orig0", "orig1", "orig2"] {
        let found = block_on(store.load_snapshot(&oid(label))).expect("load must not error");
        assert!(
            found.is_none(),
            "original snapshot '{label}' must be removed after compaction"
        );
    }

    // Only the covering snapshot remains.
    let all = block_on(store.list_snapshots()).expect("list must succeed");
    assert_eq!(all.len(), 1, "only the covering snapshot must remain");
    assert_eq!(all[0].id, report.covering_snapshot_id);
}
