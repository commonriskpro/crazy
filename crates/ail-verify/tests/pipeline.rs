// ── ail-verify::pipeline tests ───────────────────────────────────────────
//
// Strict TDD — tests for the canonical VerificationPipeline facade.
// Spec: verification-pipeline/spec §4 (full policy compliance integration)
// Design: canonical verification facade that sequences all checkers.

use ail_core::semantic_graph::{
    EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, RefinementRef, RefinementStatus,
    SemanticGraph, TrustLevel, TrustMetadata, TypeFacts,
};
use ail_verify::codegen_checker::ArtifactEntry;
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::policy::{PolicyDecision, PolicyRule};
use ail_verify::solver::SimpleSolver;

fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

fn make_ctx<'a>(
    graph: &'a SemanticGraph,
    solver: &'a SimpleSolver,
    profile: &'a str,
    rules: &'a [PolicyRule],
) -> PipelineContext<'a> {
    PipelineContext {
        graph,
        manifests: &[],
        profile,
        solver,
        approvals: &[],
        rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    }
}

// ── Scenario: empty graph → report with policy decision ───────────────────
// GIVEN an empty graph with "test" profile and no rules
// WHEN VerificationPipeline::run is called
// THEN report has a policy_decision (Passed for empty)
#[test]
fn empty_graph_produces_report_with_policy_decision() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let rules = vec![];
    let ctx = make_ctx(&graph, &solver, "test", &rules);

    let report = VerificationPipeline::run(&ctx);

    assert!(
        report.policy_decision.is_some(),
        "pipeline must set policy_decision"
    );
    assert!(
        report.policy_audit.is_some(),
        "pipeline must set policy_audit"
    );
}

// ── Scenario: graph with only proven nodes passes prod policy ─────────────
// GIVEN a graph with a Function node with nominal type (→ Proven type entry)
// AND profile "test" with no blocking rules
// WHEN VerificationPipeline::run is called
// THEN policy_decision is Passed or PassedWithWarnings
#[test]
fn proven_graph_passes_test_profile_policy() {
    use ail_core::semantic_graph::TypeFacts;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_ok");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });
    let g = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let rules = vec![PolicyRule::ProfileGate("test".to_string())];
    let ctx = make_ctx(&g, &solver, "test", &rules);

    let report = VerificationPipeline::run(&ctx);

    let passed = matches!(
        report.policy_decision.unwrap(),
        PolicyDecision::Passed | PolicyDecision::PassedWithWarnings(_)
    );
    assert!(passed, "proven nodes must pass test profile policy");
}

// ── Scenario: pipeline collects entries from multiple checkers ────────────
// GIVEN a graph with a resource node, a boundary node, and a function node
// WHEN VerificationPipeline::run is called
// THEN report entries include entries from all active checker stages
#[test]
fn pipeline_collects_entries_from_multiple_stages() {
    let mut resource_node = GraphNode::new(NodeRef(0), NodeKind::Type, "lock");
    resource_node.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("resource:linear".into()),
        tags: vec!["released".into()],
    });

    let mut boundary_node = GraphNode::new(NodeRef(1), NodeKind::Boundary, "stripe");
    boundary_node.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("boundary".into()),
        tags: vec![
            "has-trust-level".into(),
            "has-contract".into(),
            "has-handler".into(),
            "has-owner".into(),
            "has-review-policy".into(),
        ],
    });

    let g = SemanticGraph {
        nodes: vec![resource_node, boundary_node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let rules = vec![];
    let ctx = make_ctx(&g, &solver, "test", &rules);

    let report = VerificationPipeline::run(&ctx);

    // At minimum, should have entries from resource checker and boundary checker
    // (2 stages × 1 node each = at least 2 entries, plus type/effect/capability entries)
    assert!(
        report.entries.len() >= 2,
        "must have entries from multiple stages"
    );
}

// ── Scenario: pipeline stores proof_obligations in report ─────────────────
// GIVEN a graph with a Function node with contract clauses
// WHEN VerificationPipeline::run is called
// THEN report.proof_obligations is non-empty
#[test]
fn pipeline_stores_proof_obligations_in_report() {
    use ail_core::semantic_graph::ContractClauses;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "checkout");
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["amount > 0".into()],
        ensures: vec!["result.ok".into()],
    });
    let g = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&g, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    assert!(
        !report.proof_obligations.is_empty(),
        "pipeline must populate proof_obligations from contract clauses"
    );
}

// ── Scenario: degradation events recorded for Assumed obligations ──────────
// GIVEN a graph with a contract clause that degrades to Assumed (non-trivial predicate)
// WHEN VerificationPipeline::run is called
// THEN report.degradation_events is non-empty
#[test]
fn pipeline_records_degradation_events_for_assumed_obligations() {
    use ail_core::semantic_graph::ContractClauses;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_deg");
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["complex_predicate".into()], // → Assumed via SimpleSolver
        ensures: vec![],
    });
    let g = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&g, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    assert!(
        !report.degradation_events.is_empty(),
        "Assumed obligations must produce degradation_events"
    );
}

// ── Scenario: codegen artifacts checked when provided ─────────────────────
// GIVEN a pipeline context with a matching artifact entry
// WHEN VerificationPipeline::run is called
// THEN report contains artifact hash entries
#[test]
fn pipeline_checks_artifacts_when_provided() {
    let g = empty_graph();
    let solver = SimpleSolver;
    let artifacts = vec![ArtifactEntry {
        name: "core_ir".into(),
        expected_hash: "hash_a".into(),
        actual_hash: "hash_a".into(),
    }];
    let rules = vec![];
    let ctx = PipelineContext {
        graph: &g,
        manifests: &[],
        profile: "test",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &artifacts,
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    assert!(
        !report.artifact_hashes.is_empty(),
        "artifact hashes must be in report"
    );
    assert_eq!(report.artifact_hashes[0].artifact, "core_ir");
}

#[test]
fn pipeline_generates_non_contract_obligations_for_refinement_resource_concurrency_boundary() {
    let mut refined = GraphNode::new(NodeRef(0), NodeKind::Type, "PositiveInt");
    refined.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "value > 0".into(),
        status: RefinementStatus::Unverified,
        erased: false,
    });
    let mut resource = GraphNode::new(NodeRef(1), NodeKind::Type, "Lock");
    resource.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("resource:linear".into()),
        tags: vec![],
    });
    let mut concurrent = GraphNode::new(NodeRef(2), NodeKind::Function, "worker");
    concurrent.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Verified,
        tags: vec!["concurrent".into()],
    });
    let boundary = GraphNode::new(NodeRef(3), NodeKind::Boundary, "ffi.stripe");
    let graph = SemanticGraph {
        nodes: vec![refined, resource, concurrent, boundary],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);
    let stages: Vec<_> = report
        .proof_obligations
        .iter()
        .map(|entry| entry.source_stage.as_str())
        .collect();
    assert!(stages.contains(&"refinement"));
    assert!(stages.contains(&"resource"));
    assert!(stages.contains(&"concurrency"));
    assert!(stages.contains(&"boundary"));
}

#[test]
fn pipeline_policy_audit_excludes_codegen_entries_but_final_report_keeps_them() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let artifacts = vec![ArtifactEntry {
        name: "wasm".into(),
        expected_hash: "expected".into(),
        actual_hash: "actual".into(),
    }];
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
        artifacts: &artifacts,
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.claim == "codegen-consistency")
    );
    let audit = report
        .policy_audit
        .as_ref()
        .expect("policy audit must be present");
    assert!(
        audit
            .entries
            .iter()
            .all(|entry| entry.scope != "artifact:wasm")
    );
}

// ── Scenario: pipeline is deterministic ───────────────────────────────────
// GIVEN identical pipeline contexts
// WHEN run twice
// THEN both reports have identical entry counts and states
#[test]
fn pipeline_is_deterministic() {
    let g = empty_graph();
    let solver = SimpleSolver;
    let rules = vec![];
    let ctx1 = make_ctx(&g, &solver, "test", &rules);
    let ctx2 = make_ctx(&g, &solver, "test", &rules);

    let report1 = VerificationPipeline::run(&ctx1);
    let report2 = VerificationPipeline::run(&ctx2);

    assert_eq!(report1.entries.len(), report2.entries.len());
    assert_eq!(report1.summary(), report2.summary());
}

// ── Scenario: schema_version set on merged report ─────────────────────────
#[test]
fn pipeline_report_has_schema_version() {
    let g = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&g, &solver, "test", &[]);
    let report = VerificationPipeline::run(&ctx);
    assert_eq!(report.schema_version, "verification/1.0");
}

#[test]
fn full_pipeline_emits_23_steps_in_documented_order() {
    let base = empty_graph();
    let mut answer = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.answer");
    answer.body_expr = Some("literal(42)".into());
    let graph = SemanticGraph {
        nodes: vec![answer],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change answer base=0\nauthor tester\nop create_function id=fn.answer return=Int\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), Some(&base));
    let claims = report
        .entries
        .iter()
        .map(|entry| entry.claim.as_str())
        .collect::<Vec<_>>();

    for expected in [
        "01-parse-changeset",
        "02-canonicalize-changeset",
        "03-validate-op-schemas",
        "04-resolve-graph-references",
        "05-build-semantic-diff",
        "06-lower-affected-graph-to-core-ir",
        "07-type-check",
        "08-effect-capability-check",
        "09-generate-proof-obligations",
        "10-check-refinements",
        "11-check-contracts",
        "12-check-invariants-via-impact-analysis",
        "13-check-resource-lifecycle",
        "14-check-concurrency-safety",
        "15-check-boundaries-ffi-trust",
        "16-check-package-trust-dependencies",
        "17-check-policy-gates",
        "18-check-approval-records",
        "19-lower-to-anf",
        "20-check-anf-effect-resource-ordering",
        "21-generate-validate-manifest",
        "22-codegen-consistency-check",
        "23-emit-verification-report",
    ] {
        if claims.contains(&expected) {
            continue;
        }
        panic!("missing pipeline claim {expected}; claims were {claims:?}");
    }

    let pos = |claim: &str| claims.iter().position(|actual| *actual == claim).unwrap();
    assert!(pos("01-parse-changeset") < pos("02-canonicalize-changeset"));
    assert!(pos("02-canonicalize-changeset") < pos("03-validate-op-schemas"));
    assert!(pos("03-validate-op-schemas") < pos("04-resolve-graph-references"));
    assert!(pos("04-resolve-graph-references") < pos("05-build-semantic-diff"));
    assert!(pos("05-build-semantic-diff") < pos("06-lower-affected-graph-to-core-ir"));
    assert!(pos("06-lower-affected-graph-to-core-ir") < pos("07-type-check"));
    assert!(pos("07-type-check") < pos("08-effect-capability-check"));
    assert!(pos("08-effect-capability-check") < pos("09-generate-proof-obligations"));
    assert!(pos("09-generate-proof-obligations") < pos("10-check-refinements"));
    assert!(pos("10-check-refinements") < pos("11-check-contracts"));
    assert!(pos("11-check-contracts") < pos("12-check-invariants-via-impact-analysis"));
    assert!(pos("12-check-invariants-via-impact-analysis") < pos("13-check-resource-lifecycle"));
    assert!(pos("13-check-resource-lifecycle") < pos("14-check-concurrency-safety"));
    assert!(pos("14-check-concurrency-safety") < pos("15-check-boundaries-ffi-trust"));
    assert!(pos("15-check-boundaries-ffi-trust") < pos("16-check-package-trust-dependencies"));
    assert!(pos("16-check-package-trust-dependencies") < pos("17-check-policy-gates"));
    assert!(pos("17-check-policy-gates") < pos("18-check-approval-records"));
    assert!(pos("18-check-approval-records") < pos("19-lower-to-anf"));
    assert!(pos("19-lower-to-anf") < pos("20-check-anf-effect-resource-ordering"));
    assert!(pos("20-check-anf-effect-resource-ordering") < pos("21-generate-validate-manifest"));
    assert!(pos("21-generate-validate-manifest") < pos("22-codegen-consistency-check"));
    assert!(pos("22-codegen-consistency-check") < pos("23-emit-verification-report"));
}

#[test]
fn full_pipeline_fails_invalid_op_schema() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset = "change bad base=0\nauthor tester\nop add_param target=fn.answer name=x\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), Some(&graph));

    assert!(report.entries.iter().any(|entry| {
        entry.claim == "03-validate-op-schemas"
            && entry.state == ail_verify::report::VerificationState::Failed
            && entry
                .evidence
                .as_deref()
                .is_some_and(|e| e.contains("type"))
    }));
}

#[test]
fn full_pipeline_fails_unresolved_graph_reference() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change bad_ref base=0\nauthor tester\nop set_return target=fn.missing type=Int\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), Some(&graph));

    assert!(report.entries.iter().any(|entry| {
        entry.claim == "04-resolve-graph-references"
            && entry.state == ail_verify::report::VerificationState::Failed
    }));
}

#[test]
fn full_pipeline_checks_invariants_by_impact_edges() {
    let base = empty_graph();
    let invariant = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.balance_non_negative");
    let changed_fn = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.debit");
    let graph = SemanticGraph {
        nodes: vec![invariant, changed_fn],
        edges: vec![GraphEdge::new(
            NodeRef(1),
            NodeRef(0),
            EdgeKind::BreaksIfChanged,
        )],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run_with_changeset(&ctx, None, Some(&base));

    assert!(report.entries.iter().any(|entry| {
        entry.claim == "12-check-invariants-via-impact-analysis"
            && entry.scope == "inv.balance_non_negative"
            && entry.state == ail_verify::report::VerificationState::Proven
    }));
}

#[test]
fn full_pipeline_fails_anf_resource_use_after_release_order() {
    // Updated to use acquire/release terminology (TASK-16)
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.bad_resource_order");
    node.body_expr = Some("release(lock); acquire(lock)".into());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    assert!(report.entries.iter().any(|entry| {
        entry.claim == "20-check-anf-effect-resource-ordering"
            && entry.state == ail_verify::report::VerificationState::Failed
    }));
}

// ── TASK-13: Stage 19 — ANF structural analysis ───────────────────────────

#[test]
fn stage19_let_in_body_is_proven() {
    // "let x = f() in x + 1" is valid ANF → Proven
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.anf_let");
    node.body_expr = Some("let x = f() in x + 1".into());
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let failed = report.entries.iter().any(|e| {
        e.claim == "19-lower-to-anf" && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(!failed, "let...in body must not produce Unverified in stage19");
}

#[test]
fn stage19_semicolon_outside_let_is_unverified() {
    // "a; b" has bare semicolon, not in let...in context → Unverified
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.bare_semi");
    node.body_expr = Some("a; b".into());
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.scope == "fn.bare_semi"
            && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(entry.is_some(), "bare semicolon outside let...in must produce Unverified");
}

#[test]
fn stage19_while_keyword_is_unverified() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.while_loop");
    node.body_expr = Some("while true { do_something() }".into());
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.state == ail_verify::report::VerificationState::Unverified
    });
    assert!(entry.is_some(), "'while' keyword must produce Unverified");
}

#[test]
fn stage19_no_body_is_proven() {
    // Node with no body_expr → Proven (nothing to analyze)
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.no_body");
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf" && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(entry.is_some(), "no body_expr must produce Proven for stage19");
}

// ── TASK-15: Stage 20 — acquire/release pair analysis ─────────────────────

#[test]
fn stage20_release_before_acquire_fails_with_e_anf_resource_order() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.bad_order");
    node.body_expr = Some("release(db) acquire(db)".into());
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "20-check-anf-effect-resource-ordering"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence.as_deref().unwrap_or("").contains("E_ANF_RESOURCE_ORDER")
    });
    assert!(entry.is_some(), "release before acquire must produce Failed with E_ANF_RESOURCE_ORDER");
}

#[test]
fn stage20_acquire_then_release_is_proven() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.good_order");
    node.body_expr = Some("acquire(db) release(db)".into());
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "20-check-anf-effect-resource-ordering"
            && e.state == ail_verify::report::VerificationState::Proven
    });
    assert!(entry.is_some(), "acquire before release must produce Proven");
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
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
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
        report.entries.iter().filter(|e| e.claim == "10-check-refinements").collect::<Vec<_>>()
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
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
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
        report.entries.iter().filter(|e| e.claim == "10-check-refinements").collect::<Vec<_>>()
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
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
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
        report.entries.iter().filter(|e| e.claim == "10-check-refinements").collect::<Vec<_>>()
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
        n.type_facts = Some(TypeFacts { nominal: "Int".into(), generics: vec![] });
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
        n.type_facts = Some(TypeFacts { nominal: "String".into(), generics: vec![] }); // changed
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
        report.entries.iter().filter(|e| e.claim == "12-check-invariants-via-impact-analysis").collect::<Vec<_>>()
    );
}

#[test]
fn stage12_connected_changed_node_with_breaks_edge_is_proven() {
    // GIVEN same setup as above BUT with BreaksIfChanged from fn.dep to inv.stable
    // THEN stage12 entry for inv.stable is Proven
    let base_fn = {
        let mut n = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.dep");
        n.type_facts = Some(TypeFacts { nominal: "Int".into(), generics: vec![] });
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
        n.type_facts = Some(TypeFacts { nominal: "String".into(), generics: vec![] });
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
        report.entries.iter().filter(|e| e.claim == "12-check-invariants-via-impact-analysis").collect::<Vec<_>>()
    );
}

#[test]
fn stage12_no_base_graph_invariant_is_unverified() {
    // GIVEN no base graph (None)
    // THEN invariant is Unverified (existing behavior)
    let invariant = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.no_base");
    let graph = SemanticGraph { nodes: vec![invariant], edges: vec![] };
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
        report.entries.iter().filter(|e| e.claim == "12-check-invariants-via-impact-analysis").collect::<Vec<_>>()
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
        report.entries.iter().filter(|e| e.claim == "12-check-invariants-via-impact-analysis").collect::<Vec<_>>()
    );
}

// ── TASK-03: Stage 3 — op schema version + arg type validation tests ───────

#[test]
fn stage3_op_with_version_999_fails_with_version_incompatible() {
    // Op carries version=999 which exceeds CURRENT_SCHEMA_VERSION=1
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.foo");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    // set_return with version=999 arg
    let changeset =
        "change test base=0\nauthor tester\nop set_return target=fn.foo type=Int version=999\nend\n";

    let report = VerificationPipeline::run_with_changeset(&ctx, Some(changeset), None);

    let failed = report.entries.iter().any(|e| {
        e.claim == "03-validate-op-schemas"
            && e.state == ail_verify::report::VerificationState::Failed
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_OP_VERSION_INCOMPATIBLE")
    });
    assert!(failed, "version=999 must produce E_OP_VERSION_INCOMPATIBLE Failed entry");
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
    assert!(failed, "unknown type must produce E_OP_ARG_TYPE_INVALID Failed entry");
}

#[test]
fn stage3_op_with_effect_without_colon_fails_with_effect_malformed() {
    // effect=nodot has no colon separator → malformed
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.foo");
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
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.foo");
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
    assert!(!version_failed, "version=1 must NOT produce E_OP_VERSION_INCOMPATIBLE");
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
    assert!(failed, "empty base_hash must produce E_STALE_CONTEXT Failed entry");
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
    assert!(failed, "short base_hash must produce E_STALE_CONTEXT Failed entry");
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
    assert!(!stale, "valid 64-char hex base_hash must NOT produce E_STALE_CONTEXT");
}

#[test]
fn stage4_op_without_base_hash_has_no_stale_check() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);
    let changeset =
        "change test base=0\nauthor tester\nop create_function id=fn.x\nend\n";

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
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.added");
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
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.removed");
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
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.x");
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

// ── Ola5 Gap-2: Stage 19 — ANF Placeholder check ─────────────────────────

#[test]
fn stage19_function_node_without_body_produces_placeholder_entry() {
    // A Function node with no body_expr is a Placeholder → Stage 19 must flag it Unverified
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.no_body");
    let graph = SemanticGraph { nodes: vec![node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.state == ail_verify::report::VerificationState::Unverified
            && e.evidence.as_deref().unwrap_or("").to_lowercase().contains("placeholder")
    });
    assert!(
        entry.is_some(),
        "Function node with no body_expr must produce Unverified Stage 19 entry with Placeholder; entries: {:?}",
        report.entries.iter().filter(|e| e.claim == "19-lower-to-anf").collect::<Vec<_>>()
    );
}

#[test]
fn stage19_non_function_node_without_body_does_not_flag_placeholder() {
    // Module/Type/Capability nodes with no body_expr are NOT Placeholders
    let module_node = GraphNode::new(NodeRef(0), NodeKind::Module, "mod.payments");
    let type_node = GraphNode::new(NodeRef(1), NodeKind::Type, "Amount");
    let graph = SemanticGraph { nodes: vec![module_node, type_node], edges: vec![] };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "test", &[]);

    let report = VerificationPipeline::run(&ctx);

    let placeholder_entry = report.entries.iter().find(|e| {
        e.claim == "19-lower-to-anf"
            && e.state == ail_verify::report::VerificationState::Unverified
            && e.evidence.as_deref().unwrap_or("").to_lowercase().contains("placeholder")
    });
    assert!(
        placeholder_entry.is_none(),
        "Non-function nodes without body must NOT produce Placeholder Stage 19 entry"
    );
}

// ── Ola5 Gap-2: Stage 21 — manifest hash comparison ──────────────────────

#[test]
fn stage21_manifest_hash_mismatch_produces_failed_entry() {
    // When artifact_manifest_hash is provided but doesn't match the computed hash → Failed
    let cap = GraphNode::new(NodeRef(0), NodeKind::Capability, "cap.payment.charge");
    let graph = SemanticGraph { nodes: vec![cap], edges: vec![] };
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
        artifact_manifest_hash: Some("deadbeef00000000000000000000000000000000000000000000000000000000"),
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
        report.entries.iter().filter(|e| e.claim == "21-generate-validate-manifest").collect::<Vec<_>>()
    );
}

#[test]
fn stage21_no_artifact_hash_skips_hash_check() {
    // When artifact_manifest_hash is None → hash check is skipped, existing cap-set check runs
    let cap = GraphNode::new(NodeRef(0), NodeKind::Capability, "cap.payment.charge");
    let graph = SemanticGraph { nodes: vec![cap], edges: vec![] };
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
    assert!(report.entries.iter().any(|entry| {
        entry.claim == "21-generate-validate-manifest"
            && entry.state == ail_verify::report::VerificationState::Proven
    }), "no artifact_manifest_hash → existing cap-set check must still pass");
}
