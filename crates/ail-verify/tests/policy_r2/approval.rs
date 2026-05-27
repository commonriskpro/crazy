use super::helpers::*;

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
