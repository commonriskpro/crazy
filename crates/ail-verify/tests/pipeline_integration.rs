// ── ail-verify::pipeline — full pipeline + policy integration tests ────────
//
// End-to-end pipeline tests covering the 23-step sequence, changeset parsing,
// graph reference resolution, ANF resource ordering, and package trust gates
// for prod/critical profiles.
// Spec: verification-pipeline/spec §4

mod pipeline_helpers;

use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::policy::{PolicyDecision, PolicyRule};
use ail_verify::solver::SimpleSolver;

use pipeline_helpers::{
    assumed_package_manifest, empty_graph, make_ctx, strong_approval, unsafe_package_manifest,
    unverified_package_manifest,
};

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

#[test]
fn prod_pipeline_requires_active_package_assumption_approval() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![assumed_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("prod".into())];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "prod",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };
    let report = VerificationPipeline::run_with_changeset(
        &ctx,
        Some("change noop base=0\nauthor tester\nend\n"),
        Some(&graph),
    );
    let scope = "package:payments.stripe@2.3.1#assumption:stripe_idempotency";

    assert!(report.entries.iter().any(|entry| {
        entry.claim == "package-assumption-approval[prod]"
            && entry.scope == scope
            && entry.state == ail_verify::report::VerificationState::Assumed
    }));
    assert!(matches!(
        report.policy_decision,
        Some(PolicyDecision::Failed(_)) | Some(PolicyDecision::ApprovalRequired(_))
    ));
}

#[test]
fn prod_pipeline_passes_active_package_assumption_with_approval() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![assumed_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("prod".into())];
    let scope = "package:payments.stripe@2.3.1#assumption:stripe_idempotency";
    let approvals = vec![strong_approval(scope)];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "prod",
        solver: &solver,
        approvals: &approvals,
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };
    let report = VerificationPipeline::run_with_changeset(
        &ctx,
        Some("change noop base=0\nauthor tester\nend\n"),
        Some(&graph),
    );

    assert_eq!(report.policy_decision, Some(PolicyDecision::Passed));
    assert!(report.policy_audit.as_ref().is_some_and(|audit| {
        audit.entries.iter().any(|entry| {
            entry.scope == scope
                && entry.gate_decision == "passed"
                && entry.approval_used.as_deref() == Some("security-team")
        })
    }));
}

// ── Package trust gate: prod/critical pipeline integration ────────────────
//
// These tests prove that package trust violations (Unsafe, Unverified, Assumed)
// produce the correct `policy_decision` when the pipeline runs with a
// `ProfileGate("prod")` or `ProfileGate("critical")` rule.
//
// Evidence path:
//  Stage 16: PackageTrustChecker::check → VerificationEntry with trust state
//  Stage 17: PolicyEngine::evaluate_with_audit → policy_decision

// GIVEN a manifest with TrustLevel::Unsafe AND profile "prod" with ProfileGate rule
// WHEN VerificationPipeline::run is called
// THEN policy_decision is Failed (unsafe always blocked in prod)
#[test]
fn unsafe_package_in_prod_fails_pipeline() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![unsafe_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("prod".into())];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "prod",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    // Stage 16 must contain an Unsafe entry for the package
    assert!(
        report.entries.iter().any(|e| {
            e.claim.contains("package-trust")
                && e.state == ail_verify::report::VerificationState::Unsafe
                && e.scope.contains("sketchy.ffi")
        }),
        "Stage 16 must emit Unsafe entry for TrustLevel::Unsafe package; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim.contains("package"))
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(report.policy_decision, Some(PolicyDecision::Failed(_))),
        "Unsafe package in prod must produce Failed policy decision; got {:?}",
        report.policy_decision
    );
}

// GIVEN a manifest with TrustLevel::Unsafe AND profile "critical" with ProfileGate rule
// WHEN VerificationPipeline::run is called
// THEN policy_decision is Failed (critical always blocks Unsafe, no approval exemption)
#[test]
fn unsafe_package_in_critical_fails_pipeline() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![unsafe_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("critical".into())];
    // Even a Strong approval does NOT save Unsafe in critical
    let approvals = vec![strong_approval("package:sketchy.ffi@0.1.0")];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "critical",
        solver: &solver,
        approvals: &approvals,
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    assert!(
        matches!(report.policy_decision, Some(PolicyDecision::Failed(_))),
        "Unsafe package in critical must produce Failed even with Strong approval; got {:?}",
        report.policy_decision
    );
}

// GIVEN a manifest with TrustLevel::Unverified AND profile "prod" with ProfileGate rule
// WHEN VerificationPipeline::run is called
// THEN policy_decision is Failed (prod blocks Unverified)
#[test]
fn unverified_package_in_prod_fails_pipeline() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![unverified_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("prod".into())];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "prod",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    // Stage 16 must contain a blocking Unverified entry
    assert!(
        report.entries.iter().any(|e| {
            e.claim.contains("package-trust")
                && e.state == ail_verify::report::VerificationState::Unverified
                && e.blocking
                && e.scope.contains("experimental.lib")
        }),
        "Stage 16 must emit blocking Unverified entry for Unverified package in prod; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim.contains("package"))
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(report.policy_decision, Some(PolicyDecision::Failed(_))),
        "Unverified package in prod must produce Failed policy decision; got {:?}",
        report.policy_decision
    );
}

// GIVEN a manifest with TrustLevel::Unverified AND profile "critical" with ProfileGate rule
// WHEN VerificationPipeline::run is called
// THEN policy_decision is Failed (critical blocks Unverified, same as prod)
#[test]
fn unverified_package_in_critical_fails_pipeline() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![unverified_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("critical".into())];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "critical",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    // Stage 16 must label this blocking (minimum_trust_for_profile("critical") = Assumed)
    assert!(
        report.entries.iter().any(|e| {
            e.claim.contains("package-trust")
                && e.state == ail_verify::report::VerificationState::Unverified
                && e.blocking
                && e.scope.contains("experimental.lib")
        }),
        "Stage 16 must emit blocking Unverified entry for Unverified package in critical; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim.contains("package"))
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(report.policy_decision, Some(PolicyDecision::Failed(_))),
        "Unverified package in critical must produce Failed policy decision; got {:?}",
        report.policy_decision
    );
}

// GIVEN a manifest with TrustLevel::Assumed (with valid boundary/assumption)
//   AND profile "critical" with ProfileGate rule
//   AND no approval record
// WHEN VerificationPipeline::run is called
// THEN policy_decision is ApprovalRequired or Failed (Strong approval needed)
#[test]
fn assumed_package_in_critical_requires_approval() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![assumed_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("critical".into())];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "critical",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    // Stage 16 must contain an Assumed entry requiring approval
    assert!(
        report.entries.iter().any(|e| {
            e.claim.contains("package") && e.state == ail_verify::report::VerificationState::Assumed
        }),
        "Stage 16 must emit Assumed entry for Assumed package in critical; entries: {:?}",
        report
            .entries
            .iter()
            .filter(|e| e.claim.contains("package"))
            .collect::<Vec<_>>()
    );
    // Without Strong approval the decision must be ApprovalRequired or Failed
    assert!(
        matches!(
            report.policy_decision,
            Some(PolicyDecision::Failed(_)) | Some(PolicyDecision::ApprovalRequired(_))
        ),
        "Assumed package in critical without approval must produce ApprovalRequired or Failed; got {:?}",
        report.policy_decision
    );
}

// GIVEN a manifest with TrustLevel::Assumed (with valid boundary/assumption)
//   AND profile "critical" with ProfileGate rule
//   AND a Strong approval covering the per-assumption scope
//   AND a valid (noop) changeset so changeset stages resolve to Proven
// WHEN VerificationPipeline::run_with_changeset is called
// THEN policy_decision is Passed
#[test]
fn assumed_package_in_critical_with_strong_approval_passes() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let manifests = vec![assumed_package_manifest()];
    let rules = vec![PolicyRule::ProfileGate("critical".into())];
    // The per-assumption scope emitted by PackageTrustChecker in critical profile
    let scope = "package:payments.stripe@2.3.1#assumption:stripe_idempotency";
    let approvals = vec![strong_approval(scope)];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &manifests,
        profile: "critical",
        solver: &solver,
        approvals: &approvals,
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };
    // Use a noop changeset + base graph so Stages 1–5 produce Proven entries.
    // Without these, skipped-changeset Unverified entries would trigger the
    // critical profile gate and cause unrelated violations.
    let report = VerificationPipeline::run_with_changeset(
        &ctx,
        Some("change noop base=0\nauthor tester\nend\n"),
        Some(&graph),
    );

    assert_eq!(
        report.policy_decision,
        Some(PolicyDecision::Passed),
        "Assumed package in critical with Strong approval must produce Passed; got {:?}",
        report.policy_decision
    );
    // Audit must record the approval used for that scope
    assert!(
        report.policy_audit.as_ref().is_some_and(|audit| {
            audit.entries.iter().any(|e| {
                e.scope == scope
                    && e.gate_decision == "passed"
                    && e.approval_used.as_deref() == Some("security-team")
            })
        }),
        "audit must record Strong approval used for assumed package scope"
    );
}

#[test]
fn pipeline_blocks_profile_rule_mismatch_with_stable_code() {
    let graph = empty_graph();
    let solver = SimpleSolver;
    let rules = vec![PolicyRule::ProfileGate("dev".into())];
    let ctx = PipelineContext {
        graph: &graph,
        manifests: &[],
        profile: "prod",
        solver: &solver,
        approvals: &[],
        rules: &rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };

    let report = VerificationPipeline::run(&ctx);

    assert_eq!(report.verified_profile.as_deref(), Some("prod"));
    assert_eq!(report.profile_diagnostics.len(), 1);
    let diagnostic = &report.profile_diagnostics[0];
    assert_eq!(
        diagnostic.code,
        ail_verify::report::VERIFY_PROFILE_RULE_MISMATCH
    );
    assert_eq!(diagnostic.requested_profile, "prod");
    assert_eq!(diagnostic.policy_profile, "dev");
    assert!(diagnostic.blocking);
    assert!(matches!(
        report.policy_decision,
        Some(PolicyDecision::Failed(ref violations))
            if violations
                .iter()
                .any(|violation| violation.code == ail_verify::report::VERIFY_PROFILE_RULE_MISMATCH)
    ));
}
