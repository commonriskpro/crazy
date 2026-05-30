use crate::helpers::*;

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
