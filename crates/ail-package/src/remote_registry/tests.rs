use super::*;
use crate::advisory::{AdvisorySeverity, SecurityAdvisory};
use crate::manifest::{PackageDef, PackageManifest};
use crate::registry::PackageRegistry;
use crate::signing::PackageKeypair;
use crate::trust::TrustLevel;
use rand::rngs::OsRng;

fn make_manifest(name: &str, version: &str) -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: name.to_string(),
        version: version.to_string(),
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
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        // 4G fields
        reproducible_evidence: None,
    })
}

fn gen_keypair() -> PackageKeypair {
    let secret = ed25519_dalek::SigningKey::generate(&mut OsRng);
    PackageKeypair::from_bytes(&secret.to_bytes())
}

// ── publish_request_cbor_round_trip ───────────────────────────────────
// Spec scenario: "PublishRequest round-trips through CBOR"
#[test]
fn publish_request_cbor_round_trip() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let signed = kp.sign_manifest(manifest).expect("sign");
    let req = PublishRequest {
        signed_package: signed,
    };
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&req, &mut buf).expect("encode");
    let decoded: PublishRequest = ciborium::de::from_reader(buf.as_slice()).expect("decode");
    assert_eq!(decoded, req);
}

// ── in_memory_client_publish_accepts_valid ────────────────────────────
// Spec scenario: "In-memory client accepts a valid signed package"
#[test]
fn in_memory_client_publish_accepts_valid() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let signed = kp.sign_manifest(manifest).expect("sign");
    let client = InMemoryRegistryClient::new();
    let resp = client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("no transport error");
    assert!(resp.accepted);
    assert!(resp.error.is_none());
}

// ── in_memory_client_publish_persists_for_fetch ──────────────────────
// Spec scenario: "publish + fetch round-trip returns the signed package"
#[test]
fn in_memory_client_publish_persists_for_fetch() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let signed = kp.sign_manifest(manifest).expect("sign");
    let client = InMemoryRegistryClient::new();

    let publish = client
        .publish(PublishRequest {
            signed_package: signed.clone(),
        })
        .expect("publish");
    assert!(publish.accepted);

    let fetch = client
        .fetch(FetchRequest {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
        })
        .expect("fetch");

    assert_eq!(fetch.signed_package, Some(signed));
    assert!(!fetch.yanked);
    assert!(fetch.error.is_none());
}

// ── in_memory_client_publish_persists_for_search ──────────────────────
// Spec scenario: "publish + search returns registry metadata"
#[test]
fn in_memory_client_publish_persists_for_search() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let signed = kp.sign_manifest(manifest).expect("sign");
    let client = InMemoryRegistryClient::new();

    client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("publish");

    let search = client
        .search(SearchRequest {
            query: "stripe".to_string(),
            limit: None,
        })
        .expect("search");

    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].name, "payments.stripe");
    assert_eq!(search.results[0].latest_version, "1.2.0");
    assert!(!search.truncated);
}

// ── in_memory_client_search_dedupes_by_package_with_latest_version ────
// Spec scenario: "Search returns one package row with the latest version"
#[test]
fn in_memory_client_search_dedupes_by_package_with_latest_version() {
    let mut client = InMemoryRegistryClient::new();
    client
        .registry
        .register(make_manifest("payments.stripe", "1.2.0"));

    let kp = gen_keypair();
    let signed = kp
        .sign_manifest(make_manifest("payments.stripe", "1.10.0"))
        .expect("sign");
    client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("publish");

    let search = client
        .search(SearchRequest {
            query: "stripe".to_string(),
            limit: None,
        })
        .expect("search");

    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].name, "payments.stripe");
    assert_eq!(search.results[0].latest_version, "1.10.0");
}

// ── in_memory_client_publish_persists_for_verify ──────────────────────
// Spec scenario: "publish + verify checks the signed package hash"
#[test]
fn in_memory_client_publish_persists_for_verify() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed = kp.sign_manifest(manifest).expect("sign");
    let client = InMemoryRegistryClient::new();

    client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("publish");

    let verify = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            expected_hash,
        })
        .expect("verify");

    assert_eq!(verify.outcome, VerifyOutcome::Ok);
}

// ── in_memory_client_yank_blocks_published_package ────────────────────
// Spec scenario: "yank metadata blocks verification but preserves fetch"
#[test]
fn in_memory_client_yank_blocks_published_package() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed = kp.sign_manifest(manifest).expect("sign");
    let mut client = InMemoryRegistryClient::new();

    client
        .publish(PublishRequest {
            signed_package: signed.clone(),
        })
        .expect("publish");
    client.yank("payments.stripe", "1.2.0", "security regression");

    let verify = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            expected_hash,
        })
        .expect("verify");
    assert!(
        matches!(verify.outcome, VerifyOutcome::Yanked { reason } if reason == "security regression")
    );

    let fetch = client
        .fetch(FetchRequest {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
        })
        .expect("fetch");
    assert_eq!(fetch.signed_package, Some(signed));
    assert!(fetch.yanked);
}

// ── in_memory_client_publish_rejects_tampered ─────────────────────────
// Spec scenario: "In-memory client rejects tampered packages"
#[test]
fn in_memory_client_publish_rejects_tampered() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let mut signed = kp.sign_manifest(manifest).expect("sign");
    signed.manifest.version = "9.9.9".to_string(); // tamper
    let client = InMemoryRegistryClient::new();
    let resp = client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("no transport error");
    assert!(!resp.accepted);
    assert!(resp.error.is_some());
}

// ── in_memory_client_search ───────────────────────────────────────────
// Spec scenario: "Search returns packages matching query"
#[test]
fn in_memory_client_search() {
    let mut client = InMemoryRegistryClient::new();
    client
        .registry
        .register(make_manifest("payments.stripe", "1.2.0"));
    client
        .registry
        .register(make_manifest("payments.paypal", "2.0.0"));
    client
        .registry
        .register(make_manifest("utils.core", "0.1.0"));

    let resp = client
        .search(SearchRequest {
            query: "payments".to_string(),
            limit: None,
        })
        .expect("no transport error");
    assert_eq!(resp.results.len(), 2);
    assert!(!resp.truncated);
}

// ── in_memory_client_verify_ok ────────────────────────────────────────
// Spec scenario: "Verify returns Ok for matching hash"
#[test]
fn in_memory_client_verify_ok() {
    let mut client = InMemoryRegistryClient::new();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    client.registry.register(manifest);

    let resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");
    assert_eq!(resp.outcome, VerifyOutcome::Ok);
}

// ── in_memory_client_verify_hash_mismatch ─────────────────────────────
// Spec scenario: "Verify returns HashMismatch for wrong hash"
#[test]
fn in_memory_client_verify_hash_mismatch() {
    let mut client = InMemoryRegistryClient::new();
    client
        .registry
        .register(make_manifest("payments.stripe", "1.2.0"));

    let resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            expected_hash: "z".repeat(64),
        })
        .expect("no transport error");
    assert!(matches!(resp.outcome, VerifyOutcome::HashMismatch { .. }));
}

// ── in_memory_client_verify_not_found ─────────────────────────────────
// Spec scenario: "Verify returns NotFound for absent package"
#[test]
fn in_memory_client_verify_not_found() {
    let client = InMemoryRegistryClient::new();
    let resp = client
        .verify(VerifyRequest {
            name: "unknown.pkg".to_string(),
            version: "1.0.0".to_string(),
            expected_hash: "a".repeat(64),
        })
        .expect("no transport error");
    assert_eq!(resp.outcome, VerifyOutcome::NotFound);
}

// ── in_memory_client_verify_advisory ─────────────────────────────────
// Spec scenario: "Verify returns Advisory for affected package"
#[test]
fn in_memory_client_verify_advisory() {
    let mut client = InMemoryRegistryClient::new();
    client
        .registry
        .register(make_manifest("payments.stripe", "1.0.0"));
    client.add_advisory(SecurityAdvisory {
        id: "adv_001".to_string(),
        package: "payments.stripe".to_string(),
        affected_constraint: "1.0.0".to_string(),
        severity: AdvisorySeverity::Critical,
        reason: "bug".to_string(),
    });

    let resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            expected_hash: "a".repeat(64),
        })
        .expect("no transport error");
    assert!(
        matches!(resp.outcome, VerifyOutcome::Advisory { advisory_id, .. } if advisory_id == "adv_001")
    );
}

// ── publish_signed_wires_signing_into_registry ────────────────────────
// Spec scenario: "publish_signed verifies signature before registering"
#[test]
fn publish_signed_wires_signing_into_registry() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let signed = kp.sign_manifest(manifest.clone()).expect("sign");

    let mut registry = PackageRegistry::new();
    let result = publish_signed(&mut registry, &signed);
    assert!(result.is_ok());
    assert!(
        registry
            .lookup_by_name_version("payments.stripe", "1.2.0")
            .is_some()
    );
}

// ── publish_signed_rejects_tampered ───────────────────────────────────
// TRIANGULATE: tampered package is rejected
#[test]
fn publish_signed_rejects_tampered() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let mut signed = kp.sign_manifest(manifest).expect("sign");
    signed.manifest.version = "9.9.9".to_string(); // tamper

    let mut registry = PackageRegistry::new();
    assert!(publish_signed(&mut registry, &signed).is_err());
    assert!(registry.is_empty());
}

// ── publish_signed_audit_rejects_tampered_with_stable_code ────────────
// Production gate: rejected signed publishes expose stable, redacted issue codes.
#[test]
fn publish_signed_audit_rejects_tampered_with_stable_code() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let mut signed = kp.sign_manifest(manifest).expect("sign");
    signed.manifest.version = "9.9.9".to_string(); // tamper after signing

    let mut registry = PackageRegistry::new();
    let result = super::signed_publish::publish_signed_audited(&mut registry, &signed);

    assert!(!result.accepted());
    assert!(registry.is_empty());

    let audit = result.audit();
    assert!(!audit.accepted);
    assert_eq!(audit.package_name, "payments.stripe");
    assert_eq!(audit.package_version, "9.9.9");
    assert_eq!(audit.issues.len(), 1);

    let issue = &audit.issues[0];
    assert_eq!(
        issue.kind,
        super::signed_publish::SignedPublishIssueKind::SignatureInvalid
    );
    assert_eq!(issue.code, "SIGNED_PUBLISH_SIGNATURE_INVALID");
    assert_eq!(issue.category, "signature_integrity");
    assert_eq!(issue.signer_key.algorithm, "ed25519");
    assert_eq!(issue.signer_key.byte_len, 32);
    assert!(issue.signer_key.redacted);
}

// ── signed_publish_validation_orders_identity_before_signature ────────
// Production gate: audit issues are deterministic and do not leak raw keys.
#[test]
fn signed_publish_validation_orders_identity_before_signature() {
    let kp = gen_keypair();
    let manifest = make_manifest("payments.stripe", "1.2.0");
    let mut signed = kp.sign_manifest(manifest).expect("sign");
    signed.sig.signer = [0; 32];

    let audit = super::signed_publish::validate_signed_publish(&signed);

    assert!(!audit.accepted);
    let codes: Vec<_> = audit
        .issues
        .iter()
        .map(|issue| issue.code.as_str())
        .collect();
    assert_eq!(
        codes,
        vec![
            "SIGNED_PUBLISH_MISSING_KEY_ID",
            "SIGNED_PUBLISH_SIGNATURE_INVALID",
        ]
    );
    assert!(audit.issues.iter().all(|issue| issue.signer_key.redacted));
    assert!(
        audit
            .issues
            .iter()
            .all(|issue| issue.signer_key.byte_len == 32)
    );
}

// ── fetch_response_cbor_round_trip ────────────────────────────────────
// TRIANGULATE: FetchResponse with no signed_package round-trips through CBOR
#[test]
fn fetch_response_cbor_round_trip() {
    let resp = FetchResponse {
        signed_package: None,
        yanked: false,
        error: Some("not found".to_string()),
    };
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&resp, &mut buf).expect("encode");
    let decoded: FetchResponse = ciborium::de::from_reader(buf.as_slice()).expect("decode");
    assert_eq!(decoded, resp);
}

// ── sequence_monotonic_on_same_name_version_republish ─────────────────
// Regression: re-publishing the same name/version must still advance the
// sequence — it must not repeat the previous value or go backward.
//
//   GIVEN an in-memory registry
//   WHEN pkg v1.0.0 is published twice (duplicate name/version)
//   THEN the second publish response has a strictly higher sequence
#[test]
fn sequence_monotonic_on_same_name_version_republish() {
    let kp = gen_keypair();
    let client = InMemoryRegistryClient::new();

    let r1 = client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("mono.pkg", "1.0.0"))
                .expect("sign"),
        })
        .expect("first publish");
    // Re-publish identical name/version.
    let r2 = client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("mono.pkg", "1.0.0"))
                .expect("sign again"),
        })
        .expect("second publish");

    assert!(r1.accepted);
    assert!(r2.accepted);

    let s1 = r1.sequence.expect("sequence on first publish");
    let s2 = r2.sequence.expect("sequence on second publish");
    assert!(
        s2 > s1,
        "re-publishing same name/version must still advance sequence: s1={s1} s2={s2}"
    );
}

// ── search_exactly_one_result_after_same_version_republish ───────────
// Structural invariant: retain() removes the previous entry before the new
// one is pushed, so a same-name/version republish must not leave duplicate
// rows in the store.  Search must return exactly 1 result.
//
//   GIVEN an in-memory registry
//   WHEN pkg v1.0.0 is published and then republished (same name/version)
//   THEN search("dedup") returns exactly 1 result
#[test]
fn search_exactly_one_result_after_same_version_republish() {
    let kp = gen_keypair();
    let client = InMemoryRegistryClient::new();

    client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("dedup.pkg", "1.0.0"))
                .expect("sign first"),
        })
        .expect("first publish");

    // Re-publish the same name/version; retain() must deduplicate.
    client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("dedup.pkg", "1.0.0"))
                .expect("sign second"),
        })
        .expect("second publish");

    let search = client
        .search(SearchRequest {
            query: "dedup".to_string(),
            limit: None,
        })
        .expect("search");

    assert_eq!(
        search.results.len(),
        1,
        "same-version republish must not leave duplicate search entries"
    );
    assert_eq!(search.results[0].name, "dedup.pkg");
    assert_eq!(search.results[0].latest_version, "1.0.0");
}

// ── sequence_monotonic_across_unrelated_publishes ─────────────────────
// Verify that unrelated publishes (different names) also produce a strictly
// increasing sequence chain.
//
//   GIVEN three publishes of different packages in order a, b, c
//   THEN sequence(a) < sequence(b) < sequence(c)
#[test]
fn sequence_monotonic_across_unrelated_publishes() {
    let kp = gen_keypair();
    let client = InMemoryRegistryClient::new();

    let ra = client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("pkg.alpha", "1.0.0"))
                .expect("sign a"),
        })
        .expect("publish a");
    let rb = client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("pkg.beta", "1.0.0"))
                .expect("sign b"),
        })
        .expect("publish b");
    let rc = client
        .publish(PublishRequest {
            signed_package: kp
                .sign_manifest(make_manifest("pkg.gamma", "1.0.0"))
                .expect("sign c"),
        })
        .expect("publish c");

    let sa = ra.sequence.expect("sequence a");
    let sb = rb.sequence.expect("sequence b");
    let sc = rc.sequence.expect("sequence c");
    assert!(sa < sb, "sequence(a) < sequence(b): {sa} < {sb}");
    assert!(sb < sc, "sequence(b) < sequence(c): {sb} < {sc}");
}

// ── remote_registry_transport_diagnostics_are_stable_and_redacted ─────
// Production gate: transport failures expose stable codes without URLs,
// tokens, malformed bodies, or package coordinates.
#[test]
fn remote_registry_transport_diagnostics_are_stable_and_redacted() {
    let report = RemoteRegistryDiagnosticReport::from_diagnostics(vec![
        RemoteRegistryDiagnostic::malformed_response(
            RemoteRegistryOperation::Verify,
            "https://registry.internal.example/packages/payments.stripe/1.2.0?token=secret",
            br#"{"name":"payments.stripe","version":"1.2.0","token":"secret"}"#,
        ),
        RemoteRegistryDiagnostic::auth_denied(
            RemoteRegistryOperation::Publish,
            "https://registry.internal.example/publish/payments.stripe/1.2.0",
            "Bearer secret-token",
        ),
        RemoteRegistryDiagnostic::registry_unavailable(
            RemoteRegistryOperation::Fetch,
            "https://registry.internal.example/fetch/payments.stripe/1.2.0",
        ),
    ]);

    let codes: Vec<_> = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert_eq!(
        codes,
        vec![
            "REMOTE_REGISTRY_AUTH_DENIED",
            "REMOTE_REGISTRY_MALFORMED_RESPONSE",
            "REMOTE_REGISTRY_UNAVAILABLE",
        ]
    );

    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.redaction.registry_url
            && diagnostic.redaction.auth_token
            && diagnostic.redaction.package_coordinate
    }));

    let encoded = serde_json::to_string(&report).expect("serialize diagnostics");
    for leaked in [
        "registry.internal.example",
        "payments.stripe",
        "1.2.0",
        "secret-token",
        "secret",
        "Bearer",
    ] {
        assert!(
            !encoded.contains(leaked),
            "remote registry diagnostics must not leak {leaked}: {encoded}"
        );
    }
}

// ── in_memory_index_diagnostics_cover_stale_and_duplicate_redacted ────
// Production gate: represented duplicate versions and stale index rows are
// diagnosed with deterministic, redacted issue records.
#[test]
fn in_memory_index_diagnostics_cover_stale_and_duplicate_redacted() {
    let kp = gen_keypair();
    let mut client = InMemoryRegistryClient::new();

    client
        .registry
        .register_signed(
            kp.sign_manifest(make_manifest("private.alpha", "1.0.0"))
                .expect("sign indexed package"),
        )
        .expect("register signed package");
    client
        .registry
        .register(make_manifest("private.alpha", "1.0.0"));
    client
        .registry
        .register(make_manifest("private.beta", "2.0.0"));

    let report = client.diagnose_index();
    let codes: Vec<_> = report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.ordinal))
        .collect();

    assert_eq!(
        codes,
        vec![
            ("REMOTE_REGISTRY_DUPLICATE_PUBLISH_VERSION", 0),
            ("REMOTE_REGISTRY_STALE_INDEX", 0),
        ]
    );
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.redaction.registry_url
            && diagnostic.redaction.auth_token
            && diagnostic.redaction.package_coordinate
    }));

    let encoded = serde_json::to_string(&report).expect("serialize diagnostics");
    for leaked in ["private.alpha", "private.beta", "1.0.0", "2.0.0"] {
        assert!(
            !encoded.contains(leaked),
            "index diagnostics must not leak package coordinates: {encoded}"
        );
    }
}
