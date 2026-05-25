// ── ail-compiler::anf_lowering_core ──────────────────────────────────────
//
// G3 (anf-real): Core ANF lowering tests — S1 through S12, full pipeline,
// schema version, and source-map API.
//
// Spec scenarios covered:
//   S1  — Literal lowers trivially (no let-bindings)
//   S2  — Var lowers trivially
//   S3  — Let with literal value lowers structurally
//   S4  — Call with atomic (Var) args lowers correctly
//   S5  — Nested Call arg gets let-bound before use
//   S6  — FieldGet with atomic record (Var) lowers correctly
//   S7  — FieldGet with non-atomic record (Call) gets record let-bound
//   S8  — If with Var cond lowers correctly
//   S9  — CoreNode without expr → AnfExpr::Literal(Unit) (backward compat)
//   S10 — source_ref is preserved verbatim through lowering
//   S11 — CBOR round-trip with Let expr is lossless
//   S12 — Different AnfExpr payloads → different anf_ir_hash
//   G20 R2-S1..S4 — schema_version and source_map API

mod anf_lowering_helpers;
use anf_lowering_helpers::{
    core_ir_with_expr, lower_and_collect, lower_expr, one_fn_graph, proven_report,
};

use ail_compiler::anf::{ANF_SCHEMA_VERSION, SourceMap};
use ail_compiler::hash::stable_cbor_bytes;
use ail_compiler::{
    AnfBinding, AnfExpr, CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue,
    MatchArm, StageHashes, lower_to_anf, lower_to_core_ir,
};
use ail_core::semantic_graph::NodeRef;

// ── S1: Literal lowers trivially ─────────────────────────────────────────

#[test]
fn literal_int_lowers_to_anf_literal() {
    let expr = CoreExpr::Literal(LiteralValue::Int(42));
    let anf = lower_expr(&expr);
    assert_eq!(anf, AnfExpr::Literal(LiteralValue::Int(42)));
}

#[test]
fn literal_bool_lowers_to_anf_literal() {
    let expr = CoreExpr::Literal(LiteralValue::Bool(true));
    let anf = lower_expr(&expr);
    assert_eq!(anf, AnfExpr::Literal(LiteralValue::Bool(true)));
}

#[test]
fn literal_unit_lowers_to_anf_literal() {
    let expr = CoreExpr::Literal(LiteralValue::Unit);
    let anf = lower_expr(&expr);
    assert_eq!(anf, AnfExpr::Literal(LiteralValue::Unit));
}

// S1 triangulation: no synthetic bindings emitted for a plain literal.
#[test]
fn literal_produces_no_synthetic_bindings() {
    let expr = CoreExpr::Literal(LiteralValue::Int(99));
    let (synth, _) = lower_and_collect(&expr);
    assert!(
        synth.is_empty(),
        "plain literal must produce zero synthetic bindings"
    );
}

#[test]
fn loop_break_continue_lower_to_anf_loop_variants() {
    let expr = CoreExpr::Loop {
        body: Box::new(CoreExpr::Break {
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(10))),
        }),
        termination: None,
    };

    let anf = lower_expr(&expr);

    assert_eq!(
        anf,
        AnfExpr::Loop {
            body: Box::new(AnfExpr::Break {
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            }),
        }
    );
    assert_eq!(lower_expr(&CoreExpr::Continue), AnfExpr::Continue);
}

// Wave 21A: WhileLoop with a computed condition desugars into
// Seq([Loop { body: Let { cond_tmp = cond_expr, If { ... } } }, Literal(Unit)])
// so the condition is re-evaluated on every iteration.
//
// The condition is NO LONGER atomized into an outer binding (no `out` entry).
// Instead it lives inside the Loop body as a Let, which is re-run each iteration.
#[test]
fn while_loop_computed_condition_desugars_into_loop_if() {
    let expr = CoreExpr::WhileLoop {
        cond: Box::new(CoreExpr::Call {
            func: "is_ready".to_string(),
            args: vec![],
        }),
        body: Box::new(CoreExpr::Continue),
        termination: None,
    };

    let (synth, root) = lower_and_collect(&expr);

    // No bindings pushed to the outer `out` — the condition lives inside the loop.
    assert!(
        synth.is_empty(),
        "desugared WhileLoop must push no outer synthetic bindings; got {synth:?}"
    );

    // Root must be Seq([Loop { ... }, Literal(Unit)]).
    let AnfExpr::Seq(seq) = root else {
        panic!("desugared WhileLoop must be AnfExpr::Seq, got {root:?}");
    };
    assert_eq!(
        seq.len(),
        2,
        "Seq must have 2 elements: Loop + Literal(Unit)"
    );
    assert!(
        matches!(seq[1], AnfExpr::Literal(LiteralValue::Unit)),
        "second Seq element must be Literal(Unit)"
    );

    // The first element must be a Loop.
    let AnfExpr::Loop { body } = &seq[0] else {
        panic!("first Seq element must be AnfExpr::Loop, got {:?}", seq[0]);
    };

    // The Loop body must be a Let whose value is the lowered condition expression.
    let AnfExpr::Let {
        name: cond_tmp,
        value: cond_expr,
        body: if_body,
    } = body.as_ref()
    else {
        panic!("Loop body must be AnfExpr::Let (condition binding), got {body:?}");
    };

    // The condition expression must be the Call to "is_ready".
    assert!(
        matches!(cond_expr.as_ref(), AnfExpr::Call { func, .. } if func == "is_ready"),
        "condition Let value must be Call to is_ready; got {cond_expr:?}"
    );

    // The If must branch on the condition variable.
    let AnfExpr::If {
        cond: if_cond,
        then_branch,
        else_branch,
    } = if_body.as_ref()
    else {
        panic!("Loop body's Let body must be AnfExpr::If, got {if_body:?}");
    };
    assert_eq!(
        if_cond, cond_tmp,
        "If condition must reference the bound condition temp"
    );

    // then_branch: Let { body_tmp = body_lowered, Continue }
    assert!(
        matches!(then_branch.as_ref(), AnfExpr::Let { body, .. } if matches!(body.as_ref(), AnfExpr::Continue)),
        "then_branch must be Let {{ ..., body: Continue }}; got {then_branch:?}"
    );

    // else_branch: Break { value: Literal(Unit) }
    assert!(
        matches!(
            else_branch.as_ref(),
            AnfExpr::Break { value } if matches!(value.as_ref(), AnfExpr::Literal(LiteralValue::Unit))
        ),
        "else_branch must be Break {{ value: Literal(Unit) }}; got {else_branch:?}"
    );
}

// ── S2: Var lowers trivially ─────────────────────────────────────────────

#[test]
fn var_lowers_to_anf_var() {
    let expr = CoreExpr::Var("my_var".to_string());
    let anf = lower_expr(&expr);
    assert_eq!(anf, AnfExpr::Var("my_var".to_string()));
}

// S2 triangulation: no synthetic bindings emitted for a plain var.
#[test]
fn var_produces_no_synthetic_bindings() {
    let expr = CoreExpr::Var("x".to_string());
    let (synth, _) = lower_and_collect(&expr);
    assert!(
        synth.is_empty(),
        "plain var must produce zero synthetic bindings"
    );
}

// ── S3: Let with literal value lowers structurally ───────────────────────

#[test]
fn let_with_literal_value_lowers_to_anf_let() {
    let expr = CoreExpr::Let {
        name: "y".to_string(),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(5))),
        body: Box::new(CoreExpr::Var("y".to_string())),
    };
    let anf = lower_expr(&expr);
    assert_eq!(
        anf,
        AnfExpr::Let {
            name: "y".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(5))),
            body: Box::new(AnfExpr::Var("y".to_string())),
        }
    );
}

// ── S4: Call with atomic (Var) args lowers correctly ─────────────────────

#[test]
fn call_with_var_args_lowers_to_anf_call_with_same_names() {
    let expr = CoreExpr::Call {
        func: "fn.add".to_string(),
        args: vec![
            CoreExpr::Var("x".to_string()),
            CoreExpr::Var("y".to_string()),
        ],
    };
    let anf = lower_expr(&expr);
    assert_eq!(
        anf,
        AnfExpr::Call {
            func: "fn.add".to_string(),
            args: vec!["x".to_string(), "y".to_string()],
        }
    );
}

// S4 triangulation: no synthetic bindings for already-atomic args.
#[test]
fn call_with_var_args_produces_no_synthetic_bindings() {
    let expr = CoreExpr::Call {
        func: "fn.checkout".to_string(),
        args: vec![
            CoreExpr::Var("cart_id".to_string()),
            CoreExpr::Var("user".to_string()),
        ],
    };
    let (synth, _) = lower_and_collect(&expr);
    assert!(
        synth.is_empty(),
        "call with Var args must produce no synthetic bindings"
    );
}

// ── S5: Nested Call arg gets let-bound before use ─────────────────────────

// CoreExpr::Call { func: "fn.outer", args: [Call{"fn.inner", [Var("a")]}]}
// → let anf_0 = Call{"fn.inner", ["a"]} in Call{"fn.outer", ["anf_0"]}
#[test]
fn nested_call_arg_gets_let_bound() {
    let inner = CoreExpr::Call {
        func: "fn.inner".to_string(),
        args: vec![CoreExpr::Var("a".to_string())],
    };
    let outer = CoreExpr::Call {
        func: "fn.outer".to_string(),
        args: vec![inner],
    };
    let (synth, root) = lower_and_collect(&outer);

    // One synthetic binding for the inner call result.
    assert_eq!(synth.len(), 1, "one synthetic binding for the inner call");
    let syn_binding = &synth[0];
    assert_eq!(
        syn_binding.expr,
        AnfExpr::Call {
            func: "fn.inner".to_string(),
            args: vec!["a".to_string()],
        }
    );
    // Root call uses the synthetic name as its arg.
    let tmp_name = syn_binding.name.clone();
    assert_eq!(
        root,
        AnfExpr::Call {
            func: "fn.outer".to_string(),
            args: vec![tmp_name],
        }
    );
}

// Triangulate: two nested call args → two synthetic bindings.
#[test]
fn two_nested_call_args_produce_two_synthetic_bindings() {
    let inner_a = CoreExpr::Call {
        func: "fn.a".to_string(),
        args: vec![CoreExpr::Var("x".to_string())],
    };
    let inner_b = CoreExpr::Call {
        func: "fn.b".to_string(),
        args: vec![CoreExpr::Var("y".to_string())],
    };
    let outer = CoreExpr::Call {
        func: "fn.combine".to_string(),
        args: vec![inner_a, inner_b],
    };
    let (synth, root) = lower_and_collect(&outer);

    assert_eq!(
        synth.len(),
        2,
        "two synthetic bindings for two nested calls"
    );
    if let AnfExpr::Call { args, .. } = root {
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], synth[0].name);
        assert_eq!(args[1], synth[1].name);
    } else {
        panic!("root must be AnfExpr::Call");
    }
}

// ── S6: FieldGet with atomic record (Var) lowers correctly ───────────────

#[test]
fn field_get_with_var_record_lowers_to_anf_field_get() {
    let expr = CoreExpr::FieldGet {
        record: Box::new(CoreExpr::Var("order".to_string())),
        field: "total".to_string(),
    };
    let anf = lower_expr(&expr);
    assert_eq!(
        anf,
        AnfExpr::FieldGet {
            record: "order".to_string(),
            field: "total".to_string(),
        }
    );
}

// S6 triangulation: no synthetic bindings for Var record.
#[test]
fn field_get_with_var_record_produces_no_synthetic_bindings() {
    let expr = CoreExpr::FieldGet {
        record: Box::new(CoreExpr::Var("rec".to_string())),
        field: "field_a".to_string(),
    };
    let (synth, _) = lower_and_collect(&expr);
    assert!(
        synth.is_empty(),
        "FieldGet with Var record must produce no synthetic bindings"
    );
}

// ── S7: FieldGet with non-atomic record gets record let-bound ─────────────

#[test]
fn field_get_with_call_record_gets_record_let_bound() {
    let call_expr = CoreExpr::Call {
        func: "db.read".to_string(),
        args: vec![CoreExpr::Var("id".to_string())],
    };
    let expr = CoreExpr::FieldGet {
        record: Box::new(call_expr),
        field: "amount".to_string(),
    };
    let (synth, root) = lower_and_collect(&expr);

    assert_eq!(
        synth.len(),
        1,
        "one synthetic binding for the non-atomic record"
    );
    let tmp_name = synth[0].name.clone();
    assert_eq!(
        root,
        AnfExpr::FieldGet {
            record: tmp_name,
            field: "amount".to_string(),
        }
    );
}

// ── S8: If with Var cond lowers correctly ────────────────────────────────

#[test]
fn if_with_var_cond_lowers_to_anf_if() {
    let expr = CoreExpr::If {
        cond: Box::new(CoreExpr::Var("flag".to_string())),
        then_: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        else_: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let anf = lower_expr(&expr);
    assert_eq!(
        anf,
        AnfExpr::If {
            cond: "flag".to_string(),
            then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        }
    );
}

// S8 triangulation: no synthetic bindings for Var cond.
#[test]
fn if_with_var_cond_produces_no_synthetic_bindings() {
    let expr = CoreExpr::If {
        cond: Box::new(CoreExpr::Var("ok".to_string())),
        then_: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        else_: Box::new(CoreExpr::Literal(LiteralValue::Bool(false))),
    };
    let (synth, _) = lower_and_collect(&expr);
    assert!(
        synth.is_empty(),
        "If with Var cond must produce no synthetic bindings"
    );
}

// If with non-atomic cond: cond gets let-bound.
#[test]
fn if_with_call_cond_gets_cond_let_bound() {
    let cond = CoreExpr::Call {
        func: "fn.is_valid".to_string(),
        args: vec![CoreExpr::Var("x".to_string())],
    };
    let expr = CoreExpr::If {
        cond: Box::new(cond),
        then_: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        else_: Box::new(CoreExpr::Literal(LiteralValue::Bool(false))),
    };
    let (synth, root) = lower_and_collect(&expr);

    assert_eq!(synth.len(), 1, "one synthetic binding for non-atomic cond");
    let tmp_name = synth[0].name.clone();
    if let AnfExpr::If { cond, .. } = root {
        assert_eq!(
            cond, tmp_name,
            "If.cond must reference the synthetic binding"
        );
    } else {
        panic!("root must be AnfExpr::If");
    }
}

#[test]
fn lower_to_anf_keeps_match_scrutinee_binding_local() {
    let core = core_ir_with_expr(
        NodeRef(0),
        "fn_match",
        CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Literal(LiteralValue::Int(2))),
            arms: vec![MatchArm {
                pattern: "_".to_string(),
                body: CoreExpr::Literal(LiteralValue::Int(20)),
            }],
        },
    );

    let anf = lower_to_anf(&core).expect("lower_to_anf must succeed");

    match &anf.bindings[0].expr {
        AnfExpr::Let { name, body, .. } => {
            assert_eq!(name, "anf_0");
            assert!(
                matches!(**body, AnfExpr::Match { ref scrutinee, .. } if scrutinee == "anf_0"),
                "match scrutinee must reference the local let binding"
            );
        }
        other => panic!("expected local let around match scrutinee, got {other:?}"),
    }
}

// ── S9: Backward compat — nodes without CoreExpr get Literal(Unit) ────────

#[test]
fn core_node_without_expr_produces_literal_unit() {
    let core = CoreIr {
        nodes: vec![
            CoreNode {
                source_ref: NodeRef(0),
                kind: CoreNodeKind::Module,
                name: "my_module".to_string(),
                ty: None,
                expr: None,
            },
            CoreNode {
                source_ref: NodeRef(1),
                kind: CoreNodeKind::Function,
                name: "fn_stub".to_string(),
                ty: Some(CoreType::Function {
                    params: vec![],
                    ret: Box::new(CoreType::Generic(None)),
                    effects: vec![],
                }),
                expr: None,
            },
        ],
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
    let anf = lower_to_anf(&core).expect("lower_to_anf must succeed");

    // Both nodes have expr: None → both get AnfExpr::Literal(Unit).
    for binding in &anf.bindings {
        assert_eq!(
            binding.expr,
            AnfExpr::Literal(LiteralValue::Unit),
            "binding '{}' must have AnfExpr::Literal(Unit) when CoreNode.expr is None",
            binding.name
        );
    }
}

// ── S10: source_ref preserved through lowering ────────────────────────────

#[test]
fn source_ref_preserved_through_anf_lowering() {
    let core = core_ir_with_expr(
        NodeRef(42),
        "fn_provenance",
        CoreExpr::Call {
            func: "fn.add".to_string(),
            args: vec![
                CoreExpr::Var("a".to_string()),
                CoreExpr::Var("b".to_string()),
            ],
        },
    );
    let anf = lower_to_anf(&core).expect("lower_to_anf must succeed");

    // The main binding must carry NodeRef(42).
    let main = anf
        .bindings
        .iter()
        .find(|b| b.name == "fn_provenance")
        .expect("fn_provenance binding must exist");
    assert_eq!(
        main.source_ref,
        NodeRef(42),
        "source_ref must be preserved verbatim through ANF lowering"
    );
}

// Triangulate: synthetic bindings for nested exprs carry the same source_ref.
#[test]
fn synthetic_bindings_carry_same_source_ref() {
    let inner = CoreExpr::Call {
        func: "fn.inner".to_string(),
        args: vec![CoreExpr::Var("x".to_string())],
    };
    let outer = CoreExpr::Call {
        func: "fn.outer".to_string(),
        args: vec![inner],
    };
    let core = core_ir_with_expr(NodeRef(7), "fn_nested", outer);
    let anf = lower_to_anf(&core).expect("lower_to_anf must succeed");

    // All bindings (synthetic + main) must carry NodeRef(7).
    for binding in &anf.bindings {
        assert_eq!(
            binding.source_ref,
            NodeRef(7),
            "all bindings from one CoreNode must carry its source_ref"
        );
    }
}

// ── S11: CBOR round-trip with Let expr ───────────────────────────────────

#[test]
fn anf_ir_cbor_round_trip_with_let_expr() {
    let core = core_ir_with_expr(
        NodeRef(3),
        "fn_let",
        CoreExpr::Let {
            name: "tmp".to_string(),
            value: Box::new(CoreExpr::Call {
                func: "fn.get".to_string(),
                args: vec![CoreExpr::Var("id".to_string())],
            }),
            body: Box::new(CoreExpr::Var("tmp".to_string())),
        },
    );
    let anf = lower_to_anf(&core).expect("lower_to_anf must succeed");

    // Encode and decode the bindings list.
    let bytes = stable_cbor_bytes(&anf.bindings).expect("stable_cbor_bytes must succeed");
    let decoded: Vec<AnfBinding> =
        ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");

    assert_eq!(
        decoded, anf.bindings,
        "Vec<AnfBinding> with Let expr must survive CBOR round-trip"
    );
}

// ── S12: Different AnfExpr payloads → different anf_ir_hash ──────────────

#[test]
fn different_anf_expr_payloads_produce_different_hashes() {
    let core_a = core_ir_with_expr(NodeRef(0), "fn_a", CoreExpr::Literal(LiteralValue::Int(1)));
    let core_b = core_ir_with_expr(NodeRef(0), "fn_a", CoreExpr::Literal(LiteralValue::Int(2)));

    let anf_a = lower_to_anf(&core_a).expect("lower_to_anf a");
    let anf_b = lower_to_anf(&core_b).expect("lower_to_anf b");

    assert_ne!(
        anf_a.stage_hashes.anf_ir_hash, anf_b.stage_hashes.anf_ir_hash,
        "different AnfExpr payloads must produce different anf_ir_hash"
    );
}

// Triangulate: same expr → same hash (determinism).
#[test]
fn same_anf_expr_payload_produces_same_hash() {
    let core = core_ir_with_expr(
        NodeRef(0),
        "fn_determ",
        CoreExpr::Call {
            func: "fn.add".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("y".to_string()),
            ],
        },
    );
    let anf1 = lower_to_anf(&core).expect("anf 1");
    let anf2 = lower_to_anf(&core).expect("anf 2");

    assert_eq!(
        anf1.stage_hashes.anf_ir_hash, anf2.stage_hashes.anf_ir_hash,
        "same CoreIr must produce the same anf_ir_hash (determinism)"
    );
}

// ── Full pipeline with CoreExpr ───────────────────────────────────────────

// Verify the full pipeline still works when CoreNodes carry a CoreExpr.
#[test]
fn full_pipeline_with_core_expr_succeeds() {
    let graph = one_fn_graph();
    let report = proven_report();
    let mut core = lower_to_core_ir(&graph, &report).expect("core must succeed");

    // Inject a CoreExpr into the single node.
    core.nodes[0].expr = Some(CoreExpr::Call {
        func: "fn.init".to_string(),
        args: vec![CoreExpr::Var("ctx".to_string())],
    });

    let anf = lower_to_anf(&core).expect("anf must succeed");
    assert!(anf.stage_hashes.anf_ir_hash.is_some());
    assert!(!anf.bindings.is_empty());

    // Find the binding for "fn_test" and assert its expr is a Call.
    let fn_binding = anf
        .bindings
        .iter()
        .find(|b| b.name == "fn_test")
        .expect("fn_test binding must exist");
    assert_eq!(
        fn_binding.expr,
        AnfExpr::Call {
            func: "fn.init".to_string(),
            args: vec!["ctx".to_string()],
        }
    );
}

// ── G20 R2: schema version and source map ─────────────────────────────────

// R2-S1: lower_to_anf sets schema_version to ANF_SCHEMA_VERSION.
#[test]
fn lower_to_anf_sets_schema_version() {
    let anf = lower_to_anf(&core_ir_with_expr(
        NodeRef(0),
        "fn_test",
        CoreExpr::Var("x".to_string()),
    ))
    .unwrap();
    assert_eq!(
        anf.schema_version, ANF_SCHEMA_VERSION,
        "schema_version must equal ANF_SCHEMA_VERSION"
    );
}

// R2-S2: lower_to_anf populates source_map with one entry per binding.
#[test]
fn lower_to_anf_populates_source_map() {
    let anf = lower_to_anf(&core_ir_with_expr(
        NodeRef(7),
        "fn_check",
        CoreExpr::Literal(LiteralValue::Int(1)),
    ))
    .unwrap();
    // At least one source map entry must exist (one per binding).
    assert!(
        !anf.source_map.entries.is_empty(),
        "source_map must have at least one entry"
    );
    // The first original-node entry must have node_id == NodeRef(7).
    // (Synthetic bindings may have different node_ids — all share source_ref of parent.)
    let has_node_7 = anf
        .source_map
        .entries
        .iter()
        .any(|e| e.node_id == NodeRef(7));
    assert!(
        has_node_7,
        "source_map must contain an entry with node_id == NodeRef(7)"
    );
}

// R2-S3: SourceMap::from_bindings maps binding names to node_ids.
#[test]
fn source_map_from_bindings_maps_names_to_node_ids() {
    let bindings = vec![
        AnfBinding {
            source_ref: NodeRef(10),
            name: "fn_a".to_string(),
            expr: AnfExpr::Placeholder,
        },
        AnfBinding {
            source_ref: NodeRef(20),
            name: "fn_b".to_string(),
            expr: AnfExpr::Placeholder,
        },
    ];
    let map = SourceMap::from_bindings(&bindings);
    assert_eq!(map.entries.len(), 2);
    assert_eq!(map.entries[0].binding_name, "fn_a");
    assert_eq!(map.entries[0].node_id, NodeRef(10));
    assert_eq!(map.entries[1].binding_name, "fn_b");
    assert_eq!(map.entries[1].node_id, NodeRef(20));
}

// R2-S4: SourceMapEntry optional fields are None by default.
#[test]
fn source_map_entry_optional_fields_default_none() {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(5),
        name: "fn_x".to_string(),
        expr: AnfExpr::Placeholder,
    }];
    let map = SourceMap::from_bindings(&bindings);
    let entry = &map.entries[0];
    assert!(entry.block_ref.is_none());
    assert!(entry.change_set.is_none());
    assert!(entry.contract_ref.is_none());
    assert!(entry.effect_ref.is_none());
    assert!(entry.proof_obligation_ref.is_none());
    assert!(entry.runtime_check_ref.is_none());
    assert!(entry.wasm_offset.is_none());
    assert!(entry.native_offset.is_none());
}
