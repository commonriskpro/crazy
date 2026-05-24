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

use std::collections::BTreeSet;

use ail_core::semantic_graph::{NodeRef, SemanticGraph};
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::SnapshotEnvelope;

use crate::assembly::{BudgetResult, apply_budget, assemble_provenance};
use crate::dto::{
    CONTEXT_SCHEMA_V1, ContextQuery, ContextResponse, FreshnessStatus, ImpactInfo, RedactionPolicy,
    RefactorInfo, ResponseLimits,
};
use crate::error::{ContextError, ContextResult};
use crate::freshness::{build_repair_options, resolve_freshness_status};
use crate::redaction::{compute_redaction_state, filter_redacted};
use crate::selection::{
    collect_candidates_with_history, compute_impact_info, compute_refactor_info,
};
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
    /// Explicit freshness status when the caller attempted freshness detection
    /// but could not determine the latest snapshot.
    pub freshness_status: Option<FreshnessStatus>,
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
    /// When `opts.freshness_status` is `Some`, that explicit status is used.
    /// Otherwise, when `opts.latest_snapshot_id` is `Some`, the response
    /// `freshness_status` is set to `Stale` if the snapshot id does not match.
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
        let (unredacted, redacted) = filter_redacted(candidates, redacted_refs);

        // ── Step 3a: Compute query-specific info from pre-truncation set ──
        // These info structs are derived from the full unredacted candidate
        // set so they are accurate even when the structured layer is truncated.
        let impact_info: Option<ImpactInfo> = match query {
            ContextQuery::Impact { .. } => Some(compute_impact_info(&unredacted)),
            _ => None,
        };
        let refactor_info: Option<RefactorInfo> = match query {
            ContextQuery::RefactorContext { target, .. }
            | ContextQuery::ExtractCandidates { target, .. }
            | ContextQuery::MoveSafety { target, .. } => {
                Some(compute_refactor_info(&unredacted, *target, graph))
            }
            _ => None,
        };

        // ── Step 4: Apply byte budget ─────────────────────────────────────
        let BudgetResult {
            structured,
            bytes_used: total_bytes,
            truncated,
            omitted_sections,
        } = apply_budget(unredacted, budget, &codec)?;

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
        let freshness_status =
            resolve_freshness_status(opts.freshness_status, opts.latest_snapshot_id, snapshot.id);

        // ── Step 9: Build repair options for stale responses ──────────────
        let has_stale_index = opts.index_info.iter().any(|i| i.stale);
        let repair_options =
            build_repair_options(freshness_status, truncated, query, has_stale_index);

        // ── Step 10: Redaction state and policy wiring ────────────────────
        let redaction_state = compute_redaction_state(opts.redaction_policy, redacted);
        let redaction_policy = opts.redaction_policy.cloned();

        // ── Step 11: Provenance block ─────────────────────────────────────
        let provenance = assemble_provenance(
            opts.provenance_sources,
            opts.index_info,
            snapshot.verification_report_hash,
        );

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
            impact_info,
            refactor_info,
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
