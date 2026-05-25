// ── ail-runtime::assumption_expiry_tests ─────────────────────────────────
//
// Preflight stage 7 — assumption expiry enforcement.
//
// Spec scenarios (docs/runtime.md §Startup validation step 7):
//   AE1 — No assumptions declared → passes (backward compat)
//   AE2 — Single Active assumption with no expiry → passes
//   AE3 — Active assumption with future expires_at → passes
//   AE4 — Assumption with status Expired → fails with AssumptionExpired
//   AE5 — Assumption with status Inactive → fails with AssumptionExpired
//   AE6 — Active assumption with past expires_at → fails with AssumptionExpired
//   AE7 — Multiple assumptions: first expired stops check → AssumptionExpired
//   AE8 — AssumptionExpired Display mentions assumption_id and reason

use std::time::{Duration, SystemTime};

use ail_runtime::{
    AssumptionStatus, CapabilityGrant, CapabilityManifest, PreflightFailure, ProfileAssumption,
    ResourceLimits, RuntimeError, RuntimeHost, RuntimeProfile, blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        grants,
        ResourceLimits::default(),
    )
}

fn empty_manifest_profile(wasm: &[u8]) -> (CapabilityManifest, RuntimeProfile) {
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(wasm, &manifest, vec![]);
    (manifest, profile)
}

// ── AE1 — No assumptions → passes ────────────────────────────────────────

#[test]
fn ae1_no_assumptions_passes_preflight() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    // Default profile has no assumptions — stage 7 is a no-op.

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "profile with no assumptions must pass preflight: {result:?}"
    );
}

// ── AE2 — Active assumption with no expiry → passes ──────────────────────

#[test]
fn ae2_active_assumption_no_expiry_passes() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    let profile = profile.with_assumptions(vec![ProfileAssumption::active("payment-api-v2")]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "active assumption with no expiry must pass preflight: {result:?}"
    );
}

// ── AE3 — Active assumption with future expires_at → passes ──────────────

#[test]
fn ae3_active_assumption_future_expiry_passes() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    let future = SystemTime::now() + Duration::from_secs(3600);
    let profile =
        profile.with_assumptions(vec![ProfileAssumption::active_until("ml-model-v3", future)]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "active assumption with future expiry must pass preflight: {result:?}"
    );
}

// ── AE4 — Assumption with status Expired → fails ─────────────────────────

#[test]
fn ae4_expired_assumption_fails_preflight() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    let profile = profile.with_assumptions(vec![ProfileAssumption::expired("old-contract-2024")]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::AssumptionExpired {
            assumption_id,
            reason,
        })) => {
            assert_eq!(
                assumption_id, "old-contract-2024",
                "assumption_id must match"
            );
            assert!(
                reason.contains("Expired"),
                "reason must mention Expired status: {reason}"
            );
        }
        other => panic!("expected AssumptionExpired, got {other:?}"),
    }

    // Audit must record a failure.
    let log = host.audit_log();
    assert_eq!(log.len(), 1, "exactly one audit event");
    assert!(
        !log.events()[0].is_passed(),
        "event must be PreflightFailed"
    );
}

// ── AE5 — Assumption with status Inactive → fails ────────────────────────

#[test]
fn ae5_inactive_assumption_fails_preflight() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    let profile = profile.with_assumptions(vec![ProfileAssumption::inactive("staging-db-replica")]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::AssumptionExpired {
            assumption_id,
            reason,
        })) => {
            assert_eq!(assumption_id, "staging-db-replica");
            assert!(
                reason.contains("Inactive"),
                "reason must mention Inactive status: {reason}"
            );
        }
        other => panic!("expected AssumptionExpired for inactive assumption, got {other:?}"),
    }
}

// ── AE6 — Active assumption with past expires_at → fails ─────────────────

#[test]
fn ae6_past_expiry_timestamp_fails_preflight() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    // 1 second in the past.
    let past = SystemTime::now() - Duration::from_secs(1);
    let assumption = ProfileAssumption {
        id: "old-api-contract".to_string(),
        status: AssumptionStatus::Active,
        expires_at: Some(past),
    };
    let profile = profile.with_assumptions(vec![assumption]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::AssumptionExpired {
            assumption_id,
            reason,
        })) => {
            assert_eq!(assumption_id, "old-api-contract");
            assert!(
                reason.contains("past"),
                "reason must mention past expiry: {reason}"
            );
        }
        other => panic!("expected AssumptionExpired for past expires_at, got {other:?}"),
    }
}

// ── AE7 — Multiple assumptions: first expired stops at first failure ───────

#[test]
fn ae7_multiple_assumptions_first_expired_triggers_failure() {
    let wasm = minimal_wasm();
    let (manifest, profile) = empty_manifest_profile(&wasm);
    let profile = profile.with_assumptions(vec![
        ProfileAssumption::active("live-assumption-a"),
        ProfileAssumption::expired("dead-assumption-b"),
        ProfileAssumption::active("live-assumption-c"),
    ]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::AssumptionExpired {
            assumption_id,
            ..
        })) => {
            assert_eq!(
                assumption_id, "dead-assumption-b",
                "first expired assumption must be reported"
            );
        }
        other => panic!("expected AssumptionExpired, got {other:?}"),
    }
}

// ── AE8 — AssumptionExpired Display mentions id and reason ────────────────

#[test]
fn ae8_assumption_expired_display_contains_id_and_reason() {
    let failure = PreflightFailure::AssumptionExpired {
        assumption_id: "payment-gateway-v1".to_string(),
        reason: "status is Expired".to_string(),
    };
    let msg = failure.to_string();
    assert!(
        msg.contains("payment-gateway-v1"),
        "display must include assumption_id: {msg}"
    );
    assert!(
        msg.contains("Expired"),
        "display must include reason: {msg}"
    );
}
