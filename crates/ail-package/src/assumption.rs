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
}
