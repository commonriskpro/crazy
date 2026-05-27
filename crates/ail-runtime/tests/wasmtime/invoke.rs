use super::helpers::*;

#[test]
fn exported_i64_function_can_be_invoked() {
    let mut module = wasm_encoder::Module::new();
    let mut types = wasm_encoder::TypeSection::new();
    types.ty().function([], [wasm_encoder::ValType::I64]);
    module.section(&types);
    let mut functions = wasm_encoder::FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("answer", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);
    let mut codes = wasm_encoder::CodeSection::new();
    let mut function = wasm_encoder::Function::new([]);
    function.instruction(&wasm_encoder::Instruction::I64Const(42));
    function.instruction(&wasm_encoder::Instruction::End);
    codes.function(&function);
    module.section(&codes);
    let wasm = module.finish();

    let manifest = CapabilityManifest {
        module: "invoke-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let value = instance.invoke("answer", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_zero_args_still_works() {
    let wasm = sum_wasm(0);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance.invoke("sum", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_one_arg() {
    let wasm = sum_wasm(1);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance
        .invoke("sum", &[RuntimeArg::I64(42)])
        .expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_two_args() {
    let wasm = sum_wasm(2);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance
        .invoke("sum", &[RuntimeArg::I64(20), RuntimeArg::I64(22)])
        .expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_three_args() {
    let wasm = sum_wasm(3);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance
        .invoke(
            "sum",
            &[
                RuntimeArg::I64(10),
                RuntimeArg::I64(12),
                RuntimeArg::I64(20),
            ],
        )
        .expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn compiler_function_call_double_21_invokes_to_42() {
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.double".to_string(),
            expr: AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["x".to_string(), "x".to_string()],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Let {
                name: "n".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(21))),
                body: Box::new(AnfExpr::Call {
                    func: "double".to_string(),
                    args: vec!["n".to_string()],
                }),
            },
        },
    ]);
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "function-call-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let value = instance.invoke("main", &[]).expect("main must invoke");
    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn compiler_function_with_param_invokes_with_runtime_arg() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.double".to_string(),
        expr: AnfExpr::Call {
            func: "i64.add".to_string(),
            args: vec!["x".to_string(), "x".to_string()],
        },
    }]);
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "param-function-call-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let value = instance
        .invoke("double", &[RuntimeArg::I64(21)])
        .expect("double must invoke");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn compiler_arithmetic_ops_invoke_with_i64_results() {
    let cases = [
        ("add", binary_i64_call("i64.add", 3, 4), 7),
        ("mul", binary_i64_call("i64.mul", 6, 7), 42),
        ("sub", binary_i64_call("i64.sub", 50, 8), 42),
        ("div", binary_i64_call("i64.div_s", 84, 2), 42),
        ("mod", binary_i64_call("i64.rem_s", 85, 43), 42),
    ];

    for (name, expr, expected) in cases {
        assert_eq!(
            invoke_compiler_expr(expr, &format!("fn.{name}")),
            RuntimeValue::I64(expected),
            "{name} should evaluate to {expected}"
        );
    }
}

#[test]
fn compiler_comparison_ops_invoke_with_i64_boolean_results() {
    let cases = [
        ("eq", binary_i64_call("i64.eq", 42, 42), 1),
        ("lt", binary_i64_call("i64.lt_s", 3, 5), 1),
        ("ne", binary_i64_call("i64.ne", 42, 7), 1),
        ("le", binary_i64_call("i64.le_s", 42, 42), 1),
        ("gt", binary_i64_call("i64.gt_s", 9, 5), 1),
        ("ge", binary_i64_call("i64.ge_s", 42, 42), 1),
    ];

    for (name, expr, expected) in cases {
        assert_eq!(
            invoke_compiler_expr(expr, &format!("fn.{name}")),
            RuntimeValue::I64(expected),
            "{name} should evaluate to {expected}"
        );
    }
}

#[test]
fn compiler_unary_ops_invoke_with_i64_results() {
    let neg = AnfExpr::Let {
        name: "x".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(-42))),
        body: Box::new(AnfExpr::Call {
            func: "i64.neg".to_string(),
            args: vec!["x".to_string()],
        }),
    };
    let not = AnfExpr::Let {
        name: "x".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::Call {
            func: "i64.eqz".to_string(),
            args: vec!["x".to_string()],
        }),
    };

    assert_eq!(invoke_compiler_expr(neg, "fn.neg"), RuntimeValue::I64(42));
    assert_eq!(invoke_compiler_expr(not, "fn.not"), RuntimeValue::I64(1));
}

// TRIANGULATE: the 8-byte WASM header (minimal valid module) also succeeds.

// ── Wave 24D: ACL source-level E2E tests for boolean short-circuit ─────────
//
// Spec scenarios covered (RUNTIME-ACL-AND-1, RUNTIME-ACL-AND-2,
//                          RUNTIME-ACL-OR-1,  RUNTIME-ACL-OR-2):
//
//  RUNTIME-ACL-AND-1: ACL body `and(true, false)` must atomize the `true`
//    literal, lower to ShortCircuitAnd{left:_t0=true, right:Literal(false)},
//    evaluate the left operand (I64 1 — truthy), evaluate the right operand
//    (I64 0 — the boolean representation of false), and return I64(0).
//    Proves the full pipeline for `and` when both operands are evaluated.
//
//  RUNTIME-ACL-AND-2: ACL body `and(false, abort("dead"))` must lower to
//    ShortCircuitAnd{left:_t0=false, right:Abort{"dead"}}.  With left=false
//    (I64 0 — falsy) the WASM else-branch returns I64(0) immediately; the
//    right-hand Abort (WASM `unreachable`) is NEVER reached.  The absence of
//    a trap proves short-circuit semantics are preserved end-to-end from ACL
//    source through the expr_parser, lower, and WASM emit stages.
//
//  RUNTIME-ACL-OR-1: ACL body `or(true, abort("dead"))` must lower to
//    ShortCircuitOr{left:_t0=true, right:Abort{"dead"}}.  With left=true
//    (I64 1 — truthy) the WASM then-branch returns I64(1) immediately; the
//    right-hand Abort is NEVER reached.  The absence of a trap proves
//    short-circuit semantics for `or`.
//
//  RUNTIME-ACL-OR-2: ACL body `or(false, true)` must atomize the `false`
//    literal, lower to ShortCircuitOr{left:_t0=false, right:Literal(true)},
//    evaluate both operands (left=I64(0) falsy → evaluate right=I64(1) truthy),
//    and return I64(1).  Proves the full pipeline for `or` when both
//    operands are evaluated.
//
// Boolean representation convention (from compiler_bool_literal_function_returns_i64_boolean):
//   true  → I64(1)    false → I64(0)
//
// The `abort("dead")` ACL form is handled by the `abort` case added to
// expr_parser.rs: `abort("msg")` → CoreExpr::Abort{message} →
// AnfExpr::Abort{message} → WASM `unreachable`.

// RUNTIME-ACL-AND-1
//
// ACL body: and(true, false)
//
//   Pipeline:
//   1. `and(true, false)` → CoreExpr::And{left:Lit(Bool(true)),
//      right:Lit(Bool(false))}
//   2. lower → let _t0 = Literal(Bool(true)) in
//      ShortCircuitAnd{left:"_t0", right:Literal(Bool(false))}
//   3. WASM emit ShortCircuitAnd: emit_condition_get("_t0") → I64(1) truthy;
//      If-then: emit right (Literal(Bool(false)) = I64(0)); If-else: I64(0).
//      Since left=I64(1) (truthy), then-branch fires → right evaluated = I64(0).
//   4. Returns I64(0).
//
// Proves: and(true, false) evaluates both operands and returns I64(0) (false).
#[test]
fn acl_and_true_false_evaluates_right_returns_false() {
    let acl = "\
change acl_and_1 base=0
author tester
description and(true, false): left=true so right is evaluated; result must be I64(0)
op create_function id=fn.main return=Int body=and(true, false)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "and(true, false) must evaluate right (false) and return I64(0)"
    );
}

// RUNTIME-ACL-AND-2
//
// ACL body: and(false, abort("dead"))
//
//   Pipeline:
//   1. `and(false, abort("dead"))` → CoreExpr::And{left:Lit(Bool(false)),
//      right:CoreExpr::Abort{message:"dead"}}
//   2. lower → let _t0 = Literal(Bool(false)) in
//      ShortCircuitAnd{left:"_t0", right:AnfExpr::Abort{message:"dead"}}
//   3. WASM emit ShortCircuitAnd: emit_condition_get("_t0") → I64(0) falsy;
//      If-then: emit right (Abort → `unreachable`); If-else: I64Const(0).
//      Since left=I64(0) (falsy), else-branch fires → I64(0) returned; Abort
//      (`unreachable`) is DEAD CODE and is NEVER reached at runtime.
//   4. Returns I64(0) without trapping.
//
// Diagnostic: if short-circuit were broken (right always evaluated), the Abort
// would fire → Wasmtime trap → invoke returns Err(EncodingError) → test panics
// at .expect("invoke must succeed") with the trap message.
#[test]
fn acl_and_false_left_short_circuits_abort_not_reached() {
    let acl = r#"
change acl_and_2 base=0
author tester
description and(false, abort("dead")): left=false so right (abort) must not be evaluated
op create_function id=fn.main return=Int body=and(false, abort("dead"))
end
"#;
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "and(false, abort(\"dead\")) must short-circuit: right never evaluated; must return I64(0) without trapping"
    );
}

// RUNTIME-ACL-OR-1
//
// ACL body: or(true, abort("dead"))
//
//   Pipeline:
//   1. `or(true, abort("dead"))` → CoreExpr::Or{left:Lit(Bool(true)),
//      right:CoreExpr::Abort{message:"dead"}}
//   2. lower → let _t0 = Literal(Bool(true)) in
//      ShortCircuitOr{left:"_t0", right:AnfExpr::Abort{message:"dead"}}
//   3. WASM emit ShortCircuitOr: emit_condition_get("_t0") → I64(1) truthy;
//      If-then: I64Const(1); If-else: emit right (Abort → `unreachable`).
//      Since left=I64(1) (truthy), then-branch fires → I64(1) returned; Abort
//      (`unreachable`) is DEAD CODE and is NEVER reached at runtime.
//   4. Returns I64(1) without trapping.
//
// Diagnostic: if short-circuit were broken (right always evaluated), the Abort
// would fire → Wasmtime trap → invoke returns Err(EncodingError) → test panics
// at .expect("invoke must succeed") with the trap message.
#[test]
fn acl_or_true_left_short_circuits_abort_not_reached() {
    let acl = r#"
change acl_or_1 base=0
author tester
description or(true, abort("dead")): left=true so right (abort) must not be evaluated
op create_function id=fn.main return=Int body=or(true, abort("dead"))
end
"#;
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "or(true, abort(\"dead\")) must short-circuit: right never evaluated; must return I64(1) without trapping"
    );
}

// RUNTIME-ACL-OR-2
//
// ACL body: or(false, true)
//
//   Pipeline:
//   1. `or(false, true)` → CoreExpr::Or{left:Lit(Bool(false)),
//      right:Lit(Bool(true))}
//   2. lower → let _t0 = Literal(Bool(false)) in
//      ShortCircuitOr{left:"_t0", right:Literal(Bool(true))}
//   3. WASM emit ShortCircuitOr: emit_condition_get("_t0") → I64(0) falsy;
//      If-then: I64Const(1); If-else: emit right (Literal(Bool(true)) = I64(1)).
//      Since left=I64(0) (falsy), else-branch fires → right evaluated = I64(1).
//   4. Returns I64(1).
//
// Proves: or(false, true) evaluates both operands and returns I64(1) (true).
#[test]
fn acl_or_false_true_evaluates_right_returns_true() {
    let acl = "\
change acl_or_2 base=0
author tester
description or(false, true): left=false so right is evaluated; result must be I64(1)
op create_function id=fn.main return=Int body=or(false, true)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "or(false, true) must evaluate right (true) and return I64(1)"
    );
}

// ── not / mod conformance ─────────────────────────────────────────────────
//
// Spec scenarios covered (RUNTIME-ACL-NOT-1, RUNTIME-ACL-NOT-2,
//                          RUNTIME-ACL-NOT-3,
//                          RUNTIME-ACL-MOD-1,  RUNTIME-ACL-MOD-2,
//                          RUNTIME-ACL-MOD-3,  RUNTIME-ACL-MOD-4):
//
//  RUNTIME-ACL-NOT-1: ACL body `not(true)` lowers through
//    CoreExpr::Not → lower_core_unary_to_anf("not") → ANF Call "not" with 1
//    arg → WASM I64Eqz + I64ExtendI32U.
//    true = I64(1) → Eqz → i32(0) → ExtendI32U → I64(0).
//    Returns I64(0).
//
//  RUNTIME-ACL-NOT-2: ACL body `not(false)` follows the same path.
//    false = I64(0) → Eqz → i32(1) → ExtendI32U → I64(1).
//    Returns I64(1).
//
//  RUNTIME-ACL-NOT-3: ACL body `not(eq(1,2))`.
//    eq(1,2) lowers to I64Eq → i32(0) → ExtendI32U → I64(0) (false).
//    not(I64(0)) → Eqz → i32(1) → ExtendI32U → I64(1).
//    Proves not() composes correctly with a nested comparison.
//
//  RUNTIME-ACL-MOD-1: ACL body `mod(10,3)` lowers through
//    CoreExpr::Mod → lower_core_binary_to_anf("mod") → ANF Call "mod" with 2
//    args → WASM I64RemS.
//    10 rem_s 3 = 1 → I64(1).
//
//  RUNTIME-ACL-MOD-2: ACL body `mod(10,2)`.
//    10 rem_s 2 = 0 → I64(0).
//    Proves exact divisibility returns I64(0).
//
//  RUNTIME-ACL-MOD-3: signed remainder with negative dividend.
//    Helper fn `signed_mod(a: Int, b: Int) = mod(a, b)` exposes `a` and `b`
//    as function parameters (local variables in WASM), ensuring the I64RemS
//    instruction executes `local.get a; local.get b; i64.rem_s` rather than a
//    folded I64Const.  `main` calls `signed_mod(sub(0,10), 3)`.
//    i64.rem_s(-10, 3): trunc(-10/3)=-3; remainder=-10-(-3*3)=-10+9=-1.
//    Returns I64(-1).
//
//  RUNTIME-ACL-MOD-4: signed remainder with negative divisor.
//    Same helper pattern; `main` calls `signed_mod(10, sub(0,3))`.
//    i64.rem_s(10, -3): sign of result = sign of dividend (+10).
//    trunc(10/-3)=-3; remainder=10-(-3*-3)=10-9=1.
//    Returns I64(1).
//    WASM i64.rem_s sign follows the *dividend*, not the divisor —
//    mod(10, -3) == 1, NOT -1.  Only a negative dividend yields a
//    negative remainder (cf. MOD-3 where mod(-10, 3) == -1).
//
// These tests exercise the full pipeline from ACL source:
//   parse_changeset → canonicalize → apply → lower_to_core_ir →
//   lower_to_anf → emit_wasm → Wasmtime → RuntimeValue.
//
// Encoding:
//   true → I64(1)    false → I64(0)

// RUNTIME-ACL-NOT-1
//
// ACL body: not(true)
//
//   Pipeline:
//   1. `not(true)` → CoreExpr::Not(Literal(Bool(true)))
//   2. lower → let _t0 = Literal(Bool(true)) in Call "not" [_t0]
//   3. WASM: LocalGet(_t0) [= I64(1)]; I64Eqz → i32(0); I64ExtendI32U → I64(0).
//   4. Returns I64(0).
//
// Proves: not(true) → I64(0) (logical negation of true is false).
#[test]
fn acl_not_true_returns_false() {
    let acl = "\
change acl_not_1 base=0
author tester
description not(true): logical negation of true must return I64(0)
op create_function id=fn.main return=Int body=not(true)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "not(true) must return I64(0)"
    );
}

// RUNTIME-ACL-NOT-2
//
// ACL body: not(false)
//
//   Pipeline:
//   1. `not(false)` → CoreExpr::Not(Literal(Bool(false)))
//   2. lower → let _t0 = Literal(Bool(false)) in Call "not" [_t0]
//   3. WASM: LocalGet(_t0) [= I64(0)]; I64Eqz → i32(1); I64ExtendI32U → I64(1).
//   4. Returns I64(1).
//
// Proves: not(false) → I64(1) (logical negation of false is true).
#[test]
fn acl_not_false_returns_true() {
    let acl = "\
change acl_not_2 base=0
author tester
description not(false): logical negation of false must return I64(1)
op create_function id=fn.main return=Int body=not(false)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "not(false) must return I64(1)"
    );
}

// RUNTIME-ACL-NOT-3
//
// ACL body: not(eq(1,2))
//
//   Pipeline:
//   1. `not(eq(1,2))` → CoreExpr::Not(CoreExpr::Eq(Lit(1), Lit(2)))
//   2. lower → let _t0 = 1 in let _t1 = 2 in let _t2 = call "eq" [_t0,_t1]
//      in call "not" [_t2]
//   3. WASM: eq(1,2) → I64Eq → i32(0) → I64ExtendI32U → I64(0);
//      not(I64(0)) → I64Eqz → i32(1) → I64ExtendI32U → I64(1).
//   4. Returns I64(1).
//
// Proves: not() composes correctly with a nested sub-expression (1≠2 → not false → true).
#[test]
fn acl_not_eq_1_2_returns_true() {
    let acl = "\
change acl_not_3 base=0
author tester
description not(eq(1,2)): eq(1,2)=false so not(false) must return I64(1)
op create_function id=fn.main return=Int body=not(eq(1,2))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "not(eq(1,2)) must return I64(1): eq(1,2) is false, not(false) is true"
    );
}

// RUNTIME-ACL-MOD-1
//
// ACL body: mod(10,3)
//
//   Pipeline:
//   1. `mod(10,3)` → CoreExpr::Mod(Lit(10), Lit(3))
//   2. lower → let _t0 = 10 in let _t1 = 3 in Call "mod" [_t0, _t1]
//   3. WASM: I64RemS(10, 3) = 1 → I64(1).
//   4. Returns I64(1).
//
// Proves: mod(10,3) → I64(1) (10 rem 3 = 1).
#[test]
fn acl_mod_10_3_returns_1() {
    let acl = "\
change acl_mod_1 base=0
author tester
description mod(10,3): 10 rem 3 must return I64(1)
op create_function id=fn.main return=Int body=mod(10,3)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "mod(10,3) must return I64(1)"
    );
}

// RUNTIME-ACL-MOD-2
//
// ACL body: mod(10,2)
//
//   Pipeline:
//   1. `mod(10,2)` → CoreExpr::Mod(Lit(10), Lit(2))
//   2. lower → let _t0 = 10 in let _t1 = 2 in Call "mod" [_t0, _t1]
//   3. WASM: I64RemS(10, 2) = 0 → I64(0).
//   4. Returns I64(0).
//
// Proves: mod(10,2) → I64(0) (exact divisibility yields zero remainder).
#[test]
fn acl_mod_10_2_returns_0() {
    let acl = "\
change acl_mod_2 base=0
author tester
description mod(10,2): 10 rem 2 must return I64(0)
op create_function id=fn.main return=Int body=mod(10,2)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "mod(10,2) must return I64(0): 10 is exactly divisible by 2"
    );
}

// RUNTIME-ACL-MOD-3
//
// ACL body: signed_mod(sub(0,10), 3) where signed_mod(a,b) = mod(a,b)
//
//   Strategy: `a` and `b` are function parameters, which lower to WASM local
//   variables.  The `mod(a, b)` body therefore emits
//     local.get a; local.get b; i64.rem_s
//   rather than a constant-folded I64Const — exercising real WASM codegen.
//   `main` passes `sub(0, 10)` as the dividend; the optimizer may fold the
//   sub, but the rem_s in `signed_mod` always operates on locals.
//
//   Pipeline:
//   1. `mod(a, b)` → CoreExpr::Mod(Var "a", Var "b")
//   2. lower → Call "mod" [a, b]  (locals, not consts)
//   3. WASM: I64RemS(local.get a, local.get b).
//   4. dividend = -10, divisor = 3.
//      trunc(-10 / 3) = -3; remainder = -10 - (-3 * 3) = -10 + 9 = -1.
//   5. Returns I64(-1).
//
// Proves: mod() with a negative dividend returns a negative remainder under
//   WASM i64.rem_s (truncation-toward-zero, sign follows dividend).
#[test]
fn acl_mod_neg10_3_returns_neg1() {
    let acl = "\
change acl_mod_3 base=0
author tester
description mod(-10,3): negative dividend signed remainder must return I64(-1)
op create_function id=fn.signed_mod return=Int
op add_param target=fn.signed_mod name=a type=Int
op add_param target=fn.signed_mod name=b type=Int
op set_body target=fn.signed_mod body=mod(a, b)
op create_function id=fn.main return=Int body=signed_mod(sub(0, 10), 3)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(-1),
        "mod(-10, 3) must return I64(-1): signed remainder sign follows dividend"
    );
}

// RUNTIME-ACL-MOD-4
//
// ACL body: signed_mod(10, sub(0,3)) where signed_mod(a,b) = mod(a,b)
//
//   Strategy: same as MOD-3 — `a` and `b` are function parameters (WASM
//   locals), so `mod(a, b)` emits local.get a; local.get b; i64.rem_s.
//   `main` passes `sub(0, 3)` as the divisor to exercise the negative-divisor
//   path.
//
//   Pipeline:
//   1. `mod(a, b)` → CoreExpr::Mod(Var "a", Var "b")
//   2. lower → Call "mod" [a, b]  (locals)
//   3. WASM: I64RemS(local.get a, local.get b).
//   4. dividend = 10, divisor = -3.
//      trunc(10 / -3) = -3; remainder = 10 - (-3 * -3) = 10 - 9 = 1.
//   5. Returns I64(1).
//
// Proves: mod() with a negative divisor returns a positive remainder under
//   WASM i64.rem_s — the sign of the result follows the *dividend* (+10),
//   not the divisor (-3).  This is C/WASM truncation-toward-zero semantics.
//   Key invariant: mod(10, -3) == 1, NOT -1; the negative divisor does NOT
//   flip the sign.  Compare with MOD-3: mod(-10, 3) == -1 (negative dividend).
#[test]
fn acl_mod_10_neg3_returns_1() {
    let acl = "\
change acl_mod_4 base=0
author tester
description mod(10,-3): negative divisor signed remainder must return I64(1)
op create_function id=fn.signed_mod return=Int
op add_param target=fn.signed_mod name=a type=Int
op add_param target=fn.signed_mod name=b type=Int
op set_body target=fn.signed_mod body=mod(a, b)
op create_function id=fn.main return=Int body=signed_mod(10, sub(0, 3))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "mod(10, -3) must return I64(1): i64.rem_s sign follows dividend (+10), not divisor (-3)"
    );
}

// ── Wave 27D: sub / div / gt / ne / le / ge arithmetic and comparison ──────
//
// Spec scenarios covered (RUNTIME-ACL-SUB-1, RUNTIME-ACL-DIV-1,
//                          RUNTIME-ACL-GT-1,  RUNTIME-ACL-GT-2,
//                          RUNTIME-ACL-NE-1,  RUNTIME-ACL-NE-2,
//                          RUNTIME-ACL-LE-1,  RUNTIME-ACL-LE-2,
//                          RUNTIME-ACL-GE-1,  RUNTIME-ACL-GE-2):
//
//  RUNTIME-ACL-SUB-1: ACL body `sub(10,3)` lowers through
//    CoreExpr::Sub → lower_core_binary_to_anf("sub") → ANF Call "sub" →
//    WASM I64Sub.  10 − 3 = 7 → I64(7).
//
//  RUNTIME-ACL-DIV-1: ACL body `div(84,2)` lowers through
//    CoreExpr::Div → lower_core_binary_to_anf("div") → ANF Call "div" →
//    WASM I64DivS.  84 / 2 = 42 → I64(42).
//
//  RUNTIME-ACL-GT-1: ACL body `gt(5,3)` lowers through
//    CoreExpr::Gt → lower_core_binary_to_anf("gt") → ANF Call "gt" →
//    WASM I64GtS + I64ExtendI32U.  5 > 3 = true → I64(1).
//
//  RUNTIME-ACL-GT-2: ACL body `gt(3,5)`.  3 > 5 = false → I64(0).
//    Proves the false branch of the gt codegen path.
//
//  RUNTIME-ACL-NE-1: ACL body `ne(42,7)` lowers through
//    CoreExpr::Ne → lower_core_binary_to_anf("ne") → ANF Call "ne" →
//    WASM I64Ne + I64ExtendI32U.  42 ≠ 7 = true → I64(1).
//
//  RUNTIME-ACL-NE-2: ACL body `ne(42,42)`.  42 ≠ 42 = false → I64(0).
//    Proves the false branch of the ne codegen path.
//
//  RUNTIME-ACL-LE-1: ACL body `le(42,42)` lowers through
//    CoreExpr::Le → lower_core_binary_to_anf("le") → ANF Call "le" →
//    WASM I64LeS + I64ExtendI32U.  42 ≤ 42 = true → I64(1).
//
//  RUNTIME-ACL-LE-2: ACL body `le(5,3)`.  5 ≤ 3 = false → I64(0).
//    Proves the false branch of the le codegen path.
//
//  RUNTIME-ACL-GE-1: ACL body `ge(42,42)` lowers through
//    CoreExpr::Ge → lower_core_binary_to_anf("ge") → ANF Call "ge" →
//    WASM I64GeS + I64ExtendI32U.  42 ≥ 42 = true → I64(1).
//
//  RUNTIME-ACL-GE-2: ACL body `ge(3,5)`.  3 ≥ 5 = false → I64(0).
//    Proves the false branch of the ge codegen path.
//
// These tests exercise the full pipeline from ACL source:
//   parse_changeset → canonicalize → apply → lower_to_core_ir →
//   lower_to_anf → emit_wasm → Wasmtime → RuntimeValue.
//
// Boolean representation convention: true → I64(1), false → I64(0).

// RUNTIME-ACL-SUB-1
//
// ACL body: sub(10,3)
//
//   Pipeline:
//   1. `sub(10,3)` → CoreExpr::Sub(Lit(10), Lit(3))
//   2. lower → let _t0 = 10 in let _t1 = 3 in Call "sub" [_t0, _t1]
//   3. WASM: I64Sub(10, 3) = 7 → I64(7).
//   4. Returns I64(7).
//
// Proves: sub(10,3) → I64(7) (integer subtraction end-to-end).
#[test]
fn acl_sub_10_3_returns_7() {
    let acl = "\
change acl_sub_1 base=0
author tester
description sub(10,3): 10 - 3 must return I64(7)
op create_function id=fn.main return=Int body=sub(10,3)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(7),
        "sub(10,3) must return I64(7)"
    );
}

// RUNTIME-ACL-DIV-1
//
// ACL body: div(84,2)
//
//   Pipeline:
//   1. `div(84,2)` → CoreExpr::Div(Lit(84), Lit(2))
//   2. lower → let _t0 = 84 in let _t1 = 2 in Call "div" [_t0, _t1]
//   3. WASM: I64DivS(84, 2) = 42 → I64(42).
//   4. Returns I64(42).
//
// Proves: div(84,2) → I64(42) (signed integer division end-to-end).
#[test]
fn acl_div_84_2_returns_42() {
    let acl = "\
change acl_div_1 base=0
author tester
description div(84,2): 84 / 2 must return I64(42)
op create_function id=fn.main return=Int body=div(84,2)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(42),
        "div(84,2) must return I64(42)"
    );
}

// RUNTIME-ACL-GT-1
//
// ACL body: gt(5,3)
//
//   Pipeline:
//   1. `gt(5,3)` → CoreExpr::Gt(Lit(5), Lit(3))
//   2. lower → let _t0 = 5 in let _t1 = 3 in Call "gt" [_t0, _t1]
//   3. WASM: I64GtS(5, 3) → i32(1) → I64ExtendI32U → I64(1).
//   4. Returns I64(1).
//
// Proves: gt(5,3) → I64(1) (5 > 3 is true).
#[test]
fn acl_gt_5_3_returns_1() {
    let acl = "\
change acl_gt_1 base=0
author tester
description gt(5,3): 5 > 3 must return I64(1)
op create_function id=fn.main return=Int body=gt(5,3)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "gt(5,3) must return I64(1)"
    );
}

// RUNTIME-ACL-NE-1
//
// ACL body: ne(42,7)
//
//   Pipeline:
//   1. `ne(42,7)` → CoreExpr::Ne(Lit(42), Lit(7))
//   2. lower → let _t0 = 42 in let _t1 = 7 in Call "ne" [_t0, _t1]
//   3. WASM: I64Ne(42, 7) → i32(1) → I64ExtendI32U → I64(1).
//   4. Returns I64(1).
//
// Proves: ne(42,7) → I64(1) (42 ≠ 7 is true).
#[test]
fn acl_ne_42_7_returns_1() {
    let acl = "\
change acl_ne_1 base=0
author tester
description ne(42,7): 42 != 7 must return I64(1)
op create_function id=fn.main return=Int body=ne(42,7)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "ne(42,7) must return I64(1)"
    );
}

// RUNTIME-ACL-LE-1
//
// ACL body: le(42,42)
//
//   Pipeline:
//   1. `le(42,42)` → CoreExpr::Le(Lit(42), Lit(42))
//   2. lower → let _t0 = 42 in let _t1 = 42 in Call "le" [_t0, _t1]
//   3. WASM: I64LeS(42, 42) → i32(1) → I64ExtendI32U → I64(1).
//   4. Returns I64(1).
//
// Proves: le(42,42) → I64(1) (42 ≤ 42 is true — equal values satisfy ≤).
#[test]
fn acl_le_42_42_returns_1() {
    let acl = "\
change acl_le_1 base=0
author tester
description le(42,42): 42 <= 42 must return I64(1)
op create_function id=fn.main return=Int body=le(42,42)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "le(42,42) must return I64(1)"
    );
}

// RUNTIME-ACL-GE-1
//
// ACL body: ge(42,42)
//
//   Pipeline:
//   1. `ge(42,42)` → CoreExpr::Ge(Lit(42), Lit(42))
//   2. lower → let _t0 = 42 in let _t1 = 42 in Call "ge" [_t0, _t1]
//   3. WASM: I64GeS(42, 42) → i32(1) → I64ExtendI32U → I64(1).
//   4. Returns I64(1).
//
// Proves: ge(42,42) → I64(1) (42 ≥ 42 is true — equal values satisfy ≥).
#[test]
fn acl_ge_42_42_returns_1() {
    let acl = "\
change acl_ge_1 base=0
author tester
description ge(42,42): 42 >= 42 must return I64(1)
op create_function id=fn.main return=Int body=ge(42,42)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "ge(42,42) must return I64(1)"
    );
}

// RUNTIME-ACL-GT-2
//
// ACL body: gt(3,5)
//
//   Pipeline:
//   1. `gt(3,5)` → CoreExpr::Gt(Lit(3), Lit(5))
//   2. lower → let _t0 = 3 in let _t1 = 5 in Call "gt" [_t0, _t1]
//   3. WASM: I64GtS(3, 5) → i32(0) → I64ExtendI32U → I64(0).
//   4. Returns I64(0).
//
// Proves: gt(3,5) → I64(0) (3 > 5 is false — triangulates the false branch).
#[test]
fn acl_gt_3_5_returns_0() {
    let acl = "\
change acl_gt_2 base=0
author tester
description gt(3,5): 3 > 5 must return I64(0)
op create_function id=fn.main return=Int body=gt(3,5)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "gt(3,5) must return I64(0)"
    );
}

// RUNTIME-ACL-NE-2
//
// ACL body: ne(42,42)
//
//   Pipeline:
//   1. `ne(42,42)` → CoreExpr::Ne(Lit(42), Lit(42))
//   2. lower → let _t0 = 42 in let _t1 = 42 in Call "ne" [_t0, _t1]
//   3. WASM: I64Ne(42, 42) → i32(0) → I64ExtendI32U → I64(0).
//   4. Returns I64(0).
//
// Proves: ne(42,42) → I64(0) (equal values are not unequal — false branch).
#[test]
fn acl_ne_42_42_returns_0() {
    let acl = "\
change acl_ne_2 base=0
author tester
description ne(42,42): 42 != 42 must return I64(0)
op create_function id=fn.main return=Int body=ne(42,42)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "ne(42,42) must return I64(0)"
    );
}

// RUNTIME-ACL-LE-2
//
// ACL body: le(5,3)
//
//   Pipeline:
//   1. `le(5,3)` → CoreExpr::Le(Lit(5), Lit(3))
//   2. lower → let _t0 = 5 in let _t1 = 3 in Call "le" [_t0, _t1]
//   3. WASM: I64LeS(5, 3) → i32(0) → I64ExtendI32U → I64(0).
//   4. Returns I64(0).
//
// Proves: le(5,3) → I64(0) (5 ≤ 3 is false — triangulates the false branch).
#[test]
fn acl_le_5_3_returns_0() {
    let acl = "\
change acl_le_2 base=0
author tester
description le(5,3): 5 <= 3 must return I64(0)
op create_function id=fn.main return=Int body=le(5,3)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "le(5,3) must return I64(0)"
    );
}

// RUNTIME-ACL-GE-2
//
// ACL body: ge(3,5)
//
//   Pipeline:
//   1. `ge(3,5)` → CoreExpr::Ge(Lit(3), Lit(5))
//   2. lower → let _t0 = 3 in let _t1 = 5 in Call "ge" [_t0, _t1]
//   3. WASM: I64GeS(3, 5) → i32(0) → I64ExtendI32U → I64(0).
//   4. Returns I64(0).
//
// Proves: ge(3,5) → I64(0) (3 ≥ 5 is false — triangulates the false branch).
#[test]
fn acl_ge_3_5_returns_0() {
    let acl = "\
change acl_ge_2 base=0
author tester
description ge(3,5): 3 >= 5 must return I64(0)
op create_function id=fn.main return=Int body=ge(3,5)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(0),
        "ge(3,5) must return I64(0)"
    );
}
