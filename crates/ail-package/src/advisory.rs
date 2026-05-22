// ── ail-package::advisory ─────────────────────────────────────────────────
//
// Security advisories and advisory checking for packages.
//
// # Design decisions
//
// - `AdvisoryChecker` is a stateless unit struct; all methods take a slice
//   of `SecurityAdvisory` so the caller owns the advisory store.
// - Version constraint matching is exact-string for now (semver ranges are
//   future work documented in the open design questions in packages.md).

use serde::{Deserialize, Serialize};

// ── AdvisorySeverity ──────────────────────────────────────────────────────

/// The severity level of a security advisory.
///
/// Variants are ordered from least to most severe:
/// `Low` < `Medium` < `High` < `Critical`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AdvisorySeverity {
    /// Minimal risk; no immediate action required.
    Low,
    /// Moderate risk; patching recommended soon.
    Medium,
    /// High risk; prompt patching required.
    High,
    /// Severe risk; immediate action required.
    Critical,
}

impl std::fmt::Display for AdvisorySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AdvisorySeverity::Low => "low",
            AdvisorySeverity::Medium => "medium",
            AdvisorySeverity::High => "high",
            AdvisorySeverity::Critical => "critical",
        };
        f.write_str(s)
    }
}

// ── SecurityAdvisory ──────────────────────────────────────────────────────

/// A security advisory for a specific package version range.
///
/// The `affected_constraint` field records a version constraint string
/// (e.g., `"<1.2.3"` or `"1.0.0"`).  The `AdvisoryChecker` performs
/// exact-string matching on this field for the initial implementation.
/// Semver range resolution is tracked as an open design question.
///
/// See `docs/packages.md` §Revocation and advisories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityAdvisory {
    /// Stable advisory identifier (e.g., `"adv_123"`).
    pub id: String,
    /// Name of the affected package (e.g., `"payments.stripe"`).
    pub package: String,
    /// Version constraint string describing affected versions (e.g., `"<1.2.3"`).
    pub affected_constraint: String,
    /// Severity of this advisory.
    pub severity: AdvisorySeverity,
    /// Human-readable reason / description of the vulnerability.
    pub reason: String,
}

// ── AdvisoryChecker ───────────────────────────────────────────────────────

/// Stateless checker that tests whether a package version is affected by
/// any of a given set of security advisories.
pub struct AdvisoryChecker;

impl AdvisoryChecker {
    /// Return `true` if any advisory in `advisories` matches the given
    /// `name` and `version`.
    ///
    /// Matching rules (current implementation):
    /// 1. `advisory.package == name` (exact match)
    /// 2. `advisory.affected_constraint == version` (exact match)
    ///
    /// # Note
    ///
    /// Semver range evaluation (e.g., `<1.2.3`) is an open design question
    /// listed in `docs/packages.md` and is not implemented in this version.
    pub fn is_affected(name: &str, version: &str, advisories: &[SecurityAdvisory]) -> bool {
        advisories
            .iter()
            .any(|adv| adv.package == name && adv.affected_constraint == version)
    }

    /// Return the first advisory in `advisories` that matches `name` and `version`,
    /// or `None` if no match is found.
    pub fn first_match<'a>(
        name: &str,
        version: &str,
        advisories: &'a [SecurityAdvisory],
    ) -> Option<&'a SecurityAdvisory> {
        advisories
            .iter()
            .find(|adv| adv.package == name && adv.affected_constraint == version)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_advisory() -> SecurityAdvisory {
        SecurityAdvisory {
            id: "adv_001".to_string(),
            package: "payments.stripe".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::Critical,
            reason: "idempotency handler bug".to_string(),
        }
    }

    // ── RED: advisory_severity_ordering ──────────────────────────────────
    // Spec: REQ-ADV-1 — AdvisorySeverity is ordered Low < Medium < High < Critical
    //   GIVEN AdvisorySeverity variants
    //   WHEN compared with Ord
    //   THEN the ordering is Low < Medium < High < Critical
    #[test]
    fn advisory_severity_ordering() {
        assert!(AdvisorySeverity::Low < AdvisorySeverity::Medium);
        assert!(AdvisorySeverity::Medium < AdvisorySeverity::High);
        assert!(AdvisorySeverity::High < AdvisorySeverity::Critical);
    }

    // ── RED: security_advisory_cbor_round_trip ────────────────────────────
    // Spec: REQ-ADV-5 — SecurityAdvisory is CBOR-serializable
    //   GIVEN a SecurityAdvisory with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn security_advisory_cbor_round_trip() {
        let original = sample_advisory();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialize must succeed");
        let decoded: SecurityAdvisory =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialize must succeed");

        assert_eq!(decoded, original, "decoded advisory must equal original");
    }

    // ── RED: is_affected_returns_true_for_matching_advisory ───────────────
    // Spec: REQ-ADV-3 — is_affected returns true when name+version match
    //   GIVEN advisories containing an entry for "payments.stripe" version "1.0.0"
    //   WHEN is_affected("payments.stripe", "1.0.0", &advisories) is called
    //   THEN it returns true
    #[test]
    fn is_affected_returns_true_for_matching_advisory() {
        let advisories = vec![sample_advisory()];
        assert!(
            AdvisoryChecker::is_affected("payments.stripe", "1.0.0", &advisories),
            "matching advisory must return true"
        );
    }

    // ── RED: is_affected_returns_false_for_different_version ──────────────
    // TRIANGULATE: version mismatch → not affected
    //   GIVEN advisories for version "1.0.0"
    //   WHEN is_affected is called with version "2.0.0"
    //   THEN it returns false
    #[test]
    fn is_affected_returns_false_for_different_version() {
        let advisories = vec![sample_advisory()];
        assert!(
            !AdvisoryChecker::is_affected("payments.stripe", "2.0.0", &advisories),
            "version mismatch must not be affected"
        );
    }

    // ── RED: is_affected_returns_false_for_different_name ─────────────────
    // TRIANGULATE: name mismatch → not affected
    #[test]
    fn is_affected_returns_false_for_different_name() {
        let advisories = vec![sample_advisory()];
        assert!(
            !AdvisoryChecker::is_affected("other.package", "1.0.0", &advisories),
            "name mismatch must not be affected"
        );
    }

    // ── RED: is_affected_returns_false_for_empty_advisories ───────────────
    // TRIANGULATE: empty advisory list → not affected
    #[test]
    fn is_affected_returns_false_for_empty_advisories() {
        assert!(
            !AdvisoryChecker::is_affected("any.pkg", "1.0.0", &[]),
            "empty advisories must not be affected"
        );
    }

    // ── RED: first_match_returns_matching_advisory ────────────────────────
    // Spec: REQ-ADV-3 — first_match returns the matching advisory
    #[test]
    fn first_match_returns_matching_advisory() {
        let advisories = vec![sample_advisory()];
        let found = AdvisoryChecker::first_match("payments.stripe", "1.0.0", &advisories);
        assert!(found.is_some(), "first_match must find matching advisory");
        assert_eq!(found.unwrap().id, "adv_001");
    }

    // ── RED: severity_display ─────────────────────────────────────────────
    #[test]
    fn severity_display() {
        assert_eq!(AdvisorySeverity::Low.to_string(), "low");
        assert_eq!(AdvisorySeverity::Medium.to_string(), "medium");
        assert_eq!(AdvisorySeverity::High.to_string(), "high");
        assert_eq!(AdvisorySeverity::Critical.to_string(), "critical");
    }
}
