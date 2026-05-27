use super::helpers::*;

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
