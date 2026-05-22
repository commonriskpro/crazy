// Integration tests: schema migration runner.
//
// Spec scenarios:
//   - "default_catalog lists V0ToV1 migration"
//   - "migration applies on fresh (v0) store, writes version 1"
//   - "already-v1 store returns AlreadyAtVersion"
//   - "current_version on empty store returns 0"
//   - "current_version after migration returns 1"

use std::sync::Arc;

use ail_storage::{
    MigrationError, backends::memory::MemoryObjectStore, migration::default_catalog,
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

// ── default_catalog reports V0ToV1 migration ──────────────────────────────────
// Spec: default_catalog() must contain exactly one migration: V0 → V1.
// Verified indirectly: applying on a fresh store must reach v1.
#[test]
fn default_catalog_has_v0_to_v1() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();
        let result = catalog.apply(store).await;
        assert!(
            result.is_ok(),
            "default_catalog must successfully apply on a v0 store: {result:?}"
        );
        let new_version = result.unwrap();
        assert_eq!(
            new_version, 1,
            "default_catalog must advance store to version 1"
        );
    });
}

// ── migration applies on fresh store, writes version 1 ───────────────────────
// Spec: After apply() on a v0 store, current_version returns 1.
#[test]
fn migration_on_fresh_store_writes_version_1() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();

        let new_version = catalog
            .apply(Arc::clone(&store))
            .await
            .expect("apply on v0 store must succeed");

        assert_eq!(new_version, 1, "apply must return the new schema version 1");

        let stored = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version must succeed after migration");
        assert_eq!(
            stored, 1,
            "current_version must return 1 after successful migration"
        );
    });
}

// ── already-v1 store returns AlreadyAtVersion ─────────────────────────────────
// Spec: Calling apply() when the store is already at the latest version must
//       return Err(MigrationError::AlreadyAtVersion).
#[test]
fn migration_on_v1_store_returns_already_at_version() {
    block_on(async {
        let store = Arc::new(MemoryObjectStore::new());
        let catalog = default_catalog();

        // First apply: advances to v1
        catalog
            .apply(Arc::clone(&store))
            .await
            .expect("first apply must succeed");

        // Second apply: already at v1 → error
        let result = catalog.apply(Arc::clone(&store)).await;
        match result {
            Err(MigrationError::AlreadyAtVersion(v)) => {
                assert_eq!(v, 1, "AlreadyAtVersion must report the current version");
            }
            other => panic!("expected Err(AlreadyAtVersion(1)), got {other:?}"),
        }
    });
}

// ── TRIANGULATE: current_version after migration returns 1 ───────────────────
// Forces real logic: if current_version read the wrong key or defaulted to 0
// unconditionally, this fails.
#[test]
fn current_version_after_migration_is_one() {
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
        catalog
            .apply(Arc::clone(&store))
            .await
            .expect("apply must succeed");

        // After migration: v1
        let after = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version after migration");
        assert_eq!(after, 1, "after migration version must be 1");

        // The two values must differ — rules out a trivially hardcoded implementation
        assert_ne!(before, after, "version must advance after migration");
    });
}
