use super::*;

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

