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

use crate::dto::{
    CONTEXT_SCHEMA_V1, ContextQuery, ContextResponse, FreshnessStatus, ProvenanceBlock, QueryScope,
    RedactionPolicy, RedactionState, RepairOption, ResponseLimits,
};
use crate::error::{ContextError, ContextResult};
use crate::summary::render_summary;

// ── BuildOptions ──────────────────────────────────────────────────────────

/// Extended options for `ResponseBuilder::build_full`.
///
/// All fields are optional — defaults produce the same behaviour as the
/// original `build` / `build_with_history` entry points.
#[derive(Default)]
pub struct BuildOptions<'a> {
    /// Snapshots for History/Why provenance chains.
    pub all_snapshots: &'a [SnapshotEnvelope],
    /// When `Some`, compared against `snapshot.id` to detect staleness.
    pub latest_snapshot_id: Option<&'a ail_storage::object::ObjectId>,
    /// When `Some`, wired into `redaction_state` and `redaction_policy`.
    pub redaction_policy: Option<&'a RedactionPolicy>,
    /// When `true`, caller is considered privileged (bypasses access-denied).
    /// When `false` and the query targets a restricted node, returns `E_ACCESS_DENIED`.
    pub authorized: bool,
    /// Unix-millisecond timestamp injected as `generated_at`.
    /// `0` means "use zero" (deterministic for tests).
    pub generated_at: u64,
    /// Provenance sources list (e.g., `["semantic_graph", "verification_reports"]`).
    pub provenance_sources: &'a [String],
    /// Index info records to attach to provenance.
    pub index_info: &'a [crate::dto::IndexInfo],
}

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
        Self::build_full(
            query,
            graph,
            snapshot,
            redacted_refs,
            &BuildOptions::default(),
        )
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
        let opts = BuildOptions {
            all_snapshots,
            authorized: true,
            ..Default::default()
        };
        Self::build_full(query, graph, snapshot, redacted_refs, &opts)
    }

    /// Full build with all options — the canonical entry point.
    ///
    /// # Security enforcement
    ///
    /// When `opts.authorized` is `false` and the query targets a node that is
    /// in `redacted_refs`, `E_ACCESS_DENIED` is returned before any traversal.
    ///
    /// # Freshness detection
    ///
    /// When `opts.latest_snapshot_id` is `Some`, the response `freshness_status`
    /// is set to `Stale` if the snapshot id does not match.
    ///
    /// # Redaction wiring
    ///
    /// When `opts.redaction_policy` is `Some`, it is attached to the response
    /// as `redaction_policy` and `redaction_state` is set to `Partial` or
    /// `Restricted` depending on whether any nodes were withheld.
    pub fn build_full(
        query: &ContextQuery,
        graph: &SemanticGraph,
        snapshot: &SnapshotEnvelope,
        redacted_refs: &BTreeSet<NodeRef>,
        opts: &BuildOptions<'_>,
    ) -> ContextResult<ContextResponse> {
        let budget = query.budget();
        if budget == 0 {
            return Err(ContextError::InvalidBudget);
        }

        // ── Security check ────────────────────────────────────────────────
        // If not authorized and the query targets a redacted node, deny access.
        if !opts.authorized && query.target().is_some_and(|t| redacted_refs.contains(&t)) {
            return Err(ContextError::AccessDenied);
        }

        let codec = CborCodec;

        // ── Step 1: query_hash = blake3(CBOR(query)) ─────────────────────
        let query_cbor = codec
            .encode(query)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        let query_hash = *blake3::hash(&query_cbor).as_bytes();

        // ── Step 2: Collect candidates (sorted by NodeRef) ────────────────
        let (candidates, history_entries) =
            collect_candidates_with_history(query, graph, snapshot, opts.all_snapshots)?;

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

        // ── Step 8: Determine freshness status ────────────────────────────
        let freshness_status = match opts.latest_snapshot_id {
            None => FreshnessStatus::Fresh,
            Some(latest_id) => {
                if *latest_id == snapshot.id {
                    FreshnessStatus::Fresh
                } else {
                    FreshnessStatus::Stale
                }
            }
        };

        // ── Step 9: Build repair options for stale responses ──────────────
        let mut repair_options: Vec<RepairOption> = Vec::new();
        if freshness_status == FreshnessStatus::Stale {
            repair_options.push(RepairOption {
                option_id: "query_latest".to_string(),
                description: "Re-issue the query at the latest snapshot".to_string(),
                suggested_query: query
                    .target()
                    .map(|t| format!("context {:?} snapshot=latest", t)),
            });
        }
        if truncated {
            repair_options.push(RepairOption {
                option_id: "narrow_scope".to_string(),
                description: "Narrow the query scope or increase the budget".to_string(),
                suggested_query: None,
            });
        }
        // Check for stale indexes.
        let has_stale_index = opts.index_info.iter().any(|i| i.stale);
        if has_stale_index {
            repair_options.push(RepairOption {
                option_id: "rebuild_index".to_string(),
                description: "Rebuild stale derived indexes and retry".to_string(),
                suggested_query: None,
            });
        }

        // ── Step 10: Redaction state and policy wiring ────────────────────
        let redaction_state = if let Some(policy) = opts.redaction_policy {
            if policy.requires_approval {
                RedactionState::Restricted
            } else if redacted {
                RedactionState::Partial
            } else {
                RedactionState::None
            }
        } else if redacted {
            RedactionState::Partial
        } else {
            RedactionState::None
        };
        let redaction_policy = opts.redaction_policy.cloned();

        // ── Step 11: Provenance block ─────────────────────────────────────
        let mut provenance = ProvenanceBlock {
            sources: opts.provenance_sources.to_vec(),
            indexes: opts.index_info.to_vec(),
            reports: Vec::new(),
        };
        // Always include the semantic graph as a source.
        if !provenance.sources.iter().any(|s| s == "semantic_graph") {
            provenance.sources.insert(0, "semantic_graph".to_string());
        }
        // Attach verification report hash from snapshot if present.
        if let Some(report_hash) = snapshot.verification_report_hash {
            provenance.reports.push(report_hash);
        }

        Ok(ContextResponse {
            schema: CONTEXT_SCHEMA_V1.to_string(),
            graph_root_hash: snapshot.graph_root_hash,
            query_hash,
            context_hash,
            freshness: snapshot.created_at,
            generated_at: opts.generated_at,
            snapshot: snapshot.clone(),
            structured,
            summary,
            redacted,
            redaction_state,
            redaction_policy,
            truncated,
            limits,
            history_entries,
            freshness_status,
            provenance,
            repair_options,
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

            // Callers (nodes to update after refactor) — reverse BFS.
            let callers = reverse_bfs(graph, &node_map, *target, &[EdgeKind::Calls]);
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
                &[EdgeKind::Reads, EdgeKind::Writes, EdgeKind::Calls],
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
            verification_report_hash: None,
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
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Emits),
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
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
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
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
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
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
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
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
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
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
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
            verification_report_hash: None,
        };
        let snap2 = SnapshotEnvelope {
            id: snap2_id,
            graph_root_hash: snap2_id,
            parent_id: Some(genesis_id),
            applied_change_id: None,
            created_at: 2_000,
            verification_report_hash: None,
        };
        let snap3 = SnapshotEnvelope {
            id: snap3_id,
            graph_root_hash: snap3_id,
            parent_id: Some(snap2_id),
            applied_change_id: None,
            created_at: 3_000,
            verification_report_hash: None,
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

    // ── proofs_query_returns_target_and_proves_nodes ──────────────────────
    // Spec: Proofs query returns the target node plus Proves-edge reachable nodes.
    //
    // Graph: fn.checkout --Proves--> invariant.stock_never_negative
    // Proofs(fn.checkout) = {fn.checkout, invariant.stock_never_negative}
    //
    // RED: ContextQuery::Proofs did not exist → compile error.
    // GREEN: variant + builder arm makes it compile and pass.
    #[test]
    fn proofs_query_returns_target_and_proves_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Invariant, "stock_never_negative"),
                GraphNode::new(NodeRef(2), NodeKind::Module, "unrelated"),
            ],
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves)],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Proofs {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("proofs build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(
            ids.contains(&0),
            "target checkout must be in proofs result; got: {ids:?}"
        );
        assert!(
            ids.contains(&1),
            "invariant stock_never_negative must be in proofs; got: {ids:?}"
        );
        assert!(
            !ids.contains(&2),
            "unrelated module must not appear; got: {ids:?}"
        );
    }

    // ── proofs_missing_target_returns_node_not_found ──────────────────────
    #[test]
    fn proofs_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Proofs {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── resources_query_returns_target_and_rw_nodes ───────────────────────
    // Spec: Resources query returns target plus Reads/Writes-reachable nodes.
    //
    // Graph: fn.process_file --Reads--> file.handle, --Writes--> file.output
    // Resources(fn.process_file) = {fn.process_file, file.handle, file.output}
    //
    // RED: ContextQuery::Resources did not exist → compile error.
    // GREEN: variant + builder arm makes it compile and pass.
    #[test]
    fn resources_query_returns_target_and_rw_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "process_file"),
                GraphNode::new(NodeRef(1), NodeKind::Type, "file.handle"),
                GraphNode::new(NodeRef(2), NodeKind::Type, "file.output"),
                GraphNode::new(NodeRef(3), NodeKind::Module, "unrelated"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Reads),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Writes),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Resources {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("resources build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(ids.contains(&1), "read dep must be in result; got: {ids:?}");
        assert!(
            ids.contains(&2),
            "write dep must be in result; got: {ids:?}"
        );
        assert!(!ids.contains(&3), "unrelated must not appear; got: {ids:?}");
    }

    // ── resources_missing_target_returns_node_not_found ───────────────────
    #[test]
    fn resources_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Resources {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── boundaries_query_returns_target_and_boundary_nodes ───────────────
    // Spec: Boundaries query returns target plus Boundary-kind nodes reachable
    // from it.
    //
    // Graph: module.checkout --DependsOn--> boundary.Stripe, --Calls--> fn.pay
    // Boundaries(module.checkout) = {module.checkout, boundary.Stripe}
    //                               (fn.pay is not a Boundary node)
    //
    // RED: ContextQuery::Boundaries did not exist → compile error.
    // GREEN: variant + builder arm makes it compile and pass.
    #[test]
    fn boundaries_query_returns_target_and_boundary_nodes() {
        use ail_core::semantic_graph::NodeKind;
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Module, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Boundary, "Stripe"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "pay"), // not boundary
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Calls),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Boundaries {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("boundaries build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(
            ids.contains(&0),
            "target module must be in result; got: {ids:?}"
        );
        assert!(
            ids.contains(&1),
            "Stripe boundary must be in result; got: {ids:?}"
        );
        assert!(
            !ids.contains(&2),
            "non-boundary fn.pay must not appear; got: {ids:?}"
        );
    }

    // ── boundaries_missing_target_returns_node_not_found ─────────────────
    #[test]
    fn boundaries_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Boundaries {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── why_query_returns_target_proves_breaks_and_history ───────────────
    // Spec: Why query traces provenance via Proves and BreaksIfChanged edges
    // and returns the snapshot history chain.
    //
    // Graph: fn.checkout --Proves--> invariant.paid, --BreaksIfChanged--> type.Cart
    // Why(fn.checkout) = {fn.checkout, invariant.paid, type.Cart} + history
    //
    // RED: ContextQuery::Why did not exist → compile error.
    // GREEN: variant + builder arm makes it compile and pass.
    #[test]
    fn why_query_returns_target_proves_breaks_and_history() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Invariant, "paid"),
                GraphNode::new(NodeRef(2), NodeKind::Type, "Cart"),
                GraphNode::new(NodeRef(3), NodeKind::Module, "unrelated"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::BreaksIfChanged),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Why {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp =
            ResponseBuilder::build_with_history(&query, &graph, &snapshot, &no_redactions(), &[])
                .expect("why build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(
            ids.contains(&1),
            "invariant.paid (Proves) must be in result; got: {ids:?}"
        );
        assert!(
            ids.contains(&2),
            "type.Cart (BreaksIfChanged) must be in result; got: {ids:?}"
        );
        assert!(!ids.contains(&3), "unrelated must not appear; got: {ids:?}");
        // Why query also returns the history chain (even if 1 entry for genesis).
        assert_eq!(
            resp.history_entries.len(),
            1,
            "why query must carry history_entries; got: {:?}",
            resp.history_entries.len()
        );
    }

    // ── why_missing_target_returns_node_not_found ────────────────────────
    #[test]
    fn why_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Why {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── refactor_context_query_returns_callers_proves_effects ─────────────
    // Spec: RefactorContext returns target + callers (to update) + proofs (to
    // rerun) + effects (to preserve).
    //
    // Graph: A --Calls--> B --Proves--> C, B --Emits--> D
    // RefactorContext(B) = {B, A(caller), C(proof), D(effect)}
    //
    // RED: ContextQuery::RefactorContext did not exist → compile error.
    // GREEN: variant + builder arm makes it compile and pass.
    #[test]
    fn refactor_context_query_returns_callers_proves_effects() {
        // A=0, B=1, C=2, D=3
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Invariant, "C"),
                GraphNode::new(NodeRef(3), NodeKind::Effect, "D"),
                GraphNode::new(NodeRef(4), NodeKind::Module, "unrelated"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls), // A calls B → A is a caller
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Proves), // B proves C → C is a proof
                GraphEdge::new(NodeRef(1), NodeRef(3), EdgeKind::Emits), // B emits D → D is an effect
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::RefactorContext {
            target: NodeRef(1), // B
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("refactor_context build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "caller A must be in result; got: {ids:?}");
        assert!(ids.contains(&1), "target B must be in result; got: {ids:?}");
        assert!(ids.contains(&2), "proof C must be in result; got: {ids:?}");
        assert!(ids.contains(&3), "effect D must be in result; got: {ids:?}");
        assert!(!ids.contains(&4), "unrelated must not appear; got: {ids:?}");
    }

    // ── refactor_context_missing_target_returns_node_not_found ───────────
    #[test]
    fn refactor_context_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::RefactorContext {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── runtime_query_returns_target_and_emits_nodes ─────────────────────
    // Spec: Runtime query returns target (with capability_reqs/effect_row) plus
    // effect nodes reachable via Emits edges.
    //
    // Graph: fn.checkout --Emits--> effect.payment, fn.checkout --Calls--> fn.pay
    // Runtime(fn.checkout) = {fn.checkout, effect.payment}
    //                        (fn.pay not via Emits, excluded)
    //
    // RED: ContextQuery::Runtime did not exist → compile error.
    // GREEN: variant + builder arm makes it compile and pass.
    #[test]
    fn runtime_query_returns_target_and_emits_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Effect, "payment"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "pay"), // Calls, not Emits
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Emits),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Calls),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Runtime {
            target: NodeRef(0),
            profile: "prod".to_string(),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("runtime build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(
            ids.contains(&0),
            "target checkout must be in result; got: {ids:?}"
        );
        assert!(
            ids.contains(&1),
            "effect.payment (Emits) must be in result; got: {ids:?}"
        );
        assert!(
            !ids.contains(&2),
            "fn.pay (Calls, not Emits) must not appear; got: {ids:?}"
        );
    }

    // ── runtime_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn runtime_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Runtime {
                target: NodeRef(99),
                profile: "prod".to_string(),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── g27_freshness_status_is_fresh_by_default ──────────────────────────
    // Spec: ResponseBuilder always sets freshness_status = Fresh (the default).
    //
    // TRIANGULATE: forces the builder to set the field.
    #[test]
    fn g27_freshness_status_is_fresh_by_default() {
        use crate::dto::FreshnessStatus;
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("build must succeed");
        assert_eq!(
            resp.freshness_status,
            FreshnessStatus::Fresh,
            "builder must set freshness_status = Fresh"
        );
    }

    // ── R2 TESTS ──────────────────────────────────────────────────────────
    // All tests below cover the 10 new query variants + rich response fields.

    // ── r2_diff_query_returns_all_nodes ───────────────────────────────────
    // Spec: Diff query returns structural differences between snapshots.
    // Without two materialised graphs, returns all nodes from current graph.
    #[test]
    fn r2_diff_query_returns_all_nodes() {
        let graph = make_graph(); // 3 nodes
        let snapshot = make_snapshot();
        let query = ContextQuery::Diff {
            snapshot_a: None,
            snapshot_b: None,
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("diff build must succeed");
        assert_eq!(
            resp.structured.len(),
            3,
            "diff query must return all nodes from current graph; got {:?}",
            resp.structured.len()
        );
    }

    // ── r2_diff_query_zero_budget_rejected ────────────────────────────────
    #[test]
    fn r2_diff_query_zero_budget_rejected() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Diff {
                snapshot_a: None,
                snapshot_b: None,
                budget: 0,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::InvalidBudget));
    }

    // ── r2_risks_query_returns_target_and_breaks_if_changed ───────────────
    // Spec: Risks query returns target + BreaksIfChanged-reachable nodes.
    //
    // Graph: A --BreaksIfChanged--> B, A --Calls--> C (Calls excluded)
    // Risks(A) = {A, B}
    #[test]
    fn r2_risks_query_returns_target_and_breaks_if_changed() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
                GraphNode::new(NodeRef(1), NodeKind::Type, "B"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::BreaksIfChanged),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Calls),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Risks {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("risks build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target A must be in result; got: {ids:?}");
        assert!(
            ids.contains(&1),
            "B (BreaksIfChanged) must be in result; got: {ids:?}"
        );
        assert!(
            !ids.contains(&2),
            "C (Calls, not risk) must not appear; got: {ids:?}"
        );
    }

    // ── r2_risks_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn r2_risks_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Risks {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_todo_query_returns_target_and_proves_nodes ─────────────────────
    // Spec: Todo query returns outstanding proof obligations.
    //
    // Graph: fn.checkout --Proves--> invariant.stock
    // Todo(fn.checkout) = {fn.checkout, invariant.stock}
    #[test]
    fn r2_todo_query_returns_target_and_proves_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Invariant, "stock"),
                GraphNode::new(NodeRef(2), NodeKind::Module, "unrelated"),
            ],
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves)],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Todo {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("todo build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in todo; got: {ids:?}");
        assert!(
            ids.contains(&1),
            "proves node must be in todo; got: {ids:?}"
        );
        assert!(!ids.contains(&2), "unrelated must not appear; got: {ids:?}");
    }

    // ── r2_todo_missing_target_returns_node_not_found ─────────────────────
    #[test]
    fn r2_todo_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Todo {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_capabilities_query_returns_target_and_emits_depends ───────────
    // Spec: Capabilities returns target + Emits + DependsOn reachable nodes.
    //
    // Graph: module --Emits--> cap.payment, module --DependsOn--> dep.db
    //        module --Calls--> fn.pay (excluded)
    // Capabilities(module) = {module, cap.payment, dep.db}
    #[test]
    fn r2_capabilities_query_returns_target_and_emits_depends() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Module, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Capability, "payment"),
                GraphNode::new(NodeRef(2), NodeKind::Module, "db"),
                GraphNode::new(NodeRef(3), NodeKind::Function, "pay"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Emits),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::DependsOn),
                GraphEdge::new(NodeRef(0), NodeRef(3), EdgeKind::Calls), // excluded
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Capabilities {
            target: NodeRef(0),
            profile: "prod".to_string(),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("capabilities build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(ids.contains(&1), "Emits node must appear; got: {ids:?}");
        assert!(ids.contains(&2), "DependsOn node must appear; got: {ids:?}");
        assert!(
            !ids.contains(&3),
            "Calls node must not appear; got: {ids:?}"
        );
    }

    // ── r2_capabilities_missing_target_returns_node_not_found ────────────
    #[test]
    fn r2_capabilities_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Capabilities {
                target: NodeRef(99),
                profile: "prod".to_string(),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_handlers_query_returns_target_and_reverse_callers ─────────────
    // Spec: Handlers returns target + nodes that call it (handler bindings).
    //
    // Graph: handler_A --Calls--> cap.payment, handler_B --Calls--> cap.payment
    // Handlers(cap.payment) = {cap.payment, handler_A, handler_B}
    #[test]
    fn r2_handlers_query_returns_target_and_reverse_callers() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Capability, "payment"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "handler_A"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "handler_B"),
                GraphNode::new(NodeRef(3), NodeKind::Module, "unrelated"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::Calls),
                GraphEdge::new(NodeRef(2), NodeRef(0), EdgeKind::Calls),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Handlers {
            target: NodeRef(0),
            profile: "prod".to_string(),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("handlers build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(
            ids.contains(&0),
            "target cap.payment must be in result; got: {ids:?}"
        );
        assert!(
            ids.contains(&1),
            "handler_A must be in result; got: {ids:?}"
        );
        assert!(
            ids.contains(&2),
            "handler_B must be in result; got: {ids:?}"
        );
        assert!(!ids.contains(&3), "unrelated must not appear; got: {ids:?}");
    }

    // ── r2_handlers_missing_target_returns_node_not_found ────────────────
    #[test]
    fn r2_handlers_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Handlers {
                target: NodeRef(99),
                profile: "prod".to_string(),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_concurrency_query_returns_reads_writes_calls ───────────────────
    // Spec: Concurrency returns target + Reads/Writes/Calls reachable nodes.
    //
    // Graph: fn.process --Reads--> state, --Writes--> output, --Calls--> fn.sub
    // Concurrency(fn.process) = {fn.process, state, output, fn.sub}
    #[test]
    fn r2_concurrency_query_returns_reads_writes_calls() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "process"),
                GraphNode::new(NodeRef(1), NodeKind::Type, "state"),
                GraphNode::new(NodeRef(2), NodeKind::Type, "output"),
                GraphNode::new(NodeRef(3), NodeKind::Function, "sub"),
                GraphNode::new(NodeRef(4), NodeKind::Module, "unrelated"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Reads),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Writes),
                GraphEdge::new(NodeRef(0), NodeRef(3), EdgeKind::Calls),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Concurrency {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("concurrency build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(ids.contains(&1), "Reads node must appear; got: {ids:?}");
        assert!(ids.contains(&2), "Writes node must appear; got: {ids:?}");
        assert!(ids.contains(&3), "Calls node must appear; got: {ids:?}");
        assert!(!ids.contains(&4), "unrelated must not appear; got: {ids:?}");
    }

    // ── r2_concurrency_missing_target_returns_node_not_found ─────────────
    #[test]
    fn r2_concurrency_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Concurrency {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_tasks_query_returns_calls_and_emits ────────────────────────────
    // Spec: Tasks returns target + Calls/Emits reachable nodes (async tasks).
    //
    // Graph: fn.fetch --Calls--> fn.sub_task, --Emits--> effect.io
    //        fn.fetch --Reads--> state (excluded — not Calls/Emits)
    // Tasks(fn.fetch) = {fn.fetch, fn.sub_task, effect.io}
    #[test]
    fn r2_tasks_query_returns_calls_and_emits() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "fetch"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "sub_task"),
                GraphNode::new(NodeRef(2), NodeKind::Effect, "io"),
                GraphNode::new(NodeRef(3), NodeKind::Type, "state"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Emits),
                GraphEdge::new(NodeRef(0), NodeRef(3), EdgeKind::Reads),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Tasks {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("tasks build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(ids.contains(&1), "Calls sub_task must appear; got: {ids:?}");
        assert!(ids.contains(&2), "Emits io must appear; got: {ids:?}");
        assert!(
            !ids.contains(&3),
            "Reads state (not Calls/Emits) must not appear; got: {ids:?}"
        );
    }

    // ── r2_tasks_missing_target_returns_node_not_found ────────────────────
    #[test]
    fn r2_tasks_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Tasks {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_assumptions_query_returns_boundary_nodes ───────────────────────
    // Spec: Assumptions returns trust assumption nodes (Boundary kind) reachable
    // from target.
    //
    // Graph: module --DependsOn--> boundary.Stripe, --DependsOn--> fn.pay (not Boundary)
    // Assumptions(module) = {module, boundary.Stripe}
    #[test]
    fn r2_assumptions_query_returns_boundary_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Module, "checkout"),
                GraphNode::new(NodeRef(1), NodeKind::Boundary, "Stripe"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "pay"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
                GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::DependsOn),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::Assumptions {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("assumptions build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(
            ids.contains(&1),
            "boundary Stripe must appear; got: {ids:?}"
        );
        assert!(
            !ids.contains(&2),
            "non-boundary fn.pay must not appear; got: {ids:?}"
        );
    }

    // ── r2_assumptions_missing_target_returns_node_not_found ─────────────
    #[test]
    fn r2_assumptions_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::Assumptions {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_extract_candidates_returns_inner_nodes_without_external_callers ─
    // Spec: ExtractCandidates returns sub-nodes of target with no external callers.
    //
    // Graph: target(0) --Calls--> inner(1), inner(1) has no external caller.
    //        outer(2) --Calls--> inner(1) would make it non-candidate (excluded).
    // ExtractCandidates(0) = {0, 1} — inner has only 0 as caller (within scope).
    #[test]
    fn r2_extract_candidates_no_external_callers() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "target"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "inner"),
                GraphNode::new(NodeRef(2), NodeKind::Module, "unrelated"),
            ],
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls)],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::ExtractCandidates {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("extract_candidates build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(
            ids.contains(&1),
            "inner (no external callers) must be a candidate; got: {ids:?}"
        );
        assert!(!ids.contains(&2), "unrelated must not appear; got: {ids:?}");
    }

    // ── r2_extract_candidates_excludes_externally_called_nodes ───────────
    // TRIANGULATE: a node called by an external caller is excluded.
    //
    // Graph: target(0) --Calls--> inner(1), external(2) --Calls--> inner(1)
    // ExtractCandidates(0) = {0} only — inner has external caller (2).
    #[test]
    fn r2_extract_candidates_excludes_externally_called_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "target"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "inner"),
                GraphNode::new(NodeRef(2), NodeKind::Module, "external"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(2), NodeRef(1), EdgeKind::Calls),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::ExtractCandidates {
            target: NodeRef(0),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("extract_candidates build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
        assert!(
            !ids.contains(&1),
            "inner with external caller must NOT be a candidate; got: {ids:?}"
        );
    }

    // ── r2_extract_candidates_missing_target_returns_node_not_found ───────
    #[test]
    fn r2_extract_candidates_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::ExtractCandidates {
                target: NodeRef(99),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ── r2_move_safety_returns_target_destination_callers_contracts_effects ─
    // Spec: MoveSafety returns target + destination + callers + contracts + effects.
    //
    // Graph: caller(0) --Calls--> target(1), target(1) --Proves--> contract(2),
    //        target(1) --Emits--> effect(3), destination(4) exists.
    // MoveSafety(target=1, dest=4) = {0,1,2,3,4}
    #[test]
    fn r2_move_safety_returns_all_affected_nodes() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "caller"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "target_fn"),
                GraphNode::new(NodeRef(2), NodeKind::Invariant, "contract"),
                GraphNode::new(NodeRef(3), NodeKind::Effect, "effect"),
                GraphNode::new(NodeRef(4), NodeKind::Module, "destination"),
            ],
            edges: vec![
                GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
                GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Proves),
                GraphEdge::new(NodeRef(1), NodeRef(3), EdgeKind::Emits),
            ],
        };
        let snapshot = make_snapshot();
        let query = ContextQuery::MoveSafety {
            target: NodeRef(1),
            destination: NodeRef(4),
            budget: usize::MAX,
        };
        let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
            .expect("move_safety build must succeed");
        let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
        assert!(ids.contains(&0), "caller must be in result; got: {ids:?}");
        assert!(ids.contains(&1), "target must be in result; got: {ids:?}");
        assert!(ids.contains(&2), "contract must be in result; got: {ids:?}");
        assert!(ids.contains(&3), "effect must be in result; got: {ids:?}");
        assert!(
            ids.contains(&4),
            "destination must be in result; got: {ids:?}"
        );
    }

    // ── r2_move_safety_missing_target_returns_node_not_found ─────────────
    #[test]
    fn r2_move_safety_missing_target_returns_node_not_found() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let result = ResponseBuilder::build(
            &ContextQuery::MoveSafety {
                target: NodeRef(99),
                destination: NodeRef(0),
                budget: usize::MAX,
            },
            &graph,
            &snapshot,
            &no_redactions(),
        );
        assert_eq!(result, Err(ContextError::NodeNotFound));
    }

    // ─────────────────────────────────────────────────────────────────────
    // R2 FEATURE TESTS: generated_at, provenance, redaction state, security,
    //                   freshness detection, repair options, index reporting.
    // ─────────────────────────────────────────────────────────────────────

    // ── r2_generated_at_is_populated ─────────────────────────────────────
    // Spec: generated_at is set in the response envelope.
    #[test]
    fn r2_generated_at_is_populated() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            generated_at: 99_000,
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build_full must succeed");
        assert_eq!(
            resp.generated_at, 99_000,
            "generated_at must match opts value"
        );
    }

    // ── r2_freshness_stale_when_latest_differs ────────────────────────────
    // Spec: freshness_status is Stale when latest_snapshot_id != snapshot.id
    #[test]
    fn r2_freshness_stale_when_latest_differs() {
        use crate::dto::FreshnessStatus;
        let graph = make_graph();
        let snapshot = make_snapshot(); // id = "builder-snap"
        let other_id = ObjectId::from_bytes(b"other-snap");
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            latest_snapshot_id: Some(&other_id),
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build_full must succeed");
        assert_eq!(
            resp.freshness_status,
            FreshnessStatus::Stale,
            "freshness_status must be Stale when latest differs; got: {:?}",
            resp.freshness_status
        );
    }

    // ── r2_freshness_fresh_when_latest_matches ────────────────────────────
    // TRIANGULATE: freshness_status is Fresh when latest == snapshot.id
    #[test]
    fn r2_freshness_fresh_when_latest_matches() {
        use crate::dto::FreshnessStatus;
        let graph = make_graph();
        let snapshot = make_snapshot();
        let snap_id = snapshot.id;
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            latest_snapshot_id: Some(&snap_id),
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build_full must succeed");
        assert_eq!(
            resp.freshness_status,
            FreshnessStatus::Fresh,
            "freshness_status must be Fresh when latest matches; got: {:?}",
            resp.freshness_status
        );
    }

    // ── r2_stale_response_has_repair_option ───────────────────────────────
    // Spec: Stale response must include a query_latest repair option.
    #[test]
    fn r2_stale_response_has_repair_option() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let other_id = ObjectId::from_bytes(b"newer-snap");
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            latest_snapshot_id: Some(&other_id),
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build_full must succeed");
        assert!(
            resp.repair_options
                .iter()
                .any(|r| r.option_id == "query_latest"),
            "stale response must contain query_latest repair option; got: {:?}",
            resp.repair_options
        );
    }

    // ── r2_truncated_response_has_narrow_scope_repair_option ─────────────
    // Spec: Truncated response must include a narrow_scope repair option.
    #[test]
    fn r2_truncated_response_has_narrow_scope_repair_option() {
        let graph = make_graph(); // 3 nodes
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: 1,
        }; // too small
        let opts = BuildOptions {
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build_full must succeed even when truncated");
        assert!(resp.truncated, "response must be truncated with budget=1");
        assert!(
            resp.repair_options
                .iter()
                .any(|r| r.option_id == "narrow_scope"),
            "truncated response must contain narrow_scope repair option; got: {:?}",
            resp.repair_options
        );
    }

    // ── r2_access_denied_for_unauthorized_redacted_target ─────────────────
    // Spec: E_ACCESS_DENIED when unauthorized and target is redacted.
    #[test]
    fn r2_access_denied_for_unauthorized_redacted_target() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let mut redacted = BTreeSet::new();
        redacted.insert(NodeRef(0)); // redact the target
        let query = ContextQuery::Node {
            target: NodeRef(0),
            scope: QueryScope::Local,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: false,
            ..Default::default()
        };
        let result = ResponseBuilder::build_full(&query, &graph, &snapshot, &redacted, &opts);
        assert_eq!(
            result,
            Err(ContextError::AccessDenied),
            "unauthorized access to redacted target must return E_ACCESS_DENIED"
        );
    }

    // ── r2_authorized_caller_can_access_redacted_target ───────────────────
    // TRIANGULATE: authorized caller succeeds even when target is redacted
    // (the node is removed from structured, but E_ACCESS_DENIED is not raised).
    #[test]
    fn r2_authorized_caller_can_access_redacted_target() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let mut redacted = BTreeSet::new();
        redacted.insert(NodeRef(0));
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &redacted, &opts)
            .expect("authorized call must succeed");
        assert!(resp.redacted, "redacted flag must be set");
        // Node 0 must be absent from structured.
        assert!(
            !resp.structured.iter().any(|n| n.id == NodeRef(0)),
            "redacted node must not appear in structured"
        );
    }

    // ── r2_redaction_policy_wired_into_response ───────────────────────────
    // Spec: RedactionPolicy is attached to the response when supplied.
    #[test]
    fn r2_redaction_policy_wired_into_response() {
        use crate::dto::{RedactionPolicy, RedactionState};
        let graph = make_graph();
        let snapshot = make_snapshot();
        let mut redacted = BTreeSet::new();
        redacted.insert(NodeRef(1)); // redact node 1
        let policy = RedactionPolicy {
            label: "PII".to_string(),
            categories: vec!["secrets".to_string()],
            requires_approval: false,
        };
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            redaction_policy: Some(&policy),
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &redacted, &opts)
            .expect("build must succeed");
        assert_eq!(
            resp.redaction_state,
            RedactionState::Partial,
            "redaction_state must be Partial when nodes are withheld; got: {:?}",
            resp.redaction_state
        );
        assert_eq!(
            resp.redaction_policy,
            Some(policy),
            "redaction_policy must be wired into the response"
        );
    }

    // ── r2_restricted_policy_sets_restricted_state ────────────────────────
    // Spec: requires_approval=true → RedactionState::Restricted
    #[test]
    fn r2_restricted_policy_sets_restricted_state() {
        use crate::dto::{RedactionPolicy, RedactionState};
        let graph = make_graph();
        let snapshot = make_snapshot();
        let policy = RedactionPolicy {
            label: "internal".to_string(),
            categories: vec!["audit_logs".to_string()],
            requires_approval: true,
        };
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            redaction_policy: Some(&policy),
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build must succeed");
        assert_eq!(
            resp.redaction_state,
            RedactionState::Restricted,
            "requires_approval policy must produce Restricted state; got: {:?}",
            resp.redaction_state
        );
    }

    // ── r2_provenance_block_includes_semantic_graph_source ────────────────
    // Spec: provenance.sources always includes "semantic_graph".
    #[test]
    fn r2_provenance_block_includes_semantic_graph_source() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build must succeed");
        assert!(
            resp.provenance
                .sources
                .iter()
                .any(|s| s == "semantic_graph"),
            "provenance must contain semantic_graph source; got: {:?}",
            resp.provenance.sources
        );
    }

    // ── r2_provenance_block_includes_extra_sources ────────────────────────
    // TRIANGULATE: extra sources supplied in opts are preserved.
    #[test]
    fn r2_provenance_block_includes_extra_sources() {
        let graph = make_graph();
        let snapshot = make_snapshot();
        let extra_sources = vec![
            "verification_reports".to_string(),
            "runtime_profiles".to_string(),
        ];
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            provenance_sources: &extra_sources,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build must succeed");
        assert!(
            resp.provenance
                .sources
                .iter()
                .any(|s| s == "verification_reports"),
            "provenance must contain verification_reports; got: {:?}",
            resp.provenance.sources
        );
    }

    // ── r2_index_info_attached_to_provenance ──────────────────────────────
    // Spec: index versions/hashes are listed in provenance.indexes.
    #[test]
    fn r2_index_info_attached_to_provenance() {
        use crate::dto::IndexInfo;
        let graph = make_graph();
        let snapshot = make_snapshot();
        let indexes = vec![IndexInfo {
            kind: "call_graph".to_string(),
            hash: [0u8; 32],
            stale: false,
        }];
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            index_info: &indexes,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build must succeed");
        assert_eq!(
            resp.provenance.indexes.len(),
            1,
            "provenance.indexes must contain the supplied index info"
        );
        assert_eq!(resp.provenance.indexes[0].kind, "call_graph");
    }

    // ── r2_stale_index_triggers_rebuild_repair_option ─────────────────────
    // Spec: stale index should trigger rebuild_index repair option.
    #[test]
    fn r2_stale_index_triggers_rebuild_repair_option() {
        use crate::dto::IndexInfo;
        let graph = make_graph();
        let snapshot = make_snapshot();
        let indexes = vec![IndexInfo {
            kind: "call_graph".to_string(),
            hash: [0u8; 32],
            stale: true, // stale!
        }];
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let opts = BuildOptions {
            authorized: true,
            index_info: &indexes,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build must succeed");
        assert!(
            resp.repair_options
                .iter()
                .any(|r| r.option_id == "rebuild_index"),
            "stale index must generate rebuild_index repair option; got: {:?}",
            resp.repair_options
        );
    }

    // ── r2_new_query_variants_cbor_roundtrip ──────────────────────────────
    // All R2 query variants must survive CBOR roundtrip.
    #[test]
    fn r2_new_query_variants_cbor_roundtrip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let variants: Vec<ContextQuery> = vec![
            ContextQuery::Diff {
                snapshot_a: None,
                snapshot_b: None,
                budget: 1024,
            },
            ContextQuery::Risks {
                target: NodeRef(1),
                budget: 512,
            },
            ContextQuery::Todo {
                target: NodeRef(2),
                budget: 256,
            },
            ContextQuery::Capabilities {
                target: NodeRef(3),
                profile: "prod".to_string(),
                budget: 2048,
            },
            ContextQuery::Handlers {
                target: NodeRef(4),
                profile: "dev".to_string(),
                budget: 4096,
            },
            ContextQuery::Concurrency {
                target: NodeRef(5),
                budget: 512,
            },
            ContextQuery::Tasks {
                target: NodeRef(6),
                budget: 1024,
            },
            ContextQuery::Assumptions {
                target: NodeRef(7),
                budget: 2048,
            },
            ContextQuery::ExtractCandidates {
                target: NodeRef(8),
                budget: 4096,
            },
            ContextQuery::MoveSafety {
                target: NodeRef(9),
                destination: NodeRef(10),
                budget: 8192,
            },
        ];
        for q in &variants {
            let bytes = codec.encode(q).expect("encode must succeed");
            let decoded: ContextQuery = codec.decode(&bytes).expect("decode must succeed");
            assert_eq!(decoded, *q, "{q:?} must survive CBOR roundtrip");
        }
    }

    // ── r2_redaction_state_cbor_roundtrip ─────────────────────────────────
    // RedactionState enum must survive CBOR roundtrip.
    #[test]
    fn r2_redaction_state_cbor_roundtrip() {
        use crate::dto::RedactionState;
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        for state in [
            RedactionState::None,
            RedactionState::Partial,
            RedactionState::Restricted,
        ] {
            let bytes = codec.encode(&state).expect("encode must succeed");
            let decoded: RedactionState = codec.decode(&bytes).expect("decode must succeed");
            assert_eq!(decoded, state, "{state:?} must survive CBOR roundtrip");
        }
    }

    // ── r2_provenance_block_cbor_roundtrip ────────────────────────────────
    // ProvenanceBlock must survive CBOR roundtrip.
    #[test]
    fn r2_provenance_block_cbor_roundtrip() {
        use crate::dto::{IndexInfo, ProvenanceBlock};
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let prov = ProvenanceBlock {
            sources: vec![
                "semantic_graph".to_string(),
                "verification_reports".to_string(),
            ],
            indexes: vec![IndexInfo {
                kind: "call_graph".to_string(),
                hash: [1u8; 32],
                stale: false,
            }],
            reports: vec![[2u8; 32]],
        };
        let bytes = codec.encode(&prov).expect("encode must succeed");
        let decoded: ProvenanceBlock = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, prov, "ProvenanceBlock must survive CBOR roundtrip");
    }

    // ── r2_repair_option_cbor_roundtrip ───────────────────────────────────
    // RepairOption must survive CBOR roundtrip.
    #[test]
    fn r2_repair_option_cbor_roundtrip() {
        use crate::dto::RepairOption;
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let opt = RepairOption {
            option_id: "query_latest".to_string(),
            description: "Re-issue at latest snapshot".to_string(),
            suggested_query: Some("context fn.checkout snapshot=latest".to_string()),
        };
        let bytes = codec.encode(&opt).expect("encode must succeed");
        let decoded: RepairOption = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, opt, "RepairOption must survive CBOR roundtrip");
    }

    // ── r2_full_response_cbor_roundtrip ───────────────────────────────────
    // ContextResponse with all new R2 fields must survive CBOR roundtrip.
    #[test]
    fn r2_full_response_cbor_roundtrip() {
        use crate::dto::{
            FreshnessStatus, IndexInfo, ProvenanceBlock, RedactionState, RepairOption,
        };
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = make_graph();
        let snapshot = make_snapshot();
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: usize::MAX,
        };
        let other_id = ObjectId::from_bytes(b"other");
        let sources = vec!["semantic_graph".to_string()];
        let indexes = vec![IndexInfo {
            kind: "call_graph".to_string(),
            hash: [0u8; 32],
            stale: false,
        }];
        let opts = BuildOptions {
            authorized: true,
            latest_snapshot_id: Some(&other_id), // force Stale
            generated_at: 12345,
            provenance_sources: &sources,
            index_info: &indexes,
            ..Default::default()
        };
        let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
            .expect("build must succeed");
        assert_eq!(resp.freshness_status, FreshnessStatus::Stale);

        let bytes = codec.encode(&resp).expect("encode must succeed");
        let decoded: crate::dto::ContextResponse =
            codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, resp,
            "full R2 ContextResponse must survive CBOR roundtrip"
        );
    }
}
