// ── ail-runtime::audit_tests ─────────────────────────────────────────────
//
// TDD — RED phase written before final behavioral verification of audit.rs.
//
// Spec scenarios covered:
//   - Pass emits PreflightPassed (exactly one event)
//   - Fail emits PreflightFailed (exactly one event)
//   - Log preserves insertion order (pass then fail → correct ordering)
//   - Payload contains no raw WASM bytes (structural + field check)
//   - AuditEvent::is_passed() discriminates correctly

use ail_runtime::audit::{
    AuditEvent, AuditLog, DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED,
    DENIAL_CATEGORY_CAPABILITY_REVOKED, DENIAL_CATEGORY_HANDLER_NOT_BOUND,
    DENIAL_CATEGORY_LIMIT_MEMORY, DENIAL_CATEGORY_LIMIT_RATE, LIMIT_DENIAL_DIAGNOSTIC_KEY_FUEL,
    LIMIT_DENIAL_DIAGNOSTIC_KEY_MEMORY, LIMIT_DENIAL_DIAGNOSTIC_KEY_RATE,
    LIMIT_DENIAL_DIAGNOSTIC_KEY_TIME, LIMIT_DENIAL_SHAPE_FUEL, LIMIT_DENIAL_SHAPE_MEMORY,
    LIMIT_DENIAL_SHAPE_RATE, LIMIT_DENIAL_SHAPE_TIME,
    PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED,
    PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_REVOKED,
    PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED,
    PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_REVOKED, RUNTIME_ISSUE_DIAGNOSTIC_KEY_RESOURCE_POLICY,
    RUNTIME_ISSUE_SHAPE_RESOURCE_POLICY, RuntimeIssueAxis, RuntimeIssueDescriptor,
    runtime_issue_descriptors_for_events,
};
use ail_runtime::error::PreflightFailure;
use ail_runtime::profile::CapabilityId;

// ── Scenario: Pass emits PreflightPassed (exactly one event) ──────────────

#[test]
fn push_passed_event_results_in_one_log_entry() {
    let mut log = AuditLog::new();
    assert_eq!(log.len(), 0, "fresh log must be empty");

    log.push(AuditEvent::PreflightPassed {
        profile_name: "test-profile".to_string(),
        module_hash: "abc123".to_string(),
    });

    assert_eq!(log.len(), 1, "exactly one event after one push");
    assert!(log.events()[0].is_passed(), "event must be PreflightPassed");
}

// ── Scenario: Fail emits PreflightFailed (exactly one event) ─────────────

#[test]
fn push_failed_event_results_in_one_log_entry() {
    let mut log = AuditLog::new();
    let denied = vec![CapabilityId::new("NetworkEgress")];

    log.push(AuditEvent::PreflightFailed {
        profile_name: "test-profile".to_string(),
        denied: denied.clone(),
        reason: PreflightFailure::CapabilityDenied {
            denied: denied.clone(),
        },
    });

    assert_eq!(log.len(), 1, "exactly one event after one push");
    assert!(
        !log.events()[0].is_passed(),
        "event must be PreflightFailed"
    );
}

// ── Scenario: Log preserves insertion order ───────────────────────────────
//
// GIVEN two sequential preflights (pass then fail)
// WHEN the log is read
// THEN events appear in emission order

#[test]
fn log_preserves_insertion_order() {
    let mut log = AuditLog::new();

    log.push(AuditEvent::PreflightPassed {
        profile_name: "first".to_string(),
        module_hash: "hash-a".to_string(),
    });

    log.push(AuditEvent::PreflightFailed {
        profile_name: "second".to_string(),
        denied: vec![],
        reason: PreflightFailure::HashMismatch {
            expected: "e".to_string(),
            actual: "a".to_string(),
        },
    });

    assert_eq!(log.len(), 2, "two events after two pushes");
    assert!(
        log.events()[0].is_passed(),
        "first event (index 0) must be the pass"
    );
    assert!(
        !log.events()[1].is_passed(),
        "second event (index 1) must be the fail"
    );
}

// TRIANGULATE: three events — fail, pass, fail — preserved in order.
#[test]
fn log_preserves_order_for_three_events() {
    let mut log = AuditLog::new();

    let make_fail = |name: &str| AuditEvent::PreflightFailed {
        profile_name: name.to_string(),
        denied: vec![CapabilityId::new("X")],
        reason: PreflightFailure::CapabilityDenied {
            denied: vec![CapabilityId::new("X")],
        },
    };
    let make_pass = |name: &str| AuditEvent::PreflightPassed {
        profile_name: name.to_string(),
        module_hash: "h".to_string(),
    };

    log.push(make_fail("first"));
    log.push(make_pass("second"));
    log.push(make_fail("third"));

    assert_eq!(log.len(), 3);
    assert!(!log.events()[0].is_passed(), "index 0: fail");
    assert!(log.events()[1].is_passed(), "index 1: pass");
    assert!(!log.events()[2].is_passed(), "index 2: fail");
}

// ── Scenario: Payload contains no raw WASM bytes ─────────────────────────
//
// GIVEN a PreflightFailed event
// WHEN we inspect the payload fields
// THEN payload contains only hash digests and denied capability names
// (no Vec<u8> raw bytes — this is a structural contract of the type)

#[test]
fn preflight_failed_payload_carries_only_hash_and_names() {
    let module_hash_expected = "aabbcc".to_string();
    let module_hash_actual = "ddeeff".to_string();

    let event = AuditEvent::PreflightFailed {
        profile_name: "my-profile".to_string(),
        denied: vec![],
        reason: PreflightFailure::HashMismatch {
            expected: module_hash_expected.clone(),
            actual: module_hash_actual.clone(),
        },
    };

    // Verify fields contain hash strings, not raw bytes.
    match &event {
        AuditEvent::PreflightFailed {
            profile_name,
            denied,
            reason,
        } => {
            assert_eq!(profile_name, "my-profile");
            assert!(denied.is_empty(), "no denied caps for hash mismatch");
            match reason {
                PreflightFailure::HashMismatch { expected, actual } => {
                    // These are string hashes, NOT Vec<u8>
                    assert_eq!(expected, &module_hash_expected);
                    assert_eq!(actual, &module_hash_actual);
                }
                other => panic!("expected HashMismatch reason, got: {other:?}"),
            }
        }
        other => panic!("expected PreflightFailed, got: {other:?}"),
    }
}

// TRIANGULATE: PreflightPassed payload carries profile name and hash only.
#[test]
fn preflight_passed_payload_carries_profile_name_and_hash() {
    let event = AuditEvent::PreflightPassed {
        profile_name: "production-profile".to_string(),
        module_hash: "blake3hexdigest".to_string(),
    };

    match &event {
        AuditEvent::PreflightPassed {
            profile_name,
            module_hash,
        } => {
            assert_eq!(profile_name, "production-profile");
            assert_eq!(module_hash, "blake3hexdigest");
        }
        other => panic!("expected PreflightPassed, got: {other:?}"),
    }
}

// ── AuditLog::is_empty ────────────────────────────────────────────────────

#[test]
fn audit_log_is_empty_initially() {
    let log = AuditLog::new();
    assert!(log.is_empty(), "fresh log must report is_empty() == true");
}

#[test]
fn audit_log_is_not_empty_after_push() {
    let mut log = AuditLog::new();
    log.push(AuditEvent::PreflightPassed {
        profile_name: "p".to_string(),
        module_hash: "h".to_string(),
    });
    assert!(!log.is_empty());
}

#[test]
fn preflight_capability_denial_has_stable_redacted_profile_policy_shape() {
    let event = AuditEvent::PreflightFailed {
        profile_name: "prod-tenant-alpha".to_string(),
        denied: vec![
            CapabilityId::new("secret.read:ProductionDbPassword"),
            CapabilityId::new("network.egress:private-vpc"),
        ],
        reason: PreflightFailure::CapabilityDenied {
            denied: vec![
                CapabilityId::new("secret.read:ProductionDbPassword"),
                CapabilityId::new("network.egress:private-vpc"),
            ],
        },
    };

    assert_eq!(
        event.profile_policy_denial_shape(),
        Some(PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED)
    );
    assert_eq!(
        event.profile_policy_denial_diagnostic_key(),
        Some(PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED)
    );

    let shape = event.profile_policy_denial_shape().expect("policy shape");
    assert!(!shape.contains("ProductionDbPassword"));
    assert!(!shape.contains("private-vpc"));
    assert!(!shape.contains("prod-tenant-alpha"));
}

#[test]
fn capability_call_policy_denial_shapes_cover_not_granted_and_revoked() {
    let not_granted = AuditEvent::CapabilityCallExecuted {
        capability: CapabilityId::new("secret.read:TenantPayrollKey"),
        operation: "read-private-payroll-key".to_string(),
        handler_name: "none".to_string(),
        succeeded: false,
        duration_us: 1,
        timestamp: 1,
        profile: Some("prod-tenant-payroll".to_string()),
        module: Some("payroll-private-module".to_string()),
        function: None,
        input_hash: Some("a".repeat(64)),
        output_hash: None,
        trace_id: None,
        verification_report_hash: None,
        trace_context: None,
        denial_category: Some(DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED.to_string()),
    };
    let revoked = AuditEvent::CapabilityCallExecuted {
        capability: CapabilityId::new("network.egress:revoked-partner-api"),
        operation: "send-customer-payload".to_string(),
        handler_name: "none".to_string(),
        succeeded: false,
        duration_us: 1,
        timestamp: 1,
        profile: Some("prod-tenant-payroll".to_string()),
        module: Some("payroll-private-module".to_string()),
        function: None,
        input_hash: Some("b".repeat(64)),
        output_hash: None,
        trace_id: None,
        verification_report_hash: None,
        trace_context: None,
        denial_category: Some(DENIAL_CATEGORY_CAPABILITY_REVOKED.to_string()),
    };

    assert_eq!(
        not_granted.profile_policy_denial_shape(),
        Some(PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED)
    );
    assert_eq!(
        not_granted.profile_policy_denial_diagnostic_key(),
        Some(PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED)
    );
    assert_eq!(
        revoked.profile_policy_denial_shape(),
        Some(PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_REVOKED)
    );
    assert_eq!(
        revoked.profile_policy_denial_diagnostic_key(),
        Some(PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_REVOKED)
    );

    for event in [&not_granted, &revoked] {
        let shape = event.profile_policy_denial_shape().expect("policy shape");
        assert!(!shape.contains("TenantPayrollKey"));
        assert!(!shape.contains("revoked-partner-api"));
        assert!(!shape.contains("prod-tenant-payroll"));
        assert!(!shape.contains("payroll-private-module"));
    }
}

#[test]
fn runtime_issue_descriptors_cover_limit_capability_and_resource_policy_axes() {
    let timeout = AuditEvent::PreflightFailed {
        profile_name: "prod-tenant-private".to_string(),
        denied: vec![],
        reason: PreflightFailure::ResourceLimitExceeded {
            reason: "deadline exceeded while running private module".to_string(),
        },
    };
    let step = AuditEvent::PreflightFailed {
        profile_name: "prod-tenant-private".to_string(),
        denied: vec![],
        reason: PreflightFailure::ResourceLimitExceeded {
            reason: "fuel limit exceeded after private loop".to_string(),
        },
    };
    let memory = denied_event(DENIAL_CATEGORY_LIMIT_MEMORY);
    let capability = AuditEvent::PreflightFailed {
        profile_name: "prod-tenant-private".to_string(),
        denied: vec![CapabilityId::new("secret.read:ProductionDbPassword")],
        reason: PreflightFailure::CapabilityDenied {
            denied: vec![CapabilityId::new("secret.read:ProductionDbPassword")],
        },
    };
    let resource_policy = denied_event(DENIAL_CATEGORY_HANDLER_NOT_BOUND);

    assert_eq!(
        timeout.runtime_issue_descriptor(),
        Some(RuntimeIssueDescriptor {
            axis: RuntimeIssueAxis::Timeout,
            diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_TIME,
            shape: LIMIT_DENIAL_SHAPE_TIME,
        })
    );
    assert_eq!(
        step.runtime_issue_descriptor(),
        Some(RuntimeIssueDescriptor {
            axis: RuntimeIssueAxis::Step,
            diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_FUEL,
            shape: LIMIT_DENIAL_SHAPE_FUEL,
        })
    );
    assert_eq!(
        memory.runtime_issue_descriptor(),
        Some(RuntimeIssueDescriptor {
            axis: RuntimeIssueAxis::Memory,
            diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_MEMORY,
            shape: LIMIT_DENIAL_SHAPE_MEMORY,
        })
    );
    assert_eq!(
        capability.runtime_issue_descriptor(),
        Some(RuntimeIssueDescriptor {
            axis: RuntimeIssueAxis::Capability,
            diagnostic_key: PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED,
            shape: PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED,
        })
    );
    assert_eq!(
        resource_policy.runtime_issue_descriptor(),
        Some(RuntimeIssueDescriptor {
            axis: RuntimeIssueAxis::ResourcePolicy,
            diagnostic_key: RUNTIME_ISSUE_DIAGNOSTIC_KEY_RESOURCE_POLICY,
            shape: RUNTIME_ISSUE_SHAPE_RESOURCE_POLICY,
        })
    );
}

#[test]
fn runtime_issue_descriptors_for_events_are_redacted_deduped_and_canonical() {
    let events = vec![
        denied_event(DENIAL_CATEGORY_LIMIT_RATE),
        AuditEvent::PreflightFailed {
            profile_name: "tenant-alpha-secret".to_string(),
            denied: vec![CapabilityId::new("network.egress:private-vpc")],
            reason: PreflightFailure::CapabilityDenied {
                denied: vec![CapabilityId::new("network.egress:private-vpc")],
            },
        },
        denied_event(DENIAL_CATEGORY_LIMIT_MEMORY),
        AuditEvent::PreflightFailed {
            profile_name: "tenant-alpha-secret".to_string(),
            denied: vec![],
            reason: PreflightFailure::ResourceLimitExceeded {
                reason: "fuel limit exceeded in tenant-alpha-secret".to_string(),
            },
        },
        AuditEvent::PreflightFailed {
            profile_name: "tenant-alpha-secret".to_string(),
            denied: vec![],
            reason: PreflightFailure::ResourceLimitExceeded {
                reason: "deadline exceeded in tenant-alpha-secret".to_string(),
            },
        },
        // Duplicate timeout descriptor: batch output should de-duplicate it.
        AuditEvent::PreflightFailed {
            profile_name: "tenant-alpha-secret".to_string(),
            denied: vec![],
            reason: PreflightFailure::ResourceLimitExceeded {
                reason: "timeout while calling private endpoint".to_string(),
            },
        },
        AuditEvent::PreflightPassed {
            profile_name: "tenant-alpha-secret".to_string(),
            module_hash: "raw-private-module-hash".to_string(),
        },
    ];

    let descriptors = runtime_issue_descriptors_for_events(events.iter());

    assert_eq!(
        descriptors,
        vec![
            RuntimeIssueDescriptor {
                axis: RuntimeIssueAxis::Timeout,
                diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_TIME,
                shape: LIMIT_DENIAL_SHAPE_TIME,
            },
            RuntimeIssueDescriptor {
                axis: RuntimeIssueAxis::Step,
                diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_FUEL,
                shape: LIMIT_DENIAL_SHAPE_FUEL,
            },
            RuntimeIssueDescriptor {
                axis: RuntimeIssueAxis::Memory,
                diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_MEMORY,
                shape: LIMIT_DENIAL_SHAPE_MEMORY,
            },
            RuntimeIssueDescriptor {
                axis: RuntimeIssueAxis::Capability,
                diagnostic_key: PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED,
                shape: PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED,
            },
            RuntimeIssueDescriptor {
                axis: RuntimeIssueAxis::ResourcePolicy,
                diagnostic_key: LIMIT_DENIAL_DIAGNOSTIC_KEY_RATE,
                shape: LIMIT_DENIAL_SHAPE_RATE,
            },
        ]
    );

    for descriptor in descriptors {
        assert!(!descriptor.shape.contains("tenant-alpha-secret"));
        assert!(!descriptor.shape.contains("private-vpc"));
        assert!(!descriptor.diagnostic_key.contains("tenant-alpha-secret"));
        assert!(!descriptor.diagnostic_key.contains("private-vpc"));
    }
}

#[test]
fn audit_log_runtime_issue_descriptors_use_same_batch_ordering() {
    let mut log = AuditLog::new();
    log.push(denied_event(DENIAL_CATEGORY_LIMIT_RATE));
    log.push(AuditEvent::PreflightFailed {
        profile_name: "prod".to_string(),
        denied: vec![],
        reason: PreflightFailure::ResourceLimitExceeded {
            reason: "deadline exceeded".to_string(),
        },
    });

    assert_eq!(
        log.runtime_issue_descriptors()
            .into_iter()
            .map(|descriptor| descriptor.axis)
            .collect::<Vec<_>>(),
        vec![RuntimeIssueAxis::Timeout, RuntimeIssueAxis::ResourcePolicy]
    );
}

fn denied_event(category: &'static str) -> AuditEvent {
    AuditEvent::CapabilityCallExecuted {
        capability: CapabilityId::new("secret.read:ProductionDbPassword"),
        operation: "read-private-secret".to_string(),
        handler_name: "none".to_string(),
        succeeded: false,
        duration_us: 1,
        timestamp: 1,
        profile: Some("tenant-alpha-secret".to_string()),
        module: Some("private-module".to_string()),
        function: None,
        input_hash: Some("a".repeat(64)),
        output_hash: None,
        trace_id: None,
        verification_report_hash: None,
        trace_context: None,
        denial_category: Some(category.to_string()),
    }
}
