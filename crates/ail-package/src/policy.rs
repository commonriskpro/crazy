// ── ail-package::policy ───────────────────────────────────────────────────
//
// Profile-based trust gates and least-privilege capability policy enforcement.
//
// # Design (docs/packages.md §Package trust by profile)
//
// Trust gates vary by profile:
//   draft:    unverified allowed with warning
//   dev:      unverified private allowed by policy
//   test:     test-only assumed/unverified allowed
//   staging:  unverified blocked
//   prod:     verified or approved assumed only
//   critical: verified preferred; assumed requires strong approval
//
// # Design (docs/packages.md §Package capabilities and least privilege)
//
// Policy can reject broad requests:
//   deny capability file.write:*
//   deny capability http.call:* unless approved
//
// Policy enforcement produces either a verdict (Allow/Deny/Warn) per
// package, plus a per-capability verdict for broad-capability requests.

use serde::{Deserialize, Serialize};

use crate::trust::TrustLevel;

// ── DeploymentProfile ─────────────────────────────────────────────────────

/// Deployment profile determining which trust levels are permitted.
///
/// See `docs/packages.md` §Package trust by profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentProfile {
    /// Draft / scratch environment.  Unverified packages are allowed with a warning.
    Draft,
    /// Development environment.  Unverified private packages allowed by policy.
    Dev,
    /// Test environment.  Test-only assumed/unverified packages allowed.
    Test,
    /// Staging environment.  Unverified packages are blocked.
    Staging,
    /// Production environment.  Verified or approved-assumed only.
    Prod,
    /// Critical environment.  Verified preferred; assumed requires strong approval.
    Critical,
}

impl std::fmt::Display for DeploymentProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DeploymentProfile::Draft => "draft",
            DeploymentProfile::Dev => "dev",
            DeploymentProfile::Test => "test",
            DeploymentProfile::Staging => "staging",
            DeploymentProfile::Prod => "prod",
            DeploymentProfile::Critical => "critical",
        };
        write!(f, "{s}")
    }
}

// ── TrustGateVerdict ──────────────────────────────────────────────────────

/// The verdict of a profile-based trust gate check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustGateVerdict {
    /// The package is allowed in this profile.
    Allow,
    /// The package is conditionally allowed but emits a warning.
    Warn,
    /// The package is blocked in this profile.
    Deny,
}

// ── TrustGate ─────────────────────────────────────────────────────────────

/// Evaluates whether a package trust level is permitted in a given profile.
pub struct TrustGate;

impl TrustGate {
    /// Evaluate whether a package with the given `trust_level` is permitted
    /// in the given `profile`.
    ///
    /// | Profile   | Verified | Assumed | Unverified | Unsafe |
    /// |-----------|----------|---------|------------|--------|
    /// | Draft     | Allow    | Allow   | Warn       | Deny   |
    /// | Dev       | Allow    | Allow   | Warn       | Deny   |
    /// | Test      | Allow    | Allow   | Warn       | Deny   |
    /// | Staging   | Allow    | Allow   | Deny       | Deny   |
    /// | Prod      | Allow    | Allow   | Deny       | Deny   |
    /// | Critical  | Allow    | Warn    | Deny       | Deny   |
    ///
    /// Note: `Assumed` in `Prod` is allowed when the assumption has been
    /// explicitly accepted via an `ApprovalRecord`.  The gate check here
    /// returns `Allow` at the profile level; assumption acceptance is
    /// enforced separately by `AssumptionEnforcer`.
    pub fn evaluate(trust_level: TrustLevel, profile: DeploymentProfile) -> TrustGateVerdict {
        match (profile, trust_level) {
            // Verified: always allowed in any profile
            (_, TrustLevel::Verified) => TrustGateVerdict::Allow,

            // Unsafe: always denied in any profile
            (_, TrustLevel::Unsafe) => TrustGateVerdict::Deny,

            // Assumed
            (DeploymentProfile::Critical, TrustLevel::Assumed) => TrustGateVerdict::Warn,
            (_, TrustLevel::Assumed) => TrustGateVerdict::Allow,

            // Unverified
            (DeploymentProfile::Draft, TrustLevel::Unverified) => TrustGateVerdict::Warn,
            (DeploymentProfile::Dev, TrustLevel::Unverified) => TrustGateVerdict::Warn,
            (DeploymentProfile::Test, TrustLevel::Unverified) => TrustGateVerdict::Warn,
            (DeploymentProfile::Staging, TrustLevel::Unverified) => TrustGateVerdict::Deny,
            (DeploymentProfile::Prod, TrustLevel::Unverified) => TrustGateVerdict::Deny,
            (DeploymentProfile::Critical, TrustLevel::Unverified) => TrustGateVerdict::Deny,
        }
    }
}

// ── CapabilityPolicy ──────────────────────────────────────────────────────

/// A policy rule for capability requests.
///
/// # Example (from docs/packages.md)
/// ```text
/// deny capability file.write:*
/// deny capability http.call:* unless approved
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityPolicy {
    /// Capability pattern to match (e.g., `"file.write:*"`, `"http.call:Stripe"`).
    ///
    /// A trailing `:*` means "any provider for this capability prefix".
    pub pattern: String,
    /// The verdict applied when this rule matches.
    pub verdict: CapabilityPolicyVerdict,
}

/// Verdict for a capability policy rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityPolicyVerdict {
    /// The capability request is allowed.
    Allow,
    /// The capability request is denied.
    Deny,
    /// The capability request requires explicit approval before being allowed.
    DenyUnlessApproved,
}

// ── CapabilityPolicyEnforcer ──────────────────────────────────────────────

/// Enforces capability policies against a package's requested capabilities.
pub struct CapabilityPolicyEnforcer;

/// Result of a capability policy check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityViolation {
    /// The capability that was denied.
    pub capability: String,
    /// The policy verdict applied.
    pub verdict: CapabilityPolicyVerdict,
}

impl std::fmt::Display for CapabilityViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.verdict {
            CapabilityPolicyVerdict::Deny => {
                write!(f, "capability '{}' is denied by policy", self.capability)
            }
            CapabilityPolicyVerdict::DenyUnlessApproved => {
                write!(
                    f,
                    "capability '{}' requires explicit approval",
                    self.capability
                )
            }
            CapabilityPolicyVerdict::Allow => {
                write!(f, "capability '{}' is allowed", self.capability)
            }
        }
    }
}

impl CapabilityPolicyEnforcer {
    /// Test whether `capability` matches `pattern`.
    ///
    /// Matching rules:
    /// - If `pattern` ends with `:*`, match any capability whose prefix
    ///   (up to `:`) equals the pattern prefix.
    /// - Otherwise, exact string match.
    fn matches(capability: &str, pattern: &str) -> bool {
        if let Some(prefix) = pattern.strip_suffix(":*") {
            // Wildcard: match capability prefix before ':'
            let cap_prefix = capability.split(':').next().unwrap_or(capability);
            cap_prefix == prefix
        } else {
            capability == pattern
        }
    }

    /// Check `requested_capabilities` against `policies`.
    ///
    /// For each capability, apply the first matching policy rule in order.
    /// If no rule matches, the capability is allowed by default.
    ///
    /// Returns violations for capabilities that are `Deny` or `DenyUnlessApproved`.
    pub fn check(
        requested_capabilities: &[String],
        policies: &[CapabilityPolicy],
    ) -> Vec<CapabilityViolation> {
        let mut violations = Vec::new();

        for cap in requested_capabilities {
            // Find first matching policy rule.
            let verdict = policies
                .iter()
                .find(|p| Self::matches(cap, &p.pattern))
                .map(|p| p.verdict)
                .unwrap_or(CapabilityPolicyVerdict::Allow);

            if matches!(
                verdict,
                CapabilityPolicyVerdict::Deny | CapabilityPolicyVerdict::DenyUnlessApproved
            ) {
                violations.push(CapabilityViolation {
                    capability: cap.clone(),
                    verdict,
                });
            }
        }

        violations
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── profile_display ───────────────────────────────────────────────────
    #[test]
    fn profile_display() {
        assert_eq!(DeploymentProfile::Draft.to_string(), "draft");
        assert_eq!(DeploymentProfile::Dev.to_string(), "dev");
        assert_eq!(DeploymentProfile::Test.to_string(), "test");
        assert_eq!(DeploymentProfile::Staging.to_string(), "staging");
        assert_eq!(DeploymentProfile::Prod.to_string(), "prod");
        assert_eq!(DeploymentProfile::Critical.to_string(), "critical");
    }

    // ── verified_package_allowed_in_all_profiles ──────────────────────────
    // Spec scenario: "Verified package is always allowed"
    #[test]
    fn verified_package_allowed_in_all_profiles() {
        for profile in [
            DeploymentProfile::Draft,
            DeploymentProfile::Dev,
            DeploymentProfile::Test,
            DeploymentProfile::Staging,
            DeploymentProfile::Prod,
            DeploymentProfile::Critical,
        ] {
            assert_eq!(
                TrustGate::evaluate(TrustLevel::Verified, profile),
                TrustGateVerdict::Allow,
                "Verified must be Allow in {profile}"
            );
        }
    }

    // ── unsafe_package_denied_in_all_profiles ─────────────────────────────
    // Spec scenario: "Unsafe package is always denied"
    #[test]
    fn unsafe_package_denied_in_all_profiles() {
        for profile in [
            DeploymentProfile::Draft,
            DeploymentProfile::Dev,
            DeploymentProfile::Test,
            DeploymentProfile::Staging,
            DeploymentProfile::Prod,
            DeploymentProfile::Critical,
        ] {
            assert_eq!(
                TrustGate::evaluate(TrustLevel::Unsafe, profile),
                TrustGateVerdict::Deny,
                "Unsafe must be Deny in {profile}"
            );
        }
    }

    // ── unverified_blocked_in_staging_and_above ───────────────────────────
    // Spec scenario: "Unverified package blocked in staging/prod/critical"
    #[test]
    fn unverified_blocked_in_staging_and_above() {
        for profile in [
            DeploymentProfile::Staging,
            DeploymentProfile::Prod,
            DeploymentProfile::Critical,
        ] {
            assert_eq!(
                TrustGate::evaluate(TrustLevel::Unverified, profile),
                TrustGateVerdict::Deny,
                "Unverified must be Deny in {profile}"
            );
        }
    }

    // ── unverified_warned_in_dev_and_below ────────────────────────────────
    // Spec scenario: "Unverified package warns in draft/dev/test"
    #[test]
    fn unverified_warned_in_dev_and_below() {
        for profile in [
            DeploymentProfile::Draft,
            DeploymentProfile::Dev,
            DeploymentProfile::Test,
        ] {
            assert_eq!(
                TrustGate::evaluate(TrustLevel::Unverified, profile),
                TrustGateVerdict::Warn,
                "Unverified must be Warn in {profile}"
            );
        }
    }

    // ── assumed_warns_in_critical ─────────────────────────────────────────
    // Spec scenario: "Assumed package warns in critical profile"
    #[test]
    fn assumed_warns_in_critical() {
        assert_eq!(
            TrustGate::evaluate(TrustLevel::Assumed, DeploymentProfile::Critical),
            TrustGateVerdict::Warn
        );
    }

    // ── assumed_allowed_in_prod ───────────────────────────────────────────
    // Spec scenario: "Assumed package (with approval) allowed in prod"
    #[test]
    fn assumed_allowed_in_prod() {
        assert_eq!(
            TrustGate::evaluate(TrustLevel::Assumed, DeploymentProfile::Prod),
            TrustGateVerdict::Allow
        );
    }

    // ── capability_policy_deny_wildcard ───────────────────────────────────
    // Spec scenario: "deny capability file.write:* blocks any file.write capability"
    //   GIVEN policy: deny file.write:*
    //   WHEN package requests file.write:LocalDisk
    //   THEN violation returned
    #[test]
    fn capability_policy_deny_wildcard() {
        let policies = vec![CapabilityPolicy {
            pattern: "file.write:*".to_string(),
            verdict: CapabilityPolicyVerdict::Deny,
        }];
        let caps = vec!["file.write:LocalDisk".to_string()];
        let violations = CapabilityPolicyEnforcer::check(&caps, &policies);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].capability, "file.write:LocalDisk");
        assert_eq!(violations[0].verdict, CapabilityPolicyVerdict::Deny);
    }

    // ── capability_policy_deny_unless_approved ────────────────────────────
    // Spec scenario: "deny capability http.call:* unless approved"
    #[test]
    fn capability_policy_deny_unless_approved() {
        let policies = vec![CapabilityPolicy {
            pattern: "http.call:*".to_string(),
            verdict: CapabilityPolicyVerdict::DenyUnlessApproved,
        }];
        let caps = vec![
            "http.call:Stripe".to_string(),
            "http.call:PayPal".to_string(),
        ];
        let violations = CapabilityPolicyEnforcer::check(&caps, &policies);
        assert_eq!(violations.len(), 2);
        assert!(
            violations
                .iter()
                .all(|v| v.verdict == CapabilityPolicyVerdict::DenyUnlessApproved)
        );
    }

    // ── capability_policy_exact_match ─────────────────────────────────────
    // TRIANGULATE: exact pattern only matches exact capability
    #[test]
    fn capability_policy_exact_match() {
        let policies = vec![CapabilityPolicy {
            pattern: "http.call:Stripe".to_string(),
            verdict: CapabilityPolicyVerdict::Deny,
        }];
        // Exact match — denied
        let caps1 = vec!["http.call:Stripe".to_string()];
        assert_eq!(CapabilityPolicyEnforcer::check(&caps1, &policies).len(), 1);

        // Different provider — allowed (no matching rule)
        let caps2 = vec!["http.call:PayPal".to_string()];
        assert!(CapabilityPolicyEnforcer::check(&caps2, &policies).is_empty());
    }

    // ── capability_policy_no_rule_allows ──────────────────────────────────
    // TRIANGULATE: capability with no matching rule is allowed by default
    #[test]
    fn capability_policy_no_rule_allows() {
        let policies = vec![CapabilityPolicy {
            pattern: "file.write:*".to_string(),
            verdict: CapabilityPolicyVerdict::Deny,
        }];
        let caps = vec!["payment.charge:Stripe".to_string()];
        assert!(CapabilityPolicyEnforcer::check(&caps, &policies).is_empty());
    }

    // ── capability_policy_cbor_round_trip ─────────────────────────────────
    // TRIANGULATE: CapabilityPolicy is CBOR-serializable
    #[test]
    fn capability_policy_cbor_round_trip() {
        let p = CapabilityPolicy {
            pattern: "file.write:*".to_string(),
            verdict: CapabilityPolicyVerdict::Deny,
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&p, &mut buf).expect("encode");
        let decoded: CapabilityPolicy = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, p);
    }
}
