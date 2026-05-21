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

use crate::anf::{AnfBinding, AnfExpr, AnfIr};
use crate::core_ir::{
    CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, StageHashes,
};
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
pub(crate) fn map_node_kind(kind: NodeKind) -> CoreNodeKind {
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

// ── atomize ───────────────────────────────────────────────────────────────

/// Ensure `expr` is atomic (a variable name).
///
/// If `expr` is already `CoreExpr::Var(n)`, returns `n` without emitting any
/// binding.  Otherwise lowers `expr` to an `AnfExpr`, pushes a synthetic
/// `AnfBinding` with a fresh name, and returns that fresh name.
///
/// The pushed binding carries the same `source_ref` as the enclosing node
/// (provenance is preserved for synthetic temporaries).
fn atomize(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
    out: &mut Vec<AnfBinding>,
) -> String {
    if let CoreExpr::Var(name) = expr {
        return name.clone();
    }
    let anf_expr = lower_core_expr_to_anf(expr, fresh, source_ref, out);
    let name = format!("anf_{}", *fresh);
    *fresh += 1;
    out.push(AnfBinding {
        source_ref,
        name: name.clone(),
        expr: anf_expr,
    });
    name
}

// ── lower_core_expr_to_anf ────────────────────────────────────────────────

/// Recursively lower a `CoreExpr` to an `AnfExpr`.
///
/// Non-atomic sub-expressions (nested calls, non-trivial conditions, etc.)
/// are atomized: a synthetic `AnfBinding` is pushed to `out` and the
/// sub-expression is replaced by a `Var` reference to that binding.
///
/// All synthetic bindings carry `source_ref` for end-to-end provenance.
pub fn lower_core_expr_to_anf(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
    out: &mut Vec<AnfBinding>,
) -> AnfExpr {
    match expr {
        // Atomic values — no sub-expressions to flatten.
        CoreExpr::Literal(v) => AnfExpr::Literal(v.clone()),
        CoreExpr::Var(n) => AnfExpr::Var(n.clone()),

        // Let: lower value and body recursively; no atomization needed.
        CoreExpr::Let { name, value, body } => {
            let anf_value = lower_core_expr_to_anf(value, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Let {
                name: name.clone(),
                value: Box::new(anf_value),
                body: Box::new(anf_body),
            }
        }

        // If: condition must be atomic (atomize if needed).
        CoreExpr::If { cond, then_, else_ } => {
            let cond_name = atomize(cond, fresh, source_ref, out);
            let anf_then = lower_core_expr_to_anf(then_, fresh, source_ref, out);
            let anf_else = lower_core_expr_to_anf(else_, fresh, source_ref, out);
            AnfExpr::If {
                cond: cond_name,
                then_branch: Box::new(anf_then),
                else_branch: Box::new(anf_else),
            }
        }

        // Call: all args must be atomic (atomize each non-Var arg).
        CoreExpr::Call { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: func.clone(),
                args: atomic_args,
            }
        }

        // FieldGet: record expression must be atomic.
        CoreExpr::FieldGet { record, field } => {
            let record_name = atomize(record, fresh, source_ref, out);
            AnfExpr::FieldGet {
                record: record_name,
                field: field.clone(),
            }
        }

        // Complex CoreExpr variants not yet lowered to ANF → Placeholder.
        // Future phases will handle Match, Lambda, RecordNew, FieldUpdate,
        // TupleNew, VariantNew, ListNew.
        CoreExpr::Match { .. }
        | CoreExpr::Lambda { .. }
        | CoreExpr::RecordNew { .. }
        | CoreExpr::FieldUpdate { .. }
        | CoreExpr::TupleNew(_)
        | CoreExpr::VariantNew { .. }
        | CoreExpr::ListNew(_)
        | CoreExpr::Placeholder => AnfExpr::Placeholder,
    }
}

// ── map_core_node_to_anf ──────────────────────────────────────────────────

/// Lower one `CoreNode` into one or more `AnfBinding`s.
///
/// If the node has a `CoreExpr` body, it is lowered via
/// `lower_core_expr_to_anf`.  Any synthetic temporaries produced during
/// flattening are pushed to `out` first (in emission order), then the node's
/// own binding is appended.
///
/// Nodes without `expr` (modules, types, capabilities, etc.) get a default
/// `AnfExpr::Literal(LiteralValue::Unit)`.
///
/// Provenance (`source_ref`) is preserved verbatim on every emitted binding,
/// including synthetic temporaries.
fn map_core_node_to_anf(node: &CoreNode, fresh: &mut u32, out: &mut Vec<AnfBinding>) {
    let anf_expr = match &node.expr {
        Some(core_expr) => lower_core_expr_to_anf(core_expr, fresh, node.source_ref, out),
        None => AnfExpr::Literal(LiteralValue::Unit),
    };
    out.push(AnfBinding {
        source_ref: node.source_ref,
        name: node.name.clone(),
        expr: anf_expr,
    });
}

// ── nominal_to_core_type ─────────────────────────────────────────────────

/// Map a `TypeFacts.nominal` string to a `CoreType` variant.
///
/// Recognised nominals correspond to the 20 type primitives listed in
/// `docs/core-ir.md §3`.  Any unrecognised nominal falls back to
/// `CoreType::Generic`.
pub fn nominal_to_core_type(nominal: &str) -> CoreType {
    match nominal {
        "Unit" => CoreType::Unit,
        "Never" => CoreType::Never,
        "Bool" => CoreType::Bool,
        "Int" => CoreType::Int,
        "UInt" => CoreType::UInt,
        "Float" => CoreType::Float,
        "Text" => CoreType::Text,
        "Bytes" => CoreType::Bytes,
        "Record" => CoreType::Record,
        "Variant" => CoreType::Variant,
        "Tuple" => CoreType::Tuple,
        "List" => CoreType::List,
        "Map" => CoreType::Map,
        "Set" => CoreType::Set,
        "Option" => CoreType::Option,
        "Result" => CoreType::Result,
        "Function" => CoreType::Function,
        "Handle" => CoreType::Handle,
        "Refinement" => CoreType::Refinement,
        "Generic" => CoreType::Generic,
        _ => CoreType::Generic,
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
    // G2: populate `ty` from `GraphNode.type_facts.nominal` when present.
    // `expr` is left `None` at this stage; expression bodies are deferred to
    // the expression-lowering phase.
    let nodes: Vec<CoreNode> = graph
        .nodes
        .iter()
        .map(|gn| CoreNode {
            source_ref: gn.id,
            kind: map_node_kind(gn.kind),
            name: gn.name.clone(),
            ty: gn
                .type_facts
                .as_ref()
                .map(|tf| nominal_to_core_type(&tf.nominal)),
            expr: None,
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
    // Lower each CoreNode — collecting synthetic temporaries and node bindings.
    let mut bindings: Vec<AnfBinding> = Vec::with_capacity(core.nodes.len());
    let mut fresh: u32 = 0;
    for node in &core.nodes {
        map_core_node_to_anf(node, &mut fresh, &mut bindings);
    }

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
        VerificationReport::new(vec![VerificationEntry {
            claim: "claim".to_string(),
            state,
            scope: "s".to_string(),
            evidence: None,
        }])
    }

    fn proven_report() -> VerificationReport {
        VerificationReport::new(vec![])
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
            ty: None,
            expr: None,
        };
        let mut fresh = 0u32;
        let mut out = Vec::new();
        map_core_node_to_anf(&node, &mut fresh, &mut out);
        let binding = out.into_iter().next().expect("must produce one binding");
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

    // ── nominal_to_core_type ─────────────────────────────────────────────

    // S6 (partial): all 20 known nominals map to their CoreType variant.
    #[test]
    fn all_known_nominals_map_to_correct_core_type() {
        let cases: &[(&str, CoreType)] = &[
            ("Unit", CoreType::Unit),
            ("Never", CoreType::Never),
            ("Bool", CoreType::Bool),
            ("Int", CoreType::Int),
            ("UInt", CoreType::UInt),
            ("Float", CoreType::Float),
            ("Text", CoreType::Text),
            ("Bytes", CoreType::Bytes),
            ("Record", CoreType::Record),
            ("Variant", CoreType::Variant),
            ("Tuple", CoreType::Tuple),
            ("List", CoreType::List),
            ("Map", CoreType::Map),
            ("Set", CoreType::Set),
            ("Option", CoreType::Option),
            ("Result", CoreType::Result),
            ("Function", CoreType::Function),
            ("Handle", CoreType::Handle),
            ("Refinement", CoreType::Refinement),
            ("Generic", CoreType::Generic),
        ];
        for (nominal, expected) in cases {
            assert_eq!(
                nominal_to_core_type(nominal),
                *expected,
                "nominal {nominal:?} must map to {expected:?}"
            );
        }
    }

    // S7: unknown nominal falls back to CoreType::Generic.
    #[test]
    fn unknown_nominal_maps_to_generic() {
        assert_eq!(nominal_to_core_type("Exotic"), CoreType::Generic);
        assert_eq!(nominal_to_core_type(""), CoreType::Generic);
        assert_eq!(nominal_to_core_type("int"), CoreType::Generic); // case-sensitive
    }

    // ── G2 lower_to_core_ir with type_facts ──────────────────────────────

    // S6: lower_to_core_ir populates CoreType::Int for a node with
    // type_facts.nominal = "Int".
    #[test]
    fn lower_to_core_ir_populates_core_type_from_type_facts() {
        use ail_core::semantic_graph::TypeFacts;

        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "amount");
        node.type_facts = Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert_eq!(
            core.nodes[0].ty,
            Some(CoreType::Int),
            "node with TypeFacts.nominal=Int must get ty=Some(CoreType::Int)"
        );
    }

    // S8: lower_to_core_ir leaves ty = None for nodes without type_facts.
    #[test]
    fn lower_to_core_ir_leaves_ty_none_without_type_facts() {
        let graph = one_node_graph(); // GraphNode::new — type_facts is None
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert_eq!(
            core.nodes[0].ty, None,
            "node without TypeFacts must have ty=None"
        );
    }

    // S7 (lowering): lower_to_core_ir uses Generic for unknown nominals.
    #[test]
    fn lower_to_core_ir_uses_generic_for_unknown_nominal() {
        use ail_core::semantic_graph::TypeFacts;

        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "exotic");
        node.type_facts = Some(TypeFacts {
            nominal: "Exotic".to_string(),
            generics: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert_eq!(
            core.nodes[0].ty,
            Some(CoreType::Generic),
            "unknown nominal must produce CoreType::Generic"
        );
    }

    // expr is always None after lower_to_core_ir (deferred phase).
    #[test]
    fn lower_to_core_ir_expr_is_always_none() {
        let graph = one_node_graph();
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert!(
            core.nodes[0].expr.is_none(),
            "expr must be None after lower_to_core_ir (deferred to expression lowering)"
        );
    }
}
