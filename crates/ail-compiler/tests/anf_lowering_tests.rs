// ── ail-compiler::anf_lowering_tests ─────────────────────────────────────
//
// G3 (anf-real): Integration tests for ANF lowering from Core IR.
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

use ail_compiler::anf::{ANF_SCHEMA_VERSION, SourceMap};
use ail_compiler::hash::stable_cbor_bytes;
use ail_compiler::lower::lower_core_expr_to_anf;
use ail_compiler::{
    AnfExpr, CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, StageHashes,
    lower_to_anf, lower_to_core_ir,
};
#[allow(unused_imports)]
use ciborium;
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── Helpers ───────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn one_fn_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn_test")],
        edges: vec![],
    }
}

/// Build a `CoreIr` with a single node carrying the given `CoreExpr`.
fn core_ir_with_expr(source_ref: NodeRef, name: &str, expr: CoreExpr) -> CoreIr {
    CoreIr {
        nodes: vec![CoreNode {
            source_ref,
            kind: CoreNodeKind::Function,
            name: name.to_string(),
            ty: Some(CoreType::Function),
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
    }
}

/// Helper: lower a single `CoreExpr` and return the resulting `AnfExpr`.
/// Any synthetic bindings emitted during flattening are discarded — this
/// helper is only suitable for expressions that produce no temporaries
/// themselves (use `lower_and_collect` for the full binding list).
fn lower_expr(expr: &CoreExpr) -> AnfExpr {
    let mut fresh = 0u32;
    let mut out = Vec::new();
    lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out)
}

/// Lower a `CoreExpr` and return `(synthetic_bindings, root_anf_expr)`.
fn lower_and_collect(expr: &CoreExpr) -> (Vec<ail_compiler::AnfBinding>, AnfExpr) {
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let root = lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out);
    (out, root)
}

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
                ty: Some(CoreType::Function),
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
    use ail_compiler::AnfBinding;

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
    assert!(has_node_7, "source_map must contain an entry with node_id == NodeRef(7)");
}

// R2-S3: SourceMap::from_bindings maps binding names to node_ids.
#[test]
fn source_map_from_bindings_maps_names_to_node_ids() {
    use ail_compiler::AnfBinding;
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
    use ail_compiler::AnfBinding;
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

// ── G20 R2: short-circuit lowering ────────────────────────────────────────

// R2-S5: CoreExpr::And lowers to AnfExpr::ShortCircuitAnd.
// Left is atomized; right is a nested AnfExpr (lazy evaluation).
#[test]
fn and_lowers_to_short_circuit_and() {
    let expr = CoreExpr::And {
        left: Box::new(CoreExpr::Var("a".to_string())),
        right: Box::new(CoreExpr::Var("b".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var left must not produce synthetic bindings");
    match result {
        AnfExpr::ShortCircuitAnd { left, right } => {
            assert_eq!(left, "a");
            assert_eq!(*right, AnfExpr::Var("b".to_string()));
        }
        other => panic!("expected ShortCircuitAnd, got {other:?}"),
    }
}

// R2-S6: CoreExpr::Or lowers to AnfExpr::ShortCircuitOr.
#[test]
fn or_lowers_to_short_circuit_or() {
    let expr = CoreExpr::Or {
        left: Box::new(CoreExpr::Var("x".to_string())),
        right: Box::new(CoreExpr::Var("y".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ShortCircuitOr { left, right } => {
            assert_eq!(left, "x");
            assert_eq!(*right, AnfExpr::Var("y".to_string()));
        }
        other => panic!("expected ShortCircuitOr, got {other:?}"),
    }
}

// R2-S7: And with non-Var left → left is let-bound (atomized).
#[test]
fn and_with_complex_left_is_atomized() {
    let expr = CoreExpr::And {
        left: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        right: Box::new(CoreExpr::Var("b".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal left must be let-bound");
    match result {
        AnfExpr::ShortCircuitAnd { left, .. } => {
            assert!(left.starts_with("anf_"), "left must be synthetic: {left}");
        }
        other => panic!("expected ShortCircuitAnd, got {other:?}"),
    }
}

// ── G20 R2: EffectCall lowering ───────────────────────────────────────────

// R2-S8: CoreExpr::EffectCall lowers to AnfExpr::EffectCall with atomized args.
#[test]
fn effect_call_lowers_correctly() {
    let expr = CoreExpr::EffectCall {
        capability: "database".to_string(),
        func: "read".to_string(),
        args: vec![CoreExpr::Var("cart_id".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var arg must not produce bindings");
    match result {
        AnfExpr::EffectCall { capability, func, args } => {
            assert_eq!(capability, "database");
            assert_eq!(func, "read");
            assert_eq!(args, vec!["cart_id"]);
        }
        other => panic!("expected EffectCall, got {other:?}"),
    }
}

// R2-S9: EffectCall with non-Var arg atomizes it.
#[test]
fn effect_call_atomizes_non_var_args() {
    let expr = CoreExpr::EffectCall {
        capability: "payment".to_string(),
        func: "charge".to_string(),
        args: vec![CoreExpr::Literal(LiteralValue::Int(100))],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal arg must produce one synthetic binding");
    match result {
        AnfExpr::EffectCall { args, .. } => {
            assert!(args[0].starts_with("anf_"), "arg must be synthetic: {}", args[0]);
        }
        other => panic!("expected EffectCall, got {other:?}"),
    }
}

// ── G20 R2: Dispatch lowering ─────────────────────────────────────────────

// R2-S10: CoreExpr::Dispatch lowers to AnfExpr::Dispatch.
#[test]
fn dispatch_lowers_correctly() {
    let expr = CoreExpr::Dispatch {
        handler: "PaymentProvider".to_string(),
        method: "charge".to_string(),
        args: vec![CoreExpr::Var("amount".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::Dispatch { handler, method, args } => {
            assert_eq!(handler, "PaymentProvider");
            assert_eq!(method, "charge");
            assert_eq!(args, vec!["amount"]);
        }
        other => panic!("expected Dispatch, got {other:?}"),
    }
}

// ── G20 R2: TaskSpawn lowering ────────────────────────────────────────────

// R2-S11: CoreExpr::TaskSpawn lowers to AnfExpr::TaskSpawn.
#[test]
fn task_spawn_lowers_correctly() {
    let expr = CoreExpr::TaskSpawn {
        func: "worker.process".to_string(),
        args: vec![CoreExpr::Var("payload".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::TaskSpawn { func, args } => {
            assert_eq!(func, "worker.process");
            assert_eq!(args, vec!["payload"]);
        }
        other => panic!("expected TaskSpawn, got {other:?}"),
    }
}

// ── G20 R2: ChannelSend / ChannelReceive lowering ─────────────────────────

// R2-S12: CoreExpr::ChannelSend lowers to AnfExpr::ChannelSend (both atomic).
#[test]
fn channel_send_lowers_correctly() {
    let expr = CoreExpr::ChannelSend {
        channel: Box::new(CoreExpr::Var("ch".to_string())),
        value: Box::new(CoreExpr::Var("msg".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelSend { channel, value } => {
            assert_eq!(channel, "ch");
            assert_eq!(value, "msg");
        }
        other => panic!("expected ChannelSend, got {other:?}"),
    }
}

// R2-S13: CoreExpr::ChannelReceive lowers to AnfExpr::ChannelReceive.
#[test]
fn channel_recv_lowers_correctly() {
    let expr = CoreExpr::ChannelReceive {
        channel: Box::new(CoreExpr::Var("ch".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelReceive { channel } => {
            assert_eq!(channel, "ch");
        }
        other => panic!("expected ChannelReceive, got {other:?}"),
    }
}

// ── G20 R2: RuntimeCheck lowering ─────────────────────────────────────────

// R2-S14: CoreExpr::RuntimeCheck lowers to AnfExpr::RuntimeCheck.
// Contract checks MUST survive lowering.
#[test]
fn runtime_check_lowers_correctly() {
    let expr = CoreExpr::RuntimeCheck {
        check_ref: "contract.balance_non_negative".to_string(),
        cond: Box::new(CoreExpr::Var("is_valid".to_string())),
        msg: "balance must be non-negative".to_string(),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var cond must not produce synthetic bindings");
    match result {
        AnfExpr::RuntimeCheck { check_ref, cond, msg } => {
            assert_eq!(check_ref, "contract.balance_non_negative");
            assert_eq!(cond, "is_valid");
            assert_eq!(msg, "balance must be non-negative");
        }
        other => panic!("expected RuntimeCheck, got {other:?}"),
    }
}

// R2-S15: RuntimeCheck with non-Var cond atomizes it.
#[test]
fn runtime_check_atomizes_non_var_cond() {
    let expr = CoreExpr::RuntimeCheck {
        check_ref: "contract.positive".to_string(),
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        msg: "must be positive".to_string(),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal cond must be let-bound");
    match result {
        AnfExpr::RuntimeCheck { cond, .. } => {
            assert!(cond.starts_with("anf_"), "cond must be synthetic: {cond}");
        }
        other => panic!("expected RuntimeCheck, got {other:?}"),
    }
}

// ── G20 R2: Resource acquire/release ordering ─────────────────────────────

// R2-S16: CoreExpr::ResourceAcquire lowers to AnfExpr::ResourceAcquire.
#[test]
fn resource_acquire_lowers_correctly() {
    let expr = CoreExpr::ResourceAcquire {
        resource: "db.connection".to_string(),
        args: vec![CoreExpr::Var("conn_str".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ResourceAcquire { resource, args } => {
            assert_eq!(resource, "db.connection");
            assert_eq!(args, vec!["conn_str"]);
        }
        other => panic!("expected ResourceAcquire, got {other:?}"),
    }
}

// R2-S17: CoreExpr::ResourceRelease lowers to AnfExpr::ResourceRelease.
#[test]
fn resource_release_lowers_correctly() {
    let expr = CoreExpr::ResourceRelease {
        handle: Box::new(CoreExpr::Var("conn".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ResourceRelease { handle } => {
            assert_eq!(handle, "conn");
        }
        other => panic!("expected ResourceRelease, got {other:?}"),
    }
}

// R2-S18: ResourceRelease atomizes non-Var handle.
#[test]
fn resource_release_atomizes_non_var_handle() {
    // Non-Var handle: a Call that returns a handle
    let expr = CoreExpr::ResourceRelease {
        handle: Box::new(CoreExpr::Call {
            func: "db.get_handle".to_string(),
            args: vec![],
        }),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Call handle must be let-bound");
    match result {
        AnfExpr::ResourceRelease { handle } => {
            assert!(handle.starts_with("anf_"), "handle must be synthetic: {handle}");
        }
        other => panic!("expected ResourceRelease, got {other:?}"),
    }
}

// ── G20 R2: composite children full ANF normalization ─────────────────────

// R2-S19: RecordNew with Literal field — field is let-bound (atomized).
#[test]
fn record_new_literal_field_is_let_bound() {
    let expr = CoreExpr::RecordNew {
        fields: vec![("price".to_string(), CoreExpr::Literal(LiteralValue::Int(99)))],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal field must produce one synthetic binding");
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
    assert_eq!(out.len(), 2, "Two Literal elements → two synthetic bindings");
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
    assert_eq!(out.len(), 1, "Literal payload must produce one synthetic binding");
    match result {
        AnfExpr::VariantNew { payload, .. } => {
            assert!(matches!(*payload.unwrap(), AnfExpr::Var(_)));
        }
        other => panic!("expected VariantNew, got {other:?}"),
    }
}

// ── G23: new concurrency + cell primitives — lowering integration tests ──

// G23-S1: TaskAwait lowers correctly (Var task → no synthetic bindings).
#[test]
fn task_await_lowers_correctly() {
    let expr = CoreExpr::TaskAwait {
        task: Box::new(CoreExpr::Var("t0".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var task must not produce bindings");
    match result {
        AnfExpr::TaskAwait { task } => assert_eq!(task, "t0"),
        other => panic!("expected TaskAwait, got {other:?}"),
    }
}

// G23-S2: TaskCancel lowers correctly.
#[test]
fn task_cancel_lowers_correctly() {
    let expr = CoreExpr::TaskCancel {
        task: Box::new(CoreExpr::Var("t1".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::TaskCancel { task } => assert_eq!(task, "t1"),
        other => panic!("expected TaskCancel, got {other:?}"),
    }
}

// G23-S3: TaskGroup body is lowered recursively.
#[test]
fn task_group_body_lowered_recursively() {
    let expr = CoreExpr::TaskGroup {
        body: Box::new(CoreExpr::Call {
            func: "fn.work".to_string(),
            args: vec![CoreExpr::Var("ctx".to_string())],
        }),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var arg inside body must not produce extra bindings");
    match result {
        AnfExpr::TaskGroup { body } => {
            assert_eq!(
                *body,
                AnfExpr::Call {
                    func: "fn.work".to_string(),
                    args: vec!["ctx".to_string()],
                }
            );
        }
        other => panic!("expected TaskGroup, got {other:?}"),
    }
}

// G23-S4: ChannelNew unbounded — capacity None preserved.
#[test]
fn channel_new_unbounded_lowers_correctly() {
    let expr = CoreExpr::ChannelNew { capacity: None };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelNew { capacity } => assert!(capacity.is_none()),
        other => panic!("expected ChannelNew(None), got {other:?}"),
    }
}

// TRIANGULATE: ChannelNew bounded — capacity Some(n) preserved.
#[test]
fn channel_new_bounded_preserves_capacity() {
    let expr = CoreExpr::ChannelNew { capacity: Some(128) };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelNew { capacity } => assert_eq!(capacity, Some(128)),
        other => panic!("expected ChannelNew(Some(128)), got {other:?}"),
    }
}

// G23-S5: Select — Var channel, binding and body preserved correctly.
#[test]
fn select_var_channel_lowers_correctly() {
    use ail_compiler::core_ir::SelectClause;
    let expr = CoreExpr::Select {
        branches: vec![SelectClause {
            channel: Box::new(CoreExpr::Var("inbox".to_string())),
            binding: "msg".to_string(),
            body: CoreExpr::Var("msg".to_string()),
        }],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var channel must produce no synthetic bindings");
    match result {
        AnfExpr::Select { branches } => {
            assert_eq!(branches.len(), 1);
            assert_eq!(branches[0].channel, "inbox");
            assert_eq!(branches[0].binding, "msg");
            assert_eq!(branches[0].body, AnfExpr::Var("msg".to_string()));
        }
        other => panic!("expected Select, got {other:?}"),
    }
}

// TRIANGULATE: Select with two branches, second channel is a Call → atomized.
#[test]
fn select_two_branches_complex_channel_is_atomized() {
    use ail_compiler::core_ir::SelectClause;
    let expr = CoreExpr::Select {
        branches: vec![
            SelectClause {
                channel: Box::new(CoreExpr::Var("ch_a".to_string())),
                binding: "a".to_string(),
                body: CoreExpr::Var("a".to_string()),
            },
            SelectClause {
                channel: Box::new(CoreExpr::Call {
                    func: "fn.get_ch".to_string(),
                    args: vec![],
                }),
                binding: "b".to_string(),
                body: CoreExpr::Var("b".to_string()),
            },
        ],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "one non-Var channel must produce one synthetic binding");
    match result {
        AnfExpr::Select { branches } => {
            assert_eq!(branches.len(), 2);
            assert_eq!(branches[0].channel, "ch_a"); // Var — unchanged
            assert!(branches[1].channel.starts_with("anf_")); // atomized
        }
        other => panic!("expected Select, got {other:?}"),
    }
}

// G23-S6: Timeout — Var duration preserved, body lowered.
#[test]
fn timeout_var_duration_lowers_correctly() {
    let expr = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Var("deadline".to_string())),
        body: Box::new(CoreExpr::Var("work".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var operands must not produce synthetic bindings");
    match result {
        AnfExpr::Timeout { duration, body } => {
            assert_eq!(duration, "deadline");
            assert_eq!(*body, AnfExpr::Var("work".to_string()));
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// TRIANGULATE: Timeout with Literal duration → atomized.
#[test]
fn timeout_literal_duration_is_atomized() {
    let expr = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Literal(LiteralValue::Int(1000))),
        body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal duration must produce one synthetic binding");
    match result {
        AnfExpr::Timeout { duration, .. } => {
            assert!(duration.starts_with("anf_"), "duration must be synthetic: {duration}");
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// G23-S7: CellNew — Var init preserved.
#[test]
fn cell_new_var_init_lowers_correctly() {
    let expr = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Var("initial".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::CellNew { init } => assert_eq!(init, "initial"),
        other => panic!("expected CellNew, got {other:?}"),
    }
}

// TRIANGULATE: CellNew with Literal init → atomized.
#[test]
fn cell_new_literal_init_is_atomized() {
    let expr = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal init must produce one synthetic binding");
    match result {
        AnfExpr::CellNew { init } => {
            assert!(init.starts_with("anf_"), "init must be synthetic: {init}");
        }
        other => panic!("expected CellNew, got {other:?}"),
    }
}

// G23-S8: CellGet — Var cell preserved.
#[test]
fn cell_get_var_cell_lowers_correctly() {
    let expr = CoreExpr::CellGet {
        cell: Box::new(CoreExpr::Var("total".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::CellGet { cell } => assert_eq!(cell, "total"),
        other => panic!("expected CellGet, got {other:?}"),
    }
}

// G23-S9: CellSet — both Var cell and Var value preserved.
#[test]
fn cell_set_var_operands_lowers_correctly() {
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Var("acc".to_string())),
        value: Box::new(CoreExpr::Var("next_val".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var operands must not produce synthetic bindings");
    match result {
        AnfExpr::CellSet { cell, value } => {
            assert_eq!(cell, "acc");
            assert_eq!(value, "next_val");
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// TRIANGULATE: CellSet with non-Var value → value is atomized.
#[test]
fn cell_set_literal_value_is_atomized() {
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Var("counter".to_string())),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(99))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal value must produce one synthetic binding");
    match result {
        AnfExpr::CellSet { cell, value } => {
            assert_eq!(cell, "counter"); // Var → unchanged
            assert!(value.starts_with("anf_"), "value must be synthetic: {value}");
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// G23-S10: CellSet with Literal cell AND Literal value → both atomized.
#[test]
fn cell_set_both_literal_operands_are_atomized() {
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Call {
            func: "fn.get_cell".to_string(),
            args: vec![],
        }),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 2, "two non-Var operands must produce two synthetic bindings");
    match result {
        AnfExpr::CellSet { cell, value } => {
            assert!(cell.starts_with("anf_"), "cell must be synthetic");
            assert!(value.starts_with("anf_"), "value must be synthetic");
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// G23-CBOR: AnfExpr::TaskAwait survives CBOR round-trip.
#[test]
fn task_await_cbor_round_trip() {
    use ail_compiler::hash::stable_cbor_bytes;
    let expr = AnfExpr::TaskAwait { task: "t0".to_string() };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-CBOR: AnfExpr::TaskGroup survives CBOR round-trip.
#[test]
fn task_group_cbor_round_trip() {
    use ail_compiler::hash::stable_cbor_bytes;
    let expr = AnfExpr::TaskGroup {
        body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-CBOR: AnfExpr::Select survives CBOR round-trip.
#[test]
fn select_cbor_round_trip() {
    use ail_compiler::anf::AnfSelectClause;
    use ail_compiler::hash::stable_cbor_bytes;
    let expr = AnfExpr::Select {
        branches: vec![AnfSelectClause {
            channel: "ch".to_string(),
            binding: "v".to_string(),
            body: AnfExpr::Var("v".to_string()),
        }],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-CBOR: AnfExpr::CellSet survives CBOR round-trip.
#[test]
fn cell_set_cbor_round_trip() {
    use ail_compiler::hash::stable_cbor_bytes;
    let expr = AnfExpr::CellSet {
        cell: "c".to_string(),
        value: "v".to_string(),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-FULL: Full pipeline with TaskGroup + CellNew/CellGet/CellSet succeeds.
#[test]
fn full_pipeline_with_cell_ops_succeeds() {
    use ail_compiler::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // Simulate: let c = CellNew(0); CellSet(c, CellGet(c) + 1)
    let expr = CoreExpr::Let {
        name: "c".to_string(),
        value: Box::new(CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        }),
        body: Box::new(CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        }),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    // Expect CellNew(Literal(0)) to generate a synthetic binding for the init,
    // then CellSet(Var("c"), Literal(1)) to generate a synthetic for value.
    // Top-level result is a Let.
    match result {
        AnfExpr::Let { name, .. } => assert_eq!(name, "c"),
        other => panic!("expected Let, got {other:?}"),
    }
}

// ── G20 R2: CBOR round-trips for new AnfExpr variants ────────────────────

// R2-S22: AnfExpr::ShortCircuitAnd CBOR round-trip.
#[test]
fn short_circuit_and_cbor_round_trip() {
    use ail_compiler::hash::stable_cbor_bytes;
    let expr = AnfExpr::ShortCircuitAnd {
        left: "a".to_string(),
        right: Box::new(AnfExpr::Var("b".to_string())),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S23: AnfExpr::EffectCall CBOR round-trip.
#[test]
fn effect_call_cbor_round_trip() {
    let expr = AnfExpr::EffectCall {
        capability: "db".to_string(),
        func: "read".to_string(),
        args: vec!["cart_id".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S24: AnfExpr::RuntimeCheck CBOR round-trip.
#[test]
fn runtime_check_cbor_round_trip() {
    let expr = AnfExpr::RuntimeCheck {
        check_ref: "contract.positive".to_string(),
        cond: "is_valid".to_string(),
        msg: "must be positive".to_string(),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S25: AnfExpr::ResourceAcquire CBOR round-trip.
#[test]
fn resource_acquire_cbor_round_trip() {
    let expr = AnfExpr::ResourceAcquire {
        resource: "db.conn".to_string(),
        args: vec!["conn_str".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S26: AnfExpr::TaskSpawn CBOR round-trip.
#[test]
fn task_spawn_cbor_round_trip() {
    let expr = AnfExpr::TaskSpawn {
        func: "worker.process".to_string(),
        args: vec!["payload".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S27: AnfExpr::Dispatch CBOR round-trip.
#[test]
fn dispatch_cbor_round_trip() {
    let expr = AnfExpr::Dispatch {
        handler: "PaymentProvider".to_string(),
        method: "charge".to_string(),
        args: vec!["amount".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S28: AnfIr schema_version is 1 and survives CBOR round-trip.
#[test]
fn anf_ir_schema_version_survives_cbor() {
    let anf = lower_to_anf(&core_ir_with_expr(
        NodeRef(0),
        "fn_test",
        CoreExpr::Literal(LiteralValue::Unit),
    ))
    .unwrap();
    assert_eq!(anf.schema_version, 1);

    let bytes = stable_cbor_bytes(&anf).unwrap();
    let decoded: ail_compiler::AnfIr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded.schema_version, 1);
}

// R2-S29: AnfIr source_map entries match bindings count and node_ids.
#[test]
fn anf_ir_source_map_matches_bindings() {
    let anf = lower_to_anf(&core_ir_with_expr(
        NodeRef(3),
        "fn_mapped",
        CoreExpr::Var("x".to_string()),
    ))
    .unwrap();
    // Every binding must have a corresponding source map entry.
    assert_eq!(anf.source_map.entries.len(), anf.bindings.len());
    // The last entry's node_id must match the root binding's source_ref.
    let last_entry = anf.source_map.entries.last().unwrap();
    let last_binding = anf.bindings.last().unwrap();
    assert_eq!(last_entry.node_id, last_binding.source_ref);
}
