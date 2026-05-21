// ── ail-change: data model tests ─────────────────────────────────────────
//
// Strict TDD — RED phase.
// All tests reference `ail_change::model` types that do NOT exist yet.
// Compilation failure here is the expected RED signal.

use ail_change::model::{
    AssertExists, AssertHash, BlockHash, ChangeSet, ChangeSetMeta, ChangeSetOp, ChangeSetOutcome,
    SnapshotId, Timestamp,
};
use ail_core::semantic_graph::NodeRef;

// ── helpers ───────────────────────────────────────────────────────────────

fn meta() -> ChangeSetMeta {
    ChangeSetMeta {
        author: "alice".to_string(),
        description: "test changeset".to_string(),
        timestamp: Timestamp(0),
    }
}

fn snapshot_id(n: u64) -> SnapshotId {
    SnapshotId(n)
}

// ── Scenario: ChangeSet with ops ──────────────────────────────────────────
// GIVEN valid meta, a snapshot id, and one or more ops
// WHEN `ChangeSet` is constructed
// THEN all fields are accessible and the value is valid
#[test]
fn changeset_with_ops_is_constructible() {
    let cs = ChangeSet {
        meta: meta(),
        base_snapshot_id: snapshot_id(1),
        ops: vec![ChangeSetOp::Create],
    };
    assert_eq!(cs.ops.len(), 1);
    assert_eq!(cs.base_snapshot_id, snapshot_id(1));
    assert_eq!(cs.meta.author, "alice");
}

// ── Scenario: ChangeSet with empty ops ───────────────────────────────────
// GIVEN valid meta and a snapshot id and no ops
// WHEN `ChangeSet` is constructed
// THEN construction succeeds without error
#[test]
fn changeset_with_empty_ops_is_valid() {
    let cs = ChangeSet {
        meta: meta(),
        base_snapshot_id: snapshot_id(0),
        ops: vec![],
    };
    assert!(cs.ops.is_empty());
}

// ── Scenario: All seven ChangeSetOp variants present ─────────────────────
// GIVEN the `ChangeSetOp` enum definition
// WHEN its variants are constructed
// THEN all seven named variants exist
#[test]
fn changeset_op_all_seven_variants_exist() {
    // Constructing each variant confirms it compiles and exists.
    let variants: Vec<ChangeSetOp> = vec![
        ChangeSetOp::Create,
        ChangeSetOp::Set,
        ChangeSetOp::Add,
        ChangeSetOp::Remove,
        ChangeSetOp::Connect,
        ChangeSetOp::Infer,
        ChangeSetOp::Verify,
    ];
    assert_eq!(variants.len(), 7);
}

// ── Scenario: ChangeSetMeta fields readable ───────────────────────────────
// GIVEN a `ChangeSetMeta` with all fields populated
// WHEN embedded in a `ChangeSet`
// THEN each meta field is readable from the outer struct
#[test]
fn changeset_meta_fields_are_readable() {
    let cs = ChangeSet {
        meta: ChangeSetMeta {
            author: "bob".to_string(),
            description: "meta test".to_string(),
            timestamp: Timestamp(42),
        },
        base_snapshot_id: snapshot_id(7),
        ops: vec![],
    };
    assert_eq!(cs.meta.author, "bob");
    assert_eq!(cs.meta.description, "meta test");
    assert_eq!(cs.meta.timestamp, Timestamp(42));
}

// ── Scenario: ChangeSetOutcome Applied / RebaseRequired / Failed ──────────
// GIVEN the enum definition
// WHEN variants are matched
// THEN Applied, RebaseRequired, and Failed all exist with expected shapes
#[test]
fn changeset_outcome_variants_exist() {
    let applied = ChangeSetOutcome::Applied;
    assert!(matches!(applied, ChangeSetOutcome::Applied));

    let rebase = ChangeSetOutcome::RebaseRequired {
        current_snapshot_id: snapshot_id(99),
    };
    match rebase {
        ChangeSetOutcome::RebaseRequired {
            current_snapshot_id,
        } => {
            assert_eq!(current_snapshot_id, snapshot_id(99));
        }
        _ => panic!("unexpected variant"),
    }

    let failed = ChangeSetOutcome::Failed {
        reason: "oops".to_string(),
    };
    match failed {
        ChangeSetOutcome::Failed { reason } => assert_eq!(reason, "oops"),
        _ => panic!("unexpected variant"),
    }
}

// ── TRIANGULATE: ChangeSetOutcome — Applied differs from RebaseRequired ───
// Ensures the enum has distinct variants (not a single-variant alias).
#[test]
fn changeset_outcome_applied_neq_rebase_required() {
    let applied = ChangeSetOutcome::Applied;
    let rebase = ChangeSetOutcome::RebaseRequired {
        current_snapshot_id: snapshot_id(1),
    };
    assert_ne!(applied, rebase);
}

// ── Scenario: AssertExists and AssertHash are constructible ───────────────
// GIVEN the assertion types
// WHEN constructed with a NodeRef
// THEN they are usable as op preconditions
#[test]
fn assert_exists_is_constructible() {
    let node = NodeRef(10);
    let assertion = AssertExists { node_id: node };
    assert_eq!(assertion.node_id, NodeRef(10));
}

#[test]
fn assert_hash_is_constructible() {
    let node = NodeRef(5);
    let hash = BlockHash([0u8; 32]);
    let assertion = AssertHash {
        node_id: node,
        expected_hash: hash.clone(),
    };
    assert_eq!(assertion.node_id, NodeRef(5));
    assert_eq!(assertion.expected_hash, hash);
}

// ── TRIANGULATE: snapshot ids with different values are not equal ─────────
#[test]
fn snapshot_ids_are_value_comparable() {
    assert_ne!(snapshot_id(0), snapshot_id(1));
    assert_eq!(snapshot_id(42), snapshot_id(42));
}
