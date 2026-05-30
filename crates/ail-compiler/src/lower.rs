// ── ail-compiler::lower ───────────────────────────────────────────────────
//
// Pipeline lowering functions: SemanticGraph → CoreIr → AnfIr.
//
// # Pre-condition (both functions)
//
// `lower_to_core_ir` MUST be called with a `VerificationReport` whose
// `summary()` is `Proven` or `RuntimeChecked`.  Any other summary causes
// `Err(CompileError::RejectedReport)` to be returned immediately.
//
// # Hash chain contract
//
// - `graph_snapshot_hash    = blake3(graph_cbor_bytes)`
// - `verification_report_hash = blake3(report_cbor_bytes)`
// - `core_ir_hash           = blake3(graph_snapshot_hash || core_ir_bytes)`
// - `anf_ir_hash            = blake3(core_ir_hash || anf_ir_bytes)`
//
// # Determinism contract
//
// All collections are `Vec` / `BTreeMap` — never `HashMap`.
// `stable_cbor_bytes` + BLAKE3 gives byte-identical output across runs.

mod lower_expr;
mod lower_items;

// ── Public / crate-visible re-exports ────────────────────────────────────
//
// These re-exports preserve the original `crate::lower::*` public surface so
// that callers in `lib.rs`, `incremental.rs`, and integration tests are
// unaffected by the internal split.

pub use lower_expr::lower_core_expr_to_anf;
pub(crate) use lower_items::map_node_kind;
pub use lower_items::nominal_to_core_type;

// ── Private imports used only within this module ──────────────────────────

use std::collections::BTreeMap;

use ail_core::semantic_graph::{GraphValidationError, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::{VerificationReport, VerificationState};

use crate::anf::{ANF_SCHEMA_VERSION, AnfBinding, AnfIr, SourceMap};
use crate::core_ir::{CoreIr, CoreNode, StageHashes};
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
use crate::optimize::optimize_bindings;

use lower_items::{
    NodeProvenance, build_enriched_source_map, expr_from_graph_node, extract_provenance_lookup,
    map_core_node_to_anf,
};

// ── is_report_accepted ────────────────────────────────────────────────────

/// Return `true` if `report.summary()` is `Proven` or `RuntimeChecked`.
///
/// All other summary states (`Assumed`, `Unverified`, `Unsafe`, `Failed`)
/// are treated as rejected.  This is a local policy helper for Phase 7;
/// a future top-level `VerificationReport.status` field should replace it.
pub fn is_report_accepted(report: &VerificationReport) -> bool {
    matches!(
        report.summary(),
        VerificationState::Proven | VerificationState::RuntimeChecked
    )
}

// ── lower_to_core_ir ──────────────────────────────────────────────────────

/// Lower a verified `SemanticGraph` into a `CoreIr`.
///
/// # Pre-conditions
///
/// - `report.summary()` must be `Proven` or `RuntimeChecked`; any other
///   summary immediately returns `Err(CompileError::RejectedReport)`.
///
/// # Hash chain
///
/// Seals `core_ir_hash = blake3(graph_snapshot_hash || core_ir_bytes)`.
/// `graph_snapshot_hash` and `verification_report_hash` are recorded in
/// `StageHashes` for downstream stages.
///
/// # Errors
///
/// - `CompileError::RejectedReport` — report summary is not accepted.
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_core_ir(
    graph: &SemanticGraph,
    report: &VerificationReport,
) -> Result<CoreIr, CompileError> {
    // Gate: reject unacceptable reports.
    if !is_report_accepted(report) {
        return Err(CompileError::RejectedReport);
    }

    // Gate: validate graph structural invariants (unique refs, no dangling edges).
    graph.validate().map_err(|e| match e {
        GraphValidationError::DuplicateRef(r) => {
            CompileError::InvalidGraph(format!("duplicate NodeRef({})", r.0))
        }
        GraphValidationError::DanglingEdge { r#ref, .. } => CompileError::MissingNode(r#ref),
        GraphValidationError::EffectRowNoEmitsEdge(r) => CompileError::InvalidGraph(format!(
            "effect_row declared but no Emits edge on NodeRef({})",
            r.0
        )),
        GraphValidationError::CapabilityReqsMissingNode {
            owner_ref,
            cap_name,
        } => CompileError::InvalidGraph(format!(
            "capability '{}' required by NodeRef({}) has no matching Capability node",
            cap_name, owner_ref.0
        )),
    })?;

    // Hash the pipeline inputs (empty parent → blake3(content)).
    let graph_cbor = stable_cbor_bytes(graph)?;
    let graph_snapshot_hash = hash_with_parent(&[], &graph_cbor);

    let report_cbor = stable_cbor_bytes(report)?;
    let verification_report_hash = hash_with_parent(&[], &report_cbor);

    // Lower each source GraphNode to a CoreNode (1-to-1, in traversal order).
    // G2: populate `ty` from `GraphNode.type_facts.nominal` when present.
    // Function `body_expr` strings are parsed into executable CoreExpr bodies;
    // legacy literal runtime-check bodies are preserved as a fallback.
    let nodes: Vec<CoreNode> = graph
        .nodes
        .iter()
        .map(|gn| {
            Ok(CoreNode {
                source_ref: gn.id,
                kind: map_node_kind(gn.kind),
                name: gn.name.clone(),
                ty: gn
                    .type_facts
                    .as_ref()
                    .map(|tf| nominal_to_core_type(&tf.nominal)),
                expr: if gn.kind == NodeKind::Function {
                    expr_from_graph_node(gn)?
                } else {
                    None
                },
            })
        })
        .collect::<Result<Vec<_>, CompileError>>()?;

    // Seal: core_ir_hash = blake3(graph_snapshot_hash || core_ir_bytes).
    let core_ir_bytes = stable_cbor_bytes(&nodes)?;
    let core_ir_hash = hash_with_parent(&graph_snapshot_hash, &core_ir_bytes);

    Ok(CoreIr {
        nodes,
        stage_hashes: StageHashes {
            graph_snapshot_hash,
            verification_report_hash,
            core_ir_hash,
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    })
}

// ── lower_to_anf_impl ────────────────────────────────────────────────────

/// Shared ANF lowering implementation.
///
/// When `provenance_lookup` is `Some`, each `SourceMapEntry` is enriched with
/// semantic provenance extracted from the original `SemanticGraph`.  When it
/// is `None`, all optional provenance fields remain `None` (backward-compat
/// path used by `lower_to_anf`).
fn lower_to_anf_impl(
    core: &CoreIr,
    provenance_lookup: Option<&BTreeMap<NodeRef, NodeProvenance>>,
) -> Result<AnfIr, CompileError> {
    // Lower each CoreNode — collecting synthetic temporaries and node bindings.
    let mut bindings: Vec<AnfBinding> = Vec::with_capacity(core.nodes.len());
    let mut fresh: u32 = 0;
    for node in &core.nodes {
        map_core_node_to_anf(node, &mut fresh, &mut bindings);
    }
    let bindings = optimize_bindings(bindings);

    // Build semantic source map, optionally enriched with provenance.
    let source_map = match provenance_lookup {
        Some(lookup) => build_enriched_source_map(&bindings, lookup),
        None => SourceMap::from_bindings(&bindings),
    };

    // Seal: anf_ir_hash = blake3(core_ir_hash || anf_ir_bytes).
    // Note: anf_ir_hash covers *bindings* only, not the source map, so it is
    // identical whether or not provenance enrichment is applied.
    let anf_ir_bytes = stable_cbor_bytes(&bindings)?;
    let anf_ir_hash = hash_with_parent(&core.stage_hashes.core_ir_hash, &anf_ir_bytes);

    // Extend the stage hashes from Core IR.
    let mut stage_hashes = core.stage_hashes.clone();
    stage_hashes.anf_ir_hash = Some(anf_ir_hash);

    Ok(AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings,
        source_map,
        stage_hashes,
    })
}

// ── lower_to_anf ─────────────────────────────────────────────────────────

/// Normalize a `CoreIr` into Administrative Normal Form (`AnfIr`).
///
/// # Pure transformation
///
/// No side effects, no I/O.  The same `CoreIr` always produces an identical
/// `AnfIr` (same bindings, same `anf_ir_hash`).
///
/// # Provenance
///
/// Every `AnfBinding.source_ref` is copied verbatim from its `CoreNode`
/// counterpart.  The mapping is 1-to-1 in Phase 7; structural normalisation
/// (e.g. let-bindings for sub-expressions) is deferred to Phase 8.
///
/// All `SourceMapEntry` optional provenance fields (`block_ref`, `change_set`,
/// etc.) remain `None` in this path.  Use `lower_to_anf_with_graph` when the
/// original `SemanticGraph` is available to get enriched provenance.
///
/// # Hash chain
///
/// Extends the chain: `anf_ir_hash = blake3(core_ir_hash || anf_ir_bytes)`.
///
/// # Errors
///
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_anf(core: &CoreIr) -> Result<AnfIr, CompileError> {
    lower_to_anf_impl(core, None)
}

// ── lower_to_anf_with_graph ───────────────────────────────────────────────

/// Normalize a `CoreIr` into ANF with full semantic provenance enrichment.
///
/// Identical to `lower_to_anf` in every respect except that each
/// `SourceMapEntry` is enriched with provenance data extracted from the
/// original `SemanticGraph`:
///
/// | `SourceMapEntry` field  | Source in `GraphNode`                        |
/// |-------------------------|----------------------------------------------|
/// | `change_set`            | `provenance.change_id`                       |
/// | `block_ref`             | Derived: `"block.<name>"` for Module/Boundary|
/// | `contract_ref`          | Derived: `"contract.<name>"` when clauses set|
/// | `effect_ref`            | First effect in `effect_row.effects`         |
/// | `runtime_check_ref`     | First `RuntimeCheckMeta.hash`                |
/// | `proof_obligation_ref`  | Always `None` — upstream not producing yet   |
///
/// # Hash chain
///
/// `anf_ir_hash` is identical to the one produced by `lower_to_anf` because
/// it covers only the bindings, not the source-map provenance fields.
///
/// # Errors
///
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_anf_with_graph(
    core: &CoreIr,
    graph: &SemanticGraph,
) -> Result<AnfIr, CompileError> {
    let lookup = extract_provenance_lookup(graph);
    lower_to_anf_impl(core, Some(&lookup))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
