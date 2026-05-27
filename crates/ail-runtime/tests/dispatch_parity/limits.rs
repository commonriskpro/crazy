use super::helpers::*;

#[test]
fn wasm_call_to_granted_but_unbound_capability_returns_minus_one() {
    let cap = CapabilityId::new("parity.no-handler");
    let anf = effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "parity-no-handler".to_string(),
        requires: vec![cap.clone()],
    };
    // Grant the capability but register NO handler
    let profile = profile_granting(&artifact.wasm, &manifest, cap.clone());

    let mut host = RuntimeHost::new(); // No handlers registered
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    // dispatch_host_call: granted but no handler → returns -1
    let result = instance.invoke("main", &[]).expect("invoke does not panic");

    // R-4c: returns -1
    assert_eq!(
        result,
        RuntimeValue::I64(-1),
        "no-handler capability must return -1"
    );

    // R-4c: audit log must have a failed event
    let host_log = host.audit_log();
    let has_no_handler_event = host_log.events().iter().any(|e| {
        matches!(
            e,
            AuditEvent::CapabilityCallExecuted {
                succeeded: false,
                ..
            }
        )
    });
    assert!(
        has_no_handler_event,
        "no-handler call must produce a failed audit event"
    );
}

// ── Increment-asymmetry regression (pre-existing, matches main) ───────────
//
// `dispatch_host_call`       — increments BEFORE handler lookup.
// `dispatch_host_call_write` — increments AFTER  handler is found.
//
// Both behaviors are intentional and match the original monolithic host.rs in
// main verbatim.  The tests below lock them in to prevent silent regressions.

/// `dispatch_host_call` increments `capability_calls_used` before checking for
/// a handler.  A granted-but-unbound call therefore exhausts the call budget,
/// causing a subsequent call to a *bound* handler to be rejected.
///
/// Sequence (max_calls = 1):
///   1. Call cap-A (granted, no handler) → slot consumed (count = 1), returns -1.
///   2. Call cap-B (granted, has handler) → count 1 >= 1 → limit exceeded, -1.
///
/// The handler for cap-B must NOT be invoked.
#[test]
fn dispatch_host_call_unbound_consumes_max_call_slot() {
    let cap_a = CapabilityId::new("asymm.hc-unbound");
    let cap_b = CapabilityId::new("asymm.hc-bound");
    let handler_b = Arc::new(TrackingHandler::new("handler-b", cap_b.clone()));

    let wasm = two_cap_host_call_wasm(cap_a.as_str(), cap_b.as_str());
    let manifest = CapabilityManifest {
        module: "asymm-hc-test".to_string(),
        requires: vec![cap_a.clone(), cap_b.clone()],
    };
    let profile = profile_granting_two_caps_with_limits(
        &wasm,
        &manifest,
        cap_a.clone(),
        cap_b.clone(),
        ResourceLimits {
            max_capability_calls: Some(1),
            ..Default::default()
        },
    );
    let mut host = RuntimeHost::new().with_handler(handler_b.clone());
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // cap-A consumed the only slot; cap-B is rejected by the limit check.
    assert_eq!(
        result,
        RuntimeValue::I64(-1),
        "cap-B must be rejected because the unbound cap-A call consumed the slot"
    );
    assert_eq!(
        handler_b.call_count(),
        0,
        "handler-b must not be called when the call slot is exhausted by an unbound cap"
    );
}

/// `dispatch_host_call_write` increments `capability_calls_used` only after a
/// handler is found.  A granted-but-unbound call therefore does NOT exhaust
/// the call budget, allowing a subsequent call to a *bound* handler to succeed.
///
/// Sequence (max_calls = 1):
///   1. Call cap-A (granted, no handler) → slot NOT consumed (count stays 0), -1.
///   2. Call cap-B (granted, has handler) → count 0 < 1 → handler runs → bytes written.
///
/// The handler for cap-B MUST be invoked and the call MUST succeed.
#[test]
fn dispatch_host_call_write_unbound_does_not_consume_slot() {
    let cap_a = CapabilityId::new("asymm.hw-unbound");
    let cap_b = CapabilityId::new("asymm.hw-bound");
    let handler_b = Arc::new(TrackingHandler::new("handler-b-write", cap_b.clone()));

    let wasm = two_cap_host_call_write_wasm(cap_a.as_str(), cap_b.as_str());
    let manifest = CapabilityManifest {
        module: "asymm-hw-test".to_string(),
        requires: vec![cap_a.clone(), cap_b.clone()],
    };
    let profile = profile_granting_two_caps_with_limits(
        &wasm,
        &manifest,
        cap_a.clone(),
        cap_b.clone(),
        ResourceLimits {
            max_capability_calls: Some(1),
            ..Default::default()
        },
    );
    let mut host = RuntimeHost::new().with_handler(handler_b.clone());
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // cap-A did NOT consume a slot; cap-B succeeds and writes 8 bytes (the
    // TrackingHandler returns 99_i64 as 8 LE bytes).
    assert_eq!(
        result,
        RuntimeValue::I32(8),
        "cap-B must succeed because the unbound cap-A call did not consume a slot"
    );
    assert_eq!(
        handler_b.call_count(),
        1,
        "handler-b must be called exactly once"
    );
}
