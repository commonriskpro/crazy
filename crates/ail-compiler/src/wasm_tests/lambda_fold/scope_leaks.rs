use super::*;

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
