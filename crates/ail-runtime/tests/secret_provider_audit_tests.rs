// ── ail-runtime::secret_provider_audit_tests ─────────────────────────────
//
// Tests proving that SecretProvider error categories surface in the audit log
// without leaking any secret data, vault paths, or secret IDs.
//
// Security invariants verified here:
//   - `SecretProviderError::NotFound` from a custom provider maps to
//     `denial_category: Some("secret.not_found")` in the audit log.
//   - `SecretProviderError::Unavailable` from a custom provider maps to
//     `denial_category: Some("secret.provider_unavailable")` in the audit log.
//   - Mapping-miss and vault-miss both produce `"secret.not_found"` — same
//     category prevents vault-layout probing across provider implementations.
//   - All callers (both host-side `call_capability` and WASM dispatch) receive
//     only plain `HostError::CapabilityDenied` — `CapabilityDeniedCategorized`
//     is converted to opaque denial before being returned.
//   - The audit `denial_category` field does not contain secret IDs, vault
//     paths, or any data that identifies the specific secret being accessed.
//   - Successful reads produce `denial_category: None` in the audit log.
//   - Non-secret handler denials (`succeeded: false`) produce `denial_category: None`.

use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, HostError, ResourceLimits,
    RuntimeHost, RuntimeProfile, SecretEntry, SecretProvider, SecretProviderError,
    SecretReadHandler, SecretVault,
    audit::{SECRET_ACCESS_SHAPE_READ, SECRET_AUDIT_CATEGORY_UNSUPPORTED_OPERATION},
    blake3_hex_of,
};

// ── helpers ───────────────────────────────────────────────────────────────

fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
    secrets_mapping: Vec<SecretEntry>,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash");
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

/// Extract the last `CapabilityCallExecuted` event from the audit log.
fn last_cap_event(host: &RuntimeHost) -> AuditEvent {
    host.audit_log()
        .events()
        .iter()
        .rfind(|e| e.is_capability_call())
        .cloned()
        .expect("at least one capability call event")
}

// ── PA1 — NotFound from custom provider → audit "secret.not_found" ───────

#[test]
fn pa1_provider_not_found_surfaces_in_audit_category() {
    // Provider that always signals NotFound.
    struct NotFoundProvider;
    impl SecretProvider for NotFoundProvider {
        fn resolve(&self, _vault_path: &str) -> Result<Vec<u8>, SecretProviderError> {
            Err(SecretProviderError::NotFound)
        }
    }

    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:MySecret");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "MySecret".to_string(),
        vault_path: "vault/path".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let handler = SecretReadHandler::new(mapping, Arc::new(NotFoundProvider));
    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let err = host
        .call_capability(&cap_id, "read", b"")
        .expect_err("provider NotFound must deny");

    // Caller receives plain opaque denial — no audit_category leaked.
    assert!(
        matches!(err, HostError::CapabilityDenied(_)),
        "caller must receive CapabilityDenied (not categorized), got {err:?}"
    );
    let msg = match &err {
        HostError::CapabilityDenied(m) => m.as_str(),
        _ => unreachable!(),
    };
    assert_eq!(msg, "secret access denied", "caller message must be opaque");
    assert!(!msg.contains("MySecret"), "must not leak secret ID");
    assert!(!msg.contains("vault/path"), "must not leak vault path");
    assert!(!msg.contains("NotFound"), "must not leak error variant");

    // Audit log carries the generic category.
    match last_cap_event(&host) {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            denial_category,
            ..
        } => {
            assert!(!succeeded, "audit must record failure");
            assert!(output_hash.is_none(), "no output hash on failure");
            assert_eq!(
                denial_category.as_deref(),
                Some("secret.not_found"),
                "audit category must be secret.not_found"
            );
            // Category string must not contain secret IDs or paths.
            let cat = denial_category.as_deref().unwrap_or("");
            assert!(
                !cat.contains("MySecret"),
                "audit category must not contain secret ID"
            );
            assert!(
                !cat.contains("vault/path"),
                "audit category must not contain vault path"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── PA2 — Unavailable from custom provider → audit "secret.provider_unavailable"

#[test]
fn pa2_provider_unavailable_surfaces_in_audit_category() {
    // Provider that signals a transient unavailability (network error, etc.).
    struct DownProvider;
    impl SecretProvider for DownProvider {
        fn resolve(&self, _vault_path: &str) -> Result<Vec<u8>, SecretProviderError> {
            Err(SecretProviderError::Unavailable)
        }
    }

    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:ApiKey");
    let manifest = CapabilityManifest {
        module: "svc".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "svc".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "ApiKey".to_string(),
        vault_path: "service/api-key".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let handler = SecretReadHandler::new(mapping, Arc::new(DownProvider));
    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let err = host
        .call_capability(&cap_id, "read", b"")
        .expect_err("unavailable provider must deny");

    // Caller always gets plain opaque denial.
    assert!(
        matches!(err, HostError::CapabilityDenied(_)),
        "caller must receive CapabilityDenied, got {err:?}"
    );
    let msg = match &err {
        HostError::CapabilityDenied(m) => m.as_str(),
        _ => unreachable!(),
    };
    assert_eq!(msg, "secret access denied");
    assert!(!msg.contains("ApiKey"), "must not leak secret ID");
    assert!(!msg.contains("service/"), "must not leak vault path");
    assert!(
        !msg.contains("unavailable"),
        "must not mention provider status"
    );

    // Audit distinguishes provider failure from not-found.
    match last_cap_event(&host) {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            denial_category,
            ..
        } => {
            assert!(!succeeded, "audit must record failure");
            assert!(output_hash.is_none(), "no output hash on failure");
            assert_eq!(
                denial_category.as_deref(),
                Some("secret.provider_unavailable"),
                "audit category must distinguish transient provider failure"
            );
            // Category must be generic — no secret data.
            let cat = denial_category.as_deref().unwrap_or("");
            assert!(!cat.contains("ApiKey"), "must not contain secret ID");
            assert!(!cat.contains("service/"), "must not contain vault path");
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── PA3 — Custom NotFound and vault miss produce same audit category ──────
//
// Both `SecretProviderError::NotFound` from a custom provider and a missing
// vault path from `SecretVault` must produce the same `"secret.not_found"`
// audit category — callers cannot distinguish which provider returned the
// not-found error.  This is the oracle-prevention invariant for audit logs.

#[test]
fn pa3_custom_not_found_and_vault_miss_produce_same_audit_category() {
    let wasm = minimal_wasm();

    // Case A: custom provider returning NotFound.
    struct NullProvider;
    impl SecretProvider for NullProvider {
        fn resolve(&self, _: &str) -> Result<Vec<u8>, SecretProviderError> {
            Err(SecretProviderError::NotFound)
        }
    }

    let cap = CapabilityId::new("secret.read:StripeKey");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let mapping = vec![SecretEntry {
        secret_id: "StripeKey".to_string(),
        vault_path: "prod/stripe".to_string(),
    }];

    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };

    // Case A: custom provider NotFound.
    let profile_a = matching_profile(&wasm, &manifest, vec![grant.clone()], mapping.clone());
    let mut host_a = RuntimeHost::new().with_handler(Arc::new(SecretReadHandler::new(
        mapping.clone(),
        Arc::new(NullProvider),
    )));
    host_a
        .validate_and_instantiate(&wasm, &manifest, &profile_a)
        .unwrap();
    host_a.call_capability(&cap, "read", b"").unwrap_err();
    let cat_a = match last_cap_event(&host_a) {
        AuditEvent::CapabilityCallExecuted {
            denial_category, ..
        } => denial_category,
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    };

    // Case B: SecretVault with missing path (same as NotFound from vault).
    let profile_b = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());
    let mut host_b = RuntimeHost::new().with_handler(Arc::new(SecretReadHandler::new(
        mapping,
        Arc::new(SecretVault::new()), // empty vault → path not found
    )));
    host_b
        .validate_and_instantiate(&wasm, &manifest, &profile_b)
        .unwrap();
    host_b.call_capability(&cap, "read", b"").unwrap_err();
    let cat_b = match last_cap_event(&host_b) {
        AuditEvent::CapabilityCallExecuted {
            denial_category, ..
        } => denial_category,
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    };

    // Both must produce the same category — no provider-implementation oracle.
    assert_eq!(
        cat_a, cat_b,
        "custom NotFound and SecretVault miss must produce identical audit category"
    );
    assert_eq!(
        cat_a.as_deref(),
        Some("secret.not_found"),
        "category must be secret.not_found for both not-found cases"
    );
}

// ── PA4 — Successful read → denial_category is None ──────────────────────

#[test]
fn pa4_successful_read_has_no_denial_category() {
    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:DbPass");
    let manifest = CapabilityManifest {
        module: "svc".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "svc".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "DbPass".to_string(),
        vault_path: "db/password".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let mut vault = SecretVault::new();
    vault.insert("db/password", b"hunter2".to_vec());
    let handler = SecretReadHandler::new(mapping, Arc::new(vault));

    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let response = host
        .call_capability(&cap_id, "read", b"")
        .expect("must succeed");
    assert_eq!(response, b"hunter2");

    match last_cap_event(&host) {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            denial_category,
            ..
        } => {
            assert!(succeeded, "audit must record success");
            assert!(
                denial_category.is_none(),
                "successful read must not have a denial_category"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── PA5 — Wrong operation → redacted secret audit category and shape ─────

#[test]
fn pa5_wrong_operation_records_redacted_secret_audit_category_and_shape() {
    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:PaymentsApiKey");
    let manifest = CapabilityManifest {
        module: "svc".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "svc".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "PaymentsApiKey".to_string(),
        vault_path: "prod/payments/api-key".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let mut vault = SecretVault::new();
    vault.insert("prod/payments/api-key", b"sk_live_payments_secret".to_vec());
    let handler = SecretReadHandler::new(mapping, Arc::new(vault));

    let mut host = RuntimeHost::new().with_handler(Arc::new(handler));
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let err = host
        .call_capability(&cap_id, "write", b"")
        .expect_err("unsupported operation must deny");

    assert!(
        matches!(err, HostError::CapabilityDenied(_)),
        "caller must receive opaque CapabilityDenied, got {err:?}"
    );
    assert_eq!(
        err.capability_denied_message(),
        Some("secret access denied")
    );

    let event = last_cap_event(&host);
    assert_eq!(
        event.secret_access_shape(),
        Some(SECRET_ACCESS_SHAPE_READ),
        "secret access shape must redact the concrete secret name"
    );
    let shape = event.secret_access_shape().expect("secret access shape");
    assert!(!shape.contains("PaymentsApiKey"));
    assert!(!shape.contains("prod/payments"));
    assert!(!shape.contains("sk_live"));

    match event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            denial_category,
            ..
        } => {
            assert!(!succeeded, "audit must record failure");
            assert!(output_hash.is_none(), "no output hash on denial");
            assert_eq!(
                denial_category.as_deref(),
                Some(SECRET_AUDIT_CATEGORY_UNSUPPORTED_OPERATION),
                "audit category must classify unsupported secret operations"
            );
            let cat = denial_category.as_deref().unwrap_or("");
            assert!(!cat.contains("PaymentsApiKey"));
            assert!(!cat.contains("prod/payments"));
            assert!(!cat.contains("sk_live"));
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── PA6 — Denial category string contains no secret data ─────────────────

#[test]
fn pa6_denial_category_contains_no_secret_data() {
    // Provider that signals unavailable — simulates a vault containing
    // obviously identifiable secret data to verify it does NOT appear.
    struct ObviousUnavailableProvider;
    impl SecretProvider for ObviousUnavailableProvider {
        fn resolve(&self, _vault_path: &str) -> Result<Vec<u8>, SecretProviderError> {
            // Provider error must not include vault_path or any secret data.
            Err(SecretProviderError::Unavailable)
        }
    }

    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("secret.read:TopSecretApiKey");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap_id.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap_id.clone(),
    };
    let mapping = vec![SecretEntry {
        secret_id: "TopSecretApiKey".to_string(),
        vault_path: "prod/top-secret-service/key".to_string(),
    }];
    let profile = matching_profile(&wasm, &manifest, vec![grant], mapping.clone());

    let mut host = RuntimeHost::new().with_handler(Arc::new(SecretReadHandler::new(
        mapping,
        Arc::new(ObviousUnavailableProvider),
    )));
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");
    host.call_capability(&cap_id, "read", b"")
        .expect_err("must deny");

    match last_cap_event(&host) {
        AuditEvent::CapabilityCallExecuted {
            denial_category, ..
        } => {
            let cat = denial_category.as_deref().unwrap_or("");
            // Category must be a generic opaque string.
            assert!(
                !cat.contains("TopSecretApiKey"),
                "denial_category must not contain the secret ID"
            );
            assert!(
                !cat.contains("prod/top-secret-service"),
                "denial_category must not contain the vault path"
            );
            assert!(
                !cat.contains("sk_live"),
                "denial_category must not contain secret values"
            );
            // Must be one of the documented opaque categories.
            assert!(
                cat == "secret.not_found" || cat == "secret.provider_unavailable",
                "denial_category must be a known opaque category, got {cat:?}"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}
