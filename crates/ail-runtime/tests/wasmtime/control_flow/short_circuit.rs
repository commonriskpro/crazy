use crate::helpers::*;

#[test]
fn short_circuit_and_false_left_skips_right() {
    let expr = AnfExpr::Let {
        name: "f".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
        body: Box::new(AnfExpr::ShortCircuitAnd {
            left: "f".to_string(),
            right: Box::new(AnfExpr::Abort {
                message: "dead code: AND right with false left".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.and_false"),
        RuntimeValue::I64(0),
        "ShortCircuitAnd with left=false must return I64(0) without evaluating right"
    );
}

// RUNTIME-SHORTCIRCUITAND-2
//
// fn.main =
//   let t = true in
//   let r = 7    in
//   ShortCircuitAnd { left: "t", right: Var("r") }
//
// left=true → then branch → evaluates right (Var("r") = 7) → I64(7).
#[test]
fn short_circuit_and_true_left_evaluates_right() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
            body: Box::new(AnfExpr::ShortCircuitAnd {
                left: "t".to_string(),
                right: Box::new(AnfExpr::Var("r".to_string())),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.and_true"),
        RuntimeValue::I64(7),
        "ShortCircuitAnd with left=true must evaluate right and return I64(7)"
    );
}

// RUNTIME-SHORTCIRCUITOR-1
//
// fn.main =
//   let t = true in
//   ShortCircuitOr { left: "t", right: Abort{"dead code"} }
//
// left=true → then branch → I64(1); right (Abort) is NEVER evaluated.
// If short-circuit were broken and right were reached, Abort would trap.
// No trap proves right was not evaluated.
#[test]
fn short_circuit_or_true_left_skips_right() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::ShortCircuitOr {
            left: "t".to_string(),
            right: Box::new(AnfExpr::Abort {
                message: "dead code: OR right with true left".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.or_true"),
        RuntimeValue::I64(1),
        "ShortCircuitOr with left=true must return I64(1) without evaluating right"
    );
}

// RUNTIME-SHORTCIRCUITOR-2
//
// fn.main =
//   let f = false in
//   let r = 7     in
//   ShortCircuitOr { left: "f", right: Var("r") }
//
// left=false → else branch → evaluates right (Var("r") = 7) → I64(7).
#[test]
fn short_circuit_or_false_left_evaluates_right() {
    let expr = AnfExpr::Let {
        name: "f".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
        body: Box::new(AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
            body: Box::new(AnfExpr::ShortCircuitOr {
                left: "f".to_string(),
                right: Box::new(AnfExpr::Var("r".to_string())),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.or_false"),
        RuntimeValue::I64(7),
        "ShortCircuitOr with left=false must evaluate right and return I64(7)"
    );
}
