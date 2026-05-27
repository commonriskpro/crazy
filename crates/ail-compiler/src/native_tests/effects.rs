use super::helpers::*;

#[test]
fn native_effect_call_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "id".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "db".to_string(),
                func: "read".to_string(),
                args: vec!["id".to_string()],
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "EffectCall must produce different bytes than Placeholder"
    );
}

#[test]
fn native_effect_call_two_capabilities_differ() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let make_effect = |cap: &str| {
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "id".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::EffectCall {
                    capability: cap.to_string(),
                    func: "read".to_string(),
                    args: vec!["id".to_string()],
                }),
            },
        })
    };
    let art_db = emit_native(&make_effect("db")).unwrap();
    let art_fs = emit_native(&make_effect("fs")).unwrap();
    assert_ne!(
        art_db.native_bytes, art_fs.native_bytes,
        "EffectCall('db') and EffectCall('fs') must produce different bytes"
    );
}

#[test]
fn native_effect_call_native_hash_is_some() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "id".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "db".to_string(),
                func: "read".to_string(),
                args: vec!["id".to_string()],
            }),
        },
    });
    let art = emit_native(&anf).unwrap();
    assert!(
        art.hash_chain.native_hash.is_some(),
        "native_hash must be Some for EffectCall"
    );
}

// ── TASK-J0: Lambda closure env construction ──────────────────────────
//
// PR2 invariant: captures must NOT be silently dropped.
//
// Scenario map:
//   J-1: Lambda with no captures → bare fn-ptr, compiles, differs from Placeholder.
//   J-2: Two no-capture lambdas with different bodies → different bytes.
//   J-3: Lambda with one capture → compiles without error.
//   J-4: Lambda with captures → different bytes than the same lambda with no captures
//        (closure env allocation changes the emitted code).
//   J-5: Lambda with two captures → different bytes than lambda with one capture
//        (env size and stored values differ).
