// ── ail-package::assumption ───────────────────────────────────────────────
//
// `PackageAssumption` and `AssumptionState` — model assumption lifecycle for
// packages with `TrustLevel::Assumed`.
//
// # Determinism contract
//
// All collection fields use `Vec` or `BTreeMap`, never `HashMap`.

use serde::{Deserialize, Serialize};

// ── AssumptionState ───────────────────────────────────────────────────────

/// Lifecycle state of a `PackageAssumption`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssumptionState {
    /// Submitted but not yet reviewed.
    Proposed,
    /// Approved for use but not yet activated in production.
    Approved,
    /// Currently active and in production use.
    Active,
    /// Past its expiry date; no longer valid.
    Expired,
    /// Explicitly revoked before expiry.
    Revoked,
    /// Review process determined the assumption was incorrect.
    FailedReview,
}

impl AssumptionState {
    /// Return `true` only for the `Active` state.
    ///
    /// All other states — including `Approved` — are not considered active
    /// because they either await activation, or have ended.
    pub fn is_active(self) -> bool {
        matches!(self, AssumptionState::Active)
    }
}

// ── PackageAssumption ─────────────────────────────────────────────────────

/// A documented assumption attached to a package with `TrustLevel::Assumed`.
///
/// An assumption records a claim that cannot yet be fully verified, along
/// with ownership and expiry metadata so the lifecycle can be tracked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageAssumption {
    /// Stable identifier for this assumption (e.g., `"assume-stripe-pci-scope"`).
    pub id: String,
    /// Human-readable claim being assumed (e.g., "Vendor is PCI-DSS certified").
    pub claim: String,
    /// Trust boundary this assumption operates within.
    pub boundary: String,
    /// Team or individual responsible for monitoring this assumption.
    pub owner: String,
    /// Optional expiry date expressed as an ISO-8601 date string (e.g., `"2026-12-31"`).
    pub expires: Option<String>,
    /// Current lifecycle state.
    pub state: AssumptionState,
}

impl PackageAssumption {
    /// Return `true` if this assumption is currently `Active`.
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }
}

// ── ApprovalRecord ────────────────────────────────────────────────────────

/// An explicit acceptance record for a `PackageAssumption` by an importing project.
///
/// Consumer projects must explicitly accept or reject package assumptions.
/// This record captures that acceptance, who approved it, and in which project.
///
/// # Example (from docs/packages.md)
/// ```text
/// approve_assumption stripe_idempotency for project.checkout by=security
/// ```
///
/// See `docs/packages.md` §Assumptions and boundaries in packages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    /// ID of the assumption being accepted (matches `PackageAssumption::id`).
    pub assumption_id: String,
    /// Name of the importing project that accepts this assumption.
    pub project: String,
    /// Identity of the approver (e.g., a team, reviewer ID, or role).
    pub approved_by: String,
}

// ── AssumptionEnforcer ────────────────────────────────────────────────────

/// Enforces that all `Assumed` packages have their assumptions explicitly
/// accepted by the consuming project.
pub struct AssumptionEnforcer;

/// Error returned by [`AssumptionEnforcer::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssumptionEnforcementError {
    /// An assumption from a package was not accepted by the consuming project.
    UnacceptedAssumption {
        /// The assumption ID that was not accepted.
        assumption_id: String,
        /// The package that shipped the assumption.
        package: String,
    },
}

impl std::fmt::Display for AssumptionEnforcementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssumptionEnforcementError::UnacceptedAssumption {
                assumption_id,
                package,
            } => write!(
                f,
                "assumption '{assumption_id}' from package '{package}' \
                 has not been explicitly accepted by the consuming project"
            ),
        }
    }
}

impl std::error::Error for AssumptionEnforcementError {}

impl AssumptionEnforcer {
    /// Check that every assumption in `assumptions` has been accepted by
    /// `project` via an `ApprovalRecord` in `approvals`.
    ///
    /// # Errors
    ///
    /// Returns one `AssumptionEnforcementError` per unaccepted assumption.
    pub fn check(
        package: &str,
        assumptions: &[PackageAssumption],
        project: &str,
        approvals: &[ApprovalRecord],
    ) -> Vec<AssumptionEnforcementError> {
        assumptions
            .iter()
            .filter(|a| a.is_active()) // only Active assumptions require approval
            .filter(|a| {
                !approvals
                    .iter()
                    .any(|r| r.assumption_id == a.id && r.project == project)
            })
            .map(|a| AssumptionEnforcementError::UnacceptedAssumption {
                assumption_id: a.id.clone(),
                package: package.to_string(),
            })
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_assumption(state: AssumptionState) -> PackageAssumption {
        PackageAssumption {
            id: "assume-test".to_string(),
            claim: "Vendor is audited".to_string(),
            boundary: "payments".to_string(),
            owner: "platform-team".to_string(),
            expires: None,
            state,
        }
    }

    // ── expired_assumption_is_not_active ──────────────────────────────────
    // Spec scenario: "Expired assumption is not Active"
    //   GIVEN a PackageAssumption with state: Expired
    //   WHEN is_active() is called
    //   THEN returns false
    #[test]
    fn expired_assumption_is_not_active() {
        let a = make_assumption(AssumptionState::Expired);
        assert!(!a.is_active());
    }

    // ── active_assumption_is_active ───────────────────────────────────────
    #[test]
    fn active_assumption_is_active() {
        let a = make_assumption(AssumptionState::Active);
        assert!(a.is_active());
    }

    // ── TRIANGULATE: non_active_states_return_false ───────────────────────
    #[test]
    fn non_active_states_return_false() {
        for state in [
            AssumptionState::Proposed,
            AssumptionState::Approved,
            AssumptionState::Expired,
            AssumptionState::Revoked,
            AssumptionState::FailedReview,
        ] {
            assert!(
                !make_assumption(state).is_active(),
                "state {state:?} should not be active"
            );
        }
    }

    // ── approval_record_cbor_round_trip ───────────────────────────────────
    // Spec scenario: "ApprovalRecord round-trips through CBOR"
    //   GIVEN an ApprovalRecord with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn approval_record_cbor_round_trip() {
        let record = ApprovalRecord {
            assumption_id: "stripe_idempotency".to_string(),
            project: "project.checkout".to_string(),
            approved_by: "security".to_string(),
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&record, &mut buf).expect("encode");
        let decoded: ApprovalRecord = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, record);
    }

    // ── enforcer_passes_when_all_assumptions_accepted ─────────────────────
    // Spec scenario: "All assumptions accepted — enforcer returns no errors"
    //   GIVEN a package with one assumption and an ApprovalRecord for it
    //   WHEN AssumptionEnforcer::check is called
    //   THEN returns empty Vec
    #[test]
    fn enforcer_passes_when_all_assumptions_accepted() {
        let assumptions = vec![make_assumption(AssumptionState::Active)];
        let approvals = vec![ApprovalRecord {
            assumption_id: "assume-test".to_string(),
            project: "project.checkout".to_string(),
            approved_by: "security".to_string(),
        }];
        let errors = AssumptionEnforcer::check(
            "payments.stripe",
            &assumptions,
            "project.checkout",
            &approvals,
        );
        assert!(errors.is_empty(), "all accepted — no errors expected");
    }

    // ── enforcer_fails_for_unaccepted_assumption ──────────────────────────
    // Spec scenario: "Unaccepted assumption — enforcer returns error"
    //   GIVEN a package with an assumption and NO matching ApprovalRecord
    //   WHEN AssumptionEnforcer::check is called
    //   THEN returns one UnacceptedAssumption error
    #[test]
    fn enforcer_fails_for_unaccepted_assumption() {
        let assumptions = vec![make_assumption(AssumptionState::Active)];
        let errors = AssumptionEnforcer::check(
            "payments.stripe",
            &assumptions,
            "project.checkout",
            &[], // no approvals
        );
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(
                &errors[0],
                AssumptionEnforcementError::UnacceptedAssumption {
                    assumption_id,
                    package,
                } if assumption_id == "assume-test" && package == "payments.stripe"
            ),
            "expected UnacceptedAssumption for assume-test"
        );
    }

    // ── enforcer_approval_for_different_project_does_not_count ───────────
    // TRIANGULATE: approval for a different project does not satisfy the check
    #[test]
    fn enforcer_approval_for_different_project_does_not_count() {
        let assumptions = vec![make_assumption(AssumptionState::Active)];
        let approvals = vec![ApprovalRecord {
            assumption_id: "assume-test".to_string(),
            project: "project.other".to_string(), // different project
            approved_by: "security".to_string(),
        }];
        let errors = AssumptionEnforcer::check(
            "payments.stripe",
            &assumptions,
            "project.checkout",
            &approvals,
        );
        assert_eq!(
            errors.len(),
            1,
            "approval for different project must not count"
        );
    }

    // ── enforcer_no_assumptions_no_errors ─────────────────────────────────
    // TRIANGULATE: package with no assumptions never errors
    #[test]
    fn enforcer_no_assumptions_no_errors() {
        let errors = AssumptionEnforcer::check("payments.stripe", &[], "project.checkout", &[]);
        assert!(errors.is_empty());
    }

    // ── B1: Active-state filter ────────────────────────────────────────────
    // Spec PKG-ASSUME-1: Proposed assumption + no approval → no error
    #[test]
    fn proposed_assumption_without_approval_produces_no_error() {
        let assumptions = vec![make_assumption(AssumptionState::Proposed)];
        let errors = AssumptionEnforcer::check(
            "payments.stripe",
            &assumptions,
            "project.checkout",
            &[], // no approvals
        );
        assert!(
            errors.is_empty(),
            "Proposed assumption must not require approval"
        );
    }

    // Spec PKG-ASSUME-1: Expired assumption + no approval → no error
    #[test]
    fn expired_assumption_without_approval_produces_no_error() {
        let assumptions = vec![make_assumption(AssumptionState::Expired)];
        let errors =
            AssumptionEnforcer::check("payments.stripe", &assumptions, "project.checkout", &[]);
        assert!(
            errors.is_empty(),
            "Expired assumption must not require approval"
        );
    }

    // TRIANGULATE: Revoked assumption + no approval → no error
    #[test]
    fn revoked_assumption_without_approval_produces_no_error() {
        let assumptions = vec![make_assumption(AssumptionState::Revoked)];
        let errors =
            AssumptionEnforcer::check("payments.stripe", &assumptions, "project.checkout", &[]);
        assert!(
            errors.is_empty(),
            "Revoked assumption must not require approval"
        );
    }

    // Spec PKG-ASSUME-1: Active assumption + no approval → error
    // (confirms Active still enforced after filter)
    #[test]
    fn active_assumption_without_approval_still_produces_error() {
        let assumptions = vec![make_assumption(AssumptionState::Active)];
        let errors =
            AssumptionEnforcer::check("payments.stripe", &assumptions, "project.checkout", &[]);
        assert_eq!(
            errors.len(),
            1,
            "Active assumption without approval must still be an error"
        );
    }

    // TRIANGULATE: Mixed active + proposed — only active produces error
    #[test]
    fn mixed_assumptions_only_active_produces_error() {
        let mut active = make_assumption(AssumptionState::Active);
        active.id = "assume-active".to_string();
        let mut proposed = make_assumption(AssumptionState::Proposed);
        proposed.id = "assume-proposed".to_string();

        let assumptions = vec![active, proposed];
        let errors =
            AssumptionEnforcer::check("payments.stripe", &assumptions, "project.checkout", &[]);
        assert_eq!(
            errors.len(),
            1,
            "only the Active assumption must produce an error"
        );
        assert!(
            matches!(&errors[0], AssumptionEnforcementError::UnacceptedAssumption {
                assumption_id, ..
            } if assumption_id == "assume-active")
        );
    }
}
