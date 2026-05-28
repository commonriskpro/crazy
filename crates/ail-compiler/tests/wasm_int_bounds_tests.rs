use ail_compiler::core_ir::{LiteralValue, StageHashes};
use ail_compiler::{AnfBinding, AnfExpr, AnfIr, SourceMap, emit_wasm};
use ail_core::semantic_graph::NodeRef;
use wasmparser::{Operator, Parser, Payload};

fn sealed_anf(binding: AnfBinding) -> AnfIr {
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

fn let_int(name: &str, value: i64, body: AnfExpr) -> AnfExpr {
    AnfExpr::Let {
        name: name.to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(value))),
        body: Box::new(body),
    }
}

fn emit_call_wasm(func: &str, arg_names: &[&str], arg_values: &[i64]) -> Vec<u8> {
    let expr = arg_names.iter().zip(arg_values.iter()).rev().fold(
        AnfExpr::Call {
            func: func.to_string(),
            args: arg_names.iter().map(|name| name.to_string()).collect(),
        },
        |body, (name, value)| let_int(name, *value, body),
    );
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr,
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("emitted WASM must validate");
    artifact.wasm
}

fn operator_names(wasm: &[u8]) -> Vec<&'static str> {
    let mut names = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("payload must parse") {
            let mut reader = body.get_operators_reader().expect("operators reader");
            while !reader.eof() {
                match reader.read().expect("operator must read") {
                    Operator::I64LeS => names.push("i64.le_s"),
                    Operator::I64GeS => names.push("i64.ge_s"),
                    Operator::I64LtS => names.push("i64.lt_s"),
                    Operator::I64GtS => names.push("i64.gt_s"),
                    Operator::If { .. } => names.push("if"),
                    _ => {}
                }
            }
        }
    }
    names
}

#[test]
fn wasm_emits_int_min_as_signed_bound_branch() {
    let wasm = emit_call_wasm("int.min", &["left", "right"], &[10, -2]);
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.le_s"),
        "int.min must compare signed bounds: {ops:?}"
    );
    assert!(
        ops.contains(&"if"),
        "int.min must select a result with a branch: {ops:?}"
    );
}

#[test]
fn wasm_emits_int_max_as_signed_bound_branch() {
    let wasm = emit_call_wasm("int.max", &["left", "right"], &[10, -2]);
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.ge_s"),
        "int.max must compare signed bounds: {ops:?}"
    );
    assert!(
        ops.contains(&"if"),
        "int.max must select a result with a branch: {ops:?}"
    );
}

#[test]
fn wasm_emits_int_clamp_as_two_signed_bound_branches() {
    let wasm = emit_call_wasm("int.clamp", &["value", "low", "high"], &[42, 0, 10]);
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.lt_s"),
        "int.clamp must check low bound: {ops:?}"
    );
    assert!(
        ops.contains(&"i64.gt_s"),
        "int.clamp must check high bound: {ops:?}"
    );
    assert!(
        ops.iter().filter(|name| **name == "if").count() >= 2,
        "int.clamp must select from low/high/value with nested branches: {ops:?}"
    );
}
