// ── ail-dogfood::tests::changeset_self ───────────────────────────────────
//
// Integration tests for `build_changeset_self()`.
//
// # Coverage
//
// - The returned ChangeSet is a valid value (non-empty ops, correct description,
//   defined base_snapshot_id)
// - CBOR round-trip: encode → decode → re-encode → byte-identical
// - Applying with a mismatched snapshot id returns `RebaseRequired` (not panic)

use ail_change::apply::{SnapshotBridge, apply};
use ail_change::canonical::canonicalize;
use ail_change::model::{ChangeSetOp, ChangeSetOutcome, SnapshotId};
use ail_core::semantic_graph::SemanticGraph;
use ail_dogfood::changeset_self::build_changeset_self;
use ail_storage::codec::{CborCodec, ContentCodec};

// ── Minimal SnapshotBridge stub ───────────────────────────────────────────

struct FixedSnapshot(SnapshotId);

impl SnapshotBridge for FixedSnapshot {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

// ── changeset_self_is_valid_value ─────────────────────────────────────────
// Spec scenario: "Self-describing ChangeSet is a valid value"
//   GIVEN build_changeset_self() is called
//   WHEN the resulting ChangeSet is inspected
//   THEN ops is non-empty and contains ChangeSetOp::Create
//   AND meta.description contains the string "ChangeSet"
//   AND base_snapshot_id is a defined SnapshotId
#[test]
fn changeset_self_is_valid_value() {
    let cs = build_changeset_self();

    assert!(!cs.ops.is_empty(), "ops must be non-empty; got empty vec");
    assert!(
        cs.ops.contains(&ChangeSetOp::Create),
        "ops must contain ChangeSetOp::Create; got {:?}",
        cs.ops
    );
    assert!(
        cs.meta.description.contains("ChangeSet"),
        "meta.description must contain \"ChangeSet\"; got {:?}",
        cs.meta.description
    );
    // SnapshotId is a defined value — any SnapshotId(n) qualifies
    let _snapshot_id: SnapshotId = cs.base_snapshot_id;
}

// ── changeset_self_cbor_round_trips ──────────────────────────────────────
// Spec scenario: "Self-describing ChangeSet CBOR round-trips"
//   GIVEN the self-describing ChangeSet is encoded to CBOR
//   WHEN decoded and re-encoded
//   THEN the output bytes are identical
#[test]
fn changeset_self_cbor_round_trips() {
    use ail_change::model::ChangeSet;

    let codec = CborCodec;
    let cs = build_changeset_self();

    let bytes_a = codec.encode(&cs).expect("first encode must succeed");
    let decoded: ChangeSet = codec.decode(&bytes_a).expect("decode must succeed");
    let bytes_b = codec.encode(&decoded).expect("re-encode must succeed");

    assert_eq!(
        bytes_a, bytes_b,
        "CBOR round-trip must produce byte-identical output"
    );
}

// ── changeset_self_apply_with_mismatched_snapshot_returns_rebase_required ─
// Spec scenario: "Self-description outcome maps to applied or rebase-required"
//   GIVEN the self-describing ChangeSet is passed to apply_changeset()
//   WHEN the current graph snapshot differs from base_snapshot_id
//   THEN outcome is ChangeSetOutcome::RebaseRequired (not a panic or error)
#[test]
fn changeset_self_apply_with_mismatched_snapshot_returns_rebase_required() {
    let cs = build_changeset_self();
    let base_id = cs.base_snapshot_id;

    // Canonicalize the raw ChangeSet for use with apply()
    let canonical = canonicalize(cs);

    // Use a snapshot id that is guaranteed to differ from base_id
    let live_id = SnapshotId(base_id.0.wrapping_add(1));
    let bridge = FixedSnapshot(live_id);

    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };

    let outcome = apply(canonical, &mut graph, &bridge);

    match outcome {
        ChangeSetOutcome::RebaseRequired {
            current_snapshot_id,
        } => {
            assert_eq!(
                current_snapshot_id, live_id,
                "RebaseRequired must carry the live snapshot id"
            );
        }
        other => panic!("expected RebaseRequired; got {:?}", other),
    }
}
