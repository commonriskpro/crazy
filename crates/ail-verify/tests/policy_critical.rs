// ── ail-verify::policy — Wave 6D critical profile rigor tests ────────────
//
// These tests prove that the `critical` profile is MEANINGFULLY stricter than
// `prod`, not merely another label.  Each test documents exactly where and why
// the two profiles diverge.
//
// Behavioral differences between `critical` and `prod`:
//
//   1. Unsafe  — prod: passes with Strong security-exception approval.
//                critical: ALWAYS blocks, no approval exemption.
//
//   2. RuntimeChecked — prod: passes silently (no warning).
//                       critical: PassedWithWarnings (POLICY_RUNTIME_CHECK_ADVISORY).
//
//   3. Solver diagnostics — prod: informational (not checked by policy gate).
//                           critical: each Timeout/ResourceLimited/Unsupported
//                           diagnostic is a POLICY_SOLVER_DIAGNOSTIC_BLOCKED violation.
//
// Unknown profiles mirror `critical` (strict-by-default fallback).

use ail_verify::policy::{
    ApprovalRecord, ApprovalStrength, POLICY_CRITICAL_APPROVAL_INCOMPLETE,
    POLICY_RUNTIME_CHECK_ADVISORY, POLICY_SOLVER_DIAGNOSTIC_BLOCKED, POLICY_WEAK_ASSUMPTION,
    PolicyDecision, PolicyEngine, PolicyInput, PolicyRule,
};
use ail_verify::report::{
    SolverDiagnostic, SolverDiagnosticStatus, VerificationEntry, VerificationReport,
    VerificationState,
};

// ── Helpers ───────────────────────────────────────────────────────────────

fn entry(scope: &str, state: VerificationState) -> VerificationEntry {
    VerificationEntry {
        claim: "test-claim".into(),
        state,
        scope: scope.into(),
        evidence: None,
        blocking: false,
        repair_options: vec![],
    }
}

fn report_with(entries: Vec<VerificationEntry>) -> VerificationReport {
    VerificationReport {
        entries,
        ..Default::default()
    }
}

fn report_with_solver_diag(
    entries: Vec<VerificationEntry>,
    diagnostics: Vec<SolverDiagnostic>,
) -> VerificationReport {
    VerificationReport {
        entries,
        solver_diagnostics: diagnostics,
        ..Default::default()
    }
}

fn strong_approval(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.to_string(),
        approver: "security-team".to_string(),
        reason: "approved under security exception".to_string(),
        strength: ApprovalStrength::Strong,
    }
}

fn solver_diag(obligation_id: &str, status: SolverDiagnosticStatus) -> SolverDiagnostic {
    let reason = match status {
        SolverDiagnosticStatus::Timeout => "solver_timeout: budget exceeded",
        SolverDiagnosticStatus::ResourceLimited => "solver_resource_limited: memory exhausted",
        SolverDiagnosticStatus::Unsupported => "solver_unsupported: quantifier fragment",
    };
    SolverDiagnostic {
        code: status.issue_code().to_string(),
        obligation_id: obligation_id.to_string(),
        source_stage: "contract".to_string(),
        status,
        reason: reason.to_string(),
        repair_options: vec![],
    }
}

fn evaluate_profile(
    report: &VerificationReport,
    profile: &str,
    approvals: &[ApprovalRecord],
) -> PolicyDecision {
    PolicyEngine::evaluate(&PolicyInput {
        report,
        rules: &[PolicyRule::ProfileGate(profile.into())],
        approvals,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    })
}

// ── Difference 1: Unsafe handling ─────────────────────────────────────────
//
// prod allows Unsafe with a Strong security-exception approval.
// critical ALWAYS blocks Unsafe — no exception.

#[test]
fn prod_passes_unsafe_with_strong_approval_but_critical_does_not() {
    let report = report_with(vec![entry("fn.unsafe_op", VerificationState::Unsafe)]);
    let approvals = [strong_approval("fn.unsafe_op")];

    // prod: Strong approval is a valid security exception → passes
    let prod_decision = evaluate_profile(&report, "prod", &approvals);
    assert!(
        !matches!(prod_decision, PolicyDecision::Failed(_)),
        "prod must pass Unsafe with Strong security-exception approval; got {prod_decision:?}"
    );

    // critical: Unsafe ALWAYS blocks, regardless of approval strength
    let critical_decision = evaluate_profile(&report, "critical", &approvals);
    assert!(
        matches!(critical_decision, PolicyDecision::Failed(_)),
        "critical must block Unsafe even with Strong approval — no exceptions; \
         got {critical_decision:?}"
    );
}

#[test]
fn critical_unsafe_violation_uses_stable_profile_gate_code() {
    let report = report_with(vec![entry("fn.unsafe_op", VerificationState::Unsafe)]);
    let decision = evaluate_profile(&report, "critical", &[]);
    match decision {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations
                    .iter()
                    .any(|v| v.code == ail_verify::policy::POLICY_PROFILE_GATE),
                "critical Unsafe block must use POLICY_PROFILE_GATE code; got {violations:?}"
            );
        }
        other => panic!("expected Failed for critical+Unsafe, got {other:?}"),
    }
}

// ── Difference 2: RuntimeChecked — critical warns, prod passes cleanly ────

#[test]
fn prod_passes_runtime_checked_cleanly_no_warning() {
    let report = report_with(vec![entry(
        "fn.runtime_val",
        VerificationState::RuntimeChecked,
    )]);
    let decision = evaluate_profile(&report, "prod", &[]);
    assert_eq!(
        decision,
        PolicyDecision::Passed,
        "prod must pass RuntimeChecked with bare Passed (no advisory warning); \
         got {decision:?}"
    );
}

#[test]
fn critical_runtime_checked_emits_advisory_warning_not_failure() {
    let report = report_with(vec![entry(
        "fn.runtime_val",
        VerificationState::RuntimeChecked,
    )]);
    let decision = evaluate_profile(&report, "critical", &[]);

    // Must NOT hard-block (Failed)
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "critical must NOT hard-block RuntimeChecked; got {decision:?}"
    );

    // Must emit PassedWithWarnings carrying POLICY_RUNTIME_CHECK_ADVISORY
    match &decision {
        PolicyDecision::PassedWithWarnings(warnings) => {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.code == POLICY_RUNTIME_CHECK_ADVISORY),
                "critical RuntimeChecked must emit POLICY_RUNTIME_CHECK_ADVISORY warning; \
                 got {warnings:?}"
            );
            assert!(
                warnings.iter().any(|w| w.scope == "fn.runtime_val"),
                "warning scope must match entry scope; got {warnings:?}"
            );
        }
        other => panic!("critical+RuntimeChecked must be PassedWithWarnings, got {other:?}"),
    }
}

#[test]
fn critical_runtime_check_advisory_is_stable_machine_readable_code() {
    // The constant must not be a Rust Debug representation.
    // We verify it's the expected stable ASCII identifier.
    assert_eq!(
        POLICY_RUNTIME_CHECK_ADVISORY,
        "POLICY_RUNTIME_CHECK_ADVISORY"
    );
}

// ── Difference 3: Solver diagnostics — critical blocks, prod allows ───────
//
// When report.solver_diagnostics is non-empty, critical profile emits
// POLICY_SOLVER_DIAGNOSTIC_BLOCKED violations for each diagnostic.
// prod profile leaves solver diagnostics informational (no violation).

#[test]
fn prod_does_not_block_on_solver_timeout_diagnostic() {
    // An Assumed entry (from solver timeout) with Strong approval — passes in prod
    let report = report_with_solver_diag(
        vec![entry("fn.contract_fn", VerificationState::Assumed)],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    let approvals = [strong_approval("fn.contract_fn")];
    let decision = evaluate_profile(&report, "prod", &approvals);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "prod must NOT block on solver diagnostic; got {decision:?}"
    );
}

#[test]
fn critical_blocks_on_solver_timeout_diagnostic() {
    let report = report_with_solver_diag(
        vec![entry("fn.contract_fn", VerificationState::Assumed)],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    let approvals = [strong_approval("fn.contract_fn")];
    let decision = evaluate_profile(&report, "critical", &approvals);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "critical must block on Timeout solver diagnostic; got {decision:?}"
    );
}

#[test]
fn critical_blocks_on_solver_resource_limited_diagnostic() {
    let report = report_with_solver_diag(
        vec![entry("fn.contract_fn", VerificationState::Assumed)],
        vec![solver_diag(
            "po_rl_1",
            SolverDiagnosticStatus::ResourceLimited,
        )],
    );
    let approvals = [strong_approval("fn.contract_fn")];
    let decision = evaluate_profile(&report, "critical", &approvals);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "critical must block on ResourceLimited solver diagnostic; got {decision:?}"
    );
}

#[test]
fn critical_blocks_on_solver_unsupported_diagnostic() {
    let report = report_with_solver_diag(
        vec![entry("fn.contract_fn", VerificationState::Assumed)],
        vec![solver_diag(
            "po_unsup_1",
            SolverDiagnosticStatus::Unsupported,
        )],
    );
    let approvals = [strong_approval("fn.contract_fn")];
    let decision = evaluate_profile(&report, "critical", &approvals);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "critical must block on Unsupported solver diagnostic; got {decision:?}"
    );
}

#[test]
fn critical_solver_violation_carries_stable_machine_readable_code() {
    let report = report_with_solver_diag(
        vec![],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    match evaluate_profile(&report, "critical", &[]) {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations
                    .iter()
                    .any(|v| v.code == POLICY_SOLVER_DIAGNOSTIC_BLOCKED),
                "solver diagnostic violation must use stable POLICY_SOLVER_DIAGNOSTIC_BLOCKED \
                 code; got {violations:?}"
            );
            // Scope must be the obligation_id, not a Rust Debug string
            assert!(
                violations.iter().any(|v| v.scope == "po_timeout_1"),
                "solver diagnostic violation scope must be the obligation_id; \
                 got {violations:?}"
            );
        }
        other => panic!("expected Failed for critical+solver diagnostic, got {other:?}"),
    }
}

#[test]
fn critical_blocks_all_three_solver_diagnostic_types_independently() {
    for (status, obligation_id) in [
        (SolverDiagnosticStatus::Timeout, "po_t"),
        (SolverDiagnosticStatus::ResourceLimited, "po_r"),
        (SolverDiagnosticStatus::Unsupported, "po_u"),
    ] {
        let report = report_with_solver_diag(vec![], vec![solver_diag(obligation_id, status)]);
        let decision = evaluate_profile(&report, "critical", &[]);
        assert!(
            matches!(decision, PolicyDecision::Failed(_)),
            "critical must block on {status:?} solver diagnostic; got {decision:?}"
        );
    }
}

#[test]
fn critical_no_solver_diagnostics_passes_clean_report() {
    // Baseline: a clean critical report (all Proven, no solver diagnostics) must pass.
    let report = report_with(vec![
        entry("fn.a", VerificationState::Proven),
        entry("fn.b", VerificationState::Proven),
    ]);
    let decision = evaluate_profile(&report, "critical", &[]);
    assert_eq!(
        decision,
        PolicyDecision::Passed,
        "critical with no diagnostics and all-Proven entries must give bare Passed"
    );
}

#[test]
fn critical_solver_diagnostic_violation_json_round_trips_stably() {
    // Machine-readable contract: PolicyDecision must serialize without Debug leaks.
    let report = report_with_solver_diag(
        vec![],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    let decision = evaluate_profile(&report, "critical", &[]);

    let full_report = VerificationReport {
        policy_decision: Some(decision),
        ..Default::default()
    };
    let json = serde_json::to_string(&full_report).expect("serialize");

    // The JSON must contain our stable code, not Rust Debug format
    assert!(
        json.contains("POLICY_SOLVER_DIAGNOSTIC_BLOCKED"),
        "serialized report must contain stable code; json={json}"
    );
    // Must NOT contain Rust variant names in Debug form
    assert!(
        !json.contains("Timeout\"") && !json.contains("ResourceLimited\""),
        "serialized report must not leak Rust Debug enum names; json={json}"
    );

    // Round-trip fidelity
    let decoded: VerificationReport = serde_json::from_str(&json).expect("deserialize");
    match decoded.policy_decision {
        Some(PolicyDecision::Failed(vs)) => {
            assert!(
                vs.iter()
                    .any(|v| v.code == POLICY_SOLVER_DIAGNOSTIC_BLOCKED)
            );
        }
        other => panic!("expected Failed after round-trip, got {other:?}"),
    }
}

// ── Unknown profile mirrors critical (strict-by-default) ─────────────────

#[test]
fn unknown_profile_blocks_solver_diagnostics_like_critical() {
    let report = report_with_solver_diag(
        vec![],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    let decision = evaluate_profile(&report, "totally_custom_profile", &[]);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "unknown profiles must mirror critical for solver diagnostics; got {decision:?}"
    );
}

#[test]
fn unknown_profile_runtime_checked_emits_advisory_like_critical() {
    let report = report_with(vec![entry(
        "fn.runtime_val",
        VerificationState::RuntimeChecked,
    )]);
    let decision = evaluate_profile(&report, "totally_custom_profile", &[]);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "unknown profile must NOT hard-block RuntimeChecked; got {decision:?}"
    );
    match &decision {
        PolicyDecision::PassedWithWarnings(warnings) => {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.code == POLICY_RUNTIME_CHECK_ADVISORY),
                "unknown profile RuntimeChecked must emit POLICY_RUNTIME_CHECK_ADVISORY; \
                 got {warnings:?}"
            );
        }
        other => panic!("expected PassedWithWarnings for unknown+RuntimeChecked, got {other:?}"),
    }
}

// ── Prod behavior preserved (regression guard) ────────────────────────────

#[test]
fn prod_assumed_without_approval_is_approval_required_not_solver_blocked() {
    // prod must NOT apply solver-diagnostic sweep — that's critical-only
    let report = report_with_solver_diag(
        vec![entry("fn.assumed", VerificationState::Assumed)],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    // no approvals — prod Assumed → ApprovalRequired
    match evaluate_profile(&report, "prod", &[]) {
        PolicyDecision::ApprovalRequired(_) => {} // expected
        PolicyDecision::Failed(vs) => {
            // Must NOT be because of solver diagnostic
            assert!(
                !vs.iter()
                    .any(|v| v.code == POLICY_SOLVER_DIAGNOSTIC_BLOCKED),
                "prod must not emit POLICY_SOLVER_DIAGNOSTIC_BLOCKED; got {vs:?}"
            );
        }
        other => panic!("prod with unapproved Assumed must not hard-pass; got {other:?}"),
    }
}

#[test]
fn prod_assumed_with_strong_approval_passes_even_with_solver_diagnostic() {
    let report = report_with_solver_diag(
        vec![entry("fn.assumed", VerificationState::Assumed)],
        vec![solver_diag("po_timeout_1", SolverDiagnosticStatus::Timeout)],
    );
    let approvals = [strong_approval("fn.assumed")];
    let decision = evaluate_profile(&report, "prod", &approvals);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "prod must pass Assumed with Strong approval regardless of solver diagnostics; \
         got {decision:?}"
    );
}

// ── Assumed approval-strength preserved between critical and prod ─────────
//
// Both critical and prod require Strong approval for Assumed.
// This test is a regression guard ensuring prod wasn't accidentally tightened
// to critical semantics for Assumed (they should remain the same here).

#[test]
fn critical_blocks_strong_assumed_approval_without_approver() {
    let report = report_with(vec![entry("fn.assumed", VerificationState::Assumed)]);
    let approvals = [ApprovalRecord {
        scope: "fn.assumed".to_string(),
        approver: "   ".to_string(),
        reason: "reviewed critical boundary assumption".to_string(),
        strength: ApprovalStrength::Strong,
    }];

    match evaluate_profile(&report, "critical", &approvals) {
        PolicyDecision::Failed(violations) => assert!(
            violations
                .iter()
                .any(|v| v.code == POLICY_CRITICAL_APPROVAL_INCOMPLETE),
            "critical must reject anonymous Strong Assumed approvals with stable code; got {violations:?}"
        ),
        other => panic!("expected Failed for anonymous critical approval, got {other:?}"),
    }
}

#[test]
fn critical_blocks_strong_assumed_approval_without_reason() {
    let report = report_with(vec![entry("fn.assumed", VerificationState::Assumed)]);
    let approvals = [ApprovalRecord {
        scope: "fn.assumed".to_string(),
        approver: "security-team".to_string(),
        reason: "\t".to_string(),
        strength: ApprovalStrength::Strong,
    }];

    match evaluate_profile(&report, "critical", &approvals) {
        PolicyDecision::Failed(violations) => assert!(
            violations
                .iter()
                .any(|v| v.code == POLICY_CRITICAL_APPROVAL_INCOMPLETE),
            "critical must reject reasonless Strong Assumed approvals with stable code; got {violations:?}"
        ),
        other => panic!("expected Failed for reasonless critical approval, got {other:?}"),
    }
}

#[test]
fn unknown_profile_blocks_incomplete_assumed_approval_like_critical() {
    let report = report_with(vec![entry("fn.assumed", VerificationState::Assumed)]);
    let approvals = [ApprovalRecord {
        scope: "fn.assumed".to_string(),
        approver: "security-team".to_string(),
        reason: "".to_string(),
        strength: ApprovalStrength::Strong,
    }];

    match evaluate_profile(&report, "custom-critical-like", &approvals) {
        PolicyDecision::Failed(violations) => assert!(
            violations
                .iter()
                .any(|v| v.code == POLICY_CRITICAL_APPROVAL_INCOMPLETE),
            "unknown profiles must reject incomplete critical-style approvals; got {violations:?}"
        ),
        other => panic!("expected Failed for unknown profile incomplete approval, got {other:?}"),
    }
}

#[test]
fn critical_and_prod_both_reject_weak_assumed_approval() {
    let report = report_with(vec![entry("fn.assumed", VerificationState::Assumed)]);
    let weak_approvals = [ApprovalRecord {
        scope: "fn.assumed".to_string(),
        approver: "dev-team".to_string(),
        reason: "weak boundary assumption".to_string(),
        strength: ApprovalStrength::Weak,
    }];

    for profile in ["critical", "prod"] {
        let decision = evaluate_profile(&report, profile, &weak_approvals);
        match &decision {
            PolicyDecision::Failed(violations) => {
                assert!(
                    violations.iter().any(|v| v.code == POLICY_WEAK_ASSUMPTION),
                    "{profile} must reject weak Assumed with POLICY_WEAK_ASSUMPTION; \
                     got {violations:?}"
                );
            }
            other => panic!("{profile} must reject weak Assumed approval; got {other:?}"),
        }
    }
}
