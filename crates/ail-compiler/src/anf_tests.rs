// Tests for the ANF IR types.
// Declared from anf.rs as: #[cfg(test)] #[path = "anf_tests.rs"] mod tests;

use super::*;
use crate::core_ir::{LiteralValue, StageHashes};
use crate::hash::stable_cbor_bytes;
#[allow(unused_imports)]
use ciborium;

// ── AnfExpr construction ──────────────────────────────────────────────

// All AnfExpr variants are constructible without panic.
#[test]
fn all_anf_expr_variants_are_constructible() {
    let _lit = AnfExpr::Literal(LiteralValue::Int(42));
    let _var = AnfExpr::Var("x".to_string());
    let _let = AnfExpr::Let {
        name: "y".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        body: Box::new(AnfExpr::Var("y".to_string())),
    };
    let _if = AnfExpr::If {
        cond: "flag".to_string(),
        then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
    };
    let _call = AnfExpr::Call {
        func: "fn.add".to_string(),
        args: vec!["a".to_string(), "b".to_string()],
    };
    let _fg = AnfExpr::FieldGet {
        record: "order".to_string(),
        field: "total".to_string(),
    };
    let _ret = AnfExpr::Return(Box::new(AnfExpr::Var("result".to_string())));
    let _seq = AnfExpr::Seq(vec![
        AnfExpr::Call {
            func: "db.write".to_string(),
            args: vec!["order".to_string()],
        },
        AnfExpr::Literal(LiteralValue::Unit),
    ]);
    let _placeholder = AnfExpr::Placeholder;
    // G20 variants
    let _match = AnfExpr::Match {
        scrutinee: "v".to_string(),
        arms: vec![AnfMatchArm {
            pattern: "Some(x)".to_string(),
            body: AnfExpr::Var("x".to_string()),
        }],
    };
    let _lambda = AnfExpr::Lambda {
        params: vec!["x".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Var("x".to_string())),
    };
    let _record = AnfExpr::RecordNew {
        fields: vec![(
            "amount".to_string(),
            AnfExpr::Literal(LiteralValue::Int(10)),
        )],
    };
    let _field_update = AnfExpr::FieldUpdate {
        record: "order".to_string(),
        field: "status".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Text("Paid".to_string()))),
    };
    let _tuple = AnfExpr::TupleNew(vec![
        AnfExpr::Var("a".to_string()),
        AnfExpr::Var("b".to_string()),
    ]);
    let _variant = AnfExpr::VariantNew {
        tag: "Ok".to_string(),
        payload: Some(Box::new(AnfExpr::Var("x".to_string()))),
    };
    let _list = AnfExpr::ListNew(vec![AnfExpr::Literal(LiteralValue::Int(1))]);
    let _loop = AnfExpr::Loop {
        body: Box::new(AnfExpr::Break {
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
        }),
    };
    let _break = AnfExpr::Break {
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
    };
    let _continue = AnfExpr::Continue;
    let _while_loop = AnfExpr::WhileLoop {
        cond: "flag".to_string(),
        body: Box::new(AnfExpr::Continue),
    };
}

// G23: AnfSelectClause is constructible with correct fields.
#[test]
fn anf_select_clause_is_constructible() {
    let clause = AnfSelectClause {
        channel: "inbox".to_string(),
        binding: "msg".to_string(),
        body: AnfExpr::Var("msg".to_string()),
    };
    assert_eq!(clause.channel, "inbox");
    assert_eq!(clause.binding, "msg");
    assert_eq!(clause.body, AnfExpr::Var("msg".to_string()));
}

// G23: all new concurrency + cell AnfExpr variants are constructible.
#[test]
fn all_new_concurrency_cell_anf_expr_variants_are_constructible() {
    let _task_await = AnfExpr::TaskAwait {
        task: "t".to_string(),
    };
    let _task_cancel = AnfExpr::TaskCancel {
        task: "t".to_string(),
    };
    let _task_group = AnfExpr::TaskGroup {
        body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
    };
    let _channel_new_unbounded = AnfExpr::ChannelNew { capacity: None };
    let _channel_new_bounded = AnfExpr::ChannelNew { capacity: Some(32) };
    let _select = AnfExpr::Select {
        branches: vec![AnfSelectClause {
            channel: "ch".to_string(),
            binding: "v".to_string(),
            body: AnfExpr::Var("v".to_string()),
        }],
    };
    let _timeout = AnfExpr::Timeout {
        duration: "dur".to_string(),
        body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
    };
    let _cell_new = AnfExpr::CellNew {
        init: "zero".to_string(),
    };
    let _cell_get = AnfExpr::CellGet {
        cell: "c".to_string(),
    };
    let _cell_set = AnfExpr::CellSet {
        cell: "c".to_string(),
        value: "v".to_string(),
    };
    // All constructed without panic — test passes.
}

// TRIANGULATE: channel operands are atomic strings (not nested exprs).
#[test]
fn anf_task_await_task_is_atomic_string() {
    let expr = AnfExpr::TaskAwait {
        task: "task_0".to_string(),
    };
    if let AnfExpr::TaskAwait { task } = expr {
        assert_eq!(task, "task_0");
    } else {
        panic!("expected TaskAwait");
    }
}

// G23: new concurrency + cell variants CBOR round-trip.
#[test]
fn new_concurrency_cell_anf_variants_cbor_round_trip() {
    use crate::hash::stable_cbor_bytes;
    let variants: Vec<AnfExpr> = vec![
        AnfExpr::TaskAwait {
            task: "t".to_string(),
        },
        AnfExpr::TaskCancel {
            task: "t".to_string(),
        },
        AnfExpr::TaskGroup {
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        },
        AnfExpr::ChannelNew { capacity: None },
        AnfExpr::ChannelNew { capacity: Some(4) },
        AnfExpr::Select {
            branches: vec![AnfSelectClause {
                channel: "ch".to_string(),
                binding: "v".to_string(),
                body: AnfExpr::Var("v".to_string()),
            }],
        },
        AnfExpr::Timeout {
            duration: "d".to_string(),
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        },
        AnfExpr::CellNew {
            init: "zero".to_string(),
        },
        AnfExpr::CellGet {
            cell: "c".to_string(),
        },
        AnfExpr::CellSet {
            cell: "c".to_string(),
            value: "v".to_string(),
        },
    ];
    for expr in &variants {
        let bytes = stable_cbor_bytes(expr).expect("encode must succeed");
        let decoded: AnfExpr =
            ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
        assert_eq!(&decoded, expr, "AnfExpr must survive CBOR round-trip");
    }
}

// G20: AnfMatchArm is constructible and has correct fields.
#[test]
fn anf_match_arm_is_constructible() {
    let arm = AnfMatchArm {
        pattern: "None".to_string(),
        body: AnfExpr::Literal(LiteralValue::Unit),
    };
    assert_eq!(arm.pattern, "None");
    assert_eq!(arm.body, AnfExpr::Literal(LiteralValue::Unit));
}

// G20: AnfExpr::Match — scrutinee is a String (atomic name).
#[test]
fn anf_match_scrutinee_is_atomic_string() {
    let expr = AnfExpr::Match {
        scrutinee: "payment".to_string(),
        arms: vec![AnfMatchArm {
            pattern: "Ok(r)".to_string(),
            body: AnfExpr::Var("r".to_string()),
        }],
    };
    if let AnfExpr::Match { scrutinee, arms } = &expr {
        assert_eq!(scrutinee, "payment");
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].pattern, "Ok(r)");
    } else {
        panic!("expected Match variant");
    }
}

// G20: AnfExpr::Match CBOR round-trip.
#[test]
fn anf_match_cbor_round_trip() {
    let expr = AnfExpr::Match {
        scrutinee: "result".to_string(),
        arms: vec![
            AnfMatchArm {
                pattern: "Ok(v)".to_string(),
                body: AnfExpr::Var("v".to_string()),
            },
            AnfMatchArm {
                pattern: "Err(e)".to_string(),
                body: AnfExpr::Var("e".to_string()),
            },
        ],
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, expr, "AnfExpr::Match must survive CBOR round-trip");
}

// G20: AnfExpr::Lambda — params and body are correct.
#[test]
fn anf_lambda_fields_are_correct() {
    let expr = AnfExpr::Lambda {
        params: vec!["x".to_string(), "y".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Var("x".to_string())),
    };
    if let AnfExpr::Lambda {
        params,
        body,
        captures,
    } = &expr
    {
        assert_eq!(params, &["x", "y"]);
        assert!(captures.is_empty());
        assert_eq!(**body, AnfExpr::Var("x".to_string()));
    } else {
        panic!("expected Lambda variant");
    }
}

// G20: AnfExpr::Lambda CBOR round-trip.
#[test]
fn anf_lambda_cbor_round_trip() {
    let expr = AnfExpr::Lambda {
        params: vec!["a".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::Lambda must survive CBOR round-trip"
    );
}

// G20: AnfExpr::RecordNew CBOR round-trip.
#[test]
fn anf_record_new_cbor_round_trip() {
    let expr = AnfExpr::RecordNew {
        fields: vec![
            (
                "name".to_string(),
                AnfExpr::Literal(LiteralValue::Text("Alice".to_string())),
            ),
            ("age".to_string(), AnfExpr::Literal(LiteralValue::Int(30))),
        ],
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::RecordNew must survive CBOR round-trip"
    );
}

// G20: AnfExpr::FieldUpdate — record is an atomic String.
#[test]
fn anf_field_update_record_is_atomic_string() {
    let expr = AnfExpr::FieldUpdate {
        record: "order".to_string(),
        field: "status".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Text("Paid".to_string()))),
    };
    if let AnfExpr::FieldUpdate { record, field, .. } = &expr {
        assert_eq!(record, "order");
        assert_eq!(field, "status");
    } else {
        panic!("expected FieldUpdate variant");
    }
}

// G20: AnfExpr::FieldUpdate CBOR round-trip.
#[test]
fn anf_field_update_cbor_round_trip() {
    let expr = AnfExpr::FieldUpdate {
        record: "rec".to_string(),
        field: "x".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::FieldUpdate must survive CBOR round-trip"
    );
}

// G20: AnfExpr::TupleNew CBOR round-trip.
#[test]
fn anf_tuple_new_cbor_round_trip() {
    let expr = AnfExpr::TupleNew(vec![
        AnfExpr::Literal(LiteralValue::Int(1)),
        AnfExpr::Literal(LiteralValue::Bool(false)),
    ]);
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::TupleNew must survive CBOR round-trip"
    );
}

// G20: AnfExpr::VariantNew with payload CBOR round-trip.
#[test]
fn anf_variant_new_with_payload_cbor_round_trip() {
    let expr = AnfExpr::VariantNew {
        tag: "Some".to_string(),
        payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::VariantNew with payload must survive CBOR round-trip"
    );
}

// G20: AnfExpr::VariantNew without payload CBOR round-trip.
#[test]
fn anf_variant_new_no_payload_cbor_round_trip() {
    let expr = AnfExpr::VariantNew {
        tag: "None".to_string(),
        payload: None,
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::VariantNew without payload must survive CBOR round-trip"
    );
}

// G20: AnfExpr::ListNew CBOR round-trip.
#[test]
fn anf_list_new_cbor_round_trip() {
    let expr = AnfExpr::ListNew(vec![
        AnfExpr::Literal(LiteralValue::Int(1)),
        AnfExpr::Literal(LiteralValue::Int(2)),
        AnfExpr::Literal(LiteralValue::Int(3)),
    ]);
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "AnfExpr::ListNew must survive CBOR round-trip"
    );
}

// If.cond is a String (atomic), not a nested AnfExpr.
#[test]
fn anf_if_cond_is_atomic_string() {
    let expr = AnfExpr::If {
        cond: "my_flag".to_string(),
        then_branch: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        else_branch: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
    };
    if let AnfExpr::If { cond, .. } = expr {
        assert_eq!(cond, "my_flag");
    } else {
        panic!("expected If variant");
    }
}

// Call.args are Vec<String> (atomic names), not nested expressions.
#[test]
fn anf_call_args_are_atomic_strings() {
    let expr = AnfExpr::Call {
        func: "fn.checkout".to_string(),
        args: vec!["cart_id".to_string(), "user_id".to_string()],
    };
    if let AnfExpr::Call { func, args } = expr {
        assert_eq!(func, "fn.checkout");
        assert_eq!(args, vec!["cart_id", "user_id"]);
    } else {
        panic!("expected Call variant");
    }
}

// ── AnfBinding ────────────────────────────────────────────────────────

// Scenario: AnfBinding preserves its source_ref provenance.
// Spec: "every AnfBinding.source_ref matches origin NodeRef"
#[test]
fn anf_binding_preserves_source_ref() {
    let binding = AnfBinding {
        source_ref: NodeRef(7),
        name: "fn_x".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Unit),
    };
    assert_eq!(
        binding.source_ref,
        NodeRef(7),
        "source_ref must be preserved verbatim"
    );
}

// Scenario: AnfBinding with Let expr is constructible.
#[test]
fn anf_binding_with_let_expr() {
    let binding = AnfBinding {
        source_ref: NodeRef(5),
        name: "fn_checkout".to_string(),
        expr: AnfExpr::Let {
            name: "cart".to_string(),
            value: Box::new(AnfExpr::Call {
                func: "db.read".to_string(),
                args: vec!["cart_id".to_string()],
            }),
            body: Box::new(AnfExpr::Var("cart".to_string())),
        },
    };
    assert_eq!(binding.source_ref, NodeRef(5));
    assert_eq!(binding.name, "fn_checkout");
}

// Scenario: AnfIr is constructible with bindings and stage hashes.
#[test]
fn anf_ir_is_constructible() {
    let bindings = vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "mod_root".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Unit),
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn_main".to_string(),
            expr: AnfExpr::Placeholder,
        },
    ];
    let source_map = crate::anf::SourceMap::from_bindings(&bindings);
    let ir = AnfIr {
        schema_version: crate::anf::ANF_SCHEMA_VERSION,
        bindings,
        source_map,
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    };
    assert_eq!(ir.bindings.len(), 2);
    assert!(ir.stage_hashes.anf_ir_hash.is_some());
}

// TRIANGULATE: stable_cbor_bytes on Vec<AnfBinding> is deterministic.
#[test]
fn anf_binding_list_cbor_is_deterministic() {
    let bindings = vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "a".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(1)),
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "b".to_string(),
            expr: AnfExpr::Var("a".to_string()),
        },
        AnfBinding {
            source_ref: NodeRef(2),
            name: "c".to_string(),
            expr: AnfExpr::Placeholder,
        },
    ];
    let b1 = stable_cbor_bytes(&bindings).expect("first encode");
    let b2 = stable_cbor_bytes(&bindings).expect("second encode");
    assert_eq!(b1, b2, "Vec<AnfBinding> must produce identical CBOR bytes");
}

// TRIANGULATE: different binding lists produce different CBOR bytes.
#[test]
fn different_anf_binding_lists_produce_different_cbor() {
    let list_a = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "x".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(1)),
    }];
    let list_b = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "x".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(2)),
    }];
    let b_a = stable_cbor_bytes(&list_a).expect("encode a");
    let b_b = stable_cbor_bytes(&list_b).expect("encode b");
    assert_ne!(
        b_a, b_b,
        "different AnfBinding lists must produce different CBOR"
    );
}

// ── G32: SourceMapEntry typed ref fields ──────────────────────────────

// Spec: SourceMapEntry uses typed ref newtypes for provenance fields.
// RED: written after types exist; GREEN from the start of this change.
#[test]
fn source_map_entry_with_typed_refs_is_constructible() {
    use ail_core::semantic_graph::{
        BlockRef, ContractRef, EffectRef, ProofObligationRef, RuntimeCheckRef,
    };

    let entry = SourceMapEntry {
        binding_name: "fn_checkout".to_string(),
        node_id: NodeRef(0),
        block_ref: Some(BlockRef("block_checkout".to_string())),
        change_set: Some("change.add_checkout".to_string()),
        contract_ref: Some(ContractRef("contract.payment".to_string())),
        effect_ref: Some(EffectRef("effect.db.read".to_string())),
        proof_obligation_ref: Some(ProofObligationRef("proof.no_negative_balance".to_string())),
        runtime_check_ref: Some(RuntimeCheckRef("rtcheck.null_guard".to_string())),
        wasm_offset: None,
        native_offset: None,
    };
    assert_eq!(entry.block_ref.as_ref().unwrap().0, "block_checkout");
    assert_eq!(entry.contract_ref.as_ref().unwrap().0, "contract.payment");
    assert_eq!(entry.effect_ref.as_ref().unwrap().0, "effect.db.read");
    assert_eq!(
        entry.proof_obligation_ref.as_ref().unwrap().0,
        "proof.no_negative_balance"
    );
    assert_eq!(
        entry.runtime_check_ref.as_ref().unwrap().0,
        "rtcheck.null_guard"
    );
}

// TRIANGULATE: SourceMapEntry with typed refs survives CBOR round-trip.
#[test]
fn source_map_entry_typed_refs_cbor_round_trip() {
    use ail_core::semantic_graph::{BlockRef, ContractRef};

    let entry = SourceMapEntry {
        binding_name: "fn_pay".to_string(),
        node_id: NodeRef(3),
        block_ref: Some(BlockRef("block_pay".to_string())),
        change_set: Some("change.add_payment".to_string()),
        contract_ref: Some(ContractRef("contract.payment.verify".to_string())),
        effect_ref: None,
        proof_obligation_ref: None,
        runtime_check_ref: None,
        wasm_offset: None,
        native_offset: None,
    };
    let bytes = stable_cbor_bytes(&entry).expect("encode");
    let decoded: SourceMapEntry = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, entry,
        "SourceMapEntry with typed refs must survive CBOR round-trip"
    );
}

// Spec: SourceMap from_bindings builds entries with None for all optional fields.
#[test]
fn source_map_from_bindings_sets_all_optional_fields_to_none() {
    let bindings = vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_a".to_string(),
            expr: AnfExpr::Placeholder,
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn_b".to_string(),
            expr: AnfExpr::Placeholder,
        },
    ];
    let sm = SourceMap::from_bindings(&bindings);
    assert_eq!(sm.entries.len(), 2);
    for entry in &sm.entries {
        assert!(
            entry.block_ref.is_none(),
            "block_ref must be None from from_bindings"
        );
        assert!(
            entry.change_set.is_none(),
            "change_set must be None from from_bindings"
        );
        assert!(
            entry.contract_ref.is_none(),
            "contract_ref must be None from from_bindings"
        );
        assert!(
            entry.effect_ref.is_none(),
            "effect_ref must be None from from_bindings"
        );
        assert!(
            entry.proof_obligation_ref.is_none(),
            "proof_obligation_ref must be None from from_bindings"
        );
        assert!(
            entry.runtime_check_ref.is_none(),
            "runtime_check_ref must be None from from_bindings"
        );
        assert!(
            entry.wasm_offset.is_none(),
            "wasm_offset must be None from from_bindings"
        );
        assert!(
            entry.native_offset.is_none(),
            "native_offset must be None from from_bindings"
        );
    }
}

// Spec: source_map has one entry per binding (including synthetic ones).
#[test]
fn source_map_preserves_duplicate_node_refs_for_synthetic_bindings() {
    // Two bindings with the same source_ref simulate G20 synthetic expansion.
    let bindings = vec![
        AnfBinding {
            source_ref: NodeRef(5),
            name: "fn_x".to_string(),
            expr: AnfExpr::Placeholder,
        },
        AnfBinding {
            source_ref: NodeRef(5), // duplicate NodeRef (synthetic)
            name: "anf_0".to_string(),
            expr: AnfExpr::Placeholder,
        },
    ];
    let sm = SourceMap::from_bindings(&bindings);
    assert_eq!(
        sm.entries.len(),
        2,
        "duplicate NodeRefs must NOT be collapsed"
    );
    assert_eq!(sm.entries[0].node_id, NodeRef(5));
    assert_eq!(sm.entries[1].node_id, NodeRef(5));
    assert_eq!(sm.entries[0].binding_name, "fn_x");
    assert_eq!(sm.entries[1].binding_name, "anf_0");
}

// Spec: empty input yields empty source map.
#[test]
fn source_map_from_empty_bindings_is_empty() {
    let sm = SourceMap::from_bindings(&[]);
    assert!(
        sm.entries.is_empty(),
        "empty bindings must produce empty source map"
    );
}

// Scenario: source_ref is not dropped when name is the same.
#[test]
fn anf_binding_distinct_refs_are_not_equal() {
    let b1 = AnfBinding {
        source_ref: NodeRef(3),
        name: "shared_name".to_string(),
        expr: AnfExpr::Placeholder,
    };
    let b2 = AnfBinding {
        source_ref: NodeRef(4),
        name: "shared_name".to_string(),
        expr: AnfExpr::Placeholder,
    };
    assert_ne!(b1, b2, "bindings with different NodeRefs must not be equal");
}

// S11: CBOR round-trip for AnfBinding with Let expr is lossless.
#[test]
fn anf_binding_cbor_round_trip_with_let_expr() {
    let binding = AnfBinding {
        source_ref: NodeRef(9),
        name: "fn_round_trip".to_string(),
        expr: AnfExpr::Let {
            name: "tmp".to_string(),
            value: Box::new(AnfExpr::Call {
                func: "fn.add".to_string(),
                args: vec!["x".to_string(), "y".to_string()],
            }),
            body: Box::new(AnfExpr::Var("tmp".to_string())),
        },
    };
    let bytes = stable_cbor_bytes(&binding).expect("encode must succeed");
    let decoded: AnfBinding = ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
    assert_eq!(
        decoded, binding,
        "AnfBinding with Let expr must survive CBOR round-trip"
    );
}
