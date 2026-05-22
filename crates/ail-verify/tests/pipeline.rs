// ── ail-verify::pipeline tests ───────────────────────────────────────────
//
// Strict TDD — tests for the canonical VerificationPipeline facade.
// Spec: verification-pipeline/spec §4 (full policy compliance integration)
// Design: canonical verification facade that sequences all checkers.

use ail_core::semantic_graph::{
    GraphNode, NodeKind, NodeRef, RefinementRef, RefinementStatus, SemanticGraph, TrustMetadata,
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
        level: "resource:linear".into(),
        tags: vec!["released".into()],
    });

    let mut boundary_node = GraphNode::new(NodeRef(1), NodeKind::Boundary, "stripe");
    boundary_node.trust_metadata = Some(TrustMetadata {
        level: "boundary".into(),
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
        level: "resource:linear".into(),
        tags: vec![],
    });
    let mut concurrent = GraphNode::new(NodeRef(2), NodeKind::Function, "worker");
    concurrent.trust_metadata = Some(TrustMetadata {
        level: "verified".into(),
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
