use super::helpers::*;

#[test]
fn draft_profile_passes_unverified_but_emits_warning() {
    let report = report_with(vec![entry(
        "type",
        "draft.fn",
        VerificationState::Unverified,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("draft".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    // Must NOT hard-block (Failed) — draft allows Unverified
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "draft allows Unverified — must not hard-block"
    );
    // But must emit warnings (PassedWithWarnings, not bare Passed)
    match decision {
        PolicyDecision::PassedWithWarnings(warnings) => {
            assert!(
                warnings.iter().any(|w| w.scope == "draft.fn"),
                "must warn about unverified entry in draft; warnings: {warnings:?}"
            );
        }
        PolicyDecision::Passed => {
            panic!("draft with Unverified must emit PassedWithWarnings, not bare Passed");
        }
        other => panic!("expected PassedWithWarnings, got {other:?}"),
    }
}

#[test]
fn draft_profile_passes_clean_report_without_warnings() {
    let report = report_with(vec![entry("type", "proven.fn", VerificationState::Proven)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("draft".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "draft with only Proven entries must give bare Passed"
    );
}

// ── R2-5: Dev profile — boundary-required for Assumed ─────────────────────
//
// dev allows "assumed with boundary" but must block bare Assumed
// that has no boundary evidence (no approval and no evidence).
// Also: "private unverified only if annotated" — unannotated private
// Unverified should warn.

#[test]
fn dev_profile_blocks_assumed_without_boundary() {
    // Assumed without any approval or evidence = no boundary → block in dev
    let report = report_with(vec![entry(
        "type",
        "internal.fn",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("dev".into())],
        approvals: &[], // no approval = no boundary
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(
            decision,
            PolicyDecision::Failed(_) | PolicyDecision::ApprovalRequired(_)
        ),
        "dev must require boundary/approval for bare Assumed entries"
    );
}

#[test]
fn dev_profile_passes_assumed_with_approval() {
    let report = report_with(vec![entry(
        "type",
        "internal.fn",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("dev".into())],
        approvals: &[strong_approval_for("internal.fn")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "dev must pass Assumed with explicit approval/boundary"
    );
}

#[test]
fn dev_profile_blocks_unverified_public_scope() {
    // This was covered in R1 but must still hold in R2 with the new input shape
    let report = report_with(vec![entry(
        "type",
        "pub::my_fn",
        VerificationState::Unverified,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("dev".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "dev must block Unverified in public scope"
    );
}

// ── R2-6: Test profile — test-only assumptions ─────────────────────────────
//
// test profile allows "test-only assumptions" with weaker gating than prod.
// It still blocks Failed and unsafe without approval.
// Assumed is allowed (test-only boundary) without requiring strong approval.

#[test]
fn test_profile_allows_assumed_without_strong_approval() {
    let report = report_with(vec![entry(
        "type",
        "mock.service",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("test".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "test profile must allow Assumed entries (test-only assumptions)"
    );
}

#[test]
fn test_profile_blocks_unsafe_without_approval() {
    let report = report_with(vec![entry("type", "unsafe.fn", VerificationState::Unsafe)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("test".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "test profile must still block Unsafe without approval"
    );
}

#[test]
fn test_profile_blocks_failed_entries() {
    let report = report_with(vec![entry("type", "broken.fn", VerificationState::Failed)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("test".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "test profile must block Failed entries"
    );
}

// ── R2-7: Staging — unapproved Assumed blocks ─────────────────────────────

#[test]
fn staging_blocks_assumed_without_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "external.stripe",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("staging".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(
            decision,
            PolicyDecision::Failed(_) | PolicyDecision::ApprovalRequired(_)
        ),
        "staging must block/require-approval for unapproved Assumed"
    );
}

#[test]
fn staging_passes_assumed_with_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "external.stripe",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("staging".into())],
        approvals: &[strong_approval_for("external.stripe")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "staging must pass Assumed with explicit strong approval"
    );
}

#[test]
fn staging_blocks_unverified() {
    let report = report_with(vec![entry("type", "any.fn", VerificationState::Unverified)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("staging".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "staging must block Unverified"
    );
}

// ── R2-8: Prod — unapproved Assumed blocks; security exception for Unsafe ──
//
// prod blocks Unsafe EXCEPT when there's a security-exception approval.
// prod blocks unapproved Assumed.
// A "security exception" is a Strong approval on the scope.

#[test]
fn prod_blocks_assumed_without_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "external.payment",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(
            decision,
            PolicyDecision::Failed(_) | PolicyDecision::ApprovalRequired(_)
        ),
        "prod must block unapproved Assumed"
    );
}

#[test]
fn prod_assumed_without_approval_is_machine_readable_approval_required() {
    let report = report_with(vec![entry(
        "boundary",
        "external.payment",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };

    match PolicyEngine::evaluate(&input) {
        PolicyDecision::ApprovalRequired(scopes) => assert_eq!(scopes, vec!["external.payment"]),
        other => panic!("prod Assumed without approval must be ApprovalRequired, got {other:?}"),
    }
}

#[test]
fn prod_passes_assumed_with_strong_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "external.payment",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[strong_approval_for("external.payment")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "prod must pass Assumed with strong approval"
    );
}

#[test]
fn prod_blocks_unsafe_without_security_exception() {
    let report = report_with(vec![entry("type", "unsafe.fn", VerificationState::Unsafe)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "prod must block Unsafe without security exception"
    );
}

#[test]
fn prod_blocks_unsafe_with_only_weak_security_exception() {
    let report = report_with(vec![entry("type", "unsafe.fn", VerificationState::Unsafe)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[weak_approval_for("unsafe.fn")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };

    match PolicyEngine::evaluate(&input) {
        PolicyDecision::Failed(violations) => assert!(
            violations.iter().any(|v| v.code == POLICY_UNSAFE_BLOCKED),
            "prod must reject weak unsafe security exceptions with POLICY_UNSAFE_BLOCKED; got {violations:?}"
        ),
        other => panic!("expected prod to reject weak unsafe security exception, got {other:?}"),
    }
}

#[test]
fn prod_passes_unsafe_with_security_exception() {
    // A "security exception" is represented as a Strong approval
    let report = report_with(vec![entry("type", "unsafe.fn", VerificationState::Unsafe)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[strong_approval_for("unsafe.fn")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "prod must allow Unsafe with strong security-exception approval"
    );
}

#[test]
fn prod_blocks_assumed_with_only_weak_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "external.payment",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[weak_approval_for("external.payment")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };

    match PolicyEngine::evaluate(&input) {
        PolicyDecision::Failed(violations) => assert!(
            violations.iter().any(|v| v.code == POLICY_WEAK_ASSUMPTION),
            "prod must reject weak Assumed approvals with POLICY_WEAK_ASSUMPTION; got {violations:?}"
        ),
        other => panic!("expected prod to reject weak Assumed approval, got {other:?}"),
    }
}

#[test]
fn prod_blocks_unverified_even_with_strong_approval() {
    let report = report_with(vec![entry(
        "type",
        "external.ai_response",
        VerificationState::Unverified,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[strong_approval_for("external.ai_response")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };

    match PolicyEngine::evaluate(&input) {
        PolicyDecision::Failed(violations) => assert!(
            violations.iter().any(|v| v.code == POLICY_PROFILE_GATE),
            "prod must reject Unverified regardless of approval records; got {violations:?}"
        ),
        other => panic!("expected prod to reject approved Unverified entry, got {other:?}"),
    }
}

#[test]
fn prod_passes_runtime_checked_entry() {
    let report = report_with(vec![entry(
        "type",
        "validated.payload",
        VerificationState::RuntimeChecked,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };

    assert_eq!(PolicyEngine::evaluate(&input), PolicyDecision::Passed);
}

#[test]
fn prod_audit_classifies_mixed_gate_results() {
    let report = report_with(vec![
        entry("type", "safe.fn", VerificationState::Proven),
        entry(
            "type",
            "validated.payload",
            VerificationState::RuntimeChecked,
        ),
        entry("boundary", "external.payment", VerificationState::Assumed),
        entry(
            "type",
            "external.ai_response",
            VerificationState::Unverified,
        ),
        entry("type", "unsafe.fn", VerificationState::Unsafe),
    ]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[weak_approval_for("unsafe.fn")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };

    let (decision, audit) = PolicyEngine::evaluate_with_audit(&input);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "mixed prod report with unsafe/unverified/unapproved assumed entries must fail"
    );
    assert_eq!(audit.profile, "prod");
    assert!(
        audit.entries.iter().any(|e| {
            e.scope == "safe.fn" && e.state == "proven" && e.gate_decision == "passed"
        })
    );
    assert!(audit.entries.iter().any(|e| {
        e.scope == "validated.payload"
            && e.state == "runtime_checked"
            && e.gate_decision == "passed"
    }));
    assert!(audit.entries.iter().any(|e| {
        e.scope == "external.payment"
            && e.state == "assumed"
            && e.gate_decision == "approval_required"
    }));
    assert!(audit.entries.iter().any(|e| {
        e.scope == "external.ai_response" && e.state == "unverified" && e.gate_decision == "failed"
    }));
    assert!(
        audit.entries.iter().any(|e| {
            e.scope == "unsafe.fn" && e.state == "unsafe" && e.gate_decision == "failed"
        })
    );
}

// ── R2-9: Critical — strong-approval/weak-assumption gate ─────────────────
//
// critical profile:
//   - Assumed requires Strong approval (Weak is not enough → POLICY_WEAK_ASSUMPTION)
//   - Unsafe is always blocked (even with Weak approval)
//   - RuntimeChecked only passes if policy accepts it (here: always allowed for now)
//   - Unverified always blocks
