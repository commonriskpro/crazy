// ── ail-change::storage_bridge ───────────────────────────────────────────
//
// Compiled only under the `storage-bridge` Cargo feature.
//
// # `MemorySnapshotBridge`
//
// An in-process `SnapshotBridge` backed by `MemoryObjectStore` for use in
// tests and local development.  Wraps an `ObjectBackedGraphStore` so that
// snapshot and log persistence follow the same path as production backends.
//
// # Trait surface
//
// Implements `SnapshotBridge` (from `crate::apply`) for snapshot-guard checks.
// Provides async `save_snapshot`, `load_snapshot`, and `append_changeset_log`
// methods that delegate directly to the wrapped `ObjectBackedGraphStore`.
//
// # Feature gate
//
// The entire module is `#[cfg(feature = "storage-bridge")]` so that `ail-change`
// compiles cleanly without the feature and exposes no bridge symbols.

#[cfg(feature = "storage-bridge")]
mod bridge {
    use ail_storage::{
        backends::memory::MemoryObjectStore,
        error::StorageResult,
        graph::{ChangeSetLogEntry, GraphStore, ObjectBackedGraphStore, SnapshotEnvelope},
        object::ObjectId,
    };

    use crate::{apply::SnapshotBridge, model::SnapshotId};

    // ── MemorySnapshotBridge ──────────────────────────────────────────────

    /// In-memory `SnapshotBridge` for tests and local development.
    ///
    /// Holds a `current_id` which is returned by `current_snapshot_id()` for
    /// the snapshot-guard check in `apply()`.  The wrapped `ObjectBackedGraphStore`
    /// provides persistent (within-process) snapshot and log storage.
    pub struct MemorySnapshotBridge {
        current_id: SnapshotId,
        store: ObjectBackedGraphStore<MemoryObjectStore>,
    }

    impl MemorySnapshotBridge {
        /// Create a new `MemorySnapshotBridge` with the given current snapshot id.
        pub fn new(current_id: SnapshotId) -> Self {
            Self {
                current_id,
                store: ObjectBackedGraphStore::new(MemoryObjectStore::new()),
            }
        }

        /// Persist a `SnapshotEnvelope` to the backing store.
        ///
        /// Returns `envelope.id` on success (per `GraphStore::save_snapshot`
        /// spec: callers use this id with `load_snapshot`).
        pub async fn save_snapshot(
            &self,
            envelope: &SnapshotEnvelope,
        ) -> StorageResult<ObjectId> {
            self.store.save_snapshot(envelope).await
        }

        /// Retrieve a previously saved `SnapshotEnvelope` by its identity.
        ///
        /// Returns `None` if the id was never saved.
        pub async fn load_snapshot(
            &self,
            id: &ObjectId,
        ) -> StorageResult<Option<SnapshotEnvelope>> {
            self.store.load_snapshot(id).await
        }

        /// Append a `ChangeSetLogEntry` to the backing store.
        ///
        /// Returns the content-addressed `ObjectId` of the stored bytes.
        pub async fn append_changeset_log(
            &self,
            entry: &ChangeSetLogEntry,
        ) -> StorageResult<ObjectId> {
            self.store.append_changeset_log(entry).await
        }
    }

    impl SnapshotBridge for MemorySnapshotBridge {
        fn current_snapshot_id(&self) -> SnapshotId {
            self.current_id
        }
    }
}

#[cfg(feature = "storage-bridge")]
pub use bridge::MemorySnapshotBridge;

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(all(feature = "storage-bridge", test))]
mod tests {
    use ail_storage::{
        graph::{ChangeSetLogEntry, SnapshotEnvelope},
        object::ObjectId,
    };
    use futures::executor::block_on;

    use super::MemorySnapshotBridge;
    use crate::{apply::SnapshotBridge, model::SnapshotId};

    fn oid(label: &str) -> ObjectId {
        ObjectId::from_bytes(label.as_bytes())
    }

    // Scenario: current_snapshot_id returns the initialised value.
    //   GIVEN MemorySnapshotBridge initialised with SnapshotId(42)
    //   WHEN current_snapshot_id() is called
    //   THEN SnapshotId(42) is returned
    #[test]
    fn current_snapshot_id_returns_initial_value() {
        let bridge = MemorySnapshotBridge::new(SnapshotId(42));
        assert_eq!(
            bridge.current_snapshot_id(),
            SnapshotId(42),
            "current_snapshot_id must return the value passed to new()"
        );
    }

    // TRIANGULATE: a different initial id is also returned correctly.
    //   GIVEN MemorySnapshotBridge initialised with SnapshotId(0)
    //   WHEN current_snapshot_id() is called
    //   THEN SnapshotId(0) is returned
    #[test]
    fn current_snapshot_id_zero_is_valid() {
        let bridge = MemorySnapshotBridge::new(SnapshotId(0));
        assert_eq!(
            bridge.current_snapshot_id(),
            SnapshotId(0),
            "SnapshotId(0) must be a valid initial value"
        );
    }

    // Scenario: snapshot saved and retrievable.
    //   GIVEN apply() returned Applied
    //   WHEN save_snapshot is called with a SnapshotEnvelope
    //   THEN load_snapshot(envelope.id) returns the same envelope
    #[test]
    fn save_snapshot_and_retrieve_by_id() {
        let bridge = MemorySnapshotBridge::new(SnapshotId(1));
        let envelope = SnapshotEnvelope {
            id: oid("snap-bridge-1"),
            graph_root_hash: oid("root-bridge-1"),
            parent_id: None,
            applied_change_id: None,
            created_at: 0,
        };

        let returned_id =
            block_on(bridge.save_snapshot(&envelope)).expect("save_snapshot must succeed");

        assert_eq!(
            returned_id, envelope.id,
            "save_snapshot must return envelope.id"
        );

        let loaded = block_on(bridge.load_snapshot(&envelope.id))
            .expect("load_snapshot must not error")
            .expect("saved snapshot must be retrievable");

        assert_eq!(
            loaded.id, envelope.id,
            "loaded envelope id must match the saved one"
        );
        assert_eq!(
            loaded.graph_root_hash, envelope.graph_root_hash,
            "graph_root_hash must survive the round-trip"
        );
    }

    // Scenario: changeset log appended.
    //   GIVEN apply() returned Applied
    //   WHEN append_changeset_log is called with a ChangeSetLogEntry
    //   THEN an ObjectId is returned without error
    #[test]
    fn append_changeset_log_returns_object_id() {
        let bridge = MemorySnapshotBridge::new(SnapshotId(0));
        let entry = ChangeSetLogEntry {
            id: oid("cs-bridge-1"),
            base_snapshot_id: oid("snap-bridge-1"),
            payload_hash: oid("payload-bridge-1"),
            created_at: 1_000,
        };

        let result = block_on(bridge.append_changeset_log(&entry));
        assert!(result.is_ok(), "append_changeset_log must succeed; got: {result:?}");
        let cas_id = result.unwrap();
        assert_ne!(
            cas_id.as_bytes(),
            &[0u8; 32],
            "CAS id must be a real content hash, not all-zero"
        );
    }
}
