// ── ail-change::apply ─────────────────────────────────────────────────────
//
// Atomic apply of a `CanonicalChangeSet` to a `SemanticGraph`.
//
// # Protocol
//
// 1. **Snapshot guard**: compare `cs.base_snapshot_id` against
//    `bridge.current_snapshot_id()`.  A mismatch returns
//    `ChangeSetOutcome::RebaseRequired` immediately, with no graph mutation.
//
// 2. **Clone-before-apply**: the graph is cloned before any mutation.
//    Any failure at any point restores the clone (rollback).
//
// 3. **Precondition evaluation**: `AssertExists` and `AssertHash` preconditions
//    are checked before the first op is applied.  Failure → `Failed` + rollback.
//
// 4. **Op application**: each `CanonicalOp` mutates the graph via its payload.
//    After each op `graph.validate()` is called; the first error → `Failed` +
//    rollback.
//
// 5. **Success**: all preconditions pass and all ops apply cleanly →
//    `ChangeSetOutcome::Applied`.

use ail_core::semantic_graph::SemanticGraph;

use crate::{
    canonical::{CanonicalChangeSet, OpPayload, Precondition},
    model::{BlockHash, ChangeSetOutcome, SnapshotId},
};

// ── SnapshotBridge ────────────────────────────────────────────────────────

/// Abstraction over the storage layer for snapshot-identity checks.
///
/// The implementation is expected to be cheap — typically a single integer
/// read from an in-memory field or an atomic counter.
pub trait SnapshotBridge {
    /// Return the current (live) snapshot id of the graph being modified.
    fn current_snapshot_id(&self) -> SnapshotId;
}

// ── apply ─────────────────────────────────────────────────────────────────

/// Atomically apply a `CanonicalChangeSet` to a `SemanticGraph`.
///
/// Returns:
/// - `Applied` — all preconditions passed and all ops were applied cleanly.
/// - `RebaseRequired { current_snapshot_id }` — `cs.base_snapshot_id` does
///   not match the live snapshot id.  Graph is unmodified.
/// - `Failed { reason }` — a precondition failed or an op violated a graph
///   invariant.  Graph is restored to its pre-apply state (rollback).
pub fn apply(
    cs: CanonicalChangeSet,
    graph: &mut SemanticGraph,
    bridge: &dyn SnapshotBridge,
) -> ChangeSetOutcome {
    // ── Step 1: Snapshot guard ────────────────────────────────────────────
    let live_id = bridge.current_snapshot_id();
    if cs.base_snapshot_id != live_id {
        return ChangeSetOutcome::RebaseRequired {
            current_snapshot_id: live_id,
        };
    }

    // ── Step 2: Clone before any mutation ────────────────────────────────
    let rollback = graph.clone();

    // ── Step 3: Evaluate preconditions ───────────────────────────────────
    for precondition in &cs.preconditions {
        if let Some(reason) = evaluate_precondition(precondition, graph) {
            *graph = rollback;
            return ChangeSetOutcome::Failed { reason };
        }
    }

    // ── Step 4: Apply ops ─────────────────────────────────────────────────
    for op in &cs.ops {
        apply_payload(&op.payload, graph);

        if let Err(err) = graph.validate() {
            *graph = rollback;
            return ChangeSetOutcome::Failed {
                reason: format!("graph invariant violated: {err:?}"),
            };
        }
    }

    // ── Step 5: Success ───────────────────────────────────────────────────
    ChangeSetOutcome::Applied
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Evaluate a single precondition against the current graph state.
///
/// Returns `None` if the precondition passes, or `Some(reason)` if it fails.
fn evaluate_precondition(precondition: &Precondition, graph: &SemanticGraph) -> Option<String> {
    match precondition {
        Precondition::AssertExists(assert) => {
            let exists = graph.nodes.iter().any(|n| n.id == assert.node_id);
            if exists {
                None
            } else {
                Some(format!(
                    "AssertExists failed: node {:?} not found in graph",
                    assert.node_id
                ))
            }
        }
        Precondition::AssertHash(assert) => {
            // Minimal implementation: compute blake3 of the node's CBOR encoding
            // and compare against the expected hash.
            let node = graph.nodes.iter().find(|n| n.id == assert.node_id);
            match node {
                None => Some(format!(
                    "AssertHash failed: node {:?} not found in graph",
                    assert.node_id
                )),
                Some(n) => {
                    let computed = compute_node_hash(n);
                    if computed == assert.expected_hash {
                        None
                    } else {
                        Some(format!(
                            "AssertHash failed: node {:?} hash mismatch",
                            assert.node_id
                        ))
                    }
                }
            }
        }
    }
}

/// Apply an `OpPayload` to the graph (no validation — caller validates after).
fn apply_payload(payload: &OpPayload, graph: &mut SemanticGraph) {
    match payload {
        OpPayload::CreateNode(node) => {
            graph.nodes.push((**node).clone());
        }
        OpPayload::AddEdge(edge) => {
            graph.edges.push(edge.clone());
        }
        OpPayload::RemoveNode(node_id) => {
            graph.nodes.retain(|n| &n.id != node_id);
            // Also remove dangling edges whose source or target was this node.
            graph
                .edges
                .retain(|e| &e.source != node_id && &e.target != node_id);
        }
        OpPayload::SetNodeName { node_id, name } => {
            if let Some(n) = graph.nodes.iter_mut().find(|n| &n.id == node_id) {
                n.name.clone_from(name);
            }
        }
        OpPayload::Noop => {
            // Intentional no-op: Infer/Verify and raw-ChangeSet-derived ops.
        }
    }
}

/// Compute blake3 hash of a node's CBOR encoding for `AssertHash` checks.
fn compute_node_hash(node: &ail_core::semantic_graph::GraphNode) -> BlockHash {
    let mut bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(node, &mut bytes).expect("GraphNode serialization must not fail");
    BlockHash(*blake3::hash(&bytes).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::canonicalize_parsed, parser::parse_changeset};
    use ail_core::semantic_graph::{NodeKind, SemanticGraph};

    struct TestBridge;

    impl SnapshotBridge for TestBridge {
        fn current_snapshot_id(&self) -> SnapshotId {
            SnapshotId(0)
        }
    }

    #[test]
    fn apply_parsed_function_create_payload_inserts_graph_node() {
        let parsed = parse_changeset(
            "change e2e base=0\nauthor tester\ndescription e2e\nop create_function id=fn.answer return=Int value=42\nend\n",
        )
        .expect("fixture must parse");
        let canonical = canonicalize_parsed(parsed);
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let outcome = apply(canonical, &mut graph, &TestBridge);

        assert_eq!(outcome, ChangeSetOutcome::Applied);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].kind, NodeKind::Function);
        assert_eq!(graph.nodes[0].name, "fn.answer");
    }
}
