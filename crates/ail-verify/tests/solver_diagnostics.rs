// ── ail-verify::report — solver diagnostic tests ──────────────────────────

use ail_core::semantic_graph::{ContractClauses, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::proof::{
    ClauseRole, ObligationAttempt, ObligationLedgerEntry, ObligationState, ProofObligation,
};
use ail_verify::report::{
    SolverDiagnostic, SolverDiagnosticStatus, VerificationReport,
    solver_diagnostic_status_from_reason,
};
use ail_verify::solver::{Solver, SolverOutcome};

struct DiagnosticSolver;

impl Solver for DiagnosticSolver {
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome {
        match obligation.predicate.as_str() {
            "timeout_predicate" => {
                SolverOutcome::Assumed("solver_timeout: exceeded solver budget".into())
            }
            "resource_predicate" => SolverOutcome::Assumed(
                "solver_resource_limited: exhausted solver memory budget".into(),
            ),
            "unsupported_predicate" => SolverOutcome::Unsupported,
            _ => SolverOutcome::Unsupported,
        }
    }
}

fn graph_with_solver_cases() -> SemanticGraph {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "diagnostic_fn");
    node.contract_clauses = Some(ContractClauses {
        requires: vec![
            "timeout_predicate".into(),
            "resource_predicate".into(),
            "unsupported_predicate".into(),
        ],
        ensures: vec![],
    });
    SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    }
}

fn make_ctx<'a>(graph: &'a SemanticGraph, solver: &'a dyn Solver) -> PipelineContext<'a> {
    PipelineContext {
        graph,
        manifests: &[],
        profile: "test",
        solver,
        approvals: &[],
        rules: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    }
}

fn ledger_entry_with_attempts(
    attempts: Vec<ObligationAttempt>,
    degradation_reason: Option<&str>,
) -> ObligationLedgerEntry {
    ObligationLedgerEntry {
        id: "po_test".into(),
        obligation: ProofObligation {
            predicate: "diagnostic_predicate".into(),
            role: ClauseRole::Requires,
            scope: "diagnostic_fn".into(),
        },
        state: ObligationState::Assumed("not proven".into()),
        source_stage: "contract".into(),
        attempts,
        degradation_reason: degradation_reason.map(str::to_owned),
        repair_options: vec![],
    }
}

#[test]
fn solver_diagnostic_status_serializes_stable_snake_case() {
    let report = VerificationReport {
        solver_diagnostics: vec![
            SolverDiagnostic {
                obligation_id: "po_1".into(),
                source_stage: "contract".into(),
                status: SolverDiagnosticStatus::Timeout,
                reason: "timed out".into(),
                repair_options: vec!["split the predicate".into()],
            },
            SolverDiagnostic {
                obligation_id: "po_2".into(),
                source_stage: "contract".into(),
                status: SolverDiagnosticStatus::ResourceLimited,
                reason: "resource budget exhausted".into(),
                repair_options: vec!["reduce search space".into()],
            },
            SolverDiagnostic {
                obligation_id: "po_3".into(),
                source_stage: "contract".into(),
                status: SolverDiagnosticStatus::Unsupported,
                reason: "predicate fragment unavailable".into(),
                repair_options: vec!["rewrite predicate".into()],
            },
        ],
        ..Default::default()
    };

    let json = serde_json::to_string(&report).expect("serialize report");

    assert!(json.contains(r#""status":"timeout""#));
    assert!(json.contains(r#""status":"resource_limited""#));
    assert!(json.contains(r#""status":"unsupported""#));
    assert!(!json.contains("Timeout"));
    assert!(!json.contains("ResourceLimited"));
    assert!(!json.contains("Unsupported"));
}

#[test]
fn solver_reason_classification_requires_solver_scoped_prefixes() {
    assert_eq!(
        solver_diagnostic_status_from_reason("solver_timeout: exceeded budget"),
        Some(SolverDiagnosticStatus::Timeout)
    );
    assert_eq!(
        solver_diagnostic_status_from_reason("solver_resource_limited: memory budget"),
        Some(SolverDiagnosticStatus::ResourceLimited)
    );
    assert_eq!(
        solver_diagnostic_status_from_reason("solver_unsupported: quantifier fragment"),
        Some(SolverDiagnosticStatus::Unsupported)
    );

    assert_eq!(solver_diagnostic_status_from_reason("timeout"), None);
    assert_eq!(
        solver_diagnostic_status_from_reason("timeout: request"),
        None
    );
    assert_eq!(
        solver_diagnostic_status_from_reason("resource_limited: host cap"),
        None
    );
    assert_eq!(
        solver_diagnostic_status_from_reason("unsupported: platform"),
        None
    );
}

#[test]
fn solver_diagnostics_ignore_degradation_reason_fallback() {
    let entry = ledger_entry_with_attempts(vec![], Some("solver_timeout: degrade note"));

    assert_eq!(SolverDiagnostic::from_ledger_entry(&entry), None);
}

#[test]
fn solver_diagnostics_accept_explicit_solver_attempt_statuses() {
    let entry = ledger_entry_with_attempts(
        vec![ObligationAttempt {
            stage: "solver".into(),
            outcome: "timeout".into(),
            evidence: None,
        }],
        Some("unrelated degradation reason"),
    );

    let diagnostic = SolverDiagnostic::from_ledger_entry(&entry).expect("solver diagnostic");

    assert_eq!(diagnostic.status, SolverDiagnosticStatus::Timeout);
    assert_eq!(diagnostic.reason, "timeout");
}

#[test]
fn pipeline_derives_solver_diagnostics_for_representative_outcomes() {
    let graph = graph_with_solver_cases();
    let solver = DiagnosticSolver;
    let ctx = make_ctx(&graph, &solver);

    let report = VerificationPipeline::run(&ctx);

    let statuses: Vec<SolverDiagnosticStatus> = report
        .solver_diagnostics
        .iter()
        .map(|diagnostic| diagnostic.status)
        .collect();

    assert_eq!(
        statuses,
        vec![
            SolverDiagnosticStatus::Timeout,
            SolverDiagnosticStatus::ResourceLimited,
            SolverDiagnosticStatus::Unsupported,
        ]
    );

    for status in [
        SolverDiagnosticStatus::Timeout,
        SolverDiagnosticStatus::ResourceLimited,
        SolverDiagnosticStatus::Unsupported,
    ] {
        let diagnostic = report
            .solver_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.status == status)
            .expect("status diagnostic present");
        assert!(
            !diagnostic.repair_options.is_empty(),
            "{status:?} diagnostics must include repair guidance"
        );
    }
}
