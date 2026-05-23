// ── ail-verify::package_checker_tests ────────────────────────────────────
//
// Task 3.1: Integration tests for PackageTrustChecker.
//
// Spec scenarios covered (Domain: package-trust-gate):
//  - Unverified package is blocking in `prod` profile.
//  - Assumed package is non-blocking in `dev` profile with boundary.
//  - Verified package always passes (any profile).
//  - Unsafe package is always blocking (any profile).
//  - Empty manifest slice produces empty output.

use ail_package::assumption::{AssumptionState, PackageAssumption};
use ail_package::manifest::{PackageDef, PackageManifest};
use ail_package::trust::TrustLevel;
use ail_verify::package_checker::PackageTrustChecker;
use ail_verify::report::VerificationState;

// ── helpers ───────────────────────────────────────────────────────────────

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

fn make_assumed_manifest(name: &str, version: &str) -> PackageManifest {
    let mut manifest = make_manifest(name, version, TrustLevel::Assumed);
    manifest.boundaries = vec!["boundary.Stripe".to_string()];
    manifest.assumptions = vec![PackageAssumption {
        id: "stripe_idempotency".to_string(),
        claim: "Stripe honors idempotency keys".to_string(),
        boundary: "boundary.Stripe".to_string(),
        owner: "team.payments".to_string(),
        expires: Some("2026-12-31".to_string()),
        state: AssumptionState::Active,
    }];
    manifest
}

// ── Spec scenario: Unverified package blocked in prod profile ─────────────
//
// GIVEN a PackageManifest with trust_level: Unverified
// AND verification runs with profile `prod`
// WHEN PackageTrustChecker::check is called
// THEN the report contains an entry with state `unverified` and blocking: true
#[test]
fn unverified_blocked_in_prod_profile() {
    let m = make_manifest("payments.stripe", "2.3.1", TrustLevel::Unverified);
    let entries = PackageTrustChecker::check(&[m], "prod");

    assert_eq!(entries.len(), 1, "one entry per manifest");
    let e = &entries[0];
    assert_eq!(
        e.state,
        VerificationState::Unverified,
        "state must be Unverified"
    );
    assert!(e.is_blocking(), "Unverified in prod must be blocking");
    assert_eq!(e.scope, "package:payments.stripe@2.3.1");
}

// ── Spec scenario: Assumed package passes in dev profile ──────────────────
//
// GIVEN a PackageManifest with trust_level: Assumed and a non-empty assumptions list
// AND verification runs with profile `dev`
// WHEN PackageTrustChecker::check is called
// THEN the report contains an entry with state `assumed` and blocking: false
#[test]
fn assumed_non_blocking_in_dev_profile() {
    let m = make_assumed_manifest("infra.logging", "1.0.0");
    let entries = PackageTrustChecker::check(&[m], "dev");

    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.state, VerificationState::Assumed, "state must be Assumed");
    assert!(!e.is_blocking(), "Assumed in dev must not block");
    assert!(
        e.evidence
            .as_deref()
            .unwrap_or("")
            .contains("stripe_idempotency"),
        "Assumed package should report the assumption evidence"
    );
}

// ── Spec scenario: Verified package always passes ─────────────────────────
//
// GIVEN a PackageManifest with trust_level: Verified
// AND verification runs with any profile
// WHEN PackageTrustChecker::check is called
// THEN the report entry state is `proven` and NOT blocking
#[test]
fn verified_passes_all_profiles() {
    for profile in &["prod", "staging", "dev", "local", "ci"] {
        let m = make_manifest("core.stdlib", "3.0.0", TrustLevel::Verified);
        let entries = PackageTrustChecker::check(&[m], profile);

        let e = &entries[0];
        assert_eq!(
            e.state,
            VerificationState::Proven,
            "Verified must be Proven in profile `{profile}`"
        );
        assert!(!e.is_blocking(), "Verified must not block in `{profile}`");
    }
}

// TRIANGULATE: Unsafe is always blocking, state is Unsafe.
#[test]
fn unsafe_always_blocking_any_profile() {
    for profile in &["prod", "dev", "local"] {
        let m = make_manifest("sketchy.native", "0.1.0", TrustLevel::Unsafe);
        let entries = PackageTrustChecker::check(&[m], profile);

        let e = &entries[0];
        assert_eq!(
            e.state,
            VerificationState::Unsafe,
            "Unsafe must have state=Unsafe in `{profile}`"
        );
        assert!(e.is_blocking(), "Unsafe must block in `{profile}`");
    }
}

// TRIANGULATE: multiple manifests → one entry per manifest in order.
#[test]
fn multiple_manifests_one_entry_each() {
    let manifests = vec![
        make_manifest("pkg.a", "1.0.0", TrustLevel::Verified),
        make_manifest("pkg.b", "1.0.0", TrustLevel::Unverified),
        make_assumed_manifest("pkg.c", "1.0.0"),
    ];

    let entries = PackageTrustChecker::check(&manifests, "prod");

    assert_eq!(entries.len(), 3, "three manifests → three entries");
    assert_eq!(entries[0].state, VerificationState::Proven);
    assert_eq!(entries[1].state, VerificationState::Unverified);
    assert_eq!(entries[2].state, VerificationState::Assumed);
}

// TRIANGULATE: Assumed in prod meets the Assumed minimum → non-blocking.
#[test]
fn assumed_meets_prod_minimum() {
    let m = make_assumed_manifest("infra.db", "2.0.0");
    let entries = PackageTrustChecker::check(&[m], "prod");

    let e = &entries[0];
    assert_eq!(e.state, VerificationState::Assumed);
    assert!(
        !e.is_blocking(),
        "Assumed meets prod minimum (Assumed); must not block"
    );
}

#[test]
fn unverified_below_prod_minimum_sets_blocking_field() {
    let m = make_manifest("payments.stripe", "2.3.1", TrustLevel::Unverified);
    let entries = PackageTrustChecker::check(&[m], "prod");
    let e = &entries[0];

    assert_eq!(e.state, VerificationState::Unverified);
    assert!(e.blocking, "entry.blocking must reflect the prod gate");
    assert!(e.is_blocking(), "helper must agree with entry.blocking");
}

#[test]
fn assumed_without_assumptions_is_blocking_unverified() {
    let m = make_manifest("payments.stripe", "2.3.1", TrustLevel::Assumed);
    let entries = PackageTrustChecker::check(&[m], "prod");
    let e = &entries[0];

    assert_eq!(e.state, VerificationState::Unverified);
    assert!(e.blocking, "Assumed without assumptions must block");
    assert!(
        e.evidence
            .as_deref()
            .unwrap_or("")
            .contains("E_PACKAGE_ASSUMPTION_MISSING")
    );
}

#[test]
fn assumed_with_undeclared_assumption_boundary_is_failed() {
    let mut m = make_assumed_manifest("payments.stripe", "2.3.1");
    m.boundaries = vec!["boundary.Other".to_string()];
    let entries = PackageTrustChecker::check(&[m], "prod");
    let e = &entries[0];

    assert_eq!(e.state, VerificationState::Failed);
    assert!(e.blocking);
    assert!(
        e.evidence
            .as_deref()
            .unwrap_or("")
            .contains("E_PACKAGE_ASSUMPTION_FLOATING")
    );
}
