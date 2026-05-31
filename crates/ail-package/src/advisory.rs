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

// ── AdvisoryPolicyAction / AdvisoryPolicyIssue ────────────────────────────

/// Policy action produced by the package advisory gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryPolicyAction {
    /// The package is allowed, but the issue should be surfaced.
    Warn,
    /// The package must be blocked by policy.
    Block,
}

/// Stable package policy issue code.
///
/// Codes are intentionally explicit strings so callers can key CI gates, audit
/// output, and UI affordances without depending on prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvisoryPolicyIssueCode {
    /// The exact package version has been yanked.
    #[serde(rename = "package.yanked")]
    PackageYanked,
    /// A matching critical security advisory blocks the package.
    #[serde(rename = "package.advisory.critical")]
    AdvisoryCritical,
    /// A matching high security advisory blocks the package.
    #[serde(rename = "package.advisory.high")]
    AdvisoryHigh,
    /// A matching medium or low advisory warns but does not block.
    #[serde(rename = "package.advisory.warning")]
    AdvisoryWarning,
}

impl AdvisoryPolicyIssueCode {
    /// Return the stable machine-readable issue code.
    pub fn as_str(self) -> &'static str {
        match self {
            AdvisoryPolicyIssueCode::PackageYanked => "package.yanked",
            AdvisoryPolicyIssueCode::AdvisoryCritical => "package.advisory.critical",
            AdvisoryPolicyIssueCode::AdvisoryHigh => "package.advisory.high",
            AdvisoryPolicyIssueCode::AdvisoryWarning => "package.advisory.warning",
        }
    }
}

impl std::fmt::Display for AdvisoryPolicyIssueCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One issue emitted by the advisory policy gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryPolicyIssue {
    /// Stable machine-readable issue code.
    pub code: AdvisoryPolicyIssueCode,
    /// Whether this issue warns or blocks.
    pub action: AdvisoryPolicyAction,
    /// Advisory ID when the issue came from an advisory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisory_id: Option<String>,
    /// Advisory severity when the issue came from an advisory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<AdvisorySeverity>,
    /// Stable human-readable reason.
    pub reason: String,
}

/// Deterministic advisory policy decision for a package version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryPolicyDecision {
    /// Issues in policy priority order: yank, critical, high, then warnings.
    pub issues: Vec<AdvisoryPolicyIssue>,
}

impl AdvisoryPolicyDecision {
    /// Return true when at least one issue blocks the package.
    pub fn is_blocked(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.action == AdvisoryPolicyAction::Block)
    }

    /// Return blocking issue codes in deterministic policy order.
    pub fn blocking_codes(&self) -> Vec<&'static str> {
        self.issues
            .iter()
            .filter(|issue| issue.action == AdvisoryPolicyAction::Block)
            .map(|issue| issue.code.as_str())
            .collect()
    }
}

/// Package policy gate for yanks and matching security advisories.
pub struct AdvisoryPolicyGate;

impl AdvisoryPolicyGate {
    /// Evaluate policy for a package version.
    ///
    /// Blocking order is deterministic and independent of registry insertion
    /// order: yank first, then critical advisories, high advisories, and
    /// finally medium/low advisory warnings. A yanked package with advisories
    /// reports both conditions instead of hiding the advisory behind the yank.
    pub fn evaluate(
        name: &str,
        version: &str,
        yanked_reason: Option<&str>,
        advisories: &[SecurityAdvisory],
    ) -> AdvisoryPolicyDecision {
        let mut issues = Vec::new();

        if let Some(reason) = yanked_reason {
            issues.push(AdvisoryPolicyIssue {
                code: AdvisoryPolicyIssueCode::PackageYanked,
                action: AdvisoryPolicyAction::Block,
                advisory_id: None,
                severity: None,
                reason: reason.to_string(),
            });
        }

        for advisory in AdvisoryChecker::matches(name, version, advisories) {
            let (code, action) = match advisory.severity {
                AdvisorySeverity::Critical => (
                    AdvisoryPolicyIssueCode::AdvisoryCritical,
                    AdvisoryPolicyAction::Block,
                ),
                AdvisorySeverity::High => (
                    AdvisoryPolicyIssueCode::AdvisoryHigh,
                    AdvisoryPolicyAction::Block,
                ),
                AdvisorySeverity::Medium | AdvisorySeverity::Low => (
                    AdvisoryPolicyIssueCode::AdvisoryWarning,
                    AdvisoryPolicyAction::Warn,
                ),
            };

            issues.push(AdvisoryPolicyIssue {
                code,
                action,
                advisory_id: Some(advisory.id.clone()),
                severity: Some(advisory.severity),
                reason: advisory.reason.clone(),
            });
        }

        AdvisoryPolicyDecision { issues }
    }
}

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
/// semver range matching with exact-string fallback for unparseable values.
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

    #[test]
    fn policy_issue_codes_are_stable_strings() {
        let encoded = serde_json::to_string(&AdvisoryPolicyIssueCode::AdvisoryCritical)
            .expect("policy issue code must serialize");

        assert_eq!(encoded, "\"package.advisory.critical\"");
        assert_eq!(
            AdvisoryPolicyIssueCode::PackageYanked.as_str(),
            "package.yanked"
        );
        assert_eq!(
            AdvisoryPolicyIssueCode::AdvisoryCritical.as_str(),
            "package.advisory.critical"
        );
        assert_eq!(
            AdvisoryPolicyIssueCode::AdvisoryHigh.as_str(),
            "package.advisory.high"
        );
        assert_eq!(
            AdvisoryPolicyIssueCode::AdvisoryWarning.as_str(),
            "package.advisory.warning"
        );
    }

    #[test]
    fn advisory_policy_blocks_critical_and_high_advisories_in_stable_order() {
        let advisories = vec![
            SecurityAdvisory {
                id: "adv_low".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::Low,
                reason: "low signal".to_string(),
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
                reason: "critical".to_string(),
            },
            SecurityAdvisory {
                id: "adv_high_a".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "high a".to_string(),
            },
        ];

        let decision = AdvisoryPolicyGate::evaluate("payments.stripe", "1.5.0", None, &advisories);

        assert!(decision.is_blocked());
        assert_eq!(
            decision.blocking_codes(),
            vec![
                "package.advisory.critical",
                "package.advisory.high",
                "package.advisory.high"
            ]
        );
        assert_eq!(
            decision
                .issues
                .iter()
                .map(|issue| issue.advisory_id.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("adv_critical"),
                Some("adv_high_a"),
                Some("adv_high_b"),
                Some("adv_low")
            ]
        );
        assert_eq!(
            decision
                .issues
                .iter()
                .map(|issue| issue.action)
                .collect::<Vec<_>>(),
            vec![
                AdvisoryPolicyAction::Block,
                AdvisoryPolicyAction::Block,
                AdvisoryPolicyAction::Block,
                AdvisoryPolicyAction::Warn
            ]
        );
    }

    #[test]
    fn advisory_policy_reports_yanked_and_advisory_without_order_flapping() {
        let advisories = vec![
            SecurityAdvisory {
                id: "adv_high".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "1.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "credential leak".to_string(),
            },
            SecurityAdvisory {
                id: "adv_critical".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "1.0.0".to_string(),
                severity: AdvisorySeverity::Critical,
                reason: "remote execution".to_string(),
            },
        ];

        let decision = AdvisoryPolicyGate::evaluate(
            "payments.stripe",
            "1.0.0",
            Some("compromised release"),
            &advisories,
        );

        assert!(decision.is_blocked());
        assert_eq!(
            decision.blocking_codes(),
            vec![
                "package.yanked",
                "package.advisory.critical",
                "package.advisory.high"
            ]
        );
        assert_eq!(
            decision
                .issues
                .iter()
                .map(|issue| (issue.code.as_str(), issue.advisory_id.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("package.yanked", None),
                ("package.advisory.critical", Some("adv_critical")),
                ("package.advisory.high", Some("adv_high"))
            ]
        );
    }

    #[test]
    fn advisory_policy_warns_without_blocking_for_medium_and_low_only() {
        let advisories = vec![
            SecurityAdvisory {
                id: "adv_medium".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "1.0.0".to_string(),
                severity: AdvisorySeverity::Medium,
                reason: "medium issue".to_string(),
            },
            SecurityAdvisory {
                id: "adv_low".to_string(),
                package: "payments.stripe".to_string(),
                affected_constraint: "1.0.0".to_string(),
                severity: AdvisorySeverity::Low,
                reason: "low issue".to_string(),
            },
        ];

        let decision = AdvisoryPolicyGate::evaluate("payments.stripe", "1.0.0", None, &advisories);

        assert!(!decision.is_blocked());
        assert!(decision.blocking_codes().is_empty());
        assert_eq!(
            decision
                .issues
                .iter()
                .map(|issue| (
                    issue.code.as_str(),
                    issue.action,
                    issue.advisory_id.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "package.advisory.warning",
                    AdvisoryPolicyAction::Warn,
                    Some("adv_medium")
                ),
                (
                    "package.advisory.warning",
                    AdvisoryPolicyAction::Warn,
                    Some("adv_low")
                )
            ]
        );
    }
}
