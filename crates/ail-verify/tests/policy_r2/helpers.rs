pub(super) use ail_verify::policy::{
    ApprovalRecord, ApprovalStrength, POLICY_PROFILE_GATE, POLICY_UNSAFE_BLOCKED,
    POLICY_WEAK_ASSUMPTION, PolicyDecision, PolicyEngine, PolicyInput, PolicyRule,
};
pub(super) use ail_verify::policy::{PolicyAudit, PolicyAuditEntry};
pub(super) use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

// ── helpers ───────────────────────────────────────────────────────────────

pub(super) fn entry(claim: &str, scope: &str, state: VerificationState) -> VerificationEntry {
    VerificationEntry {
        claim: claim.into(),
        state,
        scope: scope.into(),
        evidence: None,
        blocking: false,
        repair_options: vec![],
    }
}

pub(super) fn report_with(entries: Vec<VerificationEntry>) -> VerificationReport {
    VerificationReport {
        entries,
        ..Default::default()
    }
}

pub(super) fn strong_approval_for(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.to_string(),
        approver: "security-team".to_string(),
        reason: "explicitly approved for this context".to_string(),
        strength: ApprovalStrength::Strong,
    }
}

pub(super) fn weak_approval_for(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.to_string(),
        approver: "dev-team".to_string(),
        reason: "weak boundary assumption".to_string(),
        strength: ApprovalStrength::Weak,
    }
}
