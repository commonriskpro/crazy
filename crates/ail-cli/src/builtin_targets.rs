// ── ail-cli::builtin_targets ───────────────────────────────────────────────
//
// Pre-built ANF fixtures for named built-in runtime targets.

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, LiteralValue, SourceMap, StageHashes,
};
use ail_core::semantic_graph::NodeRef;

/// Return a pre-built `AnfIr` for a named built-in target, or `None` if the
/// target must be compiled from the project graph.
pub(crate) fn runtime_anf_for_target(target: &str) -> Option<AnfIr> {
    let (name, expr) = match target {
        "fn.add" | "add" => (
            "fn.add",
            AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["a".to_string(), "b".to_string()],
            },
        ),
        "fn.double" | "double" => (
            "fn.double",
            AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["x".to_string(), "x".to_string()],
            },
        ),
        "fn.answer" | "answer" => ("fn.answer", AnfExpr::Literal(LiteralValue::Int(42))),
        _ => return None,
    };
    Some(anf_for_binding(name, expr))
}

fn anf_for_binding(name: &str, expr: AnfExpr) -> AnfIr {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: name.to_string(),
        expr,
    }];
    let source_map = SourceMap::from_bindings(&bindings);
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings,
        source_map,
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0; 32],
            verification_report_hash: [0; 32],
            core_ir_hash: [0; 32],
            anf_ir_hash: Some([0; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}
