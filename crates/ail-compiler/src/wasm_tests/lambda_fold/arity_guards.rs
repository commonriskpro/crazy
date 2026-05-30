use super::*;

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

