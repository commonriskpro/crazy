use super::*;

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

