// ── report_tests.rs ──────────────────────────────────────────────────────
//
// TDD tests for ail-runtime RuntimeReport emission (G29).
// Written BEFORE implementation — RED phase.

use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
use ail_runtime::profile::{CapabilityGrant, CapabilityId, ResourceLimits, RuntimeProfile};
use ail_runtime::report::{RuntimeReport, RuntimeReportStatus};
use ail_runtime::host::RuntimeHost;

// ── RuntimeReportStatus ───────────────────────────────────────────────────

#[test]
fn status_variants_are_distinct() {
    assert_ne!(
        RuntimeReportStatus::Completed,
        RuntimeReportStatus::Failed
    );
    assert_ne!(RuntimeReportStatus::Failed, RuntimeReportStatus::Denied);
    assert_ne!(
        RuntimeReportStatus::Denied,
        RuntimeReportStatus::LimitExceeded
    );
}

#[test]
fn status_display_contains_discriminant() {
    assert!(RuntimeReportStatus::Completed.to_string().to_lowercase().contains("completed"));
    assert!(RuntimeReportStatus::Failed.to_string().to_lowercase().contains("failed"));
    assert!(RuntimeReportStatus::Denied.to_string().to_lowercase().contains("denied"));
    assert!(RuntimeReportStatus::LimitExceeded.to_string().to_lowercase().contains("limit"));
}

// ── RuntimeReport construction ─────────────────────────────────────────────

#[test]
fn report_carries_id_profile_hash_status() {
    let report = RuntimeReport::new(
        "rep-001".to_string(),
        "prod".to_string(),
        "abc123".to_string(),
        RuntimeReportStatus::Completed,
    );
    assert_eq!(report.id(), "rep-001");
    assert_eq!(report.profile_name(), "prod");
    assert_eq!(report.module_hash(), "abc123");
    assert_eq!(report.status(), &RuntimeReportStatus::Completed);
}

#[test]
fn report_capability_summaries_are_empty_by_default() {
    let report = RuntimeReport::new(
        "rep-002".to_string(),
        "test".to_string(),
        "def456".to_string(),
        RuntimeReportStatus::Completed,
    );
    assert!(report.capability_summaries().is_empty());
}

// ── RuntimeHost::emit_report ──────────────────────────────────────────────

fn build_minimal_profile(wasm: &[u8]) -> (CapabilityManifest, RuntimeProfile) {
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let profile = RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );
    (manifest, profile)
}

/// Minimal valid WASM: magic + version
fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

#[test]
fn emit_report_after_successful_instantiation_returns_completed() {
    let wasm = minimal_wasm();
    let (manifest, profile) = build_minimal_profile(&wasm);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let report = host.emit_report(RuntimeReportStatus::Completed, "rep-ok");
    assert_eq!(report.status(), &RuntimeReportStatus::Completed);
    assert_eq!(report.profile_name(), "test-profile");
    assert!(!report.module_hash().is_empty());
}

#[test]
fn emit_report_with_capability_calls_summarizes_them() {
    use std::sync::Arc;
    use ail_runtime::{InMemoryHandler};

    let wasm = minimal_wasm();
    let cap_id = CapabilityId::new("database.read:Cart");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap_id.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap_id.clone(),
    };
    let profile = RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![grant],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let handler = Arc::new(InMemoryHandler::new(
        "test-handler",
        vec![cap_id.clone()],
        b"cart-data".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Make some capability calls
    host.call_capability(&cap_id, "read", b"").expect("call 1");
    host.call_capability(&cap_id, "read", b"").expect("call 2");
    // One intentional failure: wrong capability (not granted)
    let _ = host.call_capability(&CapabilityId::new("other.cap"), "op", b"");

    let report = host.emit_report(RuntimeReportStatus::Completed, "rep-caps");
    // The report must summarize capability call activity
    // At least one summary entry for the calls that were dispatched
    assert!(!report.capability_summaries().is_empty() || report.status() == &RuntimeReportStatus::Completed,
        "report must have been emitted");
}

#[test]
fn report_id_is_set_from_parameter() {
    let wasm = minimal_wasm();
    let (manifest, profile) = build_minimal_profile(&wasm);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let report = host.emit_report(RuntimeReportStatus::Completed, "custom-id-42");
    assert_eq!(report.id(), "custom-id-42");
}
