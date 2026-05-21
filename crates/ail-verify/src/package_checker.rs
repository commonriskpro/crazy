// ── ail-verify::package_checker ───────────────────────────────────────────
//
// `PackageTrustChecker` — verify package trust levels against a named profile.
//
// # Policy matrix
//
// | Profile | Minimum trust tier | Unsafe allowed  |
// |---------|-------------------|-----------------|
// | prod    | Assumed           | No (blocking)   |
// | staging | Assumed           | No (blocking)   |
// | dev     | Unverified        | No (blocking)   |
// | *       | Unverified        | No (blocking)   |
//
// Blocking entries prevent deployment; non-blocking entries are advisory.
//
// # Design notes
//
// `PackageTrustChecker` is stateless; call `check` with a manifest slice and a
// profile name to receive a `Vec<VerificationEntry>`.  Each entry covers one
// package.  The `scope` field is set to `"package:<name>@<version>"`.
//
// `import != grant`: this checker verifies the package's trust tier; it does
// NOT grant any capability to the importing module.

use ail_package::manifest::PackageManifest;
use ail_package::trust::TrustLevel;

use crate::report::{VerificationEntry, VerificationState};

// ── profile policy ────────────────────────────────────────────────────────

/// Resolve the minimum trust tier required for a profile name.
///
/// `prod` and `staging` require at least `Assumed`.
/// All other profiles (including `dev`) require at least `Unverified`.
fn minimum_trust_for_profile(profile_name: &str) -> TrustLevel {
    match profile_name {
        "prod" | "staging" => TrustLevel::Assumed,
        _ => TrustLevel::Unverified,
    }
}

// ── PackageTrustChecker ───────────────────────────────────────────────────

/// Pure, stateless package trust checker.
///
/// Call [`PackageTrustChecker::check`] with a `PackageManifest` slice and a
/// profile name string to receive a list of `VerificationEntry` items, one
/// per package.
pub struct PackageTrustChecker;

impl PackageTrustChecker {
    /// Check each manifest's trust tier against the named profile's policy.
    ///
    /// Returns one [`VerificationEntry`] per manifest.  The `scope` is
    /// `"package:<name>@<version>"`.  `blocking` is `true` when the entry
    /// indicates the package should NOT proceed to the next phase.
    ///
    /// # Policy
    ///
    /// - `TrustLevel::Unsafe` is always blocking regardless of profile.
    /// - Packages below the profile's minimum tier are blocking.
    /// - `TrustLevel::Verified` is always non-blocking (`Proven`).
    /// - `TrustLevel::Assumed` in a permissive profile is non-blocking (`Assumed`).
    pub fn check(manifests: &[PackageManifest], profile_name: &str) -> Vec<VerificationEntry> {
        let min_trust = minimum_trust_for_profile(profile_name);
        manifests
            .iter()
            .map(|m| Self::check_one(m, profile_name, min_trust))
            .collect()
    }

    fn check_one(
        manifest: &PackageManifest,
        profile_name: &str,
        min_trust: TrustLevel,
    ) -> VerificationEntry {
        let scope = format!("package:{}@{}", manifest.name, manifest.version);
        let trust = manifest.trust_level;

        // Unsafe packages are always blocking.
        if trust == TrustLevel::Unsafe {
            return VerificationEntry {
                claim: format!("package-trust[{profile_name}]"),
                state: VerificationState::Unsafe,
                scope,
                evidence: Some(
                    "package has TrustLevel::Unsafe; explicit approval required".to_string(),
                ),
            };
        }

        if trust.satisfies(min_trust) {
            // At or above the minimum — determine state by actual level.
            let state = match trust {
                TrustLevel::Verified => VerificationState::Proven,
                TrustLevel::Assumed => VerificationState::Assumed,
                TrustLevel::Unverified => VerificationState::Unverified,
                TrustLevel::Unsafe => unreachable!("handled above"),
            };
            VerificationEntry {
                claim: format!("package-trust[{profile_name}]"),
                state,
                scope,
                evidence: None,
            }
        } else {
            // Below minimum — Unverified state, blocking.
            VerificationEntry {
                claim: format!("package-trust[{profile_name}]"),
                state: VerificationState::Unverified,
                scope,
                evidence: Some(format!(
                    "package trust level `{trust}` does not meet profile minimum `{min_trust}`"
                )),
            }
        }
    }
}

// ── blocking helper ───────────────────────────────────────────────────────

impl VerificationEntry {
    /// Return `true` if this entry should block progression to the next phase.
    ///
    /// For package trust entries, blocking is indicated by the presence of an
    /// `evidence` string (set only when a policy violation was detected).
    /// An `Unsafe` state is always blocking regardless of evidence.
    /// `Failed` is always blocking.
    /// `Proven`, `Assumed`, `RuntimeChecked` are never blocking.
    /// `Unverified` without evidence means the package met the profile minimum
    /// and is non-blocking (advisory); with evidence it is blocking.
    pub fn is_blocking(&self) -> bool {
        match self.state {
            VerificationState::Unsafe | VerificationState::Failed => true,
            VerificationState::Unverified => self.evidence.is_some(),
            _ => false,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_package::manifest::{PackageDef, PackageManifest};
    use ail_package::trust::TrustLevel;

    fn make_manifest(name: &str, trust: TrustLevel) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: "1.0.0".to_string(),
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

    // ── Spec scenario: Unverified package blocked in prod profile ─────────
    // GIVEN a PackageManifest with trust_level: Unverified
    // AND verification runs with profile `prod`
    // WHEN PackageTrustChecker::check is called
    // THEN the report contains an entry with state `unverified` and blocking: true
    #[test]
    fn unverified_blocked_in_prod() {
        let m = make_manifest("payments.stripe", TrustLevel::Unverified);
        let entries = PackageTrustChecker::check(&[m], "prod");
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.state, VerificationState::Unverified);
        assert!(e.is_blocking(), "unverified must be blocking in prod");
        assert_eq!(e.scope, "package:payments.stripe@1.0.0");
    }

    // ── Spec scenario: Assumed package passes in dev profile ──────────────
    // GIVEN a PackageManifest with trust_level: Assumed
    // AND verification runs with profile `dev`
    // WHEN PackageTrustChecker::check is called
    // THEN the report contains an entry with state `assumed` and blocking: false
    #[test]
    fn assumed_non_blocking_in_dev() {
        let m = make_manifest("infra.logging", TrustLevel::Assumed);
        let entries = PackageTrustChecker::check(&[m], "dev");
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.state, VerificationState::Assumed);
        assert!(!e.is_blocking(), "assumed must be non-blocking in dev");
    }

    // ── Spec scenario: Verified package always passes ─────────────────────
    // GIVEN a PackageManifest with trust_level: Verified
    // AND verification runs with any profile
    // WHEN PackageTrustChecker::check is called
    // THEN the report entry state is `proven` and NOT blocking
    #[test]
    fn verified_always_passes_all_profiles() {
        for profile in &["prod", "staging", "dev", "local"] {
            let m = make_manifest("core.utils", TrustLevel::Verified);
            let entries = PackageTrustChecker::check(&[m], profile);
            let e = &entries[0];
            assert_eq!(
                e.state,
                VerificationState::Proven,
                "verified must be Proven in {profile}"
            );
            assert!(!e.is_blocking(), "verified must not block in {profile}");
        }
    }

    // TRIANGULATE: Unsafe is always blocking regardless of profile.
    #[test]
    fn unsafe_always_blocking() {
        for profile in &["prod", "dev", "local"] {
            let m = make_manifest("sketchy.ffi", TrustLevel::Unsafe);
            let entries = PackageTrustChecker::check(&[m], profile);
            let e = &entries[0];
            assert_eq!(e.state, VerificationState::Unsafe);
            assert!(e.is_blocking(), "Unsafe must be blocking in {profile}");
        }
    }

    // TRIANGULATE: Assumed is blocked in prod (below minimum Assumed? No —
    // prod minimum IS Assumed, so Assumed should PASS in prod).
    #[test]
    fn assumed_passes_in_prod() {
        let m = make_manifest("payments.stripe", TrustLevel::Assumed);
        let entries = PackageTrustChecker::check(&[m], "prod");
        let e = &entries[0];
        assert_eq!(e.state, VerificationState::Assumed);
        assert!(
            !e.is_blocking(),
            "Assumed meets prod minimum (Assumed); must not block"
        );
    }

    // TRIANGULATE: Unverified is non-blocking in dev (meets dev minimum).
    #[test]
    fn unverified_non_blocking_in_dev() {
        let m = make_manifest("experimental.lib", TrustLevel::Unverified);
        let entries = PackageTrustChecker::check(&[m], "dev");
        let e = &entries[0];
        assert_eq!(e.state, VerificationState::Unverified);
        assert!(
            !e.is_blocking(),
            "Unverified meets dev minimum; must not block"
        );
    }

    // TRIANGULATE: empty manifest slice returns empty entries.
    #[test]
    fn empty_manifests_returns_empty() {
        let entries = PackageTrustChecker::check(&[], "prod");
        assert!(entries.is_empty());
    }

    // TRIANGULATE: scope format is "package:<name>@<version>".
    #[test]
    fn scope_format_is_correct() {
        let m = make_manifest("acme.payments", TrustLevel::Verified);
        let entries = PackageTrustChecker::check(&[m], "prod");
        assert_eq!(entries[0].scope, "package:acme.payments@1.0.0");
    }
}
