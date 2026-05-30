use crate::helpers::*;

#[test]
fn while_loop_false_condition_body_never_runs() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "zero".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "flag".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                    body: Box::new(AnfExpr::Let {
                        name: "_w".to_string(),
                        value: Box::new(AnfExpr::WhileLoop {
                            cond: "flag".to_string(),
                            body: Box::new(AnfExpr::CellSet {
                                cell: "c".to_string(),
                                value: "zero".to_string(),
                            }),
                        }),
                        body: Box::new(AnfExpr::CellGet {
                            cell: "c".to_string(),
                        }),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "WhileLoop with false condition must skip the body; CellGet must return 42"
    );
}

// RUNTIME-WHILE-2
//
// fn.main =
//   let start = 5 in
//   let c     = CellNew(start) in
//   let go    = true in
//   let _w    = while(go,
//                 let cur  = CellGet(c)           in
//                 let one  = 1                    in
//                 let next = sub(cur, one)         in
//                 let _s   = CellSet(c, next)     in
//                 break(0))                         ← exits after one iteration
//   in
//   CellGet(c)
//
// go = true → condition fires, body runs once:
//   cur  = 5
//   next = 5 − 1 = 4
//   CellSet(c, 4)
//   break → exits loop
// CellGet(c) must return 4.
//
// This also proves that WhileLoop returns a unit (I32 0) so it can be used
// as the value of the outer Let binding without a WASM validation error.
#[test]
fn while_loop_body_runs_once_then_breaks() {
    let expr = AnfExpr::Let {
        name: "start".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(5))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "start".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "go".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                body: Box::new(AnfExpr::Let {
                    name: "_w".to_string(),
                    value: Box::new(AnfExpr::WhileLoop {
                        cond: "go".to_string(),
                        body: Box::new(AnfExpr::Let {
                            name: "cur".to_string(),
                            value: Box::new(AnfExpr::CellGet {
                                cell: "c".to_string(),
                            }),
                            body: Box::new(AnfExpr::Let {
                                name: "one".to_string(),
                                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                                body: Box::new(AnfExpr::Let {
                                    name: "next".to_string(),
                                    value: Box::new(AnfExpr::Call {
                                        func: "-".to_string(),
                                        args: vec!["cur".to_string(), "one".to_string()],
                                    }),
                                    body: Box::new(AnfExpr::Let {
                                        name: "_s".to_string(),
                                        value: Box::new(AnfExpr::CellSet {
                                            cell: "c".to_string(),
                                            value: "next".to_string(),
                                        }),
                                        body: Box::new(AnfExpr::Break {
                                            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                                        }),
                                    }),
                                }),
                            }),
                        }),
                    }),
                    body: Box::new(AnfExpr::CellGet {
                        cell: "c".to_string(),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(4),
        "WhileLoop body must run once (5→4), break, then CellGet must return 4"
    );
}

// ── Wave 19A: ANF control-flow execution conformance ─────────────────────
//
// Spec scenarios covered (RUNTIME-SEQ-1..3, RUNTIME-RETURN-1..3,
// RUNTIME-CONTINUE-1, RUNTIME-ABORT-1, RUNTIME-ASSUME-1,
// RUNTIME-RUNTIMECHECK-1..2, RUNTIME-SHORTCIRCUITAND-1..2,
// RUNTIME-SHORTCIRCUITOR-1..2):
//
//  RUNTIME-SEQ-1: Empty Seq produces unit (I32 0) — proves the empty-Seq
//    guard pushes I32Const(0) rather than leaving the stack underflowed.
//
//  RUNTIME-SEQ-2: Single-element Seq(Unit) returns the element's value
//    (I32 0) — proves the single-element path emits no spurious Drop.
//
//  RUNTIME-SEQ-3: Multi-element Seq applies both CellSet effects in order;
//    the cell holds the last written value — proves intermediate results are
//    dropped and both effects execute sequentially.
//
//  RUNTIME-RETURN-1: Return(42) causes the function to exit with I64(42)
//    before the implicit End — proves the Return instruction transfers
//    control and the value is carried correctly.
//
//  RUNTIME-RETURN-2: Return inside a taken if-branch exits before the
//    else branch would evaluate — proves early return on a conditional path.
//
//  RUNTIME-RETURN-3: Return(Unit) is the first element of a Seq; the second
//    element is Abort.  The function returns I32(0) without trapping, proving
//    Return's early-exit semantics: if Return did not emit the WASM `return`
//    instruction, Abort would fire and the invocation would return
//    Err(EncodingError) instead of Ok(I32(0)).
//
//  RUNTIME-CONTINUE-1: Continue inside a WhileLoop body jumps back to the
//    loop's condition check.  A counter cell increments each iteration;
//    the loop exits via Break when the counter reaches 3.  CellGet must
//    return 3 — proves Continue restarts iteration without loss of side
//    effects accumulated in the body.
//
//  RUNTIME-ABORT-1: Abort always traps — invoke returns
//    Err(RuntimeError::EncodingError) containing a Wasmtime unreachable
//    message.
//
//  RUNTIME-ASSUME-1: Assume emits no instructions and causes no trap; the
//    function returns normally with RuntimeValue::Unit — proves Assume is a
//    pure static annotation with zero runtime cost.
//
//  RUNTIME-RUNTIMECHECK-1: RuntimeCheck with cond=false (no violation
//    detected) does not trap; the function returns RuntimeValue::Unit —
//    proves the guard fires only when the condition is truthy.
//
//  RUNTIME-RUNTIMECHECK-2: RuntimeCheck with cond=true (violation
//    detected) traps — invoke returns Err(RuntimeError::EncodingError).
//    NOTE: `cond` in RuntimeCheck is the *violation* predicate; a truthy
//    cond means the check failed.
//
//  RUNTIME-SHORTCIRCUITAND-1: ShortCircuitAnd with left=false returns
//    I64(0) without evaluating right — right is an Abort that would trap
//    if reached, proving right is never executed.
//
//  RUNTIME-SHORTCIRCUITAND-2: ShortCircuitAnd with left=true evaluates
//    right (Literal 7) and returns I64(7).
//
//  RUNTIME-SHORTCIRCUITOR-1: ShortCircuitOr with left=true returns I64(1)
//    without evaluating right — right is an Abort that would trap if
//    reached, proving right is never executed.
//
//  RUNTIME-SHORTCIRCUITOR-2: ShortCircuitOr with left=false evaluates
//    right (Literal 7) and returns I64(7).

#[test]
fn continue_in_while_loop_restarts_iteration() {
    let expr = AnfExpr::Let {
        name: "go".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::Let {
            name: "init".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "c".to_string(),
                value: Box::new(AnfExpr::CellNew {
                    init: "init".to_string(),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "one".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    body: Box::new(AnfExpr::Let {
                        name: "three".to_string(),
                        value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                        body: Box::new(AnfExpr::Let {
                            name: "_w".to_string(),
                            value: Box::new(AnfExpr::WhileLoop {
                                cond: "go".to_string(),
                                body: Box::new(AnfExpr::Let {
                                    name: "cur".to_string(),
                                    value: Box::new(AnfExpr::CellGet {
                                        cell: "c".to_string(),
                                    }),
                                    body: Box::new(AnfExpr::Let {
                                        name: "next".to_string(),
                                        value: Box::new(AnfExpr::Call {
                                            func: "+".to_string(),
                                            args: vec!["cur".to_string(), "one".to_string()],
                                        }),
                                        body: Box::new(AnfExpr::Let {
                                            name: "_s".to_string(),
                                            value: Box::new(AnfExpr::CellSet {
                                                cell: "c".to_string(),
                                                value: "next".to_string(),
                                            }),
                                            body: Box::new(AnfExpr::Let {
                                                name: "done_val".to_string(),
                                                value: Box::new(AnfExpr::Call {
                                                    func: "==".to_string(),
                                                    args: vec![
                                                        "next".to_string(),
                                                        "three".to_string(),
                                                    ],
                                                }),
                                                body: Box::new(AnfExpr::If {
                                                    cond: "done_val".to_string(),
                                                    then_branch: Box::new(AnfExpr::Break {
                                                        value: Box::new(AnfExpr::Literal(
                                                            LiteralValue::Unit,
                                                        )),
                                                    }),
                                                    else_branch: Box::new(AnfExpr::Continue),
                                                }),
                                            }),
                                        }),
                                    }),
                                }),
                            }),
                            body: Box::new(AnfExpr::CellGet {
                                cell: "c".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(3),
        "Continue must restart each iteration; counter must reach 3 then Break"
    );
}

// RUNTIME-ABORT-1
//
// fn.main =
//   let _a = Abort { message: "test abort" } in
//   Literal(0)
//
// Abort emits Unreachable, placing the stack in the unreachable (polymorphic)
// state.  The Let binding's LocalSet and Literal(0) are dead code — valid WASM
// because unreachable code is polymorphically accepted.  The outer body
// (Literal(0)) gives the binding a declared I64 return type so it is exported.
// When invoked, Abort fires immediately → trap → RuntimeError::EncodingError.
