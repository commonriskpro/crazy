use super::helpers::*;

#[test]
fn closure_fold_empty_list_returns_init() {
    assert_eq!(
        invoke_closure_fold(10, 7, vec![]),
        RuntimeValue::I64(7),
        "empty-list fold must return init unchanged"
    );
}

// Scenario: single element — one reducer application.
//
// reducer(acc, x) = acc + x + bias  (bias=10, init=0)
// Fold(0, [5], reducer) = 0 + 5 + 10 = 15
#[test]
fn closure_fold_single_element_applies_reducer_once() {
    assert_eq!(
        invoke_closure_fold(10, 0, vec![5]),
        RuntimeValue::I64(15),
        "single-element fold with bias=10 must return 15"
    );
}

// Scenario: multiple elements — reducer applied once per element.
//
// reducer(acc, x) = acc + x + bias  (bias=10, init=0)
// Fold(0, [1, 2, 3], reducer):
//   step1: reducer(0,  1) = 0  + 1  + 10 = 11
//   step2: reducer(11, 2) = 11 + 2  + 10 = 23
//   step3: reducer(23, 3) = 23 + 3  + 10 = 36
#[test]
fn closure_fold_multi_element_accumulates_with_bias() {
    assert_eq!(
        invoke_closure_fold(10, 0, vec![1, 2, 3]),
        RuntimeValue::I64(36),
        "3-element fold with bias=10 must return 36"
    );
}

// Scenario: two-binding module — top-level function as fold reducer.
//
// fn.add_impl: body = Call{"add", ["a", "b"]} — free vars a, b → params (i64,i64).
//              WASM type = (i64, i64) → i64.
//
// fn.sum: Fold { init:"zero", list:"lst", func:"add_impl" }.
//         Emitter path: functions.get("add_impl") → table index 0.
//         call_indirect(fold_reducer_type, table[0]) dispatches to add_impl.
//
// Expected: fold(0, [1,2,3], add_impl) = ((0+1)+2)+3 = 6.
//
// This test isolates the top-level-function dispatch path in the Fold emitter
// (vs. the hoisted-lambda path used in invoke_closure_fold).
#[test]
fn fold_top_level_function_reducer_yields_6() {
    // fn.add_impl: body has free vars a, b → WASM params; returns a+b.
    let add_impl = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.add_impl".to_string(),
        expr: AnfExpr::Call {
            func: "add".to_string(),
            args: vec!["a".to_string(), "b".to_string()],
        },
    };

    // fn.sum: fold(0, [1,2,3], add_impl)
    let sum = AnfBinding {
        source_ref: NodeRef(1),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(1)),
                    AnfExpr::Literal(LiteralValue::Int(2)),
                    AnfExpr::Literal(LiteralValue::Int(3)),
                ])),
                body: Box::new(AnfExpr::Fold {
                    init: "zero".to_string(),
                    list: "lst".to_string(),
                    func: "add_impl".to_string(),
                }),
            }),
        },
    };

    let anf = sealed_anf(vec![add_impl, sum]);
    let wasm = emit_wasm(&anf)
        .expect("two-binding top-level-function fold must compile")
        .wasm;
    let manifest = CapabilityManifest {
        module: "fold-top-level-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("fold-top-level WASM must instantiate");
    assert_eq!(
        instance.invoke("sum", &[]).expect("invoke must succeed"),
        RuntimeValue::I64(6),
        "fold(0, [1,2,3], add_impl) with top-level reducer must return I64(6)"
    );
}

// Scenario: multiple captures — Lambda closes over two independent values.
//
// reducer(acc, x) = let s = acc + x in let t = s + bias1 in t + bias2
// bias1=3, bias2=7 (sum=10), init=0, list=[1, 2]
//   step1: reducer(0, 1)  = 0  + 1 + 3 + 7 = 11
//   step2: reducer(11, 2) = 11 + 2 + 3 + 7 = 23
#[test]
fn closure_fold_multiple_captures_both_loaded_from_env() {
    // fn.main =
    //   let bias1 = 3
    //   let bias2 = 7
    //   let lst   = ListNew([1, 2])
    //   let f     = Lambda(params=[acc,x], captures=[bias1, bias2],
    //                 body = let s = acc+x in let t = s+bias1 in t+bias2)
    //   let zero  = 0
    //   Fold(init=zero, list=lst, func=f)
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "bias1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
            body: Box::new(AnfExpr::Let {
                name: "bias2".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(1)),
                        AnfExpr::Literal(LiteralValue::Int(2)),
                    ])),
                    body: Box::new(AnfExpr::Let {
                        name: "f".to_string(),
                        value: Box::new(AnfExpr::Lambda {
                            params: vec!["acc".to_string(), "x".to_string()],
                            captures: vec!["bias1".to_string(), "bias2".to_string()],
                            body: Box::new(AnfExpr::Let {
                                name: "s".to_string(),
                                value: Box::new(AnfExpr::Call {
                                    func: "+".to_string(),
                                    args: vec!["acc".to_string(), "x".to_string()],
                                }),
                                body: Box::new(AnfExpr::Let {
                                    name: "t".to_string(),
                                    value: Box::new(AnfExpr::Call {
                                        func: "+".to_string(),
                                        args: vec!["s".to_string(), "bias1".to_string()],
                                    }),
                                    body: Box::new(AnfExpr::Call {
                                        func: "+".to_string(),
                                        args: vec!["t".to_string(), "bias2".to_string()],
                                    }),
                                }),
                            }),
                        }),
                        body: Box::new(AnfExpr::Let {
                            name: "zero".to_string(),
                            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                            body: Box::new(AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "f".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        },
    };

    let anf = sealed_anf(vec![binding]);
    let wasm = emit_wasm(&anf)
        .expect("two-capture closure-fold ANF must compile")
        .wasm;
    let manifest = CapabilityManifest {
        module: "closure-fold-two-captures-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("two-capture closure-fold WASM must instantiate");
    let value = instance.invoke("main", &[]).expect("main must invoke");
    assert_eq!(
        value,
        RuntimeValue::I64(23),
        "two-capture fold over [1,2] with bias1=3 bias2=7 must return 23"
    );
}

// ── Wave 17D: VariantNew + Match runtime execution conformance ────────────
//
// Spec scenarios covered (RUNTIME-VARIANT-MATCH-1..4):
//
//  RUNTIME-VARIANT-MATCH-1: single-binding constructor "Ok(x)" extracts the
//    variant payload from linear memory (offset 8) and binds it; the arm body
//    references and uses the bound variable.
//
//  RUNTIME-VARIANT-MATCH-2: tag-only constructor "None" matches by discriminant
//    only — no payload read is attempted (correct for payload-less variants).
//
//  RUNTIME-VARIANT-MATCH-3: wildcard "_" fires when the scrutinee's tag does not
//    match any earlier arm; the wrong-tag arm is fully skipped.
//
//  RUNTIME-VARIANT-MATCH-4: arm ordering is respected — the "None" arm is
//    evaluated first, fails the tag check, and the subsequent "Some(x)" arm
//    correctly extracts the payload.
//
// Design note — arm body shape:
//   Arms that bind a payload variable `x` use `Call { "+", ["x", "x"] }` as
//   the body rather than the bare `Var("x")`.  Both forms now work: the Wave 17D
//   `infer_expr_type` fix temporarily adds the payload binding to `locals` before
//   inferring each arm's body type, so `Var("x")` resolves to `Some(I64)` rather
//   than `None`.  `x + x` is a deliberate proof-of-value choice — the result 42
//   (= 21 + 21) proves both that the correct payload (21) was extracted from
//   linear memory and that the binding is live in the arm body.

// RUNTIME-ACL-FOLD-1
//
// Two-function ACL module:
//
//   fn.add_ints: body = add(a, b)
//     Free variables a, b → WASM params (i64, i64); result i64.
//     Type matches fold-reducer signature `(i64, i64) → i64`.
//     Placed at table[0] in the WASM function table.
//
//   fn.main: body = fold(0, list(1, 2, 3), add_ints)
//     Parsed as CoreExpr::Fold { init:Lit(0), list:ListNew([1,2,3]),
//                                func:Var("add_ints") }.
//     Lowered to AnfExpr::Fold { init:"_t0", list:"_t1", func:"add_ints" }.
//     Emitter path: functions.get("add_ints") → func_idx;
//       table_idx = func_idx − function_offset = 0;
//       call_indirect(fold_reducer_type, table[0]) dispatches to add_ints.
//
// Fold execution:
//   acc = 0
//   acc = add_ints(0, 1) = 1
//   acc = add_ints(1, 2) = 3
//   acc = add_ints(3, 3) = 6
//   Returns I64(6).
//
// Type-check note: add_ints binding signature (type index 0) and the
// fold-reducer type (appended at end) are both (i64, i64) → i64.  Wasmtime 28
// canonicalises structurally identical types so call_indirect passes the type
// check.  If a future engine does not canonicalise, fix by deduplicating the
// type section (emit one shared entry for (i64,i64)→i64 reused by both the
// binding and the fold-reducer slot).
//
// Bug fixed (Wave 22C): optimize.rs `uses_var` previously returned `false`
// for `AnfExpr::Fold { .. }`, so the dead-let pass eliminated the let-bindings
// for `init` and `list` before WASM emit.  The fix makes `uses_var` check all
// three atom fields (init, list, func) so the optimizer retains those bindings.
#[test]
fn acl_fold_named_function_reducer_over_list_123_yields_6() {
    let acl = "\
change acl_fold_1 base=0
author tester
description fold(0, list(1,2,3), add_ints): named function reducer must return 6
op create_function id=fn.add_ints return=Int body=add(a, b)
op create_function id=fn.main return=Int body=fold(0, list(1, 2, 3), add_ints)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(6),
        "fold(0, list(1,2,3), add_ints): named top-level function reducer must return I64(6)"
    );
}

// ── Wave 22B: ACL `index(collection, index)` form ─────────────────────────
//
// Spec scenarios covered (RUNTIME-ACL-INDEX-1, RUNTIME-ACL-INDEX-2):
//
//  RUNTIME-ACL-INDEX-1: ACL body `let(lst, list(5, 10), index(lst, 0))` must
//    parse to CoreExpr::Let { name:"lst", value:ListNew([Lit(5),Lit(10)]),
//    body:IndexGet{collection:Var("lst"),index:Lit(0)} }, lower to ANF,
//    emit WASM, instantiate, and return I64(5).
//    Proves the full pipeline from ACL source → expr_parser → IndexGet →
//    lower_to_anf → wasm_emit → runtime execution without crash.
//
//  RUNTIME-ACL-INDEX-2: ACL body `let(lst, list(5, 10), index(lst, 1))` must
//    return I64(10) — same pipeline, accessing the second element.
//    Together with RUNTIME-ACL-INDEX-1 this proves index arithmetic:
//    element address = ptr + 8 + index * 8.

// RUNTIME-ACL-INDEX-1
//
// ACL body: let(lst, list(5, 10), index(lst, 0))
//
//   Pipeline:
//   1. `list(5, 10)` → parse_expr → CoreExpr::ListNew([Lit(5), Lit(10)])
//   2. `index(lst, 0)` → CoreExpr::IndexGet{collection:Var("lst"),index:Lit(0)}
//   3. `let(lst, <list>, <index>)` → CoreExpr::Let{name:"lst",...}
//   4. lower_to_anf → let _t0=5 in let _t1=10 in let lst=ListNew([_t0,_t1]) in
//      let _t2=0 in IndexGet{collection:"lst", index:"_t2"}
//   5. emit_wasm → IndexGet: addr = lst_ptr + 8 + 0*8 = lst_ptr+8 → 5
//   6. invoke → RuntimeValue::I64(5)
//
// Proves: index(collection, 0) resolves to the first list element end-to-end.
#[test]
fn acl_index_form_first_element_returns_i64_5() {
    let acl = "\
change acl_index_1 base=0
author tester
description let(lst, list(5,10), index(lst,0)): index at 0 must return I64(5)
op create_function id=fn.main return=Int body=let(lst, list(5, 10), index(lst, 0))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(5),
        "let(lst, list(5,10), index(lst,0)) must return I64(5) via IndexGet at offset 8"
    );
}

// RUNTIME-ACL-INDEX-2
//
// ACL body: let(lst, list(5, 10), index(lst, 1))
//
//   Pipeline:
//   1..4. Same as RUNTIME-ACL-INDEX-1 up through ANF lowering.
//   5. emit_wasm → IndexGet: addr = lst_ptr + 8 + 1*8 = lst_ptr+16 → 10
//   6. invoke → RuntimeValue::I64(10)
//
// Proves: index(collection, 1) resolves to the second list element; the
// index * 8 stride arithmetic is correct.
#[test]
fn acl_index_form_second_element_returns_i64_10() {
    let acl = "\
change acl_index_2 base=0
author tester
description let(lst, list(5,10), index(lst,1)): index at 1 must return I64(10)
op create_function id=fn.main return=Int body=let(lst, list(5, 10), index(lst, 1))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(10),
        "let(lst, list(5,10), index(lst,1)) must return I64(10) via IndexGet at offset 16"
    );
}

// RUNTIME-ACL-FOLD-2
//
// ACL body: fold(0, list(1,2,3), lambda(acc, x, add(acc, x)))
//
//   Pipeline:
//   1. `lambda(acc, x, add(acc, x))` → CoreExpr::Lambda{params:["acc","x"],
//      body:CoreExpr::Add(Var("acc"), Var("x"))}
//   2. Fold atomizes init→_t0=0, list→_t1=ListNew([1,2,3]),
//      func→_t2=Lambda{params,captures:[],body:Call{add,[acc,x]}}.
//   3. ANF: Let{_t0=0, Let{_t1=ListNew([1,2,3]), Let{_t2=Lambda{...},
//      Fold{init:_t0,list:_t1,func:_t2}}}}
//   4. emit_wasm Lambda: 2 params, 0 captures → hoistable fold reducer;
//      emits i64.const <table_idx>.  Fold dispatches via call_indirect using
//      fold_reducer_type (i64,i64)→i64.
//   5. Fold execution: acc=0 → add(0,1)=1 → add(1,2)=3 → add(3,3)=6.
//   6. Returns I64(6).
//
// Proves the full ACL source → inline-lambda-as-fold-reducer → runtime path
// for the no-capture (hoistable) Lambda shape.
#[test]
fn acl_fold_inline_lambda_reducer_over_list_123_yields_6() {
    let acl = "\
change acl_fold_2 base=0
author tester
description fold(0, list(1,2,3), lambda(acc,x,add(acc,x))): inline no-capture lambda reducer must return 6
op create_function id=fn.main return=Int body=fold(0, list(1, 2, 3), lambda(acc, x, add(acc, x)))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(6),
        "fold(0, list(1,2,3), lambda(acc,x,add(acc,x))): inline lambda reducer must return I64(6)"
    );
}

// RUNTIME-ACL-FOLD-3
//
// ACL body: let(bias, 10, fold(0, list(1,2), lambda(acc, x, add(add(acc, x), bias))))
//
//   Pipeline:
//   1. Outer let binds bias=10.
//   2. `lambda(acc, x, add(add(acc, x), bias))` → CoreExpr::Lambda{params:["acc","x"],
//      body:CoreExpr::Add(Add(Var("acc"),Var("x")), Var("bias"))}
//   3. Lower lambda body: Add(Add(acc,x), bias) →
//      Let{_t0=Call{add,[acc,x]}, Call{add,[_t0,bias]}}.
//      collect_free_vars with bound=["acc","x"]: "bias" is free → captures=["bias"].
//   4. Fold atomizes: init→_t1=0, list→_t2=ListNew([1,2]),
//      func→_t3=Lambda{params,captures:["bias"],body:Let{...}}.
//   5. ANF (simplified):
//      Let{bias=10,
//        Let{_t1=0,
//          Let{_t2=ListNew([1,2]),
//            Let{_t3=Lambda{params:[acc,x],captures:[bias],body:...},
//              Fold{init:_t1,list:_t2,func:_t3}}}}}
//   6. emit_wasm Lambda: 2 params, 1 capture → closure-hoistable reducer;
//      Lambda node writes closure env to heap: [fn_idx: i64 @ 0, bias: i64 @ 8].
//      Fold loads env_ptr (I32), promotes to I64, and dispatches via
//      call_indirect(closure_reducer_type, table[env.fn_idx]) passing
//      (env_ptr, acc, elem).  Reducer body loads bias from env at offset 8.
//   7. Fold execution with bias=10:
//      acc=0 → add(add(0,1),10) = add(1,10) = 11
//      acc=11 → add(add(11,2),10) = add(13,10) = 23
//   8. Returns I64(23).
//
// Proves the full ACL source → inline-lambda-with-closure → runtime path
// for the capturing Lambda shape, including env write, env read inside the
// loop body, and correct carry of the bias value across all iterations.
#[test]
fn acl_fold_inline_capturing_lambda_with_bias_over_list_12_yields_23() {
    let acl = "\
change acl_fold_3 base=0
author tester
description let(bias,10,fold(0,list(1,2),lambda(acc,x,add(add(acc,x),bias)))): capturing lambda reducer must return 23
op create_function id=fn.main return=Int body=let(bias, 10, fold(0, list(1, 2), lambda(acc, x, add(add(acc, x), bias))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(23),
        "fold with capturing lambda (bias=10) over [1,2]: step1=11 step2=23 → must return I64(23)"
    );
}
