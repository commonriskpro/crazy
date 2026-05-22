// ── ail-change: apply tests ───────────────────────────────────────────────
//
// Strict TDD — RED phase.
// All tests reference types in `ail_change::apply` that do NOT exist yet.
// Compilation failure here is the expected RED signal.

use ail_change::{
    apply::{SnapshotBridge, apply},
    canonical::{CanonicalChangeSet, CanonicalMeta, CanonicalOp, OpPayload, Precondition},
    model::{AssertExists, BlockHash, ChangeSetOp, ChangeSetOutcome, SnapshotId, Timestamp},
};
use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};

// ── helpers ───────────────────────────────────────────────────────────────

/// Simple in-memory snapshot bridge for tests.
struct MockBridge(SnapshotId);

impl SnapshotBridge for MockBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

fn canonical_meta() -> CanonicalMeta {
    CanonicalMeta {
        author: "alice".to_string(),
        description: "test changeset".to_string(),
        timestamp: Timestamp(0),
    }
}

fn graph_node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), kind, name)
}

fn graph_edge(source: u32, target: u32, kind: EdgeKind) -> GraphEdge {
    GraphEdge {
        source: NodeRef(source),
        target: NodeRef(target),
        kind,
    }
}

fn create_op(node: GraphNode) -> CanonicalOp {
    CanonicalOp {
        kind: ChangeSetOp::Create,
        payload: OpPayload::CreateNode(Box::new(node)),
        block_hash: BlockHash([1u8; 32]),
    }
}

fn connect_op(edge: GraphEdge) -> CanonicalOp {
    CanonicalOp {
        kind: ChangeSetOp::Connect,
        payload: OpPayload::AddEdge(edge),
        block_hash: BlockHash([2u8; 32]),
    }
}

fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

fn single_node_graph(id: u32) -> SemanticGraph {
    SemanticGraph {
        nodes: vec![graph_node(id, NodeKind::Module, "existing")],
        edges: vec![],
    }
}

// ── Scenario: matching base returns Applied ───────────────────────────────
// GIVEN a canonical changeset whose base_snapshot_id matches bridge.current_snapshot_id()
// AND the ops include a CreateNode and a ConnectNodes
// WHEN apply is called
// THEN the outcome is Applied and the graph reflects the applied ops
#[test]
fn matching_base_snapshot_returns_applied_and_graph_reflects_ops() {
    let mut graph = empty_graph();
    let bridge = MockBridge(SnapshotId(1));

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(1),
        preconditions: vec![],
        ops: vec![
            create_op(graph_node(0, NodeKind::Module, "mod_a")),
            create_op(graph_node(1, NodeKind::Function, "fn_b")),
            connect_op(graph_edge(0, 1, EdgeKind::DependsOn)),
        ],
    };

    let outcome = apply(cs, &mut graph, &bridge);

    assert_eq!(outcome, ChangeSetOutcome::Applied);
    assert_eq!(
        graph.nodes.len(),
        2,
        "graph must contain both created nodes"
    );
    assert_eq!(
        graph.edges.len(),
        1,
        "graph must contain the connected edge"
    );
}

// ── TRIANGULATE: empty ops with matching base also returns Applied ─────────
// Proves the snapshot guard succeeds independently of op count.
#[test]
fn matching_base_with_no_ops_returns_applied() {
    let mut graph = empty_graph();
    let bridge = MockBridge(SnapshotId(5));

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(5),
        preconditions: vec![],
        ops: vec![],
    };

    let outcome = apply(cs, &mut graph, &bridge);
    assert_eq!(outcome, ChangeSetOutcome::Applied);
}

// ── Scenario: stale base returns RebaseRequired ───────────────────────────
// GIVEN a canonical changeset whose base_snapshot_id DOES NOT match bridge.current_snapshot_id()
// WHEN apply is called
// THEN the outcome is RebaseRequired carrying the live snapshot id
// AND the graph is NOT modified
#[test]
fn stale_base_snapshot_returns_rebase_required_with_graph_unmodified() {
    let mut graph = empty_graph();
    let bridge = MockBridge(SnapshotId(99)); // live = 99

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(1), // stale: 1 ≠ 99
        preconditions: vec![],
        ops: vec![create_op(graph_node(
            0,
            NodeKind::Module,
            "should_not_appear",
        ))],
    };

    let pre_graph = graph.clone();
    let outcome = apply(cs, &mut graph, &bridge);

    assert_eq!(
        outcome,
        ChangeSetOutcome::RebaseRequired {
            current_snapshot_id: SnapshotId(99),
        }
    );
    assert_eq!(
        graph, pre_graph,
        "graph must be unmodified after stale-base rejection"
    );
}

// ── TRIANGULATE: zero-snapshot ids also guard correctly ───────────────────
// Ensures the guard is not special-cased for zero.
#[test]
fn stale_base_zero_vs_nonzero_also_returns_rebase_required() {
    let mut graph = empty_graph();
    let bridge = MockBridge(SnapshotId(0));

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(1), // 1 ≠ 0
        preconditions: vec![],
        ops: vec![],
    };

    let outcome = apply(cs, &mut graph, &bridge);
    assert!(matches!(
        outcome,
        ChangeSetOutcome::RebaseRequired {
            current_snapshot_id: SnapshotId(0)
        }
    ));
}

// ── Scenario: mid-apply op failure triggers rollback ─────────────────────
// GIVEN a canonical changeset where op K (the 3rd of 3) creates a duplicate NodeRef
// WHEN apply is called
// THEN the outcome is Failed
// AND the graph is restored to its pre-apply state (clone-before-apply rollback)
#[test]
fn mid_apply_op_failure_returns_failed_and_graph_is_rolled_back() {
    let mut graph = empty_graph();
    let bridge = MockBridge(SnapshotId(1));

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(1),
        preconditions: vec![],
        ops: vec![
            create_op(graph_node(1, NodeKind::Module, "mod_a")),
            create_op(graph_node(2, NodeKind::Function, "fn_b")),
            // Op 3: duplicate NodeRef(1) — violates graph invariants
            create_op(graph_node(1, NodeKind::Type, "duplicate_id")),
        ],
    };

    let pre_apply_graph = graph.clone();
    let outcome = apply(cs, &mut graph, &bridge);

    assert!(
        matches!(outcome, ChangeSetOutcome::Failed { .. }),
        "mid-apply failure must produce Failed outcome"
    );
    assert_eq!(
        graph, pre_apply_graph,
        "graph must be rolled back to pre-apply state"
    );
}

// ── Scenario: AssertExists on missing node triggers rollback ──────────────
// GIVEN a canonical changeset with a precondition AssertExists for a node NOT in the graph
// WHEN apply is called
// THEN the outcome is Failed
// AND the graph is unmodified
#[test]
fn assert_exists_on_missing_node_triggers_failed_and_rollback() {
    let mut graph = empty_graph(); // NodeRef(99) does NOT exist
    let bridge = MockBridge(SnapshotId(1));

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(1),
        preconditions: vec![Precondition::AssertExists(AssertExists {
            node_id: NodeRef(99),
        })],
        ops: vec![create_op(graph_node(
            0,
            NodeKind::Module,
            "should_not_be_created",
        ))],
    };

    let pre_graph = graph.clone();
    let outcome = apply(cs, &mut graph, &bridge);

    assert!(
        matches!(outcome, ChangeSetOutcome::Failed { .. }),
        "AssertExists on missing node must produce Failed outcome"
    );
    assert_eq!(
        graph, pre_graph,
        "graph must be unmodified after precondition failure"
    );
}

// ── TRIANGULATE: AssertExists on PRESENT node allows apply to proceed ─────
// Confirms the precondition passes when the node IS in the graph.
#[test]
fn assert_exists_on_present_node_allows_apply_to_proceed() {
    // Graph already has NodeRef(0)
    let mut graph = single_node_graph(0);
    let bridge = MockBridge(SnapshotId(1));

    let cs = CanonicalChangeSet {
        meta: canonical_meta(),
        base_snapshot_id: SnapshotId(1),
        preconditions: vec![Precondition::AssertExists(AssertExists {
            node_id: NodeRef(0),
        })],
        ops: vec![create_op(graph_node(1, NodeKind::Function, "fn_new"))],
    };

    let outcome = apply(cs, &mut graph, &bridge);

    assert_eq!(outcome, ChangeSetOutcome::Applied);
    assert_eq!(graph.nodes.len(), 2, "new node must have been added");
}
