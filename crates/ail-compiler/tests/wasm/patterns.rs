use super::helpers::*;

#[test]
fn anf_match_on_i64_literal_emits_real_branching() {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.match_value".to_string(),
        expr: AnfExpr::Let {
            name: "tag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "tag".to_string(),
                arms: vec![
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "1".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(10)),
                    },
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "2".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(20)),
                    },
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "_".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(30)),
                    },
                ],
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm failed");

    wasmparser::validate(&artifact.wasm).expect("match wasm must validate");
    let ops = operators(&artifact.wasm);
    assert!(
        ops.iter().any(|op| op == "I64Eq"),
        "expected i64 equality in {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "expected If cascade in {ops:?}"
    );
    assert!(
        !ops.iter().any(|op| op == "Unreachable"),
        "match must not emit unreachable: {ops:?}"
    );
}

// Previously these tested that constructor patterns traps (they were unimplemented).
// Now that constructor pattern matching is implemented, they verify the CORRECT behavior.

#[test]
fn constructor_match_ok_with_payload_binding_runs_arm_body() {
    // match(Ok(7)) { Ok(value) => 99 }
    // Should match the Ok arm and return 99 (not trap).
    let ops = emit_valid_wasm(
        AnfExpr::Let {
            name: "result".to_string(),
            value: Box::new(AnfExpr::VariantNew {
                tag: "Ok".to_string(),
                payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(7)))),
            }),
            body: Box::new(AnfExpr::Match {
                scrutinee: "result".to_string(),
                arms: vec![ail_compiler::anf::AnfMatchArm {
                    pattern: "Ok(value)".to_string(),
                    body: AnfExpr::Literal(LiteralValue::Int(99)),
                }],
            }),
        },
        "fn.constructor_match_ok",
    );

    // Must emit a tag load (I32Load) and comparison — not an unconditional trap.
    assert!(
        ops.iter().any(|op| op.starts_with("I32Load")),
        "constructor match must emit I32Load for tag check, got {ops:?}"
    );
    // The arm body (99) must be reachable.
    assert!(
        ops.iter().any(|op| op == "I64Const { value: 99 }"),
        "constructor arm body must be emitted, got {ops:?}"
    );
}

#[test]
fn constructor_match_ok_with_wildcard_fallback_works() {
    // match(Ok(7)) { Ok(value) => 99, _ => 0 }
    // Should match the Ok arm (not fall through to wildcard).
    let ops = emit_valid_wasm(
        AnfExpr::Let {
            name: "result".to_string(),
            value: Box::new(AnfExpr::VariantNew {
                tag: "Ok".to_string(),
                payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(7)))),
            }),
            body: Box::new(AnfExpr::Match {
                scrutinee: "result".to_string(),
                arms: vec![
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "Ok(value)".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(99)),
                    },
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "_".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(0)),
                    },
                ],
            }),
        },
        "fn.constructor_match_ok_with_wildcard",
    );

    // Must emit a tag load and a real if-else (not an unconditional trap before the wildcard).
    assert!(
        ops.iter().any(|op| op.starts_with("I32Load")),
        "constructor match must emit I32Load for tag check, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op == "I64Const { value: 99 }"),
        "Ok arm body must be emitted, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op == "I64Const { value: 0 }"),
        "wildcard fallback body must be emitted, got {ops:?}"
    );
}

#[test]
fn multi_binding_constructor_pattern_traps() {
    // Wave 16B: multi-binding patterns like `"Ok(a, b)"` are unsupported at compile time.
    // emit_wasm must return Err(UnsupportedPatternSyntax) — NOT a runtime Unreachable.
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.multi_binding_trap".to_string(),
        expr: AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::VariantNew {
                tag: "Ok".to_string(),
                payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(1)))),
            }),
            body: Box::new(AnfExpr::Match {
                scrutinee: "v".to_string(),
                arms: vec![ail_compiler::anf::AnfMatchArm {
                    pattern: "Ok(a, b)".to_string(),
                    body: AnfExpr::Literal(LiteralValue::Int(1)),
                }],
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(binding));
    assert!(
        matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "multi-binding constructor pattern must be rejected at compile time with UnsupportedPatternSyntax, got {result:?}"
    );
}

#[test]
fn nested_constructor_pattern_against_i64_scrutinee_rejected() {
    // Wave 16B: nested constructor patterns like `"Ok(Some(x))"` are unsupported regardless of
    // the scrutinee type — emit_wasm must return Err(UnsupportedPatternSyntax).
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.nested_constructor_i64".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "n".to_string(),
                arms: vec![ail_compiler::anf::AnfMatchArm {
                    pattern: "Ok(Some(x))".to_string(),
                    body: AnfExpr::Literal(LiteralValue::Int(0)),
                }],
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(binding));
    assert!(
        matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "nested constructor pattern must be rejected at compile time with UnsupportedPatternSyntax, got {result:?}"
    );
}

#[test]
fn core_match_lowers_to_anf_match_and_emits_valid_wasm() {
    let core = CoreExpr::Match {
        scrutinee: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        arms: vec![
            MatchArm {
                pattern: "1".to_string(),
                body: CoreExpr::Literal(LiteralValue::Int(11)),
            },
            MatchArm {
                pattern: "_".to_string(),
                body: CoreExpr::Literal(LiteralValue::Int(22)),
            },
        ],
    };
    let mut fresh = 0;
    let mut synthetic = Vec::new();
    let expr = lower_core_expr_to_anf(&core, &mut fresh, NodeRef(0), &mut synthetic);
    assert!(matches!(expr, AnfExpr::Match { .. }));

    let mut bindings = synthetic;
    bindings.push(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.core_match".to_string(),
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
    wasmparser::validate(&artifact.wasm).expect("core match wasm must validate");
}

#[test]
fn parsed_match_body_lowers_to_anf_and_emits_valid_wasm() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.match_surface");
    node.body_expr = Some("match(2, 1, 10, 2, 20, _, 30)".to_string());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let core = lower_to_core_ir(&graph, &proven_report()).expect("core lowering must parse match");
    assert!(
        matches!(core.nodes[0].expr, Some(CoreExpr::Match { .. })),
        "body_expr must parse to CoreExpr::Match"
    );

    let anf = lower_to_anf(&core).expect("ANF lowering must handle parsed match");
    assert!(
        contains_match(&anf.bindings[0].expr),
        "parsed match must survive into ANF"
    );

    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("parsed match wasm must validate");
    let ops = operators(&artifact.wasm);
    assert!(
        ops.iter().any(|op| op == "I64Eq"),
        "parsed match must emit equality checks, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "parsed match must emit branch cascade, got {ops:?}"
    );
}

// ── New operator and constructor pattern pipeline tests ────────────────────

// ── Control flow and effect pipeline tests ────────────────────────────────

/// Run a body_expr string through the full pipeline and return WASM operators.

#[test]
fn ne_operator_parses_and_emits_valid_wasm() {
    // ne(x, 0) parsed from body_expr should lower to ANF call + I64Ne
    let ops = pipeline_ops("ne(x, 0)", "fn.ne_test");
    assert!(
        ops.iter().any(|op| op == "I64Ne"),
        "ne() must emit I64Ne, got {ops:?}"
    );
}

#[test]
fn le_operator_parses_and_emits_valid_wasm() {
    let ops = pipeline_ops("le(score, 100)", "fn.le_test");
    assert!(
        ops.iter().any(|op| op == "I64LeS"),
        "le() must emit I64LeS, got {ops:?}"
    );
}

#[test]
fn ge_operator_parses_and_emits_valid_wasm() {
    let ops = pipeline_ops("ge(score, 0)", "fn.ge_test");
    assert!(
        ops.iter().any(|op| op == "I64GeS"),
        "ge() must emit I64GeS, got {ops:?}"
    );
}

#[test]
fn not_operator_parses_and_emits_valid_wasm() {
    // not(flag) should emit I64Eqz (logical negation)
    let ops = pipeline_ops("not(flag)", "fn.not_test");
    assert!(
        ops.iter().any(|op| op == "I64Eqz"),
        "not() must emit I64Eqz, got {ops:?}"
    );
}

#[test]
fn none_constructor_parses_and_emits_variant_with_tag_zero() {
    // none() → VariantNew { tag: "None", payload: None }
    // None has well-known tag 0 → I32Const { value: 0 } in tag slot
    let ops = pipeline_ops("none()", "fn.none_test");
    // Must allocate memory and store tag=0
    assert!(
        ops.iter().any(|op| op == "I32Const { value: 0 }"),
        "none() must store tag discriminant 0, got {ops:?}"
    );
}

#[test]
fn effect_call_parses_and_emits_host_call() {
    // Use a no-arg effect call so there are no unbound variable references.
    // effect_call(clock, now) — must emit host_call import + Call instruction.
    let ops = pipeline_ops("effect_call(clock, now)", "fn.effect_call_test");
    // Effect calls emit Call instruction for the host_call import
    assert!(
        ops.iter().any(|op| op.starts_with("Call")),
        "effect_call() must emit a Call to host_call, got {ops:?}"
    );
}

#[test]
fn option_match_pipeline_emits_tag_load_and_branching() {
    // Full pipeline: parse match, lower, emit, validate
    // match(some(7), Some(v), v, None, 0)
    let ops = pipeline_ops(
        "let(opt, some(7), match(opt, Some(v), v, None, 0))",
        "fn.option_match",
    );
    // Must emit I32Load (tag read) and conditional branching
    assert!(
        ops.iter().any(|op| op.starts_with("I32Load")),
        "option match must emit I32Load for tag check, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "option match must emit If branching, got {ops:?}"
    );
}

#[test]
fn result_match_pipeline_emits_tag_load_and_payload_binding() {
    // match(ok(99), Ok(val), val, Err(e), -1)
    let ops = pipeline_ops(
        "let(res, ok(99), match(res, Ok(val), val, Err(e), -1))",
        "fn.result_match",
    );
    // Must emit I32Load (tag), I64Load (payload binding), and branching
    assert!(
        ops.iter().any(|op| op.starts_with("I32Load")),
        "result match must emit I32Load for tag check, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("I64Load")),
        "result match must emit I64Load to bind payload, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "result match must emit If branching, got {ops:?}"
    );
}

#[test]
fn lambda_parses_and_lowers_to_anf_successfully() {
    // lambda(x, add(x, 1)) — must parse and lower without error (WASM is a stub i32)
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.lambda");
    node.body_expr = Some("lambda(x, add(x, 1))".to_string());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("core lowering must succeed");
    assert!(
        matches!(core.nodes[0].expr, Some(CoreExpr::Lambda { .. })),
        "body_expr must parse to CoreExpr::Lambda"
    );
    let anf = lower_to_anf(&core).expect("ANF lowering must handle lambda");
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("lambda wasm must validate");
}
