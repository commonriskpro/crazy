use super::*;

#[test]
fn closure_capture_lambda_no_capture() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> x
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Var("x".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.is_empty(),
            "identity lambda must have no captures; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda that references an outer variable — must appear in captures.
#[test]
fn closure_capture_lambda_with_outer_var() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> add(x, outer_val)   — `outer_val` is free
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Call {
            func: "add".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("outer_val".to_string()),
            ],
        }),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.contains(&"outer_val".to_string()),
            "outer_val must be captured; got {captures:?}"
        );
        assert!(
            !captures.contains(&"x".to_string()),
            "param x must NOT appear in captures; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda param shadows an outer variable of the same name — the
// outer name must NOT appear in captures (the param takes precedence).
#[test]
fn closure_capture_lambda_shadowed_param_not_captured() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(outer) -> outer  — param named "outer" shadows any outer binding
    let expr = CoreExpr::Lambda {
        params: vec!["outer".to_string()],
        body: Box::new(CoreExpr::Var("outer".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.is_empty(),
            "`outer` is shadowed by param; captures must be empty; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda whose body contains an EffectCall that references an outer
// variable — the outer variable must appear in captures.
#[test]
fn closure_capture_lambda_effect_call_arg_captured() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> db.read(x, context_id)  — `context_id` is free
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::EffectCall {
            capability: "db".to_string(),
            func: "read".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("context_id".to_string()),
            ],
        }),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.contains(&"context_id".to_string()),
            "context_id must be captured from EffectCall arg; got {captures:?}"
        );
        assert!(
            !captures.contains(&"x".to_string()),
            "param x must NOT be captured; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// ── End closure-capture tests ─────────────────────────────────────────────

// ── Wave 10A: Bytes literal emit, descriptor, and data-section tests ──────

// Scenario: derive_wasm_type on a Bytes literal must return WasmTypeDescriptor::Bytes.
// Proves the compiler side of the descriptor contract for Bytes.

