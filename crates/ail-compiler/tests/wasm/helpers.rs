pub(super) use ail_compiler::core_ir::{CoreExpr, LiteralValue, MatchArm, StageHashes};
pub(super) use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, CompileError, SourceMap, emit_wasm,
    lower::{lower_core_expr_to_anf, lower_to_anf, lower_to_core_ir},
};
pub(super) use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
pub(super) use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

pub(super) fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

pub(super) fn graph_with_n_nodes(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    }
}

pub(super) fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

pub(super) fn anf_for_graph(graph: &SemanticGraph) -> ail_compiler::AnfIr {
    let core = lower_to_core_ir(graph, &proven_report()).expect("lower_to_core_ir failed");
    lower_to_anf(&core).expect("lower_to_anf failed")
}

pub(super) fn sealed_anf(binding: AnfBinding) -> AnfIr {
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

pub(super) fn operators(wasm: &[u8]) -> Vec<String> {
    use wasmparser::{Parser, Payload};

    let mut names = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("payload must parse") {
            let mut reader = body
                .get_operators_reader()
                .expect("operators reader must build");
            while !reader.eof() {
                names.push(format!("{:?}", reader.read().expect("operator must read")));
            }
        }
    }
    names
}

pub(super) fn emit_valid_wasm(expr: AnfExpr, name: &str) -> Vec<String> {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: name.to_string(),
        expr,
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");
    operators(&artifact.wasm)
}

pub(super) fn contains_match(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Match { .. } => true,
        AnfExpr::Let { value, body, .. } => contains_match(value) || contains_match(body),
        _ => false,
    }
}

pub(super) fn pipeline_ops(body_expr: &str, fn_name: &str) -> Vec<String> {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, fn_name);
    node.body_expr = Some(body_expr.to_string());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report())
        .unwrap_or_else(|e| panic!("core lowering failed for {body_expr:?}: {e:?}"));
    let anf = lower_to_anf(&core)
        .unwrap_or_else(|e| panic!("ANF lowering failed for {body_expr:?}: {e:?}"));
    let artifact =
        emit_wasm(&anf).unwrap_or_else(|e| panic!("emit_wasm failed for {body_expr:?}: {e:?}"));
    wasmparser::validate(&artifact.wasm)
        .unwrap_or_else(|e| panic!("wasm validation failed for {body_expr:?}: {e:?}"));
    operators(&artifact.wasm)
}
