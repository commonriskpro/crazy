use crate::helpers::*;

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
