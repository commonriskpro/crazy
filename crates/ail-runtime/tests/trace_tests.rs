// ── ail-runtime: distributed tracing (Gap 2) ─────────────────────────────
//
// Strict TDD — RED phase.
//
// Scenarios:
//   1. TraceContext propagates trace_id through call_capability audit event.
//   2. A child span is created: new span_id, parent_span_id == parent span.
//   3. set_trace_context on RuntimeInstance does not panic (smoke test).
//   4. Without trace context set, audit event has trace_context == None.

use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, InMemoryHandler,
    ResourceLimits, RuntimeHost, RuntimeProfile, TraceContext, blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn setup_host_with_cap(cap: &CapabilityId) -> RuntimeHost {
    let wasm = minimal_wasm();
    let manifest = CapabilityManifest {
        module: "trace-test".to_string(),
        requires: vec![cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    let grant = CapabilityGrant {
        module: "trace-test".to_string(),
        capability: cap.clone(),
    };
    let profile = RuntimeProfile::new(
        "trace-test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![grant],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );
    let handler = Arc::new(InMemoryHandler::new(
        "trace-handler",
        vec![cap.clone()],
        b"ok".to_vec(),
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");
    host
}

// ── Scenario 1: trace_id propagates ──────────────────────────────────────
// GIVEN a RuntimeHost with a TraceContext set
// WHEN call_capability is invoked
// THEN the CapabilityCallExecuted audit event carries the same trace_id
#[test]
fn trace_id_propagates_through_capability_call() {
    let cap = CapabilityId::new("trace.read");
    let mut host = setup_host_with_cap(&cap);

    let ctx = TraceContext {
        trace_id: "trace-abc-123".to_string(),
        span_id: "span-root-001".to_string(),
        parent_span_id: None,
    };
    host.set_trace_context(ctx);

    host.call_capability(&cap, "read", b"").expect("call must succeed");

    let log = host.audit_log();
    let events = log.events();
    let call_event = events
        .iter()
        .find(|e| e.is_capability_call())
        .expect("must have a CapabilityCallExecuted event");

    match call_event {
        AuditEvent::CapabilityCallExecuted { trace_context, .. } => {
            let tc = trace_context
                .as_ref()
                .expect("trace_context must be Some when set on host");
            assert_eq!(tc.trace_id, "trace-abc-123",
                "trace_id must propagate to audit event");
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── Scenario 2: child span is created ────────────────────────────────────
// GIVEN a RuntimeHost with a TraceContext set
// WHEN call_capability is invoked
// THEN the audit event's trace_context has a NEW span_id and parent_span_id == parent
#[test]
fn capability_call_creates_child_span() {
    let cap = CapabilityId::new("trace.write");
    let mut host = setup_host_with_cap(&cap);

    let parent_ctx = TraceContext {
        trace_id: "trace-def-456".to_string(),
        span_id: "span-parent-002".to_string(),
        parent_span_id: None,
    };
    host.set_trace_context(parent_ctx.clone());

    host.call_capability(&cap, "write", b"").expect("call must succeed");

    let log = host.audit_log();
    let call_event = log
        .events()
        .iter()
        .find(|e| e.is_capability_call())
        .expect("must have CapabilityCallExecuted");

    match call_event {
        AuditEvent::CapabilityCallExecuted { trace_context, .. } => {
            let tc = trace_context
                .as_ref()
                .expect("trace_context must be Some");
            assert_eq!(tc.trace_id, "trace-def-456",
                "child span must inherit trace_id");
            assert_ne!(tc.span_id, "span-parent-002",
                "child span must have a new span_id");
            assert_eq!(
                tc.parent_span_id.as_deref(),
                Some("span-parent-002"),
                "child span_id must point to parent"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── Scenario 3: no trace context → None in audit ─────────────────────────
// GIVEN a RuntimeHost WITHOUT a TraceContext set
// WHEN call_capability is invoked
// THEN the CapabilityCallExecuted event has trace_context == None
#[test]
fn no_trace_context_yields_none_in_audit() {
    let cap = CapabilityId::new("trace.none");
    let mut host = setup_host_with_cap(&cap);
    // Deliberately do NOT call set_trace_context.

    host.call_capability(&cap, "read", b"").expect("call must succeed");

    let log = host.audit_log();
    let call_event = log
        .events()
        .iter()
        .find(|e| e.is_capability_call())
        .expect("must have CapabilityCallExecuted");

    match call_event {
        AuditEvent::CapabilityCallExecuted { trace_context, .. } => {
            assert!(
                trace_context.is_none(),
                "trace_context must be None when no context was set"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── Scenario 4: set_trace_context on RuntimeInstance ─────────────────────
// GIVEN a RuntimeInstance (WASM instantiated)
// WHEN set_trace_context is called
// THEN no panic occurs and the context is stored in the instance
#[test]
fn set_trace_context_on_instance_does_not_panic() {
    let cap = CapabilityId::new("trace.instance");
    let wasm = minimal_wasm();
    let manifest = CapabilityManifest {
        module: "trace-inst".to_string(),
        requires: vec![cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash");
    let grant = CapabilityGrant {
        module: "trace-inst".to_string(),
        capability: cap.clone(),
    };
    let profile = RuntimeProfile::new(
        "trace-inst-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![grant],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("must instantiate");

    let ctx = TraceContext {
        trace_id: "trace-inst-789".to_string(),
        span_id: "span-inst-001".to_string(),
        parent_span_id: None,
    };
    // Must not panic.
    instance.set_trace_context(ctx.clone());

    // Verify the context was stored by reading it back.
    let stored = instance.trace_context();
    assert_eq!(stored.as_ref(), Some(&ctx),
        "stored trace context must match what was set");
}
