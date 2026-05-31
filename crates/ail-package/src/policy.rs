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
use std::collections::BTreeSet;

use crate::manifest::PackageManifest;
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

// ── UnsafeSurfaceApproval / UnsafeSurfacePolicyEnforcer ───────────────────

/// An explicit approval for one entry in a package's `unsafe_surface` list.
///
/// Policy can allow specific unsafe surface items via approval records.
/// See `docs/packages.md` §Unsafe packages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsafeSurfaceApproval {
    /// The `name` field of the approved `UnsafeSurfaceEntry`.
    pub surface_id: String,
}

/// Enforces that every declared unsafe surface entry has an explicit approval.
pub struct UnsafeSurfacePolicyEnforcer;

impl UnsafeSurfacePolicyEnforcer {
    /// Return the surface IDs of `declared` entries that have no matching
    /// approval in `approvals`.
    ///
    /// An entry is approved when its `name` field matches an
    /// `UnsafeSurfaceApproval::surface_id`.
    pub fn check(
        declared: &[crate::surface::UnsafeSurfaceEntry],
        approvals: &[UnsafeSurfaceApproval],
    ) -> Vec<String> {
        declared
            .iter()
            .filter(|entry| {
                !approvals
                    .iter()
                    .any(|approval| approval.surface_id == entry.name)
            })
            .map(|entry| entry.name.clone())
            .collect()
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

// ── PackageProductionPolicy diagnostics ───────────────────────────────────

/// Production package governance policy inputs.
///
/// The policy intentionally stores human-authored match patterns, but public
/// diagnostics emitted from it are redacted: they report stable manifest paths,
/// indexes, and policy indexes instead of copying package coordinates,
/// capability IDs, license text, or export names.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PackageProductionPolicy {
    /// Capability policies applied to required and exported capabilities.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capability_policies: Vec<CapabilityPolicy>,
    /// Export-name patterns that production governance denies.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub denied_exports: Vec<String>,
    /// Import source-package patterns that production governance denies.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub denied_imports: Vec<String>,
    /// Allowed SPDX license expressions. Empty means license policy is not represented.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub allowed_licenses: Vec<String>,
    /// Minimum package trust tier. `None` means minimum-trust policy is not represented.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub minimum_trust: Option<TrustLevel>,
    /// Deployment profile trust gate. `None` means profile policy is not represented.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub deployment_profile: Option<DeploymentProfile>,
    /// When true, include publish-readiness diagnostics from the manifest.
    #[serde(default)]
    pub require_publish_ready: bool,
}

impl PackageProductionPolicy {
    /// Construct a policy with no represented production restrictions.
    pub fn permissive() -> Self {
        Self::default()
    }
}

/// Machine-readable production package-policy diagnostic class.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PackagePolicyDiagnosticKind {
    /// A required or exported capability matched a deny policy.
    DeniedCapability,
    /// An export declaration matched a deny policy.
    DeniedExport,
    /// An import declaration matched a deny policy.
    DeniedImport,
    /// The package license did not match represented license policy.
    LicensePolicyMismatch,
    /// The package trust level is below represented minimum-trust policy.
    MinimumTrustPolicyMismatch,
    /// The package trust level is denied by represented deployment-profile policy.
    ProfileTrustPolicyMismatch,
    /// The manifest failed represented production publish-readiness policy.
    PublishPolicyMismatch,
}

/// Stable redacted location for a production package-policy diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackagePolicyDiagnosticDescriptor {
    /// Stable manifest field path such as `manifest.required_capabilities`.
    pub path: String,
    /// Offending collection entry index, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// Related entry index, when applicable (for example duplicate-of).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_index: Option<usize>,
    /// Matching policy rule index, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_index: Option<usize>,
}

/// Stable, redacted production package-policy diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackagePolicyDiagnostic {
    /// Machine-readable issue class.
    pub kind: PackagePolicyDiagnosticKind,
    /// Redacted location descriptor. Never includes manifest or policy values.
    pub descriptor: PackagePolicyDiagnosticDescriptor,
    /// Human-readable diagnostic. Never includes manifest or policy values.
    pub message: String,
}

impl PackagePolicyDiagnostic {
    fn new(
        kind: PackagePolicyDiagnosticKind,
        path: impl Into<String>,
        index: Option<usize>,
        related_index: Option<usize>,
        policy_index: Option<usize>,
        message: &'static str,
    ) -> Self {
        Self {
            kind,
            descriptor: PackagePolicyDiagnosticDescriptor {
                path: path.into(),
                index,
                related_index,
                policy_index,
            },
            message: message.to_string(),
        }
    }
}

/// Enforces production package governance policy and emits redacted diagnostics.
pub struct PackagePolicyEnforcer;

impl PackagePolicyEnforcer {
    /// Return all represented production policy diagnostics in deterministic order.
    ///
    /// The returned diagnostics are sorted and de-duplicated by stable redacted
    /// issue shape. No raw package coordinates, capability IDs, export names,
    /// import names, license strings, or policy patterns are copied into the
    /// diagnostic payload.
    pub fn diagnostics(
        manifest: &PackageManifest,
        policy: &PackageProductionPolicy,
    ) -> Vec<PackagePolicyDiagnostic> {
        let mut diagnostics = BTreeSet::new();

        push_capability_policy_diagnostics(
            &mut diagnostics,
            &manifest.required_capabilities,
            "manifest.required_capabilities",
            &policy.capability_policies,
        );
        push_capability_policy_diagnostics(
            &mut diagnostics,
            &manifest.exported_capabilities,
            "manifest.exported_capabilities",
            &policy.capability_policies,
        );
        push_export_policy_diagnostics(&mut diagnostics, manifest, &policy.denied_exports);
        push_import_policy_diagnostics(&mut diagnostics, manifest, &policy.denied_imports);
        push_license_policy_diagnostic(&mut diagnostics, manifest, &policy.allowed_licenses);
        push_trust_policy_diagnostics(
            &mut diagnostics,
            manifest,
            policy.minimum_trust,
            policy.deployment_profile,
        );
        if policy.require_publish_ready {
            push_publish_policy_diagnostics(&mut diagnostics, manifest);
        }

        diagnostics.into_iter().collect()
    }

    /// Validate a manifest against represented production governance policy.
    pub fn check(
        manifest: &PackageManifest,
        policy: &PackageProductionPolicy,
    ) -> Result<(), Vec<PackagePolicyDiagnostic>> {
        let diagnostics = Self::diagnostics(manifest, policy);
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(diagnostics)
        }
    }
}

fn push_capability_policy_diagnostics(
    diagnostics: &mut BTreeSet<PackagePolicyDiagnostic>,
    capabilities: &[String],
    path: &'static str,
    policies: &[CapabilityPolicy],
) {
    for (index, capability) in capabilities.iter().enumerate() {
        let Some((policy_index, verdict)) = policies
            .iter()
            .enumerate()
            .find(|(_, policy)| CapabilityPolicyEnforcer::matches(capability, &policy.pattern))
            .map(|(policy_index, policy)| (policy_index, policy.verdict))
        else {
            continue;
        };

        let message = match verdict {
            CapabilityPolicyVerdict::Allow => continue,
            CapabilityPolicyVerdict::Deny => {
                "capability request is denied by production package policy"
            }
            CapabilityPolicyVerdict::DenyUnlessApproved => {
                "capability request requires explicit production package policy approval"
            }
        };

        diagnostics.insert(PackagePolicyDiagnostic::new(
            PackagePolicyDiagnosticKind::DeniedCapability,
            path,
            Some(index),
            None,
            Some(policy_index),
            message,
        ));
    }
}

fn push_export_policy_diagnostics(
    diagnostics: &mut BTreeSet<PackagePolicyDiagnostic>,
    manifest: &PackageManifest,
    denied_exports: &[String],
) {
    for (index, export) in manifest.exports.iter().enumerate() {
        if let Some(policy_index) = first_matching_pattern_index(&export.name, denied_exports) {
            diagnostics.insert(PackagePolicyDiagnostic::new(
                PackagePolicyDiagnosticKind::DeniedExport,
                "manifest.exports.name",
                Some(index),
                None,
                Some(policy_index),
                "export declaration is denied by production package policy",
            ));
        }
    }
}

fn push_import_policy_diagnostics(
    diagnostics: &mut BTreeSet<PackagePolicyDiagnostic>,
    manifest: &PackageManifest,
    denied_imports: &[String],
) {
    for (index, import) in manifest.imports.iter().enumerate() {
        if let Some(policy_index) =
            first_matching_pattern_index(&import.source_package, denied_imports)
        {
            diagnostics.insert(PackagePolicyDiagnostic::new(
                PackagePolicyDiagnosticKind::DeniedImport,
                "manifest.imports.source_package",
                Some(index),
                None,
                Some(policy_index),
                "import declaration is denied by production package policy",
            ));
        }
    }
}

fn push_license_policy_diagnostic(
    diagnostics: &mut BTreeSet<PackagePolicyDiagnostic>,
    manifest: &PackageManifest,
    allowed_licenses: &[String],
) {
    if allowed_licenses.is_empty() {
        return;
    }

    let license_allowed = manifest.license.as_ref().is_some_and(|actual_license| {
        let actual_license = actual_license.trim();
        !actual_license.is_empty()
            && allowed_licenses
                .iter()
                .any(|allowed_license| allowed_license.trim() == actual_license)
    });

    if !license_allowed {
        diagnostics.insert(PackagePolicyDiagnostic::new(
            PackagePolicyDiagnosticKind::LicensePolicyMismatch,
            "manifest.license",
            None,
            None,
            None,
            "package license does not satisfy represented production package policy",
        ));
    }
}

fn push_trust_policy_diagnostics(
    diagnostics: &mut BTreeSet<PackagePolicyDiagnostic>,
    manifest: &PackageManifest,
    minimum_trust: Option<TrustLevel>,
    deployment_profile: Option<DeploymentProfile>,
) {
    if let Some(minimum_trust) = minimum_trust {
        if !manifest.trust_level.satisfies(minimum_trust) {
            diagnostics.insert(PackagePolicyDiagnostic::new(
                PackagePolicyDiagnosticKind::MinimumTrustPolicyMismatch,
                "manifest.trust_level",
                None,
                None,
                None,
                "package trust level is below represented production package policy",
            ));
        }
    }

    if let Some(deployment_profile) = deployment_profile {
        if TrustGate::evaluate(manifest.trust_level, deployment_profile) == TrustGateVerdict::Deny {
            diagnostics.insert(PackagePolicyDiagnostic::new(
                PackagePolicyDiagnosticKind::ProfileTrustPolicyMismatch,
                "manifest.trust_level",
                None,
                None,
                None,
                "package trust level is denied by represented deployment profile policy",
            ));
        }
    }
}

fn push_publish_policy_diagnostics(
    diagnostics: &mut BTreeSet<PackagePolicyDiagnostic>,
    manifest: &PackageManifest,
) {
    for publish_issue in manifest.production_validation_issues() {
        diagnostics.insert(PackagePolicyDiagnostic::new(
            PackagePolicyDiagnosticKind::PublishPolicyMismatch,
            publish_issue.descriptor.path,
            publish_issue.descriptor.index,
            publish_issue.descriptor.duplicate_of,
            None,
            "package manifest does not satisfy production publish-readiness policy",
        ));
    }
}

fn first_matching_pattern_index(value: &str, patterns: &[String]) -> Option<usize> {
    patterns
        .iter()
        .position(|pattern| CapabilityPolicyEnforcer::matches(value, pattern))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_manifest() -> PackageManifest {
        PackageManifest::from_def(crate::manifest::PackageDef {
            name: "payments.stripe".to_string(),
            version: "1.2.3".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: Some("MIT".to_string()),
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            reproducible_evidence: None,
        })
    }

    fn export_declaration(name: &str) -> crate::export::ExportDeclaration {
        crate::export::ExportDeclaration {
            name: name.to_string(),
            signature: "Request -> Response".to_string(),
            effects: vec![],
            contracts: vec![],
            visibility: crate::export::ExportVisibility::Public,
            stability: crate::export::ExportStability::Stable,
            trust_state: None,
        }
    }

    fn import_declaration(source_package: &str) -> crate::import::ImportDeclaration {
        crate::import::ImportDeclaration {
            source_package: source_package.to_string(),
            items: vec!["charge".to_string()],
            version_constraint: Some("^1".to_string()),
        }
    }

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

    // ── package_policy_diagnostics_cover_denied_surface_and_redact ───────
    // Spec scenario: production governance reports denied capability/export/import
    // without copying manifest or policy values into diagnostics.
    #[test]
    fn package_policy_diagnostics_cover_denied_surface_and_redact() {
        let mut manifest = policy_manifest();
        manifest.required_capabilities = vec![
            "file.write:/secret/customer.csv".to_string(),
            "network.raw:token-bearing-value".to_string(),
        ];
        manifest.exported_capabilities = vec!["http.call:https://internal.example".to_string()];
        manifest.exports = vec![export_declaration("admin.delete_all")];
        manifest.imports = vec![import_declaration("internal.secrets")];

        let policy = PackageProductionPolicy {
            capability_policies: vec![
                CapabilityPolicy {
                    pattern: "file.write:*".to_string(),
                    verdict: CapabilityPolicyVerdict::Deny,
                },
                CapabilityPolicy {
                    pattern: "network.raw:*".to_string(),
                    verdict: CapabilityPolicyVerdict::Allow,
                },
                CapabilityPolicy {
                    pattern: "http.call:*".to_string(),
                    verdict: CapabilityPolicyVerdict::DenyUnlessApproved,
                },
            ],
            denied_exports: vec!["admin.delete_all".to_string()],
            denied_imports: vec!["internal.secrets".to_string()],
            ..PackageProductionPolicy::permissive()
        };

        let diagnostics = PackagePolicyEnforcer::diagnostics(&manifest, &policy);
        let shapes = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.kind.clone(),
                    diagnostic.descriptor.path.as_str(),
                    diagnostic.descriptor.index,
                    diagnostic.descriptor.policy_index,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            shapes,
            vec![
                (
                    PackagePolicyDiagnosticKind::DeniedCapability,
                    "manifest.exported_capabilities",
                    Some(0),
                    Some(2),
                ),
                (
                    PackagePolicyDiagnosticKind::DeniedCapability,
                    "manifest.required_capabilities",
                    Some(0),
                    Some(0),
                ),
                (
                    PackagePolicyDiagnosticKind::DeniedExport,
                    "manifest.exports.name",
                    Some(0),
                    Some(0),
                ),
                (
                    PackagePolicyDiagnosticKind::DeniedImport,
                    "manifest.imports.source_package",
                    Some(0),
                    Some(0),
                ),
            ]
        );

        let public_payload = format!("{diagnostics:?}");
        for raw_value in [
            "file.write:/secret/customer.csv",
            "network.raw:token-bearing-value",
            "http.call:https://internal.example",
            "admin.delete_all",
            "internal.secrets",
            "file.write:*",
            "http.call:*",
        ] {
            assert!(
                !public_payload.contains(raw_value),
                "diagnostic payload must redact {raw_value}"
            );
        }
        assert!(PackagePolicyEnforcer::check(&manifest, &policy).is_err());
    }

    // ── package_policy_diagnostics_report_represented_mismatches ─────────
    // Spec scenario: represented license, trust, profile, and publish policy
    // mismatches produce stable diagnostics without raw field values.
    #[test]
    fn package_policy_diagnostics_report_represented_mismatches() {
        let mut manifest = policy_manifest();
        manifest.version = "not-semver".to_string();
        manifest.trust_level = TrustLevel::Unverified;
        manifest.license = Some("GPL-3.0-only".to_string());

        let policy = PackageProductionPolicy {
            allowed_licenses: vec!["MIT".to_string(), "Apache-2.0".to_string()],
            minimum_trust: Some(TrustLevel::Assumed),
            deployment_profile: Some(DeploymentProfile::Prod),
            require_publish_ready: true,
            ..PackageProductionPolicy::permissive()
        };

        let diagnostics = PackagePolicyEnforcer::diagnostics(&manifest, &policy);
        let kinds = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.kind.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                PackagePolicyDiagnosticKind::LicensePolicyMismatch,
                PackagePolicyDiagnosticKind::MinimumTrustPolicyMismatch,
                PackagePolicyDiagnosticKind::ProfileTrustPolicyMismatch,
                PackagePolicyDiagnosticKind::PublishPolicyMismatch,
            ]
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == PackagePolicyDiagnosticKind::PublishPolicyMismatch
                && diagnostic.descriptor.path == "manifest.version"
        }));

        let public_payload = format!("{diagnostics:?}");
        assert!(!public_payload.contains("GPL-3.0-only"));
        assert!(!public_payload.contains("not-semver"));
        assert!(!public_payload.contains("MIT"));
        assert!(!public_payload.contains("Apache-2.0"));
    }

    // ── package_policy_diagnostics_are_deterministic_and_deduplicated ─────
    // TRIANGULATE: repeated policy rules do not create duplicate diagnostics,
    // and repeated calls return the same stable order.
    #[test]
    fn package_policy_diagnostics_are_deterministic_and_deduplicated() {
        let mut manifest = policy_manifest();
        manifest.required_capabilities = vec!["file.write:LocalDisk".to_string()];
        manifest.exports = vec![export_declaration("admin.delete_all")];

        let policy = PackageProductionPolicy {
            capability_policies: vec![
                CapabilityPolicy {
                    pattern: "file.write:*".to_string(),
                    verdict: CapabilityPolicyVerdict::Deny,
                },
                CapabilityPolicy {
                    pattern: "file.write:*".to_string(),
                    verdict: CapabilityPolicyVerdict::Deny,
                },
            ],
            denied_exports: vec![
                "admin.delete_all".to_string(),
                "admin.delete_all".to_string(),
            ],
            ..PackageProductionPolicy::permissive()
        };

        let first = PackagePolicyEnforcer::diagnostics(&manifest, &policy);
        let second = PackagePolicyEnforcer::diagnostics(&manifest, &policy);
        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == PackagePolicyDiagnosticKind::DeniedCapability
                })
                .count(),
            1
        );
        assert_eq!(
            first
                .iter()
                .filter(|diagnostic| diagnostic.kind == PackagePolicyDiagnosticKind::DeniedExport)
                .count(),
            1
        );
    }

    // ── B3: UnsafeSurfacePolicy ────────────────────────────────────────────

    fn make_surface_entry(name: &str) -> crate::surface::UnsafeSurfaceEntry {
        crate::surface::UnsafeSurfaceEntry {
            kind: "ffi".to_string(),
            name: name.to_string(),
            description: "test".to_string(),
        }
    }

    // Spec PKG-UNSAFE-1: unapproved surface entry → violation
    #[test]
    fn unsafe_surface_unapproved_entry_produces_violation() {
        let surface = vec![make_surface_entry("fn.native_hash")];
        let violations = UnsafeSurfacePolicyEnforcer::check(&surface, &[]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0], "fn.native_hash");
    }

    // Spec PKG-UNSAFE-1: approved surface entry → no violation
    #[test]
    fn unsafe_surface_approved_entry_produces_no_violation() {
        let surface = vec![make_surface_entry("fn.native_hash")];
        let approvals = vec![UnsafeSurfaceApproval {
            surface_id: "fn.native_hash".to_string(),
        }];
        let violations = UnsafeSurfacePolicyEnforcer::check(&surface, &approvals);
        assert!(violations.is_empty());
    }

    // Spec PKG-UNSAFE-1: empty surface → no violation
    #[test]
    fn unsafe_surface_empty_surface_no_violation() {
        let violations = UnsafeSurfacePolicyEnforcer::check(&[], &[]);
        assert!(violations.is_empty());
    }

    // Spec PKG-UNSAFE-1: multiple entries mixed → only unapproved returned
    #[test]
    fn unsafe_surface_mixed_entries_only_unapproved_returned() {
        let surface = vec![
            make_surface_entry("fn.approved_fn"),
            make_surface_entry("fn.unapproved_fn"),
        ];
        let approvals = vec![UnsafeSurfaceApproval {
            surface_id: "fn.approved_fn".to_string(),
        }];
        let violations = UnsafeSurfacePolicyEnforcer::check(&surface, &approvals);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0], "fn.unapproved_fn");
    }
}
