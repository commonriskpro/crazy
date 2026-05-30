use super::*;

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

