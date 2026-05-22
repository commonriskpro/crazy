// Backup and portability export bundles.
//
// # Design
//
// `build_export_bundle` reads the snapshot log from a `GraphStore`, then
// collects snapshots according to the requested `ExportScope`.  The result
// is a pure-data `ExportBundle` — no bytes are written to disk in this
// phase; the bundle is returned to the caller for serialization or transfer.
//
// # Scope semantics
//
// `include_history`: when true, all snapshots reachable from the store are
// included.  When false, only the root snapshot is included.
//
// `include_artifacts` and `include_schemas` are flags recorded in the scope
// for consumers to honour during serialization; this crate does not itself
// gather artifact or schema objects.
//
// # Determinism
//
// `ExportBundle` follows the project's determinism contract: no HashMap
// fields, no floats, timestamps as u64 Unix milliseconds.

use serde::{Deserialize, Serialize};

use crate::error::StorageResult;
use crate::graph::GraphStore;
use crate::object::ObjectId;

// ── ExportScope ───────────────────────────────────────────────────────────

/// Controls what content is included in an [`ExportBundle`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportScope {
    /// When `true`, all snapshots in the store are included (history).
    /// When `false`, only the root snapshot is included.
    pub include_history: bool,
    /// When `true`, artifact references are included (honour by the consumer).
    pub include_artifacts: bool,
    /// When `true`, schema/migrator references are included.
    pub include_schemas: bool,
}

// ── ExportBundle ──────────────────────────────────────────────────────────

/// A portable snapshot of a project's graph storage.
///
/// `snapshot_ids` lists all snapshot identifiers included in the bundle,
/// ordered by snapshot `created_at` (ascending), ties broken by `id` bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportBundle {
    /// The root snapshot that anchors this export.
    pub root_snapshot_id: ObjectId,
    /// The scope that was applied when building this bundle.
    pub scope: ExportScope,
    /// All snapshot identifiers included in this bundle.
    pub snapshot_ids: Vec<ObjectId>,
    /// Unix timestamp in milliseconds when this bundle was created.
    pub created_at: u64,
}

// ── build_export_bundle ───────────────────────────────────────────────────

/// Build an [`ExportBundle`] from `store`.
///
/// # Parameters
/// - `store`            — The `GraphStore` to export from.
/// - `root_snapshot_id` — The root snapshot to anchor the bundle.
/// - `scope`            — Controls which snapshots are included.
/// - `now_ms`           — Unix timestamp in milliseconds (injected for determinism).
///
/// # Errors
/// Propagates any `StorageError` from `list_snapshots`.
pub async fn build_export_bundle<S>(
    store: &S,
    root_snapshot_id: ObjectId,
    scope: ExportScope,
    now_ms: u64,
) -> StorageResult<ExportBundle>
where
    S: GraphStore + Send + Sync,
{
    let snapshot_ids = if scope.include_history {
        // Include all snapshots from the store, sorted for determinism.
        let mut all = store.list_snapshots().await?;
        all.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.as_bytes().cmp(b.id.as_bytes()))
        });
        all.into_iter().map(|s| s.id).collect()
    } else {
        // Only the root snapshot.
        vec![root_snapshot_id]
    };

    Ok(ExportBundle {
        root_snapshot_id,
        scope,
        snapshot_ids,
        created_at: now_ms,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::memory::MemoryObjectStore;
    use crate::graph::{ObjectBackedGraphStore, SnapshotEnvelope};

    fn make_envelope(seed: u8, created_at: u64) -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(&[seed; 32]);
        let root = ObjectId::from_bytes(&[seed + 100; 32]);
        SnapshotEnvelope {
            id,
            graph_root_hash: root,
            parent_id: None,
            applied_change_id: None,
            created_at,
            verification_report_hash: None,
        }
    }

    fn scope_with_history() -> ExportScope {
        ExportScope {
            include_history: true,
            include_artifacts: false,
            include_schemas: false,
        }
    }

    fn scope_root_only() -> ExportScope {
        ExportScope {
            include_history: false,
            include_artifacts: false,
            include_schemas: false,
        }
    }

    // Scenario: bundle with include_history=false contains only root snapshot.
    //   GIVEN store with 3 snapshots
    //   WHEN build_export_bundle with root snap id and include_history=false
    //   THEN snapshot_ids has exactly 1 entry = root_snapshot_id
    #[tokio::test]
    async fn export_root_only() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e1 = make_envelope(1, 100);
        let e2 = make_envelope(2, 200);
        let e3 = make_envelope(3, 300);
        store.save_snapshot(&e1).await.expect("save e1");
        store.save_snapshot(&e2).await.expect("save e2");
        store.save_snapshot(&e3).await.expect("save e3");

        let bundle =
            build_export_bundle(&store, e2.id, scope_root_only(), 99999)
                .await
                .expect("build");
        assert_eq!(bundle.root_snapshot_id, e2.id);
        assert_eq!(bundle.snapshot_ids, vec![e2.id]);
        assert_eq!(bundle.scope, scope_root_only());
    }

    // Scenario: bundle with include_history=true contains all snapshots.
    //   GIVEN store with 3 snapshots
    //   WHEN build_export_bundle with include_history=true
    //   THEN snapshot_ids has 3 entries in created_at order
    #[tokio::test]
    async fn export_with_history() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e1 = make_envelope(1, 100);
        let e2 = make_envelope(2, 200);
        let e3 = make_envelope(3, 300);
        store.save_snapshot(&e1).await.expect("save e1");
        store.save_snapshot(&e2).await.expect("save e2");
        store.save_snapshot(&e3).await.expect("save e3");

        let bundle =
            build_export_bundle(&store, e1.id, scope_with_history(), 99999)
                .await
                .expect("build");
        assert_eq!(bundle.snapshot_ids.len(), 3);
        // Must be sorted by created_at.
        assert_eq!(bundle.snapshot_ids[0], e1.id);
        assert_eq!(bundle.snapshot_ids[1], e2.id);
        assert_eq!(bundle.snapshot_ids[2], e3.id);
    }

    // Scenario: bundle root_snapshot_id is preserved correctly.
    #[tokio::test]
    async fn export_root_snapshot_id_preserved() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(42, 0);
        store.save_snapshot(&e).await.expect("save");
        let bundle =
            build_export_bundle(&store, e.id, scope_root_only(), 1234)
                .await
                .expect("build");
        assert_eq!(bundle.root_snapshot_id, e.id);
        assert_eq!(bundle.created_at, 1234);
    }

    // Scenario: empty store with include_history=true yields empty snapshot_ids.
    #[tokio::test]
    async fn export_empty_store_with_history() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let root_id = ObjectId::from_bytes(&[0u8; 32]);
        let bundle = build_export_bundle(&store, root_id, scope_with_history(), 0)
            .await
            .expect("build");
        assert!(bundle.snapshot_ids.is_empty());
    }
}
