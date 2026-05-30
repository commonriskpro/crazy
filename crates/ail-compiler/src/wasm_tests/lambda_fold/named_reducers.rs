use super::*;

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
