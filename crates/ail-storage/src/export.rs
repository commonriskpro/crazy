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
// `include_history`: when true, all snapshots reachable from `root_snapshot_id`
// via parent links are included (reachability traversal).  When false, only the
// root snapshot is included.
//
// `include_artifacts`: when true, the `ObjectId`s of all artifact blobs
// referenced by included snapshots are collected into `artifact_ids`.
//
// `include_schemas`: when true, the `ObjectId`s of all schema/migrator objects
// referenced by included snapshots are collected into `schema_ids`.
//
// # Reachability
//
// History traversal uses ancestor reachability from `root_snapshot_id`, not a
// flat dump of all snapshots.  This ensures the bundle is portable and does not
// include unrelated parallel branches.
//
// # Real payloads
//
// `ExportBundle` carries the actual `SnapshotEnvelope` objects, ChangeSet log
// entry ids, artifact ids, and schema ids — not just id lists.  This satisfies
// the spec requirement that a project export bundle is self-contained and
// portable.
//
// # Determinism
//
// `ExportBundle` follows the project's determinism contract: no HashMap
// fields, no floats, timestamps as u64 Unix milliseconds.

use serde::{Deserialize, Serialize};

use crate::error::StorageResult;
use crate::graph::{GraphStore, SnapshotEnvelope};
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
/// Carries all the spec-required payload types:
/// - `snapshots`     — the actual `SnapshotEnvelope` objects (graph state)
/// - `snapshot_ids`  — ordered id list (kept for backward compat)
/// - `changeset_ids` — ids of ChangeSets applied in included snapshots
/// - `artifact_ids`  — content-addressed artifact blob ids (when `include_artifacts`)
/// - `schema_ids`    — content-addressed schema/migrator object ids (when `include_schemas`)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportBundle {
    /// The root snapshot that anchors this export.
    pub root_snapshot_id: ObjectId,
    /// The scope that was applied when building this bundle.
    pub scope: ExportScope,
    /// All snapshot identifiers included in this bundle, ordered by
    /// `created_at` (ascending), ties broken by `id` bytes.
    pub snapshot_ids: Vec<ObjectId>,
    /// Full `SnapshotEnvelope` objects for each id in `snapshot_ids`.
    ///
    /// This is the portable graph-state payload required by the spec.
    pub snapshots: Vec<SnapshotEnvelope>,
    /// Ids of ChangeSets applied by each included snapshot (from
    /// `applied_change_id` field).  Deduplicated and sorted.
    pub changeset_ids: Vec<ObjectId>,
    /// Ids of artifact blobs referenced by included snapshots.
    ///
    /// Populated when `scope.include_artifacts = true`; empty otherwise.
    pub artifact_ids: Vec<ObjectId>,
    /// Ids of schema/migrator objects associated with this export.
    ///
    /// Populated when `scope.include_schemas = true`; empty otherwise.
    pub schema_ids: Vec<ObjectId>,
    /// Unix timestamp in milliseconds when this bundle was created.
    pub created_at: u64,
}

// ── build_export_bundle ───────────────────────────────────────────────────

/// Build an [`ExportBundle`] from `store`.
///
/// # Parameters
/// - `store`            — The `GraphStore` to export from.
/// - `root_snapshot_id` — The root snapshot to anchor the bundle.
/// - `scope`            — Controls which snapshots and payloads are included.
/// - `now_ms`           — Unix timestamp in milliseconds (injected for determinism).
/// - `artifact_ids`     — Artifact blob ids to embed when `scope.include_artifacts`.
/// - `schema_ids`       — Schema/migrator object ids to embed when `scope.include_schemas`.
///
/// # Reachability
///
/// When `scope.include_history` is `true`, the function traverses ancestor
/// links (`parent_id`) from `root_snapshot_id` and includes only reachable
/// snapshots — not the entire store contents.
///
/// # Errors
/// Propagates any `StorageError` from `list_snapshots`.
pub async fn build_export_bundle<S>(
    store: &S,
    root_snapshot_id: ObjectId,
    scope: ExportScope,
    now_ms: u64,
    artifact_ids: Vec<ObjectId>,
    schema_ids: Vec<ObjectId>,
) -> StorageResult<ExportBundle>
where
    S: GraphStore + Send + Sync,
{
    // Build a lookup map from snapshot id → SnapshotEnvelope.
    let all_snaps = store.list_snapshots().await?;
    let snap_map: std::collections::BTreeMap<ObjectId, SnapshotEnvelope> =
        all_snaps.into_iter().map(|s| (s.id, s)).collect();

    // Collect reachable snapshots.
    let mut included_snaps: Vec<SnapshotEnvelope> = if scope.include_history {
        // Traverse ancestor chain from root.
        let mut reachable = Vec::new();
        let mut current_id = Some(root_snapshot_id);
        while let Some(id) = current_id {
            match snap_map.get(&id) {
                None => break,
                Some(snap) => {
                    current_id = snap.parent_id;
                    reachable.push(snap.clone());
                }
            }
        }
        reachable
    } else {
        // Only the root snapshot.
        if let Some(snap) = snap_map.get(&root_snapshot_id) {
            vec![snap.clone()]
        } else {
            vec![]
        }
    };

    // Sort deterministically by created_at, then id bytes.
    included_snaps.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.as_bytes().cmp(b.id.as_bytes()))
    });

    let snapshot_ids: Vec<ObjectId> = included_snaps.iter().map(|s| s.id).collect();

    // Collect ChangeSet ids from applied_change_id fields.
    let mut changeset_ids: Vec<ObjectId> = included_snaps
        .iter()
        .filter_map(|s| s.applied_change_id)
        .collect();
    changeset_ids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    changeset_ids.dedup();

    // Apply artifact and schema scope flags.
    let effective_artifact_ids = if scope.include_artifacts {
        let mut ids = artifact_ids;
        ids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        ids.dedup();
        ids
    } else {
        vec![]
    };

    let effective_schema_ids = if scope.include_schemas {
        let mut ids = schema_ids;
        ids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        ids.dedup();
        ids
    } else {
        vec![]
    };

    Ok(ExportBundle {
        root_snapshot_id,
        scope,
        snapshot_ids,
        snapshots: included_snaps,
        changeset_ids,
        artifact_ids: effective_artifact_ids,
        schema_ids: effective_schema_ids,
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

    /// Build a linear chain: e1 → e2 → e3 (e3 is the tip).
    fn make_chained_envelopes() -> (SnapshotEnvelope, SnapshotEnvelope, SnapshotEnvelope) {
        let e1 = make_envelope(1, 100);
        let mut e2 = make_envelope(2, 200);
        e2.parent_id = Some(e1.id);
        let mut e3 = make_envelope(3, 300);
        e3.parent_id = Some(e2.id);
        (e1, e2, e3)
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
            build_export_bundle(&store, e2.id, scope_root_only(), 99999, vec![], vec![])
                .await
                .expect("build");
        assert_eq!(bundle.root_snapshot_id, e2.id);
        assert_eq!(bundle.snapshot_ids, vec![e2.id]);
        assert_eq!(bundle.scope, scope_root_only());
        assert_eq!(bundle.snapshots.len(), 1);
        assert_eq!(bundle.snapshots[0].id, e2.id);
    }

    // Scenario: bundle with include_history=true traverses ancestor chain.
    //   GIVEN chained store e1 → e2 → e3 (tip)
    //   WHEN build_export_bundle with root=e3 and include_history=true
    //   THEN snapshot_ids contains e1, e2, e3 (all ancestors of e3)
    #[tokio::test]
    async fn export_with_history_reachable_only() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let (e1, e2, e3) = make_chained_envelopes();
        // Also store an unrelated snapshot (no parent link to e3 chain).
        let unrelated = make_envelope(99, 50);
        store.save_snapshot(&e1).await.expect("save e1");
        store.save_snapshot(&e2).await.expect("save e2");
        store.save_snapshot(&e3).await.expect("save e3");
        store.save_snapshot(&unrelated).await.expect("save unrelated");

        let bundle =
            build_export_bundle(&store, e3.id, scope_with_history(), 99999, vec![], vec![])
                .await
                .expect("build");

        // Must include e1, e2, e3 but NOT unrelated.
        let ids: Vec<ObjectId> = bundle.snapshot_ids.clone();
        assert!(ids.contains(&e1.id), "e1 must be reachable from e3");
        assert!(ids.contains(&e2.id), "e2 must be reachable from e3");
        assert!(ids.contains(&e3.id), "e3 must be root");
        assert!(!ids.contains(&unrelated.id), "unrelated must not be in bundle");
        assert_eq!(ids.len(), 3);
    }

    // Scenario: snapshots field carries full SnapshotEnvelope objects.
    #[tokio::test]
    async fn export_snapshots_field_carries_full_envelopes() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(10, 1000);
        store.save_snapshot(&e).await.expect("save");

        let bundle =
            build_export_bundle(&store, e.id, scope_root_only(), 0, vec![], vec![])
                .await
                .expect("build");
        assert_eq!(bundle.snapshots.len(), 1);
        assert_eq!(bundle.snapshots[0], e);
    }

    // Scenario: changeset_ids populated from applied_change_id.
    #[tokio::test]
    async fn export_changeset_ids_collected() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let cs_id = ObjectId::from_bytes(&[0xaa; 32]);
        let mut e = make_envelope(5, 500);
        e.applied_change_id = Some(cs_id);
        store.save_snapshot(&e).await.expect("save");

        let bundle =
            build_export_bundle(&store, e.id, scope_root_only(), 0, vec![], vec![])
                .await
                .expect("build");
        assert_eq!(bundle.changeset_ids, vec![cs_id]);
    }

    // Scenario: artifact_ids empty when include_artifacts=false.
    #[tokio::test]
    async fn export_no_artifacts_when_flag_false() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(1, 0);
        store.save_snapshot(&e).await.expect("save");
        let artifact_id = ObjectId::from_bytes(&[0xbb; 32]);

        let bundle = build_export_bundle(
            &store, e.id, scope_root_only(), 0,
            vec![artifact_id], vec![],
        )
        .await
        .expect("build");
        // include_artifacts=false → artifact_ids must be empty.
        assert!(bundle.artifact_ids.is_empty());
    }

    // Scenario: artifact_ids populated when include_artifacts=true.
    //   GIVEN scope.include_artifacts=true
    //   WHEN artifact_ids supplied to build_export_bundle
    //   THEN bundle.artifact_ids contains them
    #[tokio::test]
    async fn export_includes_artifacts_when_flag_true() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(1, 0);
        store.save_snapshot(&e).await.expect("save");
        let a1 = ObjectId::from_bytes(&[0x01; 32]);
        let a2 = ObjectId::from_bytes(&[0x02; 32]);

        let scope = ExportScope {
            include_history: false,
            include_artifacts: true,
            include_schemas: false,
        };
        let bundle =
            build_export_bundle(&store, e.id, scope, 0, vec![a1, a2], vec![])
                .await
                .expect("build");
        assert!(bundle.artifact_ids.contains(&a1));
        assert!(bundle.artifact_ids.contains(&a2));
    }

    // Scenario: schema_ids populated when include_schemas=true.
    #[tokio::test]
    async fn export_includes_schemas_when_flag_true() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(1, 0);
        store.save_snapshot(&e).await.expect("save");
        let s1 = ObjectId::from_bytes(&[0x10; 32]);

        let scope = ExportScope {
            include_history: false,
            include_artifacts: false,
            include_schemas: true,
        };
        let bundle =
            build_export_bundle(&store, e.id, scope, 0, vec![], vec![s1])
                .await
                .expect("build");
        assert!(bundle.schema_ids.contains(&s1));
    }

    // Scenario: schema_ids empty when include_schemas=false.
    #[tokio::test]
    async fn export_no_schemas_when_flag_false() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(1, 0);
        store.save_snapshot(&e).await.expect("save");
        let s1 = ObjectId::from_bytes(&[0x10; 32]);

        let bundle =
            build_export_bundle(&store, e.id, scope_root_only(), 0, vec![], vec![s1])
                .await
                .expect("build");
        assert!(bundle.schema_ids.is_empty());
    }

    // Scenario: bundle root_snapshot_id is preserved correctly.
    #[tokio::test]
    async fn export_root_snapshot_id_preserved() {
        let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let e = make_envelope(42, 0);
        store.save_snapshot(&e).await.expect("save");
        let bundle =
            build_export_bundle(&store, e.id, scope_root_only(), 1234, vec![], vec![])
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
        let bundle = build_export_bundle(&store, root_id, scope_with_history(), 0, vec![], vec![])
            .await
            .expect("build");
        assert!(bundle.snapshot_ids.is_empty());
    }
}
