use super::helpers::*;

// ── Scenario R-4a: granted capability → handler dispatched, returns result ─

#[test]
fn wasm_call_to_granted_capability_dispatches_handler() {
    let cap = CapabilityId::new("parity.granted");
    let anf = effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "parity-granted".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_granting(&artifact.wasm, &manifest, cap.clone());
    let handler = Arc::new(TrackingHandler::new("tracking-handler", cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    // Invoke the WASM function which calls the capability via dispatch_host_call
    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    // R-4a: handler was dispatched, result is the handler's return (99)
    assert_eq!(
        result,
        RuntimeValue::I64(99),
        "granted handler must return its value"
    );
    assert_eq!(
        handler.call_count(),
        1,
        "handler must have been called once"
    );

    // R-4a: audit log must have a succeeded CapabilityCallExecuted event in host log
    let host_log = host.audit_log();
    let has_success = host_log.events().iter().any(|e| {
        matches!(
            e,
            AuditEvent::CapabilityCallExecuted {
                succeeded: true,
                ..
            }
        )
    });
    assert!(
        has_success,
        "host audit log must have a succeeded capability call event"
    );
}

// ── Scenario R-4b: ungranted capability → returns -1, no handler called ───

#[test]
fn wasm_call_to_ungranted_capability_returns_minus_one() {
    let cap = CapabilityId::new("parity.ungranted");
    let anf = effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    // Manifest requires no caps, so no grants in profile
    let manifest = CapabilityManifest {
        module: "parity-ungranted".to_string(),
        requires: vec![],
    };
    let profile = profile_no_grants(&artifact.wasm, &manifest);
    let handler = Arc::new(TrackingHandler::new("should-not-be-called", cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes (no caps required)");

    // Invoke: dispatch_host_call will deny (not granted) → returns -1 sentinel
    let result = instance.invoke("main", &[]).expect("invoke does not panic");

    // R-4b: returns -1 (the sentinel for denied/error)
    assert_eq!(
        result,
        RuntimeValue::I64(-1),
        "ungranted capability must return -1"
    );

    // R-4b: handler must NOT have been called
    assert_eq!(
        handler.call_count(),
        0,
        "handler must not be called for ungranted capability"
    );

    // R-4b: host audit log must have a failed event
    let host_log = host.audit_log();
    let has_denied = host_log.events().iter().any(|e| {
        matches!(
            e,
            AuditEvent::CapabilityCallExecuted {
                succeeded: false,
                ..
            }
        )
    });
    assert!(has_denied, "denied call must produce a failed audit event");
}

#[test]
fn wasm_call_payload_limit_returns_minus_one_and_skips_handler() {
    let cap = CapabilityId::new("parity.payload-limit");
    let anf = effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "parity-payload-limit".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_granting_with_limits(
        &artifact.wasm,
        &manifest,
        cap.clone(),
        ResourceLimits {
            payload_size_limit: Some(7),
            ..Default::default()
        },
    );
    let handler = Arc::new(TrackingHandler::new("should-not-be-called", cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    let result = instance.invoke("main", &[]).expect("invoke succeeds");
    assert_eq!(result, RuntimeValue::I64(-1));
    assert_eq!(handler.call_count(), 0, "handler must not run");
}

#[test]
fn wasm_call_revoked_after_instantiation_returns_minus_one_and_skips_handler() {
    let cap = CapabilityId::new("parity.revoked-after-instantiation");
    let anf = effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "parity-revoked".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_granting(&artifact.wasm, &manifest, cap.clone());
    let handler = Arc::new(TrackingHandler::new("should-not-be-called", cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    host.revoke_capability(
        manifest.module.clone(),
        cap.as_str(),
        profile.name(),
        InFlightPolicy::AllowComplete,
    );

    let result = instance.invoke("main", &[]).expect("invoke succeeds");
    assert_eq!(result, RuntimeValue::I64(-1));
    assert_eq!(handler.call_count(), 0, "handler must not run");
    assert!(
        has_failed_capability_event(&host, &cap),
        "revoked WASM dispatch must produce a failed audit event"
    );
}
