use super::helpers::*;

#[test]
fn anf_if_emits_real_wasm_if_else() {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.branch".to_string(),
        expr: AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::If {
                cond: "flag".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm failed");

    wasmparser::validate(&artifact.wasm).expect("if wasm must validate");
    let ops = operators(&artifact.wasm);
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "expected If in {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op == "Else"),
        "expected Else in {ops:?}"
    );
    assert!(
        !ops.iter().any(|op| op == "Unreachable"),
        "if must not emit unreachable: {ops:?}"
    );
}

#[test]
fn effect_call_emits_host_call_import_and_call() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(41))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "test.counter".to_string(),
                func: "inc".to_string(),
                args: vec!["n".to_string()],
            }),
        },
    };
    let anf = sealed_anf(binding);
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("effect wasm must validate");

    let mut saw_import = false;
    let mut saw_host_call = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.expect("payload must parse") {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.expect("import must parse");
                    let rendered = format!("{import:?}");
                    if rendered.contains("ail") && rendered.contains("host_call") {
                        saw_import = true;
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().expect("operators");
                while !reader.eof() {
                    if matches!(
                        reader.read().expect("operator"),
                        Operator::Call { function_index: 0 }
                    ) {
                        saw_host_call = true;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(saw_import, "expected ail/host_call import");
    assert!(
        saw_host_call,
        "expected effect call to call imported function 0"
    );
}

#[test]
fn core_if_lowers_to_anf_if_and_emits_valid_wasm() {
    let core = CoreExpr::If {
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(false))),
        then_: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        else_: Box::new(CoreExpr::Literal(LiteralValue::Int(2))),
    };
    let mut fresh = 0;
    let mut synthetic = Vec::new();
    let expr = lower_core_expr_to_anf(&core, &mut fresh, NodeRef(0), &mut synthetic);
    assert!(matches!(expr, AnfExpr::If { .. }));

    let mut bindings = synthetic;
    bindings.push(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.core_branch".to_string(),
        expr,
    });
    let anf = AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
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
    };

    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("core if wasm must validate");
}

#[test]
fn loop_break_parses_and_emits_loop_block() {
    // loop(break(42)) — must emit a Block + Loop + Br for break
    let ops = pipeline_ops("loop(break(42))", "fn.loop_break");
    assert!(
        ops.iter().any(|op| op.starts_with("Loop")),
        "loop() must emit a Loop block, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("Br")),
        "break() must emit Br for loop exit, got {ops:?}"
    );
}

#[test]
fn while_loop_parses_and_emits_loop_block() {
    // while(flag, break(0)) — must emit a Loop block with conditional exit
    let ops = pipeline_ops("while(flag, break(0))", "fn.while_loop");
    assert!(
        ops.iter().any(|op| op.starts_with("Loop")),
        "while() must emit a Loop block, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("Block")),
        "while() must emit a Block for break exit, got {ops:?}"
    );
}

#[test]
fn return_parses_and_emits_return_instruction() {
    // return(99) — must emit Return instruction
    let ops = pipeline_ops("return(99)", "fn.return_test");
    assert!(
        ops.iter().any(|op| op == "Return"),
        "return() must emit Return instruction, got {ops:?}"
    );
}
