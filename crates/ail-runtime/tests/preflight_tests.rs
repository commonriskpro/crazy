// ── ail-runtime::preflight_tests ─────────────────────────────────────────
//
// Task 3.1 (RED): Tests written BEFORE host.rs / RuntimeHost exist.
//
// Spec scenarios covered:
//  - Hash mismatch on WASM bytes halts preflight before capability check.
//  - Ungranted capability produces CapabilityDenied failure.
//  - All grants satisfied + valid WASM → Ok(RuntimeInstance) + PreflightPassed audit.
//  - Manifest CBOR hash mismatch fails with HashMismatch.
//  - AuditLog receives exactly one event per validate_and_instantiate call.

use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, PreflightFailure, ResourceLimits,
    RuntimeError, RuntimeHost, RuntimeProfile, blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

/// Minimal structurally-valid WASM: magic + version only (no sections).
fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

/// Build a `RuntimeProfile` whose hashes match `wasm` and `manifest` exactly.
fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .expect("manifest CBOR hash must succeed");
    RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64), // verification_report_hash — not checked in preflight
        manifest_hash,
        grants,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
}

// ── Scenario 1: WASM hash mismatch halts before grant check ───────────────

// Even when the capability is granted, a wrong module_hash must fail with
// HashMismatch — and the audit log records one PreflightFailed event.
#[test]
fn hash_mismatch_halts_before_grant_check() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap,
    };
    // Profile with WRONG module_hash — capability IS granted, so only the
    // hash check should stop preflight.
    let profile = RuntimeProfile::new(
        "test-profile".to_string(),
        "wrong_hash_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        "a".repeat(64),
        manifest.blake3_hex().unwrap(),
        vec![grant],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    // Must fail with HashMismatch.
    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::HashMismatch { expected, actual })) => {
            assert_eq!(
                expected,
                profile.module_hash(),
                "expected field must equal profile.module_hash()"
            );
            assert_eq!(
                actual,
                blake3_hex_of(&wasm),
                "actual field must equal BLAKE3 of wasm bytes"
            );
        }
        other => panic!("expected HashMismatch, got {other:?}"),
    }

    // Exactly one PreflightFailed event.
    let log = host.audit_log();
    assert_eq!(log.len(), 1, "exactly one audit event per call");
    assert!(
        !log.events()[0].is_passed(),
        "event must be PreflightFailed"
    );
}

// ── Scenario 2: Ungranted capability denied ───────────────────────────────

// All hashes match, but the required capability is absent from the profile.
// Must fail with CapabilityDenied carrying the denied capability.
#[test]
fn ungranted_capability_denied() {
    let wasm = minimal_wasm();
    let required = CapabilityId::new("NetworkEgress");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![required.clone()],
    };
    // Profile has correct hashes but NO grants at all.
    let profile = matching_profile(&wasm, &manifest, vec![]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied { denied })) => {
            assert_eq!(denied.len(), 1, "exactly one capability denied");
            assert_eq!(denied[0], required, "denied capability must match required");
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }

    let log = host.audit_log();
    assert_eq!(log.len(), 1, "exactly one audit event per call");
    assert!(
        !log.events()[0].is_passed(),
        "event must be PreflightFailed"
    );
}

// TRIANGULATE: two ungranted capabilities both appear in the denied list.
#[test]
fn multiple_ungranted_capabilities_all_denied() {
    let wasm = minimal_wasm();
    let cap_a = CapabilityId::new("NetworkEgress");
    let cap_b = CapabilityId::new("FileWrite");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap_a.clone(), cap_b.clone()],
    };
    let profile = matching_profile(&wasm, &manifest, vec![]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied { denied })) => {
            assert_eq!(denied.len(), 2, "both capabilities must be denied");
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }
}

// ── Scenario 3: All grants pass → PreflightPassed ─────────────────────────

// All hashes match and the required capability is granted.
// Must succeed and push PreflightPassed to the audit log.
#[test]
fn all_grants_pass_emits_preflight_passed() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap,
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "expected Ok(RuntimeInstance), got {result:?}"
    );

    let log = host.audit_log();
    assert_eq!(log.len(), 1, "exactly one audit event per call");
    assert!(log.events()[0].is_passed(), "event must be PreflightPassed");
}

// TRIANGULATE: empty requires list with no grants also passes preflight.
#[test]
fn empty_requires_with_no_grants_passes_preflight() {
    let wasm = minimal_wasm();
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest, vec![]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "empty requires must pass when no capabilities needed"
    );
    assert!(host.audit_log().events()[0].is_passed());
}

// ── Scenario 4: Manifest CBOR hash mismatch ──────────────────────────────

// WASM hash matches but capability_manifest_hash in the profile doesn't match
// the CBOR hash of the supplied manifest. Must fail with HashMismatch.
#[test]
fn manifest_hash_mismatch_fails() {
    let wasm = minimal_wasm();
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let module_hash = blake3_hex_of(&wasm);
    // Profile with wrong manifest hash.
    let profile = RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        "wrong_manifest_hash_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::HashMismatch { .. }
            ))
        ),
        "manifest hash mismatch must return HashMismatch, got {result:?}"
    );

    let log = host.audit_log();
    assert_eq!(log.len(), 1);
    assert!(!log.events()[0].is_passed());
}

// ── AuditLog accumulates events across multiple calls ────────────────────

// Each call to validate_and_instantiate appends exactly one event.
// Two calls → two events, in insertion order.
#[test]
fn audit_log_accumulates_across_calls() {
    let wasm = minimal_wasm();
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let good_profile = matching_profile(&wasm, &manifest, vec![]);
    let bad_profile = RuntimeProfile::new(
        "bad".to_string(),
        "wrong_hash_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        "a".repeat(64),
        manifest.blake3_hex().unwrap(),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let _ = host.validate_and_instantiate(&wasm, &manifest, &good_profile);
    let _ = host.validate_and_instantiate(&wasm, &manifest, &bad_profile);

    let log = host.audit_log();
    assert_eq!(log.len(), 2, "two calls must produce two audit events");
    assert!(log.events()[0].is_passed(), "first call passed");
    assert!(!log.events()[1].is_passed(), "second call failed");
}
