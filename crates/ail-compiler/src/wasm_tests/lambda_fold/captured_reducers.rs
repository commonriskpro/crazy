use super::*;

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

