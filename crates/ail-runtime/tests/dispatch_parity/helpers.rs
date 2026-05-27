pub(super) use std::sync::Arc;
pub(super) use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, SourceMap,
    core_ir::{LiteralValue, StageHashes},
};
pub(super) use ail_core::semantic_graph::NodeRef;
pub(super) use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostError, HostResult,
    InFlightPolicy, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};
pub(super) use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction, MemorySection, MemoryType, Module, TypeSection,
    ValType,
};

// ── helpers ──────────────────────────────────────────────────────────────

pub(super) struct TrackingHandler {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
    name: String,
}

impl TrackingHandler {
    pub(super) fn new(name: &str, cap: CapabilityId) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            caps: vec![cap],
            name: name.to_string(),
        }
    }

    pub(super) fn call_count(&self) -> usize {
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

pub(super) struct FailingHandler {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
    name: String,
}

impl FailingHandler {
    pub(super) fn new(name: &str, cap: CapabilityId) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            caps: vec![cap],
            name: name.to_string(),
        }
    }

    pub(super) fn call_count(&self) -> usize {
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
pub(super) fn effect_anf_with_arg(cap_name: &str) -> AnfIr {
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
pub(super) fn structured_effect_anf_with_arg(cap_name: &str) -> AnfIr {
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

pub(super) fn direct_host_call_write_wasm(cap_name: &str, out_ptr: i32, out_max: i32) -> Vec<u8> {
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

pub(super) fn instantiate_direct_host_call_write(
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

pub(super) fn profile_granting(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    cap: CapabilityId,
) -> RuntimeProfile {
    profile_granting_with_limits(wasm, manifest, cap, ResourceLimits::default())
}

pub(super) fn profile_granting_with_limits(
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

pub(super) fn profile_no_grants(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
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

pub(super) fn has_failed_capability_event(host: &RuntimeHost, cap: &CapabilityId) -> bool {
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

// ── Helpers for two-cap asymmetry regression tests ───────────────────────

/// Build a WASM module that calls `ail/host_call` twice in sequence:
/// first with `cap_a` (stored at ptr 0), then with `cap_b` (stored at ptr 64).
/// The module returns the i64 result of the second call.
///
/// Memory layout: [0..63] = cap_a name, [64..127] = cap_b name,
///                [128..129] = "op", [192..] = args (0 words).
pub(super) fn two_cap_host_call_wasm(cap_a: &str, cap_b: &str) -> Vec<u8> {
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
pub(super) fn two_cap_host_call_write_wasm(cap_a: &str, cap_b: &str) -> Vec<u8> {
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
pub(super) fn profile_granting_two_caps_with_limits(
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
