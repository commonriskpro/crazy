use super::helpers::*;

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
