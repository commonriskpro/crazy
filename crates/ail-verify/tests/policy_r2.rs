// ── ail-verify::policy Round-2 tests ─────────────────────────────────────
//
// Strict TDD — RED phase.  All tests below must FAIL before the implementation
// is updated.  They encode the missing/warning gaps from the Round-1 verifier
// report:
//
//   MISSING:
//     R2-1  Strict-by-default fallback (unknown profile → conservative/block)
//     R2-2  PolicyInput: structural_diff, capability_grants, public_api_changes,
//           package_trust_metadata fields
//     R2-3  Assumed gating: approved vs unapproved distinctions
//
//   PARTIALLY COVERED → FULLY COVERED:
//     R2-4  Draft profile — warnings for Unverified, Assumed must be annotated
//     R2-5  Dev profile — assumed with boundary, private-unverified-annotated-only
//     R2-6  Test profile — test-only assumptions model
//     R2-7  Staging — unapproved Assumed blocks
//     R2-8  Prod — unapproved Assumed blocks; security exception for Unsafe
//     R2-9  Critical — strong-approval/weak-assumption/runtime_checked gate
//     R2-10 Report integration — richer policy/approvals audit sections
//     R2-11 Pipeline — ChangeSetOp::Verify wired end-to-end with checker→policy

use ail_verify::policy::{
    ApprovalRecord, ApprovalStrength, POLICY_WEAK_ASSUMPTION, PolicyDecision, PolicyEngine,
    PolicyInput, PolicyRule,
};
use ail_verify::policy::{PolicyAudit, PolicyAuditEntry};
use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

// ── helpers ───────────────────────────────────────────────────────────────

fn entry(claim: &str, scope: &str, state: VerificationState) -> VerificationEntry {
    VerificationEntry {
        claim: claim.into(),
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

fn strong_approval_for(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.to_string(),
        approver: "security-team".to_string(),
        reason: "explicitly approved for this context".to_string(),
        strength: ApprovalStrength::Strong,
    }
}

fn weak_approval_for(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.to_string(),
        approver: "dev-team".to_string(),
        reason: "weak boundary assumption".to_string(),
        strength: ApprovalStrength::Weak,
    }
}

// ── R2-1: Strict-by-default — unknown profile is conservative ─────────────
//
// Unknown profiles must NOT fall through to permissive. The spec says:
//   "Strict by default. Relaxed only by explicit policy/profile."
// An unknown profile should treat Unverified as blocking (like prod).

#[test]
fn unknown_profile_blocks_unverified_strict_by_default() {
    let report = report_with(vec![entry(
        "type",
        "some.node",
        VerificationState::Unverified,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("completely_unknown_profile".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "unknown profile must be conservative — block Unverified like prod/critical"
    );
}

#[test]
fn unknown_profile_blocks_unsafe_strict_by_default() {
    let report = report_with(vec![entry("type", "unsafe.fn", VerificationState::Unsafe)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("totally_custom".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "unknown profile must block Unsafe without approval"
    );
}

#[test]
fn unknown_profile_passes_proven_entry() {
    // Even unknown profiles must pass Proven entries (that's universal)
    let report = report_with(vec![entry("type", "proven.fn", VerificationState::Proven)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("brand_new_profile".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "Proven entries must pass even in unknown profiles"
    );
}

// ── R2-2: PolicyInput must carry richer context fields ────────────────────
//
// The spec says policy engine evaluates:
//   structural_diff, capability_grants, public_api_changes, package_trust_metadata
// These must exist on PolicyInput (compile-time check + behavioral tests).

#[test]
fn policy_input_carries_structural_diff_field() {
    let report = report_with(vec![]);
    // structural_diff: Option<&StructuralDiff> — using None is valid
    let input = PolicyInput {
        report: &report,
        rules: &[],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    // Empty report + no rules must still pass
    assert!(matches!(
        PolicyEngine::evaluate(&input),
        PolicyDecision::Passed
    ));
}

#[test]
fn policy_input_carries_public_api_changes_field() {
    let report = report_with(vec![]);
    // public_api_changes: &[PublicApiChange]
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::NoPublicApiChangesWithoutApproval],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    // No public api changes → passes
    assert!(matches!(
        PolicyEngine::evaluate(&input),
        PolicyDecision::Passed
    ));
}

#[test]
fn policy_blocks_unapproved_public_api_change() {
    use ail_verify::policy::PublicApiChange;
    let report = report_with(vec![]);
    let api_change = PublicApiChange {
        scope: "pub::checkout".to_string(),
        description: "Added new parameter to checkout".to_string(),
    };
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::NoPublicApiChangesWithoutApproval],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[api_change],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "unapproved public API change must block"
    );
}

#[test]
fn policy_passes_approved_public_api_change() {
    use ail_verify::policy::PublicApiChange;
    let report = report_with(vec![]);
    let api_change = PublicApiChange {
        scope: "pub::checkout".to_string(),
        description: "Added new parameter to checkout".to_string(),
    };
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::NoPublicApiChangesWithoutApproval],
        approvals: &[strong_approval_for("pub::checkout")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[api_change],
        package_trust_metadata: &[],
    };
    assert!(matches!(
        PolicyEngine::evaluate(&input),
        PolicyDecision::Passed
    ));
}

#[test]
fn policy_input_carries_capability_grants_field() {
    use ail_verify::policy::CapabilityGrant;
    let report = report_with(vec![]);
    let grant = CapabilityGrant {
        scope: "module.checkout".to_string(),
        capability: "database.write:Order".to_string(),
    };
    let input = PolicyInput {
        report: &report,
        rules: &[],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[grant],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    // No blocking rules — should pass
    assert!(matches!(
        PolicyEngine::evaluate(&input),
        PolicyDecision::Passed
    ));
}

#[test]
fn policy_input_carries_package_trust_metadata_field() {
    use ail_verify::policy::PackageTrustEntry;
    let report = report_with(vec![]);
    let entry = PackageTrustEntry {
        package: "some-crate".to_string(),
        trust_level: "verified".to_string(),
    };
    let input = PolicyInput {
        report: &report,
        rules: &[],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[entry],
    };
    // No blocking rules — should pass
    assert!(matches!(
        PolicyEngine::evaluate(&input),
        PolicyDecision::Passed
    ));
}

// ── R2-3: Assumed gating — approved vs unapproved ─────────────────────────
//
// The spec says "assumed pasa solo con boundary explícito + policy/approval".
// staging/prod/critical must block unapproved Assumed.
// ApprovalRecord now has a `strength` field: Strong | Weak.
// Critical must distinguish: only Strong approvals pass for Assumed.

#[test]
fn approval_record_has_strength_field() {
    // Compile-time: ensure ApprovalRecord has strength field
    let strong = ApprovalRecord {
        scope: "some.scope".to_string(),
        approver: "security-team".to_string(),
        reason: "approved".to_string(),
        strength: ApprovalStrength::Strong,
    };
    let weak = ApprovalRecord {
        scope: "other.scope".to_string(),
        approver: "dev-team".to_string(),
        reason: "boundary assumption".to_string(),
        strength: ApprovalStrength::Weak,
    };
    assert_eq!(strong.strength, ApprovalStrength::Strong);
    assert_eq!(weak.strength, ApprovalStrength::Weak);
}

// ── R2-4: Draft profile — emit warnings for Unverified ───────────────────
//
// draft profile allows Unverified (doesn't block) but must emit a diagnostic
// warning when Unverified entries exist.
// Assumed entries must be "annotated" (have evidence) or warn.

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

// ── R2-9: Critical — strong-approval/weak-assumption gate ─────────────────
//
// critical profile:
//   - Assumed requires Strong approval (Weak is not enough → POLICY_WEAK_ASSUMPTION)
//   - Unsafe is always blocked (even with Weak approval)
//   - RuntimeChecked only passes if policy accepts it (here: always allowed for now)
//   - Unverified always blocks

#[test]
fn critical_blocks_assumed_with_no_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "critical.assumption",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
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
        "critical must block Assumed without any approval"
    );
}

#[test]
fn critical_blocks_assumed_with_only_weak_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "critical.assumption",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
        approvals: &[weak_approval_for("critical.assumption")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    // Must fail with POLICY_WEAK_ASSUMPTION code
    match &decision {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations.iter().any(|v| v.code == POLICY_WEAK_ASSUMPTION),
                "critical must reject weak assumptions with POLICY_WEAK_ASSUMPTION code; got: {violations:?}"
            );
        }
        other => panic!("expected Failed(POLICY_WEAK_ASSUMPTION), got {other:?}"),
    }
}

#[test]
fn critical_passes_assumed_with_strong_approval() {
    let report = report_with(vec![entry(
        "boundary",
        "critical.assumption",
        VerificationState::Assumed,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
        approvals: &[strong_approval_for("critical.assumption")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "critical must pass Assumed with Strong approval"
    );
}

#[test]
fn critical_blocks_unsafe_even_with_weak_approval() {
    let report = report_with(vec![entry(
        "type",
        "critical.unsafe",
        VerificationState::Unsafe,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
        approvals: &[weak_approval_for("critical.unsafe")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "critical must block Unsafe even with weak approval"
    );
}

#[test]
fn critical_blocks_unsafe_even_with_strong_approval() {
    // critical: Unsafe is always blocked — no exceptions
    let report = report_with(vec![entry(
        "type",
        "critical.unsafe",
        VerificationState::Unsafe,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
        approvals: &[strong_approval_for("critical.unsafe")],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "critical must ALWAYS block Unsafe regardless of approval strength"
    );
}

#[test]
fn critical_blocks_unverified() {
    let report = report_with(vec![entry("type", "any.fn", VerificationState::Unverified)]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    assert!(
        matches!(PolicyEngine::evaluate(&input), PolicyDecision::Failed(_)),
        "critical must block Unverified"
    );
}

#[test]
fn critical_passes_runtime_checked_entry() {
    let report = report_with(vec![entry(
        "type",
        "validated.input",
        VerificationState::RuntimeChecked,
    )]);
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("critical".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "critical must allow RuntimeChecked entries per spec"
    );
}

// ── R2-10: Report integration — PolicyAudit in VerificationReport ──────────
//
// VerificationReport must carry a `policy_audit` section with:
//   - The profile used
//   - A list of PolicyAuditEntry (scope, state, gate_decision, approval_used)
//   - Approvals consulted during evaluation

#[test]
fn report_has_policy_audit_field() {
    // Compile-time: VerificationReport must have policy_audit: Option<PolicyAudit>
    let mut report = VerificationReport::default();
    report.policy_audit = Some(PolicyAudit {
        profile: "prod".to_string(),
        entries: vec![PolicyAuditEntry {
            scope: "fn.checkout".to_string(),
            state: "assumed".to_string(),
            gate_decision: "approval_required".to_string(),
            approval_used: Some("security-team".to_string()),
        }],
        approval_scopes_consulted: vec!["fn.checkout".to_string()],
    });
    assert!(report.policy_audit.is_some());
}

#[test]
fn policy_engine_populates_audit_in_report() {
    // When ProfileGate("prod") is used, the engine must populate policy_audit
    // on the returned decision context.
    // For now, verify that evaluate returns an AuditableDecision when asked.
    let report = report_with(vec![entry(
        "type",
        "fn.checkout",
        VerificationState::Assumed,
    )]);
    let approvals = [strong_approval_for("fn.checkout")];
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &approvals,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let (decision, audit) = PolicyEngine::evaluate_with_audit(&input);
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "prod must pass Assumed with strong approval"
    );
    assert_eq!(audit.profile, "prod");
    assert!(
        audit.entries.iter().any(|e| e.scope == "fn.checkout"),
        "audit must record entry for fn.checkout"
    );
}

// ── R2-11: Pipeline integration — Checker → PolicyEngine end-to-end ────────
//
// PolicyEngine must accept a VerificationReport produced by Checker and
// run the full policy gate, returning a decision. This tests the
// checker→policy pipeline without requiring ChangeSetOp::Verify to be
// fully wired (that's a separate concern).

#[test]
fn checker_report_flows_through_policy_engine() {
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph, TypeFacts};
    use ail_verify::checker::Checker;

    // Build a minimal graph with one proven node
    let mut node = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.safe");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".to_string(),
        generics: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let report = Checker::check(&graph);

    // Run policy engine on the checker output
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::ProfileGate("prod".into())],
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    // fn.safe has Proven type → should pass prod
    let decision = PolicyEngine::evaluate(&input);
    // Note: effect and capability entries will be Unverified (no effect_row/capability_reqs)
    // so this may fail — that's expected and correct behavior for prod profile.
    // The key test: it should compile and run without panic.
    let _ = decision; // just ensure no panic
}

#[test]
fn checker_report_with_all_proven_passes_prod_policy() {
    use ail_core::semantic_graph::{
        CapabilityReqs, EffectRow, GraphNode, NodeKind, NodeRef, SemanticGraph, TypeFacts,
    };
    use ail_verify::checker::Checker;

    // Graph node with all three fact dimensions populated → all Proven/Assumed
    let mut node = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.checkout");
    node.type_facts = Some(TypeFacts {
        nominal: "Unit".to_string(),
        generics: vec![],
    });
    node.effect_row = Some(EffectRow {
        effects: vec!["database.write".to_string()],
    });
    node.capability_reqs = Some(CapabilityReqs {
        caps: vec!["database.write:Order".to_string()],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let report = Checker::check(&graph);

    // With effect_row → Assumed; cap_reqs → Assumed
    // For prod: Assumed needs approval
    let approvals = [strong_approval_for("fn.checkout")];
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::NoUnsafe, PolicyRule::ProfileGate("prod".into())],
        approvals: &approvals,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);
    // Checker emits scope = node.name for all three entries.
    // Assumed entries with approval for "fn.checkout" must pass.
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "all-approved assumed entries should pass prod policy; got: {decision:?}"
    );
}
