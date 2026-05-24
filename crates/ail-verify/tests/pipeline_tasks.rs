// ── ail-verify::pipeline — per-stage task tests ──────────────────────────
//
// Tests for individual pipeline stage behaviour:
// Stage 3 (op schema), Stage 4 (snapshot hash), Stage 5 (semantic diff),
// Stage 10 (refinements), Stage 12 (impact analysis),
// Stage 19 (ANF lowering), Stage 20 (resource ordering), Stage 21 (manifest).
// Spec: verification-pipeline/spec §4

mod pipeline_helpers;

use ail_core::semantic_graph::{
    EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, RefinementRef, RefinementStatus,
    SemanticGraph, TypeFacts,
};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::solver::SimpleSolver;

use pipeline_helpers::{empty_graph, make_ctx};

// ── TASK-13: Stage 19 — ANF structural analysis ───────────────────────────

#[test]
fn stage19_let_in_body_is_proven() {
    // "let x = f() in x + 1" is valid ANF → Proven
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.anf_let");
    node.body_expr = Some("let x = f() in x + 1".into());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let failed = report.entries.iter().any(|e| {
        e.claim == "19-lower-to-anf" && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(
        !failed,
        "let...in body must not produce Unverified in stage19"
    );
}

#[test]
fn stage19_semicolon_outside_let_is_unverified() {
    // "a; b" has bare semicolon, not in let...in context → Unverified
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.bare_semi");
    node.body_expr = Some("a; b".into());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.scope == "fn.bare_semi"
            && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(
        entry.is_some(),
        "bare semicolon outside let...in must produce Unverified"
    );
}

#[test]
fn stage19_while_keyword_is_unverified() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.while_loop");
    node.body_expr = Some("while true { do_something() }".into());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf" && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(entry.is_some(), "'while' keyword must produce Unverified");
}

#[test]
fn stage19_no_body_is_proven() {
    // Node with no body_expr → Proven (nothing to analyze)
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.no_body");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf" && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        entry.is_some(),
        "no body_expr must produce Proven for stage19"
    );
}

// ── TASK-15: Stage 20 — acquire/release pair analysis ─────────────────────

#[test]
fn stage20_release_before_acquire_fails_with_e_anf_resource_order() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.bad_order");
    node.body_expr = Some("release(db) acquire(db)".into());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "20-check-anf-effect-resource-ordering"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_ANF_RESOURCE_ORDER")
    });
    assert!(
        entry.is_some(),
        "release before acquire must produce Failed with E_ANF_RESOURCE_ORDER"
    );
}

#[test]
fn stage20_acquire_then_release_is_proven() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.good_order");
    node.body_expr = Some("acquire(db) release(db)".into());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "20-check-anf-effect-resource-ordering"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        entry.is_some(),
        "acquire before release must produce Proven"
    );
}

#[test]
fn full_pipeline_validates_manifest_capabilities() {
    let cap = GraphNode::new(NodeRef(0), NodeKind::Capability, "cap.payment.charge");
    let graph = SemanticGraph {
        nodes: vec![cap],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let manifest_caps = vec!["cap.payment.charge".to_string()];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &[],
        profile: "test",
        solver: &solver,
        approvals: &[],
        rules: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &manifest_caps,
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    assert!(report.entries.iter().any(|entry| {
        entry.claim == "21-generate-validate-manifest"
            && entry.state == ail_verify::report::VerificationState::Proven
    }));
}

// ── TASK-09: Stage 10 — solver-backed refinement check ────────────────────

#[test]
fn stage10_unverified_refinement_true_predicate_proves_via_solver() {
    // GIVEN a node with Unverified refinement and predicate "true"
    // WHEN the pipeline runs (SimpleSolver proves "true")
    // THEN stage10 entry is Proven
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "PositiveInt");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "true".into(),
        status: RefinementStatus::Unverified,
        erased: false,
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "10-check-refinements"
            && e.scope == "PositiveInt"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        entry.is_some(),
        "Unverified refinement with 'true' predicate must be Proven via solver; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "10-check-refinements")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage10_unverified_refinement_unsupported_predicate_becomes_assumed() {
    // GIVEN a node with Unverified refinement and predicate "x > 0" (unsupported by SimpleSolver)
    // WHEN the pipeline runs
    // THEN stage10 entry is Assumed (not Unverified)
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "PositiveAmount");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "x > 0".into(),
        status: RefinementStatus::Unverified,
        erased: false,
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "10-check-refinements"
            && e.scope == "PositiveAmount"
            && e.state == ail_verify::report::VerificationState::Assumed
    });
    assert!(
        entry.is_some(),
        "Unverified refinement with unsupported predicate must be Assumed; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "10-check-refinements")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage10_proven_refinement_stays_proven_without_solver_call() {
    // GIVEN a node with Proven refinement status
    // WHEN the pipeline runs
    // THEN stage10 entry is Proven (status honoured, no solver needed)
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "SafeInt");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "value != 0".into(),
        status: RefinementStatus::Proven,
        erased: false,
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "10-check-refinements"
            && e.scope == "SafeInt"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        entry.is_some(),
        "Proven refinement must stay Proven; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "10-check-refinements")
            .collect::<Vec<_>>()
    );
}

// ── TASK-11: Stage 12 — BFS impact analysis ───────────────────────────────

#[test]
fn stage12_connected_changed_node_without_breaks_edge_is_unverified_with_evidence() {
    // GIVEN base graph has invariant + fn.dep with same type_facts
    // AND target graph has fn.dep with changed type_facts
    // AND there is a DependsOn edge from invariant to fn.dep (connected)
    // AND NO BreaksIfChanged edge
    // THEN stage12 entry for invariant is Unverified with fn.dep in evidence
    let base_fn = {
        let mut n = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.dep");
        n.type_facts = Some(TypeFacts {
            nominal: "Int".into(),
            generics: vec![],
        });
        n
    };
    let base = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.stable"),
            base_fn,
        ],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn)],
    };

    let changed_fn = {
        let mut n = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.dep");
        n.type_facts = Some(TypeFacts {
            nominal: "String".into(),
            generics: vec![],
        }); // changed
        n
    };
    let target = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.stable"),
            changed_fn,
        ],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn)],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&target, &solver, "test", &[]);

    let report = VerificationPipeline::run_with_changeset(&ctx, None, Some(&base));

    let entry = report.entries.iter().find(|e| {
        e.claim == "12-check-invariants-via-impact-analysis"
            && e.scope == "inv.stable"
            && e.state == ail_verify::report::VerificationState::Unverified
            && e.evidence.as_deref().unwrap_or("").contains("fn.dep")
    });
    assert!(
        entry.is_some(),
        "connected changed node without BreaksIfChanged must produce Unverified with node name; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "12-check-invariants-via-impact-analysis")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage12_connected_changed_node_with_breaks_edge_is_proven() {
    // GIVEN same setup as above BUT with BreaksIfChanged from fn.dep to inv.stable
    // THEN stage12 entry for inv.stable is Proven
    let base_fn = {
        let mut n = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.dep");
        n.type_facts = Some(TypeFacts {
            nominal: "Int".into(),
            generics: vec![],
        });
        n
    };
    let base = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.stable"),
            base_fn,
        ],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn)],
    };

    let changed_fn = {
        let mut n = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.dep");
        n.type_facts = Some(TypeFacts {
            nominal: "String".into(),
            generics: vec![],
        });
        n
    };
    let target = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.stable"),
            changed_fn,
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::BreaksIfChanged),
        ],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&target, &solver, "test", &[]);

    let report = VerificationPipeline::run_with_changeset(&ctx, None, Some(&base));

    let entry = report.entries.iter().find(|e| {
        e.claim == "12-check-invariants-via-impact-analysis"
            && e.scope == "inv.stable"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        entry.is_some(),
        "changed node covered by BreaksIfChanged must produce Proven; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "12-check-invariants-via-impact-analysis")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage12_no_base_graph_invariant_is_unverified() {
    // GIVEN no base graph (None)
    // THEN invariant is Unverified (existing behavior)
    let invariant = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.no_base");
    let graph = SemanticGraph {
        nodes: vec![invariant],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run_with_changeset(&ctx, None, None);

    let entry = report.entries.iter().find(|e| {
        e.claim == "12-check-invariants-via-impact-analysis"
            && e.scope == "inv.no_base"
            && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(
        entry.is_some(),
        "no base graph must produce Unverified for invariants; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "12-check-invariants-via-impact-analysis")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage12_no_changed_nodes_invariant_is_proven() {
    // GIVEN base and target graphs are identical (no changes)
    // THEN invariant is Proven (no impact detected)
    let inv = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.stable_no_change");
    let fn_node = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.stable");
    let graph = SemanticGraph {
        nodes: vec![inv, fn_node],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn)],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    // Both base and target are identical → no changed nodes
    let report = VerificationPipeline::run_with_changeset(&ctx, None, Some(&graph));

    let entry = report.entries.iter().find(|e| {
        e.claim == "12-check-invariants-via-impact-analysis"
            && e.scope == "inv.stable_no_change"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        entry.is_some(),
        "no changed nodes must produce Proven for invariants; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "12-check-invariants-via-impact-analysis")
            .collect::<Vec<_>>()
    );
}

// ── TASK-03: Stage 3 — op schema version + arg type validation tests ───────

#[test]
fn stage3_op_with_version_999_fails_with_version_incompatible() {
    // Op carries version=999 which exceeds CURRENT_SCHEMA_VERSION=1
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.foo");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    // set_return with version=999 arg
    let changeset = "change test base=0\nauthor tester\nop set_return target=fn.foo type=Int version=999\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_VERSION_INCOMPATIBLE")
    });
    assert!(
        failed,
        "version=999 must produce E_OP_VERSION_INCOMPATIBLE Failed entry"
    );
}

#[test]
fn stage3_op_with_unknown_type_fails_with_arg_type_invalid() {
    // type=UnknownType999 is not a known primitive and not in the graph
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop set_return target=fn.x type=UnknownType999\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_ARG_TYPE_INVALID")
    });
    assert!(
        failed,
        "unknown type must produce E_OP_ARG_TYPE_INVALID Failed entry"
    );
}

#[test]
fn stage3_op_with_effect_without_colon_fails_with_effect_malformed() {
    // effect=nodot has no colon separator → malformed
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.foo");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop add_effect target=fn.foo effect=nodot\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_ARG_EFFECT_MALFORMED")
    });
    assert!(
        failed,
        "effect without colon must produce E_OP_ARG_EFFECT_MALFORMED Failed entry"
    );
}

#[test]
fn stage3_op_with_version_1_is_proven() {
    // version=1 is valid (CURRENT_SCHEMA_VERSION)
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.foo");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop set_return target=fn.foo type=Int version=1\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    // Should not have E_OP_VERSION_INCOMPATIBLE for this op
    let version_failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_VERSION_INCOMPATIBLE")
    });
    assert!(
        !version_failed,
        "version=1 must NOT produce E_OP_VERSION_INCOMPATIBLE"
    );
}

// ── TASK-05: Stage 4 — snapshot hash freshness tests ─────────────────────

#[test]
fn stage4_empty_base_hash_fails_with_stale_context() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop annotate target=snapshot base_hash=\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_STALE_CONTEXT")
    });
    assert!(
        failed,
        "empty base_hash must produce E_STALE_CONTEXT Failed entry"
    );
}

#[test]
fn stage4_short_base_hash_fails_with_stale_context() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    // 8-char hex (not 64)
    let changeset =
        "change test base=0\nauthor tester\nop annotate target=snapshot base_hash=abcdef12\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_STALE_CONTEXT")
    });
    assert!(
        failed,
        "short base_hash must produce E_STALE_CONTEXT Failed entry"
    );
}

#[test]
fn stage4_valid_64char_hex_base_hash_is_proven() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let hash = "a".repeat(64);
    let changeset = format!(
        "change test base=0\nauthor tester\nop annotate target=snapshot base_hash={hash}\nend\n"
    );

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset.as_str()), None);

    let stale = report.entries.iter().any(|e| {
        e.evidence
            .as_deref()
            .unwrap_or("")
            .contains("E_STALE_CONTEXT")
    });
    assert!(
        !stale,
        "valid 64-char hex base_hash must NOT produce E_STALE_CONTEXT"
    );
}

#[test]
fn stage4_op_without_base_hash_has_no_stale_check() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset = "change test base=0\nauthor tester\nop create_function id=fn.x\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let stale = report.entries.iter().any(|e| {
        e.evidence
            .as_deref()
            .unwrap_or("")
            .contains("E_STALE_CONTEXT")
    });
    assert!(!stale, "op without base_hash must not trigger stale check");
}

// ── TASK-07: Stage 5 — structural diff per-node entries ──────────────────

#[test]
fn stage5_added_node_produces_proven_entry_with_node_name_scope() {
    // base has no nodes, target has one → added node
    let base = empty_graph();
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.added");
    let target = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&target, &solver, "test", &[]);

    let report = VerificationPipeline::run_with_changeset(&ctx, None, Some(&base));

    let added_entry = report.entries.iter().find(|e| {
        e.claim == "05-build-semantic-diff"
            && e.scope == "fn.added"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(
        added_entry.is_some(),
        "added node must produce Proven entry scoped to node name; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "05-build-semantic-diff")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage5_removed_node_produces_unverified_entry_with_node_name_scope() {
    // base has one node, target has none → removed
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.removed");
    let base = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let target = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&target, &solver, "test", &[]);

    let report = VerificationPipeline::run_with_changeset(&ctx, None, Some(&base));

    let removed_entry = report.entries.iter().find(|e| {
        e.claim == "05-build-semantic-diff"
            && e.scope == "fn.removed"
            && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(
        removed_entry.is_some(),
        "removed node must produce Unverified entry scoped to node name; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "05-build-semantic-diff")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage5_no_base_graph_produces_single_unverified_entry() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.x");
    let target = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&target, &solver, "test", &[]);

    // No base graph → existing behavior: single Unverified entry
    let report = VerificationPipeline::run_with_changeset(&ctx, None, None);

    let diff_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "05-build-semantic-diff")
        .collect();
    assert_eq!(diff_entries.len(), 1, "no base → exactly 1 diff entry");
    assert_eq!(
        diff_entries[0].state,
        ail_verify::report::VerificationState::Unverified
    );
}

// ── Ola5 Gap-3: Op schema type validation beyond hardcoded list ───────────

#[test]
fn stage3_op_with_qualified_external_type_passes() {
    // Payment.Amount follows the Package.Type pattern → must be accepted
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop set_return target=fn.x type=Payment.Amount\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_ARG_TYPE_INVALID")
    });
    assert!(
        !failed,
        "qualified external type Payment.Amount must NOT produce E_OP_ARG_TYPE_INVALID"
    );
}

#[test]
fn stage3_op_with_multi_segment_qualified_type_passes() {
    // Domain.Sub.Type is a multi-segment qualified external type → must be accepted
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop set_return target=fn.x type=Domain.Sub.Type\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_ARG_TYPE_INVALID")
    });
    assert!(
        !failed,
        "multi-segment qualified type Domain.Sub.Type must NOT produce E_OP_ARG_TYPE_INVALID"
    );
}

// ── Ola5 Gap-2: Stage 19 — ANF body-less function structured diagnostic ───

#[test]
fn stage19_function_node_without_body_produces_placeholder_entry() {
    // A Function node with no body_expr must produce a structured E_ANF_NO_BODY
    // diagnostic in Stage 19 (Unverified).
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.no_body");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.state == ail_verify::report::VerificationState::Unverified
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .starts_with("E_ANF_NO_BODY")
    });
    assert!(
        entry.is_some(),
        "Function node with no body_expr must produce Unverified Stage 19 entry with E_ANF_NO_BODY; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "19-lower-to-anf")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage19_non_function_node_without_body_does_not_flag_placeholder() {
    // Module/Type/Capability nodes with no body_expr must NOT produce E_ANF_NO_BODY.
    let module_node = GraphNode::new(NodeRef(0), NodeKind::Module, "mod.payments");
    let type_node = GraphNode::new(NodeRef(1), NodeKind::Type, "Amount");
    let graph = SemanticGraph {
        nodes: vec![module_node, type_node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let no_body_entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.state == ail_verify::report::VerificationState::Unverified
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .starts_with("E_ANF_NO_BODY")
    });
    assert!(
        no_body_entry.is_none(),
        "Non-function nodes without body must NOT produce E_ANF_NO_BODY Stage 19 entry"
    );
}

// ── Ola5 Gap-2: Stage 21 — manifest hash comparison ──────────────────────

#[test]
fn stage21_manifest_hash_mismatch_produces_failed_entry() {
    // When artifact_manifest_hash is provided but doesn't match the computed hash → Failed
    let cap = GraphNode::new(NodeRef(0), NodeKind::Capability, "cap.payment.charge");
    let graph = SemanticGraph {
        nodes: vec![cap],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let manifest_caps = vec!["cap.payment.charge".to_string()];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &[],
        profile: "test",
        solver: &solver,
        approvals: &[],
        rules: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &manifest_caps,
        artifact_manifest_hash: Some(
            "deadbeef00000000000000000000000000000000000000000000000000000000",
        ),
    };

    let report = VerificationPipeline::run(&ctx);

    let failed = report.entries.iter().any(|e| {
        e.claim == "21-generate-validate-manifest"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_MANIFEST_HASH_MISMATCH")
    });
    assert!(
        failed,
        "wrong artifact_manifest_hash must produce E_MANIFEST_HASH_MISMATCH Failed entry; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim == "21-generate-validate-manifest")
            .collect::<Vec<_>>()
    );
}

#[test]
fn stage21_no_artifact_hash_skips_hash_check() {
    // When artifact_manifest_hash is None → hash check is skipped, existing cap-set check runs
    let cap = GraphNode::new(NodeRef(0), NodeKind::Capability, "cap.payment.charge");
    let graph = SemanticGraph {
        nodes: vec![cap],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let manifest_caps = vec!["cap.payment.charge".to_string()];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &[],
        profile: "test",
        solver: &solver,
        approvals: &[],
        rules: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &manifest_caps,
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    // Existing behavior: cap-set check passes when graph caps == manifest caps
    assert!(
        report.entries.iter().any(|entry| {
            entry.claim == "21-generate-validate-manifest"
                && entry.state == ail_verify::report::VerificationState::Proven
        }),
        "no artifact_manifest_hash → existing cap-set check must still pass"
    );
}
