// ── ail-package::trust ────────────────────────────────────────────────────
//
// `TrustLevel` — ordered enumeration of package trust tiers.
//
// # Ordering contract
//
// The derived `Ord` places variants in declaration order (lowest discriminant
// first).  We want `Unsafe < Unverified < Assumed < Verified`, so the enum
// variants are declared in ascending order.  This means `Verified` is the
// maximum value and satisfies any minimum-trust gate.
//
// # Determinism contract
//
// `TrustLevel` is serialized as a CBOR integer (discriminant) by `ciborium`.
// Never reorder variants; that would silently break all stored CBOR data.

use serde::{Deserialize, Serialize};

// ── TrustLevel ────────────────────────────────────────────────────────────

/// The trust tier assigned to a package.
///
/// Variants are ordered from least trusted to most trusted:
/// `Unsafe` < `Unverified` < `Assumed` < `Verified`.
///
/// Use [`TrustLevel::satisfies`] to check whether a package meets a minimum
/// required tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    /// No trust assertions.  Requires explicit unsafe-surface declaration
    /// and an explicit approval record to pass any trust gate.
    Unsafe = 0,
    /// Trust is not established; the package has not been reviewed.
    Unverified = 1,
    /// Trust is assumed based on boundary contracts; review is ongoing.
    Assumed = 2,
    /// Full trust: the package has passed a review and is reproducibly built.
    Verified = 3,
}

impl TrustLevel {
    /// Return `true` if `self` meets or exceeds `minimum`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use ail_package::trust::TrustLevel;
    ///
    /// assert!(TrustLevel::Verified.satisfies(TrustLevel::Assumed));
    /// assert!(!TrustLevel::Unverified.satisfies(TrustLevel::Assumed));
    /// ```
    pub fn satisfies(self, minimum: TrustLevel) -> bool {
        self >= minimum
    }
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TrustLevel::Unsafe => "unsafe",
            TrustLevel::Unverified => "unverified",
            TrustLevel::Assumed => "assumed",
            TrustLevel::Verified => "verified",
        };
        f.write_str(s)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── satisfies_verified_meets_any_minimum ──────────────────────────────
    // Spec scenario: "Ordering is stable"
    //   GIVEN TrustLevel::Verified
    //   WHEN satisfies(Assumed) is called
    //   THEN returns true
    #[test]
    fn satisfies_verified_meets_any_minimum() {
        assert!(TrustLevel::Verified.satisfies(TrustLevel::Unsafe));
        assert!(TrustLevel::Verified.satisfies(TrustLevel::Unverified));
        assert!(TrustLevel::Verified.satisfies(TrustLevel::Assumed));
        assert!(TrustLevel::Verified.satisfies(TrustLevel::Verified));
    }

    // ── satisfies_unverified_does_not_meet_assumed ────────────────────────
    // Spec scenario: "Ordering is stable"
    //   GIVEN TrustLevel::Unverified
    //   WHEN satisfies(Assumed) is called
    //   THEN returns false
    #[test]
    fn satisfies_unverified_does_not_meet_assumed() {
        assert!(!TrustLevel::Unverified.satisfies(TrustLevel::Assumed));
        assert!(!TrustLevel::Unverified.satisfies(TrustLevel::Verified));
    }

    // ── satisfies_assumed_meets_itself_and_below ──────────────────────────
    #[test]
    fn satisfies_assumed_meets_itself_and_below() {
        assert!(TrustLevel::Assumed.satisfies(TrustLevel::Unsafe));
        assert!(TrustLevel::Assumed.satisfies(TrustLevel::Unverified));
        assert!(TrustLevel::Assumed.satisfies(TrustLevel::Assumed));
        assert!(!TrustLevel::Assumed.satisfies(TrustLevel::Verified));
    }

    // ── satisfies_unsafe_meets_only_itself ────────────────────────────────
    #[test]
    fn satisfies_unsafe_meets_only_itself() {
        assert!(TrustLevel::Unsafe.satisfies(TrustLevel::Unsafe));
        assert!(!TrustLevel::Unsafe.satisfies(TrustLevel::Unverified));
    }

    // ── ordering_is_total_and_stable ─────────────────────────────────────
    // TRIANGULATE: the derived Ord matches declaration order.
    #[test]
    fn ordering_is_total_and_stable() {
        assert!(TrustLevel::Unsafe < TrustLevel::Unverified);
        assert!(TrustLevel::Unverified < TrustLevel::Assumed);
        assert!(TrustLevel::Assumed < TrustLevel::Verified);
    }

    // ── display_produces_lowercase_string ────────────────────────────────
    #[test]
    fn display_produces_lowercase_string() {
        assert_eq!(TrustLevel::Verified.to_string(), "verified");
        assert_eq!(TrustLevel::Unverified.to_string(), "unverified");
        assert_eq!(TrustLevel::Assumed.to_string(), "assumed");
        assert_eq!(TrustLevel::Unsafe.to_string(), "unsafe");
    }
}
