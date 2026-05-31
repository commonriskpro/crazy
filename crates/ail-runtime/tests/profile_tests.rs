// ── ail-runtime::profile_tests ────────────────────────────────────────────
//
// TDD — RED phase written before profile.rs existed.
//
// Spec scenarios covered:
//   - Valid profile constructed → all fields accessible
//   - Empty grants denies all capabilities
//   - CapabilityId equality
//   - CapabilityGrant field access
//   - ResourceLimits field access

use ail_runtime::manifest::CapabilityManifest;
use ail_runtime::profile::{
    CapabilityGrant, CapabilityId, CapabilityRevocationRegistry, InFlightPolicy, ResourceLimits,
    RuntimeCapabilityDiagnosticKind, RuntimeProfile, redacted_capability_descriptor,
};

// ── Scenario: Valid profile constructed ───────────────────────────────────

#[test]
fn profile_new_exposes_all_fields() {
    let id = CapabilityId::new("FileRead");
    let grant = CapabilityGrant {
        module: "my-module".to_string(),
        capability: id.clone(),
    };
    let limits = ResourceLimits {
        max_memory_bytes: Some(64 * 1024 * 1024),
        max_fuel: Some(1_000_000),
        ..Default::default()
    };

    let profile = RuntimeProfile::new(
        "test-profile".to_string(),
        "blake3-module-hash-hex".to_string(),
        "blake3-report-hash-hex".to_string(),
        "blake3-manifest-hash-hex".to_string(),
        vec![grant.clone()],
        limits.clone(),
    );

    assert_eq!(profile.name(), "test-profile");
    assert_eq!(profile.module_hash(), "blake3-module-hash-hex");
    assert_eq!(profile.verification_report_hash(), "blake3-report-hash-hex");
    assert_eq!(
        profile.capability_manifest_hash(),
        "blake3-manifest-hash-hex"
    );
    assert_eq!(profile.grants().len(), 1);
    assert_eq!(profile.grants()[0].capability, id);
    assert_eq!(profile.limits().max_memory_bytes, Some(64 * 1024 * 1024));
    assert_eq!(profile.limits().max_fuel, Some(1_000_000));
}

// ── Scenario: CapabilityId equality ──────────────────────────────────────

#[test]
fn capability_id_equality_by_value() {
    let a = CapabilityId::new("FileRead");
    let b = CapabilityId::new("FileRead");
    let c = CapabilityId::new("NetworkEgress");

    assert_eq!(a, b, "same name → equal");
    assert_ne!(a, c, "different name → not equal");
}

// TRIANGULATE: as_str returns the inner name.
#[test]
fn capability_id_as_str_returns_name() {
    let id = CapabilityId::new("DatabaseRead");
    assert_eq!(id.as_str(), "DatabaseRead");
}

// ── Scenario: Empty grants denies all capabilities ────────────────────────
//
// This scenario is exercised in preflight_tests (Phase 3) but the profile
// type must support construction with an empty grants list.

#[test]
fn profile_with_empty_grants_is_constructible() {
    let profile = RuntimeProfile::new(
        "no-grants".to_string(),
        "hash1".to_string(),
        "hash2".to_string(),
        "hash3".to_string(),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );

    assert_eq!(profile.name(), "no-grants");
    assert!(
        profile.grants().is_empty(),
        "profile must preserve empty grants list"
    );
}

// ── CapabilityGrant field access ──────────────────────────────────────────

#[test]
fn capability_grant_exposes_module_and_capability() {
    let id = CapabilityId::new("NetworkEgress");
    let grant = CapabilityGrant {
        module: "net-module".to_string(),
        capability: id.clone(),
    };

    assert_eq!(grant.module, "net-module");
    assert_eq!(grant.capability, id);
}

// ── ResourceLimits field access ───────────────────────────────────────────

#[test]
fn resource_limits_none_is_valid() {
    let limits = ResourceLimits {
        max_memory_bytes: None,
        max_fuel: None,
        ..Default::default()
    };
    assert!(limits.max_memory_bytes.is_none());
    assert!(limits.max_fuel.is_none());
}

// TRIANGULATE: ResourceLimits with values.
#[test]
fn resource_limits_some_is_accessible() {
    let limits = ResourceLimits {
        max_memory_bytes: Some(1024),
        max_fuel: Some(500),
        ..Default::default()
    };
    assert_eq!(limits.max_memory_bytes, Some(1024));
    assert_eq!(limits.max_fuel, Some(500));
}

#[test]
fn revocation_registry_records_returns_borrowed_view() {
    let mut registry = CapabilityRevocationRegistry::new();
    registry.revoke(
        "module",
        "FileRead",
        "profile",
        InFlightPolicy::AllowComplete,
    );

    let records = registry.records();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].module, "module");
    assert_eq!(records[0].capability, "FileRead");
    assert_eq!(records[0].profile, "profile");
}

#[test]
fn cloned_revocation_registry_shares_runtime_revocations() {
    let mut registry = CapabilityRevocationRegistry::new();
    let cloned = registry.clone();

    registry.revoke(
        "module",
        "FileRead",
        "profile",
        InFlightPolicy::AllowComplete,
    );

    assert!(cloned.is_revoked("module", "FileRead", "profile"));
    assert_eq!(cloned.records_snapshot().len(), 1);
}

#[test]
fn redacted_capability_descriptor_hides_targets_and_unsafe_names() {
    assert_eq!(
        redacted_capability_descriptor(&CapabilityId::new("secret.read:ProductionDbPassword")),
        "secret.read:<redacted>"
    );
    assert_eq!(
        redacted_capability_descriptor(&CapabilityId::new("network.egress:private-vpc")),
        "network.egress:<redacted>"
    );
    assert_eq!(
        redacted_capability_descriptor(&CapabilityId::new(
            "TenantAlphaSecret:ProductionDbPassword"
        )),
        "capability:<opaque>"
    );
}

#[test]
fn capability_diagnostics_for_manifest_are_redacted_deduped_and_canonical() {
    let manifest = CapabilityManifest {
        module: "tenant-alpha-private-module".to_string(),
        requires: vec![
            CapabilityId::new("secret.read:ProductionDbPassword"),
            CapabilityId::new("network.egress:private-vpc"),
            CapabilityId::new("secret.read:AnotherProductionSecret"),
            CapabilityId::new("log.write"),
        ],
    };
    let profile = RuntimeProfile::new(
        "prod-tenant-alpha".to_string(),
        "hash1".to_string(),
        "hash2".to_string(),
        "hash3".to_string(),
        vec![CapabilityGrant {
            module: manifest.module.clone(),
            capability: CapabilityId::new("log.write"),
        }],
        ResourceLimits::default(),
    );

    let diagnostics = profile.capability_diagnostics_for_manifest(&manifest);

    assert_eq!(
        diagnostics.len(),
        2,
        "same redacted secret family is deduped"
    );
    assert_eq!(
        diagnostics[0].kind,
        RuntimeCapabilityDiagnosticKind::MissingGrant
    );
    assert_eq!(diagnostics[0].capability, "network.egress:<redacted>");
    assert_eq!(
        diagnostics[1].kind,
        RuntimeCapabilityDiagnosticKind::MissingGrant
    );
    assert_eq!(diagnostics[1].capability, "secret.read:<redacted>");
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.profile, "profile:<active>");
        assert_eq!(diagnostic.module, "module:<bound>");
        assert!(!diagnostic.capability.contains("ProductionDbPassword"));
        assert!(!diagnostic.capability.contains("private-vpc"));
        assert!(!diagnostic.capability.contains("prod-tenant-alpha"));
        assert!(
            !diagnostic
                .capability
                .contains("tenant-alpha-private-module")
        );
    }
}

#[test]
fn capability_access_diagnostics_classify_ambient_mismatch_and_denied_accesses() {
    let secret_cap = CapabilityId::new("secret.read:ProductionDbPassword");
    let network_cap = CapabilityId::new("network.egress:private-vpc");
    let payment_cap = CapabilityId::new("payment.charge:customer-card");
    let profile = RuntimeProfile::new(
        "prod-tenant-alpha".to_string(),
        "hash1".to_string(),
        "hash2".to_string(),
        "hash3".to_string(),
        vec![CapabilityGrant {
            module: "checkout".to_string(),
            capability: secret_cap.clone(),
        }],
        ResourceLimits::default(),
    );

    let ambient = profile
        .capability_diagnostic_for_access(None, &secret_cap)
        .expect("missing module binding must be diagnostic");
    let mismatch = profile
        .capability_diagnostic_for_access(Some("admin"), &secret_cap)
        .expect("grant for a different module must be diagnostic");
    let denied = profile
        .capability_diagnostic_for_access(Some("checkout"), &network_cap)
        .expect("ungranted capability must be diagnostic");
    let granted = profile.capability_diagnostic_for_access(Some("checkout"), &secret_cap);

    assert_eq!(
        ambient.kind,
        RuntimeCapabilityDiagnosticKind::AmbientAccessAttempt
    );
    assert_eq!(ambient.module, "module:<ambient>");
    assert_eq!(
        mismatch.kind,
        RuntimeCapabilityDiagnosticKind::ProfileMismatch
    );
    assert_eq!(
        denied.kind,
        RuntimeCapabilityDiagnosticKind::DeniedCapability
    );
    assert!(granted.is_none());

    let batch = profile.capability_diagnostics_for_accesses([
        (Some("checkout"), &network_cap),
        (None, &secret_cap),
        (Some("admin"), &secret_cap),
        (Some("checkout"), &payment_cap),
    ]);
    let kinds: Vec<_> = batch.iter().map(|diagnostic| diagnostic.kind).collect();

    assert_eq!(
        kinds,
        vec![
            RuntimeCapabilityDiagnosticKind::AmbientAccessAttempt,
            RuntimeCapabilityDiagnosticKind::ProfileMismatch,
            RuntimeCapabilityDiagnosticKind::DeniedCapability,
            RuntimeCapabilityDiagnosticKind::DeniedCapability,
        ],
        "batch diagnostics must use canonical production-safety ordering"
    );
    for diagnostic in batch {
        assert!(!diagnostic.capability.contains("ProductionDbPassword"));
        assert!(!diagnostic.capability.contains("private-vpc"));
        assert!(!diagnostic.capability.contains("customer-card"));
        assert!(!diagnostic.profile.contains("prod-tenant-alpha"));
        assert!(!diagnostic.module.contains("checkout"));
        assert!(!diagnostic.module.contains("admin"));
    }
}
