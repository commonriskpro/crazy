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

use ail_compiler::hash::stable_cbor_bytes;
use ail_compiler::lower::lower_core_expr_to_anf;
use ail_compiler::{
    AnfExpr, CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, StageHashes,
    lower_to_anf, lower_to_core_ir,
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── Helpers ───────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport { entries: vec![] }
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
