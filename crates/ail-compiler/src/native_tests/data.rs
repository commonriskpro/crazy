use super::helpers::*;

#[test]
fn native_record_new_differs_from_placeholder() {
    let art = emit_native(&anf_with_record(vec![("x", 1), ("y", 2)])).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "RecordNew must produce different bytes than Placeholder"
    );
    assert_eq!(
        infer_cranelift_return_type(&crate::anf::AnfExpr::RecordNew {
            fields: vec![(
                "x".to_string(),
                crate::anf::AnfExpr::Literal(crate::core_ir::LiteralValue::Int(1))
            )],
        }),
        Some(cranelift_codegen::ir::types::I64)
    );
}

#[test]
fn native_field_get_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::RecordNew {
                fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10)))],
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "r".to_string(),
                field: "x".to_string(),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "FieldGet must produce different bytes than Placeholder"
    );
}

#[test]
fn native_field_update_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::RecordNew {
                fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(1)))],
            }),
            body: Box::new(AnfExpr::FieldUpdate {
                record: "r".to_string(),
                field: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "FieldUpdate must produce different bytes than Placeholder"
    );
}

#[test]
fn native_record_zero_fields_compiles() {
    let art = emit_native(&anf_with_record(vec![]));
    assert!(art.is_ok(), "RecordNew{{[]}} must compile without panic");
}

// ── TASK-H0: VariantNew / ListNew / TupleNew ──────────────────────────

#[test]
fn native_variant_new_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "VariantNew must produce different bytes than Placeholder"
    );
}

#[test]
fn native_list_new_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Int(2)),
        ]),
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "ListNew must produce different bytes than Placeholder"
    );
}

#[test]
fn native_tuple_new_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(3)),
            AnfExpr::Literal(LiteralValue::Int(4)),
        ]),
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "TupleNew must produce different bytes than Placeholder"
    );
}

#[test]
fn native_index_get_with_bounds_guard_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;

    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "list".to_string(),
            value: Box::new(AnfExpr::ListNew(vec![
                AnfExpr::Literal(LiteralValue::Int(1)),
                AnfExpr::Literal(LiteralValue::Int(2)),
            ])),
            body: Box::new(AnfExpr::Let {
                name: "idx".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::IndexGet {
                    collection: "list".to_string(),
                    index: "idx".to_string(),
                }),
            }),
        },
    });

    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "native IndexGet must lower to real guarded code, not a placeholder trap"
    );
}

#[test]
fn native_variant_two_tags_differ() {
    use crate::anf::{AnfBinding, AnfExpr};
    let make_variant = |tag: &str| {
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::VariantNew {
                tag: tag.to_string(),
                payload: None,
            },
        })
    };
    let art_ok = emit_native(&make_variant("Ok")).unwrap();
    let art_err = emit_native(&make_variant("Err")).unwrap();
    assert_ne!(
        art_ok.native_bytes, art_err.native_bytes,
        "VariantNew('Ok') and VariantNew('Err') must produce different bytes (different tag ids)"
    );
}

// ── TASK-I0: EffectCall — RED ─────────────────────────────────────────

#[test]
fn native_bytes_literal_differs_from_placeholder() {
    let art = emit_native(&anf_with_bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Literal(Bytes([0xDE, 0xAD, 0xBE, 0xEF])) must produce different bytes than Placeholder"
    );
}

// B-2: Two different byte slices produce different native_bytes (triangulation).
#[test]
fn native_bytes_two_slices_differ() {
    let art1 = emit_native(&anf_with_bytes(vec![1, 2, 3])).unwrap();
    let art2 = emit_native(&anf_with_bytes(vec![4, 5, 6])).unwrap();
    assert_ne!(
        art1.native_bytes, art2.native_bytes,
        "Bytes([1,2,3]) and Bytes([4,5,6]) must produce different native_bytes"
    );
}

// B-3: Same byte slice is interned once (deduplication in NativeDataLayout).
#[test]
fn native_bytes_same_slice_deduplicated() {
    let mut layout = NativeDataLayout::default();
    let idx1 = layout.intern_bytes(&[0xCA, 0xFE]);
    let idx2 = layout.intern_bytes(&[0xCA, 0xFE]);
    assert_eq!(idx1, idx2, "Same byte slice must intern to same index");
    assert_eq!(
        layout.bytes_table.len(),
        1,
        "Only one bytes_table entry should exist for duplicate slices"
    );
}

// B-4: infer_cranelift_return_type returns I64 for Bytes.
#[test]
fn native_bytes_infer_return_type_is_i64() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::types;
    let expr = AnfExpr::Literal(LiteralValue::Bytes(vec![1, 2, 3]));
    assert_eq!(
        infer_cranelift_return_type(&expr),
        Some(types::I64),
        "infer_cranelift_return_type for Literal(Bytes) must return Some(I64)"
    );
}

// B-5: Empty byte slice compiles without panic.
#[test]
fn native_bytes_empty_slice_compiles() {
    let result = emit_native(&anf_with_bytes(vec![]));
    assert!(
        result.is_ok(),
        "Literal(Bytes([])) must compile without panic: {:?}",
        result.err()
    );
}
