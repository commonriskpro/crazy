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

use ail_runtime::profile::{
    CapabilityGrant, CapabilityId, CapabilityRevocationRegistry, InFlightPolicy, ResourceLimits,
    RuntimeProfile,
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
