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

        // ── G20: Expression body lowering ────────────────────────────────

        // Match: scrutinee must be atomic (atomize if non-Var).
        // Each arm body is lowered recursively.
        CoreExpr::Match { scrutinee, arms } => {
            let scrutinee_name = atomize(scrutinee, fresh, source_ref, out);
            let anf_arms = arms
                .iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern.clone(),
                    body: lower_core_expr_to_anf(&arm.body, fresh, source_ref, out),
                })
                .collect();
            AnfExpr::Match {
                scrutinee: scrutinee_name,
                arms: anf_arms,
            }
        }

        // Lambda: params are already names; lower body recursively.
        CoreExpr::Lambda { params, body } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Lambda {
                params: params.clone(),
                body: Box::new(anf_body),
            }
        }

        // RecordNew: lower each field value recursively.
        // Field values are not atomized — they may be arbitrary AnfExprs.
        CoreExpr::RecordNew { fields } => {
            let anf_fields = fields
                .iter()
                .map(|(name, val)| {
                    let anf_val = lower_core_expr_to_anf(val, fresh, source_ref, out);
                    (name.clone(), anf_val)
                })
                .collect();
            AnfExpr::RecordNew { fields: anf_fields }
        }

        // FieldUpdate: record expression must be atomic; value is lowered recursively.
        CoreExpr::FieldUpdate { record, field, value } => {
            let record_name = atomize(record, fresh, source_ref, out);
            let anf_value = lower_core_expr_to_anf(value, fresh, source_ref, out);
            AnfExpr::FieldUpdate {
                record: record_name,
                field: field.clone(),
                value: Box::new(anf_value),
            }
        }

        // TupleNew: lower each element recursively.
        CoreExpr::TupleNew(elems) => {
            let anf_elems = elems
                .iter()
                .map(|e| lower_core_expr_to_anf(e, fresh, source_ref, out))
                .collect();
            AnfExpr::TupleNew(anf_elems)
        }

        // VariantNew: lower payload recursively if present.
        CoreExpr::VariantNew { tag, payload } => {
            let anf_payload = payload.as_ref().map(|p| {
                Box::new(lower_core_expr_to_anf(p, fresh, source_ref, out))
            });
            AnfExpr::VariantNew {
                tag: tag.clone(),
                payload: anf_payload,
            }
        }

        // ListNew: lower each element recursively.
        CoreExpr::ListNew(elems) => {
            let anf_elems = elems
                .iter()
                .map(|e| lower_core_expr_to_anf(e, fresh, source_ref, out))
                .collect();
            AnfExpr::ListNew(anf_elems)
        }

        // CoreExpr::Placeholder → AnfExpr::Placeholder (no expression body).
        CoreExpr::Placeholder => AnfExpr::Placeholder,
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
        VerificationReport {
            entries: vec![VerificationEntry {
                claim: "claim".to_string(),
                state,
                scope: "s".to_string(),
                evidence: None,
            }],
            ..Default::default()
        }
    }

    fn proven_report() -> VerificationReport {
        VerificationReport {
            entries: vec![],
            ..Default::default()
        }
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

    // ── G20: Expression body lowering tests ──────────────────────────────

    // Helper: lower a single CoreExpr to AnfExpr (no prior bindings).
    fn lower_single(expr: &CoreExpr) -> (AnfExpr, Vec<AnfBinding>) {
        let mut fresh = 0u32;
        let mut out: Vec<AnfBinding> = Vec::new();
        let result = lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out);
        (result, out)
    }

    // S1: Match — scrutinee Var is preserved as atomic name.
    #[test]
    fn lower_match_var_scrutinee_is_preserved() {
        use crate::core_ir::MatchArm;
        let expr = CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("payment".to_string())),
            arms: vec![MatchArm {
                pattern: "Ok(r)".to_string(),
                body: CoreExpr::Var("r".to_string()),
            }],
        };
        let (result, out) = lower_single(&expr);
        // Scrutinee is already Var, so no extra bindings emitted.
        assert!(out.is_empty(), "Var scrutinee must not produce extra bindings");
        match result {
            crate::anf::AnfExpr::Match { scrutinee, arms } => {
                assert_eq!(scrutinee, "payment");
                assert_eq!(arms.len(), 1);
                assert_eq!(arms[0].pattern, "Ok(r)");
            }
            other => panic!("expected AnfExpr::Match, got {other:?}"),
        }
    }

    // S1b: Match — non-Var scrutinee is atomized (produces synthetic binding).
    #[test]
    fn lower_match_complex_scrutinee_is_atomized() {
        use crate::core_ir::MatchArm;
        let expr = CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
            arms: vec![MatchArm {
                pattern: "_".to_string(),
                body: CoreExpr::Literal(LiteralValue::Unit),
            }],
        };
        let (result, out) = lower_single(&expr);
        // Literal scrutinee must be atomized → one synthetic binding.
        assert!(!out.is_empty(), "Literal scrutinee must produce a synthetic binding");
        match result {
            crate::anf::AnfExpr::Match { scrutinee, .. } => {
                // scrutinee must be the synthetic name, not "42"
                assert!(scrutinee.starts_with("anf_"), "scrutinee must be synthetic name, got {scrutinee}");
            }
            other => panic!("expected AnfExpr::Match, got {other:?}"),
        }
    }

    // S2: Lambda — params and body lowered correctly.
    #[test]
    fn lower_lambda_params_and_body() {
        let expr = CoreExpr::Lambda {
            params: vec!["x".to_string(), "y".to_string()],
            body: Box::new(CoreExpr::Var("x".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty(), "Lambda body Var must not produce extra bindings");
        match result {
            crate::anf::AnfExpr::Lambda { params, body } => {
                assert_eq!(params, vec!["x", "y"]);
                assert_eq!(*body, crate::anf::AnfExpr::Var("x".to_string()));
            }
            other => panic!("expected AnfExpr::Lambda, got {other:?}"),
        }
    }

    // S3: RecordNew — field values are lowered recursively.
    #[test]
    fn lower_record_new_field_values() {
        let expr = CoreExpr::RecordNew {
            fields: vec![
                ("amount".to_string(), CoreExpr::Literal(LiteralValue::Int(10))),
                ("label".to_string(), CoreExpr::Var("lbl".to_string())),
            ],
        };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty(), "simple RecordNew must not produce synthetic bindings");
        match result {
            crate::anf::AnfExpr::RecordNew { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "amount");
                assert_eq!(fields[0].1, crate::anf::AnfExpr::Literal(LiteralValue::Int(10)));
                assert_eq!(fields[1].0, "label");
                assert_eq!(fields[1].1, crate::anf::AnfExpr::Var("lbl".to_string()));
            }
            other => panic!("expected AnfExpr::RecordNew, got {other:?}"),
        }
    }

    // S4: FieldUpdate — record Var is preserved as atomic name.
    #[test]
    fn lower_field_update_var_record_is_preserved() {
        let expr = CoreExpr::FieldUpdate {
            record: Box::new(CoreExpr::Var("order".to_string())),
            field: "status".to_string(),
            value: Box::new(CoreExpr::Literal(LiteralValue::Text("Paid".to_string()))),
        };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty(), "Var record must not produce extra bindings");
        match result {
            crate::anf::AnfExpr::FieldUpdate { record, field, value } => {
                assert_eq!(record, "order");
                assert_eq!(field, "status");
                assert_eq!(*value, crate::anf::AnfExpr::Literal(LiteralValue::Text("Paid".to_string())));
            }
            other => panic!("expected AnfExpr::FieldUpdate, got {other:?}"),
        }
    }

    // S5: TupleNew — elements are lowered recursively.
    #[test]
    fn lower_tuple_new_elements() {
        let expr = CoreExpr::TupleNew(vec![
            CoreExpr::Var("a".to_string()),
            CoreExpr::Var("b".to_string()),
        ]);
        let (result, _out) = lower_single(&expr);
        match result {
            crate::anf::AnfExpr::TupleNew(elems) => {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0], crate::anf::AnfExpr::Var("a".to_string()));
                assert_eq!(elems[1], crate::anf::AnfExpr::Var("b".to_string()));
            }
            other => panic!("expected AnfExpr::TupleNew, got {other:?}"),
        }
    }

    // S6: VariantNew with payload.
    #[test]
    fn lower_variant_new_with_payload() {
        let expr = CoreExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(CoreExpr::Var("x".to_string()))),
        };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty(), "Var payload must not produce extra bindings");
        match result {
            crate::anf::AnfExpr::VariantNew { tag, payload } => {
                assert_eq!(tag, "Ok");
                assert_eq!(*payload.unwrap(), crate::anf::AnfExpr::Var("x".to_string()));
            }
            other => panic!("expected AnfExpr::VariantNew, got {other:?}"),
        }
    }

    // S6b: VariantNew without payload.
    #[test]
    fn lower_variant_new_no_payload() {
        let expr = CoreExpr::VariantNew {
            tag: "None".to_string(),
            payload: None,
        };
        let (result, _out) = lower_single(&expr);
        match result {
            crate::anf::AnfExpr::VariantNew { tag, payload } => {
                assert_eq!(tag, "None");
                assert!(payload.is_none());
            }
            other => panic!("expected AnfExpr::VariantNew, got {other:?}"),
        }
    }

    // S7: ListNew — elements are lowered recursively.
    #[test]
    fn lower_list_new_elements() {
        let expr = CoreExpr::ListNew(vec![
            CoreExpr::Literal(LiteralValue::Int(1)),
            CoreExpr::Var("x".to_string()),
        ]);
        let (result, _out) = lower_single(&expr);
        match result {
            crate::anf::AnfExpr::ListNew(elems) => {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0], crate::anf::AnfExpr::Literal(LiteralValue::Int(1)));
                assert_eq!(elems[1], crate::anf::AnfExpr::Var("x".to_string()));
            }
            other => panic!("expected AnfExpr::ListNew, got {other:?}"),
        }
    }

    // S8: No Placeholder produced for real CoreExpr variants.
    #[test]
    fn real_core_exprs_do_not_produce_placeholder() {
        use crate::core_ir::MatchArm;
        let real_exprs = vec![
            CoreExpr::Match {
                scrutinee: Box::new(CoreExpr::Var("x".to_string())),
                arms: vec![MatchArm {
                    pattern: "_".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Unit),
                }],
            },
            CoreExpr::Lambda {
                params: vec![],
                body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::RecordNew { fields: vec![] },
            CoreExpr::FieldUpdate {
                record: Box::new(CoreExpr::Var("r".to_string())),
                field: "f".to_string(),
                value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::TupleNew(vec![]),
            CoreExpr::VariantNew { tag: "A".to_string(), payload: None },
            CoreExpr::ListNew(vec![]),
        ];
        for expr in &real_exprs {
            let (result, _out) = lower_single(expr);
            assert_ne!(
                result,
                crate::anf::AnfExpr::Placeholder,
                "CoreExpr::{expr:?} must NOT produce Placeholder"
            );
        }
    }

    // S9: CoreExpr::Placeholder still produces AnfExpr::Placeholder.
    #[test]
    fn placeholder_still_maps_to_placeholder() {
        let (result, _out) = lower_single(&CoreExpr::Placeholder);
        assert_eq!(result, crate::anf::AnfExpr::Placeholder);
    }
}
