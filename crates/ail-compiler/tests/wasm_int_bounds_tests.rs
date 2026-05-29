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
                    Operator::I64Eq => names.push("i64.eq"),
                    Operator::I64Add => names.push("i64.add"),
                    Operator::I64Sub => names.push("i64.sub"),
                    Operator::I64DivS => names.push("i64.div_s"),
                    Operator::I64RemS => names.push("i64.rem_s"),
                    Operator::I32And => names.push("i32.and"),
                    Operator::I32Or => names.push("i32.or"),
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

#[test]
fn wasm_emits_int_abs_or_as_overflow_safe_signed_branch() {
    let wasm = emit_call_wasm("int.abs_or", &["value", "fallback"], &[-7, 99]);
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.eq"),
        "int.abs_or must check the minimum Int overflow case: {ops:?}"
    );
    assert!(
        ops.contains(&"i64.lt_s"),
        "int.abs_or must check whether value is negative: {ops:?}"
    );
    assert!(
        ops.contains(&"i64.sub"),
        "int.abs_or must negate negative values with subtraction: {ops:?}"
    );
    assert!(
        ops.iter().filter(|name| **name == "if").count() >= 2,
        "int.abs_or must select fallback/value/negated value with nested branches: {ops:?}"
    );
}

#[test]
fn wasm_emits_int_neg_or_as_overflow_safe_signed_branch() {
    let wasm = emit_call_wasm("int.neg_or", &["value", "fallback"], &[-5, 17]);
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.eq"),
        "int.neg_or must check the minimum Int overflow case: {ops:?}"
    );
    assert!(
        ops.contains(&"i64.sub"),
        "int.neg_or must negate safe values with subtraction: {ops:?}"
    );
    assert!(
        ops.contains(&"if"),
        "int.neg_or must branch between fallback and negated value: {ops:?}"
    );
}

#[test]
fn wasm_emits_int_add_or_as_overflow_safe_signed_branch() {
    let wasm = emit_call_wasm("int.add_or", &["left", "right", "fallback"], &[40, 2, -1]);
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.add"),
        "int.add_or must still emit signed addition on the safe path: {ops:?}"
    );
    assert!(
        ops.contains(&"i32.and"),
        "int.add_or must combine sign-specific overflow guards: {ops:?}"
    );
    assert!(
        ops.contains(&"i32.or"),
        "int.add_or must combine positive and negative overflow guards: {ops:?}"
    );
    assert!(
        ops.contains(&"if"),
        "int.add_or must branch between fallback and sum: {ops:?}"
    );
}

#[test]
fn wasm_emits_int_div_or_as_trap_safe_signed_branch() {
    let wasm = emit_call_wasm(
        "int.div_or",
        &["value", "divisor", "fallback"],
        &[21, 3, -1],
    );
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.eq"),
        "int.div_or must check zero divisor and signed overflow guards: {ops:?}"
    );
    assert!(
        ops.contains(&"i32.and"),
        "int.div_or must combine the i64::MIN / -1 overflow guard: {ops:?}"
    );
    assert!(
        ops.contains(&"i64.div_s"),
        "int.div_or must still emit signed division on the safe path: {ops:?}"
    );
    assert!(
        ops.iter().filter(|name| **name == "if").count() >= 2,
        "int.div_or must branch around unsafe division paths: {ops:?}"
    );
}

#[test]
fn wasm_emits_int_rem_or_as_trap_safe_signed_branch() {
    let wasm = emit_call_wasm(
        "int.rem_or",
        &["value", "divisor", "fallback"],
        &[22, 5, -1],
    );
    let ops = operator_names(&wasm);

    assert!(
        ops.contains(&"i64.eq"),
        "int.rem_or must check zero divisor and signed overflow guards: {ops:?}"
    );
    assert!(
        ops.contains(&"i32.and"),
        "int.rem_or must combine the i64::MIN / -1 overflow guard: {ops:?}"
    );
    assert!(
        ops.contains(&"i64.rem_s"),
        "int.rem_or must still emit signed remainder on the safe path: {ops:?}"
    );
    assert!(
        ops.iter().filter(|name| **name == "if").count() >= 2,
        "int.rem_or must branch around unsafe remainder paths: {ops:?}"
    );
}
