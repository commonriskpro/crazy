// ── ail-verify::policy tests ──────────────────────────────────────────────
//
// Strict TDD — GREEN phase (Round 1 tests updated for Round 2 struct changes).
//
// Policy engine model:
//   PolicyEngine::evaluate(&PolicyInput) -> PolicyDecision
//   PolicyDecision: Passed | PassedWithWarnings | Failed | ApprovalRequired
//   PolicyRule: NoUnsafe | NoUnverifiedPublicApi | RequireApproval | ProfileGate(String)

use ail_verify::policy::{
    ApprovalRecord, ApprovalStrength, POLICY_PROFILE_GATE, POLICY_UNSAFE_BLOCKED,
    POLICY_UNVERIFIED_PUBLIC_API, PolicyDecision, PolicyEngine, PolicyInput, PolicyRule,
};
use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

// ── helpers ───────────────────────────────────────────────────────────────

fn entry(claim: &str, scope: &str, state: VerificationState) -> VerificationEntry {
    VerificationEntry {
        claim: claim.into(),
        state,
        scope: scope.into(),
        evidence: None,
        blocking: false,
    }
}

fn report_with(entries: Vec<VerificationEntry>) -> VerificationReport {
    VerificationReport {
        entries,
        ..Default::default()
    }
}

fn approval_for(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.to_string(),
        approver: "security-team".to_string(),
        reason: "explicitly approved for this context".to_string(),
        strength: ApprovalStrength::Strong,
    }
}

/// Shorthand for building a minimal PolicyInput with no context fields set.
macro_rules! policy_input {
    (report = $r:expr, rules = $rules:expr, approvals = $approvals:expr) => {
        PolicyInput {
            report: $r,
            rules: $rules,
            approvals: $approvals,
            structural_diff: None,
            capability_grants: &[],
            public_api_changes: &[],
            package_trust_metadata: &[],
        }
    };
}

// ── R8: Empty report passes all rules ────────────────────────────────────

#[test]
fn empty_report_passes_all_rules() {
    let report = report_with(vec![]);
    let input = policy_input!(
        report = &report,
        rules = &[
            PolicyRule::NoUnsafe,
            PolicyRule::NoUnverifiedPublicApi,
            PolicyRule::RequireApproval,
            PolicyRule::ProfileGate("prod".into()),
        ],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "empty report must pass all rules"
    );
}

// ── R1: NoUnsafe blocks Unsafe entry without approval ─────────────────────

#[test]
fn no_unsafe_blocks_unsafe_entry_without_approval() {
    let report = report_with(vec![entry(
        "type",
        "fn.transfer",
        VerificationState::Unsafe,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnsafe],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    match decision {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations.iter().any(|v| v.code == POLICY_UNSAFE_BLOCKED),
                "must have POLICY_UNSAFE_BLOCKED violation"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// ── R2: NoUnsafe passes Unsafe entry WITH approval ────────────────────────

#[test]
fn no_unsafe_passes_unsafe_entry_with_approval() {
    let report = report_with(vec![entry(
        "type",
        "fn.transfer",
        VerificationState::Unsafe,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnsafe],
        approvals = &[approval_for("fn.transfer")]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "Unsafe entry with approval must pass NoUnsafe rule"
    );
}

// ── R3: NoUnverifiedPublicApi blocks Unverified pub:: scope ───────────────

#[test]
fn no_unverified_public_api_blocks_public_unverified() {
    let report = report_with(vec![entry(
        "type",
        "pub::checkout",
        VerificationState::Unverified,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnverifiedPublicApi],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    match decision {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations
                    .iter()
                    .any(|v| v.code == POLICY_UNVERIFIED_PUBLIC_API),
                "must have POLICY_UNVERIFIED_PUBLIC_API violation"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// ── R3 complement: NoUnverifiedPublicApi allows Unverified private scope ──

#[test]
fn no_unverified_public_api_allows_private_unverified() {
    let report = report_with(vec![entry(
        "type",
        "internal::checkout",
        VerificationState::Unverified,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnverifiedPublicApi],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "Unverified in private scope must pass NoUnverifiedPublicApi"
    );
}

// ── R4: RequireApproval returns ApprovalRequired for Unsafe without approval

#[test]
fn require_approval_returns_approval_required_for_unsafe() {
    let report = report_with(vec![entry(
        "type",
        "ffi.raw_ptr",
        VerificationState::Unsafe,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::RequireApproval],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    match decision {
        PolicyDecision::ApprovalRequired(scopes) => {
            assert!(
                scopes.contains(&"ffi.raw_ptr".to_string()),
                "must list ffi.raw_ptr as needing approval"
            );
        }
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
}

// ── R4 complement: RequireApproval passes Unsafe WITH approval ────────────

#[test]
fn require_approval_passes_unsafe_with_approval() {
    let report = report_with(vec![entry(
        "type",
        "ffi.raw_ptr",
        VerificationState::Unsafe,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::RequireApproval],
        approvals = &[approval_for("ffi.raw_ptr")]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "Unsafe with approval must pass RequireApproval"
    );
}

// ── R5: ProfileGate — prod blocks Unverified ──────────────────────────────

#[test]
fn profile_gate_prod_blocks_unverified() {
    let report = report_with(vec![entry(
        "type",
        "some.node",
        VerificationState::Unverified,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::ProfileGate("prod".into())],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    match decision {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations.iter().any(|v| v.code == POLICY_PROFILE_GATE),
                "must have POLICY_PROFILE_GATE violation"
            );
        }
        other => panic!("expected Failed for prod+Unverified, got {other:?}"),
    }
}

// ── R6: ProfileGate — draft allows Unverified (with warning) ─────────────
//
// NOTE: In Round 2, draft with Unverified returns PassedWithWarnings, not
// bare Passed. "Allows" means does NOT hard-block (Failed).

#[test]
fn profile_gate_draft_allows_unverified() {
    let report = report_with(vec![entry(
        "type",
        "some.node",
        VerificationState::Unverified,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::ProfileGate("draft".into())],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    // draft must NOT hard-block Unverified (may warn, may pass — both acceptable)
    assert!(
        !matches!(decision, PolicyDecision::Failed(_)),
        "draft profile must not hard-block Unverified entries"
    );
}

// ── R7: ProfileGate — Failed always blocks in any profile ────────────────

#[test]
fn profile_gate_always_blocks_failed_entries() {
    for profile in &["draft", "dev", "test", "staging", "prod", "critical"] {
        let report = report_with(vec![entry("type", "broken.fn", VerificationState::Failed)]);
        let input = policy_input!(
            report = &report,
            rules = &[PolicyRule::ProfileGate(profile.to_string())],
            approvals = &[]
        );
        let decision = PolicyEngine::evaluate(&input);
        assert!(
            matches!(decision, PolicyDecision::Failed(_)),
            "Failed entry must block in profile {profile}"
        );
    }
}

// ── R7b: ProfileGate — Unsafe blocks in prod/staging/critical (no approval)

#[test]
fn profile_gate_blocks_unsafe_in_strict_profiles() {
    for profile in &["prod", "staging", "critical"] {
        let report = report_with(vec![entry("type", "unsafe.fn", VerificationState::Unsafe)]);
        let input = policy_input!(
            report = &report,
            rules = &[PolicyRule::ProfileGate(profile.to_string())],
            approvals = &[]
        );
        let decision = PolicyEngine::evaluate(&input);
        assert!(
            matches!(decision, PolicyDecision::Failed(_)),
            "Unsafe must block in strict profile {profile}"
        );
    }
}

// ── R6b: ProfileGate — dev blocks Unverified in public scope ─────────────

#[test]
fn profile_gate_dev_blocks_unverified_public() {
    let report = report_with(vec![entry(
        "type",
        "pub::my_fn",
        VerificationState::Unverified,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::ProfileGate("dev".into())],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "dev profile must block Unverified in public scope"
    );
}

// ── R6c: ProfileGate — dev allows Unverified in private scope ────────────

#[test]
fn profile_gate_dev_allows_unverified_private() {
    let report = report_with(vec![entry(
        "type",
        "internal::my_fn",
        VerificationState::Unverified,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::ProfileGate("dev".into())],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Passed),
        "dev profile must allow Unverified in private scope"
    );
}

// ── R9: Multiple rules — most severe wins ────────────────────────────────

#[test]
fn multiple_rules_collect_all_violations() {
    let report = report_with(vec![
        entry("type", "fn.transfer", VerificationState::Unsafe),
        entry("type", "pub::api_fn", VerificationState::Unverified),
    ]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnsafe, PolicyRule::NoUnverifiedPublicApi],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    match decision {
        PolicyDecision::Failed(violations) => {
            assert!(
                violations.iter().any(|v| v.code == POLICY_UNSAFE_BLOCKED),
                "must have POLICY_UNSAFE_BLOCKED"
            );
            assert!(
                violations
                    .iter()
                    .any(|v| v.code == POLICY_UNVERIFIED_PUBLIC_API),
                "must have POLICY_UNVERIFIED_PUBLIC_API"
            );
        }
        other => panic!("expected Failed with multiple violations, got {other:?}"),
    }
}

// ── R9b: Failed beats ApprovalRequired ───────────────────────────────────

#[test]
fn failed_beats_approval_required_in_priority() {
    // NoUnsafe → Failed (no approval), RequireApproval → ApprovalRequired
    // Final decision should be Failed (most severe)
    let report = report_with(vec![entry("type", "fn.danger", VerificationState::Unsafe)]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnsafe, PolicyRule::RequireApproval],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(
        matches!(decision, PolicyDecision::Failed(_)),
        "Failed must beat ApprovalRequired in priority"
    );
}

// ── R10: VerificationReport policy_decision field round-trips via serde ───

#[test]
fn verification_report_policy_decision_field_serializes() {
    use ail_verify::policy::PolicyViolation;

    let violation = PolicyViolation {
        code: POLICY_UNSAFE_BLOCKED.to_string(),
        scope: "fn.transfer".to_string(),
        message: "unsafe not approved".to_string(),
    };
    let mut report = VerificationReport::default();
    report.policy_decision = Some(PolicyDecision::Failed(vec![violation]));

    // Round-trip through serde_json (test the shape is serializable)
    let json = serde_json::to_string(&report).expect("must serialize");
    let deserialized: VerificationReport = serde_json::from_str(&json).expect("must deserialize");

    match deserialized.policy_decision {
        Some(PolicyDecision::Failed(vs)) => {
            assert_eq!(vs.len(), 1);
            assert_eq!(vs[0].code, POLICY_UNSAFE_BLOCKED);
            assert_eq!(vs[0].scope, "fn.transfer");
        }
        other => panic!("expected Failed after round-trip, got {other:?}"),
    }
}

// ── Triangulation: NoUnsafe passes entry with Proven state ───────────────

#[test]
fn no_unsafe_passes_proven_entry() {
    let report = report_with(vec![entry("type", "fn.safe", VerificationState::Proven)]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnsafe],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    assert!(matches!(decision, PolicyDecision::Passed));
}

// ── Triangulation: violation scope field matches triggering entry scope ────

#[test]
fn violation_scope_matches_triggering_entry_scope() {
    let report = report_with(vec![entry(
        "type",
        "pub::my_api",
        VerificationState::Unsafe,
    )]);
    let input = policy_input!(
        report = &report,
        rules = &[PolicyRule::NoUnsafe],
        approvals = &[]
    );
    let decision = PolicyEngine::evaluate(&input);
    match decision {
        PolicyDecision::Failed(violations) => {
            assert_eq!(violations[0].scope, "pub::my_api");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
