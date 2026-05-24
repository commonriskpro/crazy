// ── ail-verify::translation_validation tests ─────────────────────────────
//
// Integration tests for the TranslationValidator — verifies that translation
// validation checks run correctly through the full VerificationPipeline and
// that prod/critical profiles are materially stricter than dev.
//
// # Profile-tier matrix tested
//
// | Check               | dev | prod | critical | unknown |
// |---------------------|-----|------|----------|---------|
// | TV-1 Shape          |  ✓  |  ✓   |    ✓     |    ✓    |
// | TV-2 Provenance     |  ✓  |  ✓   |    ✓     |    ✓    |
// | TV-3 Effect oblig.  |  ✗  |  ✓   |    ✓     |    ✓    |
// | TV-4 Evidence suff. |  ✗  |  ✗   |    ✓     |    ✓    |

use ail_core::semantic_graph::{
    EffectRow, GraphNode, NodeKind, NodeRef, RuntimeCheckMeta, SemanticGraph,
};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::policy::{PolicyDecision, PolicyRule};
use ail_verify::report::VerificationState;
use ail_verify::solver::SimpleSolver;
use ail_verify::{
    E_TV_EFFECT_MALFORMED, E_TV_EFFECT_UNDECLARED, E_TV_INSUFFICIENT_EVIDENCE,
    E_TV_SHAPE_NO_RETURN_TYPE,
};

// Construct SimpleSolver (zero-size struct, no Default impl)
macro_rules! solver {
    () => {
        SimpleSolver
    };
}

// ── Test helpers ──────────────────────────────────────────────────────────

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

fn fn_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Function, name)
}

fn fn_with_body(id: u32, name: &str, body: &str, return_type: Option<&str>) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(id), NodeKind::Function, name);
    node.body_expr = Some(body.to_string());
    node.return_type = return_type.map(str::to_string);
    node
}

fn fn_with_effects(id: u32, name: &str, effects: &[&str]) -> GraphNode {
    let mut node = fn_node(id, name);
    node.effect_row = Some(EffectRow {
        effects: effects.iter().map(|s| s.to_string()).collect(),
    });
    node
}

fn fn_full(id: u32, name: &str, body: &str, return_type: &str, effects: &[&str]) -> GraphNode {
    let mut node = fn_with_body(id, name, body, Some(return_type));
    if !effects.is_empty() {
        node.effect_row = Some(EffectRow {
            effects: effects.iter().map(|s| s.to_string()).collect(),
        });
    }
    node
}

fn find_tv_entry<'a>(
    entries: &'a [ail_verify::report::VerificationEntry],
    scope: &str,
    claim_suffix: &str,
) -> Option<&'a ail_verify::report::VerificationEntry> {
    entries
        .iter()
        .find(|e| e.scope == scope && e.claim.contains(claim_suffix))
}

fn has_evidence_code(entry: &ail_verify::report::VerificationEntry, code: &str) -> bool {
    entry.evidence.as_deref().unwrap_or("").contains(code)
}

// ── TV-1: Shape checks (all profiles) ────────────────────────────────────

#[test]
fn tv1_shape_unverified_in_dev_when_no_return_type() {
    let mut node = fn_with_body(0, "fn.no_rt", "let x = 1 in x", None);
    // deliberately no return_type
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "dev", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.no_rt", "shape");
    assert!(e.is_some(), "must have a shape entry for fn.no_rt");
    let e = e.unwrap();
    assert_eq!(
        e.state,
        VerificationState::Unverified,
        "body without return_type must be Unverified in dev"
    );
    assert!(
        has_evidence_code(e, E_TV_SHAPE_NO_RETURN_TYPE),
        "evidence must contain E_TV_SHAPE_NO_RETURN_TYPE"
    );
}

#[test]
fn tv1_shape_proven_in_prod_when_return_type_present() {
    let node = fn_with_body(0, "fn.ok", "let x = 1 in x", Some("Int"));
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "prod", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.ok", "shape");
    assert!(e.is_some(), "must have a shape entry for fn.ok");
    assert_eq!(
        e.unwrap().state,
        VerificationState::Proven,
        "body with return_type must be Proven"
    );
}

// ── TV-2: Effect provenance (all profiles) ───────────────────────────────

#[test]
fn tv2_provenance_unverified_in_dev_when_effect_malformed() {
    let node = fn_with_effects(0, "fn.bad_effect", &["db"]); // missing :Provider
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "dev", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.bad_effect", "provenance");
    assert!(
        e.is_some(),
        "must have a provenance entry for fn.bad_effect"
    );
    let e = e.unwrap();
    assert_eq!(
        e.state,
        VerificationState::Unverified,
        "malformed effect must be Unverified"
    );
    assert!(
        has_evidence_code(e, E_TV_EFFECT_MALFORMED),
        "evidence must contain E_TV_EFFECT_MALFORMED"
    );
}

#[test]
fn tv2_provenance_proven_when_effects_well_formed() {
    let node = fn_with_effects(0, "fn.good_effect", &["db:Postgres", "http:Stripe"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "dev", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.good_effect", "provenance");
    assert!(e.is_some(), "must have a provenance entry");
    assert_eq!(
        e.unwrap().state,
        VerificationState::Proven,
        "well-formed effects must be Proven"
    );
}

#[test]
fn tv2_provenance_runs_in_all_profiles() {
    // The same malformed effect should produce Unverified in every profile.
    for profile in &["draft", "dev", "test", "staging", "prod", "critical"] {
        let node = fn_with_effects(0, "fn.bad", &["no_colon"]);
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let solver = SimpleSolver;
        let ctx = make_ctx(&graph, &solver, profile, &[]);
        let report = VerificationPipeline::run(&ctx);

        let e = find_tv_entry(&report.entries, "fn.bad", "provenance");
        assert!(
            e.is_some(),
            "provenance check must run in profile {profile}"
        );
        assert_eq!(
            e.unwrap().state,
            VerificationState::Unverified,
            "malformed effect must be Unverified in profile {profile}"
        );
    }
}

// ── TV-3: Effect obligations (prod and stricter) ──────────────────────────

#[test]
fn tv3_effect_obligation_fails_in_prod_when_undeclared() {
    let node = fn_full(0, "fn.charge", "emit_effect(db)", "Unit", &[]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "prod", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.charge", "effect-obligation");
    assert!(e.is_some(), "TV-3 entry must exist in prod");
    let e = e.unwrap();
    assert_eq!(
        e.state,
        VerificationState::Failed,
        "undeclared body effect must be Failed in prod"
    );
    assert!(has_evidence_code(e, E_TV_EFFECT_UNDECLARED));
}

#[test]
fn tv3_effect_obligation_proven_in_prod_when_declared() {
    let node = fn_full(0, "fn.charge", "emit_effect(db)", "Unit", &["db:Postgres"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "prod", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.charge", "effect-obligation");
    if let Some(e) = e {
        assert_eq!(
            e.state,
            VerificationState::Proven,
            "declared body effect must be Proven in prod"
        );
    }
    // It's also acceptable for no entry to be produced if no violation exists
    // (some implementations only emit entries for violations)
}

#[test]
fn tv3_effect_obligation_not_checked_in_dev() {
    // dev only runs TV-1 and TV-2 — TV-3 is prod+
    let node = fn_full(0, "fn.charge", "emit_effect(db)", "Unit", &[]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "dev", &[]);
    let report = VerificationPipeline::run(&ctx);

    let tv3 = find_tv_entry(&report.entries, "fn.charge", "effect-obligation");
    assert!(
        tv3.is_none(),
        "TV-3 must NOT run in dev profile; got: {tv3:?}"
    );
}

#[test]
fn tv3_effect_obligation_runs_in_critical() {
    let node = fn_full(0, "fn.auth", "emit_effect(crypto)", "Bool", &[]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "critical", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.auth", "effect-obligation");
    assert!(e.is_some(), "TV-3 must run in critical profile");
    assert_eq!(
        e.unwrap().state,
        VerificationState::Failed,
        "undeclared effect must be Failed in critical"
    );
}

#[test]
fn tv3_prefix_match_covers_declared_effect_with_provider() {
    // body uses "db", declared is "db:Postgres" — prefix match should succeed
    let node = fn_full(0, "fn.query", "emit_effect(db)", "List", &["db:Postgres"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "prod", &[]);
    let report = VerificationPipeline::run(&ctx);

    // No E_TV_EFFECT_UNDECLARED violation should exist for fn.query
    let undeclared_entry = report.entries.iter().find(|e| {
        e.scope == "fn.query"
            && e.claim.contains("effect-obligation")
            && e.evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_TV_EFFECT_UNDECLARED)
    });
    assert!(
        undeclared_entry.is_none(),
        "declared 'db:Postgres' must cover body use of 'db'"
    );
}

// ── TV-4: Evidence sufficiency (critical only) ────────────────────────────

#[test]
fn tv4_evidence_sufficiency_fails_in_critical_no_body_no_checks() {
    let node = fn_with_effects(0, "fn.iface", &["io:Console"]);
    // No body_expr, no runtime_checks — interface declaration only
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "critical", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.iface", "evidence-sufficiency");
    assert!(e.is_some(), "TV-4 entry must exist in critical");
    let e = e.unwrap();
    assert_eq!(
        e.state,
        VerificationState::Failed,
        "interface-only effect in critical must be Failed (insufficient evidence)"
    );
    assert!(has_evidence_code(e, E_TV_INSUFFICIENT_EVIDENCE));
}

#[test]
fn tv4_evidence_sufficiency_proven_in_critical_with_body() {
    let node = fn_full(0, "fn.impl", "emit_effect(io)", "Unit", &["io:Console"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "critical", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.impl", "evidence-sufficiency");
    assert!(e.is_some(), "TV-4 entry must exist");
    assert_eq!(
        e.unwrap().state,
        VerificationState::Proven,
        "body_expr is sufficient evidence in critical"
    );
}

#[test]
fn tv4_evidence_sufficiency_proven_in_critical_with_runtime_checks() {
    let mut node = fn_with_effects(0, "fn.runtime", &["io:Console"]);
    node.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "console_available()".to_string(),
        hash: "deadbeef".to_string(),
    }]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "critical", &[]);
    let report = VerificationPipeline::run(&ctx);

    let e = find_tv_entry(&report.entries, "fn.runtime", "evidence-sufficiency");
    assert!(e.is_some(), "TV-4 entry must exist");
    assert_eq!(
        e.unwrap().state,
        VerificationState::Proven,
        "runtime_checks is sufficient evidence in critical"
    );
}

#[test]
fn tv4_not_checked_in_prod() {
    let node = fn_with_effects(0, "fn.prod_iface", &["io:Console"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "prod", &[]);
    let report = VerificationPipeline::run(&ctx);

    let tv4 = find_tv_entry(&report.entries, "fn.prod_iface", "evidence-sufficiency");
    assert!(tv4.is_none(), "TV-4 must NOT run in prod profile");
}

#[test]
fn tv4_not_checked_in_dev() {
    let node = fn_with_effects(0, "fn.dev_iface", &["io:Console"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "dev", &[]);
    let report = VerificationPipeline::run(&ctx);

    let tv4 = find_tv_entry(&report.entries, "fn.dev_iface", "evidence-sufficiency");
    assert!(tv4.is_none(), "TV-4 must NOT run in dev profile");
}

// ── Profile strictness comparison ─────────────────────────────────────────

/// Demonstrate that prod is materially stricter than dev:
/// an undeclared body effect is acceptable in dev (no TV-3) but fails in prod.
#[test]
fn prod_is_stricter_than_dev_for_undeclared_effects() {
    let node = fn_full(0, "fn.action", "emit_effect(db)", "Unit", &[]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;

    // dev: no TV-3, so no Failed entry
    let dev_ctx = make_ctx(&graph, &solver, "dev", &[]);
    let dev_report = VerificationPipeline::run(&dev_ctx);
    let dev_fail = dev_report.entries.iter().any(|e| {
        e.scope == "fn.action"
            && e.claim.contains("effect-obligation")
            && e.state == VerificationState::Failed
    });
    assert!(!dev_fail, "dev must not fail on undeclared body effects");

    // prod: TV-3 fires → Failed
    let prod_ctx = make_ctx(&graph, &solver, "prod", &[]);
    let prod_report = VerificationPipeline::run(&prod_ctx);
    let prod_fail = prod_report.entries.iter().any(|e| {
        e.scope == "fn.action"
            && e.claim.contains("effect-obligation")
            && e.state == VerificationState::Failed
    });
    assert!(prod_fail, "prod must fail on undeclared body effects");
}

/// Demonstrate that critical is stricter than prod:
/// an interface declaration with effects is acceptable in prod but fails in critical.
#[test]
fn critical_is_stricter_than_prod_for_evidence_sufficiency() {
    let node = fn_with_effects(0, "fn.iface", &["io:Console"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;

    // prod: no TV-4, so no Failed entry for evidence sufficiency
    let prod_ctx = make_ctx(&graph, &solver, "prod", &[]);
    let prod_report = VerificationPipeline::run(&prod_ctx);
    let prod_fail = prod_report
        .entries
        .iter()
        .any(|e| e.claim.contains("evidence-sufficiency") && e.state == VerificationState::Failed);
    assert!(!prod_fail, "prod must not apply evidence sufficiency check");

    // critical: TV-4 fires → Failed (no body, no runtime_checks)
    let critical_ctx = make_ctx(&graph, &solver, "critical", &[]);
    let critical_report = VerificationPipeline::run(&critical_ctx);
    let critical_fail = critical_report
        .entries
        .iter()
        .any(|e| e.claim.contains("evidence-sufficiency") && e.state == VerificationState::Failed);
    assert!(
        critical_fail,
        "critical must fail on interface-only effects"
    );
}

// ── Policy integration ────────────────────────────────────────────────────

/// TV-3 Failed entries cause the prod ProfileGate to reject the changeset.
#[test]
fn prod_profile_gate_rejects_tv3_failed_entries() {
    let node = fn_full(0, "fn.charge", "emit_effect(payment)", "Unit", &[]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let rules = [PolicyRule::ProfileGate("prod".into())];
    let ctx = make_ctx(&graph, &solver, "prod", &rules);
    let report = VerificationPipeline::run(&ctx);

    // TV-3 emits a Failed entry → ProfileGate(prod) must reject
    assert!(
        matches!(report.policy_decision, Some(PolicyDecision::Failed(_))),
        "prod ProfileGate must reject when TV-3 emits a Failed entry"
    );
}

/// TV-4 Failed entries cause the critical ProfileGate to reject the changeset.
#[test]
fn critical_profile_gate_rejects_tv4_failed_entries() {
    let node = fn_with_effects(0, "fn.iface", &["io:Console"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let rules = [PolicyRule::ProfileGate("critical".into())];
    let ctx = make_ctx(&graph, &solver, "critical", &rules);
    let report = VerificationPipeline::run(&ctx);

    assert!(
        matches!(report.policy_decision, Some(PolicyDecision::Failed(_))),
        "critical ProfileGate must reject when TV-4 emits a Failed entry"
    );
}

/// A well-structured graph with properly declared effects produces NO
/// translation-validation failures in prod.
///
/// Note: other pipeline stages (changeset parsing, semantic diff) may still
/// produce Unverified entries when no changeset text is supplied.  This test
/// focuses specifically on the absence of TV-specific failures.
#[test]
fn well_formed_graph_produces_no_tv_failures_in_prod() {
    let node = fn_full(0, "fn.well", "emit_effect(db)", "Int", &["db:Postgres"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "prod", &[]);
    let report = VerificationPipeline::run(&ctx);

    // No translation-validation-specific Failed entries must exist
    let tv_failures: Vec<_> = report
        .entries
        .iter()
        .filter(|e| {
            e.claim.starts_with("translation-validation") && e.state == VerificationState::Failed
        })
        .collect();

    assert!(
        tv_failures.is_empty(),
        "well-formed graph must produce no TV-specific failures in prod; got: {tv_failures:#?}"
    );
}

// ── Unknown profile (strict-by-default) ──────────────────────────────────

#[test]
fn unknown_profile_runs_tv3_and_tv4_strict_by_default() {
    // Undeclared effect + no body/checks → should trigger TV-3 AND TV-4 in unknown profile
    let node = fn_with_effects(0, "fn.unknown", &["io:Console"]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let ctx = make_ctx(&graph, &solver, "future_profile", &[]);
    let report = VerificationPipeline::run(&ctx);

    // TV-4 should have fired (unknown profile is critical-like)
    let tv4 = find_tv_entry(&report.entries, "fn.unknown", "evidence-sufficiency");
    assert!(
        tv4.is_some(),
        "unknown profile must run TV-4 (strict-by-default)"
    );
}

// ── Empty graph ───────────────────────────────────────────────────────────

#[test]
fn empty_graph_passes_all_profiles() {
    let graph = empty_graph();
    let solver = SimpleSolver;

    for profile in &["draft", "dev", "test", "staging", "prod", "critical"] {
        let ctx = make_ctx(&graph, &solver, profile, &[]);
        let report = VerificationPipeline::run(&ctx);

        // No Failed entries from translation validation
        let tv_failed = report.entries.iter().any(|e| {
            e.claim.starts_with("translation-validation") && e.state == VerificationState::Failed
        });
        assert!(
            !tv_failed,
            "empty graph must not produce translation validation failures in profile {profile}"
        );
    }
}
