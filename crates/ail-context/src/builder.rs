// ── ail-context::builder ──────────────────────────────────────────────────
//
// Bounded, hash-stable context-response builder.
//
// # Algorithm
//
// 1. Validate `budget > 0` (rejects zero with `E_INVALID_BUDGET`).
// 2. Collect candidate nodes from the graph according to `ContextQuery` +
//    `QueryScope` (find target for `Node` queries; BFS for `Full` scope).
// 3. Filter redacted `NodeRef`s — sets `redacted = true` if any removed.
// 4. Greedily accumulate nodes (CBOR per-node bytes) until `budget` is
//    exhausted — sets `truncated = true` if stopped early.
// 5. Compute `context_hash = blake3(CBOR(structured))`.
// 6. Render `summary` from `structured` via `render_summary`.
// 7. Assemble and return `ContextResponse`.
//
// # Determinism
//
// Candidate nodes are always sorted by `NodeRef` (ascending) before budget
// accounting.  This guarantees that identical inputs produce identical
// `structured` slices and therefore identical `context_hash` values.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::SnapshotEnvelope;

use crate::dto::{ContextQuery, ContextResponse, QueryScope};
use crate::error::{ContextError, ContextResult};
use crate::summary::render_summary;

// ── ResponseBuilder ───────────────────────────────────────────────────────

/// Pure, synchronous context-response builder.
///
/// All inputs must already be materialised (snapshot envelope + decoded
/// semantic graph).  The builder performs no I/O.
pub struct ResponseBuilder;

impl ResponseBuilder {
    /// Build a bounded `ContextResponse` from fully-materialised inputs.
    ///
    /// # Errors
    ///
    /// - `ContextError::InvalidBudget` — `query.budget() == 0`.
    /// - `ContextError::NodeNotFound`  — `ContextQuery::Node` target absent.
    /// - `ContextError::Codec`         — CBOR encode failure (should not
    ///   happen with well-formed graph nodes).
    pub fn build(
        query: &ContextQuery,
        graph: &SemanticGraph,
        snapshot: &SnapshotEnvelope,
        redacted_refs: &BTreeSet<NodeRef>,
    ) -> ContextResult<ContextResponse> {
        let budget = query.budget();
        if budget == 0 {
            return Err(ContextError::InvalidBudget);
        }

        let codec = CborCodec;

        // ── Step 1: Collect candidates (sorted by NodeRef) ────────────────
        let candidates: Vec<GraphNode> = collect_candidates(query, graph)?;

        // ── Step 2: Apply redaction ───────────────────────────────────────
        let mut redacted = false;
        let unredacted: Vec<GraphNode> = candidates
            .into_iter()
            .filter(|n| {
                if redacted_refs.contains(&n.id) {
                    redacted = true;
                    false
                } else {
                    true
                }
            })
            .collect();

        // ── Step 3: Apply byte budget ─────────────────────────────────────
        let mut structured: Vec<GraphNode> = Vec::new();
        let mut total_bytes: usize = 0;
        let mut truncated = false;

        for node in unredacted {
            let node_bytes = codec
                .encode(&node)
                .map_err(|e| ContextError::Codec(e.to_string()))?;
            if total_bytes + node_bytes.len() > budget {
                truncated = true;
                break;
            }
            total_bytes += node_bytes.len();
            structured.push(node);
        }

        // ── Step 4: Compute context_hash = blake3(CBOR(structured)) ───────
        let structured_cbor = codec
            .encode(&structured)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        let context_hash = *blake3::hash(&structured_cbor).as_bytes();

        // ── Step 5: Render summary from structured only ───────────────────
        let summary = render_summary(&structured);

        Ok(ContextResponse {
            graph_root_hash: snapshot.graph_root_hash,
            context_hash,
            freshness: snapshot.created_at,
            snapshot: snapshot.clone(),
            structured,
            summary,
            redacted,
            truncated,
        })
    }
}

// ── collect_candidates (pure helper) ─────────────────────────────────────

/// Collect matching nodes from `graph` according to `query`, sorted by
/// `NodeRef` (ascending).
fn collect_candidates(
    query: &ContextQuery,
    graph: &SemanticGraph,
) -> ContextResult<Vec<GraphNode>> {
    match query {
        ContextQuery::Node { target, scope, .. } => {
            // Verify target exists.
            let target_node = graph
                .nodes
                .iter()
                .find(|n| n.id == *target)
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            match scope {
                QueryScope::Local => Ok(vec![target_node]),
                QueryScope::Full => {
                    // BFS from target along outgoing edges.
                    let node_map: BTreeMap<NodeRef, &GraphNode> =
                        graph.nodes.iter().map(|n| (n.id, n)).collect();
                    let mut visited: BTreeSet<NodeRef> = BTreeSet::new();
                    let mut queue: VecDeque<NodeRef> = VecDeque::new();
                    let mut reachable_refs: Vec<NodeRef> = Vec::new();

                    queue.push_back(*target);
                    visited.insert(*target);

                    while let Some(ref_id) = queue.pop_front() {
                        reachable_refs.push(ref_id);
                        for edge in &graph.edges {
                            if edge.source == ref_id && !visited.contains(&edge.target) {
                                visited.insert(edge.target);
                                queue.push_back(edge.target);
                            }
                        }
                    }

                    // Sort by NodeRef for determinism.
                    reachable_refs.sort();
                    Ok(reachable_refs
                        .iter()
                        .filter_map(|r| node_map.get(r).map(|n| (*n).clone()))
                        .collect())
                }
            }
        }
        ContextQuery::Graph { .. } => {
            // Return all nodes sorted by NodeRef.
            // For Graph queries, Local and Full both return all nodes; the
            // byte budget provides the primary scoping mechanism.
            let mut nodes: Vec<GraphNode> = graph.nodes.clone();
            nodes.sort_by_key(|n| n.id);
            Ok(nodes)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;

    // ── helpers ───────────────────────────────────────────────────────────

    fn make_snapshot() -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(b"builder-snap");
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: 1_000,
        }
    }

    fn make_graph() -> SemanticGraph {
        // 3 nodes: 0 → 1 → 2 (chain)
        SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Module, "core"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "run"),
                GraphNode::new(NodeRef(2), NodeKind::Effect, "io"),
            ],
            edges: vec![
                GraphEdge {
                    source: NodeRef(0),
                    target: NodeRef(1),
                    kind: EdgeKind::DependsOn,
                },
                GraphEdge {
                    source: NodeRef(1),
                    target: NodeRef(2),
                    kind: EdgeKind::Emits,
                },
            ],
        }
    }

    fn no_redactions() -> BTreeSet<NodeRef> {
        BTreeSet::new()
    }

    // ── zero_budget_returns_invalid_budget ────────────────────────────────
    // Spec scenario: "Zero-budget query is rejected".
    //
    // RED: `ResponseBuilder::build` did not exist → compile error.
    // GREEN: budget == 0 guard at the start of build() makes it pass.
    #[test]
    fn zero_budget_returns_invalid_budget() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: 0,
        };
        let result = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions());
        assert_eq!(
            result,
            Err(ContextError::InvalidBudget),
            "budget = 0 must return Err(InvalidBudget)"
        );
    }

    // ── node_query_local_returns_target_only ──────────────────────────────
    // Spec scenario: "Valid node query is accepted".
    #[test]
    fn node_query_local_returns_target_only() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Node {
            target: NodeRef(0),
            scope: QueryScope::Local,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        assert_eq!(resp.structured.len(), 1, "Local scope must return 1 node");
        assert_eq!(
            resp.structured[0].id,
            NodeRef(0),
            "Local scope must return the target node"
        );
        assert!(!resp.truncated, "must not be truncated with max budget");
        assert!(!resp.redacted, "must not be redacted with empty set");
    }

    // ── node_query_full_returns_all_reachable ─────────────────────────────
    // Spec: Full scope traverses BFS from target.
    // TRIANGULATE: forces real BFS logic (Local test alone would not).
    #[test]
    fn node_query_full_returns_all_reachable() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Node {
            target: NodeRef(0),
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        // 0 → 1 → 2: all three are reachable from 0
        assert_eq!(
            resp.structured.len(),
            3,
            "Full scope from root must reach all 3 nodes; got {:?}",
            resp.structured.iter().map(|n| n.id).collect::<Vec<_>>()
        );
    }

    // ── node_query_missing_target_returns_node_not_found ──────────────────
    // Spec scenario: "Missing node returns E_NODE_NOT_FOUND".
    #[test]
    fn node_query_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Node {
            target: NodeRef(99),
            scope: QueryScope::Local,
            budget: usize::MAX,
        };
        let result = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions());
        assert_eq!(
            result,
            Err(ContextError::NodeNotFound),
            "missing target must return Err(NodeNotFound)"
        );
    }

    // ── context_hash_stable_for_identical_inputs ──────────────────────────
    // Spec scenario: "context_hash is stable for identical inputs".
    #[test]
    fn context_hash_stable_for_identical_inputs() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp_a = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("first build");
        let resp_b = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("second build");
        assert_eq!(
            resp_a.context_hash, resp_b.context_hash,
            "identical inputs must produce identical context_hash"
        );
    }

    // ── different_inputs_produce_different_hashes ─────────────────────────
    // Spec scenario: "Different structured layers produce different hashes".
    // TRIANGULATE: forces real hash logic (not a hardcoded constant).
    #[test]
    fn different_inputs_produce_different_hashes() {
        let snapshot = make_snapshot();

        let graph_a = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "a")],
            edges: vec![],
        };
        let graph_b = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "b")],
            edges: vec![],
        };
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };

        let resp_a =
            ResponseBuilder::build(&query, &graph_a, &snapshot, &no_redactions()).expect("a");
        let resp_b =
            ResponseBuilder::build(&query, &graph_b, &snapshot, &no_redactions()).expect("b");

        assert_ne!(
            resp_a.context_hash, resp_b.context_hash,
            "distinct structured layers must produce distinct context_hash"
        );
    }

    // ── budget_exceeded_sets_truncated ────────────────────────────────────
    // Spec scenario: "Truncation flag set when budget is exceeded".
    #[test]
    fn budget_exceeded_sets_truncated() {
        let graph = make_graph(); // 3 nodes
        let snapshot = make_snapshot();
        // budget = 1 byte: definitely smaller than any CBOR-encoded node
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: 1,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed even with tiny budget");
        assert!(
            resp.truncated,
            "structured layer exceeding budget must set truncated = true"
        );
    }

    // ── redacted_node_absent_from_structured ──────────────────────────────
    // Spec scenario: "Redaction flag set when nodes are withheld".
    #[test]
    fn redacted_node_absent_from_structured() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let mut redacted_refs = BTreeSet::new();
        redacted_refs.insert(NodeRef(1)); // redact the middle node

        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &redacted_refs)
            .expect("build must succeed");
        assert!(resp.redacted, "redacted flag must be true when a node is withheld");
        let ids: Vec<NodeRef> = resp.structured.iter().map(|n| n.id).collect();
        assert!(
            !ids.contains(&NodeRef(1)),
            "redacted node must be absent from structured; got: {ids:?}"
        );
        assert_eq!(ids.len(), 2, "2 of 3 nodes survive redaction");
    }

    // ── TRIANGULATE: graph_query_full_includes_all_nodes ─────────────────
    // Different from node_query_full: exercises the Graph branch of collect_candidates.
    #[test]
    fn graph_query_full_includes_all_nodes() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        assert_eq!(resp.structured.len(), 3, "Graph + Full must include all 3 nodes");
        // Verify NodeRef order
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2], "nodes must be sorted by NodeRef");
    }

    // ── freshness_equals_snapshot_created_at ─────────────────────────────
    // Spec: `freshness` is `snapshot.created_at`.
    #[test]
    fn freshness_equals_snapshot_created_at() {
        let graph = make_graph();
        let snapshot = make_snapshot(); // created_at = 1_000
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        assert_eq!(
            resp.freshness, 1_000,
            "freshness must equal snapshot.created_at"
        );
    }
}
