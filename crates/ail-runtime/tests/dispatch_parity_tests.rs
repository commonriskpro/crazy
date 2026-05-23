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
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostError, HostResult,
    InFlightPolicy, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction, MemorySection, MemoryType, Module, TypeSection,
    ValType,
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

struct FailingHandler {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
    name: String,
}

impl FailingHandler {
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

impl Handler for FailingHandler {
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
        Err(HostError::Custom("handler failed".to_string()))
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

/// Build an AnfIr whose EffectCall uses `host_call_write` by flowing into a structured value.
fn structured_effect_anf_with_arg(cap_name: &str) -> AnfIr {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "effect_result".to_string(),
                value: Box::new(AnfExpr::EffectCall {
                    capability: cap_name.to_string(),
                    func: "op".to_string(),
                    args: vec!["n".to_string()],
                }),
                body: Box::new(AnfExpr::RecordNew {
                    fields: vec![(
                        "value".to_string(),
                        AnfExpr::Var("effect_result".to_string()),
                    )],
                }),
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

fn direct_host_call_write_wasm(cap_name: &str, out_ptr: i32, out_max: i32) -> Vec<u8> {
    const CAP_PTR: i32 = 0;
    const OP_PTR: i32 = 64;
    const ARGS_PTR: i32 = 128;

    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function(
        vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        vec![ValType::I32],
    );
    types.ty().function(vec![], vec![ValType::I32]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("ail", "host_call_write", EntityType::Function(0));
    module.section(&imports);

    let mut functions = FunctionSection::new();
    functions.function(1);
    module.section(&functions);

    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("main", ExportKind::Func, 1);
    module.section(&exports);

    let mut codes = CodeSection::new();
    let mut function = Function::new(vec![]);
    function.instruction(&Instruction::I32Const(CAP_PTR));
    function.instruction(&Instruction::I32Const(cap_name.len() as i32));
    function.instruction(&Instruction::I32Const(OP_PTR));
    function.instruction(&Instruction::I32Const(2));
    function.instruction(&Instruction::I32Const(ARGS_PTR));
    function.instruction(&Instruction::I32Const(0));
    function.instruction(&Instruction::I32Const(out_ptr));
    function.instruction(&Instruction::I32Const(out_max));
    function.instruction(&Instruction::Call(0));
    function.instruction(&Instruction::End);
    codes.function(&function);
    module.section(&codes);

    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(CAP_PTR), cap_name.bytes());
    data.active(0, &ConstExpr::i32_const(OP_PTR), "op".bytes());
    module.section(&data);

    module.finish()
}

fn instantiate_direct_host_call_write(
    cap: &CapabilityId,
    out_ptr: i32,
    out_max: i32,
    limits: ResourceLimits,
    handlers: Vec<Arc<dyn Handler + Send + Sync>>,
) -> (RuntimeHost, ail_runtime::RuntimeInstance) {
    let wasm = direct_host_call_write_wasm(cap.as_str(), out_ptr, out_max);
    let manifest = CapabilityManifest {
        module: "parity-write-direct".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_granting_with_limits(&wasm, &manifest, cap.clone(), limits);
    let mut host = RuntimeHost::new();
    for handler in handlers {
        host = host.with_handler(handler);
    }
    let instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight passes");
    (host, instance)
}

fn profile_granting(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    cap: CapabilityId,
) -> RuntimeProfile {
    profile_granting_with_limits(wasm, manifest, cap, ResourceLimits::default())
}

fn profile_granting_with_limits(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    cap: CapabilityId,
    limits: ResourceLimits,
) -> RuntimeProfile {
    RuntimeProfile::new(
        "dispatch-parity-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![CapabilityGrant {
            module: manifest.module.clone(),
            capability: cap,
        }],
        limits,
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

fn has_failed_capability_event(host: &RuntimeHost, cap: &CapabilityId) -> bool {
    host.audit_log().events().iter().any(|event| {
        matches!(
            event,
            AuditEvent::CapabilityCallExecuted {
                capability,
                succeeded: false,
                ..
            } if capability == cap
        )
    })
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
