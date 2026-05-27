pub(super) use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
pub(super) use ail_verify::report::VerificationReport;

pub(super) use super::super::emit_wasm;
pub(super) use crate::anf::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, AnfSelectClause, SourceMap,
};
pub(super) use crate::core_ir::{LiteralValue, StageHashes};
pub(super) use crate::error::CompileError;
pub(super) use crate::lower::{lower_to_anf, lower_to_core_ir};
pub(super) use crate::wasm_abi::{
    EffectDataLayout, WasmScalarType, WasmSignature, WasmTypeDescriptor, binding_params,
    collect_free_vars, derive_wasm_type, lambda_body_params,
};
pub(super) use crate::wasm_sections::build_type_section;

pub(super) fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

pub(super) fn anf_for_n(n: usize) -> AnfIr {
    let graph = SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).unwrap();
    lower_to_anf(&core).unwrap()
}

pub(super) fn sealed_anf(bindings: Vec<AnfBinding>) -> AnfIr {
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&bindings),
        bindings,
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

/// Build a minimal `AnfIr` with a single binding whose body is the given expr.
pub(super) fn anf_with_single_binding(name: &str, body: AnfExpr) -> AnfIr {
    sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(42),
        name: name.to_string(),
        expr: body,
    }])
}

pub(super) fn emit_two_variant_anf(tag_a: &str, tag_b: &str) -> AnfIr {
    // One function body with two sequential VariantNew lets.
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.variants".to_string(),
        expr: AnfExpr::Let {
            name: "v1".to_string(),
            value: Box::new(AnfExpr::VariantNew {
                tag: tag_a.to_string(),
                payload: None,
            }),
            body: Box::new(AnfExpr::Let {
                name: "v2".to_string(),
                value: Box::new(AnfExpr::VariantNew {
                    tag: tag_b.to_string(),
                    payload: None,
                }),
                body: Box::new(AnfExpr::Var("v1".to_string())),
            }),
        },
    };
    sealed_anf(vec![binding])
}

/// Extract all I32Const values seen in the code section of a WASM binary.
pub(super) fn i32_const_values_in_code(wasm: &[u8]) -> Vec<i32> {
    use wasmparser::{Operator, Parser, Payload};
    let mut values = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I32Const { value } = reader.read().unwrap() {
                    values.push(value);
                }
            }
        }
    }
    values
}
