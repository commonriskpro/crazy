# Migration Guide

<!-- Status: Implemented subset. Documents current storage schema migrations; future migrations must extend this file. -->
<!-- Release metadata: latest-storage-schema=3; compatibility-breaking=false -->

This document describes how to upgrade an existing AIL object store across schema
versions, and how to roll back if something goes wrong.

## Schema Version Overview

The AIL storage layer tracks a schema version in the object store. The version
determines which data layout the store uses and which migrations have been applied.

| Version | Description |
|---------|-------------|
| 0 | Implicit — no version record in the store (pre-migration runner era). |
| 1 | Baseline — version record written; no structural change to existing objects. |
| 2 | Structural no-op — version record written; preserves existing objects. |
| 3 | Structural no-op — version record written; preserves graph, verification, and ACL data. |

## v0 → v1 Upgrade

Schema v1 is a **structural no-op**: no existing objects are modified. The only
change is the addition of a version record keyed by `blake3(CBOR(1))` in the
object store.

The built-in catalog currently continues with v1 -> v2 and v2 -> v3 structural
no-op steps. A fresh v0 store therefore migrates to v3 when using
`default_catalog()`.

### When is this migration needed?

If your store was created before the migration runner was introduced (i.e., you are
running code from before this release), the store is implicitly at v0. The first
call to `MigrationCatalog::apply` will advance it to the latest registered
version.

### Running the migration

```rust
use std::sync::Arc;
use ail_storage::{backends::memory::MemoryObjectStore, migration::default_catalog};

let store = Arc::new(your_store); // any ObjectStore implementor
let catalog = default_catalog();

match catalog.apply(Arc::clone(&store)).await {
    Ok(new_version) => println!("Migrated to v{new_version}"),
    Err(ail_storage::MigrationError::AlreadyAtVersion(v)) => {
        println!("Already at v{v} — no migration needed");
    }
    Err(e) => return Err(e.into()),
}
```

Checking the current version without applying:

```rust
let version = catalog.current_version(Arc::clone(&store)).await?;
println!("Current schema version: {version}");
```

Previewing the operational plan without applying:

```rust
let report = catalog.dry_run(Arc::clone(&store)).await?;
println!(
    "current=v{} target=v{} pending_steps={}",
    report.current_version,
    report.target_version,
    report.pending_steps.len()
);
```

`dry_run` only reads version markers and walks the registered catalog. It does
not call migration bodies and does not write version records. If the catalog has
no contiguous path from the current version to the target, `blocked_at_version`
reports where planning stopped.

### Is it safe to apply the migration without downtime?

Yes. The v0 → v1 migration writes a single small object. The store continues to
serve reads and writes during the migration. No existing objects are removed or
modified.

### Is it idempotent?

Yes. Calling `apply` on an already-migrated store returns
`Err(MigrationError::AlreadyAtVersion(3))` for the current built-in catalog and
makes no changes.

## Rollback

Schema v1-v3 adds version marker objects to the store; it does not remove or
transform any existing data. To roll back to v0:

1. **Stop all writers** that use the migrated store.
2. **Restore from a snapshot** taken before the migration ran. If you use
   `TempfileObjectStore` or a Postgres-backed store, restore from your backup.
3. **Verify** the restored store reports v0:
   ```rust
   let v = catalog.current_version(Arc::clone(&store)).await?;
   assert_eq!(v, 0);
   ```

If you have no snapshot, the safest option is to delete the version record
objects for the applied versions (each identified by
`ObjectId::from_bytes(&CborCodec.encode(&version).unwrap())`) from the store
directly. This is store-backend-specific and should only be done in a controlled
maintenance window.

## Future Migrations

When a new migration is registered in `MigrationCatalog` (e.g., v1 → v2), the
`apply` call will run all pending migrations in sequence. Consult the next section
of this guide (or the `CHANGELOG.md`) for instructions specific to that version.
