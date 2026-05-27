use super::helpers::*;

#[test]
fn host_call_write_denied_capability_emits_failed_audit_event() {
    let cap = CapabilityId::new("parity.write-denied");
    let anf = structured_effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "parity-write-denied".to_string(),
        requires: vec![],
    };
    let profile = profile_no_grants(&artifact.wasm, &manifest);
    let handler = Arc::new(TrackingHandler::new("should-not-be-called", cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(handler.call_count(), 0, "handler must not run");
    assert!(
        has_failed_capability_event(&host, &cap),
        "denied host_call_write must produce a failed audit event"
    );
}

#[test]
fn host_call_write_payload_limit_emits_failed_audit_event_and_skips_handler() {
    let cap = CapabilityId::new("parity.write-payload-limit");
    let anf = structured_effect_anf_with_arg(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "parity-write-payload-limit".to_string(),
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

    instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(handler.call_count(), 0, "handler must not run");
    assert!(
        has_failed_capability_event(&host, &cap),
        "payload-limited host_call_write must produce a failed audit event"
    );
}

#[test]
fn host_call_write_missing_handler_emits_failed_audit_event() {
    let cap = CapabilityId::new("parity.write-missing-handler");
    let (host, mut instance) =
        instantiate_direct_host_call_write(&cap, 256, 8, ResourceLimits::default(), vec![]);

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I32(-1));
    assert!(
        has_failed_capability_event(&host, &cap),
        "missing handler host_call_write must produce a failed audit event"
    );
}

#[test]
fn host_call_write_handler_error_emits_failed_audit_event() {
    let cap = CapabilityId::new("parity.write-handler-error");
    let handler = Arc::new(FailingHandler::new("failing-handler", cap.clone()));
    let (host, mut instance) = instantiate_direct_host_call_write(
        &cap,
        256,
        8,
        ResourceLimits::default(),
        vec![handler.clone()],
    );

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I32(-1));
    assert_eq!(handler.call_count(), 1, "handler must run once");
    assert!(
        has_failed_capability_event(&host, &cap),
        "handler error host_call_write must produce a failed audit event"
    );
}

#[test]
fn host_call_write_output_limit_emits_failed_audit_event() {
    let cap = CapabilityId::new("parity.write-output-limit");
    let handler = Arc::new(TrackingHandler::new("tracking-handler", cap.clone()));
    let (host, mut instance) = instantiate_direct_host_call_write(
        &cap,
        256,
        8,
        ResourceLimits {
            output_size_limit: Some(7),
            ..Default::default()
        },
        vec![handler.clone()],
    );

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I32(-1));
    assert_eq!(handler.call_count(), 1, "handler must run before limit");
    assert!(
        has_failed_capability_event(&host, &cap),
        "output-limited host_call_write must produce a failed audit event"
    );
}

#[test]
fn host_call_write_output_buffer_too_small_emits_failed_audit_event() {
    let cap = CapabilityId::new("parity.write-output-buffer-small");
    let handler = Arc::new(TrackingHandler::new("tracking-handler", cap.clone()));
    let (host, mut instance) = instantiate_direct_host_call_write(
        &cap,
        256,
        4,
        ResourceLimits::default(),
        vec![handler.clone()],
    );

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I32(-1));
    assert_eq!(
        handler.call_count(),
        1,
        "handler must run before buffer check"
    );
    assert!(
        has_failed_capability_event(&host, &cap),
        "too-small host_call_write output buffer must produce a failed audit event"
    );
}

#[test]
fn host_call_write_memory_write_failure_emits_failed_audit_event() {
    let cap = CapabilityId::new("parity.write-memory-failure");
    let handler = Arc::new(TrackingHandler::new("tracking-handler", cap.clone()));
    let (host, mut instance) = instantiate_direct_host_call_write(
        &cap,
        70_000,
        8,
        ResourceLimits::default(),
        vec![handler.clone()],
    );

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I32(-1));
    assert_eq!(
        handler.call_count(),
        1,
        "handler must run before memory write"
    );
    assert!(
        has_failed_capability_event(&host, &cap),
        "failed host_call_write memory write must produce a failed audit event"
    );
}
