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

use ail_core::semantic_graph::{GraphValidationError, NodeKind, SemanticGraph};
use ail_verify::report::{VerificationReport, VerificationState};

use crate::anf::{AnfBinding, AnfIr};
use crate::core_ir::{CoreIr, CoreNode, CoreNodeKind, StageHashes};
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};

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

// ── map_node_kind ─────────────────────────────────────────────────────────

/// Map a `NodeKind` (source graph) to its `CoreNodeKind` counterpart.
///
/// In Phase 7 the two enums are structurally identical; the mapping is kept
/// explicit so the compiler IR can diverge from the source model in future
/// phases without a breaking change here.
fn map_node_kind(kind: NodeKind) -> CoreNodeKind {
    match kind {
        NodeKind::Module => CoreNodeKind::Module,
        NodeKind::Function => CoreNodeKind::Function,
        NodeKind::Type => CoreNodeKind::Type,
        NodeKind::Effect => CoreNodeKind::Effect,
        NodeKind::Capability => CoreNodeKind::Capability,
        NodeKind::Contract => CoreNodeKind::Contract,
        NodeKind::Invariant => CoreNodeKind::Invariant,
        NodeKind::Test => CoreNodeKind::Test,
        NodeKind::Boundary => CoreNodeKind::Boundary,
        NodeKind::Package => CoreNodeKind::Package,
    }
}

// ── map_core_node_to_anf ──────────────────────────────────────────────────

/// Lower one `CoreNode` into a single `AnfBinding`.
///
/// Provenance (`source_ref`) is preserved verbatim.  The binding name is
/// copied from the `CoreNode` without modification.
fn map_core_node_to_anf(node: &CoreNode) -> AnfBinding {
    AnfBinding {
        source_ref: node.source_ref,
        name: node.name.clone(),
    }
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
    })?;

    // Hash the pipeline inputs (empty parent → blake3(content)).
    let graph_cbor = stable_cbor_bytes(graph)?;
    let graph_snapshot_hash = hash_with_parent(&[], &graph_cbor);

    let report_cbor = stable_cbor_bytes(report)?;
    let verification_report_hash = hash_with_parent(&[], &report_cbor);

    // Lower each source GraphNode to a CoreNode (1-to-1, in traversal order).
    let nodes: Vec<CoreNode> = graph
        .nodes
        .iter()
        .map(|gn| CoreNode {
            source_ref: gn.id,
            kind: map_node_kind(gn.kind),
            name: gn.name.clone(),
        })
        .collect();

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
        },
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
/// # Hash chain
///
/// Extends the chain: `anf_ir_hash = blake3(core_ir_hash || anf_ir_bytes)`.
///
/// # Errors
///
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_anf(core: &CoreIr) -> Result<AnfIr, CompileError> {
    // Lower each CoreNode to an AnfBinding in traversal order.
    let bindings: Vec<AnfBinding> = core.nodes.iter().map(map_core_node_to_anf).collect();

    // Seal: anf_ir_hash = blake3(core_ir_hash || anf_ir_bytes).
    let anf_ir_bytes = stable_cbor_bytes(&bindings)?;
    let anf_ir_hash = hash_with_parent(&core.stage_hashes.core_ir_hash, &anf_ir_bytes);

    // Extend the stage hashes from Core IR.
    let mut stage_hashes = core.stage_hashes.clone();
    stage_hashes.anf_ir_hash = Some(anf_ir_hash);

    Ok(AnfIr {
        bindings,
        stage_hashes,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────

    fn report_with_state(state: VerificationState) -> VerificationReport {
        VerificationReport {
            entries: vec![VerificationEntry {
                claim: "claim".to_string(),
                state,
                scope: "s".to_string(),
                evidence: None,
            }],
        }
    }

    fn proven_report() -> VerificationReport {
        VerificationReport { entries: vec![] }
    }

    fn one_node_graph() -> SemanticGraph {
        SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "m")],
            edges: vec![],
        }
    }

    // ── is_report_accepted ────────────────────────────────────────────────

    // Proven and RuntimeChecked are accepted; all others are rejected.
    #[test]
    fn proven_is_accepted() {
        assert!(is_report_accepted(&proven_report()));
    }

    #[test]
    fn runtime_checked_is_accepted() {
        let report = report_with_state(VerificationState::RuntimeChecked);
        assert!(is_report_accepted(&report));
    }

    #[test]
    fn failed_is_rejected() {
        let report = report_with_state(VerificationState::Failed);
        assert!(!is_report_accepted(&report));
    }

    #[test]
    fn assumed_is_rejected() {
        let report = report_with_state(VerificationState::Assumed);
        assert!(!is_report_accepted(&report));
    }

    #[test]
    fn unverified_is_rejected() {
        let report = report_with_state(VerificationState::Unverified);
        assert!(!is_report_accepted(&report));
    }

    #[test]
    fn unsafe_is_rejected() {
        let report = report_with_state(VerificationState::Unsafe);
        assert!(!is_report_accepted(&report));
    }

    // ── map_node_kind ─────────────────────────────────────────────────────

    // All 10 source kinds map to their CoreNodeKind counterpart.
    #[test]
    fn all_node_kinds_map_correctly() {
        let cases = [
            (NodeKind::Module, CoreNodeKind::Module),
            (NodeKind::Function, CoreNodeKind::Function),
            (NodeKind::Type, CoreNodeKind::Type),
            (NodeKind::Effect, CoreNodeKind::Effect),
            (NodeKind::Capability, CoreNodeKind::Capability),
            (NodeKind::Contract, CoreNodeKind::Contract),
            (NodeKind::Invariant, CoreNodeKind::Invariant),
            (NodeKind::Test, CoreNodeKind::Test),
            (NodeKind::Boundary, CoreNodeKind::Boundary),
            (NodeKind::Package, CoreNodeKind::Package),
        ];
        for (src, expected) in cases {
            assert_eq!(
                map_node_kind(src),
                expected,
                "NodeKind::{src:?} must map to CoreNodeKind::{expected:?}"
            );
        }
    }

    // ── map_core_node_to_anf ──────────────────────────────────────────────

    // Provenance and name are preserved verbatim.
    #[test]
    fn map_core_node_to_anf_preserves_source_ref_and_name() {
        let node = CoreNode {
            source_ref: NodeRef(7),
            kind: CoreNodeKind::Function,
            name: "fn_x".to_string(),
        };
        let binding = map_core_node_to_anf(&node);
        assert_eq!(binding.source_ref, NodeRef(7));
        assert_eq!(binding.name, "fn_x");
    }

    // ── lower_to_core_ir ──────────────────────────────────────────────────

    #[test]
    fn rejected_report_returns_rejected_error() {
        let graph = one_node_graph();
        let report = report_with_state(VerificationState::Failed);
        assert_eq!(
            lower_to_core_ir(&graph, &report),
            Err(CompileError::RejectedReport)
        );
    }

    #[test]
    fn accepted_report_returns_core_ir() {
        let graph = one_node_graph();
        let report = proven_report();
        let result = lower_to_core_ir(&graph, &report);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap().nodes.len(), 1);
    }

    // ── lower_to_anf ─────────────────────────────────────────────────────

    #[test]
    fn lower_to_anf_produces_one_binding_per_core_node() {
        let graph = one_node_graph();
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        let anf = lower_to_anf(&core).unwrap();
        assert_eq!(anf.bindings.len(), 1);
        assert_eq!(anf.bindings[0].source_ref, NodeRef(0));
    }

    #[test]
    fn anf_ir_hash_is_set_after_lowering() {
        let graph = one_node_graph();
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        let anf = lower_to_anf(&core).unwrap();
        assert!(anf.stage_hashes.anf_ir_hash.is_some());
    }
}
