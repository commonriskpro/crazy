use super::*;

#[test]
fn lower_print_sugar_to_log_write_effect_call() {
    use crate::anf::AnfExpr;
    use crate::core_ir::{CoreIr, CoreNode, CoreNodeKind, LiteralValue, StageHashes};

    let expr = crate::expr_parser::parse_expr("print(\"Hello, world!\")").unwrap();
    let core = CoreIr {
        nodes: vec![CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Function,
            name: "fn.main".to_string(),
            ty: None,
            expr: Some(expr),
        }],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    };

    let anf = lower_to_anf(&core).expect("print sugar must lower to ANF");
    match &anf.bindings[0].expr {
        AnfExpr::Let { name, value, body } => {
            assert_eq!(name, "anf_0");
            assert_eq!(
                **value,
                AnfExpr::Literal(LiteralValue::Text("Hello, world!".to_string()))
            );
            assert_eq!(
                **body,
                AnfExpr::EffectCall {
                    capability: "log.write".to_string(),
                    func: "write".to_string(),
                    args: vec!["anf_0".to_string()],
                }
            );
        }
        other => panic!("expected print sugar to lower through a local let, got {other:?}"),
    }
}

// S1: Match — scrutinee Var is preserved as atomic name.
#[test]
fn lower_match_var_scrutinee_is_preserved() {
    use crate::core_ir::{CoreExpr, MatchArm};
    let expr = CoreExpr::Match {
        scrutinee: Box::new(CoreExpr::Var("payment".to_string())),
        arms: vec![MatchArm {
            pattern: "Ok(r)".to_string(),
            body: CoreExpr::Var("r".to_string()),
        }],
    };
    let (result, out) = lower_single(&expr);
    // Scrutinee is already Var, so no extra bindings emitted.
    assert!(
        out.is_empty(),
        "Var scrutinee must not produce extra bindings"
    );
    match result {
        crate::anf::AnfExpr::Match { scrutinee, arms } => {
            assert_eq!(scrutinee, "payment");
            assert_eq!(arms.len(), 1);
            assert_eq!(arms[0].pattern, "Ok(r)");
        }
        other => panic!("expected AnfExpr::Match, got {other:?}"),
    }
}

// S1b: Match — non-Var scrutinee is atomized (produces synthetic binding).
#[test]
fn lower_match_complex_scrutinee_is_atomized() {
    use crate::core_ir::{CoreExpr, LiteralValue, MatchArm};
    let expr = CoreExpr::Match {
        scrutinee: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
        arms: vec![MatchArm {
            pattern: "_".to_string(),
            body: CoreExpr::Literal(LiteralValue::Unit),
        }],
    };
    let (result, out) = lower_single(&expr);
    // Literal scrutinee must be atomized → one synthetic binding.
    assert!(
        !out.is_empty(),
        "Literal scrutinee must produce a synthetic binding"
    );
    match result {
        crate::anf::AnfExpr::Match { scrutinee, .. } => {
            // scrutinee must be the synthetic name, not "42"
            assert!(
                scrutinee.starts_with("anf_"),
                "scrutinee must be synthetic name, got {scrutinee}"
            );
        }
        other => panic!("expected AnfExpr::Match, got {other:?}"),
    }
}

// S2: Lambda — params and body lowered correctly.
#[test]
fn lower_lambda_params_and_body() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string(), "y".to_string()],
        body: Box::new(CoreExpr::Var("x".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Lambda body Var must not produce extra bindings"
    );
    match result {
        crate::anf::AnfExpr::Lambda {
            params,
            body,
            captures,
        } => {
            assert_eq!(params, vec!["x", "y"]);
            // Body is Var("x") which is bound by params — so no captures.
            assert!(captures.is_empty(), "no free vars in identity lambda");
            assert_eq!(*body, crate::anf::AnfExpr::Var("x".to_string()));
        }
        other => panic!("expected AnfExpr::Lambda, got {other:?}"),
    }
}

// S3: RecordNew — field values are fully ANF-normalized (let-bound atomics).
//
// Full ANF normalization: non-Var field values must be let-bound before use.
// A Var field still passes through atomize but returns the same name without
// producing an extra binding.  A Literal field WILL produce a synthetic
// binding (anf_0) and the field value will be Var("anf_0").
#[test]
fn lower_record_new_field_values() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::RecordNew {
        fields: vec![
            (
                "amount".to_string(),
                CoreExpr::Literal(LiteralValue::Int(10)),
            ),
            ("label".to_string(), CoreExpr::Var("lbl".to_string())),
        ],
    };
    let (result, out) = lower_single(&expr);
    // Literal field must produce one synthetic binding.
    assert_eq!(
        out.len(),
        1,
        "Literal field must produce one synthetic binding, got {out:?}"
    );
    assert_eq!(
        out[0].expr,
        crate::anf::AnfExpr::Literal(LiteralValue::Int(10))
    );
    let synthetic_name = out[0].name.clone();
    match result {
        crate::anf::AnfExpr::RecordNew { fields } => {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].0, "amount");
            // Literal field → Var(synthetic_name)
            assert_eq!(fields[0].1, crate::anf::AnfExpr::Var(synthetic_name));
            assert_eq!(fields[1].0, "label");
            // Var field → Var("lbl") (same name, no extra binding)
            assert_eq!(fields[1].1, crate::anf::AnfExpr::Var("lbl".to_string()));
        }
        other => panic!("expected AnfExpr::RecordNew, got {other:?}"),
    }
}

// S4: FieldUpdate — record Var is preserved as atomic name;
//     value is also atomized (full ANF normalization).
#[test]
fn lower_field_update_var_record_is_preserved() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::FieldUpdate {
        record: Box::new(CoreExpr::Var("order".to_string())),
        field: "status".to_string(),
        value: Box::new(CoreExpr::Literal(LiteralValue::Text("Paid".to_string()))),
    };
    let (result, out) = lower_single(&expr);
    // Literal value must produce one synthetic binding.
    assert_eq!(
        out.len(),
        1,
        "Literal value must produce one synthetic binding"
    );
    let value_name = out[0].name.clone();
    match result {
        crate::anf::AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            assert_eq!(record, "order");
            assert_eq!(field, "status");
            // Value is now a Var referring to the synthetic binding.
            assert_eq!(*value, crate::anf::AnfExpr::Var(value_name));
        }
        other => panic!("expected AnfExpr::FieldUpdate, got {other:?}"),
    }
}

// S5: TupleNew — elements are lowered recursively.
#[test]
fn lower_tuple_new_elements() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::TupleNew(vec![
        CoreExpr::Var("a".to_string()),
        CoreExpr::Var("b".to_string()),
    ]);
    let (result, _out) = lower_single(&expr);
    match result {
        crate::anf::AnfExpr::TupleNew(elems) => {
            assert_eq!(elems.len(), 2);
            assert_eq!(elems[0], crate::anf::AnfExpr::Var("a".to_string()));
            assert_eq!(elems[1], crate::anf::AnfExpr::Var("b".to_string()));
        }
        other => panic!("expected AnfExpr::TupleNew, got {other:?}"),
    }
}

// S6: VariantNew with payload.
#[test]
fn lower_variant_new_with_payload() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::VariantNew {
        tag: "Ok".to_string(),
        payload: Some(Box::new(CoreExpr::Var("x".to_string()))),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var payload must not produce extra bindings"
    );
    match result {
        crate::anf::AnfExpr::VariantNew { tag, payload } => {
            assert_eq!(tag, "Ok");
            assert_eq!(*payload.unwrap(), crate::anf::AnfExpr::Var("x".to_string()));
        }
        other => panic!("expected AnfExpr::VariantNew, got {other:?}"),
    }
}

// S6b: VariantNew without payload.
#[test]
fn lower_variant_new_no_payload() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::VariantNew {
        tag: "None".to_string(),
        payload: None,
    };
    let (result, _out) = lower_single(&expr);
    match result {
        crate::anf::AnfExpr::VariantNew { tag, payload } => {
            assert_eq!(tag, "None");
            assert!(payload.is_none());
        }
        other => panic!("expected AnfExpr::VariantNew, got {other:?}"),
    }
}

// S7: ListNew — elements are fully ANF-normalized (let-bound atomics).
//
// Full ANF normalization: non-Var elements are let-bound.
// Literal(1) → synthetic binding anf_0 → Var("anf_0")
// Var("x")   → passes through atomize as "x" → Var("x")
#[test]
fn lower_list_new_elements() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::ListNew(vec![
        CoreExpr::Literal(LiteralValue::Int(1)),
        CoreExpr::Var("x".to_string()),
    ]);
    let (result, out) = lower_single(&expr);
    // Literal element must produce one synthetic binding.
    assert_eq!(
        out.len(),
        1,
        "Literal element must produce one synthetic binding"
    );
    let lit_name = out[0].name.clone();
    match result {
        crate::anf::AnfExpr::ListNew(elems) => {
            assert_eq!(elems.len(), 2);
            assert_eq!(elems[0], crate::anf::AnfExpr::Var(lit_name));
            assert_eq!(elems[1], crate::anf::AnfExpr::Var("x".to_string()));
        }
        other => panic!("expected AnfExpr::ListNew, got {other:?}"),
    }
}

// S8: No Placeholder produced for real CoreExpr variants.
#[test]
fn real_core_exprs_do_not_produce_placeholder() {
    use crate::core_ir::{CoreExpr, LiteralValue, MatchArm};
    let real_exprs = vec![
        CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("x".to_string())),
            arms: vec![MatchArm {
                pattern: "_".to_string(),
                body: CoreExpr::Literal(LiteralValue::Unit),
            }],
        },
        CoreExpr::Lambda {
            params: vec![],
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::RecordNew { fields: vec![] },
        CoreExpr::FieldUpdate {
            record: Box::new(CoreExpr::Var("r".to_string())),
            field: "f".to_string(),
            value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::TupleNew(vec![]),
        CoreExpr::VariantNew {
            tag: "A".to_string(),
            payload: None,
        },
        CoreExpr::ListNew(vec![]),
    ];
    for expr in &real_exprs {
        let (result, _out) = lower_single(expr);
        assert_ne!(
            result,
            crate::anf::AnfExpr::Placeholder,
            "CoreExpr::{expr:?} must NOT produce Placeholder"
        );
    }
}

// S9: CoreExpr::Placeholder still produces AnfExpr::Placeholder.
#[test]
fn placeholder_still_maps_to_placeholder() {
    use crate::core_ir::CoreExpr;
    let (result, _out) = lower_single(&CoreExpr::Placeholder);
    assert_eq!(result, crate::anf::AnfExpr::Placeholder);
}
