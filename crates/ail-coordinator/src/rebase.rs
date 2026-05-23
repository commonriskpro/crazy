// ── ail-coordinator::rebase ───────────────────────────────────────────────
//
// Pure semantic rebase logic for the coordinator.
//
// # Overview
//
// `rebase()` is a pure function: it takes a stale `CanonicalChangeSet` and the
// `StructuralDiff` of ops committed since the changeset's base, and decides
// whether the changeset can be safely rebased onto the live snapshot.
//
// # Conflict rule (conservative)
//
// Any pending op that references a `NodeRef` present in `diff.touched_nodes`
// is classified as a conflict.  This is the Phase 13 conservative rule —
// relaxing it (e.g., allowing additive ops on the same node) is future work.
//
// # NodeRef extraction
//
// Ops are inspected via their `OpPayload` variant.  `Noop` and name-based
// payloads carry no `NodeRef` and are invisible to conflict detection.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use ail_change::canonical::{CanonicalChangeSet, CanonicalOp, OpPayload};
use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};
use serde::{Deserialize, Serialize};

use crate::conflict::ConflictReason;

// ── StructuralDiff ────────────────────────────────────────────────────────

/// The set of `NodeRef`s touched by ops already committed to the live snapshot.
///
/// Built from the ops of the most-recently applied `CanonicalChangeSet` so
/// the coordinator can detect whether a stale pending changeset conflicts.
///
/// An empty `touched_nodes` means the committed ops carried no meaningful graph
/// mutations (e.g., all `Noop` payloads) — rebase will always succeed in that
/// case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralDiff {
    /// `NodeRef`s that were created, removed, renamed, or had edges added by
    /// the committed ops.
    pub touched_nodes: BTreeSet<NodeRef>,
}

impl StructuralDiff {
    /// Build a `StructuralDiff` by extracting `NodeRef`s from a slice of ops.
    ///
    /// Recognises: `CreateNode`, `RemoveNode`, `SetNodeName`, `AddEdge`.
    /// `Noop` is skipped — it carries no graph-identity information.
    pub fn from_ops(ops: &[CanonicalOp]) -> Self {
        let mut touched_nodes = BTreeSet::new();
        for op in ops {
            extract_node_refs(&op.payload, &mut touched_nodes);
        }
        Self { touched_nodes }
    }
}

/// Extract all `NodeRef`s referenced by an `OpPayload` into `out`.
fn extract_node_refs(payload: &OpPayload, out: &mut BTreeSet<NodeRef>) {
    match payload {
        OpPayload::CreateNode(node) => {
            out.insert(node.id);
        }
        OpPayload::RemoveNode(node_ref) => {
            out.insert(*node_ref);
        }
        OpPayload::SetNodeName { node_id, .. } => {
            out.insert(*node_id);
        }
        OpPayload::AddEdge(edge) => {
            out.insert(edge.source);
            out.insert(edge.target);
        }
        OpPayload::RemoveNodeByName(_)
        | OpPayload::RenameNodeByName { .. }
        | OpPayload::AddEdgeByName { .. }
        | OpPayload::RemoveEdgeByName { .. }
        | OpPayload::SetReturnByName { .. }
        | OpPayload::SetBodyByName { .. }
        | OpPayload::SetMetadataByName { .. }
        | OpPayload::AddParamByName { .. }
        | OpPayload::AddEffectByName { .. }
        | OpPayload::RemoveEffectByName { .. }
        | OpPayload::AddContractByName { .. }
        | OpPayload::RemoveContractByName { .. }
        | OpPayload::AddCapabilityReqByName { .. }
        | OpPayload::RemoveCapabilityReqByName { .. }
        | OpPayload::SetVisibilityByName { .. }
        | OpPayload::AddBindingByName { .. }
        | OpPayload::AddInferredFactByName { .. }
        | OpPayload::AddDerivedImplByName { .. }
        | OpPayload::AddGeneratedArtifactByName { .. }
        | OpPayload::AddAssertionByName { .. }
        | OpPayload::SetWorkflowStateByName { .. } => {
            // Name-based payloads are resolved against the live graph during
            // apply. StructuralDiff is NodeRef-based, so there is no stable
            // NodeRef to extract at canonicalization/rebase time.
        }
        OpPayload::Noop => {
            // No NodeRef to extract.
        }
    }
}

// ── RebaseResult ─────────────────────────────────────────────────────────

/// Outcome of attempting to rebase a stale `CanonicalChangeSet`.
#[derive(Debug)]
pub enum RebaseResult {
    /// Rebase succeeded; the changeset is now ready to apply against the live
    /// snapshot.  The `base_snapshot_id` has been updated to the live id.
    Rebased(CanonicalChangeSet),
    /// Rebase failed because a pending op touches a `NodeRef` that was already
    /// mutated by the committed diff.
    Conflict(ConflictReason),
}

// ── rebase ────────────────────────────────────────────────────────────────

/// Attempt to rebase `pending` onto the live snapshot given `diff`.
///
/// # Algorithm
///
/// 1. Collect all `NodeRef`s referenced by `pending.ops`.
/// 2. Intersect with `diff.touched_nodes`.
/// 3. If the intersection is empty → `Rebased` (update `base_snapshot_id`).
/// 4. Otherwise classify the conflict and return `Conflict(reason)`.
///
/// Conflict classification (conservative — Phase 13):
/// - Any `RemoveNode` in `diff` whose `NodeRef` is also in `pending.ops` →
///   `NodeDeletedWhileModified`.
/// - Any other overlap → `SameNodeModifiedIncompatibly`.
pub fn rebase(
    mut pending: CanonicalChangeSet,
    diff: &StructuralDiff,
    live_snapshot_id: ail_change::model::SnapshotId,
) -> RebaseResult {
    // Step 1: collect NodeRefs from pending ops.
    let mut pending_nodes: BTreeSet<NodeRef> = BTreeSet::new();
    for op in &pending.ops {
        extract_node_refs(&op.payload, &mut pending_nodes);
    }

    // Step 2: intersect with committed diff.
    let conflicts: BTreeSet<NodeRef> = pending_nodes
        .intersection(&diff.touched_nodes)
        .copied()
        .collect();

    // Step 3: no intersection — rebase succeeds.
    if conflicts.is_empty() {
        pending.base_snapshot_id = live_snapshot_id;
        return RebaseResult::Rebased(pending);
    }

    // Step 4: classify the conflict.
    //
    // If the committed diff contains a `RemoveNode` for any conflicting
    // NodeRef, classify as `NodeDeletedWhileModified`.  Otherwise fall back
    // to `SameNodeModifiedIncompatibly`.
    //
    // Note: `StructuralDiff` only stores the set of touched refs, not the
    // full op list.  The coordinator stores the committed ops separately for
    // this classification — but the design says to classify based on `diff`
    // alone at this layer.  We use a conservative rule: the diff carries
    // `removed_nodes` separately when needed.  For Phase 13, the coordinator
    // passes context via `removed_nodes` in `RebaseContext`.
    //
    // Since `StructuralDiff` does not distinguish remove from modify, the
    // coordinator calls the richer `classify_conflict` helper below.
    RebaseResult::Conflict(ConflictReason::SameNodeModifiedIncompatibly)
}

/// Classify the conflict reason given the set of NodeRefs removed by the
/// committed ops and the set of conflicting NodeRefs.
///
/// Used by the coordinator after calling `rebase()` returns a conflict —
/// the coordinator has the full op list to distinguish removes from modifies.
pub fn classify_conflict(
    conflicts: &BTreeSet<NodeRef>,
    removed_in_diff: &BTreeSet<NodeRef>,
) -> ConflictReason {
    // If any conflicting NodeRef was removed by the committed ops, the pending
    // agent is modifying a node that no longer exists.
    if conflicts.intersection(removed_in_diff).next().is_some() {
        ConflictReason::NodeDeletedWhileModified
    } else {
        ConflictReason::SameNodeModifiedIncompatibly
    }
}

// ── MergeResult ───────────────────────────────────────────────────────────

/// Outcome of a semantic graph merge operation.
#[derive(Debug)]
pub enum MergeResult {
    /// The merge succeeded; the combined graph is returned.
    ///
    /// Only additive changes (nodes and edges present in `right` but absent in
    /// `left`) are incorporated.  Nodes that already exist in `left` with
    /// identical content are silently de-duplicated.
    Merged(SemanticGraph),
    /// A semantic conflict was detected for the given `NodeRef`.
    ///
    /// Both sides modified the node's semantic content (e.g., `return_type`,
    /// `body_expr`, or `effect_row`) in incompatible ways.
    Conflict {
        /// Classification of why the merge cannot proceed.
        reason: ConflictReason,
        /// The `NodeRef` that triggered the conflict.
        node_ref: NodeRef,
    },
}

// ── semantic_merge ────────────────────────────────────────────────────────

/// Attempt to semantically merge graph `right` into graph `left`.
///
/// # Algorithm
///
/// 1. Build a map of `left` nodes by `NodeRef`.
/// 2. For each node in `right`:
///    a. **Not in `left`** (new node) → add to merged result.
///    b. **In `left`, identical** → skip (already present; dedup).
///    c. **In `left`, different semantic fields** (`return_type`, `body_expr`,
///       or `effect_row`) → return
///       `MergeResult::Conflict { reason: ConflictReason::IncompatibleNodeModification, … }`.
///    d. **In `left`, non-semantic fields differ** (e.g., provenance, schema)
///       → treat as compatible; keep `left` version.
/// 3. Add edges from `right` that are not present in `left`.
/// 4. Return `MergeResult::Merged(combined)`.
///
/// # Additive-only guarantee
///
/// Only nodes and edges from `right` that do not conflict with `left` are
/// incorporated.  The merged graph always contains all of `left` plus any
/// non-conflicting additions from `right`.
pub fn semantic_merge(left: &SemanticGraph, right: &SemanticGraph) -> MergeResult {
    // Step 1: index left nodes by NodeRef.
    let left_by_ref: BTreeMap<NodeRef, &GraphNode> = left.nodes.iter().map(|n| (n.id, n)).collect();

    let mut merged = left.clone();

    // Step 2: process each right node.
    for right_node in &right.nodes {
        match left_by_ref.get(&right_node.id) {
            None => {
                // New node — additive addition.
                merged.nodes.push(right_node.clone());
            }
            Some(left_node) => {
                // Node exists in both — check semantic field conflicts.
                if node_has_semantic_conflict(left_node, right_node) {
                    return MergeResult::Conflict {
                        reason: ConflictReason::IncompatibleNodeModification,
                        node_ref: right_node.id,
                    };
                }
                // Compatible (identical or only non-semantic fields differ) → keep left.
            }
        }
    }

    // Step 3: add edges from right that are absent in left.
    let left_edges: BTreeSet<(NodeRef, NodeRef, String)> = left
        .edges
        .iter()
        .map(|e| (e.source, e.target, format!("{:?}", e.kind)))
        .collect();

    for right_edge in &right.edges {
        let key = (
            right_edge.source,
            right_edge.target,
            format!("{:?}", right_edge.kind),
        );
        if !left_edges.contains(&key) {
            merged.edges.push(right_edge.clone());
        }
    }

    MergeResult::Merged(merged)
}

/// Returns `true` when `left` and `right` disagree on at least one semantic field.
///
/// Semantic fields are: `return_type`, `body_expr`, and `effect_row`.
/// All other fields (provenance, schema, trust_metadata, …) are considered
/// non-semantic for merge purposes and are ignored.
fn node_has_semantic_conflict(left: &GraphNode, right: &GraphNode) -> bool {
    left.return_type != right.return_type
        || left.body_expr != right.body_expr
        || left.effect_row != right.effect_row
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_change::{
        canonical::{CanonicalChangeSet, CanonicalMeta, CanonicalOp, OpPayload},
        model::{BlockHash, SnapshotId, Timestamp},
    };
    use ail_core::semantic_graph::{
        EdgeKind, EffectRow, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };

    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────

    fn dummy_hash() -> BlockHash {
        BlockHash([0u8; 32])
    }

    fn meta() -> CanonicalMeta {
        CanonicalMeta {
            author: "test".into(),
            description: "test cs".into(),
            timestamp: Timestamp(0),
        }
    }

    fn create_op(node_ref: u32) -> CanonicalOp {
        CanonicalOp {
            kind: ail_change::model::ChangeSetOp::Create,
            payload: OpPayload::CreateNode(Box::new(GraphNode::new(
                NodeRef(node_ref),
                NodeKind::Function,
                "fn",
            ))),
            block_hash: dummy_hash(),
            ..Default::default()
        }
    }

    fn remove_op(node_ref: u32) -> CanonicalOp {
        CanonicalOp {
            kind: ail_change::model::ChangeSetOp::Remove,
            payload: OpPayload::RemoveNode(NodeRef(node_ref)),
            block_hash: dummy_hash(),
            ..Default::default()
        }
    }

    fn rename_op(node_ref: u32) -> CanonicalOp {
        CanonicalOp {
            kind: ail_change::model::ChangeSetOp::Set,
            payload: OpPayload::SetNodeName {
                node_id: NodeRef(node_ref),
                name: "new_name".into(),
            },
            block_hash: dummy_hash(),
            ..Default::default()
        }
    }

    fn edge_op(src: u32, tgt: u32) -> CanonicalOp {
        CanonicalOp {
            kind: ail_change::model::ChangeSetOp::Connect,
            payload: OpPayload::AddEdge(GraphEdge::new(
                NodeRef(src),
                NodeRef(tgt),
                EdgeKind::Calls,
            )),
            block_hash: dummy_hash(),
            ..Default::default()
        }
    }

    fn cs_with_ops(base: u64, ops: Vec<CanonicalOp>) -> CanonicalChangeSet {
        CanonicalChangeSet {
            meta: meta(),
            base_snapshot_id: SnapshotId(base),
            preconditions: vec![],
            ops,
            ..Default::default()
        }
    }

    // ── Task 2.5a: disjoint NodeRefs → Rebased ───────────────────────────
    //
    // Spec: Non-conflicting concurrent ChangeSets both apply
    //   GIVEN committed diff touched NodeRef(1) (fn.cart_total)
    //   WHEN pending adds NodeRef(2) (fn.checkout) — disjoint
    //   THEN rebase returns Rebased with base_snapshot_id updated to live
    #[test]
    fn disjoint_node_refs_produces_rebased() {
        let diff = StructuralDiff::from_ops(&[create_op(1)]);
        let pending = cs_with_ops(0, vec![create_op(2)]);
        let live = SnapshotId(1);

        match rebase(pending, &diff, live) {
            RebaseResult::Rebased(rebased) => {
                assert_eq!(
                    rebased.base_snapshot_id, live,
                    "base_snapshot_id must be updated to live"
                );
            }
            RebaseResult::Conflict(r) => panic!("expected Rebased, got Conflict({r:?})"),
        }
    }

    // ── Task 2.5b: same NodeRef modified → SameNodeModifiedIncompatibly ──
    //
    // Spec: Conflicting ops on same NodeRef are irresolvable
    //   GIVEN committed diff renamed NodeRef(1) (fn.cart_total)
    //   WHEN pending also modifies NodeRef(1)
    //   THEN rebase returns Conflict(SameNodeModifiedIncompatibly)
    #[test]
    fn same_node_ref_conflict_produces_same_node_modified() {
        let diff = StructuralDiff::from_ops(&[rename_op(1)]);
        let pending = cs_with_ops(0, vec![rename_op(1)]);
        let live = SnapshotId(1);

        match rebase(pending, &diff, live) {
            RebaseResult::Conflict(ConflictReason::SameNodeModifiedIncompatibly) => {}
            other => panic!("expected SameNodeModifiedIncompatibly, got {other:?}"),
        }
    }

    // ── Task 2.5c: RemoveNode in diff + pending modifies same → NodeDeletedWhileModified
    //
    // Uses classify_conflict directly since rebase() uses the conservative fallback.
    // The coordinator calls classify_conflict after rebase returns Conflict.
    #[test]
    fn removed_node_conflict_produces_node_deleted_while_modified() {
        // Committed: removed NodeRef(5)
        // Pending: renames NodeRef(5)
        let committed_removes: BTreeSet<NodeRef> = [NodeRef(5)].into();
        let conflicts: BTreeSet<NodeRef> = [NodeRef(5)].into();

        let reason = classify_conflict(&conflicts, &committed_removes);
        assert_eq!(reason, ConflictReason::NodeDeletedWhileModified);
    }

    // ── StructuralDiff::from_ops extracts all payload types ──────────────

    #[test]
    fn from_ops_extracts_create_node_refs() {
        let diff = StructuralDiff::from_ops(&[create_op(10)]);
        assert!(diff.touched_nodes.contains(&NodeRef(10)));
    }

    #[test]
    fn from_ops_extracts_remove_node_refs() {
        let diff = StructuralDiff::from_ops(&[remove_op(20)]);
        assert!(diff.touched_nodes.contains(&NodeRef(20)));
    }

    #[test]
    fn from_ops_extracts_set_node_name_refs() {
        let diff = StructuralDiff::from_ops(&[rename_op(30)]);
        assert!(diff.touched_nodes.contains(&NodeRef(30)));
    }

    #[test]
    fn from_ops_extracts_add_edge_both_endpoints() {
        let diff = StructuralDiff::from_ops(&[edge_op(1, 2)]);
        assert!(diff.touched_nodes.contains(&NodeRef(1)));
        assert!(diff.touched_nodes.contains(&NodeRef(2)));
    }

    #[test]
    fn from_ops_ignores_noop() {
        let noop = CanonicalOp {
            kind: ail_change::model::ChangeSetOp::Infer,
            payload: OpPayload::Noop,
            block_hash: dummy_hash(),
            ..Default::default()
        };
        let diff = StructuralDiff::from_ops(&[noop]);
        assert!(diff.touched_nodes.is_empty());
    }

    // ── semantic_merge helpers ─────────────────────────────────────────────

    fn simple_graph(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> SemanticGraph {
        SemanticGraph { nodes, edges }
    }

    fn fn_node(id: u32, name: &str) -> GraphNode {
        GraphNode::new(NodeRef(id), NodeKind::Function, name)
    }

    // ── semantic_merge: additive new nodes merge cleanly ─────────────────
    //
    // Spec: non-conflicting new nodes from right are added to the merge result.
    //   GIVEN left has NodeRef(0) and right has NodeRef(1) (disjoint)
    //   WHEN semantic_merge(left, right) is called
    //   THEN MergeResult::Merged is returned containing both nodes
    //
    // RED: semantic_merge did not exist → compile error.
    // GREEN: function added → returns Merged with both nodes.
    #[test]
    fn semantic_merge_additive_nodes_succeed() {
        let left = simple_graph(vec![fn_node(0, "checkout")], vec![]);
        let right = simple_graph(vec![fn_node(1, "payment")], vec![]);

        match semantic_merge(&left, &right) {
            MergeResult::Merged(merged) => {
                let refs: Vec<u32> = merged.nodes.iter().map(|n| n.id.0).collect();
                assert!(
                    refs.contains(&0),
                    "left node NodeRef(0) must survive; got: {refs:?}"
                );
                assert!(
                    refs.contains(&1),
                    "right node NodeRef(1) must be added; got: {refs:?}"
                );
                assert_eq!(
                    merged.nodes.len(),
                    2,
                    "merged graph must have exactly 2 nodes"
                );
            }
            MergeResult::Conflict { reason, node_ref } => {
                panic!("expected Merged, got Conflict({reason:?}, {node_ref:?})");
            }
        }
    }

    // ── semantic_merge: incompatible return_type triggers conflict ────────
    //
    // Spec: both agents modify the same node's return_type → IncompatibleNodeModification.
    //   GIVEN left has NodeRef(0) with return_type = Some("Int")
    //   AND right has NodeRef(0) with return_type = Some("String")
    //   WHEN semantic_merge(left, right) is called
    //   THEN MergeResult::Conflict { IncompatibleNodeModification, NodeRef(0) } is returned
    //
    // RED: IncompatibleNodeModification variant did not exist → compile error.
    // GREEN: node_has_semantic_conflict detects the return_type mismatch.
    #[test]
    fn semantic_merge_return_type_conflict() {
        let mut left_node = fn_node(0, "pay");
        left_node.return_type = Some("Int".to_string());
        let mut right_node = fn_node(0, "pay");
        right_node.return_type = Some("String".to_string()); // incompatible!

        let left = simple_graph(vec![left_node], vec![]);
        let right = simple_graph(vec![right_node], vec![]);

        match semantic_merge(&left, &right) {
            MergeResult::Conflict { reason, node_ref } => {
                assert_eq!(
                    reason,
                    ConflictReason::IncompatibleNodeModification,
                    "reason must be IncompatibleNodeModification"
                );
                assert_eq!(
                    node_ref,
                    NodeRef(0),
                    "conflicting NodeRef must be NodeRef(0)"
                );
            }
            MergeResult::Merged(_) => {
                panic!("expected Conflict for incompatible return_type, got Merged");
            }
        }
    }

    // ── TRIANGULATE: incompatible effect_row triggers conflict ───────────
    //
    // Different semantic field (effect_row) than the previous test — forces
    // real conflict detection, not just return_type comparison.
    #[test]
    fn semantic_merge_effect_row_conflict() {
        let mut left_node = fn_node(0, "transfer");
        left_node.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string()],
        });
        let mut right_node = fn_node(0, "transfer");
        right_node.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string(), "State".to_string()], // incompatible!
        });

        let left = simple_graph(vec![left_node], vec![]);
        let right = simple_graph(vec![right_node], vec![]);

        match semantic_merge(&left, &right) {
            MergeResult::Conflict { reason, node_ref } => {
                assert_eq!(reason, ConflictReason::IncompatibleNodeModification);
                assert_eq!(node_ref, NodeRef(0));
            }
            MergeResult::Merged(_) => panic!("effect_row conflict must not produce Merged"),
        }
    }

    // ── TRIANGULATE: identical nodes are de-duplicated cleanly ───────────
    //
    // If both sides have the exact same node, merge should succeed (not conflict).
    #[test]
    fn semantic_merge_identical_nodes_are_deduped() {
        let node = fn_node(0, "checkout");
        let left = simple_graph(vec![node.clone()], vec![]);
        let right = simple_graph(vec![node], vec![]);

        match semantic_merge(&left, &right) {
            MergeResult::Merged(merged) => {
                assert_eq!(
                    merged.nodes.len(),
                    1,
                    "identical nodes must be de-duplicated; got: {} nodes",
                    merged.nodes.len()
                );
            }
            MergeResult::Conflict { .. } => panic!("identical nodes must not conflict"),
        }
    }

    // ── additive edges are incorporated ──────────────────────────────────
    //
    // New edges from right (not in left) must appear in the merged result.
    #[test]
    fn semantic_merge_additive_edges_incorporated() {
        let left = simple_graph(
            vec![fn_node(0, "a"), fn_node(1, "b")],
            vec![], // no edges in left
        );
        let right = simple_graph(
            vec![fn_node(0, "a"), fn_node(1, "b")],
            vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls)],
        );

        match semantic_merge(&left, &right) {
            MergeResult::Merged(merged) => {
                assert_eq!(
                    merged.edges.len(),
                    1,
                    "new edge from right must be incorporated; got {} edges",
                    merged.edges.len()
                );
                assert_eq!(merged.edges[0].kind, EdgeKind::Calls);
            }
            MergeResult::Conflict { .. } => panic!("additive edge merge must not conflict"),
        }
    }
}
