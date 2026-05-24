// ── anf_lowering_helpers ─────────────────────────────────────────────────
//
// Shared test helpers for ANF lowering integration test binaries.
// Each helper is `pub` so that every test binary that declares
// `mod anf_lowering_helpers;` can use it freely.

#![allow(dead_code)]

use ail_compiler::lower::lower_core_expr_to_anf;
use ail_compiler::{
    AnfBinding, AnfExpr, CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, StageHashes,
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

pub fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

pub fn one_fn_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn_test")],
        edges: vec![],
    }
}

/// Build a `CoreIr` with a single node carrying the given `CoreExpr`.
pub fn core_ir_with_expr(source_ref: NodeRef, name: &str, expr: CoreExpr) -> CoreIr {
    CoreIr {
        nodes: vec![CoreNode {
            source_ref,
            kind: CoreNodeKind::Function,
            name: name.to_string(),
            ty: Some(CoreType::Function {
                params: vec![],
                ret: Box::new(CoreType::Generic(None)),
                effects: vec![],
            }),
            expr: Some(expr),
        }],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

/// Lower a single `CoreExpr` and return the resulting `AnfExpr`.
/// Discards any synthetic bindings emitted during flattening — only
/// suitable for expressions that produce no temporaries themselves.
pub fn lower_expr(expr: &CoreExpr) -> AnfExpr {
    let mut fresh = 0u32;
    let mut out = Vec::new();
    lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out)
}

/// Lower a `CoreExpr` and return `(synthetic_bindings, root_anf_expr)`.
pub fn lower_and_collect(expr: &CoreExpr) -> (Vec<AnfBinding>, AnfExpr) {
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let root = lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out);
    (out, root)
}
