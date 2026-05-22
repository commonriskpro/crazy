// ── report_extended_tests.rs ─────────────────────────────────────────────
//
// TDD tests for RuntimeReport missing fields (WARNING → must fix).
//
// Per runtime.md §"Runtime report":
//   runtime_report <id>
//   profile prod
//   module module.checkout          ← module name (string identity)
//   verification_report hash=ver_abc123  ← verification_report_hash
//   status completed | failed | denied | timeout | limit_exceeded
//   capability_calls ... end
//   runtime_checks ... end          ← runtime_checks
//   limits ... end                  ← limits
//   audit_log hash=audit_123        ← audit_log_hash
//   end

use ail_runtime::report::{
    LimitSnapshot, RuntimeCheck, RuntimeCheckResult, RuntimeReport, RuntimeReportStatus,
};
use ail_runtime::profile::CapabilityId;

// ── RuntimeCheck ─────────────────────────────────────────────────────────

#[test]
fn runtime_check_carries_name_and_result() {
    let check = RuntimeCheck {
        check_name: "decoder_validation".to_string(),
        capability: Some(CapabilityId::new("database.read:Cart")),
        result: RuntimeCheckResult::Passed,
    };
    assert_eq!(check.check_name, "decoder_validation");
    assert_eq!(check.result, RuntimeCheckResult::Passed);
}

#[test]
fn runtime_check_result_variants() {
    assert_ne!(RuntimeCheckResult::Passed, RuntimeCheckResult::Failed);
    assert_ne!(RuntimeCheckResult::Failed, RuntimeCheckResult::Skipped);
}

// ── LimitSnapshot ─────────────────────────────────────────────────────────

#[test]
fn limit_snapshot_carries_configured_and_used() {
    let snap = LimitSnapshot {
        limit_name: "timeout".to_string(),
        configured: Some("5s".to_string()),
        used: Some("1.2s".to_string()),
    };
    assert_eq!(snap.limit_name, "timeout");
    assert_eq!(snap.configured.as_deref(), Some("5s"));
    assert_eq!(snap.used.as_deref(), Some("1.2s"));
}

// ── RuntimeReport extended fields ─────────────────────────────────────────

#[test]
fn runtime_report_has_verification_report_hash() {
    let report = RuntimeReport::new(
        "rep-001".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash-abc".to_string(),
        RuntimeReportStatus::Completed,
    );
    // New constructor includes module_name; verification_report_hash set separately
    let report = report.with_verification_report_hash("ver_abc123".to_string());
    assert_eq!(report.verification_report_hash(), "ver_abc123");
}

#[test]
fn runtime_report_has_module_name() {
    let report = RuntimeReport::new(
        "rep-002".to_string(),
        "test".to_string(),
        "module.checkout".to_string(),
        "wasm-hash-def".to_string(),
        RuntimeReportStatus::Completed,
    );
    assert_eq!(report.module_name(), "module.checkout");
}

#[test]
fn runtime_report_has_runtime_checks() {
    let report = RuntimeReport::new(
        "rep-003".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash".to_string(),
        RuntimeReportStatus::Completed,
    );

    let checks = vec![
        RuntimeCheck {
            check_name: "decoder_validation".to_string(),
            capability: Some(CapabilityId::new("database.read:Cart")),
            result: RuntimeCheckResult::Passed,
        },
        RuntimeCheck {
            check_name: "refinement_check".to_string(),
            capability: None,
            result: RuntimeCheckResult::Passed,
        },
    ];

    let report = report.with_runtime_checks(checks);
    assert_eq!(report.runtime_checks().len(), 2);
    assert_eq!(report.runtime_checks()[0].check_name, "decoder_validation");
    assert_eq!(report.runtime_checks()[1].check_name, "refinement_check");
}

#[test]
fn runtime_report_has_limits() {
    let report = RuntimeReport::new(
        "rep-004".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash".to_string(),
        RuntimeReportStatus::Completed,
    );

    let limits = vec![
        LimitSnapshot {
            limit_name: "timeout".to_string(),
            configured: Some("5s".to_string()),
            used: Some("1.2s".to_string()),
        },
        LimitSnapshot {
            limit_name: "memory".to_string(),
            configured: Some("128MiB".to_string()),
            used: Some("32MiB".to_string()),
        },
    ];

    let report = report.with_limits(limits);
    assert_eq!(report.limits().len(), 2);
    assert_eq!(report.limits()[0].limit_name, "timeout");
}

#[test]
fn runtime_report_has_audit_log_hash() {
    let report = RuntimeReport::new(
        "rep-005".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash".to_string(),
        RuntimeReportStatus::Completed,
    );

    let report = report.with_audit_log_hash("audit_abc123".to_string());
    assert_eq!(report.audit_log_hash(), Some("audit_abc123"));
}

#[test]
fn runtime_report_audit_log_hash_is_none_by_default() {
    let report = RuntimeReport::new(
        "rep-006".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash".to_string(),
        RuntimeReportStatus::Completed,
    );
    assert!(report.audit_log_hash().is_none());
}

#[test]
fn runtime_report_runtime_checks_empty_by_default() {
    let report = RuntimeReport::new(
        "rep-007".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash".to_string(),
        RuntimeReportStatus::Completed,
    );
    assert!(report.runtime_checks().is_empty());
}

#[test]
fn runtime_report_limits_empty_by_default() {
    let report = RuntimeReport::new(
        "rep-008".to_string(),
        "prod".to_string(),
        "module.checkout".to_string(),
        "wasm-hash".to_string(),
        RuntimeReportStatus::Completed,
    );
    assert!(report.limits().is_empty());
}

// ── RuntimeHost::emit_report emits extended fields ─────────────────────────

#[test]
fn emit_report_includes_module_name_and_vr_hash() {
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{ResourceLimits, RuntimeProfile};

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let manifest = CapabilityManifest {
        module: "module.checkout".to_string(),
        requires: vec![],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let profile = RuntimeProfile::new(
        "prod".to_string(),
        module_hash,
        "ver_abc123".to_string(),  // verification_report_hash
        manifest_hash,
        vec![],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let report = host.emit_report(RuntimeReportStatus::Completed, "rep-001");

    // The report should carry the module name from the manifest
    assert_eq!(report.module_name(), "module.checkout");
    // The report should carry the verification_report_hash from the profile
    assert_eq!(report.verification_report_hash(), "ver_abc123");
}

#[test]
fn emit_report_includes_audit_log_hash() {
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{ResourceLimits, RuntimeProfile};

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let manifest = CapabilityManifest {
        module: "module.test".to_string(),
        requires: vec![],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let report = host.emit_report(RuntimeReportStatus::Completed, "rep-002");

    // Once any audit events exist, the audit_log_hash must be populated
    assert!(
        report.audit_log_hash().is_some(),
        "emit_report must include audit_log_hash when events exist"
    );
    let hash = report.audit_log_hash().unwrap();
    assert!(!hash.is_empty(), "audit_log_hash must not be empty");
}
