// ── ail-runtime::secret_vault_tests ──────────────────────────────────────
//
// Integration tests for the in-memory secret vault and SecretReadHandler.
//
// Scenarios:
//   T1 — Mapped secret resolves: handler returns the correct secret bytes.
//   T2 — Unmapped secret denied: capability not in secrets_mapping → CapabilityDenied.
//   T3 — Vault path missing: mapping exists but vault has no value → CapabilityDenied.
//   T4 — Audit event for a successful secret read records only hashes, never the value.
//   T5 — Audit event for a denied secret read records only hashes.
//   T6 — SecretVault Debug output is redacted (no values, no paths).
//   T7 — Full round-trip via RuntimeHost::call_capability with grant check.

use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostError,
    ResourceLimits, RuntimeHost, RuntimeProfile, SecretEntry, SecretReadHandler, SecretVault,
    blake3_hex_of,
};

// ── Test helpers ──────────────────────────────────────────────────────────

/// Minimal structurally-valid WASM: magic + version only.
fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

/// Build a `RuntimeProfile` whose hashes match `wasm` and `manifest`.
fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
    secrets_mapping: Vec<SecretEntry>,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        grants,
        ResourceLimits::default(),
    )
    .with_secrets_mapping(secrets_mapping)
}

// ── T1 — Mapped secret resolves ───────────────────────────────────────────

#[test]
fn t1_mapped_secret_resolves() {
    let mut vault = SecretVault::new();
    vault.insert("prod/stripe", b"sk_live_abc123".to_vec());

    let mapping = vec![SecretEntry {
        secret_id: "StripeApiKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];

    let handler = SecretReadHandler::new(mapping, Arc::new(vault));
    let cap = CapabilityId::new("secret.read:StripeApiKey");
    let result = handler
        .handle(&cap, "read", b"")
        .expect("mapped secret must resolve");
    assert_eq!(result, b"sk_live_abc123");
}

// ── T2 — Unmapped secret denied ───────────────────────────────────────────

#[test]
fn t2_unmapped_secret_denied() {
    let vault = SecretVault::new();
    let mapping = vec![SecretEntry {
        secret_id: "StripeApiKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];

    let handler = SecretReadHandler::new(mapping, Arc::new(vault));
    // Request a secret_id NOT in the mapping.
    let cap = CapabilityId::new("secret.read:DbPassword");
    let err = handler
        .handle(&cap, "read", b"")
        .expect_err("unmapped secret must be denied");

    let msg = match &err {
        HostError::CapabilityDenied(m) => m.clone(),
        other => panic!("expected CapabilityDenied, got {other:?}"),
    };
    // Opaque denial: must not reveal the secret ID or any vault detail.
    assert_eq!(msg, "secret access denied", "denial message must be opaque");
    assert!(!msg.contains("DbPassword"), "must not leak secret ID");
    assert!(!msg.contains("prod/"), "must not leak vault path");
}

// ── T3 — Vault path missing ───────────────────────────────────────────────

#[test]
fn t3_vault_path_missing_denied() {
    // Vault is empty — vault_path "prod/stripe" has no value.
    let vault = SecretVault::new();
    let mapping = vec![SecretEntry {
        secret_id: "StripeApiKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];

    let handler = SecretReadHandler::new(mapping, Arc::new(vault));
    let cap = CapabilityId::new("secret.read:StripeApiKey");
    let err = handler
        .handle(&cap, "read", b"")
        .expect_err("missing vault path must be denied");

    let msg = match &err {
        HostError::CapabilityDenied(m) => m.clone(),
        other => panic!("expected CapabilityDenied, got {other:?}"),
    };
    // Opaque denial: must not reveal the secret ID or vault path.
    assert_eq!(msg, "secret access denied", "denial message must be opaque");
    assert!(!msg.contains("StripeApiKey"), "must not leak secret ID");
    assert!(!msg.contains("prod/stripe"), "must not leak vault path");
}

// ── T2/T3 cross-check — unmapped and missing-vault denials are identical ──

#[test]
fn t2t3_unmapped_and_missing_vault_denials_are_identical() {
    // Unmapped case.
    let h_unmapped = SecretReadHandler::new(vec![], Arc::new(SecretVault::new()));
    let cap_unmapped = CapabilityId::new("secret.read:Ghost");
    let err_unmapped = h_unmapped
        .handle(&cap_unmapped, "read", b"")
        .expect_err("must be denied");

    // Mapped-but-vault-missing case.
    let mapping = vec![SecretEntry {
        secret_id: "StripeApiKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];
    let h_missing = SecretReadHandler::new(mapping, Arc::new(SecretVault::new()));
    let cap_missing = CapabilityId::new("secret.read:StripeApiKey");
    let err_missing = h_missing
        .handle(&cap_missing, "read", b"")
        .expect_err("must be denied");

    assert_eq!(
        err_unmapped, err_missing,
        "both denial paths must produce identical errors — no vault-layout oracle"
    );
}

// ── T4 — Audit event for success: only hashes, no raw value ──────────────

#[test]
fn t4_audit_event_for_secret_read_contains_only_hashes() {
    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:StripeApiKey");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "StripeApiKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let mut vault = SecretVault::new();
    vault.insert("prod/stripe", b"sk_live_abc123".to_vec());
    let handler = SecretReadHandler::new(mapping, Arc::new(vault));

    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let response = host
        .call_capability(&cap_id, "read", b"")
        .expect("call must succeed");

    // Response is the raw secret bytes — caller responsibility not to log.
    assert_eq!(response, b"sk_live_abc123");

    // Audit: the CapabilityCallExecuted event must NOT contain the raw bytes.
    let log = host.audit_log();
    let last_event = log
        .events()
        .iter()
        .filter(|e| e.is_capability_call())
        .last()
        .expect("at least one capability call event");

    match last_event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            input_hash,
            output_hash,
            ..
        } => {
            assert!(succeeded, "event must record success");
            // input_hash is hash of the empty payload b"".
            assert!(input_hash.is_some(), "input_hash must be present");
            // output_hash is hash of the secret bytes — NOT the bytes themselves.
            assert!(
                output_hash.is_some(),
                "output_hash must be present for success"
            );
            // output_hash must be exactly the BLAKE3 hex digest of the secret bytes.
            let output_hash_str = output_hash.as_deref().unwrap();
            let expected_hash = blake3_hex_of(b"sk_live_abc123");
            assert_eq!(
                output_hash_str, expected_hash,
                "output_hash must equal blake3_hex_of(secret_bytes)"
            );
            assert_ne!(
                output_hash_str, "sk_live_abc123",
                "output_hash must be a hash, not the raw secret value"
            );
            // Must be a 64-char hex string (BLAKE3 output).
            assert_eq!(
                output_hash_str.len(),
                64,
                "output_hash must be a 64-char BLAKE3 hex digest"
            );
        }
        _ => panic!("expected CapabilityCallExecuted, got {last_event:?}"),
    }
}

// ── T5 — Audit event for denied secret read ───────────────────────────────

#[test]
fn t5_audit_event_for_denied_secret_contains_no_value() {
    let wasm = minimal_wasm();
    // Grant secret.read:StripeApiKey but provide no handler → denial.
    let cap_id = CapabilityId::new("secret.read:StripeApiKey");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap_id.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant], vec![]);

    // No handler registered → HandlerNotBound denial path.
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let err = host
        .call_capability(&cap_id, "read", b"")
        .expect_err("call must fail — no handler bound");

    // Should be HandlerNotBound (no handler) rather than CapabilityDenied.
    assert!(
        matches!(err, HostError::HandlerNotBound(_)),
        "expected HandlerNotBound, got {err:?}"
    );

    let log = host.audit_log();
    let last_event = log
        .events()
        .iter()
        .filter(|e| e.is_capability_call())
        .last()
        .expect("at least one capability call event");

    match last_event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            ..
        } => {
            assert!(!succeeded, "denied event must record failure");
            assert!(
                output_hash.is_none(),
                "output_hash must be absent on failure"
            );
        }
        _ => panic!("expected CapabilityCallExecuted"),
    }
}

// ── T6 — SecretVault Debug is redacted ───────────────────────────────────

#[test]
fn t6_secret_vault_debug_is_redacted() {
    let mut v = SecretVault::new();
    v.insert("prod/stripe", b"sk_live_abc123".to_vec());
    v.insert("prod/db-password", b"hunter2".to_vec());

    let debug_str = format!("{v:?}");

    assert!(
        !debug_str.contains("sk_live_abc123"),
        "secret value must not appear in Debug output"
    );
    assert!(
        !debug_str.contains("hunter2"),
        "secret value must not appear in Debug output"
    );
    assert!(
        !debug_str.contains("prod/stripe"),
        "vault path must not appear in Debug output"
    );
    assert!(
        debug_str.contains("redacted"),
        "Debug output must contain 'redacted'"
    );
}

// ── T7 — Full round-trip via RuntimeHost with grant check ─────────────────

#[test]
fn t7_full_roundtrip_via_runtime_host() {
    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:DbPassword");
    let manifest = CapabilityManifest {
        module: "service".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "service".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "DbPassword".to_string(),
        vault_path: "db/password".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let mut vault = SecretVault::new();
    vault.insert("db/password", b"pg_password_xyz".to_vec());

    let handler = SecretReadHandler::new(mapping, Arc::new(vault));
    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));

    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = host
        .call_capability(&cap_id, "read", b"")
        .expect("secret read must succeed");

    assert_eq!(result, b"pg_password_xyz");
}

// ── T8 — Ungrant blocks secret read even with handler ─────────────────────

#[test]
fn t8_ungrant_blocks_secret_read() {
    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:StripeApiKey");

    // Manifest requires the cap but the profile grants NOTHING → preflight
    // fails with CapabilityDenied, so we must not grant it in the profile.
    // Instead grant an unrelated cap so preflight passes, then try to call
    // the secret.read cap directly on the host — it should be CapabilityDenied.
    let unrelated = CapabilityId::new("other.cap");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![unrelated.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: unrelated,
    };
    // No secrets_mapping or grant for secret.read:StripeApiKey.
    let profile = matching_profile(&wasm, &manifest, vec![grant], vec![]);

    let mut vault = SecretVault::new();
    vault.insert("prod/stripe", b"sk_live_abc123".to_vec());
    let mapping = vec![SecretEntry {
        secret_id: "StripeApiKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];
    let handler = SecretReadHandler::new(mapping, Arc::new(vault));
    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));

    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Attempt to read secret without a grant → CapabilityDenied.
    let err = host
        .call_capability(&cap_id, "read", b"")
        .expect_err("ungrant must deny the call");

    assert!(
        matches!(err, HostError::CapabilityDenied(_)),
        "expected CapabilityDenied, got {err:?}"
    );
}
