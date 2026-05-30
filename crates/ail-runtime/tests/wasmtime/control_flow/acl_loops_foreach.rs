use crate::helpers::*;

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
