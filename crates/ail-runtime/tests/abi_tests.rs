// ── ail-runtime::abi_tests ────────────────────────────────────────────────
//
// TDD — RED phase written before ABI type behavioral verification.
//
// Spec: HostCallId and HostResult<T> MUST be referenceable and compile
// without registered handlers (dormant in Phase 8 PR 1).
//
// Triangulation: test structural properties and type-level behavior.

use ail_runtime::abi::{HostCallId, HostError, HostResult};
use ail_runtime::profile::CapabilityId;

// ── Scenario: Host ABI types compile without handlers ─────────────────────
//
// This test verifies that HostCallId is constructible and matchable.

#[test]
fn host_call_id_capability_is_constructible() {
    let id = HostCallId::Capability {
        capability: CapabilityId::new("FileRead"),
        operation: "read".to_string(),
    };

    match &id {
        HostCallId::Capability {
            capability,
            operation,
        } => {
            assert_eq!(capability.as_str(), "FileRead");
            assert_eq!(operation, "read");
        }
    }
}

// TRIANGULATE: different HostCallId values are distinguishable by field.
#[test]
fn host_call_id_fields_are_distinguishable() {
    let read_id = HostCallId::Capability {
        capability: CapabilityId::new("FileRead"),
        operation: "read".to_string(),
    };
    let write_id = HostCallId::Capability {
        capability: CapabilityId::new("FileRead"),
        operation: "write".to_string(),
    };

    // Different operations on the same capability produce distinct IDs.
    assert_ne!(
        read_id, write_id,
        "different operations must produce different IDs"
    );
}

// ── HostResult<T> is usable without registered handlers ───────────────────

#[test]
fn host_result_ok_carries_value() {
    let result: HostResult<u32> = Ok(42);
    assert_eq!(result, Ok(42));
}

// TRIANGULATE: HostResult::Err carries the HostError message.
#[test]
fn host_result_err_carries_host_error() {
    let err = HostError {
        message: "file not found".to_string(),
    };
    let result: HostResult<u32> = Err(err);
    match result {
        Err(e) => assert_eq!(e.message, "file not found"),
        Ok(_) => panic!("expected Err, got Ok"),
    }
}

// ── HostError Display ─────────────────────────────────────────────────────

#[test]
fn host_error_display_includes_message() {
    let err = HostError {
        message: "permission denied".to_string(),
    };
    let s = err.to_string();
    assert!(
        s.contains("permission denied"),
        "Display must include the message: got '{s}'"
    );
}

// TRIANGULATE: HostCallId equality respects both capability and operation.
#[test]
fn host_call_id_equality_respects_both_fields() {
    let a = HostCallId::Capability {
        capability: CapabilityId::new("NetworkEgress"),
        operation: "connect".to_string(),
    };
    let b = HostCallId::Capability {
        capability: CapabilityId::new("NetworkEgress"),
        operation: "connect".to_string(),
    };
    let c = HostCallId::Capability {
        capability: CapabilityId::new("FileRead"),
        operation: "connect".to_string(),
    };

    assert_eq!(a, b, "same capability + operation → equal");
    assert_ne!(a, c, "different capability → not equal");
}
