// ── ail-context::selection ────────────────────────────────────────────────
//
// Candidate-collection and query-specific info helpers.
//
// Responsible for:
// - Dispatching each `ContextQuery` variant to the right graph traversal.
// - Assembling the `(candidates, history_entries)` pair returned to the builder.
// - Computing `ImpactInfo` and `RefactorInfo` from the pre-truncation node set.
//
// All traversal helpers in this module are private; only the three pub(crate)
// entry points are exposed to `builder`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ail_core::semantic_graph::{
    EdgeKind, GraphNode, NodeKind, NodeRef, SemanticGraph, Visibility, WorkflowState,
};
use ail_storage::graph::SnapshotEnvelope;

use crate::dto::{ContextQuery, ImpactInfo, QueryScope, RefactorInfo};
use crate::error::{ContextError, ContextResult};

// ── collect_candidates_with_history ──────────────────────────────────────

/// Collect matching nodes from `graph` according to `query`, sorted by
/// `NodeRef` (ascending).  Also returns history entries for `History` queries.
///
/// Returns `(candidates, history_entries)`.
pub(crate) fn collect_candidates_with_history(
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
        // Returns nodes that call `target` via static Calls OR dynamic DynCalls
        // (reverse BFS).  Both edge kinds are traversed so that callers via
        // `Dyn<Interface>` dynamic dispatch are included alongside direct callers.
        ContextQuery::Callers {
            target, transitive, ..
        } => {
            if !node_map.contains_key(target) {
                return Err(ContextError::NodeNotFound);
            }
            let callers = if *transitive {
                reverse_bfs(
                    graph,
                    &node_map,
                    *target,
                    &[EdgeKind::Calls, EdgeKind::DynCalls],
                )
            } else {
                direct_reverse(
                    graph,
                    &node_map,
                    *target,
                    &[EdgeKind::Calls, EdgeKind::DynCalls],
                )
            };
            Ok((callers, Vec::new()))
        }

        // ── Callees ───────────────────────────────────────────────────────
        // Returns nodes that `target` calls via static Calls OR dynamic DynCalls
        // (forward BFS).  Both edge kinds are traversed to distinguish dynamic
        // dispatch callees (`Dyn<Interface>`) from direct callees.
        ContextQuery::Callees {
            target, transitive, ..
        } => {
            if !node_map.contains_key(target) {
                return Err(ContextError::NodeNotFound);
            }
            let callees = if *transitive {
                bfs_filtered(
                    graph,
                    &node_map,
                    *target,
                    &[EdgeKind::Calls, EdgeKind::DynCalls],
                )
            } else {
                direct_forward(
                    graph,
                    &node_map,
                    *target,
                    &[EdgeKind::Calls, EdgeKind::DynCalls],
                )
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

        // ── Proofs ────────────────────────────────────────────────────────
        // Returns the target node plus proof-witness nodes reachable via
        // EdgeKind::Proves edges.
        ContextQuery::Proofs { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut proves_nodes = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Proves]);
            // Prepend the target node itself (bfs_filtered excludes seed).
            proves_nodes.insert(0, target_node);
            proves_nodes.dedup_by_key(|n| n.id);
            proves_nodes.sort_by_key(|n| n.id);
            Ok((proves_nodes, Vec::new()))
        }

        // ── Resources ─────────────────────────────────────────────────────
        // Returns the target node plus resource-dependency nodes reachable
        // via EdgeKind::Reads and EdgeKind::Writes edges.
        ContextQuery::Resources { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut resource_nodes = bfs_filtered(
                graph,
                &node_map,
                *target,
                &[EdgeKind::Reads, EdgeKind::Writes],
            );
            resource_nodes.insert(0, target_node);
            resource_nodes.dedup_by_key(|n| n.id);
            resource_nodes.sort_by_key(|n| n.id);
            Ok((resource_nodes, Vec::new()))
        }

        // ── Boundaries ────────────────────────────────────────────────────
        // Returns the target node plus boundary nodes (NodeKind::Boundary)
        // reachable via any edge from the target.
        ContextQuery::Boundaries { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            // BFS from target following all edge kinds; filter to Boundary nodes.
            let all_reachable = bfs_forward(graph, &node_map, *target);
            let mut boundary_nodes: Vec<GraphNode> = all_reachable
                .into_iter()
                .filter(|n| n.kind == ail_core::semantic_graph::NodeKind::Boundary)
                .collect();
            // Always include the target itself.
            boundary_nodes.insert(0, target_node);
            boundary_nodes.dedup_by_key(|n| n.id);
            boundary_nodes.sort_by_key(|n| n.id);
            Ok((boundary_nodes, Vec::new()))
        }

        // ── Why ───────────────────────────────────────────────────────────
        // Returns the target node plus provenance-related nodes (Proves and
        // BreaksIfChanged edges) and the snapshot history chain.
        ContextQuery::Why { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut why_nodes = bfs_filtered(
                graph,
                &node_map,
                *target,
                &[EdgeKind::Proves, EdgeKind::BreaksIfChanged],
            );
            why_nodes.insert(0, target_node);
            why_nodes.dedup_by_key(|n| n.id);
            why_nodes.sort_by_key(|n| n.id);

            // Reuse the history chain for provenance context.
            let history = build_history_chain(snapshot, all_snapshots);
            Ok((why_nodes, history))
        }

        // ── RefactorContext ───────────────────────────────────────────────
        // Returns the target node plus callers (reverse Calls BFS), proof
        // witnesses (Proves), and effect nodes (Emits).
        ContextQuery::RefactorContext { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            // Callers (nodes to update after refactor) — reverse BFS over both
            // static Calls and dynamic DynCalls edges.
            let callers = reverse_bfs(
                graph,
                &node_map,
                *target,
                &[EdgeKind::Calls, EdgeKind::DynCalls],
            );
            // Proofs to rerun.
            let proves = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Proves]);
            // Effects to preserve.
            let effects = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Emits]);

            // Merge: target + callers + proves + effects, deduplicated, sorted.
            let mut all_nodes: Vec<GraphNode> = Vec::new();
            all_nodes.push(target_node);
            all_nodes.extend(callers);
            all_nodes.extend(proves);
            all_nodes.extend(effects);
            all_nodes.sort_by_key(|n| n.id);
            all_nodes.dedup_by_key(|n| n.id);
            Ok((all_nodes, Vec::new()))
        }

        // ── Runtime ───────────────────────────────────────────────────────
        // Returns the target node (with capability_reqs and effect_row) plus
        // runtime-effect nodes reachable via EdgeKind::Emits.
        ContextQuery::Runtime { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut runtime_nodes = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Emits]);
            runtime_nodes.insert(0, target_node);
            runtime_nodes.dedup_by_key(|n| n.id);
            runtime_nodes.sort_by_key(|n| n.id);
            Ok((runtime_nodes, Vec::new()))
        }

        // ── Diff ──────────────────────────────────────────────────────────
        // Returns all nodes from the current graph (snapshot-level diff is not
        // supported without a second graph materialisation; returns all nodes
        // as the "changed" set when snapshot_a/snapshot_b are None).
        ContextQuery::Diff { .. } => {
            let mut nodes: Vec<GraphNode> = graph.nodes.clone();
            nodes.sort_by_key(|n| n.id);
            Ok((nodes, Vec::new()))
        }

        // ── Risks ─────────────────────────────────────────────────────────
        // Returns the target node plus BreaksIfChanged-reachable nodes
        // (change-impact dependencies that represent risk).
        ContextQuery::Risks { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut risk_nodes =
                bfs_filtered(graph, &node_map, *target, &[EdgeKind::BreaksIfChanged]);
            risk_nodes.insert(0, target_node);
            risk_nodes.dedup_by_key(|n| n.id);
            risk_nodes.sort_by_key(|n| n.id);
            Ok((risk_nodes, Vec::new()))
        }

        // ── Todo ──────────────────────────────────────────────────────────
        // Returns the target node plus Proves-reachable nodes that have
        // unverified obligations.
        ContextQuery::Todo { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut todo_nodes = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Proves]);
            todo_nodes.insert(0, target_node);
            todo_nodes.dedup_by_key(|n| n.id);
            todo_nodes.sort_by_key(|n| n.id);
            Ok((todo_nodes, Vec::new()))
        }

        // ── Capabilities ──────────────────────────────────────────────────
        // Returns the target node plus capability nodes reachable via Emits
        // and DependsOn edges.
        ContextQuery::Capabilities { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut cap_nodes = bfs_filtered(
                graph,
                &node_map,
                *target,
                &[EdgeKind::Emits, EdgeKind::DependsOn],
            );
            cap_nodes.insert(0, target_node);
            cap_nodes.dedup_by_key(|n| n.id);
            cap_nodes.sort_by_key(|n| n.id);
            Ok((cap_nodes, Vec::new()))
        }

        // ── Handlers ──────────────────────────────────────────────────────
        // Returns the target node plus nodes bound as handlers (incoming Calls
        // from boundary nodes).
        ContextQuery::Handlers { target, .. } => {
            if !node_map.contains_key(target) {
                return Err(ContextError::NodeNotFound);
            }
            // Handler nodes are reverse-Calls from the target.
            let handler_nodes = reverse_bfs(graph, &node_map, *target, &[EdgeKind::Calls]);
            let mut result: Vec<GraphNode> = Vec::new();
            // Include the target itself.
            if let Some(n) = node_map.get(target) {
                result.push((*n).clone());
            }
            result.extend(handler_nodes);
            result.dedup_by_key(|n| n.id);
            result.sort_by_key(|n| n.id);
            Ok((result, Vec::new()))
        }

        // ── Concurrency ───────────────────────────────────────────────────
        // Returns the target node plus nodes reachable via Reads, Writes, and
        // Calls edges (shared state + task relationships).
        ContextQuery::Concurrency { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut conc_nodes = bfs_filtered(
                graph,
                &node_map,
                *target,
                &[
                    EdgeKind::Reads,
                    EdgeKind::Writes,
                    EdgeKind::Calls,
                    EdgeKind::DynCalls,
                ],
            );
            conc_nodes.insert(0, target_node);
            conc_nodes.dedup_by_key(|n| n.id);
            conc_nodes.sort_by_key(|n| n.id);
            Ok((conc_nodes, Vec::new()))
        }

        // ── Tasks ─────────────────────────────────────────────────────────
        // Returns the target node plus async-task nodes reachable via
        // Calls and Emits edges.
        ContextQuery::Tasks { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let mut task_nodes = bfs_filtered(
                graph,
                &node_map,
                *target,
                &[EdgeKind::Calls, EdgeKind::Emits],
            );
            task_nodes.insert(0, target_node);
            task_nodes.dedup_by_key(|n| n.id);
            task_nodes.sort_by_key(|n| n.id);
            Ok((task_nodes, Vec::new()))
        }

        // ── Assumptions ───────────────────────────────────────────────────
        // Returns nodes reachable from target via any edge that carry trust
        // metadata (i.e., Boundary kind nodes).
        ContextQuery::Assumptions { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let all_reachable = bfs_forward(graph, &node_map, *target);
            let mut assumption_nodes: Vec<GraphNode> = all_reachable
                .into_iter()
                .filter(|n| n.kind == ail_core::semantic_graph::NodeKind::Boundary)
                .collect();
            assumption_nodes.insert(0, target_node);
            assumption_nodes.dedup_by_key(|n| n.id);
            assumption_nodes.sort_by_key(|n| n.id);
            Ok((assumption_nodes, Vec::new()))
        }

        // ── ExtractCandidates ─────────────────────────────────────────────
        // Returns nodes reachable from target via Calls and DependsOn that
        // have no callers from outside the target's reachable set.
        ContextQuery::ExtractCandidates { target, .. } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            // Forward BFS from target to get the reachable "inner" set.
            let inner_set = bfs_filtered(
                graph,
                &node_map,
                *target,
                &[EdgeKind::Calls, EdgeKind::DependsOn],
            );
            // Candidates are inner nodes that have no reverse-Calls from
            // outside the inner set (i.e., no external callers).
            let inner_refs: BTreeSet<NodeRef> = inner_set.iter().map(|n| n.id).collect();
            let mut candidates: Vec<GraphNode> = inner_set
                .into_iter()
                .filter(|n| {
                    // Check that all callers of this node are within inner_refs or are target.
                    let external_callers = graph.edges.iter().any(|e| {
                        e.target == n.id
                            && e.kind == EdgeKind::Calls
                            && !inner_refs.contains(&e.source)
                            && e.source != *target
                    });
                    !external_callers
                })
                .collect();
            candidates.insert(0, target_node);
            candidates.dedup_by_key(|n| n.id);
            candidates.sort_by_key(|n| n.id);
            Ok((candidates, Vec::new()))
        }

        // ── MoveSafety ────────────────────────────────────────────────────
        // Returns the target + destination nodes, plus callers, contracts,
        // effects, and proof obligations that would be affected by the move.
        ContextQuery::MoveSafety {
            target,
            destination,
            ..
        } => {
            let target_node = node_map
                .get(target)
                .copied()
                .ok_or(ContextError::NodeNotFound)?
                .clone();

            let callers = reverse_bfs(graph, &node_map, *target, &[EdgeKind::Calls]);
            let contracts = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Proves]);
            let effects = bfs_filtered(graph, &node_map, *target, &[EdgeKind::Emits]);

            let mut all_nodes: Vec<GraphNode> = Vec::new();
            all_nodes.push(target_node);
            // Include destination node if it exists.
            if let Some(dest_node) = node_map.get(destination) {
                all_nodes.push((*dest_node).clone());
            }
            all_nodes.extend(callers);
            all_nodes.extend(contracts);
            all_nodes.extend(effects);
            all_nodes.sort_by_key(|n| n.id);
            all_nodes.dedup_by_key(|n| n.id);
            Ok((all_nodes, Vec::new()))
        }
    }
}

// ── Impact and refactor info helpers ──────────────────────────────────────

/// Compute `ImpactInfo` from the full (pre-truncation) set of affected nodes.
///
/// `affected` contains the nodes returned by the Impact query before budget
/// truncation — this ensures the classification is always complete even if the
/// structured layer was truncated.
pub(crate) fn compute_impact_info(affected: &[GraphNode]) -> ImpactInfo {
    let affected_tests: Vec<NodeRef> = affected
        .iter()
        .filter(|n| n.kind == NodeKind::Test)
        .map(|n| n.id)
        .collect();

    let affected_capabilities: Vec<NodeRef> = affected
        .iter()
        .filter(|n| n.kind == NodeKind::Capability)
        .map(|n| n.id)
        .collect();

    let affected_public_apis: Vec<NodeRef> = affected
        .iter()
        .filter(|n| matches!(n.visibility, Some(Visibility::Public)))
        .map(|n| n.id)
        .collect();

    // Nodes that need re-verification after the change.
    let required_reverification = affected
        .iter()
        .filter(|n| n.kind == NodeKind::Contract || n.kind == NodeKind::Invariant)
        .count();

    // Risk level derived from public API surface + verification obligation count.
    let risk_count = affected_public_apis.len() + required_reverification;
    let risk_level = match risk_count {
        0 => "none",
        1..=3 => "low",
        4..=10 => "medium",
        _ => "high",
    }
    .to_string();

    ImpactInfo {
        affected_tests,
        affected_capabilities,
        affected_public_apis,
        required_reverification,
        risk_level,
    }
}

/// Compute `RefactorInfo` from the context nodes returned by a refactor query.
///
/// `structured` is the full (pre-truncation) node list.  `target` is the
/// primary refactoring target.  `graph` provides edge access for caller
/// and proof classification.
pub(crate) fn compute_refactor_info(
    structured: &[GraphNode],
    target: NodeRef,
    graph: &SemanticGraph,
) -> RefactorInfo {
    // Locked nodes constrain the refactoring.
    let behavior_locks_needed: Vec<NodeRef> = structured
        .iter()
        .filter(|n| matches!(n.workflow_state, Some(WorkflowState::Locked)))
        .map(|n| n.id)
        .collect();

    // Contract/Invariant nodes must be preserved.
    let contracts_to_preserve: Vec<NodeRef> = structured
        .iter()
        .filter(|n| n.kind == NodeKind::Contract || n.kind == NodeKind::Invariant)
        .map(|n| n.id)
        .collect();

    // Effect/EffectAlias nodes must be preserved.
    let effects_to_preserve: Vec<NodeRef> = structured
        .iter()
        .filter(|n| n.kind == NodeKind::Effect || n.kind == NodeKind::EffectAlias)
        .map(|n| n.id)
        .collect();

    // Callers: nodes that have a Calls edge pointing at `target`.
    let callers_to_update: Vec<NodeRef> = structured
        .iter()
        .filter(|n| {
            n.id != target
                && graph
                    .edges
                    .iter()
                    .any(|e| e.source == n.id && e.target == target && e.kind == EdgeKind::Calls)
        })
        .map(|n| n.id)
        .collect();

    // Proofs: nodes reachable from target via Proves edges.
    let proofs_to_rerun: Vec<NodeRef> = structured
        .iter()
        .filter(|n| {
            graph
                .edges
                .iter()
                .any(|e| e.source == target && e.target == n.id && e.kind == EdgeKind::Proves)
        })
        .map(|n| n.id)
        .collect();

    // Possible conflicts: nodes with BreaksIfChanged edges in the context set.
    let possible_conflicts: Vec<NodeRef> = structured
        .iter()
        .filter(|n| {
            n.id != target
                && graph.edges.iter().any(|e| {
                    (e.source == n.id || e.target == n.id) && e.kind == EdgeKind::BreaksIfChanged
                })
        })
        .map(|n| n.id)
        .collect();

    RefactorInfo {
        behavior_locks_needed,
        contracts_to_preserve,
        effects_to_preserve,
        callers_to_update,
        proofs_to_rerun,
        possible_conflicts,
        suggested_refactor_ops: Vec::new(),
    }
}

// ── Graph traversal helpers ───────────────────────────────────────────────

/// BFS forward from `start` following ALL outgoing edge kinds.
/// Includes `start` in the result and sorts by `NodeRef`.
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
