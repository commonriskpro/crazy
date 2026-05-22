// ── ail-runtime::dispatch_parity_tests ───────────────────────────────────
//
// TDD RED phase — verifying dispatch_host_call grant check parity with
// call_capability (R-4).
//
// Spec scenarios covered (R-4a, R-4b, R-4c):
//  - WASM call to granted capability → handler dispatched, returns result.
//  - WASM call to ungranted capability → returns -1, no handler called.
//  - WASM call to granted capability with no bound handler → returns -1.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, SourceMap,
    core_ir::{LiteralValue, StageHashes},
};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostResult,
    ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

struct TrackingHandler {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
    name: String,
}

impl TrackingHandler {
    fn new(name: &str, cap: CapabilityId) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            caps: vec![cap],
            name: name.to_string(),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Handler for TrackingHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        _operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(99_i64.to_le_bytes().to_vec())
    }
}

/// Build an AnfIr with an EffectCall to the given capability.
fn effect_anf_with_arg(cap_name: &str) -> AnfIr {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::EffectCall {
                capability: cap_name.to_string(),
                func: "op".to_string(),
                args: vec!["n".to_string()],
            }),
        },
    };
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

fn profile_granting(wasm: &[u8], manifest: &CapabilityManifest, cap: CapabilityId) -> RuntimeProfile {
    RuntimeProfile::new(
        "dispatch-parity-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![CapabilityGrant {
            module: manifest.module.clone(),
            capability: cap,
        }],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    )
}

fn profile_no_grants(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    RuntimeProfile::new(
        "dispatch-denied-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    )
}

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
    assert_eq!(result, RuntimeValue::I64(99), "granted handler must return its value");
    assert_eq!(handler.call_count(), 1, "handler must have been called once");

    // R-4a: audit log must have a succeeded CapabilityCallExecuted event in host log
    let host_log = host.audit_log();
    let has_success = host_log.events().iter().any(|e| {
        matches!(e, AuditEvent::CapabilityCallExecuted { succeeded: true, .. })
    });
    assert!(has_success, "host audit log must have a succeeded capability call event");
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
    assert_eq!(result, RuntimeValue::I64(-1), "ungranted capability must return -1");

    // R-4b: handler must NOT have been called
    assert_eq!(handler.call_count(), 0, "handler must not be called for ungranted capability");

    // R-4b: host audit log must have a failed event
    let host_log = host.audit_log();
    let has_denied = host_log.events().iter().any(|e| {
        matches!(e, AuditEvent::CapabilityCallExecuted { succeeded: false, .. })
    });
    assert!(has_denied, "denied call must produce a failed audit event");
}

// ── Scenario R-4c: granted but no handler → returns -1 ───────────────────

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
    assert_eq!(result, RuntimeValue::I64(-1), "no-handler capability must return -1");

    // R-4c: audit log must have a failed event
    let host_log = host.audit_log();
    let has_no_handler_event = host_log.events().iter().any(|e| {
        matches!(e, AuditEvent::CapabilityCallExecuted { succeeded: false, .. })
    });
    assert!(has_no_handler_event, "no-handler call must produce a failed audit event");
}
