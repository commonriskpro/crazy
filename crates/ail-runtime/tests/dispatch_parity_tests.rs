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

// ── Helpers for two-cap asymmetry regression tests ───────────────────────

/// Build a WASM module that calls `ail/host_call` twice in sequence:
/// first with `cap_a` (stored at ptr 0), then with `cap_b` (stored at ptr 64).
/// The module returns the i64 result of the second call.
///
/// Memory layout: [0..63] = cap_a name, [64..127] = cap_b name,
///                [128..129] = "op", [192..] = args (0 words).
fn two_cap_host_call_wasm(cap_a: &str, cap_b: &str) -> Vec<u8> {
    const CAP_A_PTR: i32 = 0;
    const CAP_B_PTR: i32 = 64;
    const OP_PTR: i32 = 128;
    const ARGS_PTR: i32 = 192;

    let mut module = Module::new();

    let mut types = TypeSection::new();
    // type 0: host_call signature (cap_ptr, cap_len, op_ptr, op_len, args_ptr, args_len) -> i64
    types.ty().function(
        vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        vec![ValType::I64],
    );
    // type 1: main signature
    types.ty().function(vec![], vec![ValType::I64]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("ail", "host_call", EntityType::Function(0));
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
    let mut f = Function::new(vec![]);
    // First call: cap-A (result dropped)
    f.instruction(&Instruction::I32Const(CAP_A_PTR));
    f.instruction(&Instruction::I32Const(cap_a.len() as i32));
    f.instruction(&Instruction::I32Const(OP_PTR));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Const(ARGS_PTR));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Call(0));
    f.instruction(&Instruction::Drop);
    // Second call: cap-B (result returned)
    f.instruction(&Instruction::I32Const(CAP_B_PTR));
    f.instruction(&Instruction::I32Const(cap_b.len() as i32));
    f.instruction(&Instruction::I32Const(OP_PTR));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Const(ARGS_PTR));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Call(0));
    f.instruction(&Instruction::End);
    codes.function(&f);
    module.section(&codes);

    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(CAP_A_PTR), cap_a.bytes());
    data.active(0, &ConstExpr::i32_const(CAP_B_PTR), cap_b.bytes());
    data.active(0, &ConstExpr::i32_const(OP_PTR), "op".bytes());
    module.section(&data);

    module.finish()
}

/// Build a WASM module that calls `ail/host_call_write` twice in sequence:
/// first with `cap_a`, then with `cap_b`.  Returns the i32 bytes-written
/// result of the second call.
fn two_cap_host_call_write_wasm(cap_a: &str, cap_b: &str) -> Vec<u8> {
    const CAP_A_PTR: i32 = 0;
    const CAP_B_PTR: i32 = 64;
    const OP_PTR: i32 = 128;
    const ARGS_PTR: i32 = 192;
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    let mut module = Module::new();

    let mut types = TypeSection::new();
    // type 0: host_call_write (cap_ptr, cap_len, op_ptr, op_len, args_ptr, args_len, out_ptr, out_max) -> i32
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
    // type 1: main signature
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
    let mut f = Function::new(vec![]);
    // First call: cap-A (result dropped)
    f.instruction(&Instruction::I32Const(CAP_A_PTR));
    f.instruction(&Instruction::I32Const(cap_a.len() as i32));
    f.instruction(&Instruction::I32Const(OP_PTR));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Const(ARGS_PTR));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(OUT_PTR));
    f.instruction(&Instruction::I32Const(OUT_MAX));
    f.instruction(&Instruction::Call(0));
    f.instruction(&Instruction::Drop);
    // Second call: cap-B (result returned)
    f.instruction(&Instruction::I32Const(CAP_B_PTR));
    f.instruction(&Instruction::I32Const(cap_b.len() as i32));
    f.instruction(&Instruction::I32Const(OP_PTR));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Const(ARGS_PTR));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(OUT_PTR));
    f.instruction(&Instruction::I32Const(OUT_MAX));
    f.instruction(&Instruction::Call(0));
    f.instruction(&Instruction::End);
    codes.function(&f);
    module.section(&codes);

    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(CAP_A_PTR), cap_a.bytes());
    data.active(0, &ConstExpr::i32_const(CAP_B_PTR), cap_b.bytes());
    data.active(0, &ConstExpr::i32_const(OP_PTR), "op".bytes());
    module.section(&data);

    module.finish()
}

/// Build a profile that grants two capabilities to the given manifest module.
fn profile_granting_two_caps_with_limits(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    cap_a: CapabilityId,
    cap_b: CapabilityId,
    limits: ResourceLimits,
) -> RuntimeProfile {
    RuntimeProfile::new(
        "asymm-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![
            CapabilityGrant {
                module: manifest.module.clone(),
                capability: cap_a,
            },
            CapabilityGrant {
                module: manifest.module.clone(),
                capability: cap_b,
            },
        ],
        limits,
    )
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
