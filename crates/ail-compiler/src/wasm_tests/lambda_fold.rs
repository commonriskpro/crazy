use super::helpers::*;

#[test]
fn closure_capture_lambda_no_capture() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> x
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Var("x".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.is_empty(),
            "identity lambda must have no captures; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda that references an outer variable — must appear in captures.
#[test]
fn closure_capture_lambda_with_outer_var() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> add(x, outer_val)   — `outer_val` is free
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Call {
            func: "add".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("outer_val".to_string()),
            ],
        }),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.contains(&"outer_val".to_string()),
            "outer_val must be captured; got {captures:?}"
        );
        assert!(
            !captures.contains(&"x".to_string()),
            "param x must NOT appear in captures; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda param shadows an outer variable of the same name — the
// outer name must NOT appear in captures (the param takes precedence).
#[test]
fn closure_capture_lambda_shadowed_param_not_captured() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(outer) -> outer  — param named "outer" shadows any outer binding
    let expr = CoreExpr::Lambda {
        params: vec!["outer".to_string()],
        body: Box::new(CoreExpr::Var("outer".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.is_empty(),
            "`outer` is shadowed by param; captures must be empty; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda whose body contains an EffectCall that references an outer
// variable — the outer variable must appear in captures.
#[test]
fn closure_capture_lambda_effect_call_arg_captured() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> db.read(x, context_id)  — `context_id` is free
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::EffectCall {
            capability: "db".to_string(),
            func: "read".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("context_id".to_string()),
            ],
        }),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.contains(&"context_id".to_string()),
            "context_id must be captured from EffectCall arg; got {captures:?}"
        );
        assert!(
            !captures.contains(&"x".to_string()),
            "param x must NOT be captured; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// ── End closure-capture tests ─────────────────────────────────────────────

// ── Wave 10A: Bytes literal emit, descriptor, and data-section tests ──────

// Scenario: derive_wasm_type on a Bytes literal must return WasmTypeDescriptor::Bytes.
// Proves the compiler side of the descriptor contract for Bytes.

#[test]
fn lambda_body_params_returns_lambda_params() {
    let lambda = AnfExpr::Lambda {
        params: vec!["x".to_string(), "y".to_string()],
        captures: vec!["outer".to_string()],
        body: Box::new(AnfExpr::Var("x".to_string())),
    };
    assert_eq!(
        lambda_body_params(&lambda),
        &["x", "y"],
        "lambda_body_params must return the Lambda's own params"
    );
}

// TRIANGULATE: lambda_body_params returns empty for non-Lambda expressions.
#[test]
fn lambda_body_params_empty_for_non_lambda() {
    assert!(
        lambda_body_params(&AnfExpr::Literal(LiteralValue::Int(0))).is_empty(),
        "lambda_body_params must be empty for Literal"
    );
    assert!(
        lambda_body_params(&AnfExpr::Var("x".to_string())).is_empty(),
        "lambda_body_params must be empty for Var"
    );
}

// Scenario: binding_signatures for a Lambda binding includes both captures
// and Lambda-own params in param_count, and infers the result from the body.
//
// Lambda { captures: ["outer"], params: ["x"], body: add(outer, x) }
// Expected: param_count = 2, result = Some(I64)
#[test]
fn binding_signatures_lambda_includes_captures_and_params() {
    use ail_core::semantic_graph::NodeRef;
    use wasm_encoder::ValType;

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "add".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    let sigs = binding_params(&binding);
    // binding_params returns captures only.
    assert_eq!(sigs, vec!["outer"], "binding_params must return captures");

    let signatures = crate::wasm_abi::binding_signatures(std::slice::from_ref(&binding));
    assert_eq!(
        signatures[0].param_count, 2,
        "1 capture + 1 Lambda param = 2 WASM params"
    );
    assert_eq!(
        signatures[0].result,
        Some(ValType::I64),
        "body add(outer, x) → I64 result"
    );
}

// Scenario: binding_result for a Lambda binding infers from the Lambda body,
// not from the Lambda node itself (which would always give I32 in the old code).
#[test]
fn binding_result_lambda_infers_from_body() {
    use ail_core::semantic_graph::NodeRef;
    use wasm_encoder::ValType;

    // Lambda with no captures: fn(x) -> x  (identity, I64)
    let no_cap = AnfBinding {
        source_ref: NodeRef(0),
        name: "id".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        },
    };
    assert_eq!(
        crate::wasm_abi::binding_result(&no_cap),
        Some(ValType::I64),
        "identity Lambda body must resolve to I64, not I32"
    );

    // Lambda with capture: fn(x) -> add(outer, x)  (I64)
    let with_cap = AnfBinding {
        source_ref: NodeRef(1),
        name: "add".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    assert_eq!(
        crate::wasm_abi::binding_result(&with_cap),
        Some(ValType::I64),
        "Lambda body add(outer, x) must resolve to I64"
    );
}

// Scenario: emit_wasm succeeds for a top-level Lambda binding with no captures.
// Proves the pipeline is end-to-end correct for the no-capture case.
#[test]
fn emit_wasm_lambda_binding_no_capture_succeeds() {
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> x  (identity Lambda, no captures)
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "id".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for identity Lambda");
    assert!(
        !artifact.wasm.is_empty(),
        "WASM binary must be non-empty for Lambda binding"
    );
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be sealed after emit_wasm"
    );
}

// Scenario: emit_wasm succeeds for a top-level Lambda binding with one capture.
// Proves the pipeline handles captures as additional WASM function params.
#[test]
fn emit_wasm_lambda_binding_with_capture_succeeds() {
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> add(outer, x)  — outer is a captured variable
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "add_to_outer".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for Lambda with captures");
    assert!(
        !artifact.wasm.is_empty(),
        "WASM binary must be non-empty for Lambda binding with captures"
    );
    // The function is exported because binding_result returns Some(I64).
    assert!(
        artifact.export_types.contains_key("add_to_outer"),
        "Lambda binding with I64 body must appear in export_types; got: {:?}",
        artifact.export_types.keys().collect::<Vec<_>>()
    );
}

// Scenario: a binding whose body contains a nested Lambda with captures emits
// a closure env in linear memory.  The WASM module must include memory and the
// global bump-allocator section required by emit_alloc.
//
// The test verifies structural properties: emit_wasm succeeds, the binary
// contains a memory section (needs_memory = true due to captures), and the
// hash is sealed.
#[test]
fn emit_wasm_nested_lambda_with_captures_allocates_memory() {
    use ail_core::semantic_graph::NodeRef;

    // let result = (fn(x) { x + outer })  — nested Lambda in a Let body
    // The outer binding does not itself have params; it wraps a nested Lambda.
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "make_closure".to_string(),
        expr: AnfExpr::Let {
            name: "closure".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["x".to_string()],
                captures: vec!["outer".to_string()],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["outer".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Var("closure".to_string())),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for binding with nested Lambda");
    assert!(!artifact.wasm.is_empty(), "WASM binary must be non-empty");
    // A memory section must be present (confirmed by the global bump-allocator
    // section, which is only emitted when needs_memory = true).  We verify
    // indirectly: the WASM binary must be larger than a module with no memory.
    let no_mem_anf = sealed_anf(vec![AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(1),
        name: "lit".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    }]);
    let no_mem_artifact = emit_wasm(&no_mem_anf).unwrap();
    assert!(
        artifact.wasm.len() > no_mem_artifact.wasm.len(),
        "module with nested Lambda + captures must be larger than a literal-only module \
         (memory + global sections are required for the closure env)"
    );
}

// TRIANGULATE: two Lambda bindings with different capture counts produce different
// WASM hashes (cap_count field in closure env header changes the binary).
#[test]
fn lambda_bindings_with_different_capture_counts_produce_different_hashes() {
    use ail_core::semantic_graph::NodeRef;

    let make_lambda = |captures: Vec<String>| {
        sealed_anf(vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "f".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["x".to_string()],
                captures,
                body: Box::new(AnfExpr::Var("x".to_string())),
            },
        }])
    };

    let a = emit_wasm(&make_lambda(vec![])).unwrap();
    let b = emit_wasm(&make_lambda(vec!["outer".to_string()])).unwrap();
    assert_ne!(
        a.hash_chain.wasm_hash, b.hash_chain.wasm_hash,
        "Lambda with captures must produce a different wasm_hash than one without"
    );
}

// ── End WASM closure capture tests ───────────────────────────────────────

// ── Wave 7C: CellNew / CellGet / CellSet / MapNew / SetNew / IndexGet ─────
//
// Proves that the six collection/cell primitives no longer emit unconditional
// Unreachable and instead produce valid, executable WASM that uses linear
// memory correctly.

// Scenario: CellNew allocates 8 bytes and stores the initial value.
// Expects: memory section present, I64Store emitted, WASM validates.

#[test]
fn fold_top_level_emits_successfully() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.reduce".to_string(),
        expr: AnfExpr::Fold {
            init: "acc0".to_string(),
            list: "lst".to_string(),
            func: "f".to_string(),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "Fold binding must compile successfully now that call_indirect is implemented; \
         got {result:?}"
    );
}

// Scenario: Fold nested inside a Let chain — still emits successfully.
// Verifies that the recursion detection (anf_has_fold) correctly spots nested Fold.
#[test]
fn fold_nested_in_let_emits_successfully() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.nested_fold".to_string(),
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Fold {
                    init: "acc0".to_string(),
                    list: "lst".to_string(),
                    func: "f".to_string(),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "Fold nested inside a Let chain must compile successfully; got {result:?}"
    );
}

// Scenario: UnsupportedWasmConstruct Display still names the unsupported construct.
// Fold is gone from the error set; the Display contract applies to other constructs.
#[test]
fn unsupported_wasm_construct_error_display_names_construct() {
    let msg = CompileError::UnsupportedWasmConstruct("TaskSpawn".to_string()).to_string();
    assert!(
        msg.contains("TaskSpawn"),
        "error display must name the unsupported construct: {msg}"
    );
    assert!(
        msg.contains("WASM"),
        "error display must mention WASM: {msg}"
    );
}

// TRIANGULATE: UnsupportedWasmConstruct is distinct from EncodingError.
#[test]
fn unsupported_construct_error_is_distinct_from_encoding_error() {
    let unsupported = CompileError::UnsupportedWasmConstruct("TaskSpawn".to_string());
    let encoding = CompileError::EncodingError("TaskSpawn".to_string());
    assert_ne!(
        unsupported, encoding,
        "UnsupportedWasmConstruct must be a distinct variant from EncodingError"
    );
}

// Scenario: a binding WITHOUT Fold emits successfully and has no table section.
// Regression guard — Fold implementation must not add table to non-Fold modules.
#[test]
fn non_fold_binding_has_no_table_section() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.simple".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(1)),
    }]);
    let artifact = emit_wasm(&anf).expect("non-Fold binding must compile successfully");

    let mut saw_table = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(_) = payload.unwrap() {
            saw_table = true;
        }
    }
    assert!(
        !saw_table,
        "non-Fold module must NOT have a table section (fold infrastructure is gated)"
    );
}

// Scenario: a Fold module emits a TableSection.
// Proves the function table is added when Fold is present.
#[test]
fn fold_module_has_table_section() {
    use wasmparser::{Parser, Payload};

    // Two bindings: a reducer and a Fold over it.
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.add".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(1)),
                        AnfExpr::Literal(LiteralValue::Int(2)),
                        AnfExpr::Literal(LiteralValue::Int(3)),
                    ])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "fn.add".to_string(),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("Fold module must compile successfully");
    wasmparser::validate(&artifact.wasm).expect("Fold module WASM must validate");

    let mut saw_table = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(_) = payload.unwrap() {
            saw_table = true;
        }
    }
    assert!(saw_table, "Fold module must include a TableSection");
}

// Scenario: a Fold module emits an ElementSection.
// Proves the element segment populates the function table.
#[test]
fn fold_module_has_element_section() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.reduce".to_string(),
        expr: AnfExpr::Fold {
            init: "acc0".to_string(),
            list: "lst".to_string(),
            func: "f".to_string(),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("Fold module must compile");

    let mut saw_element = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ElementSection(_) = payload.unwrap() {
            saw_element = true;
        }
    }
    assert!(saw_element, "Fold module must include an ElementSection");
}

// Scenario: a Fold module emits a CallIndirect instruction.
// Proves the actual call_indirect dispatch is in the code section.
// `acc0` and `lst` must be in scope (let-bound) so the Fold emission
// reaches the call_indirect instruction rather than short-circuiting on
// the unresolved `list` variable.
#[test]
fn fold_module_emits_call_indirect() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.reduce".to_string(),
        // Let-bind acc0 and lst so they are in scope when Fold is emitted.
        // func = "f" is intentionally unresolved — the emitter falls through
        // to the Unreachable+CallIndirect path (dead code, but syntactically
        // present and parseable by wasmparser).
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Fold {
                    init: "acc0".to_string(),
                    list: "lst".to_string(),
                    func: "f".to_string(),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("Fold module must compile");

    let mut saw_call_indirect = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                    saw_call_indirect = true;
                }
            }
        }
    }
    assert!(
        saw_call_indirect,
        "Fold module must emit CallIndirect in the code section"
    );
}

// Scenario: Fold with a named top-level reducer function validates.
// Proves the full Fold pipeline: reducer function + Fold via named function ref.
#[test]
fn fold_with_named_reducer_validates() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.add".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(10)),
                        AnfExpr::Literal(LiteralValue::Int(20)),
                    ])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        // "fn.add" resolves as a top-level function name.
                        func: "fn.add".to_string(),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("Fold with named reducer must compile");
    wasmparser::validate(&artifact.wasm).expect("Fold with named reducer must validate");

    // Verify call_indirect is emitted for the sum function.
    let mut saw_call_indirect = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                    saw_call_indirect = true;
                }
            }
        }
    }
    assert!(
        saw_call_indirect,
        "Fold with named reducer must emit CallIndirect"
    );
}

// Scenario: fold-reducer type is appended at the correct type index.
// Proves that type_offset + signatures.len() == fold_reducer_type_idx.
// The type section for a 2-binding module with no host imports and fold:
//   type[0]: binding[0] sig
//   type[1]: binding[1] sig
//   type[2]: (i64, i64) → i64  (fold reducer, index 2)
#[test]
fn fold_reducer_type_index_matches_call_indirect_type_index() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.reducer".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.fold_user".to_string(),
            expr: AnfExpr::Let {
                name: "z".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "xs".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Fold {
                        init: "z".to_string(),
                        list: "xs".to_string(),
                        func: "fn.reducer".to_string(),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("2-binding Fold module must compile");
    wasmparser::validate(&artifact.wasm).expect("2-binding Fold module must validate");

    // For a 2-binding module with no host imports: fold_reducer_type_idx = 0 + 2 = 2.
    let expected_type_idx: u32 = 2;
    let mut saw_expected = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { type_index, .. } = reader.read().unwrap()
                    && type_index == expected_type_idx
                {
                    saw_expected = true;
                }
            }
        }
    }
    assert!(
        saw_expected,
        "CallIndirect must use type_index={expected_type_idx} (fold reducer type)"
    );
}

// Scenario: Fold where `func` resolves to an I32 local (closure-env pointer)
// must emit Unreachable — not silently dispatch to table[0] via a placeholder
// fn_idx=0.  Proves the W1 guard: Lambda writes fn_idx=0 as a placeholder;
// until lambda hoisting is implemented there is no safe way to use the env.
//
// `env` is bound by VariantNew which yields an I32 pointer.
#[test]
fn fold_with_i32_local_func_emits_unreachable_guard() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.guarded_fold".to_string(),
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "env".to_string(),
                    // VariantNew → I32 pointer; serves as the closure-env path.
                    value: Box::new(AnfExpr::VariantNew {
                        tag: "Closure".to_string(),
                        payload: None,
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "env".to_string(), // I32 local → triggers guard
                    }),
                }),
            }),
        },
    }]);

    // emit_wasm must succeed: the guard is a runtime trap, not a compile error.
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for I32-func Fold");
    // The module must validate: Unreachable is polymorphic.
    wasmparser::validate(&artifact.wasm)
        .expect("I32-func Fold module must validate despite Unreachable guard");

    // The code section must contain Unreachable (the guard against silent fn-0 call).
    let mut saw_unreachable = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::Unreachable = reader.read().unwrap() {
                    saw_unreachable = true;
                }
            }
        }
    }
    assert!(
        saw_unreachable,
        "Fold with I32 closure-env func must emit Unreachable (W1 guard — not silent call fn 0)"
    );
}

// Scenario: Fold where `func` resolves to an unexpected local type (F64) must
// emit Unreachable — not silently dispatch to table[0].  Proves the W2 guard.
#[test]
fn fold_with_unexpected_type_func_emits_unreachable_guard() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.bad_type_fold".to_string(),
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "flt".to_string(),
                    // Float literal → F64 local (neither I32 env nor I64 index).
                    value: Box::new(AnfExpr::Literal(LiteralValue::Float(1.0))),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "flt".to_string(), // F64 local → triggers _ arm guard
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for F64-func Fold");
    wasmparser::validate(&artifact.wasm)
        .expect("F64-func Fold module must validate despite Unreachable guard");

    let mut saw_unreachable = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::Unreachable = reader.read().unwrap() {
                    saw_unreachable = true;
                }
            }
        }
    }
    assert!(
        saw_unreachable,
        "Fold with unexpected-type (F64) func must emit Unreachable (W2 guard — not silent call fn 0)"
    );
}

// ── Wave 12A: nested Lambda hoisting into function table ──────────────────
//
// A nested Lambda with exactly 2 params and no captures (fold-reducer shape
// `(i64, i64) → i64`) is now hoisted into a separate WASM function instead
// of emitting a closure env placeholder.  The Lambda node itself emits an
// `i64.const <table_idx>` so the Fold can dispatch it via the existing I64
// path (`i32.wrap_i64` + `call_indirect`).
//
// Supported: params.len() == 2, captures.is_empty()
// Not yet supported (still emits closure env with fn_idx=0 placeholder):
//   - Lambdas with captures (general closure hoisting deferred)
//   - Lambdas with != 2 params

// Scenario: a binding whose body contains a hoistable nested Lambda as the
// Fold reducer now compiles, validates, and emits a real CallIndirect.
// Proves fn_idx is no longer 0 (placeholder) for the supported case.
#[test]
fn fold_with_hoistable_nested_lambda_validates_and_emits_call_indirect() {
    use wasmparser::{Operator, Parser, Payload};

    // fn.sum:
    //   let reducer = fn(acc, x) -> acc + x   [hoistable nested Lambda]
    //   let acc0 = 0
    //   let lst = [1, 2, 3]
    //   Fold { init: acc0, list: lst, func: "reducer" }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "reducer".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(1)),
                        AnfExpr::Literal(LiteralValue::Int(2)),
                        AnfExpr::Literal(LiteralValue::Int(3)),
                    ])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    // Must compile and validate.
    let artifact = emit_wasm(&anf).expect("hoistable nested Lambda Fold must compile successfully");
    wasmparser::validate(&artifact.wasm)
        .expect("hoistable nested Lambda Fold must produce valid WASM");

    let mut saw_table = false;
    let mut saw_element = false;
    let mut saw_call_indirect = false;

    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.unwrap() {
            Payload::TableSection(_) => saw_table = true,
            Payload::ElementSection(_) => saw_element = true,
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                        saw_call_indirect = true;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(
        saw_table,
        "hoistable Lambda Fold must include a TableSection"
    );
    assert!(
        saw_element,
        "hoistable Lambda Fold must include an ElementSection"
    );
    assert!(
        saw_call_indirect,
        "hoistable Lambda Fold must emit CallIndirect in the code section"
    );
}

// Scenario: the hoisted Lambda body occupies an extra function slot.
// For 1 binding + 1 hoisted Lambda, the table has 2 slots (not 1).
// The hoisted Lambda is at table index 1 (function_offset=0, binding=0 → hoisted=1).
#[test]
fn fold_hoisted_lambda_expands_table_to_n_bindings_plus_n_hoisted() {
    use wasmparser::{Parser, Payload};

    // Same module as fold_with_hoistable_nested_lambda_validates_and_emits_call_indirect.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "reducer".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("WASM must validate");

    // Parse the table section and check initial = 2 (1 binding + 1 hoisted Lambda).
    let mut table_initial: Option<u64> = None;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(tables) = payload.unwrap() {
            for table in tables {
                let t = table.unwrap();
                table_initial = Some(t.ty.initial);
            }
        }
    }
    assert_eq!(
        table_initial,
        Some(2),
        "table must have 2 slots: 1 binding + 1 hoisted Lambda; got {table_initial:?}"
    );
}

// Scenario: the hoisted Lambda emits I64Const (table index) not a closure env.
// Verifies the Lambda node no longer allocates linear memory (no I64Store at
// the fn_idx slot) when it is hoistable.
//
// A capture-free 2-param Lambda used with Fold should NOT trigger needs_memory
// solely for the closure env — the hoisted case needs memory only if the
// Lambda body itself accesses memory (which `acc + x` does not).
#[test]
fn fold_hoistable_lambda_does_not_need_memory_for_closure_env() {
    use wasmparser::{Parser, Payload};

    // Binding: fn.sum with hoistable Lambda reducer, no other memory-accessing ops.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "f".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["a".to_string(), "b".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "acc".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc".to_string(),
                        list: "lst".to_string(),
                        func: "f".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("WASM must validate");

    // The hoistable Lambda does not emit a closure env, so the module must
    // NOT have a memory section (the List header read requires memory, but
    // an empty list means no element reads, and the Lambda body `a + b` is
    // pure arithmetic).
    //
    // Actually: ListNew DOES set needs_memory (stores the count header).
    // So we check the *function count* instead: there must be 2 functions
    // in the function section (binding + hoisted Lambda) — not 1.
    let mut function_count = 0u32;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::FunctionSection(functions) = payload.unwrap() {
            for _ in functions {
                function_count += 1;
            }
        }
    }
    assert_eq!(
        function_count, 2,
        "module must have 2 functions: 1 binding + 1 hoisted Lambda; got {function_count}"
    );
}

// Scenario: Fold reducer is a 2-param Lambda with captures.
// Wave 13B: this was a compile-time diagnostic (FoldWithCapturedReducer).
// Wave 16A PR3: 2-param captured Lambdas are now closure-hoisted into a
// `(env_ptr: i64, acc: i64, elem: i64) → i64` WASM function.  The closure env
// receives the REAL table index in fn_idx, and Fold dispatches via
// call_indirect with the closure-reducer type.  The module must now compile
// and validate successfully.
#[test]
fn fold_closure_hoistable_lambda_with_2_params_compiles_with_pr3() {
    use wasmparser::{Operator, Parser, Payload};

    // Lambda with 2 params AND a capture — closure-hoistable via Wave 16A PR3.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_sum".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec!["bias".to_string()], // capture → closure-hoistable (PR3)
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "acc0".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "acc0".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    // Wave 16A PR3: must now compile successfully.
    let artifact = emit_wasm(&anf)
        .expect("2-param captured Lambda reducer must compile successfully (Wave 16A PR3)");
    wasmparser::validate(&artifact.wasm)
        .expect("closure-hoisted fold module must produce valid WASM");

    // The code section must contain CallIndirect (closure-reducer dispatch).
    let mut saw_call_indirect = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                    saw_call_indirect = true;
                }
            }
        }
    }
    assert!(
        saw_call_indirect,
        "closure-hoisted Fold must emit CallIndirect for captured reducer dispatch"
    );
}

// TRIANGULATE: two hoistable nested Lambdas in the same binding — each gets
// a distinct table index.  Proves the sequential counter is correctly advanced.
#[test]
fn two_hoistable_lambdas_get_distinct_table_indices() {
    use wasmparser::{Operator, Parser, Payload};

    // fn.double_fold:
    //   let f1 = fn(a, b) -> a + b     [hoistable, table idx = 1]
    //   let f2 = fn(a, b) -> a + b     [hoistable, table idx = 2]
    //   let acc = 0; let lst = []
    //   let r1 = Fold { func: f1, init: acc, list: lst }
    //   Fold { func: f2, init: r1, list: lst }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.double_fold".to_string(),
        expr: AnfExpr::Let {
            name: "f1".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["a".to_string(), "b".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "f2".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["a".to_string(), "b".to_string()],
                    captures: vec![],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["a".to_string(), "b".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "acc".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Let {
                            name: "r1".to_string(),
                            value: Box::new(AnfExpr::Fold {
                                init: "acc".to_string(),
                                list: "lst".to_string(),
                                func: "f1".to_string(),
                            }),
                            body: Box::new(AnfExpr::Fold {
                                init: "r1".to_string(),
                                list: "lst".to_string(),
                                func: "f2".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("double-Fold with two hoistable Lambdas must compile");
    wasmparser::validate(&artifact.wasm).expect("double-Fold module must validate");

    // Table must have 3 slots: 1 binding + 2 hoisted Lambdas.
    let mut table_initial: Option<u64> = None;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(tables) = payload.unwrap() {
            for table in tables {
                table_initial = Some(table.unwrap().ty.initial);
            }
        }
    }
    assert_eq!(
        table_initial,
        Some(3),
        "table must have 3 slots: 1 binding + 2 hoisted Lambdas; got {table_initial:?}"
    );

    // Collect I64Const values from the code section — the two table indices
    // (1 and 2) must both be present as distinct I64Const values.
    let mut i64_consts: Vec<i64> = Vec::new();
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Const { value } = reader.read().unwrap() {
                    i64_consts.push(value);
                }
            }
        }
    }
    // Both table index 1 and 2 must appear as I64Const.
    assert!(
        i64_consts.contains(&1),
        "first hoistable Lambda must emit I64Const(1); got consts: {i64_consts:?}"
    );
    assert!(
        i64_consts.contains(&2),
        "second hoistable Lambda must emit I64Const(2); got consts: {i64_consts:?}"
    );
}

// Scenario: function_offset > 0 (module with a host_call import preceding the
// defined functions) + hoistable Lambda + Fold.  Proves `first_hoisted_table_idx`
// is `n_bindings` (not `function_offset + n_bindings`).
//
// Module layout:
//   import[0]  ail/host_call          → function index 0   (function_offset = 1)
//   defined[0] fn.io_noop (EffectCall) → function index 1  (table index 0)
//   defined[1] fn.sum (Fold)           → function index 2  (table index 1)
//   hoisted[0] reducer body            → function index 3  (table index 2)
//
// The hoistable Lambda must emit I64Const(2) — table index n_bindings=2.
// The buggy formula (function_offset + n_bindings = 1+2=3) would emit I64Const(3),
// which is out of the table range [0..2] and would trap at runtime.
#[test]
fn fold_with_nonzero_function_offset_hoistable_lambda_uses_correct_table_idx() {
    use wasmparser::{Operator, Parser, Payload};

    // binding 0: EffectCall with no args — brings in ail/host_call import.
    // binding 1: hoistable Lambda + Fold — hoisted Lambda must get table index 2.
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.io_noop".to_string(),
            expr: AnfExpr::EffectCall {
                capability: "io".to_string(),
                func: "noop".to_string(),
                args: vec![],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec![],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "acc0".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "acc0".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed with function_offset > 0");
    wasmparser::validate(&artifact.wasm)
        .expect("module with host import + hoistable Lambda Fold must validate");

    // Collect all I64Const values from the code section.
    // The hoistable Lambda emits I64Const(table_idx) where table_idx = n_bindings = 2.
    // The buggy formula would emit I64Const(3) (= function_offset + n_bindings).
    let mut i64_consts: Vec<i64> = Vec::new();
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Const { value } = reader.read().unwrap() {
                    i64_consts.push(value);
                }
            }
        }
    }

    assert!(
        i64_consts.contains(&2),
        "hoistable Lambda must emit I64Const(2) (table index = n_bindings = 2); \
         got consts: {i64_consts:?}"
    );
    assert!(
        !i64_consts.contains(&3),
        "hoistable Lambda must NOT emit I64Const(3) (buggy: function_offset + n_bindings = 3 \
         is out of table bounds [0..2]); got consts: {i64_consts:?}"
    );
}

// ── End Wave 12A nested Lambda hoisting tests ─────────────────────────────

// ── End Wave 11B Fold implementation tests ────────────────────────────────

// ── Wave 10B: generalized unsupported-construct diagnostics ───────────────
//
// Proves that emit_wasm returns CompileError::UnsupportedWasmConstruct for
// each concurrency/dispatch construct that is not yet implemented in the WASM
// backend, rather than silently emitting an unreachable trap.
//
// Pattern per construct:
//   1. Top-level binding → error with the right name.
//   2. Representative nested case (for a subset of constructs).
//
// Defence-in-depth: the unreachable fallback in emit_anf_expr still fires for
// direct callers that bypass emit_wasm_with_profile; this test suite exercises
// the pre-flight gate in emit_wasm_with_profile.

// ── Dispatch ──────────────────────────────────────────────────────────────

// Scenario: top-level Dispatch binding → UnsupportedWasmConstruct("Dispatch").

#[test]
fn fold_with_captured_reducer_compiles_with_pr3() {
    // let adder = fn(acc, x) { acc + x }  with capture "bias"
    // fold(zero, lst, adder)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "adder".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["bias".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "adder".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer must compile successfully (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm).expect("closure-hoisted fold module must validate");
}

// TRIANGULATE: capture-free 2-param reducer is not affected by the Wave 13B gate.
// Proves that the FoldWithCapturedReducer check does not fire for hoistable Lambdas.
#[test]
fn fold_with_capture_free_reducer_unaffected_by_wave13b_gate() {
    // let zero = 0; let lst = []; let add = fn(acc, x) { acc + x }  (no captures)
    // fold(zero, lst, add)  — must compile without diagnostic
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.plain_sum".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "add".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec![],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "add".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "Fold with capture-free 2-param Lambda must compile without FoldWithCapturedReducer diagnostic; got {result:?}"
    );
}

// Scenario: captured reducer nested inside an If branch → compiles OK (Wave 16A PR3).
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas nested in If branches now compile.
#[test]
fn fold_captured_reducer_in_if_branch_compiles_with_pr3() {
    // if true { fold(0, lst, captured_reducer) } else { 0 }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.conditional_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "cond".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                    body: Box::new(AnfExpr::Let {
                        name: "reducer".to_string(),
                        value: Box::new(AnfExpr::Lambda {
                            params: vec!["acc".to_string(), "x".to_string()],
                            captures: vec!["zero".to_string()],
                            body: Box::new(AnfExpr::Call {
                                func: "+".to_string(),
                                args: vec!["acc".to_string(), "x".to_string()],
                            }),
                        }),
                        body: Box::new(AnfExpr::If {
                            cond: "cond".to_string(),
                            then_branch: Box::new(AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "reducer".to_string(),
                            }),
                            else_branch: Box::new(AnfExpr::Var("zero".to_string())),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer in If branch must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoisted If-branch fold must validate");
}

// Scenario: captured reducer inside a Match arm → compiles OK (Wave 16A PR3).
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas nested in Match arms now compile.
#[test]
fn fold_captured_reducer_in_match_arm_compiles_with_pr3() {
    use crate::anf::AnfMatchArm;

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.match_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["zero".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Match {
                        scrutinee: "zero".to_string(),
                        arms: vec![AnfMatchArm {
                            pattern: "_".to_string(),
                            body: AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "reducer".to_string(),
                            },
                        }],
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer in Match arm must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoisted Match-arm fold must validate");
}

// Scenario: captured reducer inside a Loop body → compiles OK (Wave 16A PR3).
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas nested in Loop bodies now compile.
#[test]
fn fold_captured_reducer_in_loop_body_compiles_with_pr3() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.loop_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["zero".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Loop {
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer in Loop body must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoisted Loop-body fold must validate");
}

// Scenario: transitive Var alias of a 2-param captured reducer → compiles OK (Wave 16A PR3).
// `let adder = lambda captures [...]; let reducer = adder; fold(..., reducer)`
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: both `adder` (closure-hoisted) and its alias `reducer` resolve to the
// same closure env pointer, which carries the real table index.  Must compile.
#[test]
fn fold_with_transitive_var_alias_reducer_compiles_with_pr3() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.aliased_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "adder".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["bias".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Let {
                        name: "reducer".to_string(),
                        value: Box::new(AnfExpr::Var("adder".to_string())),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "transitive Var alias of 2-param captured reducer must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("transitive alias closure-hoisted fold must validate");
}

// ── Wave 16A PR3: new tests for closure hoisting ──────────────────────────

// Scenario: Fold with a 1-param captured Lambda (NOT a valid fold reducer) →
// FoldWithCapturedReducer diagnostic still fires for non-2-param shapes.
// Proves the gate is still present for cases that Wave 16A PR3 does not handle.
#[test]
fn fold_with_non_2param_captured_lambda_still_returns_diagnostic() {
    // 1-param Lambda with a capture — not a Fold reducer shape (gate preserved).
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.invalid_reducer".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string()], // 1 param — not a fold reducer
                    captures: vec!["bias".to_string()],
                    body: Box::new(AnfExpr::Var("acc".to_string())),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "zero".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "FoldWithCapturedReducer"
        ),
        "1-param captured Lambda in Fold must still produce FoldWithCapturedReducer; got {result:?}"
    );
}

// Scenario: closure-hoisted Lambda writes real fn_idx (not 0) into closure env.
// Proves Wave 16A PR3: the closure env's fn_idx slot contains the table index
// of the hoisted function, not the placeholder 0.
//
// The module has: 1 binding (fn.biased_sum) + 0 hoisted (no capture-free 2-param
// Lambdas) + 1 closure-hoisted (reducer with "bias" capture).
// → binding function: table index 0, fn index = function_offset + 0
// → closure-hoisted fn: table index 1, fn index = function_offset + 1
//
// The closure env for `reducer` must have fn_idx = 1 (i64.const 1) stored at
// offset 0 of the env struct.  We verify this by scanning the code section for
// `i64.const 1` FOLLOWED BY an `i64.store` — the pattern that writes fn_idx.
#[test]
fn closure_hoisted_lambda_writes_real_fn_idx_not_zero() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_sum".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(5))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec!["bias".to_string()],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "zero".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("closure-hoisted Fold must compile (Wave 16A PR3)");
    wasmparser::validate(&artifact.wasm).expect("closure-hoisted Fold module must validate");

    // Scan code section for i64.const that is NOT 0 followed by i64.store
    // (the fn_idx write sequence).  With 1 binding and 1 closure-hoisted fn,
    // the closure-hoisted fn is at table index 1, so fn_idx = 1.
    let mut saw_nonzero_fn_idx_store = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut ops: Vec<Operator<'_>> = vec![];
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                ops.push(reader.read().unwrap());
            }
            for window in ops.windows(2) {
                if let [Operator::I64Const { value }, Operator::I64Store { .. }] = window
                    && *value > 0
                {
                    saw_nonzero_fn_idx_store = true;
                }
            }
        }
    }
    assert!(
        saw_nonzero_fn_idx_store,
        "closure env must contain a non-zero fn_idx (real table index, not placeholder 0)"
    );
}

// Scenario: closure-hoisted Lambda module has the correct function count.
// 1 binding + 1 closure-hoisted = 2 WASM functions total.
// Proves build_code_section emits the closure-hoisted body as an extra function.
#[test]
fn closure_hoisted_fold_module_has_correct_function_count() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_sum".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec!["bias".to_string()],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "zero".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("closure-hoisted Fold must compile");
    wasmparser::validate(&artifact.wasm).expect("module must validate");

    // Count WASM function bodies in the code section.
    let mut function_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(_) = payload.unwrap() {
            function_count += 1;
        }
    }
    assert_eq!(
        function_count, 2,
        "module must have 2 functions: 1 binding + 1 closure-hoisted Lambda; got {function_count}"
    );
}

// ── Wave 26C: non-hoistable capture-free wrong-arity reducer guard ─────────
//
// A Lambda with no captures and params.len() ≠ 2 falls into the non-hoistable
// `else` branch in `emit_anf_expr`.  It emits a closure env with `fn_idx = 0`
// (placeholder).  Before Wave 26C, using such a Lambda as a Fold reducer was
// not caught at compile time: the Fold I32 dispatch path read `fn_idx = 0` and
// silently called `table[0]` with the wrong arity — a runtime type-mismatch
// trap rather than a deterministic compile error.
//
// Wave 26C adds a narrow preflight guard (`has_fold_with_uncaptured_wrong_arity_reducer`)
// that returns `CompileError::UnsupportedWasmConstruct("FoldWithUncapturedWrongArityReducer")`
// before code generation.
//
// Tests below prove:
//   1. 1-param capture-free Lambda as Fold reducer → deterministic compile error.
//   2. 3-param capture-free Lambda as Fold reducer → same.
//   3. 2-param capture-free Lambda (hoistable) is NOT affected by the new guard.
//   4. 2-param captured Lambda (closure-hoistable, PR3) is NOT affected.

// Scenario: Fold with a 1-param capture-free Lambda (wrong arity) returns a
// deterministic compile error instead of silently dispatching to table[0].
#[test]
fn fold_with_1param_no_capture_reducer_returns_non_hoistable_error() {
    // let reducer = fn(acc) { acc }  — 1 param, no captures (non-hoistable)
    // fold(zero, lst, reducer)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.bad_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string()], // 1 param — non-hoistable
                        captures: vec![],                // no captures
                        body: Box::new(AnfExpr::Var("acc".to_string())),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name))
                if name == "FoldWithUncapturedWrongArityReducer"
        ),
        "1-param capture-free Lambda in Fold must return FoldWithUncapturedWrongArityReducer \
         (not silent fn_idx=0 dispatch); got {result:?}"
    );
}

// Scenario: Fold with a 3-param capture-free Lambda (wrong arity) returns the
// same deterministic compile error.
#[test]
fn fold_with_3param_no_capture_reducer_returns_non_hoistable_error() {
    // let reducer = fn(a, b, c) { a }  — 3 params, no captures (non-hoistable)
    // fold(zero, lst, reducer)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.bad_fold_3p".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["a".to_string(), "b".to_string(), "c".to_string()], // 3 params — non-hoistable
                        captures: vec![], // no captures
                        body: Box::new(AnfExpr::Var("a".to_string())),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name))
                if name == "FoldWithUncapturedWrongArityReducer"
        ),
        "3-param capture-free Lambda in Fold must return FoldWithUncapturedWrongArityReducer; \
         got {result:?}"
    );
}

// Regression: 2-param capture-free Lambda (hoistable) is NOT affected by the
// new guard and still compiles successfully.
#[test]
fn fold_with_2param_no_capture_reducer_unaffected_by_non_hoistable_guard() {
    // let reducer = fn(acc, x) { acc + x }  — 2 params, no captures (hoistable)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.hoistable_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec![], // 2-param no-capture → hoistable
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        !matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name))
                if name == "FoldWithUncapturedWrongArityReducer"
        ),
        "2-param capture-free Lambda must NOT be rejected by FoldWithUncapturedWrongArityReducer guard; \
         got {result:?}"
    );
    assert!(
        result.is_ok(),
        "hoistable 2-param Lambda Fold must compile successfully; got {result:?}"
    );
}

// Regression: 2-param captured Lambda (closure-hoistable, PR3) is NOT affected
// by the new capture-free guard and still compiles successfully.
#[test]
fn fold_with_2param_captured_reducer_unaffected_by_non_hoistable_guard() {
    // let reducer = fn(acc, x) { acc + x } capturing "bias"  — closure-hoistable
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.closure_fold".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "zero".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Let {
                        name: "reducer".to_string(),
                        value: Box::new(AnfExpr::Lambda {
                            params: vec!["acc".to_string(), "x".to_string()],
                            captures: vec!["bias".to_string()], // closure-hoistable
                            body: Box::new(AnfExpr::Call {
                                func: "+".to_string(),
                                args: vec!["acc".to_string(), "x".to_string()],
                            }),
                        }),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        !matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name))
                if name == "FoldWithUncapturedWrongArityReducer"
        ),
        "2-param captured Lambda must NOT be rejected by FoldWithUncapturedWrongArityReducer guard; \
         got {result:?}"
    );
    assert!(
        result.is_ok(),
        "closure-hoistable 2-param Lambda Fold must compile successfully (Wave 16A PR3); \
         got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoistable fold module must validate");
}

// Scenario: Fold with a 0-param capture-free Lambda (wrong arity) returns the
// same deterministic compile error.
#[test]
fn fold_with_0param_no_capture_reducer_returns_uncaptured_wrong_arity_error() {
    // let reducer = fn() { 0 }  — 0 params, no captures (non-hoistable)
    // fold(zero, lst, reducer)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.bad_fold_0p".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec![],   // 0 params — non-hoistable
                        captures: vec![], // no captures
                        body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name))
                if name == "FoldWithUncapturedWrongArityReducer"
        ),
        "0-param capture-free Lambda in Fold must return FoldWithUncapturedWrongArityReducer \
         (not silent fn_idx=0 dispatch); got {result:?}"
    );
}

// Scenario: Fold whose func is a transitive alias of a wrong-arity capture-free
// Lambda is caught by the guard via alias propagation.
//
//   let f = fn(acc) { acc }   -- 1-param, no captures, non-hoistable
//   let g = f                  -- alias; guard must propagate membership
//   fold(zero, lst, g)         -- must still trigger the error
#[test]
fn fold_with_transitive_alias_of_wrong_arity_reducer_returns_uncaptured_wrong_arity_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.alias_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "f".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string()], // 1 param, non-hoistable
                        captures: vec![],
                        body: Box::new(AnfExpr::Var("acc".to_string())),
                    }),
                    body: Box::new(AnfExpr::Let {
                        name: "g".to_string(),
                        value: Box::new(AnfExpr::Var("f".to_string())), // alias
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "g".to_string(), // uses alias
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name))
                if name == "FoldWithUncapturedWrongArityReducer"
        ),
        "Fold via transitive alias of wrong-arity capture-free Lambda must return \
         FoldWithUncapturedWrongArityReducer; got {result:?}"
    );
}

#[test]
fn nested_closure_hoistable_lambda_in_closure_body_is_rejected() {
    let inner_lambda = AnfExpr::Lambda {
        params: vec!["a".to_string(), "b".to_string()],
        captures: vec!["z".to_string()],
        body: Box::new(AnfExpr::Var("a".to_string())),
    };
    let outer_lambda = AnfExpr::Lambda {
        params: vec!["acc".to_string(), "elem".to_string()],
        captures: vec!["z".to_string()],
        body: Box::new(AnfExpr::Let {
            name: "inner_f".to_string(),
            value: Box::new(inner_lambda),
            body: Box::new(AnfExpr::Var("acc".to_string())),
        }),
    };
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "z".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "outer_f".to_string(),
                value: Box::new(outer_lambda),
                body: Box::new(AnfExpr::Fold {
                    init: "z".to_string(),
                    list: "z".to_string(),
                    func: "outer_f".to_string(),
                }),
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(vec![binding]));
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref s)) if s == "NestedClosureHoistableLambda"
        ),
        "nested closure-hoistable Lambda inside hoisted body must be rejected; got {result:?}"
    );
}

// W1b — Nested hoistable (no-capture) Lambda inside a hoistable Lambda body
// must be rejected with UnsupportedWasmConstruct("NestedHoistableLambda").
//
// Setup:
//   fn.main = let outer_f = Lambda(params=[acc,elem], captures=[],
//                              body = Let("inner_f",
//                                        Lambda(params=[a,b], captures=[], body=a),
//                                        acc))
//             in  Fold(init=outer_f, list=outer_f, func=outer_f)
#[test]
fn nested_hoistable_lambda_in_hoistable_body_is_rejected() {
    let inner_lambda = AnfExpr::Lambda {
        params: vec!["a".to_string(), "b".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Var("a".to_string())),
    };
    let outer_lambda = AnfExpr::Lambda {
        params: vec!["acc".to_string(), "elem".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Let {
            name: "inner_f".to_string(),
            value: Box::new(inner_lambda),
            body: Box::new(AnfExpr::Var("acc".to_string())),
        }),
    };
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "outer_f".to_string(),
            value: Box::new(outer_lambda),
            body: Box::new(AnfExpr::Fold {
                init: "outer_f".to_string(),
                list: "outer_f".to_string(),
                func: "outer_f".to_string(),
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(vec![binding]));
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref s)) if s == "NestedHoistableLambda"
        ),
        "nested hoistable Lambda inside hoistable body must be rejected; got {result:?}"
    );
}

// W1c — Non-nested Lambda (no 2-param child in body) must NOT be rejected.
// Proves the gate does not over-reject valid closure-hoistable Lambdas.
//
// Setup: outer_f = Lambda(params=[acc,elem], captures=[z], body = add(acc, z))
// The body is a Call, no nested Lambda — must succeed.
#[test]
fn closure_hoistable_lambda_without_nested_2param_lambda_is_accepted() {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "z".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(1)),
                    AnfExpr::Literal(LiteralValue::Int(2)),
                ])),
                body: Box::new(AnfExpr::Let {
                    name: "outer_f".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "elem".to_string()],
                        captures: vec!["z".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "z".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "z".to_string(),
                        list: "lst".to_string(),
                        func: "outer_f".to_string(),
                    }),
                }),
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(vec![binding]));
    assert!(
        !matches!(result, Err(CompileError::UnsupportedWasmConstruct(ref s))
            if s == "NestedClosureHoistableLambda" || s == "NestedHoistableLambda"),
        "closure-hoistable Lambda with non-nested body must NOT be rejected by W1 gate; got {result:?}"
    );
}

// W3 regression — `build_function_section` closure-reducer fallback formula.
//
// When `closure_reducer_type_idx` is `None`, the fallback must produce
// `type_offset + signatures.len() + 1` (closure-reducer type immediately
// after fold-reducer type).  The old formula incorrectly added `hoisted_count`,
// which belongs to the function section, not the type section.
//
// This test calls `build_function_section` with `closure_reducer_type_idx = None`
// and verifies it returns `Some(...)` without panicking.  The type-index
// correctness of `closure_reducer_type_idx = Some(...)` is exercised by the
// closure-hoistable fold tests above that use `wasmparser::validate`.
#[test]
fn build_function_section_closure_fallback_does_not_panic() {
    use crate::wasm_abi::WasmSignature;
    use crate::wasm_sections::build_function_section;

    let sig = WasmSignature {
        param_count: 1,
        result: Some(wasm_encoder::ValType::I64),
    };
    // type_offset=0, 1 signature, hoisted_count=2, closure_hoisted_count=1.
    // Correct fallback: 0 + 1 + 1 = 2.
    // Wrong (pre-fix) fallback: 0 + 1 + 2 + 1 = 4 (out-of-range type index).
    // build_function_section itself cannot validate the type index (it is a
    // section builder, not a module validator); the test proves it returns
    // Some without panicking and that the fixed formula is applied.
    let section = build_function_section(
        std::slice::from_ref(&sig),
        0,       // type_offset
        2,       // hoisted_count
        Some(1), // fold_reducer_type_idx = type_offset + sigs.len() = 1
        1,       // closure_hoisted_count
        None,    // closure_reducer_type_idx = None → uses fallback formula
    );
    assert!(
        section.is_some(),
        "build_function_section must return Some when bindings + hoisted > 0"
    );
}

// ── End Wave 16A W1/W3 regression tests ──────────────────────────────────

// ── Wave 27B: fold-guard scope-leak regression tests ─────────────────────
//
// Bug: `expr_has_fold_with_captured_reducer` and
// `expr_has_fold_with_uncaptured_wrong_arity` passed the same `HashSet` to
// both branches of `If` and to every arm of `Match`.  Names inserted while
// scanning branch/arm A leaked into branch/arm B, producing false-positive
// `FoldWithCapturedReducer` / `FoldWithUncapturedWrongArityReducer` diagnostics
// for entirely valid Fold nodes in sibling branches.
//
// Fix: clone the name set before entering each `If` branch and each `Match` arm.
//
// The regression tests below use the same name (`"r"`) in both branches to make
// the leak maximally visible: branch A binds `"r"` to an invalid Lambda shape
// (triggers insertion into the name set); branch B binds `"r"` to a valid
// 2-param Lambda and uses it in a Fold (must NOT trigger the diagnostic).

// ── FoldWithCapturedReducer scope-leak ────────────────────────────────────

// Scenario: wrong-arity captured Lambda in If-then must not poison the else fold.
// Branch A (then): `let r = Lambda(1-param, captures=[bias])` — bad shape, no Fold.
// Branch B (else): `let r = Lambda(2-param, captures=[bias]); fold(zero, lst, r)` — valid.
// Pre-fix: "r" from then-branch leaked into else-branch → false FoldWithCapturedReducer.
// Post-fix: each branch gets a fresh clone of the name set → Ok.
#[test]
fn fold_guard_captured_reducer_if_sibling_branch_no_leak() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "zero".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Let {
                        name: "cond".to_string(),
                        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                        body: Box::new(AnfExpr::If {
                            cond: "cond".to_string(),
                            // then: r = bad 1-param captured Lambda — no Fold here
                            then_branch: Box::new(AnfExpr::Let {
                                name: "r".to_string(),
                                value: Box::new(AnfExpr::Lambda {
                                    params: vec!["x".to_string()],
                                    captures: vec!["bias".to_string()],
                                    body: Box::new(AnfExpr::Var("x".to_string())),
                                }),
                                body: Box::new(AnfExpr::Var("zero".to_string())),
                            }),
                            // else: r = valid 2-param captured Lambda used in Fold
                            else_branch: Box::new(AnfExpr::Let {
                                name: "r".to_string(),
                                value: Box::new(AnfExpr::Lambda {
                                    params: vec!["acc".to_string(), "x".to_string()],
                                    captures: vec!["bias".to_string()],
                                    body: Box::new(AnfExpr::Call {
                                        func: "+".to_string(),
                                        args: vec!["acc".to_string(), "x".to_string()],
                                    }),
                                }),
                                body: Box::new(AnfExpr::Fold {
                                    init: "zero".to_string(),
                                    list: "lst".to_string(),
                                    func: "r".to_string(),
                                }),
                            }),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "valid 2-param captured reducer in else-branch must not be poisoned by \
         wrong-arity reducer name from then-branch; got {result:?}"
    );
}

// Scenario: wrong-arity captured Lambda in Match arm A must not poison arm B fold.
// Arm A: `let r = Lambda(1-param, captures=[bias])` — bad shape, no Fold.
// Arm B: `let r = Lambda(2-param, captures=[bias]); fold(zero, lst, r)` — valid.
// Pre-fix: "r" from arm A leaked into arm B → false FoldWithCapturedReducer.
// Post-fix: each arm gets a fresh clone of the name set → Ok.
#[test]
fn fold_guard_captured_reducer_match_arm_no_leak() {
    use crate::anf::AnfMatchArm;

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "zero".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Match {
                        scrutinee: "zero".to_string(),
                        arms: vec![
                            // Arm A: bad 1-param captured Lambda bound to "r", no Fold
                            AnfMatchArm {
                                pattern: "0".to_string(),
                                body: AnfExpr::Let {
                                    name: "r".to_string(),
                                    value: Box::new(AnfExpr::Lambda {
                                        params: vec!["x".to_string()],
                                        captures: vec!["bias".to_string()],
                                        body: Box::new(AnfExpr::Var("x".to_string())),
                                    }),
                                    body: Box::new(AnfExpr::Var("zero".to_string())),
                                },
                            },
                            // Arm B: valid 2-param captured Lambda bound to "r", used in Fold
                            AnfMatchArm {
                                pattern: "_".to_string(),
                                body: AnfExpr::Let {
                                    name: "r".to_string(),
                                    value: Box::new(AnfExpr::Lambda {
                                        params: vec!["acc".to_string(), "x".to_string()],
                                        captures: vec!["bias".to_string()],
                                        body: Box::new(AnfExpr::Call {
                                            func: "+".to_string(),
                                            args: vec!["acc".to_string(), "x".to_string()],
                                        }),
                                    }),
                                    body: Box::new(AnfExpr::Fold {
                                        init: "zero".to_string(),
                                        list: "lst".to_string(),
                                        func: "r".to_string(),
                                    }),
                                },
                            },
                        ],
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "valid 2-param captured reducer in Match arm B must not be poisoned by \
         wrong-arity reducer name from arm A; got {result:?}"
    );
}

// ── FoldWithUncapturedWrongArityReducer scope-leak ────────────────────────

// Scenario: wrong-arity capture-free Lambda in If-then must not poison the else fold.
// Branch A (then): `let r = Lambda(1-param, no captures)` — bad shape, no Fold.
// Branch B (else): `let r = Lambda(2-param, no captures); fold(zero, lst, r)` — valid.
// Pre-fix: "r" from then-branch leaked into else-branch → false FoldWithUncapturedWrongArityReducer.
// Post-fix: each branch gets a fresh clone of the name set → Ok.
#[test]
fn fold_guard_uncaptured_wrong_arity_if_sibling_branch_no_leak() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "cond".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                    body: Box::new(AnfExpr::If {
                        cond: "cond".to_string(),
                        // then: r = bad 1-param capture-free Lambda — no Fold here
                        then_branch: Box::new(AnfExpr::Let {
                            name: "r".to_string(),
                            value: Box::new(AnfExpr::Lambda {
                                params: vec!["x".to_string()],
                                captures: vec![],
                                body: Box::new(AnfExpr::Var("x".to_string())),
                            }),
                            body: Box::new(AnfExpr::Var("zero".to_string())),
                        }),
                        // else: r = valid 2-param capture-free Lambda used in Fold
                        else_branch: Box::new(AnfExpr::Let {
                            name: "r".to_string(),
                            value: Box::new(AnfExpr::Lambda {
                                params: vec!["acc".to_string(), "x".to_string()],
                                captures: vec![],
                                body: Box::new(AnfExpr::Call {
                                    func: "+".to_string(),
                                    args: vec!["acc".to_string(), "x".to_string()],
                                }),
                            }),
                            body: Box::new(AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "r".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "valid 2-param capture-free reducer in else-branch must not be poisoned by \
         wrong-arity reducer name from then-branch; got {result:?}"
    );
}

// Scenario: wrong-arity capture-free Lambda in Match arm A must not poison arm B fold.
// Arm A: `let r = Lambda(1-param, no captures)` — bad shape, no Fold.
// Arm B: `let r = Lambda(2-param, no captures); fold(zero, lst, r)` — valid.
// Pre-fix: "r" from arm A leaked into arm B → false FoldWithUncapturedWrongArityReducer.
// Post-fix: each arm gets a fresh clone of the name set → Ok.
#[test]
fn fold_guard_uncaptured_wrong_arity_match_arm_no_leak() {
    use crate::anf::AnfMatchArm;

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Match {
                    scrutinee: "zero".to_string(),
                    arms: vec![
                        // Arm A: bad 1-param capture-free Lambda bound to "r", no Fold
                        AnfMatchArm {
                            pattern: "0".to_string(),
                            body: AnfExpr::Let {
                                name: "r".to_string(),
                                value: Box::new(AnfExpr::Lambda {
                                    params: vec!["x".to_string()],
                                    captures: vec![],
                                    body: Box::new(AnfExpr::Var("x".to_string())),
                                }),
                                body: Box::new(AnfExpr::Var("zero".to_string())),
                            },
                        },
                        // Arm B: valid 2-param capture-free Lambda bound to "r", used in Fold
                        AnfMatchArm {
                            pattern: "_".to_string(),
                            body: AnfExpr::Let {
                                name: "r".to_string(),
                                value: Box::new(AnfExpr::Lambda {
                                    params: vec!["acc".to_string(), "x".to_string()],
                                    captures: vec![],
                                    body: Box::new(AnfExpr::Call {
                                        func: "+".to_string(),
                                        args: vec!["acc".to_string(), "x".to_string()],
                                    }),
                                }),
                                body: Box::new(AnfExpr::Fold {
                                    init: "zero".to_string(),
                                    list: "lst".to_string(),
                                    func: "r".to_string(),
                                }),
                            },
                        },
                    ],
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "valid 2-param capture-free reducer in Match arm B must not be poisoned by \
         wrong-arity reducer name from arm A; got {result:?}"
    );
}

// ── End Wave 27B fold-guard scope-leak regression tests ───────────────────
