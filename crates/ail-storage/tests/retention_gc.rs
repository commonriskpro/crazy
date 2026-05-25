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

use std::collections::BTreeSet;

use ail_storage::{
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
        ..Default::default()
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
    let report: GcReport = block_on(gc_unreferenced(
        &store,
        &policy,
        &SnapshotHolds::default(),
        0,
    ))
    .expect("gc must succeed");
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
    let report = block_on(gc_unreferenced(
        &store,
        &policy,
        &SnapshotHolds::default(),
        0,
    ))
    .expect("gc must succeed");
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
    let report = block_on(gc_unreferenced(
        &store,
        &policy,
        &SnapshotHolds::default(),
        0,
    ))
    .expect("gc must succeed");
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
    let report = block_on(gc_unreferenced(
        &store,
        &policy,
        &SnapshotHolds::default(),
        0,
    ))
    .expect("gc must succeed");
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
    let report = block_on(gc_unreferenced(
        &store,
        &policy,
        &SnapshotHolds::default(),
        0,
    ))
    .expect("gc must succeed");
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
    save_all(&store, std::slice::from_ref(&s));

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

// ── SnapshotHolds: branch-head protection ────────────────────────────────

// Scenario: GC must not delete a snapshot pointed to by an active branch head.
//   GIVEN a snapshot that would be collected by policy alone (old, untagged, non-genesis)
//   AND   a branch whose head points to that snapshot
//   WHEN  gc_unreferenced is called with branch holds
//   THEN  the snapshot survives GC
#[test]
fn gc_branch_head_survives_gc() {
    let store = make_store();
    let old_snap = snapshot("branch-head", 0, Some("parent"), None); // policy alone would remove it
    save_all(&store, std::slice::from_ref(&old_snap));

    let holds = SnapshotHolds {
        branch_heads: vec![oid("branch-head")],
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    let report =
        block_on(gc_unreferenced(&store, &policy, &holds, u64::MAX)).expect("gc must succeed");

    assert_eq!(report.snapshots_examined, 1);
    assert_eq!(
        report.snapshots_retained, 1,
        "branch-head snapshot must be held"
    );
    assert_eq!(report.snapshots_removed, 0);

    // Snapshot must still be loadable.
    let loaded = block_on(store.load_snapshot(&oid("branch-head"))).expect("load must succeed");
    assert!(loaded.is_some(), "branch-head snapshot must survive GC");
}

// Scenario: after branch is deleted (hold cleared), next GC removes the snapshot.
//   GIVEN a snapshot held by a branch head
//   AND   a second GC run with an empty hold set
//   THEN  the snapshot is collected
#[test]
fn gc_cleared_branch_hold_allows_collection() {
    let store = make_store();
    let snap = snapshot("stale-head", 0, Some("p"), None);
    save_all(&store, &[snap]);

    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    // First GC: snapshot held by branch — survives.
    let held = SnapshotHolds {
        branch_heads: vec![oid("stale-head")],
        ..Default::default()
    };
    block_on(gc_unreferenced(&store, &policy, &held, 0)).expect("gc round 1");
    let after_held = block_on(store.list_snapshots()).expect("list");
    assert_eq!(after_held.len(), 1, "snapshot must survive while held");

    // Second GC: branch deleted, hold cleared — snapshot is collected.
    let empty_holds = SnapshotHolds::default();
    block_on(gc_unreferenced(&store, &policy, &empty_holds, 0)).expect("gc round 2");
    let after_cleared = block_on(store.list_snapshots()).expect("list after cleared");
    assert!(
        after_cleared.is_empty(),
        "snapshot must be collected once branch hold is removed"
    );
}

// ── SnapshotHolds: tag-lock protection ───────────────────────────────────

// Scenario: GC must not delete a snapshot locked by a tag.
//   GIVEN an unprotected snapshot (not genesis, no change, old)
//   AND   a tag pointing to that snapshot
//   WHEN  gc_unreferenced is called with tag holds
//   THEN  the snapshot survives
#[test]
fn gc_tag_locked_snapshot_survives_gc() {
    let store = make_store();
    let snap = snapshot("tagged-snap", 0, Some("p"), None);
    save_all(&store, &[snap]);

    let holds = SnapshotHolds {
        tag_locks: vec![oid("tagged-snap")],
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    let report =
        block_on(gc_unreferenced(&store, &policy, &holds, u64::MAX)).expect("gc must succeed");

    assert_eq!(
        report.snapshots_retained, 1,
        "tag-locked snapshot must survive GC"
    );
    assert_eq!(report.snapshots_removed, 0);
}

// ── SnapshotHolds: audit hold protection ─────────────────────────────────

// Scenario: GC must not delete a snapshot under an explicit audit hold.
//   GIVEN a snapshot not protected by age, genesis, or tagged rules
//   AND   the snapshot is listed in audit_holds
//   WHEN  gc_unreferenced runs
//   THEN  the snapshot is retained
#[test]
fn gc_audit_hold_snapshot_survives_gc() {
    let store = make_store();
    let snap = snapshot("audit-hold", 0, Some("p"), None);
    save_all(&store, &[snap]);

    let holds = SnapshotHolds {
        audit_holds: vec![oid("audit-hold")],
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    let report =
        block_on(gc_unreferenced(&store, &policy, &holds, u64::MAX)).expect("gc must succeed");

    assert_eq!(
        report.snapshots_retained, 1,
        "audit-held snapshot must survive GC"
    );
    assert_eq!(report.snapshots_removed, 0);
}

// Scenario: only the held snapshot survives; unheld eligible snapshots are removed.
//   GIVEN three snapshots — one held by audit, two unprotected
//   WHEN  gc_unreferenced runs
//   THEN  exactly one retained (the held one), two removed
#[test]
fn gc_only_held_snapshot_survives_among_eligible() {
    let store = make_store();
    let snaps = vec![
        snapshot("hold-me", 0, Some("p"), None), // audit hold → must survive
        snapshot("drop-a", 0, Some("p"), None),  // no hold, no policy → removed
        snapshot("drop-b", 0, Some("p"), None),  // no hold, no policy → removed
    ];
    save_all(&store, &snaps);

    let holds = SnapshotHolds {
        audit_holds: vec![oid("hold-me")],
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    let report =
        block_on(gc_unreferenced(&store, &policy, &holds, u64::MAX)).expect("gc must succeed");

    assert_eq!(report.snapshots_examined, 3);
    assert_eq!(report.snapshots_retained, 1);
    assert_eq!(report.snapshots_removed, 2);

    let survivors = block_on(store.list_snapshots()).expect("list");
    assert_eq!(survivors.len(), 1);
    assert_eq!(
        survivors[0].id,
        oid("hold-me"),
        "only the held snapshot must survive"
    );
}

// ── collect_branch_holds ──────────────────────────────────────────────────

// Scenario: collect_branch_holds returns the target_snapshot_id of each branch.
//   GIVEN two branches with distinct target_snapshot_ids
//   WHEN  collect_branch_holds is called
//   THEN  the result contains exactly those two snapshot ids
#[test]
fn collect_branch_holds_returns_target_ids() {
    let reg = BranchRegistry::new();
    block_on(async {
        reg.create_branch("main", oid("snap-main"), 100)
            .await
            .expect("create main");
        reg.create_branch("dev", oid("snap-dev"), 200)
            .await
            .expect("create dev");
    });

    let ids = block_on(collect_branch_holds(&reg)).expect("collect must succeed");
    assert_eq!(ids.len(), 2, "must return one id per branch");
    assert!(
        ids.contains(&oid("snap-main")),
        "main branch id must be present"
    );
    assert!(
        ids.contains(&oid("snap-dev")),
        "dev branch id must be present"
    );
}

// Scenario: collect_branch_holds on empty registry returns empty vec.
#[test]
fn collect_branch_holds_empty_registry_returns_empty() {
    let reg = BranchRegistry::new();
    let ids = block_on(collect_branch_holds(&reg)).expect("collect must succeed");
    assert!(ids.is_empty(), "empty registry must yield empty holds");
}

// ── collect_tag_holds ─────────────────────────────────────────────────────

// Scenario: collect_tag_holds returns the snapshot_id of each tag.
//   GIVEN two tags pointing to distinct snapshots
//   WHEN  collect_tag_holds is called
//   THEN  the result contains exactly those two snapshot ids
#[test]
fn collect_tag_holds_returns_snapshot_ids() {
    let reg = TagRegistry::new();
    block_on(async {
        reg.create_tag("v1.0", oid("snap-v1"), 100, None)
            .await
            .expect("create v1.0");
        reg.create_tag("v2.0", oid("snap-v2"), 200, None)
            .await
            .expect("create v2.0");
    });

    let ids = block_on(collect_tag_holds(&reg)).expect("collect must succeed");
    assert_eq!(ids.len(), 2, "must return one id per tag");
    assert!(
        ids.contains(&oid("snap-v1")),
        "v1.0 snapshot id must be present"
    );
    assert!(
        ids.contains(&oid("snap-v2")),
        "v2.0 snapshot id must be present"
    );
}

// Scenario: collect_tag_holds on empty registry returns empty vec.
#[test]
fn collect_tag_holds_empty_registry_returns_empty() {
    let reg = TagRegistry::new();
    let ids = block_on(collect_tag_holds(&reg)).expect("collect must succeed");
    assert!(ids.is_empty(), "empty tag registry must yield empty holds");
}

// ── End-to-end: branch/tag holds wired with collect helpers ──────────────

// Scenario: full pipeline — branch and tag holds collected then applied to GC.
//   GIVEN a store with three snapshots
//   AND   one snapshot pointed to by a branch head
//   AND   one snapshot pointed to by a tag
//   AND   one snapshot with no holds and no policy protection
//   WHEN  holds are collected via collect_branch_holds + collect_tag_holds
//   AND   gc_unreferenced is called
//   THEN  the two held snapshots survive; the unprotected one is removed
#[test]
fn gc_end_to_end_branch_and_tag_holds_protect_snapshots() {
    let graph_store = make_store();
    let branch_reg = BranchRegistry::new();
    let tag_reg = TagRegistry::new();

    let snaps = vec![
        snapshot("head-snap", 0, Some("p"), None), // held by branch
        snapshot("release-snap", 0, Some("p"), None), // held by tag
        snapshot("garbage", 0, Some("p"), None),   // no protection → removed
    ];
    save_all(&graph_store, &snaps);

    block_on(async {
        branch_reg
            .create_branch("main", oid("head-snap"), 1)
            .await
            .expect("create branch");
        tag_reg
            .create_tag("v1.0", oid("release-snap"), 2, None)
            .await
            .expect("create tag");
    });

    let branch_ids = block_on(collect_branch_holds(&branch_reg)).expect("branch holds");
    let tag_ids = block_on(collect_tag_holds(&tag_reg)).expect("tag holds");
    let holds = SnapshotHolds {
        branch_heads: branch_ids,
        tag_locks: tag_ids,
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    let report = block_on(gc_unreferenced(&graph_store, &policy, &holds, u64::MAX))
        .expect("gc must succeed");

    assert_eq!(report.snapshots_examined, 3);
    assert_eq!(
        report.snapshots_retained, 2,
        "branch head and tag lock must survive"
    );
    assert_eq!(
        report.snapshots_removed, 1,
        "only unprotected snapshot removed"
    );

    // Verify the right snapshot was removed.
    let garbage = block_on(graph_store.load_snapshot(&oid("garbage"))).expect("load");
    assert!(garbage.is_none(), "unprotected snapshot must be removed");

    let head = block_on(graph_store.load_snapshot(&oid("head-snap"))).expect("load");
    assert!(head.is_some(), "branch-head snapshot must survive");

    let rel = block_on(graph_store.load_snapshot(&oid("release-snap"))).expect("load");
    assert!(rel.is_some(), "tag-locked snapshot must survive");
}

// ── Interaction: compact_snapshots + stale holds ──────────────────────────

// Scenario: compact_snapshots replaces snapshot IDs — holds must be refreshed.
//
// `compact_snapshots` deletes original snapshots and stores a new covering
// snapshot with a DIFFERENT id.  Any `SnapshotHolds` built before compaction
// reference the old (now-deleted) ids.  If those stale holds are passed to
// `gc_unreferenced` after compaction, the covering snapshot is NOT in the
// hold set and will be removed if the retention policy also does not protect
// it.
//
// Required operational pattern:
//   1. Run `compact_snapshots`.
//   2. Update branch/tag pointers to the covering snapshot id.
//   3. Refresh holds via `collect_branch_holds` / `collect_tag_holds`.
//   4. Only then run `gc_unreferenced`.
//
// GIVEN a snapshot held by a branch (old id)
// AND   the snapshot is compacted into a covering snapshot (new id)
// WHEN  gc_unreferenced runs with the OLD hold (pre-compaction id)
// THEN  the covering snapshot is unprotected and is removed
// AND   when the hold is refreshed to the covering id, the covering survives
#[test]
fn compact_invalidates_stale_branch_holds() {
    let store = make_store();
    let snap = snapshot("old-head", 1000, Some("p"), None);
    save_all(&store, &[snap]);

    // Compact snap into a covering snapshot — original deleted, new id assigned.
    let compact_report = block_on(compact_snapshots(&store, 0, 0)).expect("compact");
    let covering_id = compact_report.covering_snapshot_id;

    // The covering id must differ from the original id.
    assert_ne!(
        covering_id,
        oid("old-head"),
        "covering id must differ from pre-compaction id"
    );

    // Stale hold: still references the pre-compaction id (which no longer exists).
    let stale_holds = SnapshotHolds {
        branch_heads: vec![oid("old-head")],
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    // GC with stale holds: covering snapshot is not in the hold set → removed.
    let gc_report =
        block_on(gc_unreferenced(&store, &policy, &stale_holds, 0)).expect("gc must succeed");
    assert_eq!(
        gc_report.snapshots_removed, 1,
        "stale hold does not protect the covering snapshot"
    );

    // Remediation: refresh holds to reference the covering snapshot id.
    // In production: update the branch pointer to covering_id first, then
    // call collect_branch_holds to rebuild holds automatically.
    let store2 = make_store();
    let snap2 = snapshot("head2", 2000, Some("p"), None);
    save_all(&store2, &[snap2]);
    let compact2 = block_on(compact_snapshots(&store2, 0, 0)).expect("compact2");

    let fresh_holds = SnapshotHolds {
        branch_heads: vec![compact2.covering_snapshot_id],
        ..Default::default()
    };
    let gc2 = block_on(gc_unreferenced(&store2, &policy, &fresh_holds, 0)).expect("gc2");
    assert_eq!(
        gc2.snapshots_retained, 1,
        "refreshed hold protects the covering snapshot"
    );
    assert_eq!(gc2.snapshots_removed, 0);
}

// ── collect_branch_holds_with_ancestry ────────────────────────────────────

// Scenario: collect_branch_holds_with_ancestry returns HEAD and full chain.
//   GIVEN a chain: genesis → middle → head (three snapshots)
//   AND   a branch points to head
//   WHEN  collect_branch_holds_with_ancestry is called
//   THEN  all three snapshot IDs are returned
#[test]
fn collect_ancestry_holds_returns_full_chain() {
    let graph_store = make_store();
    let branch_reg = BranchRegistry::new();

    let genesis = snapshot("anc-genesis", 100, None, None);
    let middle = snapshot("anc-middle", 200, Some("anc-genesis"), None);
    let head = snapshot("anc-head", 300, Some("anc-middle"), None);
    save_all(&graph_store, &[genesis, middle, head]);

    block_on(branch_reg.create_branch("main", oid("anc-head"), 300)).expect("create branch");

    let ids = block_on(collect_branch_holds_with_ancestry(
        &branch_reg,
        &graph_store,
    ))
    .expect("collect must succeed");

    assert_eq!(ids.len(), 3, "must return head + middle + genesis");
    assert!(ids.contains(&oid("anc-head")), "head must be held");
    assert!(
        ids.contains(&oid("anc-middle")),
        "middle ancestor must be held"
    );
    assert!(ids.contains(&oid("anc-genesis")), "genesis must be held");
}

// Scenario: ancestry holds protect entire branch chain from GC.
//   GIVEN a 3-snapshot chain with no policy protection on its own
//   WHEN  gc_unreferenced runs with ancestry holds
//   THEN  all three snapshots survive
#[test]
fn gc_with_ancestry_holds_protects_full_branch_chain() {
    let graph_store = make_store();
    let branch_reg = BranchRegistry::new();

    let genesis = snapshot("chain-genesis", 100, None, None);
    let middle = snapshot("chain-middle", 200, Some("chain-genesis"), None);
    let head = snapshot("chain-head", 300, Some("chain-middle"), None);
    save_all(&graph_store, &[genesis, middle, head]);

    block_on(branch_reg.create_branch("main", oid("chain-head"), 300)).expect("create branch");

    let ancestry_ids = block_on(collect_branch_holds_with_ancestry(
        &branch_reg,
        &graph_store,
    ))
    .expect("collect ancestry");

    let holds = SnapshotHolds {
        branch_heads: ancestry_ids,
        ..Default::default()
    };
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };

    let report = block_on(gc_unreferenced(&graph_store, &policy, &holds, u64::MAX))
        .expect("gc must succeed");

    assert_eq!(report.snapshots_examined, 3);
    assert_eq!(
        report.snapshots_retained, 3,
        "all three must survive via ancestry holds"
    );
    assert_eq!(report.snapshots_removed, 0);
}

// Scenario: collect_branch_holds_with_ancestry on empty registry returns empty.
#[test]
fn collect_ancestry_holds_empty_registry_returns_empty() {
    let graph_store = make_store();
    let branch_reg = BranchRegistry::new();
    let ids = block_on(collect_branch_holds_with_ancestry(
        &branch_reg,
        &graph_store,
    ))
    .expect("collect must succeed");
    assert!(ids.is_empty(), "empty registry must yield empty holds");
}

// Scenario: two branches sharing a common ancestor — shared chain is deduplicated.
//   GIVEN genesis → A (branch1 head) and genesis → B (branch2 head)
//   WHEN  collect_branch_holds_with_ancestry is called
//   THEN  result contains exactly 3 unique ids: A, B, genesis
#[test]
fn collect_ancestry_holds_deduplicates_shared_ancestors() {
    let graph_store = make_store();
    let branch_reg = BranchRegistry::new();

    let genesis = snapshot("shared-genesis", 100, None, None);
    let b1_head = snapshot("b1-head", 200, Some("shared-genesis"), None);
    let b2_head = snapshot("b2-head", 300, Some("shared-genesis"), None);
    save_all(&graph_store, &[genesis, b1_head, b2_head]);

    block_on(async {
        branch_reg
            .create_branch("b1", oid("b1-head"), 200)
            .await
            .expect("create b1");
        branch_reg
            .create_branch("b2", oid("b2-head"), 300)
            .await
            .expect("create b2");
    });

    let ids = block_on(collect_branch_holds_with_ancestry(
        &branch_reg,
        &graph_store,
    ))
    .expect("collect must succeed");

    // 3 unique ids: genesis + b1-head + b2-head (genesis not duplicated).
    assert_eq!(ids.len(), 3, "shared ancestor must not be duplicated");
    assert!(ids.contains(&oid("shared-genesis")));
    assert!(ids.contains(&oid("b1-head")));
    assert!(ids.contains(&oid("b2-head")));
}

// ── collect_reachable_object_ids_for_snapshots ────────────────────────────

// Scenario: run_gc without envelope CAS id corrupts the store.
//
// `ObjectBackedGraphStore::save_snapshot` writes two CAS objects per snapshot:
//   A. The CBOR-encoded SnapshotEnvelope (envelope bytes).
//   B. The graph_root_hash object — stored separately by the graph layer, NOT
//      by save_snapshot itself.
//
// After save_snapshot, the backing ObjectStore holds the envelope CBOR bytes
// (object A). Their CAS id is distinct from envelope.graph_root_hash.
//
// If run_gc is called with a reachable set that contains ONLY graph_root_hash,
// the envelope bytes (object A) are treated as unreachable and deleted.
// The snapshot_index still maps envelope.id → deleted CAS id, so subsequent
// list_snapshots calls return StorageError::NotFound — data corruption with
// no error at GC time.
//
// GIVEN a snapshot saved to an ObjectBackedGraphStore
// WHEN  run_gc is called with only graph_root_hash in reachable
// THEN  the envelope CAS object is deleted
// AND   list_snapshots subsequently returns an error (corrupted index)
#[test]
fn run_gc_omitting_envelope_id_corrupts_snapshot_index() {
    // Clone the MemoryObjectStore before wrapping it so we can call run_gc
    // on the same underlying storage while graph_store holds a reference.
    let obj_store = MemoryObjectStore::new();
    let obj_store_for_gc = obj_store.clone(); // shares the same Arc<Mutex<...>>
    let graph_store = ObjectBackedGraphStore::new(obj_store);

    let snap = snapshot("envelope-risk", 1000, None, None);
    block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    // Confirm exactly one CAS object exists after save_snapshot.
    // That object is the CBOR-encoded envelope, NOT the graph root content.
    let all_ids = block_on(obj_store_for_gc.list_object_ids()).expect("list");
    assert_eq!(
        all_ids.len(),
        1,
        "exactly one CAS object (the serialized envelope) must exist"
    );

    // Build reachable with ONLY the graph_root_hash — the common mistake.
    // The envelope CAS id is absent from this set.
    let graph_root_only = BTreeSet::from([snap.graph_root_hash]);
    let report = block_on(run_gc(&obj_store_for_gc, &graph_root_only)).expect("run_gc");

    assert_eq!(
        report.objects_deleted, 1,
        "envelope CAS object must be deleted when its id is absent from reachable"
    );
    assert_eq!(report.objects_examined, 1);

    // The snapshot_index still maps envelope.id → deleted CAS id.
    // list_snapshots must now return an error because the CAS object is gone.
    let result = block_on(graph_store.list_snapshots());
    assert!(
        result.is_err(),
        "list_snapshots must fail after envelope deletion — index points to missing CAS object"
    );
}

// Scenario: collect_reachable_object_ids_for_snapshots prevents envelope deletion.
//
// Using the helper to build the reachable set includes both:
//   - the envelope CBOR bytes CAS id
//   - the graph_root_hash
//
// run_gc then has no unreachable objects to delete, and the snapshot index
// remains intact.
//
// GIVEN a snapshot saved to an ObjectBackedGraphStore
// WHEN  collect_reachable_object_ids_for_snapshots is used to build reachable
// AND   run_gc is called with that reachable set
// THEN  no CAS objects are deleted
// AND   list_snapshots returns the snapshot correctly
#[test]
fn run_gc_with_collect_helper_preserves_snapshot_envelope() {
    let obj_store = MemoryObjectStore::new();
    let obj_store_for_gc = obj_store.clone();
    let graph_store = ObjectBackedGraphStore::new(obj_store);

    let snap = snapshot("envelope-safe", 2000, None, None);
    block_on(graph_store.save_snapshot(&snap)).expect("save must succeed");

    // Use the helper to build the correct reachable set.
    let reachable = collect_reachable_object_ids_for_snapshots(std::slice::from_ref(&snap))
        .expect("collect must succeed");

    // The helper must include both the graph_root_hash and the envelope CAS id.
    assert!(
        reachable.contains(&snap.graph_root_hash),
        "reachable must include graph_root_hash"
    );
    assert_eq!(
        reachable.len(),
        2,
        "reachable must contain exactly two ids: envelope CAS id + graph_root_hash"
    );

    let report = block_on(run_gc(&obj_store_for_gc, &reachable)).expect("run_gc");

    assert_eq!(
        report.objects_deleted, 0,
        "no CAS objects must be deleted when reachable includes the envelope id"
    );
    assert_eq!(report.objects_examined, 1);

    // Snapshot index is intact — list_snapshots and load_snapshot must work.
    let list = block_on(graph_store.list_snapshots()).expect("list_snapshots must succeed");
    assert_eq!(list.len(), 1, "snapshot must survive GC");
    assert_eq!(
        list[0].id, snap.id,
        "surviving snapshot must be the correct one"
    );

    let loaded = block_on(graph_store.load_snapshot(&snap.id))
        .expect("load_snapshot must succeed")
        .expect("snapshot must be present");
    assert_eq!(loaded, snap, "loaded snapshot must equal original");
}

// Scenario: end-to-end two-phase GC with gc_unreferenced + run_gc.
//
// Demonstrate the full correct GC workflow:
//   1. gc_unreferenced removes unreachable snapshot index entries.
//   2. list_snapshots enumerates retained snapshots.
//   3. collect_reachable_object_ids_for_snapshots builds the CAS reachable set.
//   4. run_gc deletes unreachable CAS objects, preserving retained envelopes.
//
// GIVEN three snapshots: one genesis (retained by keep_releases), two unprotected
// WHEN  the two-phase GC runs
// THEN  the genesis snapshot's envelope CAS object survives
// AND   the two unprotected envelopes' CAS objects are deleted
#[test]
fn end_to_end_two_phase_gc_preserves_retained_envelope_cas_objects() {
    let obj_store = MemoryObjectStore::new();
    let obj_store_for_gc = obj_store.clone();
    let graph_store = ObjectBackedGraphStore::new(obj_store);

    let genesis = snapshot("e2e-genesis", 100, None, None);
    let drop1 = snapshot("e2e-drop1", 200, Some("e2e-genesis"), None);
    let drop2 = snapshot("e2e-drop2", 300, Some("e2e-genesis"), None);

    block_on(async {
        graph_store
            .save_snapshot(&genesis)
            .await
            .expect("save genesis");
        graph_store.save_snapshot(&drop1).await.expect("save drop1");
        graph_store.save_snapshot(&drop2).await.expect("save drop2");
    });

    // Three snapshots → three envelope CAS objects in the ObjectStore.
    let initial_count = block_on(obj_store_for_gc.list_object_ids())
        .expect("list")
        .len();
    assert_eq!(
        initial_count, 3,
        "three envelope CAS objects must exist initially"
    );

    // Phase 1: snapshot-level GC — removes index entries for unprotected snapshots.
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: true, // genesis (parent_id == None) is retained
        keep_tagged: false,
    };
    let gc_report = block_on(gc_unreferenced(
        &graph_store,
        &policy,
        &SnapshotHolds::default(),
        u64::MAX,
    ))
    .expect("gc_unreferenced must succeed");

    assert_eq!(gc_report.snapshots_retained, 1, "only genesis is retained");
    assert_eq!(
        gc_report.snapshots_removed, 2,
        "two unprotected snapshots removed"
    );

    // Phase 2: collect reachable CAS ids from remaining snapshots.
    let retained = block_on(graph_store.list_snapshots()).expect("list after snapshot gc");
    assert_eq!(retained.len(), 1, "only genesis snapshot remains in index");

    let reachable = collect_reachable_object_ids_for_snapshots(&retained)
        .expect("collect_reachable must succeed");

    // Phase 3: object-level GC — deletes CAS objects not in reachable.
    let obj_report = block_on(run_gc(&obj_store_for_gc, &reachable)).expect("run_gc");

    assert_eq!(
        obj_report.objects_examined, 3,
        "all three envelope objects must be examined"
    );
    assert_eq!(
        obj_report.objects_deleted, 2,
        "two unreachable envelope objects must be deleted"
    );
    assert!(obj_report.bytes_freed > 0, "bytes_freed must be positive");

    // Genesis snapshot must still be fully accessible.
    let final_list = block_on(graph_store.list_snapshots()).expect("final list must succeed");
    assert_eq!(final_list.len(), 1, "genesis snapshot must survive");
    assert_eq!(final_list[0].id, genesis.id);

    let loaded = block_on(graph_store.load_snapshot(&genesis.id))
        .expect("load_snapshot must succeed")
        .expect("genesis must be present");
    assert_eq!(loaded.id, genesis.id);
}
