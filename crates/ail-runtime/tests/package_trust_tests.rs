// ── ail-runtime::package_trust_tests ─────────────────────────────────────
//
// Tasks 3.2 + 3.3: Package trust gate preflight tests.
//
// Spec scenarios covered (Domain: package-trust-gate):
//  - Package below minimum trust fails with PackageTrustViolation.
//  - Package at/above minimum trust passes.
//  - None min_package_trust skips gate entirely.
//  - TrustLevel::Unsafe without approval fails UnsafePackageNotApproved.
//  - Import != grant: module imports package with required_capabilities but
//    RuntimeProfile has no CapabilityGrant → CapabilityDenied.

use ail_package::manifest::PackageDef;
use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, PackageManifest, PreflightFailure,
    ResourceLimits, RuntimeError, RuntimeHost, RuntimeProfile, TrustLevel, blake3_hex_of,
};

// ── helpers ───────────────────────────────────────────────────────────────

fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn empty_manifest(module: &str) -> CapabilityManifest {
    CapabilityManifest {
        module: module.to_string(),
        requires: vec![],
    }
}

fn matching_profile(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
}

fn make_package(name: &str, trust: TrustLevel) -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        trust_level: trust,
        required_capabilities: vec!["payment.charge".to_string()],
        exported_capabilities: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
    })
}

// ── Spec scenario: Package below minimum trust fails preflight ────────────
//
// GIVEN a RuntimeProfile with min_package_trust: Assumed
// AND the module declares a dependency with trust_level: Unverified
// WHEN preflight runs
// THEN it returns Err(RuntimeError::Preflight(PreflightFailure::PackageTrustViolation))
#[test]
fn package_below_min_trust_fails_preflight() {
    let wasm = minimal_wasm();
    let cap_manifest = empty_manifest("test-module");
    let profile = matching_profile(&wasm, &cap_manifest).with_package_trust(TrustLevel::Assumed);

    let unverified_pkg = make_package("payments.stripe", TrustLevel::Unverified);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate_with_packages(
        &wasm,
        &cap_manifest,
        &profile,
        &[unverified_pkg],
    );

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::PackageTrustViolation {
            package,
            required,
            actual,
        })) => {
            assert_eq!(package, "payments.stripe");
            assert_eq!(required, TrustLevel::Assumed);
            assert_eq!(actual, TrustLevel::Unverified);
        }
        other => panic!("expected PackageTrustViolation, got {other:?}"),
    }

    // Audit log must record a failure.
    let log = host.audit_log();
    assert_eq!(log.len(), 1);
    assert!(!log.events()[0].is_passed());
}

// ── Spec scenario: Package at or above minimum trust passes ───────────────
//
// GIVEN a RuntimeProfile with min_package_trust: Assumed
// AND all declared packages have trust_level: Assumed or Verified
// WHEN preflight runs (and other checks pass)
// THEN preflight PASSES
#[test]
fn package_at_min_trust_passes_preflight() {
    let wasm = minimal_wasm();
    let cap_manifest = empty_manifest("test-module");
    let profile = matching_profile(&wasm, &cap_manifest).with_package_trust(TrustLevel::Assumed);

    let assumed_pkg = make_package("payments.stripe", TrustLevel::Assumed);
    let verified_pkg = make_package("core.utils", TrustLevel::Verified);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate_with_packages(
        &wasm,
        &cap_manifest,
        &profile,
        &[assumed_pkg, verified_pkg],
    );

    assert!(
        result.is_ok(),
        "packages at/above min trust must pass, got {result:?}"
    );

    let log = host.audit_log();
    assert!(log.events()[0].is_passed());
}

// ── Spec scenario: None min_package_trust skips package gate ─────────────
//
// GIVEN a RuntimeProfile where min_package_trust is None
// WHEN preflight runs with any set of package manifests
// THEN no package trust preflight failure is emitted
#[test]
fn none_min_trust_skips_gate() {
    let wasm = minimal_wasm();
    let cap_manifest = empty_manifest("test-module");
    // No with_package_trust() call → min_package_trust = None.
    let profile = matching_profile(&wasm, &cap_manifest);

    // Even Unsafe packages should not trigger the gate when None.
    // (Note: Stage 0 is skipped when package_manifests is empty; to test
    //  that None skips the check even with manifests we pass them explicitly.)
    let unverified_pkg = make_package("risky.lib", TrustLevel::Unverified);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate_with_packages(
        &wasm,
        &cap_manifest,
        &profile,
        &[unverified_pkg],
    );

    assert!(
        result.is_ok(),
        "None min_package_trust must skip gate for any trust level, got {result:?}"
    );
}

// ── Spec scenario: Unsafe package without approval is blocked ─────────────
//
// GIVEN a package with trust_level: Unsafe
// AND the RuntimeProfile has no UnsafePackageApproval for that package
// WHEN preflight runs
// THEN it fails with PreflightFailure::UnsafePackageNotApproved
#[test]
fn unsafe_package_blocked_without_approval() {
    let wasm = minimal_wasm();
    let cap_manifest = empty_manifest("test-module");
    let profile = matching_profile(&wasm, &cap_manifest).with_package_trust(TrustLevel::Unverified);

    let def = PackageDef {
        name: "ffi.dangerous".to_string(),
        version: "0.1.0".to_string(),
        trust_level: TrustLevel::Unsafe,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![],
        // Unsafe packages require a surface declaration; provide one.
        unsafe_surface: vec![ail_package::surface::UnsafeSurfaceEntry {
            kind: "ffi".to_string(),
            name: "libc::malloc".to_string(),
            description: "raw allocation".to_string(),
        }],
        artifact_hashes: vec![],
        build_env_hash: None,
    };
    let pkg = PackageManifest::from_def(def);

    let mut host = RuntimeHost::new();
    let result =
        host.validate_and_instantiate_with_packages(&wasm, &cap_manifest, &profile, &[pkg]);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::UnsafePackageNotApproved {
            package,
        })) => {
            assert_eq!(package, "ffi.dangerous");
        }
        other => panic!("expected UnsafePackageNotApproved, got {other:?}"),
    }
}

// ── Spec scenario: import != grant ──────────────────────────────────────
//
// GIVEN a module that imports package `payments.stripe` (trust: Assumed)
// AND the RuntimeProfile has no explicit CapabilityGrant for `payment.charge`
// WHEN preflight runs
// THEN preflight FAILS with PreflightFailure::CapabilityDenied
//
// This test proves that declaring a package dependency does NOT implicitly
// grant the capabilities listed in the package's `required_capabilities`.
#[test]
fn import_does_not_grant_capabilities() {
    let wasm = minimal_wasm();

    // The capability manifest declares that this module REQUIRES payment.charge.
    let required_cap = CapabilityId::new("payment.charge");
    let cap_manifest = CapabilityManifest {
        module: "billing-module".to_string(),
        requires: vec![required_cap.clone()],
    };

    // Profile has NO grants — even though the package declares payment.charge
    // as a required_capability, the profile must not auto-grant it.
    let profile = matching_profile(&wasm, &cap_manifest).with_package_trust(TrustLevel::Unverified);

    let stripe_pkg = make_package("payments.stripe", TrustLevel::Assumed);

    let mut host = RuntimeHost::new();
    let result =
        host.validate_and_instantiate_with_packages(&wasm, &cap_manifest, &profile, &[stripe_pkg]);

    // Must fail with CapabilityDenied — NOT with PackageTrustViolation.
    // The package passed the trust gate; the capability gate denied it.
    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied { denied })) => {
            assert!(
                denied.contains(&required_cap),
                "payment.charge must appear in the denied list"
            );
        }
        other => panic!("expected CapabilityDenied (import != grant), got {other:?}"),
    }
}

// TRIANGULATE: explicit grant permits the capability even with package trust gate.
#[test]
fn explicit_grant_permits_capability_with_package_present() {
    let wasm = minimal_wasm();
    let required_cap = CapabilityId::new("payment.charge");
    let cap_manifest = CapabilityManifest {
        module: "billing-module".to_string(),
        requires: vec![required_cap.clone()],
    };

    let wasm_hash = blake3_hex_of(&wasm);
    let manifest_hash = cap_manifest.blake3_hex().expect("hash must succeed");

    // This time the profile DOES grant payment.charge.
    let grant = CapabilityGrant {
        module: "billing-module".to_string(),
        capability: required_cap,
    };
    let profile = RuntimeProfile::new(
        "billing-prod".to_string(),
        wasm_hash,
        "a".repeat(64),
        manifest_hash,
        vec![grant],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
    .with_package_trust(TrustLevel::Unverified);

    let stripe_pkg = make_package("payments.stripe", TrustLevel::Assumed);

    let mut host = RuntimeHost::new();
    let result =
        host.validate_and_instantiate_with_packages(&wasm, &cap_manifest, &profile, &[stripe_pkg]);

    assert!(
        result.is_ok(),
        "explicit grant must permit capability, got {result:?}"
    );
    assert!(host.audit_log().events()[0].is_passed());
}

// TRIANGULATE: validate_and_instantiate (no packages) still works unchanged.
#[test]
fn validate_and_instantiate_without_packages_unchanged() {
    let wasm = minimal_wasm();
    let cap_manifest = empty_manifest("no-pkg-module");
    let profile = matching_profile(&wasm, &cap_manifest);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &cap_manifest, &profile);

    assert!(
        result.is_ok(),
        "existing API without packages must still work"
    );
}
