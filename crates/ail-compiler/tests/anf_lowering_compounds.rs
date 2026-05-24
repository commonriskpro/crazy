// ── ail-compiler::anf_lowering_compounds ─────────────────────────────────
//
// G20 R2 / ola5-compiler-core G2:
// ANF lowering tests for compound data constructors and the G2 gap variants.
//
// Spec scenarios covered:
//   R2-S19 — RecordNew with Literal field is let-bound (atomized)
//   R2-S20 — TupleNew with Literal elements are let-bound
//   R2-S21 — VariantNew Literal payload is let-bound
//   G2-1   — CoreExpr::Return lowers to AnfExpr::Return (not Placeholder)
//   G2-2   — CoreExpr::Assume lowers to AnfExpr::Assume (not Placeholder)
//   G2-3   — CoreExpr::Abort lowers to AnfExpr::Abort (not Placeholder)
//   G2-4   — CoreExpr::BoundaryCall lowers to AnfExpr::Call (not Placeholder)
//   G2-5   — CoreExpr::DynCall lowers to AnfExpr::Call (not Placeholder)
//   G2-6   — CoreExpr::IndexGet lowers to non-Placeholder
//   G2-7   — CoreExpr::MapNew lowers to non-Placeholder
//   G2-8   — CoreExpr::SetNew lowers to non-Placeholder
//   G2-9   — CoreExpr::ForEach lowers to non-Placeholder
//   G2-10  — CoreExpr::Fold lowers to non-Placeholder
//   G2-T   — BoundaryCall args are atomized
//   G2-R   — Return with non-atomic value produces synthetic let-binding

mod anf_lowering_helpers;
use anf_lowering_helpers::{lower_and_collect, lower_expr};

use ail_compiler::lower::lower_core_expr_to_anf;
use ail_compiler::{AnfExpr, CoreExpr, LiteralValue};
use ail_core::semantic_graph::NodeRef;

// ── G20 R2: composite children full ANF normalization ─────────────────────

// R2-S19: RecordNew with Literal field — field is let-bound (atomized).
#[test]
fn record_new_literal_field_is_let_bound() {
    let expr = CoreExpr::RecordNew {
        fields: vec![(
            "price".to_string(),
            CoreExpr::Literal(LiteralValue::Int(99)),
        )],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "Literal field must produce one synthetic binding"
    );
    match result {
        AnfExpr::RecordNew { fields } => {
            // Field value must be a Var referring to the synthetic binding.
            assert!(matches!(fields[0].1, AnfExpr::Var(_)));
        }
        other => panic!("expected RecordNew, got {other:?}"),
    }
}

// R2-S20: TupleNew with Literal elements — elements are let-bound.
#[test]
fn tuple_new_literal_elements_are_let_bound() {
    let expr = CoreExpr::TupleNew(vec![
        CoreExpr::Literal(LiteralValue::Int(1)),
        CoreExpr::Literal(LiteralValue::Bool(false)),
    ]);
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        2,
        "Two Literal elements → two synthetic bindings"
    );
    match result {
        AnfExpr::TupleNew(elems) => {
            assert!(matches!(elems[0], AnfExpr::Var(_)));
            assert!(matches!(elems[1], AnfExpr::Var(_)));
        }
        other => panic!("expected TupleNew, got {other:?}"),
    }
}

// R2-S21: VariantNew Literal payload is let-bound.
#[test]
fn variant_new_literal_payload_is_let_bound() {
    let expr = CoreExpr::VariantNew {
        tag: "Some".to_string(),
        payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Int(42)))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "Literal payload must produce one synthetic binding"
    );
    match result {
        AnfExpr::VariantNew { payload, .. } => {
            assert!(matches!(*payload.unwrap(), AnfExpr::Var(_)));
        }
        other => panic!("expected VariantNew, got {other:?}"),
    }
}

// ── ola5-compiler-core G2: gap variants ──────────────────────────────────

// G2-1: CoreExpr::Return lowers to AnfExpr::Return (not Placeholder).
#[test]
fn return_lowers_to_anf_return() {
    let expr = CoreExpr::Return {
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "Return must not lower to Placeholder, got {result:?}"
    );
    assert!(
        matches!(result, AnfExpr::Return(_)),
        "Return must lower to AnfExpr::Return, got {result:?}"
    );
}

// G2-2: CoreExpr::Assume lowers to AnfExpr::Assume (not Placeholder).
#[test]
fn assume_lowers_not_to_placeholder() {
    let expr = CoreExpr::Assume {
        predicate: "x > 0".to_string(),
        reason: "precondition guaranteed by caller".to_string(),
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "Assume must not lower to Placeholder, got {result:?}"
    );
}

// G2-3: CoreExpr::Abort lowers to AnfExpr::Abort (not Placeholder).
#[test]
fn abort_lowers_not_to_placeholder() {
    let expr = CoreExpr::Abort {
        message: "unreachable branch".to_string(),
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "Abort must not lower to Placeholder, got {result:?}"
    );
}

// G2-4: CoreExpr::BoundaryCall lowers to AnfExpr::Call (not Placeholder).
#[test]
fn boundary_call_lowers_not_to_placeholder() {
    let expr = CoreExpr::BoundaryCall {
        boundary: "payments.stripe".to_string(),
        func: "charge".to_string(),
        args: vec![CoreExpr::Var("amount".to_string())],
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "BoundaryCall must not lower to Placeholder, got {result:?}"
    );
}

// G2-5: CoreExpr::DynCall lowers to AnfExpr::Call (not Placeholder).
#[test]
fn dyn_call_lowers_not_to_placeholder() {
    let expr = CoreExpr::DynCall {
        interface: "Repository<User>".to_string(),
        method: "get".to_string(),
        args: vec![CoreExpr::Var("id".to_string())],
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "DynCall must not lower to Placeholder, got {result:?}"
    );
}

// G2-6: CoreExpr::IndexGet lowers to non-Placeholder.
#[test]
fn index_get_lowers_not_to_placeholder() {
    let expr = CoreExpr::IndexGet {
        collection: Box::new(CoreExpr::Var("list".to_string())),
        index: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "IndexGet must not lower to Placeholder, got {result:?}"
    );
}

// G2-7: CoreExpr::MapNew lowers to non-Placeholder.
#[test]
fn map_new_lowers_not_to_placeholder() {
    let expr = CoreExpr::MapNew {
        entries: vec![(
            CoreExpr::Literal(LiteralValue::Text("key".to_string())),
            CoreExpr::Literal(LiteralValue::Int(1)),
        )],
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "MapNew must not lower to Placeholder, got {result:?}"
    );
}

// G2-8: CoreExpr::SetNew lowers to non-Placeholder.
#[test]
fn set_new_lowers_not_to_placeholder() {
    let expr = CoreExpr::SetNew {
        elements: vec![
            CoreExpr::Literal(LiteralValue::Int(1)),
            CoreExpr::Literal(LiteralValue::Int(2)),
        ],
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "SetNew must not lower to Placeholder, got {result:?}"
    );
}

// G2-9: CoreExpr::ForEach lowers to non-Placeholder.
#[test]
fn for_each_lowers_not_to_placeholder() {
    let expr = CoreExpr::ForEach {
        binding: "item".to_string(),
        collection: Box::new(CoreExpr::Var("items".to_string())),
        body: Box::new(CoreExpr::Var("item".to_string())),
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "ForEach must not lower to Placeholder, got {result:?}"
    );
}

// G2-10: CoreExpr::Fold lowers to non-Placeholder.
#[test]
fn fold_lowers_not_to_placeholder() {
    let expr = CoreExpr::Fold {
        init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        list: Box::new(CoreExpr::Var("items".to_string())),
        func: Box::new(CoreExpr::Var("add".to_string())),
    };
    let result = lower_expr(&expr);
    assert!(
        !matches!(result, AnfExpr::Placeholder),
        "Fold must not lower to Placeholder, got {result:?}"
    );
}

// G2-T: BoundaryCall args are atomized — synthetic let-bindings emitted.
#[test]
fn boundary_call_atomizes_args() {
    let expr = CoreExpr::BoundaryCall {
        boundary: "ext.api".to_string(),
        func: "create".to_string(),
        // Two non-atomic args — must be atomized via synthetic let-bindings.
        args: vec![
            CoreExpr::Literal(LiteralValue::Int(10)),
            CoreExpr::Literal(LiteralValue::Int(20)),
        ],
    };
    let (synth, _root) = lower_and_collect(&expr);
    assert_eq!(
        synth.len(),
        2,
        "Two non-atomic BoundaryCall args must produce 2 synthetic let-bindings, got {}",
        synth.len()
    );
}

// G2-R: Return with non-atomic value produces synthetic let-binding.
#[test]
fn return_with_non_atomic_value_produces_let_binding() {
    let expr = CoreExpr::Return {
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(7))),
    };
    let (synth, root) = lower_and_collect(&expr);
    // The literal is atomized → one synthetic binding, root is Return(Var(name))
    assert_eq!(
        synth.len(),
        1,
        "Return with literal value must produce 1 synthetic binding"
    );
    assert!(
        matches!(root, AnfExpr::Return(_)),
        "Root must be AnfExpr::Return, got {root:?}"
    );
}
