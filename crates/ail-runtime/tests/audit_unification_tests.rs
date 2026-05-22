// ── ail-runtime::audit_unification_tests ─────────────────────────────────
//
// TDD RED phase — written before audit log is unified via Arc<Mutex<_>>.
//
// Spec scenarios covered (R-3a, R-3b, R-3c):
//  - Events appended in dispatch_host_call (WASM-side) are visible in
//    RuntimeHost::audit_log() after invoke returns.
//  - The CapabilityCallExecuted event has the correct capability name.
//  - Denied capability calls produce a failed audit event visible in host.

use std::sync::Arc;

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

struct EchoHandler {
    caps: Vec<CapabilityId>,
}

impl EchoHandler {
    fn new(cap: CapabilityId) -> Self {
        Self { caps: vec![cap] }
    }
}

impl Handler for EchoHandler {
    fn name(&self) -> &str {
        "echo-handler"
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
        Ok(42_i64.to_le_bytes().to_vec())
    }
}

/// Build an AnfIr with an EffectCall to the given capability.
fn effect_anf(cap_name: &str) -> AnfIr {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::EffectCall {
                capability: cap_name.to_string(),
                func: "run".to_string(),
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

fn profile_with_grant(wasm: &[u8], manifest: &CapabilityManifest, cap: CapabilityId) -> RuntimeProfile {
    RuntimeProfile::new(
        "audit-unification-test".to_string(),
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
    .with_handler_binding_required()
}

fn profile_no_grants(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    RuntimeProfile::new(
        "audit-denied-test".to_string(),
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

// ── Scenario R-3a: WASM host_call events appear in RuntimeHost::audit_log ─

#[test]
fn wasm_host_call_events_appear_in_runtime_host_audit_log() {
    let cap = CapabilityId::new("test.audit");
    let anf = effect_anf(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "audit-test-module".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_with_grant(&artifact.wasm, &manifest, cap.clone());
    let handler = Arc::new(EchoHandler::new(cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    let result = instance.invoke("main", &[]).expect("invoke succeeds");
    assert_eq!(result, RuntimeValue::I64(42));

    // R-3a: CapabilityCallExecuted event must appear in RuntimeHost::audit_log()
    let host_log = host.audit_log();
    let host_events = host_log.events();
    let has_cap_call = host_events.iter().any(|e| {
        matches!(e, AuditEvent::CapabilityCallExecuted { .. })
    });
    assert!(
        has_cap_call,
        "RuntimeHost::audit_log() must contain CapabilityCallExecuted after WASM invoke, got {host_events:?}"
    );
}

// ── Scenario R-3b: event has correct capability name ─────────────────────

#[test]
fn wasm_host_call_event_has_correct_capability_name() {
    let cap = CapabilityId::new("test.named-cap");
    let anf = effect_anf(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    let manifest = CapabilityManifest {
        module: "named-cap-module".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_with_grant(&artifact.wasm, &manifest, cap.clone());
    let handler = Arc::new(EchoHandler::new(cap.clone()));

    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    instance.invoke("main", &[]).expect("invoke succeeds");

    // R-3b: capability name must match "test.named-cap"
    let host_log = host.audit_log();
    let host_events = host_log.events();
    let cap_event = host_events.iter().find(|e| {
        matches!(e, AuditEvent::CapabilityCallExecuted { capability, .. }
            if capability.as_str() == "test.named-cap")
    });
    assert!(
        cap_event.is_some(),
        "must find CapabilityCallExecuted with capability='test.named-cap' in host audit log"
    );
}

// ── Scenario R-3c: denied capability produces failed audit event ──────────

#[test]
fn wasm_denied_capability_call_produces_failed_audit_event() {
    let cap = CapabilityId::new("test.denied-cap");
    let anf = effect_anf(cap.as_str());
    let artifact = ail_compiler::emit_wasm(&anf).expect("emit_wasm");

    // Profile grants nothing — capability is denied
    let manifest = CapabilityManifest {
        module: "denied-cap-module".to_string(),
        requires: vec![],
    };
    let profile = profile_no_grants(&artifact.wasm, &manifest);

    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes (no caps required)");

    // Invoke triggers WASM → dispatch_host_call → denied (not granted)
    // The invoke returns an error sentinel but does not panic
    let _ = instance.invoke("main", &[]);

    // R-3c: denied call produces CapabilityCallExecuted with succeeded=false
    let host_log = host.audit_log();
    let host_events = host_log.events();
    let denied_event = host_events.iter().find(|e| {
        matches!(e,
            AuditEvent::CapabilityCallExecuted { capability, succeeded: false, .. }
            if capability.as_str() == "test.denied-cap"
        )
    });
    assert!(
        denied_event.is_some(),
        "denied WASM capability call must produce failed audit event in host log, got {host_events:?}"
    );
}
