// Integration tests: schema migration runner.
//
// Spec scenarios:
//   - "default_catalog advances store from v0 to v3"
//   - "migration applies on fresh (v0) store, writes version 3"
//   - "already-at-latest store returns AlreadyAtVersion(3)"
//   - "current_version on empty store returns 0"
//   - "current_version after migration returns 3"
//   - "partial migration from v1 advances to v3"
//   - "apply_with_output returns MigrationReport for each step"
//   - "DomainVersions tracks all six schema domains"
//   - "migration creates new snapshot without overwriting old"

use std::sync::Arc;

use ail_storage::{
    MigrationError,
    backends::memory::MemoryObjectStore,
    migration::{
        DomainVersions, MigrationReport, V0ToV1Migration, default_catalog, write_version,
        MigrationStore,
    },
};
use futures::executor::block_on;

// ── current_version on empty store returns 0 ─────────────────────────────────
// Spec: A freshly created store has no schema version key → version 0.
#[test]
fn current_version_on_empty_store_is_zero() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();
        let version = catalog
            .current_version(store)
            .await
            .expect("current_version must not error on empty store");
        assert_eq!(version, 0, "empty store must report schema version 0");
    });
}

// ── default_catalog advances to v3 ───────────────────────────────────────────
// Spec: default_catalog() contains V0→V1, V1→V2, V2→V3.
// Verified indirectly: applying on a fresh store must reach v3.
#[test]
fn default_catalog_advances_to_v3() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();
        let result = catalog.apply(store).await;
        assert!(
            result.is_ok(),
            "default_catalog must successfully apply on a v0 store: {result:?}"
        );
        let new_version = result.unwrap();
        assert_eq!(new_version, 3, "default_catalog must advance store to version 3");
    });
}

// ── migration applies on fresh store, writes version 3 ───────────────────────
// Spec: After apply() on a v0 store, current_version returns 3.
#[test]
fn migration_on_fresh_store_writes_version_3() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();

        let new_version = catalog
            .apply(Arc::clone(&store))
            .await
            .expect("apply on v0 store must succeed");

        assert_eq!(new_version, 3, "apply must return the new schema version 3");

        let stored = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version must succeed after migration");
        assert_eq!(stored, 3, "current_version must return 3 after successful migration");
    });
}

// ── already-at-latest store returns AlreadyAtVersion(3) ──────────────────────
// Spec: Calling apply() when the store is already at the latest version must
//       return Err(MigrationError::AlreadyAtVersion(3)).
#[test]
fn migration_on_latest_store_returns_already_at_version() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();

        // First apply: advances to v3
        catalog
            .apply(Arc::clone(&store))
            .await
            .expect("first apply must succeed");

        // Second apply: already at v3 → error
        let result = catalog.apply(Arc::clone(&store)).await;
        match result {
            Err(MigrationError::AlreadyAtVersion(v)) => {
                assert_eq!(v, 3, "AlreadyAtVersion must report the current version 3");
            }
            other => panic!(
                "expected Err(AlreadyAtVersion(3)), got {other:?}"
            ),
        }
    });
}

// ── TRIANGULATE: current_version after migration returns 3 ───────────────────
// Forces real logic: if current_version read the wrong key or defaulted to 0
// unconditionally, this fails.
#[test]
fn current_version_after_migration_is_three() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();

        // Before migration: v0
        let before = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version before migration");
        assert_eq!(before, 0, "before migration version must be 0");

        // Apply migration
        catalog.apply(Arc::clone(&store)).await.expect("apply must succeed");

        // After migration: v3
        let after = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version after migration");
        assert_eq!(after, 3, "after migration version must be 3");

        // The two values must differ — rules out a trivially hardcoded implementation
        assert_ne!(before, after, "version must advance after migration");
    });
}

// ── partial migration: store at v1 advances to v3 ────────────────────────────
// Spec: If store is already at v1, apply() skips the V0→V1 migration and
//       applies V1→V2 and V2→V3, reaching v3.
#[test]
fn partial_migration_from_v1_reaches_v3() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let ms = MigrationStore::new(Arc::clone(&store));

        // Manually write v1 to simulate a store already migrated to v1.
        write_version(&ms, 1).await.expect("write v1");

        let catalog = default_catalog();
        let version = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version");
        assert_eq!(version, 1, "store should be at v1");

        let new_version = catalog
            .apply(Arc::clone(&store))
            .await
            .expect("apply from v1 must succeed");
        assert_eq!(new_version, 3, "must advance from v1 to v3");
    });
}

// ── V0ToV1Migration still usable standalone ───────────────────────────────────
#[test]
fn v0_to_v1_migration_standalone() {
    block_on(async {
        use ail_storage::migration::MigrationCatalog;
        let store = Arc::new(MemoryObjectStore::new());
        let mut catalog = MigrationCatalog::new();
        catalog.register(V0ToV1Migration);
        let version = catalog.apply(Arc::clone(&store)).await.expect("apply");
        assert_eq!(version, 1);
    });
}

// ── apply_with_output returns MigrationReport for each step ──────────────────
// Spec: migration report records structural equivalence / preserved semantics.
#[test]
fn apply_with_output_returns_reports() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();
        let (version, outputs) = catalog
            .apply_with_output(Arc::clone(&store))
            .await
            .expect("apply_with_output must succeed");
        assert_eq!(version, 3, "must advance to version 3");
        assert_eq!(outputs.len(), 3, "must have one output per migration step");
        // Every step must carry a report.
        for (i, output) in outputs.iter().enumerate() {
            assert!(
                output.report.is_some(),
                "step {i} must have a MigrationReport"
            );
        }
        // First step report must assert structural equivalence.
        let report = outputs[0].report.as_ref().unwrap();
        assert!(
            report.structural_equivalence,
            "step 0 must assert structural equivalence"
        );
    });
}

// ── MigrationReport preserved_semantics is non-empty ─────────────────────────
#[test]
fn migration_report_has_preserved_semantics() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();
        let (_version, outputs) = catalog
            .apply_with_output(Arc::clone(&store))
            .await
            .expect("apply_with_output");
        for (i, output) in outputs.iter().enumerate() {
            let report = output.report.as_ref().unwrap();
            assert!(
                !report.preserved_semantics.is_empty(),
                "step {i} report must list at least one preserved semantic"
            );
        }
    });
}

// ── DomainVersions has all six required domain fields ────────────────────────
// Spec: storage versions all listed schema domains:
//   graph, core_ir, acl, verification, runtime, artifact
#[test]
fn domain_versions_has_all_six_fields() {
    let dv = DomainVersions {
        graph: 1,
        core_ir: 2,
        acl: 3,
        verification: 4,
        runtime: 5,
        artifact: 6,
    };
    assert_eq!(dv.graph, 1);
    assert_eq!(dv.core_ir, 2);
    assert_eq!(dv.acl, 3);
    assert_eq!(dv.verification, 4);
    assert_eq!(dv.runtime, 5);
    assert_eq!(dv.artifact, 6);
}

// ── DomainVersions default is all zeros ──────────────────────────────────────
#[test]
fn domain_versions_default_is_all_zeros() {
    let dv = DomainVersions::default();
    assert_eq!(dv.graph, 0);
    assert_eq!(dv.core_ir, 0);
    assert_eq!(dv.acl, 0);
    assert_eq!(dv.verification, 0);
    assert_eq!(dv.runtime, 0);
    assert_eq!(dv.artifact, 0);
}

// ── DomainVersions domains are independent ────────────────────────────────────
// Advancing graph version does not change acl or runtime.
#[test]
fn domain_versions_domains_are_independent() {
    let mut dv = DomainVersions::default();
    dv.graph = 4;
    assert_eq!(dv.acl, 0, "acl must stay at 0 when only graph advances");
    assert_eq!(dv.runtime, 0, "runtime must stay at 0 when only graph advances");
    dv.acl = 2;
    assert_eq!(dv.graph, 4, "graph must stay at 4 when only acl advances");
}

// ── MigrationReport is round-trippable as a value ────────────────────────────
#[test]
fn migration_report_fields_are_accessible() {
    let report = MigrationReport {
        description: "test migration".to_owned(),
        structural_equivalence: true,
        preserved_semantics: vec!["all nodes preserved".to_owned()],
        pre_snapshot_id: None,
        post_snapshot_id: None,
    };
    assert_eq!(report.description, "test migration");
    assert!(report.structural_equivalence);
    assert_eq!(report.preserved_semantics.len(), 1);
}

// ── Migration creating a new snapshot (output.new_snapshot) ──────────────────
// Spec: Migration creates new snapshot; old snapshot not overwritten.
// We verify that the output type can carry a SnapshotEnvelope and that the
// catalog preserves it when returned via apply_with_output.
#[test]
fn migration_output_can_carry_new_snapshot() {
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;
    use ail_storage::migration::{Migration, MigrationCatalog, MigrationOutput, MigrationStore};
    use std::pin::Pin;
    use std::future::Future;

    // A migration that produces a new snapshot.
    struct SnapshotCreatingMigration;
    impl Migration for SnapshotCreatingMigration {
        fn source_version(&self) -> u32 { 0 }
        fn target_version(&self) -> u32 { 1 }
        fn up(
            &self,
            store: MigrationStore,
        ) -> Pin<Box<dyn Future<Output = Result<MigrationOutput, ail_storage::MigrationError>> + Send + '_>> {
            Box::pin(async move {
                write_version(&store, 1).await?;
                let snap = SnapshotEnvelope {
                    id: ObjectId::from_bytes(&[0xab; 32]),
                    graph_root_hash: ObjectId::from_bytes(&[0xcd; 32]),
                    parent_id: None,
                    applied_change_id: None,
                    created_at: 12345,
                    verification_report_hash: None,
                };
                Ok(MigrationOutput {
                    new_snapshot: Some(snap),
                    report: Some(MigrationReport {
                        description: "snapshot migration".to_owned(),
                        structural_equivalence: true,
                        preserved_semantics: vec!["genesis snapshot created".to_owned()],
                        pre_snapshot_id: None,
                        post_snapshot_id: Some(ObjectId::from_bytes(&[0xab; 32])),
                    }),
                })
            })
        }
    }

    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let mut catalog = MigrationCatalog::new();
        catalog.register(SnapshotCreatingMigration);
        let (_version, outputs) = catalog
            .apply_with_output(Arc::clone(&store))
            .await
            .expect("apply_with_output");
        assert_eq!(outputs.len(), 1);
        let output = &outputs[0];
        let snap = output.new_snapshot.as_ref().expect("must have new snapshot");
        assert_eq!(snap.id, ail_storage::object::ObjectId::from_bytes(&[0xab; 32]));
        assert_eq!(snap.created_at, 12345);
        // Report records the post snapshot id.
        let report = output.report.as_ref().unwrap();
        assert_eq!(
            report.post_snapshot_id,
            Some(ail_storage::object::ObjectId::from_bytes(&[0xab; 32]))
        );
    });
}
