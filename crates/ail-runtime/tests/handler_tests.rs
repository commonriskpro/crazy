// ── ail-runtime::handler_tests ───────────────────────────────────────────
//
// Spec scenarios S1–S7 from sdd/handler-system/spec:
//
//  S1 — Handler dispatches canned response + audit event recorded
//  S2 — Missing handler denial → HandlerNotBound audit event
//  S3 — Ungranted capability denied at dispatch → CapabilityDenied audit event
//  S4 — Preflight step 6 binding check fails when no handler registered (opt-in)
//  S5 — Preflight step 6 binding check passes when handler registered (opt-in)
//  S6 — Existing preflight tests still pass (backward compat — covered by all
//        other test files; verified here by running a simple preflight)
//  S7 — Multiple audit events accumulate in call order

use std::sync::Arc;

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, SourceMap, StageHashes, emit_wasm,
};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, InMemoryHandler,
    PreflightFailure, ResourceLimits, RuntimeError, RuntimeHost, RuntimeProfile, blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

/// Minimal structurally-valid WASM: magic + version only.
fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn wasm_with_one_capability_call(capability: &str, operation: &str) -> Vec<u8> {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fetch".to_string(),
        expr: AnfExpr::EffectCall {
            capability: capability.to_string(),
            func: operation.to_string(),
            args: vec![],
        },
    };
    let anf = AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0; 32],
            verification_report_hash: [0; 32],
            core_ir_hash: [0; 32],
            anf_ir_hash: Some([0; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    };
    emit_wasm(&anf).expect("emit_wasm must succeed").wasm
}

/// Build a `RuntimeProfile` whose hashes match `wasm` and `manifest` exactly.
fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
) -> RuntimeProfile {
    matching_profile_with_limits(wasm, manifest, grants, ResourceLimits::default())
}

fn matching_profile_with_limits(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
    limits: ResourceLimits,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        grants,
        limits,
    )
}

// ── S1 — Handler dispatches canned response ───────────────────────────────

#[test]
fn handler_dispatches_canned_response() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    let handler = Arc::new(InMemoryHandler::new(
        "file-handler",
        vec![cap.clone()],
        b"canned-response".to_vec(),
    ));

    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = host.call_capability(&cap, "read", &[]);
    assert_eq!(
        result,
        Ok(b"canned-response".to_vec()),
        "must return canned bytes"
    );

    // Audit log: 1 preflight passed + 1 capability call executed
    let audit = host.audit_log();
    let events = audit.events();
    assert_eq!(events.len(), 2);
    assert!(events[0].is_passed(), "first event must be PreflightPassed");
    assert!(
        events[1].is_capability_call(),
        "second event must be CapabilityCallExecuted"
    );

    match &events[1] {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            handler_name,
            ..
        } => {
            assert!(succeeded, "call must have succeeded");
            assert_eq!(handler_name, "file-handler");
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── S2 — Missing handler denial ───────────────────────────────────────────

#[test]
fn missing_handler_returns_handler_not_bound() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    // No handlers registered.
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = host.call_capability(&cap, "read", &[]);
    assert!(result.is_err(), "must fail when no handler is registered");

    let err = result.unwrap_err();
    assert!(
        matches!(err, ail_runtime::abi::HostError::HandlerNotBound(_)),
        "error must be HandlerNotBound, got: {err:?}"
    );

    // Audit: preflight passed + capability call failed
    let audit = host.audit_log();
    let events = audit.events();
    assert_eq!(events.len(), 2);
    match &events[1] {
        AuditEvent::CapabilityCallExecuted { succeeded, .. } => {
            assert!(!succeeded, "call must be recorded as failed");
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── S3 — Ungranted capability denied at dispatch ──────────────────────────

#[test]
fn ungranted_capability_denied_at_dispatch() {
    let wasm = minimal_wasm();
    let granted_cap = CapabilityId::new("FileRead");
    let denied_cap = CapabilityId::new("NetworkEgress");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![granted_cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: granted_cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    let handler = Arc::new(InMemoryHandler::new(
        "test-handler",
        vec![granted_cap.clone(), denied_cap.clone()],
        b"ok".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Call a capability that is NOT in the profile grants.
    let result = host.call_capability(&denied_cap, "connect", &[]);
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        matches!(err, ail_runtime::abi::HostError::CapabilityDenied(_)),
        "error must be CapabilityDenied, got: {err:?}"
    );

    // Audit: capability call event with succeeded=false
    let audit = host.audit_log();
    let events = audit.events();
    let cap_events: Vec<_> = events.iter().filter(|e| e.is_capability_call()).collect();
    assert_eq!(cap_events.len(), 1);
    match cap_events[0] {
        AuditEvent::CapabilityCallExecuted { succeeded, .. } => {
            assert!(!succeeded);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn max_capability_calls_blocks_second_granted_call() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile_with_limits(
        &wasm,
        &manifest,
        vec![grant],
        ResourceLimits {
            max_capability_calls: Some(1),
            ..Default::default()
        },
    );

    let handler = Arc::new(InMemoryHandler::new(
        "file-handler",
        vec![cap.clone()],
        b"ok".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    assert_eq!(host.call_capability(&cap, "read", &[]), Ok(b"ok".to_vec()));
    let err = host
        .call_capability(&cap, "read", &[])
        .expect_err("second granted call must exceed max_capability_calls");
    assert!(
        matches!(err, ail_runtime::abi::HostError::LimitExceeded(_)),
        "error must be LimitExceeded, got: {err:?}"
    );

    let audit = host.audit_log();
    let cap_events: Vec<_> = audit
        .events()
        .iter()
        .filter(|event| event.is_capability_call())
        .collect();
    assert_eq!(cap_events.len(), 2);
    match cap_events[1] {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            handler_name,
            ..
        } => {
            assert!(!succeeded, "limit denial must be audited as failed");
            assert_eq!(handler_name, "none");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn max_capability_calls_resets_between_wasm_invocations() {
    let cap = CapabilityId::new("FileRead");
    let wasm = wasm_with_one_capability_call(cap.as_str(), "read");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile_with_limits(
        &wasm,
        &manifest,
        vec![grant],
        ResourceLimits {
            max_capability_calls: Some(1),
            ..Default::default()
        },
    );

    let handler = Arc::new(InMemoryHandler::new(
        "file-handler",
        vec![cap],
        b"ok".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    instance
        .invoke("fetch", &[])
        .expect("first invocation must pass");
    instance
        .invoke("fetch", &[])
        .expect("second invocation must reset call count");
}

#[test]
fn ungranted_capability_denial_precedes_call_limit() {
    let wasm = minimal_wasm();
    let granted_cap = CapabilityId::new("FileRead");
    let denied_cap = CapabilityId::new("NetworkEgress");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![granted_cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: granted_cap.clone(),
    };
    let profile = matching_profile_with_limits(
        &wasm,
        &manifest,
        vec![grant],
        ResourceLimits {
            max_capability_calls: Some(0),
            ..Default::default()
        },
    );

    let handler = Arc::new(InMemoryHandler::new(
        "test-handler",
        vec![granted_cap.clone(), denied_cap.clone()],
        b"ok".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let err = host
        .call_capability(&denied_cap, "connect", &[])
        .expect_err("ungranted capability must still be denied by grant check");
    assert!(
        matches!(err, ail_runtime::abi::HostError::CapabilityDenied(_)),
        "deny-by-default must win before limits, got: {err:?}"
    );
}

// ── S4 — Preflight step 6 binding check fails ────────────────────────────

#[test]
fn preflight_handler_binding_check_fails_when_no_handler() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap.clone(),
    };
    // Profile with require_handler_binding = true.
    let profile = matching_profile(&wasm, &manifest, vec![grant]).with_handler_binding_required();

    // No handlers registered.
    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(result.is_err(), "preflight must fail without handler");
    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::HandlerNotBound { capability })) => {
            assert_eq!(capability, cap, "failure must name the unbound capability");
        }
        other => panic!("expected HandlerNotBound, got {other:?}"),
    }

    // Audit: one PreflightFailed event
    let audit = host.audit_log();
    let events = audit.events();
    assert_eq!(events.len(), 1);
    assert!(!events[0].is_passed());
}

// ── S5 — Preflight step 6 binding check passes ───────────────────────────

#[test]
fn preflight_handler_binding_check_passes_with_handler() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]).with_handler_binding_required();

    let handler = Arc::new(InMemoryHandler::new(
        "file-handler",
        vec![cap.clone()],
        b"ok".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "preflight must pass with handler: {result:?}"
    );
    assert!(host.audit_log().events()[0].is_passed());
}

// ── S6 — Backward compat: existing preflight unaffected ──────────────────

#[test]
fn existing_preflight_works_without_handler_binding() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap,
    };
    // Default profile — require_handler_binding is false.
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    // No handlers, no binding requirement — must succeed.
    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);
    assert!(
        result.is_ok(),
        "existing preflight must pass without handler binding enabled"
    );
}

// ── S7 — Multiple audit events accumulate in order ───────────────────────

#[test]
fn multiple_capability_calls_accumulate_audit_events() {
    let wasm = minimal_wasm();
    let cap_a = CapabilityId::new("FileRead");
    let cap_b = CapabilityId::new("FileWrite");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap_a.clone(), cap_b.clone()],
    };
    let profile = matching_profile(
        &wasm,
        &manifest,
        vec![
            CapabilityGrant {
                module: "test".to_string(),
                capability: cap_a.clone(),
            },
            CapabilityGrant {
                module: "test".to_string(),
                capability: cap_b.clone(),
            },
        ],
    );

    let handler = Arc::new(InMemoryHandler::new(
        "rw-handler",
        vec![cap_a.clone(), cap_b.clone()],
        b"data".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let _ = host.call_capability(&cap_a, "read", &[]);
    let _ = host.call_capability(&cap_b, "write", b"payload");

    let audit = host.audit_log();
    let events = audit.events();
    // 1 preflight + 2 capability calls = 3 events
    assert_eq!(events.len(), 3, "must have exactly 3 audit events");
    assert!(events[0].is_passed());
    assert!(events[1].is_capability_call());
    assert!(events[2].is_capability_call());

    // Events are in insertion order.
    match (&events[1], &events[2]) {
        (
            AuditEvent::CapabilityCallExecuted {
                capability: cap1, ..
            },
            AuditEvent::CapabilityCallExecuted {
                capability: cap2, ..
            },
        ) => {
            assert_eq!(cap1, &cap_a, "first call must be FileRead");
            assert_eq!(cap2, &cap_b, "second call must be FileWrite");
        }
        other => panic!("unexpected events: {other:?}"),
    }
}
