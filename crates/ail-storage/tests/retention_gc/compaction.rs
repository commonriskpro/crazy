use super::helpers::*;

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
