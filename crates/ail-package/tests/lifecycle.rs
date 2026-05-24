// ── ail-package — integrated lifecycle tests ──────────────────────────────
//
// These tests connect signing → publish → fetch → verify → yank/advisory flows
// across the public API of `ail-package`.
//
// # Design choices
//
// - Uses only `ail_package::*` public API (no access to private fields).
// - Keypairs are constructed from deterministic fixed bytes so tests are
//   reproducible without a random-number generator dependency.
// - All scenarios use `InMemoryRegistryClient` as the registry transport.
// - `TransparencyLog` is accessed via its fully-qualified module path because
//   it is intentionally not re-exported from `lib.rs`.

use ail_package::{
    AdvisoryChecker, AdvisorySeverity, FetchRequest, InMemoryRegistryClient, PackageKeypair,
    PublishRequest, SecurityAdvisory, VerifyOutcome, VerifyRequest,
    manifest::{PackageDef, PackageManifest},
    remote_registry::RegistryClient,
    trust::TrustLevel,
};

// ── Test helpers ──────────────────────────────────────────────────────────

/// A deterministic publisher keypair (Ed25519 over fixed bytes).
fn publisher_keypair() -> PackageKeypair {
    PackageKeypair::from_bytes(&[0x42; 32])
}

/// A second deterministic keypair for wrong-key tests.
fn attacker_keypair() -> PackageKeypair {
    PackageKeypair::from_bytes(&[0x7f; 32])
}

/// Minimal `PackageManifest` for lifecycle testing.
fn make_manifest(name: &str, version: &str) -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: name.to_string(),
        version: version.to_string(),
        trust_level: TrustLevel::Assumed,
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
        reproducible_evidence: None,
    })
}

// ── Lifecycle scenario 1: happy path ─────────────────────────────────────
//
// Spec scenario: "Full lifecycle: sign → publish → fetch → verify signature →
// verify hash"
//   GIVEN a publisher keypair and a package manifest
//   WHEN the manifest is signed, published, fetched, and its hash verified
//   THEN every step succeeds with no error and no advisory/yank flags
#[test]
fn lifecycle_sign_publish_fetch_verify_ok() {
    let kp = publisher_keypair();
    let manifest = make_manifest("payments.stripe", "1.0.0");
    let expected_hash = manifest.blake3_hex().expect("manifest hash must succeed");

    // 1. Sign the manifest.
    let signed = kp.sign_manifest(manifest).expect("sign must succeed");

    // 2. Verify the signature locally before publishing.
    signed
        .verify()
        .expect("local signature verification must pass");

    // 3. Publish to the registry.
    let client = InMemoryRegistryClient::new();
    let publish_resp = client
        .publish(PublishRequest {
            signed_package: signed.clone(),
        })
        .expect("no transport error");
    assert!(
        publish_resp.accepted,
        "valid signed package must be accepted"
    );
    assert!(publish_resp.error.is_none());
    assert!(publish_resp.log_id.is_some());

    // 4. Fetch from the registry.
    let fetch_resp = client
        .fetch(FetchRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
        })
        .expect("no transport error");
    assert_eq!(fetch_resp.signed_package.as_ref(), Some(&signed));
    assert!(!fetch_resp.yanked, "package must not be yanked");
    assert!(fetch_resp.error.is_none());

    // 5. Verify the fetched signed package signature.
    let fetched = fetch_resp.signed_package.unwrap();
    fetched
        .verify()
        .expect("fetched package signature must still be valid");

    // 6. Verify hash integrity via registry.
    let verify_resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");
    assert_eq!(
        verify_resp.outcome,
        VerifyOutcome::Ok,
        "hash must match and no advisory must be active"
    );
}

// ── Lifecycle scenario 2: yank behavior ───────────────────────────────────
//
// Spec scenario: "Yank blocks verify but preserves fetch and signature validity"
//   GIVEN a published signed package
//   WHEN the package is yanked
//   THEN verify returns Yanked (with the correct reason)
//   AND fetch still returns the signed package with yanked=true
//   AND the fetched package's Ed25519 signature remains valid
#[test]
fn lifecycle_publish_yank_verify_returns_yanked_fetch_preserves_package() {
    let kp = publisher_keypair();
    let manifest = make_manifest("payments.stripe", "1.0.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed = kp.sign_manifest(manifest).expect("sign");

    let mut client = InMemoryRegistryClient::new();
    client
        .publish(PublishRequest {
            signed_package: signed.clone(),
        })
        .expect("publish");

    // Yank the package.
    client.yank("payments.stripe", "1.0.0", "critical security regression");

    // Verify now returns Yanked.
    let verify_resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");
    assert!(
        matches!(
            &verify_resp.outcome,
            VerifyOutcome::Yanked { reason } if reason == "critical security regression"
        ),
        "verify must report Yanked with the correct reason; got {:?}",
        verify_resp.outcome
    );

    // Fetch still returns the package for reproducibility.
    let fetch_resp = client
        .fetch(FetchRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
        })
        .expect("fetch");
    assert_eq!(fetch_resp.signed_package.as_ref(), Some(&signed));
    assert!(fetch_resp.yanked, "fetch must report yanked=true");

    // Signature is still cryptographically valid after yank.
    fetch_resp
        .signed_package
        .unwrap()
        .verify()
        .expect("signature must remain valid after yank");
}

// ── Lifecycle scenario 3: advisory behavior ───────────────────────────────
//
// Spec scenario: "Advisory blocks verify but preserves fetch and signature"
//   GIVEN a published signed package
//   WHEN a security advisory is added for that package version
//   THEN verify returns Advisory with the correct advisory_id and severity
//   AND fetch still returns the signed package (advisory does not block retrieval)
//   AND the fetched package's signature remains valid
#[test]
fn lifecycle_publish_advisory_verify_returns_advisory_fetch_still_works() {
    let kp = publisher_keypair();
    let manifest = make_manifest("payments.stripe", "1.0.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed = kp.sign_manifest(manifest).expect("sign");

    let mut client = InMemoryRegistryClient::new();
    client
        .publish(PublishRequest {
            signed_package: signed.clone(),
        })
        .expect("publish");

    // Add a critical advisory for this version.
    client.add_advisory(SecurityAdvisory {
        id: "adv_stripe_001".to_string(),
        package: "payments.stripe".to_string(),
        affected_constraint: "1.0.0".to_string(),
        severity: AdvisorySeverity::Critical,
        reason: "idempotency handler bypass".to_string(),
    });

    // Verify returns Advisory.
    let verify_resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");
    assert!(
        matches!(
            &verify_resp.outcome,
            VerifyOutcome::Advisory { advisory_id, severity }
                if advisory_id == "adv_stripe_001" && *severity == AdvisorySeverity::Critical
        ),
        "verify must report Advisory; got {:?}",
        verify_resp.outcome
    );

    // Fetch is unaffected by the advisory.
    let fetch_resp = client
        .fetch(FetchRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
        })
        .expect("fetch");
    assert_eq!(fetch_resp.signed_package.as_ref(), Some(&signed));
    assert!(!fetch_resp.yanked);

    // Signature remains valid.
    fetch_resp
        .signed_package
        .unwrap()
        .verify()
        .expect("signature must remain valid under advisory");
}

// ── Lifecycle scenario 4: tampered package rejected ───────────────────────
//
// Spec scenario: "Tampered package is rejected at publish; verify returns NotFound"
//   GIVEN a signed package whose manifest is mutated after signing
//   WHEN the tampered package is published
//   THEN the registry rejects it (accepted=false)
//   AND verify returns NotFound (nothing was stored)
//   AND the tampered signed package's own .verify() returns SignatureInvalid
#[test]
fn lifecycle_tampered_package_rejected_at_publish_and_not_found_on_verify() {
    let kp = publisher_keypair();
    let manifest = make_manifest("payments.stripe", "1.0.0");
    let mut signed = kp.sign_manifest(manifest).expect("sign");

    // Tamper: mutate version after signing.
    signed.manifest.version = "9.9.9".to_string();

    // Signature verification fails locally.
    assert!(
        signed.verify().is_err(),
        "tampered package must fail local signature verification"
    );

    // Publish is rejected.
    let client = InMemoryRegistryClient::new();
    let publish_resp = client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("no transport error");
    assert!(!publish_resp.accepted, "tampered package must be rejected");
    assert!(publish_resp.error.is_some());

    // Nothing was stored — verify returns NotFound.
    let verify_resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "9.9.9".to_string(),
            expected_hash: "a".repeat(64),
        })
        .expect("no transport error");
    assert_eq!(
        verify_resp.outcome,
        VerifyOutcome::NotFound,
        "rejected package must not appear in registry"
    );
}

// ── Lifecycle scenario 5: yank takes priority over advisory ───────────────
//
// Spec contract: "A yank is a permanent, hard removal signal.  When both a
//   yank record and an active advisory exist for the same package version,
//   the verify outcome MUST be Yanked — not Advisory.  Yank subsumes any
//   advisory because the version is categorically withdrawn from use."
//   GIVEN a published package with both an active advisory AND a yank record
//   WHEN verify is called
//   THEN the outcome is Yanked (not Advisory)
#[test]
fn lifecycle_yank_takes_priority_over_advisory_in_verify() {
    let kp = publisher_keypair();
    let manifest = make_manifest("payments.stripe", "1.0.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed = kp.sign_manifest(manifest).expect("sign");

    let mut client = InMemoryRegistryClient::new();
    client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("publish");

    client.add_advisory(SecurityAdvisory {
        id: "adv_stripe_002".to_string(),
        package: "payments.stripe".to_string(),
        affected_constraint: "1.0.0".to_string(),
        severity: AdvisorySeverity::High,
        reason: "regression".to_string(),
    });
    client.yank("payments.stripe", "1.0.0", "superseded by yank");

    let verify_resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");

    assert!(
        matches!(verify_resp.outcome, VerifyOutcome::Yanked { .. }),
        "Yanked must take priority over Advisory; got {:?}",
        verify_resp.outcome
    );
}

// ── Lifecycle scenario 6: re-publish after tamper attempt ─────────────────
//
// Spec scenario: "Valid package can be published after a failed tampered attempt"
//   GIVEN a tampered publish attempt (rejected) followed by a valid publish
//   WHEN verify is called with the correct hash
//   THEN the outcome is Ok (the valid package is stored correctly)
#[test]
fn lifecycle_valid_publish_after_rejected_tamper_succeeds() {
    let kp = publisher_keypair();
    let manifest = make_manifest("utils.core", "0.2.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed_valid = kp.sign_manifest(manifest).expect("sign valid");

    // Build and tamper a different signed copy.
    let manifest_tampered = make_manifest("utils.core", "0.2.0");
    let mut signed_tampered = kp.sign_manifest(manifest_tampered).expect("sign");
    signed_tampered.manifest.version = "0.3.0".to_string(); // tamper

    let client = InMemoryRegistryClient::new();

    // Tampered publish rejected.
    let bad = client
        .publish(PublishRequest {
            signed_package: signed_tampered,
        })
        .expect("no transport error");
    assert!(!bad.accepted);

    // Valid publish accepted.
    let good = client
        .publish(PublishRequest {
            signed_package: signed_valid,
        })
        .expect("no transport error");
    assert!(good.accepted);

    // Verify returns Ok for the valid package.
    let verify_resp = client
        .verify(VerifyRequest {
            name: "utils.core".to_string(),
            version: "0.2.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");
    assert_eq!(verify_resp.outcome, VerifyOutcome::Ok);
}

// ── Lifecycle scenario 7: transparency log records publication ────────────
//
// Spec scenario: "Transparency log records each valid publication"
//   GIVEN two valid signed packages published sequentially
//   WHEN each is appended to a transparency log
//   THEN both entries are recorded with monotonically increasing sequence numbers
//   AND the transparency log rejects a tampered package
#[test]
fn lifecycle_transparency_log_records_sequential_publications() {
    use ail_package::signing::TransparencyLog;

    let kp = publisher_keypair();
    let m1 = make_manifest("payments.stripe", "1.0.0");
    let m2 = make_manifest("payments.stripe", "2.0.0");

    let signed1 = kp.sign_manifest(m1).expect("sign v1");
    let signed2 = kp.sign_manifest(m2).expect("sign v2");

    let mut log = TransparencyLog::new();
    assert!(log.is_empty());

    let entry1 = log.append("log-001", &signed1).expect("append v1");
    assert_eq!(entry1.sequence, 0);
    assert_eq!(entry1.package_name, "payments.stripe");
    assert_eq!(entry1.package_version, "1.0.0");

    let entry2 = log.append("log-002", &signed2).expect("append v2");
    assert_eq!(entry2.sequence, 1);
    assert_eq!(entry2.package_version, "2.0.0");

    assert_eq!(log.len(), 2);

    // Tampered package is rejected.
    let mut tampered = kp
        .sign_manifest(make_manifest("payments.stripe", "3.0.0"))
        .expect("sign");
    tampered.manifest.version = "3.0.1".to_string(); // tamper after signing

    let result = log.append("log-003", &tampered);
    assert!(
        result.is_err(),
        "tampered package must be rejected by transparency log"
    );
    assert_eq!(log.len(), 2, "failed append must not grow the log");
}

// ── Lifecycle scenario 8: advisory version range lifecycle ────────────────
//
// Spec scenario: "Advisory with semver range covers old versions, not newer ones"
//   GIVEN a registry with v1.0.0 (vulnerable) and v1.3.0 (patched)
//   AND an advisory for versions `<1.2.0`
//   WHEN verify is called for v1.0.0 and v1.3.0
//   THEN v1.0.0 returns Advisory and v1.3.0 returns Ok
#[test]
fn lifecycle_advisory_semver_range_covers_old_not_new() {
    let kp = publisher_keypair();

    let manifest_v1 = make_manifest("lib.auth", "1.0.0");
    let hash_v1 = manifest_v1.blake3_hex().expect("hash v1");
    let signed_v1 = kp.sign_manifest(manifest_v1).expect("sign v1");

    let manifest_v13 = make_manifest("lib.auth", "1.3.0");
    let hash_v13 = manifest_v13.blake3_hex().expect("hash v13");
    let signed_v13 = kp.sign_manifest(manifest_v13).expect("sign v13");

    let mut client = InMemoryRegistryClient::new();
    client
        .publish(PublishRequest {
            signed_package: signed_v1,
        })
        .expect("publish v1");
    client
        .publish(PublishRequest {
            signed_package: signed_v13,
        })
        .expect("publish v13");

    // Advisory covers all versions < 1.2.0.
    client.add_advisory(SecurityAdvisory {
        id: "adv_auth_range".to_string(),
        package: "lib.auth".to_string(),
        affected_constraint: "<1.2.0".to_string(),
        severity: AdvisorySeverity::High,
        reason: "auth bypass in older versions".to_string(),
    });

    // v1.0.0 is affected.
    let resp_v1 = client
        .verify(VerifyRequest {
            name: "lib.auth".to_string(),
            version: "1.0.0".to_string(),
            expected_hash: hash_v1,
        })
        .expect("no transport error");
    assert!(
        matches!(resp_v1.outcome, VerifyOutcome::Advisory { .. }),
        "v1.0.0 must be covered by advisory; got {:?}",
        resp_v1.outcome
    );

    // v1.3.0 is NOT affected.
    let resp_v13 = client
        .verify(VerifyRequest {
            name: "lib.auth".to_string(),
            version: "1.3.0".to_string(),
            expected_hash: hash_v13,
        })
        .expect("no transport error");
    assert_eq!(
        resp_v13.outcome,
        VerifyOutcome::Ok,
        "v1.3.0 must not be covered by advisory for <1.2.0"
    );
}

// ── Lifecycle scenario 9: wrong signer key is rejected ────────────────────
//
// Spec scenario: "Package signed by attacker key fails local verification and
// is rejected at publish"
//   GIVEN a manifest signed by an attacker keypair
//   WHEN the signer field is replaced with the publisher's public key
//     (simulating key forgery)
//   THEN local verify returns SignatureInvalid
//   AND publish rejects the package
#[test]
fn lifecycle_wrong_signer_key_rejected_at_publish() {
    let attacker = attacker_keypair();
    let publisher = publisher_keypair();

    let manifest = make_manifest("secure.pkg", "1.0.0");
    // Attacker signs with their key but forges the signer field.
    let mut signed = attacker.sign_manifest(manifest).expect("attacker sign");
    // Forge: replace signer bytes with publisher's public key.
    signed.sig.signer = publisher.public_key();

    // Local verification fails.
    assert!(
        signed.verify().is_err(),
        "forged signer must fail local verification"
    );

    // Publish is rejected.
    let client = InMemoryRegistryClient::new();
    let resp = client
        .publish(PublishRequest {
            signed_package: signed,
        })
        .expect("no transport error");
    assert!(!resp.accepted, "forged package must not be accepted");
}

// ── Lifecycle scenario 10: advisory checker standalone ────────────────────
//
// Spec scenario: "AdvisoryChecker works stand-alone against a fetched manifest"
//   GIVEN a fetched manifest and an advisory list
//   WHEN AdvisoryChecker::is_affected is called
//   THEN it correctly mirrors the registry verify result
#[test]
fn lifecycle_advisory_checker_mirrors_registry_verify() {
    let kp = publisher_keypair();
    let manifest = make_manifest("payments.stripe", "1.0.0");
    let expected_hash = manifest.blake3_hex().expect("hash");
    let signed = kp.sign_manifest(manifest).expect("sign");

    let mut client = InMemoryRegistryClient::new();
    client
        .publish(PublishRequest {
            signed_package: signed.clone(),
        })
        .expect("publish");

    let advisory = SecurityAdvisory {
        id: "adv_standalone".to_string(),
        package: "payments.stripe".to_string(),
        affected_constraint: "1.0.0".to_string(),
        severity: AdvisorySeverity::Medium,
        reason: "standalone check".to_string(),
    };

    // Before advisory — checker says not affected.
    assert!(!AdvisoryChecker::is_affected(
        "payments.stripe",
        "1.0.0",
        &[]
    ));

    // After advisory — checker says affected.
    assert!(AdvisoryChecker::is_affected(
        "payments.stripe",
        "1.0.0",
        std::slice::from_ref(&advisory)
    ));

    // Add to client and confirm registry verify agrees.
    client.add_advisory(advisory);
    let verify_resp = client
        .verify(VerifyRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            expected_hash,
        })
        .expect("no transport error");
    assert!(
        matches!(verify_resp.outcome, VerifyOutcome::Advisory { .. }),
        "registry verify must agree with advisory checker"
    );

    // Fetched package signature is still valid even under advisory.
    let fetch_resp = client
        .fetch(FetchRequest {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
        })
        .expect("fetch");
    fetch_resp
        .signed_package
        .unwrap()
        .verify()
        .expect("signature must remain valid under advisory");
}
