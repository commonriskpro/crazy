use super::helpers::*;

#[test]
fn native_lambda_no_captures_differs_from_placeholder() {
    let art = emit_native(&anf_lambda_no_captures(7)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Lambda with no captures must produce different bytes than Placeholder"
    );
}

// J-2: Two no-capture lambdas with different body constants differ in bytes.
#[test]
fn native_lambda_no_captures_body_triangulate() {
    let art1 = emit_native(&anf_lambda_no_captures(1)).unwrap();
    let art2 = emit_native(&anf_lambda_no_captures(99)).unwrap();
    assert_ne!(
        art1.native_bytes, art2.native_bytes,
        "Lambda no-capture: body constant 1 vs 99 must produce different bytes"
    );
}

// J-3: Lambda with one capture compiles without error.
#[test]
fn native_lambda_with_one_capture_compiles() {
    let result = emit_native(&anf_lambda_one_capture(42));
    assert!(
        result.is_ok(),
        "Lambda with one capture must compile without error: {:?}",
        result.err()
    );
}

// J-4: Lambda with a capture produces different bytes than the same lambda
// without captures.  The closure env allocation and stores change the IR.
#[test]
fn native_lambda_with_capture_differs_from_no_capture() {
    let with_cap = emit_native(&anf_lambda_one_capture(42)).unwrap();
    // Build a structurally similar no-capture lambda for comparison.
    let without_cap = emit_native(&anf_lambda_no_captures(42)).unwrap();
    assert_ne!(
        with_cap.native_bytes, without_cap.native_bytes,
        "Lambda with captures must produce different bytes than lambda with no captures: \
         closure env allocation must be emitted, not silently dropped"
    );
}

#[test]
fn native_lambda_return_var_param_compiles() {
    use crate::anf::AnfExpr;
    let anf =
        anf_lambda_returning_param_body(AnfExpr::Return(Box::new(AnfExpr::Var("p".to_string()))));
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "Lambda body Return(Var(param)) must infer an I64 return: {:?}",
        result.err()
    );
}

#[test]
fn native_lambda_let_wrapped_return_var_param_compiles() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    let anf = anf_lambda_returning_param_body(AnfExpr::Let {
        name: "tmp".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Return(Box::new(AnfExpr::Var("p".to_string())))),
    });
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "Lambda body Let(... Return(Var(param))) must infer an I64 return: {:?}",
        result.err()
    );
}

#[test]
fn native_lambda_seq_wrapped_return_var_param_compiles() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    let anf = anf_lambda_returning_param_body(AnfExpr::Seq(vec![
        AnfExpr::Literal(LiteralValue::Int(1)),
        AnfExpr::Return(Box::new(AnfExpr::Var("p".to_string()))),
    ]));
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "Lambda body Seq(... Return(Var(param))) must infer an I64 return: {:?}",
        result.err()
    );
}

// J-5a: NativeDataLayout must set needs_heap_alloc for Lambda with captures.
// Proves the pre-scan correctly identifies that a closure env requires heap
// allocation, which in turn drives __ail_malloc import in emit_native.
#[test]
fn native_data_layout_lambda_with_captures_needs_heap_alloc() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec!["x".to_string()],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        },
    };
    let layout = NativeDataLayout::for_bindings(&[binding]);
    assert!(
        layout.needs_heap_alloc,
        "Lambda with non-empty captures must set needs_heap_alloc in NativeDataLayout"
    );
}

// J-5b: NativeDataLayout must NOT set needs_heap_alloc for Lambda with no captures.
// Negative test: empty captures → no env allocation needed.
#[test]
fn native_data_layout_lambda_no_captures_no_heap_alloc() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        },
    };
    let layout = NativeDataLayout::for_bindings(&[binding]);
    assert!(
        !layout.needs_heap_alloc,
        "Lambda with empty captures must not set needs_heap_alloc in NativeDataLayout"
    );
}

// ── Wave 11A: Native Bytes literal emit ───────────────────────────────
//
// Scenario map:
//   B-1: Bytes literal compiles and produces different bytes than Placeholder.
//   B-2: Two different byte slices produce different native_bytes.
//   B-3: Same byte slice interns to the same index (deduplication).
//   B-4: infer_cranelift_return_type returns I64 for Bytes.
//   B-5: Empty byte slice compiles without panic.
