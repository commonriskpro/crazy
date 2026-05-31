// ── ail-package::advisory ─────────────────────────────────────────────────
//
// Security advisories and advisory checking for packages.
//
// # Design decisions
//
// - `AdvisoryChecker` is a stateless unit struct; all methods take a slice
//   of `SecurityAdvisory` so the caller owns the advisory store.
// - Version constraint matching supports semver range expressions as defined
//   in `docs/packages.md` §Revocation and advisories:
//     - Bare version string (e.g., `"1.0.0"`) → exact match
//     - `<VERSION`  → versions strictly less than VERSION
//     - `<=VERSION` → versions less than or equal to VERSION
//     - `>VERSION`  → versions strictly greater than VERSION
//     - `>=VERSION` → versions greater than or equal to VERSION
//     - `^VERSION`  → semver caret ranges (compatible with)
//     - `~VERSION`  → semver tilde ranges (approximately equal)
// - Constraint parsing falls back to exact-string match when unparseable.

use semver::{Version, VersionReq};
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
    fn compare_matches(left: &SecurityAdvisory, right: &SecurityAdvisory) -> std::cmp::Ordering {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.package.cmp(&right.package))
            .then_with(|| left.affected_constraint.cmp(&right.affected_constraint))
            .then_with(|| left.reason.cmp(&right.reason))
    }

    /// Test whether `version` matches `constraint`.
    ///
    /// Constraint evaluation order:
    /// 1. Try to parse as a `semver::VersionReq` (handles `<`, `<=`, `>`,
    ///    `>=`, `^`, `~`, and compound ranges).
    /// 2. Fall back to exact-string equality for bare unversioned strings.
    fn version_matches(version: &str, constraint: &str) -> bool {
        // Attempt semver VersionReq parse.  If the constraint is a bare
        // version (no operator), VersionReq parses it as `^version` (caret),
        // so we pre-check for an exact match first.
        if !constraint.starts_with(['<', '>', '^', '~', '=', '*']) {
            // Bare version string — exact match only.
            return constraint == version;
        }

        let Ok(ver) = Version::parse(version) else {
            // Package version is not a valid semver — fall back to exact match.
            return constraint == version;
        };

        match VersionReq::parse(constraint) {
            Ok(req) => req.matches(&ver),
            // Unparseable constraint — fall back to exact match.
            Err(_) => constraint == version,
        }
    }

    /// Return `true` if any advisory in `advisories` matches the given
    /// `name` and `version`.
    ///
    /// Constraint matching supports semver range expressions (e.g., `<1.2.3`).
    /// See module-level docs for the full syntax.
    pub fn is_affected(name: &str, version: &str, advisories: &[SecurityAdvisory]) -> bool {
        advisories.iter().any(|adv| {
            adv.package == name && Self::version_matches(version, &adv.affected_constraint)
        })
    }

    /// Return the highest-severity matching advisory in `advisories`,
    /// or `None` if no match is found.
    ///
    /// Ties are ordered by stable advisory metadata so callers do not make
    /// install/publish/audit decisions from registry insertion order.
    pub fn first_match<'a>(
        name: &str,
        version: &str,
        advisories: &'a [SecurityAdvisory],
    ) -> Option<&'a SecurityAdvisory> {
        Self::matches(name, version, advisories).into_iter().next()
    }

    /// Return all advisories that match `name` and `version`.
    ///
    /// Results are deterministic and severity-first: Critical, High, Medium,
    /// Low, then advisory ID and other stable fields. This keeps audit JSON,
    /// resolver diagnostics, and package policy gates stable when registry
    /// metadata arrives in a different order.
    pub fn matches<'a>(
        name: &str,
        version: &str,
        advisories: &'a [SecurityAdvisory],
    ) -> Vec<&'a SecurityAdvisory> {
        let mut matches = advisories
            .iter()
            .filter(|adv| {
                adv.package == name && Self::version_matches(version, &adv.affected_constraint)
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| Self::compare_matches(*left, *right));
        matches
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

    #[test]
    fn matches_returns_all_matching_advisories() {
        let advisories = vec![
            sample_advisory(),
            SecurityAdvisory {
                id: "adv_002".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "second matching advisory".to_string(),
            },
            SecurityAdvisory {
                id: "adv_other".to_string(),
                package: "other.package".to_string(),
                affected_constraint: "1.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "unrelated".to_string(),
            },
        ];

        let matches = AdvisoryChecker::matches("payments.stripe", "1.0.0", &advisories);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].id, "adv_001");
        assert_eq!(matches[1].id, "adv_002");
    }

    #[test]
    fn matches_are_severity_first_and_deterministic() {
        let advisories = vec![
            SecurityAdvisory {
                id: "adv_low_z".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::Low,
                reason: "low inserted first".to_string(),
            },
            SecurityAdvisory {
                id: "adv_high_b".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "high b".to_string(),
            },
            SecurityAdvisory {
                id: "adv_critical".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::Critical,
                reason: "critical inserted late".to_string(),
            },
            SecurityAdvisory {
                id: "adv_high_a".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "high a".to_string(),
            },
        ];

        let ids = AdvisoryChecker::matches("payments.stripe", "1.5.0", &advisories)
            .into_iter()
            .map(|advisory| advisory.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec!["adv_critical", "adv_high_a", "adv_high_b", "adv_low_z"],
            "matching advisories must not inherit registry insertion order"
        );
    }

    #[test]
    fn first_match_returns_highest_severity_match() {
        let advisories = vec![
            SecurityAdvisory {
                id: "adv_low_first".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::Low,
                reason: "low inserted first".to_string(),
            },
            SecurityAdvisory {
                id: "adv_critical_later".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::Critical,
                reason: "critical inserted later".to_string(),
            },
        ];

        let found = AdvisoryChecker::first_match("payments.stripe", "1.1.0", &advisories)
            .expect("matching advisory must be found");

        assert_eq!(found.id, "adv_critical_later");
        assert_eq!(found.severity, AdvisorySeverity::Critical);
    }

    // ── RED: severity_display ─────────────────────────────────────────────
    #[test]
    fn severity_display() {
        assert_eq!(AdvisorySeverity::Low.to_string(), "low");
        assert_eq!(AdvisorySeverity::Medium.to_string(), "medium");
        assert_eq!(AdvisorySeverity::High.to_string(), "high");
        assert_eq!(AdvisorySeverity::Critical.to_string(), "critical");
    }

    // ── semver_lt_constraint_matches_older_versions ───────────────────────
    // Spec scenario: "Advisory with <1.2.3 matches versions strictly less than 1.2.3"
    //   GIVEN advisory with affected_constraint: "<1.2.3"
    //   WHEN is_affected("stripe", "1.0.0", ..) is called
    //   THEN returns true
    //   WHEN is_affected("stripe", "1.2.3", ..) is called
    //   THEN returns false (1.2.3 is not strictly less than 1.2.3)
    #[test]
    fn semver_lt_constraint_matches_older_versions() {
        let advisory = SecurityAdvisory {
            id: "adv_range".to_string(),
            package: "payments.stripe".to_string(),
            affected_constraint: "<1.2.3".to_string(),
            severity: AdvisorySeverity::Critical,
            reason: "idempotency bug".to_string(),
        };
        let advisories = vec![advisory];

        assert!(
            AdvisoryChecker::is_affected("payments.stripe", "1.0.0", &advisories),
            "1.0.0 < 1.2.3 — should be affected"
        );
        assert!(
            AdvisoryChecker::is_affected("payments.stripe", "1.2.2", &advisories),
            "1.2.2 < 1.2.3 — should be affected"
        );
        assert!(
            !AdvisoryChecker::is_affected("payments.stripe", "1.2.3", &advisories),
            "1.2.3 is NOT < 1.2.3 — should not be affected"
        );
        assert!(
            !AdvisoryChecker::is_affected("payments.stripe", "2.0.0", &advisories),
            "2.0.0 is NOT < 1.2.3 — should not be affected"
        );
    }

    // ── semver_gte_constraint_matches_newer_versions ──────────────────────
    // Spec scenario: Advisory with >=1.0.0 matches versions at or above 1.0.0
    #[test]
    fn semver_gte_constraint_matches_newer_versions() {
        let advisory = SecurityAdvisory {
            id: "adv_gte".to_string(),
            package: "utils.core".to_string(),
            affected_constraint: ">=1.0.0".to_string(),
            severity: AdvisorySeverity::High,
            reason: "regression".to_string(),
        };
        let advisories = vec![advisory];

        assert!(AdvisoryChecker::is_affected(
            "utils.core",
            "1.0.0",
            &advisories
        ));
        assert!(AdvisoryChecker::is_affected(
            "utils.core",
            "2.5.0",
            &advisories
        ));
        assert!(!AdvisoryChecker::is_affected(
            "utils.core",
            "0.9.9",
            &advisories
        ));
    }

    // ── semver_caret_constraint_matches_compatible_versions ───────────────
    // Spec scenario: Advisory with ^1.0.0 matches 1.x.x but not 2.x.x
    #[test]
    fn semver_caret_constraint_matches_compatible_versions() {
        let advisory = SecurityAdvisory {
            id: "adv_caret".to_string(),
            package: "lib.auth".to_string(),
            affected_constraint: "^1.0.0".to_string(),
            severity: AdvisorySeverity::Medium,
            reason: "auth bypass".to_string(),
        };
        let advisories = vec![advisory];

        assert!(AdvisoryChecker::is_affected(
            "lib.auth",
            "1.0.0",
            &advisories
        ));
        assert!(AdvisoryChecker::is_affected(
            "lib.auth",
            "1.9.9",
            &advisories
        ));
        assert!(!AdvisoryChecker::is_affected(
            "lib.auth",
            "2.0.0",
            &advisories
        ));
    }

    // ── exact_string_still_works ──────────────────────────────────────────
    // TRIANGULATE: bare version (no operator) still does exact-match only
    #[test]
    fn exact_string_still_works() {
        let advisory = SecurityAdvisory {
            id: "adv_exact".to_string(),
            package: "payments.stripe".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::Low,
            reason: "minor".to_string(),
        };
        let advisories = vec![advisory];
        assert!(AdvisoryChecker::is_affected(
            "payments.stripe",
            "1.0.0",
            &advisories
        ));
        assert!(!AdvisoryChecker::is_affected(
            "payments.stripe",
            "1.0.1",
            &advisories
        ));
    }
}
