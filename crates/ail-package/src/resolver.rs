// ── ail-package::resolver ─────────────────────────────────────────────────
//
// Dependency resolution with trust, advisory, and yank checks.
//
// # Design decisions
//
// - `DependencyResolver` is a stateless unit struct.  All inputs (registry,
//   advisories, yank records) are passed per-call.
// - Version matching is exact-string for this implementation.  Semver range
//   evaluation (`^1.2`, `>=1.0`) is an open design question (packages.md).
// - Resolution order: NotFound → Yanked → Advisory → TrustViolation → Ok.
//   This ensures the most informative error is returned when multiple issues
//   exist simultaneously.

use crate::advisory::SecurityAdvisory;
use crate::manifest::PackageManifest;
use crate::registry::PackageRegistry;
use crate::trust::TrustLevel;
use crate::yank::YankRecord;

// ── DependencySpec ────────────────────────────────────────────────────────

/// A declared dependency with version and trust constraints.
///
/// `version` is an exact version string for this implementation.
/// `min_trust` is the minimum `TrustLevel` required for resolution to succeed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencySpec {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Required version string (exact match, e.g., `"1.2.0"`).
    pub version: String,
    /// Minimum trust level required (e.g., `TrustLevel::Assumed`).
    pub min_trust: TrustLevel,
}

// ── ResolverError ─────────────────────────────────────────────────────────

/// Errors returned by [`DependencyResolver::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolverError {
    /// No package with the requested name and version was found in the registry.
    NotFound {
        /// Package name that was not found.
        name: String,
        /// Version that was not found.
        version: String,
    },
    /// The package was yanked and cannot be used for new resolution.
    Yanked {
        /// Human-readable reason for the yank.
        reason: String,
    },
    /// The package matches a security advisory.
    Advisory {
        /// Advisory identifier.
        id: String,
        /// Advisory severity.
        severity: crate::advisory::AdvisorySeverity,
    },
    /// The package's trust level does not meet the minimum required.
    TrustViolation {
        /// The actual trust level of the resolved package.
        actual: TrustLevel,
        /// The minimum trust level required by the dependency spec.
        required: TrustLevel,
    },
}

impl std::fmt::Display for ResolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolverError::NotFound { name, version } => {
                write!(f, "package {name} {version} not found in registry")
            }
            ResolverError::Yanked { reason } => {
                write!(f, "package is yanked: {reason}")
            }
            ResolverError::Advisory { id, severity } => {
                write!(f, "security advisory {id} ({severity}) affects this package")
            }
            ResolverError::TrustViolation { actual, required } => {
                write!(
                    f,
                    "trust violation: package has trust {actual} but {required} is required"
                )
            }
        }
    }
}

impl std::error::Error for ResolverError {}

// ── DependencyResolver ────────────────────────────────────────────────────

/// Stateless dependency resolver that checks yanks, advisories, and trust.
pub struct DependencyResolver;

impl DependencyResolver {
    /// Resolve a `DependencySpec` against the given registry, advisories, and
    /// yank records.
    ///
    /// Resolution order:
    /// 1. If no matching package is found → `ResolverError::NotFound`
    /// 2. If the package is yanked → `ResolverError::Yanked`
    /// 3. If a matching advisory exists → `ResolverError::Advisory`
    /// 4. If trust level is insufficient → `ResolverError::TrustViolation`
    /// 5. Otherwise → `Ok(&PackageManifest)`
    ///
    /// # Errors
    ///
    /// Returns the first `ResolverError` encountered per the resolution order
    /// above.
    pub fn resolve<'a>(
        spec: &DependencySpec,
        registry: &'a PackageRegistry,
        advisories: &[SecurityAdvisory],
        yanks: &[YankRecord],
    ) -> Result<&'a PackageManifest, ResolverError> {
        // Step 1: lookup
        let manifest = registry
            .lookup_by_name_version(&spec.name, &spec.version)
            .ok_or_else(|| ResolverError::NotFound {
                name: spec.name.clone(),
                version: spec.version.clone(),
            })?;

        // Step 2: yank check
        if let Some(yank) = yanks
            .iter()
            .find(|y| y.name == spec.name && y.version == spec.version)
        {
            return Err(ResolverError::Yanked {
                reason: yank.reason.clone(),
            });
        }

        // Step 3: advisory check
        if let Some(adv) =
            crate::advisory::AdvisoryChecker::first_match(&spec.name, &spec.version, advisories)
        {
            return Err(ResolverError::Advisory {
                id: adv.id.clone(),
                severity: adv.severity,
            });
        }

        // Step 4: trust check
        if !manifest.trust_level.satisfies(spec.min_trust) {
            return Err(ResolverError::TrustViolation {
                actual: manifest.trust_level,
                required: spec.min_trust,
            });
        }

        Ok(manifest)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisory::{AdvisorySeverity, SecurityAdvisory};
    use crate::manifest::{PackageDef, PackageManifest};
    use crate::registry::PackageRegistry;
    use crate::trust::TrustLevel;
    use crate::yank::YankRecord;

    fn make_manifest(name: &str, version: &str, trust: TrustLevel) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: version.to_string(),
            trust_level: trust,
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
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
        })
    }

    fn spec(name: &str, version: &str, min_trust: TrustLevel) -> DependencySpec {
        DependencySpec {
            name: name.to_string(),
            version: version.to_string(),
            min_trust,
        }
    }

    // ── RED: resolve_returns_manifest_for_valid_spec ──────────────────────
    // Spec: REQ-RES-3 — successful resolution returns the manifest
    //   GIVEN a registry with "payments.stripe" v1.2.0 (Verified)
    //   WHEN resolve is called with min_trust: Assumed, no advisories, no yanks
    //   THEN returns Ok pointing to the manifest
    #[test]
    fn resolve_returns_manifest_for_valid_spec() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("payments.stripe", "1.2.0", TrustLevel::Verified));

        let result = DependencyResolver::resolve(
            &spec("payments.stripe", "1.2.0", TrustLevel::Assumed),
            &reg,
            &[],
            &[],
        );

        assert!(result.is_ok(), "valid spec must resolve successfully");
        assert_eq!(result.unwrap().name, "payments.stripe");
    }

    // ── RED: resolve_returns_not_found_for_unknown_package ────────────────
    // Spec: REQ-RES-3 — NotFound when package is absent
    //   GIVEN an empty registry
    //   WHEN resolve is called for any package
    //   THEN returns ResolverError::NotFound
    #[test]
    fn resolve_returns_not_found_for_unknown_package() {
        let reg = PackageRegistry::new();
        let result = DependencyResolver::resolve(
            &spec("unknown.pkg", "1.0.0", TrustLevel::Unverified),
            &reg,
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(ResolverError::NotFound {
                name: "unknown.pkg".to_string(),
                version: "1.0.0".to_string(),
            })
        );
    }

    // ── RED: resolve_returns_yanked_for_yanked_package ────────────────────
    // Spec: REQ-RES-4 / REQ-YANK-4 — Yanked packages are blocked in resolver
    //   GIVEN a registry with "pkg" v1.0.0 and a yank record for it
    //   WHEN resolve is called
    //   THEN returns ResolverError::Yanked
    #[test]
    fn resolve_returns_yanked_for_yanked_package() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.0.0", TrustLevel::Verified));
        let yanks = vec![YankRecord {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            reason: "security regression".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("pkg", "1.0.0", TrustLevel::Unverified),
            &reg,
            &[],
            &yanks,
        );
        assert!(
            matches!(result, Err(ResolverError::Yanked { .. })),
            "yanked package must return ResolverError::Yanked"
        );
    }

    // ── RED: resolve_returns_advisory_for_affected_package ────────────────
    // Spec: REQ-RES-4 — Advisory check blocks resolution
    //   GIVEN a registry with "stripe" v1.0.0 and a matching advisory
    //   WHEN resolve is called
    //   THEN returns ResolverError::Advisory
    #[test]
    fn resolve_returns_advisory_for_affected_package() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("stripe", "1.0.0", TrustLevel::Verified));
        let advisories = vec![SecurityAdvisory {
            id: "adv_007".to_string(),
            package: "stripe".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::Critical,
            reason: "bug".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("stripe", "1.0.0", TrustLevel::Unverified),
            &reg,
            &advisories,
            &[],
        );
        assert!(
            matches!(result, Err(ResolverError::Advisory { id, .. }) if id == "adv_007"),
            "advisory match must return ResolverError::Advisory"
        );
    }

    // ── RED: resolve_returns_trust_violation_for_low_trust ────────────────
    // Spec: REQ-RES-4 — TrustViolation when package trust < required
    //   GIVEN a registry with "pkg" v2.0.0 at Unverified trust
    //   WHEN resolve is called requiring Assumed trust
    //   THEN returns ResolverError::TrustViolation
    #[test]
    fn resolve_returns_trust_violation_for_low_trust() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "2.0.0", TrustLevel::Unverified));

        let result = DependencyResolver::resolve(
            &spec("pkg", "2.0.0", TrustLevel::Assumed),
            &reg,
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(ResolverError::TrustViolation {
                actual: TrustLevel::Unverified,
                required: TrustLevel::Assumed,
            })
        );
    }

    // ── RED: yank_takes_precedence_over_advisory ──────────────────────────
    // TRIANGULATE: resolution order — Yanked before Advisory
    //   GIVEN a package that is both yanked AND has an advisory
    //   WHEN resolve is called
    //   THEN returns ResolverError::Yanked (not Advisory)
    #[test]
    fn yank_takes_precedence_over_advisory() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.0.0", TrustLevel::Verified));
        let yanks = vec![YankRecord {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            reason: "yanked".to_string(),
        }];
        let advisories = vec![SecurityAdvisory {
            id: "adv_x".to_string(),
            package: "pkg".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::High,
            reason: "bug".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("pkg", "1.0.0", TrustLevel::Unverified),
            &reg,
            &advisories,
            &yanks,
        );
        assert!(
            matches!(result, Err(ResolverError::Yanked { .. })),
            "Yanked must take precedence over Advisory"
        );
    }
}
