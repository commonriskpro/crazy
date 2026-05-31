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

// ── Package signing trust anchors ─────────────────────────────────────────

/// Trust state for a package signing key in a local trust policy.
///
/// This intentionally models only policy state.  Expiry timestamps and revocation
/// records can live in registry metadata later; diagnostics already have stable
/// states for those cases without exposing raw key material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PackageSigningKeyTrust {
    /// The key is accepted for package signing.
    Trusted,
    /// The key is known but no longer valid for new package trust decisions.
    Expired,
    /// The key was explicitly revoked and must not be trusted.
    Revoked,
}

impl PackageSigningKeyTrust {
    /// Stable policy code for diagnostics and audit logs.
    pub fn code(self) -> &'static str {
        match self {
            PackageSigningKeyTrust::Trusted => "package.signing.key.trusted",
            PackageSigningKeyTrust::Expired => "package.signing.key.expired",
            PackageSigningKeyTrust::Revoked => "package.signing.key.revoked",
        }
    }
}

/// A local trust anchor for an Ed25519 package signing key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSigningTrustAnchor {
    /// Raw Ed25519 public key used for verification lookups.
    ///
    /// This is policy input, not diagnostic output.  Public diagnostics use only
    /// redacted key shape metadata.
    pub public_key: [u8; 32],
    /// Local trust state for this key.
    pub trust: PackageSigningKeyTrust,
}

impl PackageSigningTrustAnchor {
    /// Create a trusted package signing trust anchor.
    pub fn trusted(public_key: [u8; 32]) -> Self {
        Self {
            public_key,
            trust: PackageSigningKeyTrust::Trusted,
        }
    }

    /// Create an expired package signing trust anchor.
    pub fn expired(public_key: [u8; 32]) -> Self {
        Self {
            public_key,
            trust: PackageSigningKeyTrust::Expired,
        }
    }

    /// Create a revoked package signing trust anchor.
    pub fn revoked(public_key: [u8; 32]) -> Self {
        Self {
            public_key,
            trust: PackageSigningKeyTrust::Revoked,
        }
    }
}

/// Local package signing trust policy.
///
/// The policy is intentionally small and deterministic: the first matching key
/// entry wins, and diagnostics never include raw keys or signatures.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSigningTrustPolicy {
    trust_anchors: Vec<PackageSigningTrustAnchor>,
}

impl PackageSigningTrustPolicy {
    /// Create an empty signing trust policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a signing trust policy from explicit trust anchors.
    pub fn from_trust_anchors(trust_anchors: Vec<PackageSigningTrustAnchor>) -> Self {
        Self { trust_anchors }
    }

    /// Add one trust anchor to the policy.
    pub fn add_trust_anchor(&mut self, trust_anchor: PackageSigningTrustAnchor) {
        self.trust_anchors.push(trust_anchor);
    }

    /// Return the trust state for a public key, if the policy represents it.
    pub fn trust_for_key(&self, public_key: &[u8; 32]) -> Option<PackageSigningKeyTrust> {
        self.trust_anchors
            .iter()
            .find(|trust_anchor| &trust_anchor.public_key == public_key)
            .map(|trust_anchor| trust_anchor.trust)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── signing_trust_policy_distinguishes_key_states ────────────────────
    #[test]
    fn signing_trust_policy_distinguishes_key_states() {
        let trusted_key = [1_u8; 32];
        let expired_key = [2_u8; 32];
        let revoked_key = [3_u8; 32];
        let unknown_key = [4_u8; 32];
        let policy = PackageSigningTrustPolicy::from_trust_anchors(vec![
            PackageSigningTrustAnchor::trusted(trusted_key),
            PackageSigningTrustAnchor::expired(expired_key),
            PackageSigningTrustAnchor::revoked(revoked_key),
        ]);

        assert_eq!(
            policy.trust_for_key(&trusted_key),
            Some(PackageSigningKeyTrust::Trusted)
        );
        assert_eq!(
            policy.trust_for_key(&expired_key),
            Some(PackageSigningKeyTrust::Expired)
        );
        assert_eq!(
            policy.trust_for_key(&revoked_key),
            Some(PackageSigningKeyTrust::Revoked)
        );
        assert_eq!(policy.trust_for_key(&unknown_key), None);
    }

    // ── signing_key_trust_codes_are_stable ───────────────────────────────
    #[test]
    fn signing_key_trust_codes_are_stable() {
        assert_eq!(
            PackageSigningKeyTrust::Trusted.code(),
            "package.signing.key.trusted"
        );
        assert_eq!(
            PackageSigningKeyTrust::Expired.code(),
            "package.signing.key.expired"
        );
        assert_eq!(
            PackageSigningKeyTrust::Revoked.code(),
            "package.signing.key.revoked"
        );
    }

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
