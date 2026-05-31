use super::*;
use crate::approval::{ApprovalRecord, AssumptionRecord, AssumptionStatus};
use crate::backends::memory::MemoryObjectStore;
use crate::error::StorageResult;
use crate::graph::{GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
use crate::object::{ObjectId, ObjectStore, RawObject};
use crate::retention::EnumerableObjectStore;

#[derive(Default)]
struct FaultyObjectStore {
    ids: std::collections::BTreeSet<ObjectId>,
    objects: std::collections::BTreeMap<ObjectId, RawObject>,
}

impl FaultyObjectStore {
    fn with_listed_missing(id: ObjectId) -> Self {
        Self {
            ids: std::collections::BTreeSet::from([id]),
            objects: std::collections::BTreeMap::new(),
        }
    }

    fn with_corrupt_object(id: ObjectId, raw: RawObject) -> Self {
        Self {
            ids: std::collections::BTreeSet::from([id]),
            objects: std::collections::BTreeMap::from([(id, raw)]),
        }
    }

    fn with_object(id: ObjectId, raw: RawObject) -> Self {
        Self {
            ids: std::collections::BTreeSet::from([id]),
            objects: std::collections::BTreeMap::from([(id, raw)]),
        }
    }
}

impl ObjectStore for FaultyObjectStore {
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        Ok(ObjectId::from_bytes(&object.0))
    }

    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        Ok(self.objects.get(id).cloned())
    }

    async fn exists(&self, id: &ObjectId) -> StorageResult<bool> {
        Ok(self.objects.contains_key(id))
    }
}

#[derive(Default)]
struct DuplicateListingObjectStore {
    ids: Vec<ObjectId>,
    objects: std::collections::BTreeMap<ObjectId, RawObject>,
}

impl ObjectStore for DuplicateListingObjectStore {
    async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        Ok(ObjectId::from_bytes(&object.0))
    }

    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        Ok(self.objects.get(id).cloned())
    }

    async fn exists(&self, id: &ObjectId) -> StorageResult<bool> {
        Ok(self.objects.contains_key(id))
    }
}

impl EnumerableObjectStore for DuplicateListingObjectStore {
    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        Ok(self.objects.get(id).cloned())
    }

    async fn list_object_ids(&self) -> StorageResult<Vec<ObjectId>> {
        Ok(self.ids.clone())
    }
}

impl EnumerableObjectStore for FaultyObjectStore {
    async fn get(&self, id: &ObjectId) -> StorageResult<Option<RawObject>> {
        Ok(self.objects.get(id).cloned())
    }

    async fn list_object_ids(&self) -> StorageResult<Vec<ObjectId>> {
        Ok(self.ids.iter().copied().collect())
    }
}

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

fn make_approval(id_seed: u8, change_id_seed: u8) -> ApprovalRecord {
    ApprovalRecord {
        id: make_id(id_seed),
        subject_change_id: make_id(change_id_seed),
        canonical_change_hash: make_id(id_seed + 50),
        approver_role: "role:maintainer".to_owned(),
        approves_scope: "public_api".to_owned(),
        timestamp: 1000,
    }
}

fn make_assumption(id_seed: u8, boundary_seed: u8, status: AssumptionStatus) -> AssumptionRecord {
    AssumptionRecord {
        id: make_id(id_seed),
        boundary_id: make_id(boundary_seed),
        status,
        expires_at: None,
        owner: "team.test".to_owned(),
    }
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
        ..Default::default()
    };
    let e2 = SnapshotEnvelope {
        id: make_id(2),
        graph_root_hash: root2_id,
        parent_id: None,
        applied_change_id: None,
        created_at: 200,
        verification_report_hash: None,
        ..Default::default()
    };

    graph_store.save_snapshot(&e1).await.expect("save e1");
    graph_store.save_snapshot(&e2).await.expect("save e2");

    let report = verify_integrity(&graph_store, &obj_store, IntegrityInput::default())
        .await
        .expect("verify");
    assert!(report.passed, "report must pass");
    assert!(report.issues.is_empty());
    assert_eq!(report.snapshots_checked, 2);
}

#[tokio::test]
async fn object_store_integrity_passes_for_valid_memory_store() {
    let obj_store = MemoryObjectStore::new();
    put_seed_object(&obj_store, 10).await;
    put_seed_object(&obj_store, 20).await;

    let report = verify_object_store_integrity(&obj_store)
        .await
        .expect("verify object store");

    assert!(report.passed, "issues: {:?}", report.issues);
    assert_eq!(report.objects_checked, 2);
}

#[tokio::test]
async fn object_store_integrity_reports_listed_missing_object() {
    let missing_id = make_id(99);
    let obj_store = FaultyObjectStore::with_listed_missing(missing_id);

    let report = verify_object_store_integrity(&obj_store)
        .await
        .expect("verify object store");

    assert!(!report.passed);
    assert_eq!(report.objects_checked, 1);
    assert!(matches!(
        report.issues.as_slice(),
        [IntegrityIssue::MissingObject { id }] if *id == missing_id
    ));
}

#[tokio::test]
async fn object_store_integrity_reports_hash_mismatch() {
    let declared_id = make_id(3);
    let obj_store = FaultyObjectStore::with_corrupt_object(
        declared_id,
        RawObject(b"different bytes than declared id".to_vec()),
    );

    let report = verify_object_store_integrity(&obj_store)
        .await
        .expect("verify object store");

    assert!(!report.passed);
    assert_eq!(report.objects_checked, 1);
    assert!(matches!(
        report.issues.as_slice(),
        [IntegrityIssue::HashMismatch { id }] if *id == declared_id
    ));
}

#[tokio::test]
async fn object_store_integrity_reports_duplicate_object_entry_once() {
    let raw = RawObject(b"duplicate object".to_vec());
    let id = ObjectId::from_bytes(&raw.0);
    let obj_store = DuplicateListingObjectStore {
        ids: vec![id, id],
        objects: std::collections::BTreeMap::from([(id, raw)]),
    };

    let report = verify_object_store_integrity(&obj_store)
        .await
        .expect("verify object store");

    assert!(!report.passed);
    assert_eq!(report.objects_checked, 2);
    assert!(matches!(
        report.issues.as_slice(),
        [IntegrityIssue::DuplicateObjectEntry { id: duplicate_id }]
            if *duplicate_id == id
    ));
}

#[tokio::test]
async fn decodable_object_store_integrity_reports_corrupt_object_without_hash_mismatch() {
    let raw = RawObject(b"not valid cbor".to_vec());
    let id = ObjectId::from_bytes(&raw.0);
    let obj_store = FaultyObjectStore::with_object(id, raw);

    let report = verify_decodable_object_store_integrity::<_, SnapshotEnvelope>(&obj_store)
        .await
        .expect("verify decodable object store");

    assert!(!report.passed);
    assert_eq!(report.objects_checked, 1);
    assert!(matches!(
        report.issues.as_slice(),
        [IntegrityIssue::CorruptObject { id: corrupt_id }] if *corrupt_id == id
    ));
}

#[tokio::test]
async fn object_store_integrity_emits_stable_redacted_diagnostics() {
    let missing_id = make_id(99);
    let obj_store = FaultyObjectStore::with_listed_missing(missing_id);

    let first = verify_object_store_integrity(&obj_store)
        .await
        .expect("first verify object store");
    let second = verify_object_store_integrity(&obj_store)
        .await
        .expect("second verify object store");

    assert_eq!(first.diagnostics, second.diagnostics);
    assert_eq!(first.diagnostics.len(), 1);
    let diagnostic = &first.diagnostics[0];
    assert_eq!(diagnostic.code, "storage.cas.missing_object");
    assert_eq!(diagnostic.subject, "cas_object");
    assert!(diagnostic.fingerprint.starts_with("blake3:"));
    assert!(
        !format!("{diagnostic:?}").contains(&missing_id.to_hex()),
        "diagnostics must not expose the full object id"
    );
}

#[tokio::test]
async fn object_store_integrity_orders_issues_deterministically() {
    let missing_id = make_id(99);
    let mismatched_id = make_id(3);
    let obj_store = DuplicateListingObjectStore {
        ids: vec![mismatched_id, missing_id, mismatched_id],
        objects: std::collections::BTreeMap::from([(
            mismatched_id,
            RawObject(b"different bytes than declared id".to_vec()),
        )]),
    };

    let report = verify_object_store_integrity(&obj_store)
        .await
        .expect("verify object store");

    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "storage.cas.missing_object",
            "storage.cas.hash_mismatch",
            "storage.cas.duplicate_object_entry",
        ]
    );
    assert!(matches!(
        report.issues.as_slice(),
        [
            IntegrityIssue::MissingObject { id: first },
            IntegrityIssue::HashMismatch { id: second },
            IntegrityIssue::DuplicateObjectEntry { id: third },
        ] if *first == missing_id && *second == mismatched_id && *third == mismatched_id
    ));
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
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    let report = verify_integrity(&graph_store, &obj_store, IntegrityInput::default())
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
    let report = verify_integrity(&graph_store, &obj_store, IntegrityInput::default())
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
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    let report = verify_integrity(&graph_store, &obj_store, IntegrityInput::default())
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_orphan = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::OrphanedSnapshot { id } if *id == e.id));
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
        ..Default::default()
    };
    let e2 = SnapshotEnvelope {
        id: make_id(2),
        graph_root_hash: root2_id,
        parent_id: Some(e1.id), // valid parent
        applied_change_id: None,
        created_at: 200,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e1).await.expect("save e1");
    graph_store.save_snapshot(&e2).await.expect("save e2");

    let report = verify_integrity(&graph_store, &obj_store, IntegrityInput::default())
        .await
        .expect("verify");
    assert!(report.passed);
    assert!(report.issues.is_empty());
}

// ── Check 2: hash mismatch ────────────────────────────────────────────

// Scenario: hash mismatch produces HashMismatch issue.
//   GIVEN an object whose declared id does not match its bytes' BLAKE3
//   WHEN verify_integrity with objects_to_verify containing this pair
//   THEN report has HashMismatch issue
#[tokio::test]
async fn hash_mismatch_produces_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    // Tamper: declare id = make_id(0) but bytes = [0xFF; 32].
    let declared_id = make_id(0);
    let tampered_bytes = RawObject(vec![0xFF; 32]);

    let input = IntegrityInput {
        objects_to_verify: vec![(declared_id, tampered_bytes)],
        ..Default::default()
    };

    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_mismatch = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::HashMismatch { id } if *id == declared_id));
    assert!(has_mismatch, "must have HashMismatch issue");
}

// Scenario: object whose bytes match id does NOT produce HashMismatch.
#[tokio::test]
async fn correct_hash_does_not_produce_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let bytes = vec![0x42; 32];
    let correct_id = ObjectId::from_bytes(&bytes);
    let input = IntegrityInput {
        objects_to_verify: vec![(correct_id, RawObject(bytes))],
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(report.passed);
}

// ── Check 4: changes link to reports ──────────────────────────────────

// Scenario: ChangeSet with no linked report produces ChangeMissingReport.
//   GIVEN snapshot with applied_change_id = CS
//   AND change_report_index does not contain CS
//   WHEN verify_integrity
//   THEN ChangeMissingReport for CS
#[tokio::test]
async fn change_missing_report_produces_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let cs_id = make_id(50);
    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: Some(cs_id),
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    // Empty change_report_index → CS has no report.
    let report = verify_integrity(&graph_store, &obj_store, IntegrityInput::default())
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_issue = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::ChangeMissingReport { id } if *id == cs_id));
    assert!(has_issue, "must have ChangeMissingReport");
}

// Scenario: ChangeSet with linked report does NOT produce issue.
#[tokio::test]
async fn change_with_report_passes() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let cs_id = make_id(50);
    let report_id = make_id(60);
    let artifact_id = make_id(70);

    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: Some(cs_id),
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    let input = IntegrityInput {
        change_report_index: vec![(cs_id, report_id)],
        report_artifact_index: vec![(report_id, artifact_id)],
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(report.passed, "issues: {:?}", report.issues);
}

// ── Check 5: reports link to artifact hashes ──────────────────────────

// Scenario: report with no artifact hash produces ReportMissingArtifact.
#[tokio::test]
async fn report_missing_artifact_produces_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let cs_id = make_id(50);
    let report_id = make_id(60);

    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: Some(cs_id),
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    // change → report, but no artifact for the report.
    let input = IntegrityInput {
        change_report_index: vec![(cs_id, report_id)],
        report_artifact_index: vec![], // empty
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_issue = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::ReportMissingArtifact { id } if *id == report_id));
    assert!(has_issue, "must have ReportMissingArtifact");
}

// ── Check 6: approvals reference canonical changes ────────────────────

// Scenario: approval referencing unknown ChangeSet produces ApprovalOrphanedChange.
//   GIVEN approval with subject_change_id = CS2
//   AND the store has no snapshot with applied_change_id = CS2
//   WHEN verify_integrity
//   THEN ApprovalOrphanedChange for the approval
#[tokio::test]
async fn approval_orphaned_change_produces_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    // No snapshots with changeset ids.

    let approval = make_approval(1, 99); // subject_change_id = make_id(99), not in store
    let input = IntegrityInput {
        approvals: vec![approval.clone()],
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_issue = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::ApprovalOrphanedChange { id } if *id == approval.id));
    assert!(has_issue, "must have ApprovalOrphanedChange");
}

// Scenario: approval referencing known ChangeSet passes.
#[tokio::test]
async fn approval_with_valid_change_passes() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let cs_id = make_id(50);
    let report_id = make_id(60);
    let artifact_id = make_id(70);

    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: Some(cs_id),
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    // Approval references cs_id (which is in the snapshot).
    let approval = make_approval(1, 50); // subject_change_id = make_id(50) = cs_id
    let input = IntegrityInput {
        change_report_index: vec![(cs_id, report_id)],
        report_artifact_index: vec![(report_id, artifact_id)],
        approvals: vec![approval],
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(report.passed, "issues: {:?}", report.issues);
}

// ── Check 7: assumptions link to boundaries ───────────────────────────

// Scenario: assumption with unknown boundary_id produces AssumptionOrphanedBoundary.
#[tokio::test]
async fn assumption_orphaned_boundary_produces_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let assumption = make_assumption(1, 99, AssumptionStatus::Active);
    // known_boundary_ids does not contain make_id(99).
    let input = IntegrityInput {
        assumptions: vec![assumption.clone()],
        known_boundary_ids: vec![], // empty
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_issue = report.issues.iter().any(
        |i| matches!(i, IntegrityIssue::AssumptionOrphanedBoundary { id } if *id == assumption.id),
    );
    assert!(has_issue, "must have AssumptionOrphanedBoundary");
}

// Scenario: assumption with known boundary_id passes.
#[tokio::test]
async fn assumption_with_valid_boundary_passes() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let assumption = make_assumption(1, 50, AssumptionStatus::Active);
    let input = IntegrityInput {
        assumptions: vec![assumption],
        known_boundary_ids: vec![make_id(50)],
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(report.passed, "issues: {:?}", report.issues);
}

// ── Check 8: indexes match snapshot or are marked stale ───────────────

// Scenario: index entry with wrong root hash produces StaleIndex.
//   GIVEN snapshot with graph_root_hash = R1
//   AND index_entries contains (IX, R2) where R2 != R1
//   WHEN verify_integrity
//   THEN StaleIndex for IX
#[tokio::test]
async fn stale_index_produces_issue() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: None,
        created_at: 100,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    let index_id = make_id(200);
    let wrong_root = make_id(99); // not root_id
    let input = IntegrityInput {
        index_entries: vec![(index_id, wrong_root)],
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(!report.passed);
    let has_stale = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::StaleIndex { id } if *id == index_id));
    assert!(has_stale, "must have StaleIndex issue");
}

#[tokio::test]
async fn duplicate_index_entry_produces_redacted_diagnostic() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    let index_id = make_id(200);
    let index_root = make_id(99);

    let input = IntegrityInput {
        index_entries: vec![(index_id, index_root), (index_id, index_root)],
        ..Default::default()
    };

    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");

    assert!(!report.passed);
    assert!(matches!(
        report.issues.as_slice(),
        [IntegrityIssue::DuplicateIndexEntry { id }, IntegrityIssue::StaleIndex { .. }]
            if *id == index_id
    ));
    let duplicate = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "storage.index.duplicate_entry")
        .expect("duplicate index diagnostic");
    assert_eq!(duplicate.subject, "index_entry");
    assert!(
        !format!("{duplicate:?}").contains(&index_id.to_hex()),
        "diagnostics must not expose the full index id"
    );
}

// Scenario: index marked as stale is exempt from StaleIndex check.
//   GIVEN index_entries with wrong root but index_id in stale_index_ids
//   WHEN verify_integrity
//   THEN no StaleIndex issue
#[tokio::test]
async fn stale_marked_index_is_exempt() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: None,
        created_at: 100,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    let index_id = make_id(200);
    let wrong_root = make_id(99);
    let input = IntegrityInput {
        index_entries: vec![(index_id, wrong_root)],
        stale_index_ids: vec![index_id], // explicitly marked stale
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    // No StaleIndex for the exempt index.
    let has_stale = report
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::StaleIndex { id } if *id == index_id));
    assert!(
        !has_stale,
        "exempt stale index must not produce StaleIndex issue"
    );
}

// Scenario: index with correct root hash passes.
#[tokio::test]
async fn index_with_correct_root_passes() {
    let obj_store = MemoryObjectStore::new();
    let graph_store = ObjectBackedGraphStore::new(MemoryObjectStore::new());

    let root_id = put_seed_object(&obj_store, 10).await;
    let e = SnapshotEnvelope {
        id: make_id(1),
        graph_root_hash: root_id,
        parent_id: None,
        applied_change_id: None,
        created_at: 100,
        verification_report_hash: None,
        ..Default::default()
    };
    graph_store.save_snapshot(&e).await.expect("save");

    let index_id = make_id(200);
    let input = IntegrityInput {
        index_entries: vec![(index_id, root_id)], // correct root
        ..Default::default()
    };
    let report = verify_integrity(&graph_store, &obj_store, input)
        .await
        .expect("verify");
    assert!(report.passed, "issues: {:?}", report.issues);
}
