// Storage integrity verification.
//
// # Design
//
// `verify_integrity` iterates all snapshots in a `GraphStore` and checks that
// each snapshot's `graph_root_hash` exists as a raw object in the `ObjectStore`.
// Additional checks can be layered on top in future iterations.
//
// The function is purely read-only — it never mutates the store.
//
// # Report semantics
//
// `IntegrityReport.passed` is `true` iff `issues` is empty.  Each
// `IntegrityIssue` variant carries the `ObjectId` that caused the problem.
//
// # Determinism
//
// `IntegrityReport` follows the project's determinism contract.  `issues` is
// sorted by issue kind first (MissingObject < HashMismatch < OrphanedSnapshot),
// then by ObjectId bytes within the same kind.

use serde::{Deserialize, Serialize};

use crate::error::StorageResult;
use crate::graph::GraphStore;
use crate::object::{ObjectId, ObjectStore};

// ── IntegrityIssue ────────────────────────────────────────────────────────

/// A single problem detected by the integrity verifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrityIssue {
    /// A `graph_root_hash` in a snapshot points to an object that does not
    /// exist in the object store.
    MissingObject {
        /// The `ObjectId` that was expected but not found.
        id: ObjectId,
    },
    /// A stored object's content does not match its declared hash.
    ///
    /// Detected when `put` / `get` diverge; currently unused by
    /// `verify_integrity` (needs CAS re-verification loop — reserved).
    HashMismatch {
        /// The `ObjectId` whose content hash is inconsistent.
        id: ObjectId,
    },
    /// A snapshot's `parent_id` points to a snapshot that is not present in
    /// the store (orphaned chain link).
    OrphanedSnapshot {
        /// The `ObjectId` of the orphaned snapshot.
        id: ObjectId,
    },
}

impl IntegrityIssue {
    /// Numeric sort key for ordering issues by kind.
    fn kind_ord(&self) -> u8 {
        match self {
            IntegrityIssue::MissingObject { .. } => 0,
            IntegrityIssue::HashMismatch { .. } => 1,
            IntegrityIssue::OrphanedSnapshot { .. } => 2,
        }
    }

    /// The `ObjectId` associated with this issue.
    fn id(&self) -> &ObjectId {
        match self {
            IntegrityIssue::MissingObject { id }
            | IntegrityIssue::HashMismatch { id }
            | IntegrityIssue::OrphanedSnapshot { id } => id,
        }
    }
}

// ── IntegrityReport ───────────────────────────────────────────────────────

/// Summary of a storage integrity verification run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntegrityReport {
    /// All detected issues, sorted for determinism.
    pub issues: Vec<IntegrityIssue>,
    /// Number of snapshots examined.
    pub snapshots_checked: u64,
    /// `true` iff no issues were detected.
    pub passed: bool,
}

// ── verify_integrity ──────────────────────────────────────────────────────

/// Run integrity checks on `graph_store` against `object_store`.
///
/// # Checks performed
///
/// 1. **MissingObject** — each snapshot's `graph_root_hash` must exist in
///    `object_store`.
/// 2. **OrphanedSnapshot** — each snapshot's `parent_id`, when `Some`, must
///    be the `id` of another snapshot in the store.
///
/// # Returns
///
/// An [`IntegrityReport`] describing all issues found (or confirming a clean
/// pass).  The function never mutates either store.
///
/// # Errors
///
/// Propagates any `StorageError` from `list_snapshots` or `exists`.
pub async fn verify_integrity<G, O>(
    graph_store: &G,
    object_store: &O,
) -> StorageResult<IntegrityReport>
where
    G: GraphStore + Send + Sync,
    O: ObjectStore + Send + Sync,
{
    let snapshots = graph_store.list_snapshots().await?;
    let snapshots_checked = snapshots.len() as u64;

    // Collect snapshot ids for parent-link checks.
    let all_snapshot_ids: std::collections::BTreeSet<ObjectId> =
        snapshots.iter().map(|s| s.id).collect();

    let mut issues = Vec::new();

    for snap in &snapshots {
        // Check 1: graph_root_hash must exist as a raw object.
        let root_exists = object_store.exists(&snap.graph_root_hash).await?;
        if !root_exists {
            issues.push(IntegrityIssue::MissingObject {
                id: snap.graph_root_hash,
            });
        }

        // Check 2: parent_id (when Some) must reference a known snapshot.
        if let Some(parent_id) = snap.parent_id
            && !all_snapshot_ids.contains(&parent_id)
        {
            issues.push(IntegrityIssue::OrphanedSnapshot { id: snap.id });
        }
    }

    // Sort issues for determinism: by kind first, then by id bytes.
    issues.sort_by(|a, b| {
        a.kind_ord()
            .cmp(&b.kind_ord())
            .then(a.id().as_bytes().cmp(b.id().as_bytes()))
    });

    let passed = issues.is_empty();
    Ok(IntegrityReport {
        issues,
        snapshots_checked,
        passed,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::memory::MemoryObjectStore;
    use crate::graph::{GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
    use crate::object::{ObjectStore, RawObject};

    /// Compute the ObjectId that the MemoryObjectStore will assign when
    /// `[seed; 32]` bytes are put.  Because `put` does `ObjectId::from_bytes(&object.0)`
    /// (= BLAKE3 of the payload), the id for `[seed; 32]` bytes is
    /// `BLAKE3([seed; 32])` = `ObjectId::from_bytes(&[seed; 32])`.
    fn make_id(seed: u8) -> ObjectId {
        ObjectId::from_bytes(&[seed; 32])
    }

    /// Store `[seed; 32]` bytes in the object store.
    /// The CAS id assigned will be `ObjectId::from_bytes(&[seed; 32])` = `make_id(seed)`.
    async fn put_seed_object(object_store: &MemoryObjectStore, seed: u8) -> ObjectId {
        object_store
            .put(RawObject(vec![seed; 32]))
            .await
            .expect("put seed object")
    }



    // Scenario: valid store passes integrity check.
    //   GIVEN snapshots whose graph_root_hash objects all exist
    //   WHEN verify_integrity called
    //   THEN report.passed = true and issues empty
    #[tokio::test]
    async fn valid_store_passes() {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

        // put_seed_object(seed) stores [seed;32] bytes → CAS id = make_id(seed)
        let root1_id = put_seed_object(&obj_store, 10).await;
        let root2_id = put_seed_object(&obj_store, 20).await;

        let e1 = SnapshotEnvelope {
            id: make_id(1),
            graph_root_hash: root1_id,
            parent_id: None,
            applied_change_id: None,
            created_at: 100,
            verification_report_hash: None,
        };
        let e2 = SnapshotEnvelope {
            id: make_id(2),
            graph_root_hash: root2_id,
            parent_id: None,
            applied_change_id: None,
            created_at: 200,
            verification_report_hash: None,
        };

        graph_store.save_snapshot(&e1).await.expect("save e1");
        graph_store.save_snapshot(&e2).await.expect("save e2");

        let report = verify_integrity(&graph_store, &obj_store)
            .await
            .expect("verify");
        assert!(report.passed, "report must pass");
        assert!(report.issues.is_empty());
        assert_eq!(report.snapshots_checked, 2);
    }

    // Scenario: missing graph_root_hash object produces MissingObject issue.
    //   GIVEN snapshot whose graph_root_hash does not exist in object store
    //   WHEN verify_integrity called
    //   THEN report has MissingObject issue and passed=false
    #[tokio::test]
    async fn missing_root_object_produces_issue() {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

        // Use a root hash that we deliberately do NOT store in obj_store.
        let missing_root = make_id(99);
        let e = SnapshotEnvelope {
            id: make_id(1),
            graph_root_hash: missing_root,
            parent_id: None,
            applied_change_id: None,
            created_at: 0,
            verification_report_hash: None,
        };
        graph_store.save_snapshot(&e).await.expect("save");

        let report = verify_integrity(&graph_store, &obj_store)
            .await
            .expect("verify");
        assert!(!report.passed);
        assert_eq!(report.issues.len(), 1);
        assert!(
            matches!(
                &report.issues[0],
                IntegrityIssue::MissingObject { id } if *id == missing_root
            ),
            "must have MissingObject for graph_root_hash"
        );
    }

    // Scenario: empty store passes integrity check.
    #[tokio::test]
    async fn empty_store_passes() {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
        let report = verify_integrity(&graph_store, &obj_store)
            .await
            .expect("verify");
        assert!(report.passed);
        assert_eq!(report.snapshots_checked, 0);
    }

    // Scenario: orphaned parent_id produces OrphanedSnapshot issue.
    //   GIVEN snapshot whose parent_id points to a non-existent snapshot
    //   WHEN verify_integrity called
    //   THEN report has OrphanedSnapshot issue
    #[tokio::test]
    async fn orphaned_parent_produces_issue() {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

        // Store root object for the snapshot's graph_root_hash.
        let root_id = put_seed_object(&obj_store, 10).await;
        let ghost_parent = make_id(99); // this snapshot does not exist in store

        let e = SnapshotEnvelope {
            id: make_id(1),
            graph_root_hash: root_id,
            parent_id: Some(ghost_parent),
            applied_change_id: None,
            created_at: 0,
            verification_report_hash: None,
        };
        graph_store.save_snapshot(&e).await.expect("save");

        let report = verify_integrity(&graph_store, &obj_store)
            .await
            .expect("verify");
        assert!(!report.passed);
        let has_orphan = report.issues.iter().any(|i| {
            matches!(i, IntegrityIssue::OrphanedSnapshot { id } if *id == e.id)
        });
        assert!(has_orphan, "must have OrphanedSnapshot issue");
    }

    // Scenario: snapshot with valid parent_id passes.
    #[tokio::test]
    async fn valid_parent_chain_passes() {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

        let root1_id = put_seed_object(&obj_store, 10).await;
        let root2_id = put_seed_object(&obj_store, 20).await;

        let e1 = SnapshotEnvelope {
            id: make_id(1),
            graph_root_hash: root1_id,
            parent_id: None,
            applied_change_id: None,
            created_at: 100,
            verification_report_hash: None,
        };
        let e2 = SnapshotEnvelope {
            id: make_id(2),
            graph_root_hash: root2_id,
            parent_id: Some(e1.id), // valid parent
            applied_change_id: None,
            created_at: 200,
            verification_report_hash: None,
        };
        graph_store.save_snapshot(&e1).await.expect("save e1");
        graph_store.save_snapshot(&e2).await.expect("save e2");

        let report = verify_integrity(&graph_store, &obj_store)
            .await
            .expect("verify");
        assert!(report.passed);
        assert!(report.issues.is_empty());
    }
}
