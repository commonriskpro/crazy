// ── ail-context::builder ──────────────────────────────────────────────────
//
// Bounded, hash-stable context-response builder.
//
// # Algorithm
//
// 1. Validate `budget > 0` (rejects zero with `E_INVALID_BUDGET`).
// 2. Compute `query_hash = blake3(CBOR(query))` for the response envelope.
// 3. Collect candidate nodes from the graph according to `ContextQuery` +
//    `QueryScope` (find target for `Node` queries; BFS for `Full` scope;
//    graph-traversal for impact/callers/callees/effects/contracts/history).
// 4. Filter redacted `NodeRef`s — sets `redacted = true` if any removed.
// 5. Greedily accumulate nodes (CBOR per-node bytes) until `budget` is
//    exhausted — sets `truncated = true` if stopped early.
// 6. Compute `context_hash = blake3(CBOR(structured))`.
// 7. Render `summary` from `structured` via `render_summary`.
// 8. Populate `history_entries` for `History` queries.
// 9. Assemble and return `ContextResponse`.
//
// # Determinism
//
// Candidate nodes are always sorted by `NodeRef` (ascending) before budget
// accounting.  This guarantees that identical inputs produce identical
// `structured` slices and therefore identical `context_hash` values.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ail_core::semantic_graph::{EdgeKind, GraphNode, NodeRef, SemanticGraph};
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::SnapshotEnvelope;

use crate::dto::{CONTEXT_SCHEMA_V1, ContextQuery, ContextResponse, QueryScope, ResponseLimits};
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
    /// - `ContextError::NodeNotFound`  — node-scoped query target absent.
    /// - `ContextError::Codec`         — CBOR encode failure (should not
    ///   happen with well-formed graph nodes).
    #[cfg_attr(
        feature = "otel",
        tracing::instrument(skip_all, name = "context.response_builder.build")
    )]
    pub fn build(
        query: &ContextQuery,
        graph: &SemanticGraph,
        snapshot: &SnapshotEnvelope,
        redacted_refs: &BTreeSet<NodeRef>,
    ) -> ContextResult<ContextResponse> {
        Self::build_with_history(query, graph, snapshot, redacted_refs, &[])
    }

    /// Build a `ContextResponse` with an optional history chain.
    ///
    /// `all_snapshots` is used only for `History` queries; for all other
    /// query kinds it is ignored.  Passing an empty slice is always valid.
    pub fn build_with_history(
        query: &ContextQuery,
        graph: &SemanticGraph,
        snapshot: &SnapshotEnvelope,
        redacted_refs: &BTreeSet<NodeRef>,
        all_snapshots: &[SnapshotEnvelope],
    ) -> ContextResult<ContextResponse> {
        let budget = query.budget();
        if budget == 0 {
            return Err(ContextError::InvalidBudget);
        }

        let codec = CborCodec;

        // ── Step 1: query_hash = blake3(CBOR(query)) ─────────────────────
        let query_cbor = codec
            .encode(query)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        let query_hash = *blake3::hash(&query_cbor).as_bytes();

        // ── Step 2: Collect candidates (sorted by NodeRef) ────────────────
        let (candidates, history_entries) =
            collect_candidates_with_history(query, graph, snapshot, all_snapshots)?;

        // ── Step 3: Apply redaction ───────────────────────────────────────
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

        // ── Step 4: Apply byte budget ─────────────────────────────────────
        let mut structured: Vec<GraphNode> = Vec::new();
        let mut total_bytes: usize = 0;
        let mut truncated = false;
        let mut omitted_sections: Vec<String> = Vec::new();

        for node in unredacted {
            let node_bytes = codec
                .encode(&node)
                .map_err(|e| ContextError::Codec(e.to_string()))?;
            if total_bytes + node_bytes.len() > budget {
                truncated = true;
                omitted_sections.push("structured_nodes".to_string());
                break;
            }
            total_bytes += node_bytes.len();
            structured.push(node);
        }

        // ── Step 5: Compute context_hash = blake3(CBOR(structured)) ───────
        let structured_cbor = codec
            .encode(&structured)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        let context_hash = *blake3::hash(&structured_cbor).as_bytes();

        // ── Step 6: Render summary from structured only ───────────────────
        let summary = render_summary(&structured);

        // ── Step 7: Assemble limits block ─────────────────────────────────
        let limits = ResponseLimits {
            budget_bytes: budget,
            bytes_used: total_bytes,
            truncated,
            omitted_sections,
        };

        Ok(ContextResponse {
            schema: CONTEXT_SCHEMA_V1.to_string(),
            graph_root_hash: snapshot.graph_root_hash,
            query_hash,
            context_hash,
            freshness: snapshot.created_at,
            snapshot: snapshot.clone(),
            structured,
            summary,
            redacted,
            truncated,
            limits,
            history_entries,
        })
    }
}

// ── collect_candidates_with_history (pure helper) ─────────────────────────

/// Collect matching nodes from `graph` according to `query`, sorted by
/// `NodeRef` (ascending).  Also returns history entries for `History` queries.
///
/// Returns `(candidates, history_entries)`.
fn collect_candidates_with_history(
    query: &ContextQuery,
    graph: &SemanticGraph,
    snapshot: &SnapshotEnvelope,
    all_snapshots: &[SnapshotEnvelope],
) -> ContextResult<(Vec<GraphNode>, Vec<SnapshotEnvelope>)> {
    let node_map: BTreeMap<NodeRef, &GraphNode> = graph.nodes.iter().map(|n| (n.id, n)).collect();

    match query {
        // ── Node ──────────────────────────────────────────────────────────
        ContextQuery::Node { target, scope, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let nodes = match scope {
                QueryScope::Local => vec![target_node],
                QueryScope::Full => bfs_forward(graph, &node_map, *target),
            };
            Ok((nodes, Vec::new()))
        }

        // ── Graph ─────────────────────────────────────────────────────────
        ContextQuery::Graph { .. } => {
            let mut nodes: Vec<GraphNode> = graph.nodes.clone();
            nodes.sort_by_key(|n| n.id);
            Ok((nodes, Vec::new()))
        }

        // ── Impact ────────────────────────────────────────────────────────
        // Returns nodes that depend on `target` (reverse BFS following
        // BreaksIfChanged, DependsOn, Calls, Reads, Writes edges backward).
        ContextQuery::Impact { target, .. } => {
            // Verify target exists.
            if !node_map.contains_key(target) {
                return Err(ContextError::NodeNotFound);
            }
            let dependents = reverse_bfs(
                graph,
                &node_map,
                *target,
                &[
                    EdgeKind::BreaksIfChanged,
                    EdgeKind::DependsOn,
                    EdgeKind::Calls,
                    EdgeKind::Reads,
                    EdgeKind::Writes,
                ],
            );
            Ok((dependents, Vec::new()))
        }

        // ── Callers ───────────────────────────────────────────────────────
        // Returns nodes that call `target` via EdgeKind::Calls (reverse BFS).
        ContextQuery::Callers {
            target, transitive, ..
        } => {
            if !node_map.contains_key(target) {
                return Err(ContextError::NodeNotFound);
            }
            let callers = if *transitive {
                reverse_bfs(graph, &node_map, *target, &[EdgeKind::Calls])
            } else {
                direct_reverse(graph, &node_map, *target, &[EdgeKind::Calls])
            };
            Ok((callers, Vec::new()))
        }

        // ── Callees ───────────────────────────────────────────────────────
        // Returns nodes that `target` calls via EdgeKind::Calls (forward BFS).
        ContextQuery::Callees {
            target, transitive, ..
        } => {
            if !node_map.contains_key(target) {
                return Err(ContextError::NodeNotFound);
            }
            let callees = if *transitive {
                bfs_filtered(graph, &node_map, *target, &[EdgeKind::Calls])
            } else {
                direct_forward(graph, &node_map, *target, &[EdgeKind::Calls])
            };
            Ok((callees, Vec::new()))
        }

        // ── Effects ───────────────────────────────────────────────────────
        // Returns the target node plus nodes reachable via Emits edges.
        ContextQuery::Effects { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            // Include target itself plus Emits-reachable nodes.
            let mut emitted = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Emits]);
            // Ensure target is first (already in emitted via BFS seed), but
            // bfs_filtered excludes the seed.  Prepend target node.
            emitted.insert(0, target_node);
            // Deduplicate (target_node might appear in emitted if there's a self-loop).
            emitted.dedup_by_key(|n| n.id);
            // Sort by NodeRef for determinism.
            emitted.sort_by_key(|n| n.id);
            Ok((emitted, Vec::new()))
        }

        // ── Contracts ─────────────────────────────────────────────────────
        // Returns only the target node (with contract_clauses populated).
        ContextQuery::Contracts { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();
            Ok((vec![target_node], Vec::new()))
        }

        // ── History ───────────────────────────────────────────────────────
        // Returns the target node from the current snapshot, plus the
        // snapshot provenance chain ordered oldest-first.
        ContextQuery::History { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            // Walk the parent_id chain from `snapshot` through `all_snapshots`,
            // building oldest-first order.
            let history = build_history_chain(snapshot, all_snapshots);
            Ok((vec![target_node], history))
        }
    }
}

// ── Graph traversal helpers ───────────────────────────────────────────────

/// BFS forward from `start` following ALL outgoing edge kinds.
/// Excludes `start` from the result and sorts by `NodeRef`.
fn bfs_forward<'g>(
    graph: &'g SemanticGraph,
    node_map: &BTreeMap<NodeRef, &'g GraphNode>,
    start: NodeRef,
) -> Vec<GraphNode> {
    let mut visited: BTreeSet<NodeRef> = BTreeSet::new();
    let mut queue: VecDeque<NodeRef> = VecDeque::new();
    let mut result_refs: Vec<NodeRef> = Vec::new();

    visited.insert(start);
    queue.push_back(start);

    while let Some(cur) = queue.pop_front() {
        result_refs.push(cur);
        for edge in &graph.edges {
            if edge.source == cur && !visited.contains(&edge.target) {
                visited.insert(edge.target);
                queue.push_back(edge.target);
            }
        }
    }

    result_refs.sort();
    result_refs
        .iter()
        .filter_map(|r| node_map.get(r).map(|n| (*n).clone()))
        .collect()
}

/// BFS forward from `start` following only edges whose kind is in `kinds`.
/// Excludes `start` from the result and sorts by `NodeRef`.
fn bfs_filtered<'g>(
    graph: &'g SemanticGraph,
    node_map: &BTreeMap<NodeRef, &'g GraphNode>,
    start: NodeRef,
    kinds: &[EdgeKind],
) -> Vec<GraphNode> {
    let mut visited: BTreeSet<NodeRef> = BTreeSet::new();
    let mut queue: VecDeque<NodeRef> = VecDeque::new();
    let mut result_refs: Vec<NodeRef> = Vec::new();

    visited.insert(start);
    queue.push_back(start);

    while let Some(cur) = queue.pop_front() {
        for edge in &graph.edges {
            if edge.source == cur && kinds.contains(&edge.kind) && !visited.contains(&edge.target) {
                visited.insert(edge.target);
                queue.push_back(edge.target);
                result_refs.push(edge.target);
            }
        }
    }

    result_refs.sort();
    result_refs
        .iter()
        .filter_map(|r| node_map.get(r).map(|n| (*n).clone()))
        .collect()
}

/// Direct (one-hop) forward neighbours via edges whose kind is in `kinds`.
/// Excludes `start`; sorts by `NodeRef`.
fn direct_forward<'g>(
    graph: &'g SemanticGraph,
    node_map: &BTreeMap<NodeRef, &'g GraphNode>,
    start: NodeRef,
    kinds: &[EdgeKind],
) -> Vec<GraphNode> {
    let mut refs: BTreeSet<NodeRef> = BTreeSet::new();
    for edge in &graph.edges {
        if edge.source == start && kinds.contains(&edge.kind) {
            refs.insert(edge.target);
        }
    }
    refs.iter()
        .filter_map(|r| node_map.get(r).map(|n| (*n).clone()))
        .collect()
}

/// BFS **reverse** from `start` following edges (target → source) whose kind
/// is in `kinds`.  Excludes `start`; sorts by `NodeRef`.
fn reverse_bfs<'g>(
    graph: &'g SemanticGraph,
    node_map: &BTreeMap<NodeRef, &'g GraphNode>,
    start: NodeRef,
    kinds: &[EdgeKind],
) -> Vec<GraphNode> {
    let mut visited: BTreeSet<NodeRef> = BTreeSet::new();
    let mut queue: VecDeque<NodeRef> = VecDeque::new();
    let mut result_refs: Vec<NodeRef> = Vec::new();

    visited.insert(start);
    queue.push_back(start);

    while let Some(cur) = queue.pop_front() {
        // Find edges whose TARGET is `cur` (reverse direction).
        for edge in &graph.edges {
            if edge.target == cur && kinds.contains(&edge.kind) && !visited.contains(&edge.source) {
                visited.insert(edge.source);
                queue.push_back(edge.source);
                result_refs.push(edge.source);
            }
        }
    }

    result_refs.sort();
    result_refs
        .iter()
        .filter_map(|r| node_map.get(r).map(|n| (*n).clone()))
        .collect()
}

/// Direct (one-hop) **reverse** neighbours — nodes whose outgoing edges
/// (with kind in `kinds`) point at `start`.  Excludes `start`; sorts by `NodeRef`.
fn direct_reverse<'g>(
    graph: &'g SemanticGraph,
    node_map: &BTreeMap<NodeRef, &'g GraphNode>,
    start: NodeRef,
    kinds: &[EdgeKind],
) -> Vec<GraphNode> {
    let mut refs: BTreeSet<NodeRef> = BTreeSet::new();
    for edge in &graph.edges {
        if edge.target == start && kinds.contains(&edge.kind) {
            refs.insert(edge.source);
        }
    }
    refs.iter()
        .filter_map(|r| node_map.get(r).map(|n| (*n).clone()))
        .collect()
}

/// Walk the `parent_id` chain from `current_snapshot` through `all_snapshots`,
/// returning the chain ordered oldest-first (genesis snapshot first).
fn build_history_chain(
    current_snapshot: &SnapshotEnvelope,
    all_snapshots: &[SnapshotEnvelope],
) -> Vec<SnapshotEnvelope> {
    // Index all_snapshots by id for O(1) lookup.
    let index: BTreeMap<_, &SnapshotEnvelope> = all_snapshots.iter().map(|s| (s.id, s)).collect();

    let mut chain: Vec<SnapshotEnvelope> = Vec::new();
    let mut cursor: Option<&SnapshotEnvelope> = Some(current_snapshot);

    // Safety: the chain is at most `all_snapshots.len() + 1` long.
    let max_depth = all_snapshots.len() + 1;
    let mut depth = 0;

    while let Some(snap) = cursor {
        chain.push(snap.clone());
        depth += 1;
        if depth > max_depth {
            // Cycle guard: stop if we've walked more hops than there are snapshots.
            break;
        }
        cursor = snap.parent_id.and_then(|pid| index.get(&pid).copied());
    }

    // Reverse so oldest (genesis) is first.
    chain.reverse();
    chain
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
        assert!(
            resp.redacted,
            "redacted flag must be true when a node is withheld"
        );
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
        assert_eq!(
            resp.structured.len(),
            3,
            "Graph + Full must include all 3 nodes"
        );
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

    // ── response_has_schema_and_query_hash ────────────────────────────────
    // Spec: every response carries schema version and a stable query_hash.
    #[test]
    fn response_has_schema_and_query_hash() {
        use crate::dto::CONTEXT_SCHEMA_V1;
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        assert_eq!(resp.schema, CONTEXT_SCHEMA_V1, "schema must be context/1.0");
        // query_hash must be non-zero (blake3 of CBOR(query))
        assert_ne!(
            resp.query_hash, [0u8; 32],
            "query_hash must not be the zero array"
        );
    }

    // ── query_hash_stable_for_identical_query ─────────────────────────────
    // TRIANGULATE: same query → same query_hash.
    #[test]
    fn query_hash_stable_for_identical_query() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Local,
            budget: 1024,
        };
        let resp_a = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("first build");
        let resp_b = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("second build");
        assert_eq!(
            resp_a.query_hash, resp_b.query_hash,
            "identical query must produce identical query_hash"
        );
    }

    // ── different_queries_produce_different_query_hashes ──────────────────
    // Two distinct queries must produce distinct query_hash values.
    #[test]
    fn different_queries_produce_different_query_hashes() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let q1 = ContextQuery::Graph {
            scope: QueryScope::Local,
            budget: 1024,
        };
        let q2 = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: 1024,
        };
        let resp1 = ResponseBuilder::build(&q1, &graph, &snapshot, &no_redactions()).unwrap();
        let resp2 = ResponseBuilder::build(&q2, &graph, &snapshot, &no_redactions()).unwrap();
        assert_ne!(
            resp1.query_hash, resp2.query_hash,
            "distinct queries must produce distinct query_hash values"
        );
    }

    // ── response_has_limits_block ─────────────────────────────────────────
    // Spec: response must carry a limits block with budget_bytes and bytes_used.
    #[test]
    fn response_has_limits_block() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        assert_eq!(resp.limits.budget_bytes, usize::MAX);
        assert!(!resp.limits.truncated, "not truncated with max budget");
        assert!(
            resp.limits.bytes_used > 0,
            "bytes_used must be > 0 for non-empty graph"
        );
    }

    // ── impact_query_returns_dependents ───────────────────────────────────
    // Spec: Impact query returns nodes that depend on the target.
    //
    // Graph: A --DependsOn--> B --Calls--> C
    // Impact(B) should return A (depends on B via reverse DependsOn edge).
    #[test]
    fn impact_query_returns_dependents() {
        // A=0, B=1, C=2.  0 --DependsOn--> 1, 1 --Calls--> 2
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Module, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
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
                    kind: EdgeKind::Calls,
                },
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Impact {
            target: NodeRef(1),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("impact build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        // A (0) depends on B (1) via DependsOn; should appear.
        assert!(ids.contains(&0), "A must appear in impact(B); got: {ids:?}");
        // B (1) is the target itself; should NOT appear.
        assert!(
            !ids.contains(&1),
            "target B must not be in its own impact set; got: {ids:?}"
        );
    }

    // ── impact_missing_target_returns_node_not_found ──────────────────────
    #[test]
    fn impact_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Impact {
            target: NodeRef(99),
            budget: usize::MAX,
        };
        let result = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions());
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── callers_direct_returns_direct_callers ────────────────────────────
    // Spec: Callers(transitive=false) returns only direct callers.
    //
    // Graph: A --Calls--> B --Calls--> C
    // Callers(C, transitive=false) = {B} only.
    #[test]
    fn callers_direct_returns_direct_callers() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
            ],
            edges: vec![
                GraphEdge {
                    source: NodeRef(0),
                    target: NodeRef(1),
                    kind: EdgeKind::Calls,
                },
                GraphEdge {
                    source: NodeRef(1),
                    target: NodeRef(2),
                    kind: EdgeKind::Calls,
                },
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Callers {
            target: NodeRef(2), // C
            transitive: false,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert_eq!(ids, vec![1], "direct callers of C is only B; got: {ids:?}");
    }

    // ── callers_transitive_returns_all_callers ────────────────────────────
    // TRIANGULATE: transitive=true must follow the call chain further back.
    #[test]
    fn callers_transitive_returns_all_callers() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
            ],
            edges: vec![
                GraphEdge {
                    source: NodeRef(0),
                    target: NodeRef(1),
                    kind: EdgeKind::Calls,
                },
                GraphEdge {
                    source: NodeRef(1),
                    target: NodeRef(2),
                    kind: EdgeKind::Calls,
                },
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Callers {
            target: NodeRef(2), // C
            transitive: true,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        // Both A (0) and B (1) are transitive callers of C.
        assert!(
            ids.contains(&0),
            "A must be a transitive caller of C; got: {ids:?}"
        );
        assert!(
            ids.contains(&1),
            "B must be a direct caller of C; got: {ids:?}"
        );
        assert!(!ids.contains(&2), "C itself must not appear; got: {ids:?}");
    }

    // ── callers_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn callers_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Callers {
                target: NodeRef(99),
                transitive: false,
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── callees_direct_returns_direct_callees ─────────────────────────────
    // Spec: Callees(transitive=false) returns only direct callees.
    //
    // Graph: A --Calls--> B --Calls--> C
    // Callees(A, transitive=false) = {B} only.
    #[test]
    fn callees_direct_returns_direct_callees() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
            ],
            edges: vec![
                GraphEdge {
                    source: NodeRef(0),
                    target: NodeRef(1),
                    kind: EdgeKind::Calls,
                },
                GraphEdge {
                    source: NodeRef(1),
                    target: NodeRef(2),
                    kind: EdgeKind::Calls,
                },
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Callees {
            target: NodeRef(0), // A
            transitive: false,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert_eq!(ids, vec![1], "direct callees of A is only B; got: {ids:?}");
    }

    // ── callees_transitive_returns_all_callees ────────────────────────────
    // TRIANGULATE: transitive=true follows the call chain forward.
    #[test]
    fn callees_transitive_returns_all_callees() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
            ],
            edges: vec![
                GraphEdge {
                    source: NodeRef(0),
                    target: NodeRef(1),
                    kind: EdgeKind::Calls,
                },
                GraphEdge {
                    source: NodeRef(1),
                    target: NodeRef(2),
                    kind: EdgeKind::Calls,
                },
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Callees {
            target: NodeRef(0), // A
            transitive: true,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(
            ids.contains(&1),
            "B must be a transitive callee of A; got: {ids:?}"
        );
        assert!(
            ids.contains(&2),
            "C must be a transitive callee of A; got: {ids:?}"
        );
        assert!(!ids.contains(&0), "A itself must not appear; got: {ids:?}");
    }

    // ── callees_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn callees_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Callees {
                target: NodeRef(99),
                transitive: false,
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── effects_query_returns_target_and_emits ────────────────────────────
    // Spec: Effects query returns target plus nodes reachable via Emits edges.
    //
    // make_graph(): 0 --DependsOn--> 1, 1 --Emits--> 2
    // Effects(1) should return {1, 2}.
    #[test]
    fn effects_query_returns_target_and_emits() {
        let graph = make_graph(); // 0→1(DependsOn), 1→2(Emits)
        let snapshot = make_snapshot();
        let query = ContextQuery::Effects {
            target: NodeRef(1),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(
            ids.contains(&1),
            "target node 1 must be in effects; got: {ids:?}"
        );
        assert!(
            ids.contains(&2),
            "emitted node 2 must be in effects; got: {ids:?}"
        );
        assert!(
            !ids.contains(&0),
            "node 0 (DependsOn, not Emits) must not appear; got: {ids:?}"
        );
    }

    // ── effects_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn effects_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Effects {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── contracts_query_returns_target_node ──────────────────────────────
    // Spec: Contracts query returns only the target node.
    #[test]
    fn contracts_query_returns_target_node() {
        use ail_core::semantic_graph::ContractClauses;
        let mut target_node = GraphNode::new(NodeRef(1), NodeKind::Function, "pay");
        target_node.contract_clauses = Some(ContractClauses {
            requires: vec!["amount > 0".to_string()],
            ensures: vec!["balance_changed".to_string()],
        });
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Module, "billing"),
                target_node.clone(),
            ],
            edges: vec![],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Contracts {
            target: NodeRef(1),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
        assert_eq!(
            resp.structured.len(),
            1,
            "contracts query must return exactly 1 node"
        );
        assert_eq!(resp.structured[0].id, NodeRef(1));
        assert!(
            resp.structured[0].contract_clauses.is_some(),
            "contract_clauses must be present on the returned node"
        );
    }

    // ── contracts_missing_target_returns_node_not_found ──────────────────
    #[test]
    fn contracts_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Contracts {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── history_query_returns_target_and_chain ───────────────────────────
    // Spec: History query returns the target node + provenance chain oldest-first.
    //
    // Chain: genesis (no parent) → snap2 (parent=genesis) → snap3 (parent=snap2)
    // History(node, current=snap3) should return [genesis, snap2, snap3].
    #[test]
    fn history_query_returns_target_and_chain() {
        let genesis_id = ObjectId::from_bytes(b"genesis");
        let snap2_id = ObjectId::from_bytes(b"snap2");
        let snap3_id = ObjectId::from_bytes(b"snap3");

        let genesis = SnapshotEnvelope {
            id: genesis_id,
            graph_root_hash: genesis_id,
            parent_id: None,
            applied_change_id: None,
            created_at: 1_000,
        };
        let snap2 = SnapshotEnvelope {
            id: snap2_id,
            graph_root_hash: snap2_id,
            parent_id: Some(genesis_id),
            applied_change_id: None,
            created_at: 2_000,
        };
        let snap3 = SnapshotEnvelope {
            id: snap3_id,
            graph_root_hash: snap3_id,
            parent_id: Some(snap2_id),
            applied_change_id: None,
            created_at: 3_000,
        };

        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "checkout")],
            edges: vec![],
        };
        let query = ContextQuery::History {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let all_snapshots = vec![genesis.clone(), snap2.clone()];
        let resp = ResponseBuilder::build_with_history(
            &query,
            &graph,
            &snap3,
            &no_redactions(),
            &all_snapshots,
        )
        .expect("history build must succeed");

        // Structured must contain the target node.
        assert_eq!(resp.structured.len(), 1);
        assert_eq!(resp.structured[0].id, NodeRef(0));

        // History chain: oldest first.
        let chain_ids: Vec<u64> = resp.history_entries.iter().map(|s| s.created_at).collect();
        assert_eq!(
            chain_ids,
            vec![1_000, 2_000, 3_000],
            "history must be oldest-first; got: {chain_ids:?}"
        );
    }

    // ── history_query_single_snapshot ────────────────────────────────────
    // TRIANGULATE: history with no parent yields a chain of length 1.
    #[test]
    fn history_query_single_snapshot() {
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn0")],
            edges: vec![],
        };
        let snapshot = make_snapshot(); // no parent_id
        let query = ContextQuery::History {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp =
            ResponseBuilder::build_with_history(&query, &graph, &snapshot, &no_redactions(), &[])
                .expect("history build must succeed");
        assert_eq!(
            resp.history_entries.len(),
            1,
            "single-snapshot history must have exactly 1 entry"
        );
    }

    // ── history_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn history_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build_with_history(
            &ContextQuery::History {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
            &[],
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }
}
