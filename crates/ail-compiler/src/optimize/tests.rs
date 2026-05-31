use ail_core::semantic_graph::NodeRef;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;

use super::*;

fn binding(expr: AnfExpr) -> AnfBinding {
    AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr,
    }
}

#[test]
fn constant_folding_rewrites_integer_primitive_calls() {
    let optimized = optimize_bindings(vec![binding(AnfExpr::Let {
        name: "a".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
        body: Box::new(AnfExpr::Let {
            name: "b".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(22))),
            body: Box::new(AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["a".to_string(), "b".to_string()],
            }),
        }),
    })]);

    assert_eq!(optimized[0].expr, AnfExpr::Literal(LiteralValue::Int(42)));
}

#[test]
fn dead_code_elimination_removes_unused_pure_lets() {
    let optimized = optimize_bindings(vec![binding(AnfExpr::Let {
        name: "unused".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
    })]);

    assert_eq!(optimized[0].expr, AnfExpr::Literal(LiteralValue::Int(2)));
}

// ── eliminate_dead_pure ───────────────────────────────────────────────

#[test]
fn eliminate_dead_pure_removes_pure_non_final_seq_element() {
    // Seq: [pure_expr, effect_call]
    // The first (pure) element should be removed; the effect call kept.
    let seq = AnfExpr::Seq(vec![
        AnfExpr::Literal(LiteralValue::Int(42)), // pure — dead
        AnfExpr::EffectCall {
            capability: "clock".to_string(),
            func: "now".to_string(),
            args: vec![],
        }, // effectful — kept
    ]);
    let result = eliminate_dead_pure(vec![binding(seq)]);
    // After elimination the Seq collapses to the single EffectCall.
    assert_eq!(
        result[0].expr,
        AnfExpr::EffectCall {
            capability: "clock".to_string(),
            func: "now".to_string(),
            args: vec![],
        }
    );
}

#[test]
fn eliminate_dead_pure_keeps_all_effects_in_seq() {
    // Seq: [effect1, effect2]  — both effectful, neither removed.
    let seq = AnfExpr::Seq(vec![
        AnfExpr::EffectCall {
            capability: "db".to_string(),
            func: "write".to_string(),
            args: vec![],
        },
        AnfExpr::EffectCall {
            capability: "log".to_string(),
            func: "info".to_string(),
            args: vec![],
        },
    ]);
    let input = seq.clone();
    let result = eliminate_dead_pure(vec![binding(seq)]);
    assert_eq!(
        result[0].expr, input,
        "both effectful — seq must be unchanged"
    );
}

#[test]
fn eliminate_dead_pure_empty_seq_becomes_unit() {
    // A Seq with a single pure element that is NOT the final element
    // (edge case: Seq with one element total is the final element, kept).
    // Instead test the degenerate case where all non-final elements are pure
    // and the final element is also pure — nothing to drop but the seq collapses.
    let seq = AnfExpr::Seq(vec![AnfExpr::Literal(LiteralValue::Int(1))]);
    let result = eliminate_dead_pure(vec![binding(seq)]);
    // Single-element Seq collapses to the element.
    assert_eq!(result[0].expr, AnfExpr::Literal(LiteralValue::Int(1)));
}

// ── inline_small_pure ─────────────────────────────────────────────────

#[test]
fn inline_small_pure_inlines_single_arg_lambda() {
    // Binding A: fn.double = Lambda { params: ["x"], body: Call("mul", ["x", "two"]) }
    // Binding B: fn.main  = Let { a = Literal(2); b = Call("fn.double", ["a"]); b }
    // After inline: b = Call("mul", ["a", "two"])
    let lambda_binding = AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(0),
        name: "fn.double".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Call {
                func: "mul".to_string(),
                args: vec!["x".to_string(), "two".to_string()],
            }),
        },
    };
    let main_binding = AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(1),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "a".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
            body: Box::new(AnfExpr::Let {
                name: "b".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "fn.double".to_string(),
                    args: vec!["a".to_string()],
                }),
                body: Box::new(AnfExpr::Var("b".to_string())),
            }),
        },
    };

    let result = inline_small_pure(vec![lambda_binding, main_binding]);
    // The call to fn.double should be replaced by mul(a, two)
    let expected_b_value = AnfExpr::Call {
        func: "mul".to_string(),
        args: vec!["a".to_string(), "two".to_string()],
    };
    if let AnfExpr::Let { body, .. } = &result[1].expr {
        if let AnfExpr::Let { value, .. } = body.as_ref() {
            assert_eq!(
                value.as_ref(),
                &expected_b_value,
                "call to fn.double must be inlined"
            );
        } else {
            panic!("expected inner Let");
        }
    } else {
        panic!("expected outer Let");
    }
}

#[test]
fn inline_small_pure_does_not_inline_large_lambda() {
    // A lambda with 4+ nodes must NOT be inlined.
    let large_body = AnfExpr::Let {
        name: "t1".to_string(),
        value: Box::new(AnfExpr::Call {
            func: "add".to_string(),
            args: vec!["x".to_string(), "y".to_string()],
        }),
        body: Box::new(AnfExpr::Let {
            name: "t2".to_string(),
            value: Box::new(AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["t1".to_string(), "z".to_string()],
            }),
            body: Box::new(AnfExpr::Var("t2".to_string())),
        }),
    }; // 7 nodes — over the 3-node limit

    let lambda_binding = AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(0),
        name: "fn.big".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string(), "y".to_string(), "z".to_string()],
            captures: vec![],
            body: Box::new(large_body),
        },
    };
    let call_binding = binding(AnfExpr::Call {
        func: "fn.big".to_string(),
        args: vec!["a".to_string(), "b".to_string(), "c".to_string()],
    });

    let result = inline_small_pure(vec![lambda_binding, call_binding]);
    // Call must remain unchanged.
    assert_eq!(
        result[1].expr,
        AnfExpr::Call {
            func: "fn.big".to_string(),
            args: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        }
    );
}

// ── cse_bindings ──────────────────────────────────────────────────────

#[test]
fn cse_replaces_duplicate_pure_call_with_var_reference() {
    // let a = add(x, y) in
    // let b = add(x, y) in   ← same as a — should become: let b = a
    // pair(a, b)
    let expr = AnfExpr::Let {
        name: "a".to_string(),
        value: Box::new(AnfExpr::Call {
            func: "add".to_string(),
            args: vec!["x".to_string(), "y".to_string()],
        }),
        body: Box::new(AnfExpr::Let {
            name: "b".to_string(),
            value: Box::new(AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["x".to_string(), "y".to_string()],
            }),
            body: Box::new(AnfExpr::Call {
                func: "pair".to_string(),
                args: vec!["a".to_string(), "b".to_string()],
            }),
        }),
    };

    let result = cse_bindings(vec![binding(expr)]);

    // The value of the second Let should now be Var("a").
    if let AnfExpr::Let { body, .. } = &result[0].expr {
        if let AnfExpr::Let { value, .. } = body.as_ref() {
            assert_eq!(
                value.as_ref(),
                &AnfExpr::Var("a".to_string()),
                "duplicate pure expression must be aliased to first occurrence"
            );
        } else {
            panic!("expected inner Let");
        }
    } else {
        panic!("expected outer Let");
    }
}

#[test]
fn cse_does_not_share_across_if_branches() {
    // let cond = true in
    // if cond {
    //   let a = add(x, y) in a
    // } else {
    //   let b = add(x, y) in b
    // }
    // The two `add(x, y)` expressions are in separate branches — no CSE.
    let branch_expr = |name: &str| AnfExpr::Let {
        name: name.to_string(),
        value: Box::new(AnfExpr::Call {
            func: "add".to_string(),
            args: vec!["x".to_string(), "y".to_string()],
        }),
        body: Box::new(AnfExpr::Var(name.to_string())),
    };
    let expr = AnfExpr::Let {
        name: "cond".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::If {
            cond: "cond".to_string(),
            then_branch: Box::new(branch_expr("a")),
            else_branch: Box::new(branch_expr("b")),
        }),
    };

    let result = cse_bindings(vec![binding(expr)]);

    // Both branches should keep their original add(x, y) — not CSE'd.
    if let AnfExpr::Let { body, .. } = &result[0].expr {
        if let AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } = body.as_ref()
        {
            let expected_call = AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["x".to_string(), "y".to_string()],
            };
            if let AnfExpr::Let { value: tv, .. } = then_branch.as_ref() {
                assert_eq!(tv.as_ref(), &expected_call, "then branch must not be CSE'd");
            }
            if let AnfExpr::Let { value: ev, .. } = else_branch.as_ref() {
                assert_eq!(ev.as_ref(), &expected_call, "else branch must not be CSE'd");
            }
        } else {
            panic!("expected If");
        }
    } else {
        panic!("expected outer Let");
    }
}

// W2: optimize_expr prunes stale captures when constant-folding removes a
// captured var reference from the lambda body.
#[test]
fn optimize_expr_prunes_stale_captures_after_constant_folding() {
    // Lambda captures ["x"], but the body is `let _a = x in 42`.
    // After constant-folding removes the unused let, body becomes Literal(42)
    // and "x" is no longer free — captures must be empty after optimization.
    let lambda = AnfExpr::Lambda {
        params: vec![],
        captures: vec!["x".to_string()],
        body: Box::new(AnfExpr::Let {
            name: "_a".to_string(),
            value: Box::new(AnfExpr::Var("x".to_string())),
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        }),
    };
    let result = optimize_bindings(vec![binding(lambda)]);
    if let AnfExpr::Lambda { captures, .. } = &result[0].expr {
        assert!(
            captures.is_empty(),
            "stale capture 'x' must be pruned after constant-folding removes its use"
        );
    } else {
        panic!("expected Lambda");
    }
}

// W2: inline_calls_in_expr prunes stale captures when inlining replaces a
// call that used a captured arg with a literal body that does not.
#[test]
fn inline_calls_prunes_stale_captures_after_inlining() {
    // fn.const = Lambda { params: ["_v"], body: Literal(0) }
    // outer   = Lambda { params: [], captures: ["x"],
    //                    body: Call("fn.const", ["x"]) }
    // After inlining fn.const the body becomes Literal(0); "x" no longer free.
    let const_binding = AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(0),
        name: "fn.const".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["_v".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        },
    };
    let outer_binding = AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(1),
        name: "fn.outer".to_string(),
        expr: AnfExpr::Lambda {
            params: vec![],
            captures: vec!["x".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "fn.const".to_string(),
                args: vec!["x".to_string()],
            }),
        },
    };
    let result = inline_small_pure(vec![const_binding, outer_binding]);
    if let AnfExpr::Lambda { captures, .. } = &result[1].expr {
        assert!(
            captures.is_empty(),
            "stale capture 'x' must be pruned after fn.const is inlined away"
        );
    } else {
        panic!("expected Lambda");
    }
}

#[test]
fn dead_code_elimination_keeps_effects() {
    let expr = AnfExpr::Let {
        name: "unused".to_string(),
        value: Box::new(AnfExpr::EffectCall {
            capability: "clock.now".to_string(),
            func: "now".to_string(),
            args: vec![],
        }),
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
    };
    let optimized = optimize_bindings(vec![binding(expr.clone())]);

    assert_eq!(optimized[0].expr, expr);
}

// OPT-FOLD-DCE-1: dead-let elimination must NOT remove let bindings whose
// names appear only in AnfExpr::Fold atom fields (init, list, func).
//
// All three let values are pure Literals, so the DCE predicate
// `is_pure(value) && !uses_var(body, name)` can only reach the correct
// answer if `uses_var` returns `true` for each name via the Fold arm.
// A missing or incomplete Fold arm would silently eliminate the binding.
#[test]
fn dead_let_retains_bindings_referenced_by_fold_atoms() {
    // Build:
    //   let acc0    = Literal(0) in
    //   let lst     = Literal(0) in
    //   let reducer = Literal(0) in
    //   Fold { init: "acc0", list: "lst", func: "reducer" }
    //
    // None of the three names appear in any Var/Call/FieldGet — the Fold
    // atom fields are the sole references.  DCE must retain all three lets.
    let expr = AnfExpr::Let {
        name: "acc0".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::Let {
            name: "lst".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Fold {
                    init: "acc0".to_string(),
                    list: "lst".to_string(),
                    func: "reducer".to_string(),
                }),
            }),
        }),
    };

    let result = optimize_bindings(vec![binding(expr)]);

    let AnfExpr::Let {
        name: ref n1,
        body: ref b1,
        ..
    } = result[0].expr
    else {
        panic!("expected outer Let (acc0); got {:?}", result[0].expr);
    };
    assert_eq!(n1, "acc0", "outer Let must bind 'acc0'");

    let AnfExpr::Let {
        name: ref n2,
        body: ref b2,
        ..
    } = **b1
    else {
        panic!("expected middle Let (lst); got {:?}", b1);
    };
    assert_eq!(n2, "lst", "middle Let must bind 'lst'");

    let AnfExpr::Let {
        name: ref n3,
        body: ref b3,
        ..
    } = **b2
    else {
        panic!("expected inner Let (reducer); got {:?}", b2);
    };
    assert_eq!(n3, "reducer", "inner Let must bind 'reducer'");

    assert_eq!(
        b3.as_ref(),
        &AnfExpr::Fold {
            init: "acc0".to_string(),
            list: "lst".to_string(),
            func: "reducer".to_string(),
        },
        "body must be the Fold node with all three atom references intact"
    );
}

// ── uses_var: ShortCircuitAnd / ShortCircuitOr ────────────────────────

// OPT-USESVAR-AND-1: uses_var returns true when the queried name equals
// the left operand atom of ShortCircuitAnd.
#[test]
fn uses_var_short_circuit_and_true_for_left_name() {
    let expr = AnfExpr::ShortCircuitAnd {
        left: "x".to_string(),
        right: Box::new(AnfExpr::Var("y".to_string())),
    };
    assert!(
        uses_var(&expr, "x"),
        "uses_var must return true when name matches ShortCircuitAnd.left"
    );
}

// OPT-USESVAR-AND-2: uses_var returns false when the queried name does not
// appear in either the left atom or the right sub-expression.
#[test]
fn uses_var_short_circuit_and_false_for_unrelated_name() {
    let expr = AnfExpr::ShortCircuitAnd {
        left: "x".to_string(),
        right: Box::new(AnfExpr::Var("y".to_string())),
    };
    assert!(
        !uses_var(&expr, "z"),
        "uses_var must return false when name is absent from ShortCircuitAnd"
    );
}

// OPT-USESVAR-OR-1: uses_var returns true when the queried name equals
// the left operand atom of ShortCircuitOr.
#[test]
fn uses_var_short_circuit_or_true_for_left_name() {
    let expr = AnfExpr::ShortCircuitOr {
        left: "flag".to_string(),
        right: Box::new(AnfExpr::Var("other".to_string())),
    };
    assert!(
        uses_var(&expr, "flag"),
        "uses_var must return true when name matches ShortCircuitOr.left"
    );
}

// OPT-USESVAR-OR-2: uses_var returns false when the queried name does not
// appear in either the left atom or the right sub-expression.
#[test]
fn uses_var_short_circuit_or_false_for_unrelated_name() {
    let expr = AnfExpr::ShortCircuitOr {
        left: "flag".to_string(),
        right: Box::new(AnfExpr::Var("other".to_string())),
    };
    assert!(
        !uses_var(&expr, "z"),
        "uses_var must return false when name is absent from ShortCircuitOr"
    );
}

// ── uses_var: IndexGet ────────────────────────────────────────────────

// OPT-USESVAR-INDEXGET-1: uses_var returns true when the queried name
// matches the collection atom of IndexGet.
#[test]
fn uses_var_index_get_true_for_collection_name() {
    let expr = AnfExpr::IndexGet {
        collection: "lst".to_string(),
        index: "i".to_string(),
    };
    assert!(
        uses_var(&expr, "lst"),
        "uses_var must return true when name matches IndexGet.collection"
    );
}

// OPT-USESVAR-INDEXGET-2: uses_var returns true when the queried name
// matches the index atom of IndexGet.
#[test]
fn uses_var_index_get_true_for_index_name() {
    let expr = AnfExpr::IndexGet {
        collection: "lst".to_string(),
        index: "i".to_string(),
    };
    assert!(
        uses_var(&expr, "i"),
        "uses_var must return true when name matches IndexGet.index"
    );
}

// OPT-USESVAR-INDEXGET-3: uses_var returns false when the queried name
// does not appear in either IndexGet atom.
#[test]
fn uses_var_index_get_false_for_unrelated_name() {
    let expr = AnfExpr::IndexGet {
        collection: "lst".to_string(),
        index: "i".to_string(),
    };
    assert!(
        !uses_var(&expr, "z"),
        "uses_var must return false when name is absent from IndexGet"
    );
}

// ── uses_var: MapNew ──────────────────────────────────────────────────

// OPT-USESVAR-MAPNEW-1: uses_var returns true when the queried name
// matches a key atom in MapNew.entries.
#[test]
fn uses_var_map_new_true_for_key_name() {
    let expr = AnfExpr::MapNew {
        entries: vec![("k".to_string(), "v".to_string())],
    };
    assert!(
        uses_var(&expr, "k"),
        "uses_var must return true when name matches a MapNew key"
    );
}

// OPT-USESVAR-MAPNEW-2: uses_var returns true when the queried name
// matches a value atom in MapNew.entries.
#[test]
fn uses_var_map_new_true_for_value_name() {
    let expr = AnfExpr::MapNew {
        entries: vec![("k".to_string(), "v".to_string())],
    };
    assert!(
        uses_var(&expr, "v"),
        "uses_var must return true when name matches a MapNew value"
    );
}

// OPT-USESVAR-MAPNEW-3: uses_var returns false when the queried name
// does not appear in any MapNew entry.
#[test]
fn uses_var_map_new_false_for_unrelated_name() {
    let expr = AnfExpr::MapNew {
        entries: vec![("k".to_string(), "v".to_string())],
    };
    assert!(
        !uses_var(&expr, "z"),
        "uses_var must return false when name is absent from MapNew"
    );
}

// ── uses_var: SetNew ──────────────────────────────────────────────────

// OPT-USESVAR-SETNEW-1: uses_var returns true when the queried name
// matches an element atom in SetNew.elements.
#[test]
fn uses_var_set_new_true_for_element_name() {
    let expr = AnfExpr::SetNew {
        elements: vec!["a".to_string(), "b".to_string()],
    };
    assert!(
        uses_var(&expr, "a"),
        "uses_var must return true when name matches a SetNew element"
    );
}

// OPT-USESVAR-SETNEW-2: uses_var returns false when the queried name
// does not appear in any SetNew element.
#[test]
fn uses_var_set_new_false_for_unrelated_name() {
    let expr = AnfExpr::SetNew {
        elements: vec!["a".to_string(), "b".to_string()],
    };
    assert!(
        !uses_var(&expr, "z"),
        "uses_var must return false when name is absent from SetNew"
    );
}

// ── uses_var: ForEach ─────────────────────────────────────────────────

// OPT-USESVAR-FOREACH-1: uses_var returns true when the queried name
// matches the collection atom of ForEach.
#[test]
fn uses_var_foreach_true_for_collection_name() {
    let expr = AnfExpr::ForEach {
        collection: "lst".to_string(),
        binding: "item".to_string(),
        body: Box::new(AnfExpr::Var("item".to_string())),
    };
    assert!(
        uses_var(&expr, "lst"),
        "uses_var must return true when name matches ForEach.collection"
    );
}

// OPT-USESVAR-FOREACH-2: uses_var returns true when the queried name
// appears in the body and is not shadowed by the loop binding.
#[test]
fn uses_var_foreach_true_for_body_reference() {
    // body references both the loop variable ("item") and an outer var ("ctx").
    // uses_var("ctx") must return true because "item" != "ctx".
    let expr = AnfExpr::ForEach {
        collection: "lst".to_string(),
        binding: "item".to_string(),
        body: Box::new(AnfExpr::Call {
            func: "print".to_string(),
            args: vec!["item".to_string(), "ctx".to_string()],
        }),
    };
    assert!(
        uses_var(&expr, "ctx"),
        "uses_var must return true when name appears in ForEach body and is not shadowed"
    );
}

// OPT-USESVAR-FOREACH-3: uses_var returns false when the queried name
// does not appear in ForEach.collection or the body.
#[test]
fn uses_var_foreach_false_for_unrelated_name() {
    let expr = AnfExpr::ForEach {
        collection: "lst".to_string(),
        binding: "item".to_string(),
        body: Box::new(AnfExpr::Var("item".to_string())),
    };
    assert!(
        !uses_var(&expr, "z"),
        "uses_var must return false when name is absent from ForEach"
    );
}

// OPT-USESVAR-FOREACH-SHADOW-1: uses_var returns false when the queried
// name equals the loop binding — the binding shadows the outer name inside
// the body, so the outer binding is NOT considered used.
#[test]
fn uses_var_foreach_false_when_binding_shadows_name() {
    // binding = "x", body = Var("x").  The "x" inside the body refers to
    // the loop variable, not the outer "x" we are asking about.
    let expr = AnfExpr::ForEach {
        collection: "lst".to_string(),
        binding: "x".to_string(),
        body: Box::new(AnfExpr::Var("x".to_string())),
    };
    assert!(
        !uses_var(&expr, "x"),
        "ForEach binding 'x' shadows the outer 'x' — uses_var must return false"
    );
}

// ── optimizer diagnostics ─────────────────────────────────────────────

#[test]
fn diagnostics_reports_disabled_pass() {
    let config =
        OptimizerDiagnosticConfig::default().with_disabled_pass(OptimizerPass::InlineSmallPure);
    let diagnostics =
        diagnose_optimizer_with_config(&[binding(AnfExpr::Literal(LiteralValue::Unit))], &config);

    assert!(
        diagnostics.iter().any(|issue| {
            issue.pass == OptimizerPass::InlineSmallPure
                && issue.kind == OptimizerIssueKind::PassDisabled
                && issue.severity == OptimizerSeverity::Info
        }),
        "disabled passes must be visible to production diagnostics"
    );
}

#[test]
fn diagnostics_reports_unsupported_ir_shape() {
    let expr = AnfExpr::Select {
        branches: vec![crate::anf::AnfSelectClause {
            channel: "customer.private.channel".to_string(),
            binding: "payload".to_string(),
            body: AnfExpr::Literal(LiteralValue::Unit),
        }],
    };

    let diagnostics = diagnose_optimizer(&[binding(expr)]);

    assert!(
        diagnostics.iter().any(|issue| {
            issue.pass == OptimizerPass::CseBindings
                && issue.kind == OptimizerIssueKind::UnsupportedIrShape
                && issue.node.starts_with("anf.Select#")
        }),
        "unsupported optimizer IR shapes must be reported with a stable node descriptor"
    );
}

#[test]
fn diagnostics_reports_purity_blocking_reason() {
    let expr = AnfExpr::Seq(vec![
        AnfExpr::EffectCall {
            capability: "secret.payment.gateway".to_string(),
            func: "charge".to_string(),
            args: vec![],
        },
        AnfExpr::Literal(LiteralValue::Unit),
    ]);

    let diagnostics = diagnose_optimizer(&[binding(expr)]);
    let issue = diagnostics
        .iter()
        .find(|issue| {
            issue.pass == OptimizerPass::EliminateDeadPure
                && issue.kind == OptimizerIssueKind::PurityBlocked
        })
        .expect("expected purity blocking diagnostic for effectful seq element");

    assert!(
        issue.detail.contains("external effect"),
        "diagnostic must explain why the pass was blocked"
    );
    assert!(
        issue
            .function
            .as_deref()
            .is_some_and(|f| f.starts_with("fn#")),
        "effect target must be redacted into a stable function descriptor"
    );
    assert!(
        !issue.detail.contains("secret.payment.gateway")
            && !format!("{issue:?}").contains("charge"),
        "diagnostics must not leak raw effect/function names"
    );
}

#[test]
fn diagnostics_reports_non_idempotent_inline_pass() {
    let leaf = AnfBinding {
        source_ref: NodeRef(1),
        name: "fn.customer.secret.leaf".to_string(),
        expr: AnfExpr::Lambda {
            params: vec![],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        },
    };
    let wrapper = AnfBinding {
        source_ref: NodeRef(2),
        name: "fn.customer.secret.wrapper".to_string(),
        expr: AnfExpr::Lambda {
            params: vec![],
            captures: vec![],
            body: Box::new(AnfExpr::Call {
                func: "fn.customer.secret.leaf".to_string(),
                args: vec![],
            }),
        },
    };
    let main = AnfBinding {
        source_ref: NodeRef(3),
        name: "fn.customer.secret.main".to_string(),
        expr: AnfExpr::Call {
            func: "fn.customer.secret.wrapper".to_string(),
            args: vec![],
        },
    };

    let diagnostics = diagnose_optimizer(&[leaf, wrapper, main]);

    assert!(
        diagnostics.iter().any(|issue| {
            issue.pass == OptimizerPass::InlineSmallPure
                && issue.kind == OptimizerIssueKind::NonIdempotentPass
                && issue.severity == OptimizerSeverity::Error
        }),
        "nested small-lambda inlining must surface as a non-idempotent pass issue"
    );
}

#[test]
fn diagnostics_issue_order_is_independent_of_input_order() {
    let first = AnfBinding {
        source_ref: NodeRef(11),
        name: "fn.private.first".to_string(),
        expr: AnfExpr::ForEach {
            binding: "item".to_string(),
            collection: "items".to_string(),
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        },
    };
    let second = AnfBinding {
        source_ref: NodeRef(12),
        name: "fn.private.second".to_string(),
        expr: AnfExpr::Select {
            branches: vec![crate::anf::AnfSelectClause {
                channel: "updates".to_string(),
                binding: "msg".to_string(),
                body: AnfExpr::Literal(LiteralValue::Unit),
            }],
        },
    };

    let forward = diagnose_optimizer(&[first.clone(), second.clone()]).issues;
    let reversed = diagnose_optimizer(&[second, first]).issues;

    assert_eq!(
        forward, reversed,
        "diagnostic ordering must be deterministic, not input-order dependent"
    );
}

#[test]
fn diagnostics_redact_binding_node_and_function_descriptors() {
    let binding = AnfBinding {
        source_ref: NodeRef(42),
        name: "fn.customer.secret.calculate".to_string(),
        expr: AnfExpr::Call {
            func: "fn.customer.secret.calculate".to_string(),
            args: vec!["private_arg".to_string()],
        },
    };

    let binding_descriptor = redacted_binding_descriptor(&binding);
    let node_descriptor = redacted_node_descriptor(&binding.expr);
    let function_descriptor = redacted_function_descriptor("fn.customer.secret.calculate");

    assert!(binding_descriptor.starts_with("binding#"));
    assert!(node_descriptor.starts_with("anf.Call#"));
    assert!(function_descriptor.starts_with("fn#"));
    assert!(!binding_descriptor.contains("customer"));
    assert!(!node_descriptor.contains("customer"));
    assert!(!function_descriptor.contains("customer"));
}
