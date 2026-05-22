// Integration tests: schema migration runner.
//
// Spec scenarios:
//   - "default_catalog advances store from v0 to v3"
//   - "migration applies on fresh (v0) store, writes version 3"
//   - "already-at-latest store returns AlreadyAtVersion(3)"
//   - "current_version on empty store returns 0"
//   - "current_version after migration returns 3"
//   - "partial migration from v1 advances to v3"

use std::sync::Arc;

use ail_storage::{
    MigrationError,
    backends::memory::MemoryObjectStore,
    migration::{V0ToV1Migration, default_catalog, write_version, MigrationStore},
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
