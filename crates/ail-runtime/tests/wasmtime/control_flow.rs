use super::helpers::*;

#[test]
fn compiler_if_else_function_returns_taken_branch() {
    let wasm = compiler_wasm_for_expr(
        AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::If {
                cond: "flag".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
            }),
        },
        "fn.branch",
    );
    let manifest = CapabilityManifest {
        module: "compiler-if-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("compiler WASM must instantiate");

    let value = instance.invoke("branch", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(20));
}

#[test]
fn acl_sum_of_squares_body_compiles_and_runs() {
    let acl = "\
change expr_bodies base=0
author tester
description expression bodies
op create_function id=fn.sum_of_squares return=Int
op add_param target=fn.sum_of_squares name=x type=Int
op add_param target=fn.sum_of_squares name=y type=Int
op set_body target=fn.sum_of_squares body=add(mul(x, x), mul(y, y))
op create_function id=fn.main return=Int body=sum_of_squares(3, 4)
end
";

    let value = invoke_acl_export(acl, "main");

    assert_eq!(value, RuntimeValue::I64(25));
}

#[test]
fn acl_let_and_short_circuit_body_compiles_and_runs() {
    let acl = "\
change structured_expr_bodies base=0
author tester
description structured expression bodies
op create_function id=fn.main return=Int body=let(flag, false, and(flag, div(1, 0)))
end
";

    let value = invoke_acl_export(acl, "main");

    assert_eq!(value, RuntimeValue::I64(0));
}

#[test]
fn acl_match_literal_and_wildcard_body_compiles_and_runs() {
    let acl = "\
change match_expr_bodies base=0
author tester
description match expression bodies
op create_function id=fn.literal_hit return=Int body=match(2, 1, 10, 2, 20, _, 30)
op create_function id=fn.wildcard_hit return=Int body=match(9, 1, 10, 2, 20, _, 30)
end
";

    assert_eq!(invoke_acl_export(acl, "literal_hit"), RuntimeValue::I64(20));
    assert_eq!(
        invoke_acl_export(acl, "wildcard_hit"),
        RuntimeValue::I64(30)
    );
}

#[test]
fn compiler_bool_literal_function_returns_i64_boolean() {
    assert_eq!(
        invoke_compiler_expr(AnfExpr::Literal(LiteralValue::Bool(true)), "fn.flag"),
        RuntimeValue::I64(1)
    );
}

#[test]
fn compiler_loop_break_with_value_returns_value() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Loop {
                body: Box::new(AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                }),
            },
            "fn.count_to_ten",
        ),
        RuntimeValue::I64(10)
    );
}

// ── Wave 18D: WhileLoop compile and execution conformance ─────────────────
//
// Spec scenarios covered (RUNTIME-WHILE-1..2):
//
//  RUNTIME-WHILE-1: WhileLoop with an initially-false condition never
//    executes the body.  A cell initialised to 42 must remain 42 after the
//    loop — proves that the condition check fires before the first iteration
//    and that the BrIf(1) exit is taken immediately.
//
//  RUNTIME-WHILE-2: WhileLoop with a true condition runs exactly one
//    iteration: the body decrements a cell from 5 to 4 and then breaks out
//    via Break.  CellGet after the loop must return 4 — proves that (a) the
//    loop body executes, (b) CellSet and CellGet work inside the body, (c)
//    Break branches to the enclosing block's exit, and (d) WhileLoop pushes
//    a unit (I32 0) so it can be used as the value of a Let binding without
//    a WASM stack-underflow error.

// RUNTIME-WHILE-1
//
// fn.main =
//   let init = 42 in
//   let c    = CellNew(init) in
//   let zero = 0 in
//   let flag = false in
//   let _w   = while(flag, CellSet(c, zero)) in   ← body never runs
//   CellGet(c)
//
// Because flag = false (I64 0) the condition check `flag ≠ 0 → 0; eqz → 1`
// triggers BrIf(1) and exits the loop before the body runs.
// CellGet must return the initial value 42.
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
fn seq_empty_produces_unit() {
    assert_eq!(
        invoke_compiler_expr(AnfExpr::Seq(vec![]), "fn.seq_empty"),
        RuntimeValue::I32(0),
        "Empty Seq must produce unit I32(0)"
    );
}

// RUNTIME-SEQ-2
//
// fn.main = Seq([Literal(Unit)])
//
// Single-element Seq: no Drop is emitted (only the last element is kept).
// The element is Unit (I32 0).
#[test]
fn seq_single_element_returns_element_value() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Seq(vec![AnfExpr::Literal(LiteralValue::Unit)]),
            "fn.seq_single"
        ),
        RuntimeValue::I32(0),
        "Single-element Seq([Unit]) must return I32(0)"
    );
}

// RUNTIME-SEQ-3
//
// fn.main =
//   let init = 1       in
//   let c    = CellNew(init) in
//   let v1   = 10      in
//   let v2   = 99      in
//   let _sq  = Seq([CellSet(c, v1), CellSet(c, v2)]) in
//   CellGet(c)
//
// CellSet(c, 10) fires first (effect applied, I32(0) dropped).
// CellSet(c, 99) fires second (effect applied, I32(0) kept as Seq result).
// CellGet must return 99, proving both effects executed in order and that
// only the last value was kept from the Seq.
#[test]
fn seq_multi_element_applies_effects_in_order() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "v1".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                body: Box::new(AnfExpr::Let {
                    name: "v2".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
                    body: Box::new(AnfExpr::Let {
                        name: "_sq".to_string(),
                        value: Box::new(AnfExpr::Seq(vec![
                            AnfExpr::CellSet {
                                cell: "c".to_string(),
                                value: "v1".to_string(),
                            },
                            AnfExpr::CellSet {
                                cell: "c".to_string(),
                                value: "v2".to_string(),
                            },
                        ])),
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
        RuntimeValue::I64(99),
        "Seq([CellSet(c,10), CellSet(c,99)]): both effects run; cell must hold 99"
    );
}

// RUNTIME-RETURN-1
//
// fn.main = Return(42)
//
// Return emits the value and then the WASM `return` instruction, which exits
// the function immediately.  The function's inferred return type is I64
// (from the inner Literal), so the export signature is () → I64.
#[test]
fn return_exits_function_with_value() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Return(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
            "fn.ret"
        ),
        RuntimeValue::I64(42),
        "Return(42) must exit the function with I64(42)"
    );
}

// RUNTIME-RETURN-2
//
// fn.main =
//   let t = true in
//   if t { Return(10) } else { Literal(20) }
//
// t=true → then-branch fires: Return(10) exits the function immediately.
// The else-branch (20) is dead code.  Result must be I64(10).
#[test]
fn return_in_taken_branch_exits_before_else() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::If {
            cond: "t".to_string(),
            then_branch: Box::new(AnfExpr::Return(Box::new(AnfExpr::Literal(
                LiteralValue::Int(10),
            )))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.ret_if"),
        RuntimeValue::I64(10),
        "Return in taken then-branch must exit with 10; else (20) must not be reached"
    );
}

// RUNTIME-RETURN-3
//
// fn.main = Seq([Return(Unit), Abort("unreachable — Return above must exit")])
//
// Return(Unit) emits I32Const(0) + WASM `return`, which exits the function
// immediately.  The second Seq element (Abort → WASM `unreachable`) is dead
// code and is never reached at runtime.
//
// If Return did NOT emit the WASM `return` instruction, the Abort would fire
// and the invocation would return Err(EncodingError) rather than Ok(I32(0)).
// Receiving I32(0) without a trap is the proof that Return causes a genuine
// early exit before any subsequent statement in the same Seq executes.
//
// Type note: Seq always infers I32 as its result type; Return(Unit) → I32(0)
// matches that type exactly, so the generated WASM function is well-typed.
#[test]
fn return_in_seq_before_abort_proves_early_exit() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Seq(vec![
                AnfExpr::Return(Box::new(AnfExpr::Literal(LiteralValue::Unit))),
                AnfExpr::Abort {
                    message: "unreachable — Return above must exit the function".to_string(),
                },
            ]),
            "fn.ret_early"
        ),
        RuntimeValue::I32(0),
        "Return in Seq must exit before Abort; I32(0) without trap proves early exit"
    );
}

// RUNTIME-CONTINUE-1
//
// fn.main =
//   let go    = true  in                       ← WhileLoop condition (always truthy)
//   let init  = 0     in
//   let c     = CellNew(init) in
//   let one   = 1     in
//   let three = 3     in
//   let _w    = while(go,
//                 let cur      = CellGet(c)      in
//                 let next     = cur + one        in
//                 let _s       = CellSet(c, next) in
//                 let done_val = (next == three)  in
//                 if done_val { Break(unit) } else { Continue }
//               ) in
//   CellGet(c)
//
// Iterations (Continue fires on 1st and 2nd, Break on 3rd):
//   iter 1: cur=0, next=1, _s→c=1, done_val=0 (1≠3) → Continue
//   iter 2: cur=1, next=2, _s→c=2, done_val=0 (2≠3) → Continue
//   iter 3: cur=2, next=3, _s→c=3, done_val=1 (3==3) → Break(unit)
// CellGet must return 3 — proves Continue restarts the iteration without
// skipping the CellSet side-effect, and Break terminates the loop correctly.
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
#[test]
fn abort_always_traps() {
    let expr = AnfExpr::Let {
        name: "_a".to_string(),
        value: Box::new(AnfExpr::Abort {
            message: "test abort".to_string(),
        }),
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
    };
    let result = try_invoke_compiler_expr(expr, "fn.abort");
    assert!(
        matches!(result, Err(RuntimeError::EncodingError(_))),
        "Abort must trap and return EncodingError, got {result:?}"
    );
}

// RUNTIME-ASSUME-1
//
// Two-binding ANF:
//   fn.assume_note = Assume { predicate: "x > 0", reason: "test assumption" }
//   fn.main        = Literal(42)
//
// Assume emits NO WASM instructions (pure compile-time annotation).
// Its binding is NOT exported (binding_result = None — by design) but it IS
// compiled and validated as part of the module.
// fn.main IS exported and returns I64(42), proving the module compiles and
// instantiates correctly even when a sibling binding contains Assume.
// This demonstrates Assume's zero runtime cost: no trap, no interference.
#[test]
fn assume_has_no_runtime_effect() {
    use ail_core::semantic_graph::NodeRef;

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.assume_note".to_string(),
            expr: AnfExpr::Assume {
                predicate: "x > 0".to_string(),
                reason: "test assumption".to_string(),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(42)),
        },
    ]);
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "assume-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("module with Assume binding must instantiate");

    let value = instance.invoke("main", &[]).expect("fn.main must invoke");
    assert_eq!(
        value,
        RuntimeValue::I64(42),
        "fn.main must return I64(42); Assume in sibling binding must not interfere"
    );
}

// ── RuntimeCheck execution conformance ────────────────────────────────────
//
// DESIGN NOTE: The ail-compiler intentionally does NOT export functions whose
// top-level expression is a RuntimeCheck (binding_result = None).  This is
// a tested invariant (see C-3b in wasm_tests.rs).  To test the EXECUTION
// of the RuntimeCheck pattern we construct the equivalent WASM bytecode
// directly using wasm_encoder, bypassing the compiler.
//
// The RuntimeCheck WASM pattern emitted by ail-compiler for
//   RuntimeCheck { cond, .. }
// is exactly:
//   emit_condition_get(cond)   ; I32 on stack
//   If(BlockType::Empty)
//     Unreachable
//   End
//
// We replicate this pattern manually with a hardcoded I32 condition so the
// function can be exported and invoked.  This proves the execution semantics
// of the pattern, complementing the structural (wasmparser) proofs in
// wasm_tests.rs.

#[test]
fn runtime_check_false_cond_does_not_trap() {
    let wasm = runtime_check_pattern_wasm(0); // condition = false
    let mut instance = instantiate_test_wasm(&wasm);
    let value = instance.invoke("check", &[]).expect("check must not trap");
    assert_eq!(
        value,
        RuntimeValue::I32(42),
        "RuntimeCheck with false condition must not trap; must return I32(42)"
    );
}

// RUNTIME-RUNTIMECHECK-2
//
// RuntimeCheck pattern with condition=1 (true / violation detected).
//
// The If guard IS taken → Unreachable fires → Wasmtime trap →
// RuntimeError::EncodingError.
// Proves: when the violation predicate is true, RuntimeCheck traps.
// NOTE: `cond` in RuntimeCheck is the *violation* predicate — truthy means
// "check failed", not "assertion holds".
#[test]
fn runtime_check_true_cond_traps() {
    let wasm = runtime_check_pattern_wasm(1); // condition = true
    let mut instance = instantiate_test_wasm(&wasm);
    let result = instance.invoke("check", &[]);
    assert!(
        matches!(result, Err(RuntimeError::EncodingError(_))),
        "RuntimeCheck with true condition must trap with EncodingError, got {result:?}"
    );
}

// RUNTIME-SHORTCIRCUITAND-1
//
// fn.main =
//   let f = false in
//   ShortCircuitAnd { left: "f", right: Abort{"dead code"} }
//
// left=false → else branch → I64(0); right (Abort) is NEVER evaluated.
// If short-circuit were broken and right were reached, Abort would trap.
// No trap proves right was not evaluated.
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

#[test]
fn acl_while_false_condition_body_never_runs() {
    let acl = "\
change acl_while_1 base=0
author tester
description while(flag=false, body) must skip body and return unit I32(0)
op create_function id=fn.main return=Int body=let(flag, false, while(flag, 42))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I32(0),
        "let(flag, false, while(flag, 42)) must return I32(0) without running the body"
    );
}

// RUNTIME-ACL-WHILE-2
//
// ACL body (multi-let):
//   let(zero, 0,
//     let(c, cell_new(zero),
//       let(one, 1,
//         let(go, true,
//           let(_w, while(go, let(_s, cell_set(c, one), break(go))),
//             cell_get(c)
//           )
//         )
//       )
//     )
//   )
//
//   Pipeline:
//   1. zero=0, c=CellNew(0), one=1, go=true (I64 1).
//   2. WhileLoop: go=truthy → enter body.
//      Body: _s=CellSet(c, one=1) — writes I64(1) into cell; break(go) → Br(1).
//   3. WhileLoop exits via break; pushes I32Const(0) as _w.
//   4. CellGet(c) → I64(1).
//
// Proves: ACL while body executes exactly once; CellSet persists through
// linear memory; CellGet reads back the written value; break exits the loop.
// All sub-expression arguments are pre-bound Vars — no atomized binding is lost.
#[test]
fn acl_while_body_runs_once_and_mutates_cell() {
    let acl = "\
change acl_while_2 base=0
author tester
description while body runs once: CellSet writes 1 to cell, CellGet reads 1
op create_function id=fn.main return=Int body=let(zero, 0, let(c, cell_new(zero), let(one, 1, let(go, true, let(_w, while(go, let(_s, cell_set(c, one), break(go))), cell_get(c))))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "while body must run once: CellSet(c,1) then break → CellGet(c) must return I64(1)"
    );
}

// RUNTIME-ACL-WHILE-3 (Wave 20A)
//
// ACL body:
//   let(x, 0,
//     let(c, cell_new(x),
//       let(_w, while(lt(x, 3),
//         let(_s, cell_set(c, 99),
//           break(x)
//         )
//       ),
//       cell_get(c)
//     )
//   )
//
//   Pipeline:
//   1. x=0 (I64 0), c=CellNew(x) — cell initialised to 0.
//   2. Condition: lower_core_expr_to_anf_local emits
//      Let { anf_0 = 3; Let { anf_1 = lt(x, anf_0); WhileLoop{cond:anf_1, ...} } }.
//      anf_1 = lt(0, 3) = I64(1) → truthy → loop body enters.
//   3. Body: anf_2 = 99; CellSet(c, anf_2); break(x) → Br(1) exits loop.
//   4. WhileLoop → I32Const(0) as _w.
//   5. CellGet(c) → I64(99).
//
// Regression: without the WhileLoop arm in lower_core_expr_to_anf_local, the
// `_` fallthrough discards the binding for `anf_1`; emit_condition_get falls
// back to I32Const(0) (condition always false); loop never runs; CellGet → 0.
#[test]
fn acl_while_computed_lt_condition_body_runs() {
    let acl = "\
change acl_while_3 base=0
author tester
description while(lt(x,3)) with x=0: condition is computed, body must run and set cell to 99
op create_function id=fn.main return=Int body=let(x, 0, let(c, cell_new(x), let(_w, while(lt(x, 3), let(_s, cell_set(c, 99), break(x))), cell_get(c))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(99),
        "while(lt(x,3)) with x=0: body must run once, CellSet(c,99), CellGet must return I64(99)"
    );
}

// RUNTIME-ACL-WHILE-4 (Wave 20A)
//
// ACL body:
//   let(zero, 0,
//     let(c, cell_new(zero),
//       let(_w, while(eq(cell_get(c), zero),
//         let(seven, 7,
//           let(_s, cell_set(c, seven),
//             break(zero)
//           )
//         )
//       ),
//       cell_get(c)
//     )
//   )
//
//   Pipeline:
//   1. zero=0, c=CellNew(0) — cell initialised to 0.
//   2. Condition: lower_core_expr_to_anf_local emits
//      Let { anf_N = CellGet("c"); Let { anf_M = eq(anf_N, zero);
//            Let { anf_K = anf_M; WhileLoop{cond:anf_K, ...} } } }.
//      anf_M = eq(0, 0) = I64(1) → truthy → loop body enters.
//   3. Body: seven=7; CellSet(c, seven); break(zero) → Br(1) exits loop.
//   4. WhileLoop → I32Const(0) as _w.
//   5. CellGet(c) → I64(7).
//
// Regression: exercises WhileLoop + CellGet atomization fix together.
// Without fix: condition binding lost → loop never runs → CellGet → I64(0).
#[test]
fn acl_while_computed_cell_get_eq_condition_body_runs() {
    let acl = "\
change acl_while_4 base=0
author tester
description while(eq(cell_get(c),zero)) with c=0: computed cell condition, body must run and set cell to 7
op create_function id=fn.main return=Int body=let(zero, 0, let(c, cell_new(zero), let(_w, while(eq(cell_get(c), zero), let(seven, 7, let(_s, cell_set(c, seven), break(zero)))), cell_get(c))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(7),
        "while(eq(cell_get(c),zero)) with c=0: body must run once, CellSet(c,7), CellGet must return I64(7)"
    );
}

// ── Wave 21A: computed WhileLoop condition re-evaluation ──────────────────
//
// Spec scenarios covered (RUNTIME-ACL-WHILE-5, RUNTIME-ACL-WHILE-6):
//
//  RUNTIME-ACL-WHILE-5 (Wave 21A): while(lt(cell_get(c), 3), body) where the
//    condition is a computed expression re-evaluated on every iteration.
//    The body increments the cell each iteration; the loop must run exactly 3
//    times (cell 0→1→2→3) and terminate when the condition becomes false.
//    cell_get(c) must return I64(3).
//
//    Before the fix, `atomize_local` hoisted the condition outside the loop:
//    `anf_cond = lt(cell_get(c), 3)` was evaluated once (= true) and reused
//    every iteration — the loop would never terminate.
//    After the fix, the desugaring places the condition inside the Loop body
//    as a Let binding re-evaluated on each iteration.
//
//  RUNTIME-ACL-WHILE-6 (Wave 21A): while(lt(cell_get(c), 1), body) with the
//    initial cell value already satisfying the exit condition (cell = 1 and
//    1 < 1 = false).  The body must never run; cell_get(c) must return I64(1).
//    Proves the desugared Loop evaluates the condition before the first
//    iteration and exits immediately when it is false.

// RUNTIME-ACL-WHILE-5
//
// ACL body:
//   let(init, 0,
//     let(c, cell_new(init),
//       let(one, 1,
//         let(three, 3,
//           let(_w, while(lt(cell_get(c), three),
//             let(cur, cell_get(c),
//               let(next, add(cur, one),
//                 cell_set(c, next)
//               )
//             )
//           ),
//           cell_get(c)
//         )
//       )
//     )
//   )
//
// Pipeline:
// 1. init=0, c=CellNew(0), one=1, three=3.
// 2. WhileLoop desugared to Loop { Let { cond = lt(cell_get(c), three),
//    If { cond_tmp → then: (body; Continue), else: Break(unit) } } };
//    Literal(unit) unit sentinel after loop.
// 3. Iteration 1: cond = lt(0,3)=true → body: cur=0, next=1, CellSet(c,1); Continue.
//    Iteration 2: cond = lt(1,3)=true → body: cur=1, next=2, CellSet(c,2); Continue.
//    Iteration 3: cond = lt(2,3)=true → body: cur=2, next=3, CellSet(c,3); Continue.
//    Iteration 4: cond = lt(3,3)=false → Break(unit) exits loop.
// 4. cell_get(c) → I64(3).
//
// Without the Wave 21A fix: cond = lt(0,3) evaluated once = true; loop never
// exits; the test would hang indefinitely.
#[test]
fn acl_while_computed_condition_reruns_each_iteration_reaches_3() {
    let acl = "\
change acl_while_5 base=0
author tester
description while(lt(cell_get(c),3)): condition re-evaluated each iteration, loop runs 3 times, cell reaches 3
op create_function id=fn.main return=Int body=let(init, 0, let(c, cell_new(init), let(one, 1, let(three, 3, let(_w, while(lt(cell_get(c), three), let(cur, cell_get(c), let(next, add(cur, one), cell_set(c, next)))), cell_get(c))))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(3),
        "while(lt(cell_get(c),3)) must re-evaluate condition each iteration: loop runs 3 times, cell must reach I64(3)"
    );
}

// RUNTIME-ACL-WHILE-6
//
// ACL body:
//   let(one, 1,
//     let(c, cell_new(one),
//       let(_w, while(lt(cell_get(c), one),
//         let(zero, 0,
//           cell_set(c, zero)   ← body never runs: initial cell = 1 ≥ 1
//         )
//       ),
//       cell_get(c)
//     )
//   )
//
// Pipeline:
// 1. one=1, c=CellNew(1) — cell initialised to 1.
// 2. Condition: lt(cell_get(c), one) = lt(1, 1) = false → Break(unit) fires immediately.
//    Body (cell_set) never runs.
// 3. cell_get(c) → I64(1).
//
// Proves the desugared Loop evaluates the condition before the first
// iteration and exits immediately when false — matching the semantics
// of a standard while loop.
#[test]
fn acl_while_computed_condition_false_at_start_body_never_runs() {
    let acl = "\
change acl_while_6 base=0
author tester
description while(lt(cell_get(c),1)) with c=1: condition false at start, body never runs, cell stays 1
op create_function id=fn.main return=Int body=let(one, 1, let(c, cell_new(one), let(_w, while(lt(cell_get(c), one), let(zero, 0, cell_set(c, zero))), cell_get(c))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "while(lt(cell_get(c),1)) with c=1: condition false at start, body must not run, cell_get must return I64(1)"
    );
}

// ── Wave 22C: ACL source-level E2E tests for iteration ───────────────────
//
// Spec scenarios covered (RUNTIME-ACL-FOREACH-1, RUNTIME-ACL-WHILE-7,
// RUNTIME-ACL-FOLD-1):
//
//  RUNTIME-ACL-FOREACH-1 (Wave 22C): ACL body `foreach(x, list(1,2,3), body)`
//    where `body` accumulates `x` into a mutable cell.
//    Pipeline: foreach → CoreExpr::ForEach → AnfExpr::ForEach → WASM inline
//    loop.  After 3 iterations (x=1,2,3) the cell must reach I64(6).
//    Proves the full ACL source → runtime path for ForEach accumulation.
//
//  RUNTIME-ACL-WHILE-7 (Wave 22C): ACL body `while(lt(cell_get(c), 3), body)`
//    where the condition uses integer literals 3 and 1 DIRECTLY (no named
//    variables `three`/`one`).  The cell must reach I64(3).
//    Proves Wave 21A desugaring handles inline integer literals atomised fresh
//    inside the Loop body on every iteration — re-evaluating `cell_get(c)`
//    each time.  Complementary to RUNTIME-ACL-WHILE-5 (which uses named vars).
//
//  RUNTIME-ACL-FOLD-1 (Wave 22C): Two-function ACL module:
//    `fn.add_ints` body `add(a, b)` → free-variable params a,b → WASM type
//    `(i64, i64) → i64`, matching the fold-reducer type.
//    `fn.main` body `fold(0, list(1, 2, 3), add_ints)` → fold dispatches via
//    `call_indirect` at table[0] (top-level-function path in the emitter).
//    Expected result: I64(6).  Proves named function reference as fold reducer
//    at ACL source level.

// RUNTIME-ACL-FOREACH-1
//
// ACL body:
//   let(init, 0,
//     let(c, cell_new(init),
//       let(lst, list(1, 2, 3),
//         let(_fe,
//           foreach(x, lst,
//             let(cur, cell_get(c),
//               let(next, add(cur, x),
//                 cell_set(c, next)
//               )
//             )
//           ),
//           cell_get(c)
//         )
//       )
//     )
//   )
//
// Pipeline:
// 1. init=0, c=CellNew(0), lst=ListNew([1,2,3]).
// 2. ForEach emits an inline WASM loop over lst:
//    Iteration x=1: cur=cell_get(c)=0, next=add(0,1)=1, CellSet(c,1).
//    Iteration x=2: cur=cell_get(c)=1, next=add(1,2)=3, CellSet(c,3).
//    Iteration x=3: cur=cell_get(c)=3, next=add(3,3)=6, CellSet(c,6).
//    ForEach produces unit (I32 0) so it can appear as the value of let(_fe).
// 3. cell_get(c) → I64(6).
//
// Proves: foreach ACL form → ForEach ANF → WASM loop → cell accumulation.
#[test]
fn acl_foreach_cell_accumulator_over_list_123_yields_6() {
    let acl = "\
change acl_foreach_1 base=0
author tester
description foreach(x, list(1,2,3), cell-accumulate x): cell must reach 6
op create_function id=fn.main return=Int body=let(init, 0, let(c, cell_new(init), let(lst, list(1, 2, 3), let(_fe, foreach(x, lst, let(cur, cell_get(c), let(next, add(cur, x), cell_set(c, next)))), cell_get(c)))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(6),
        "foreach(x, list(1,2,3), cell-accumulate): cell must reach I64(6)"
    );
}

// RUNTIME-ACL-WHILE-7
//
// ACL body:
//   let(init, 0,
//     let(c, cell_new(init),
//       let(_w,
//         while(lt(cell_get(c), 3),
//           let(cur, cell_get(c),
//             let(next, add(cur, 1),
//               cell_set(c, next)
//             )
//           )
//         ),
//         cell_get(c)
//       )
//     )
//   )
//
// Pipeline:
// 1. init=0, c=CellNew(0).
// 2. WhileLoop desugared (Wave 21A) to Loop { Let { cond_expr } }.
//    Integer literals 3 and 1 are atomised as fresh locals INSIDE the Loop
//    body on each iteration — proving the desugar path handles inline literals.
//    Iteration 1: cond=lt(cell_get(c)=0, 3)=true → cur=0, next=1, cell=1; Continue.
//    Iteration 2: cond=lt(cell_get(c)=1, 3)=true → cur=1, next=2, cell=2; Continue.
//    Iteration 3: cond=lt(cell_get(c)=2, 3)=true → cur=2, next=3, cell=3; Continue.
//    Iteration 4: cond=lt(cell_get(c)=3, 3)=false → Break(unit) exits loop.
// 3. cell_get(c) → I64(3).
//
// Distinguishes from RUNTIME-ACL-WHILE-5: uses `3` and `1` directly in the
// condition/body (no `let(three, 3, ...)` wrapper), exercising the literal
// atomisation path inside the desugared Loop body.
#[test]
fn acl_while_computed_lt_inline_literals_increments_to_3() {
    let acl = "\
change acl_while_7 base=0
author tester
description while(lt(cell_get(c),3)) inline literals: condition re-evaluated each iteration, cell reaches 3
op create_function id=fn.main return=Int body=let(init, 0, let(c, cell_new(init), let(_w, while(lt(cell_get(c), 3), let(cur, cell_get(c), let(next, add(cur, 1), cell_set(c, next)))), cell_get(c))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(3),
        "while(lt(cell_get(c),3)) with inline literals: condition re-evaluated each iteration; cell must reach I64(3)"
    );
}
