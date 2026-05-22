// ── ail-coordinator integration tests ─────────────────────────────────────
//
// Tests the full coordinator lifecycle: sequential applies, non-conflicting
// concurrent rebases, and irresolvable conflicts.
//
// All tests use `#[tokio::test]` for async execution.
//
// # Spec coverage
//
// | Test | Spec scenario |
// |------|---------------|
// | `sequential_applies_both_succeed` | Sequential Serialization |
// | `non_conflicting_concurrent_both_apply` | Semantic Rebase for Non-Conflicting |
// | `irresolvable_conflict_same_node_modified` | Deterministic Conflict Classification — same node modified |
// | `irresolvable_conflict_node_deleted_while_modified` | Deterministic Conflict Classification — node deleted |

use ail_change::{
    canonical::{CanonicalChangeSet, CanonicalMeta, CanonicalOp, OpPayload},
    model::{BlockHash, ChangeSetOp, SnapshotId, Timestamp},
};
use ail_coordinator::{
    conflict::ConflictReason,
    coordinator::{Coordinator, CoordinatorOutcome},
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};

// ── helpers ───────────────────────────────────────────────────────────────

fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

fn dummy_hash() -> BlockHash {
    BlockHash([0u8; 32])
}

fn meta(author: &str) -> CanonicalMeta {
    CanonicalMeta {
        author: author.into(),
        description: "integration test changeset".into(),
        timestamp: Timestamp(0),
    }
}

fn create_node_op(node_ref: u32, name: &str) -> CanonicalOp {
    CanonicalOp {
        kind: ChangeSetOp::Create,
        payload: OpPayload::CreateNode(Box::new(GraphNode::new(
            NodeRef(node_ref),
            NodeKind::Function,
            name,
        ))),
        block_hash: dummy_hash(),
    }
}

fn remove_node_op(node_ref: u32) -> CanonicalOp {
    CanonicalOp {
        kind: ChangeSetOp::Remove,
        payload: OpPayload::RemoveNode(NodeRef(node_ref)),
        block_hash: dummy_hash(),
    }
}

fn rename_node_op(node_ref: u32, new_name: &str) -> CanonicalOp {
    CanonicalOp {
        kind: ChangeSetOp::Set,
        payload: OpPayload::SetNodeName {
            node_id: NodeRef(node_ref),
            name: new_name.into(),
        },
        block_hash: dummy_hash(),
    }
}

fn cs(base: u64, author: &str, ops: Vec<CanonicalOp>) -> CanonicalChangeSet {
    CanonicalChangeSet {
        meta: meta(author),
        base_snapshot_id: SnapshotId(base),
        preconditions: vec![],
        ops,
    }
}

// ── Task 3.2: Sequential applies — both Applied; snapshot id = 2 after both
//
// Spec: Sequential Serialization
//   GIVEN Coordinator initialized with SnapshotId(0) and empty graph
//   WHEN agent A submits cs with base_snapshot_id = SnapshotId(0) → Applied, live = 1
//   AND  agent B submits cs with base_snapshot_id = SnapshotId(1) → Applied, live = 2
//   THEN snapshot id is 2 after both
#[tokio::test]
async fn sequential_applies_both_succeed() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());

    // Agent A: creates fn.cart_total
    let cs_a = cs(0, "agent_a", vec![create_node_op(1, "fn.cart_total")]);
    let outcome_a = coord.submit(cs_a).await;
    assert!(
        matches!(
            outcome_a,
            CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(1)
            }
        ),
        "agent A must get Applied{{applied_snapshot_id: SnapshotId(1)}}; got {outcome_a:?}"
    );

    // Agent B: creates fn.checkout with base = SnapshotId(1)
    let cs_b = cs(1, "agent_b", vec![create_node_op(2, "fn.checkout")]);
    let outcome_b = coord.submit(cs_b).await;
    assert!(
        matches!(
            outcome_b,
            CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(2)
            }
        ),
        "agent B must get Applied{{applied_snapshot_id: SnapshotId(2)}}; got {outcome_b:?}"
    );
}

// ── Task 3.3: Non-conflicting concurrent ChangeSets — disjoint NodeRefs
//
// Spec: Semantic Rebase for Non-Conflicting
//   GIVEN agent A added fn.cart_total (live id now SnapshotId(1))
//   WHEN agent B submits with base_snapshot_id = SnapshotId(0) adding fn.checkout
//   THEN coordinator rebases B onto SnapshotId(1)
//   AND outcome is RebaseApplied { rebased_onto: SnapshotId(1), applied_snapshot_id: SnapshotId(2) }
#[tokio::test]
async fn non_conflicting_concurrent_both_apply() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());

    // Agent A applies first.
    let cs_a = cs(0, "agent_a", vec![create_node_op(1, "fn.cart_total")]);
    let outcome_a = coord.submit(cs_a).await;
    assert!(
        matches!(outcome_a, CoordinatorOutcome::Applied { .. }),
        "agent A must apply cleanly; got {outcome_a:?}"
    );

    // Agent B was at base 0 but A already applied — disjoint NodeRef(2).
    let cs_b = cs(0, "agent_b", vec![create_node_op(2, "fn.checkout")]);
    let outcome_b = coord.submit(cs_b).await;
    assert!(
        matches!(
            outcome_b,
            CoordinatorOutcome::RebaseApplied {
                rebased_onto: SnapshotId(1),
                applied_snapshot_id: SnapshotId(2),
            }
        ),
        "agent B must get RebaseApplied{{rebased_onto: SnapshotId(1), applied_snapshot_id: SnapshotId(2)}}; got {outcome_b:?}"
    );
}

// ── Task 3.4: Irresolvable conflict — same NodeRef modified by both agents
//
// Spec: Conflicting ops on same NodeRef are irresolvable
//   GIVEN agent A renamed fn.cart_total (live id now SnapshotId(1))
//   WHEN agent B submits with base_snapshot_id = SnapshotId(0) renaming fn.cart_total
//   THEN outcome is ConflictIrresolvable { SameNodeModifiedIncompatibly }
//   AND live snapshot id does NOT advance (remains SnapshotId(1))
#[tokio::test]
async fn irresolvable_conflict_same_node_modified() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());

    // Seed the graph: create fn.cart_total so renaming it is valid.
    let seed = cs(0, "seed", vec![create_node_op(1, "fn.cart_total")]);
    coord.submit(seed).await;
    // live id = 1

    // Agent A renames fn.cart_total.
    let cs_a = cs(1, "agent_a", vec![rename_node_op(1, "fn.cart_total_v2")]);
    let outcome_a = coord.submit(cs_a).await;
    assert!(
        matches!(
            outcome_a,
            CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(2)
            }
        ),
        "agent A must apply cleanly; got {outcome_a:?}"
    );
    // live id = 2

    // Agent B: stale base (0 or 1), also renames NodeRef(1).
    let cs_b = cs(
        1,
        "agent_b",
        vec![rename_node_op(1, "fn.cart_total_renamed_by_b")],
    );
    let outcome_b = coord.submit(cs_b).await;
    assert!(
        matches!(
            outcome_b,
            CoordinatorOutcome::ConflictIrresolvable {
                reason: ConflictReason::SameNodeModifiedIncompatibly
            }
        ),
        "agent B must get ConflictIrresolvable{{SameNodeModifiedIncompatibly}}; got {outcome_b:?}"
    );
}

// ── Task 3.5: Node deleted while modified by another agent
//
// Spec: Node deleted while another agent modifies it
//   GIVEN agent A deleted fn.old_checkout (live id = SnapshotId(2))
//   WHEN agent B submits with base_snapshot_id = SnapshotId(0) renaming fn.old_checkout
//   THEN outcome is ConflictIrresolvable { NodeDeletedWhileModified }
#[tokio::test]
async fn irresolvable_conflict_node_deleted_while_modified() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());

    // Seed: create fn.old_checkout.
    let seed = cs(0, "seed", vec![create_node_op(5, "fn.old_checkout")]);
    coord.submit(seed).await;
    // live id = 1

    // Agent A: remove fn.old_checkout.
    let cs_a = cs(1, "agent_a", vec![remove_node_op(5)]);
    let outcome_a = coord.submit(cs_a).await;
    assert!(
        matches!(outcome_a, CoordinatorOutcome::Applied { .. }),
        "agent A must apply cleanly; got {outcome_a:?}"
    );
    // live id = 2

    // Agent B: stale base 1, renames fn.old_checkout (which was deleted by A).
    let cs_b = cs(
        1,
        "agent_b",
        vec![rename_node_op(5, "fn.old_checkout_renamed")],
    );
    let outcome_b = coord.submit(cs_b).await;
    assert!(
        matches!(
            outcome_b,
            CoordinatorOutcome::ConflictIrresolvable {
                reason: ConflictReason::NodeDeletedWhileModified
            }
        ),
        "agent B must get ConflictIrresolvable{{NodeDeletedWhileModified}}; got {outcome_b:?}"
    );
}
