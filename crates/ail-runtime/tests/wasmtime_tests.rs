// ── ail-runtime::wasmtime_tests ──────────────────────────────────────────
//
// Task 3.2 (RED): Tests written BEFORE host.rs / RuntimeHost exist.
//
// Spec scenarios covered:
//  - Malformed WASM bytes are rejected at Wasmtime validation (WasmValidationError).
//  - WASM produced by ail-compiler validates and instantiates successfully.
//  - Failed preflight (hash mismatch) blocks Wasmtime invocation.
//  - RuntimeHost::new() succeeds (engine initialization is infallible from caller's view).

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, LiteralValue, SourceMap, StageHashes,
    emit_wasm, lower_to_anf, lower_to_core_ir,
};
use ail_core::semantic_graph::{NodeRef, SemanticGraph};
use ail_runtime::{
    CapabilityManifest, PreflightFailure, ResourceLimits, RuntimeError, RuntimeHost,
    RuntimeProfile, RuntimeValue, blake3_hex_of,
};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

/// Compile a minimal SemanticGraph through ail-compiler and return the WASM bytes.
fn compiler_wasm() -> Vec<u8> {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = VerificationReport {
        entries: vec![],
        ..Default::default()
    };
    let core = lower_to_core_ir(&graph, &report).expect("lower_to_core_ir failed");
    let anf = lower_to_anf(&core).expect("lower_to_anf failed");
    emit_wasm(&anf).expect("emit_wasm failed").wasm
}

/// Build a RuntimeProfile whose hashes match the provided WASM and manifest exactly.
fn matching_profile(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .expect("manifest CBOR hash must succeed");
    RuntimeProfile::new(
        "wasmtime-test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
}

fn sealed_anf(bindings: Vec<AnfBinding>) -> AnfIr {
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&bindings),
        bindings,
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

fn compiler_wasm_for_expr(expr: AnfExpr, name: &str) -> Vec<u8> {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: name.to_string(),
        expr,
    };
    let anf = sealed_anf(vec![binding]);
    emit_wasm(&anf).expect("emit_wasm failed").wasm
}

// ── Scenario 1: Malformed WASM is rejected ────────────────────────────────

// Garbage bytes that are not a valid WASM module must fail with
// WasmValidationError.  Preflight passes (correct hashes, no required caps)
// so the failure originates in Wasmtime's structural validator.
#[test]
fn malformed_wasm_rejected_at_validation() {
    let garbage: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, 0x02, 0x03];
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&garbage, &manifest);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&garbage, &manifest, &profile);

    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::WasmValidationError(_)
            ))
        ),
        "malformed WASM must produce WasmValidationError, got {result:?}"
    );

    let log = host.audit_log();
    assert_eq!(log.len(), 1, "exactly one audit event");
    assert!(
        !log.events()[0].is_passed(),
        "event must be PreflightFailed"
    );
}

// TRIANGULATE: completely empty byte slice is also malformed WASM.
#[test]
fn empty_bytes_rejected_at_validation() {
    let empty: Vec<u8> = vec![];
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&empty, &manifest);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&empty, &manifest, &profile);

    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::WasmValidationError(_)
            ))
        ),
        "empty bytes must produce WasmValidationError, got {result:?}"
    );
}

// ── Scenario 2: ail-compiler WASM validates and instantiates ─────────────

// WASM produced by the compiler pipeline must pass Wasmtime structural
// validation and produce an Ok(RuntimeInstance).
#[test]
fn compiler_wasm_validates_and_instantiates() {
    let wasm = compiler_wasm();
    let manifest = CapabilityManifest {
        module: "compiler-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);

    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("compiler-emitted WASM must instantiate");

    assert_eq!(instance.export_count(), 0);

    let log = host.audit_log();
    assert_eq!(log.len(), 1);
    assert!(log.events()[0].is_passed(), "event must be PreflightPassed");
}

#[test]
fn exported_i64_function_can_be_invoked() {
    let mut module = wasm_encoder::Module::new();
    let mut types = wasm_encoder::TypeSection::new();
    types.ty().function([], [wasm_encoder::ValType::I64]);
    module.section(&types);
    let mut functions = wasm_encoder::FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("answer", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);
    let mut codes = wasm_encoder::CodeSection::new();
    let mut function = wasm_encoder::Function::new([]);
    function.instruction(&wasm_encoder::Instruction::I64Const(42));
    function.instruction(&wasm_encoder::Instruction::End);
    codes.function(&function);
    module.section(&codes);
    let wasm = module.finish();

    let manifest = CapabilityManifest {
        module: "invoke-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let value = instance.invoke("answer", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn compiler_if_else_function_returns_taken_branch() {
    let wasm = compiler_wasm_for_expr(
        AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::If {
                cond: "flag".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
            }),
        },
        "fn.branch",
    );
    let manifest = CapabilityManifest {
        module: "compiler-if-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("compiler WASM must instantiate");

    let value = instance.invoke("branch", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(20));
}

#[test]
fn compiler_function_call_double_21_invokes_to_42() {
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.double".to_string(),
            expr: AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["x".to_string(), "x".to_string()],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Let {
                name: "n".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(21))),
                body: Box::new(AnfExpr::Call {
                    func: "double".to_string(),
                    args: vec!["n".to_string()],
                }),
            },
        },
    ]);
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "function-call-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let value = instance.invoke("main", &[]).expect("main must invoke");
    assert_eq!(value, RuntimeValue::I64(42));
}

// TRIANGULATE: the 8-byte WASM header (minimal valid module) also succeeds.
#[test]
fn minimal_wasm_header_validates_and_instantiates() {
    // magic + version = valid empty WASM module accepted by Wasmtime.
    let wasm: Vec<u8> = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let manifest = CapabilityManifest {
        module: "minimal-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);

    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("minimal WASM header must instantiate");

    assert_eq!(instance.export_count(), 0);
}

// ── Scenario 3: Failed preflight blocks Wasmtime ─────────────────────────

// If the WASM hash doesn't match, Wasmtime validation must NOT be attempted.
// The error must be HashMismatch, not WasmValidationError.
// Even if the WASM bytes happen to be valid, preflight failure is reported first.
#[test]
fn failed_preflight_blocks_wasmtime_invocation() {
    // Use valid WASM bytes so we can be sure the error is from preflight, not Wasmtime.
    let wasm = compiler_wasm();
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    // Profile with intentionally wrong module_hash.
    let profile = RuntimeProfile::new(
        "bad-hash-profile".to_string(),
        "wrong_hash_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        "a".repeat(64),
        manifest.blake3_hex().unwrap(),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    // Must be HashMismatch (preflight), NOT WasmValidationError (Wasmtime).
    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::HashMismatch { .. }
            ))
        ),
        "failed preflight must block Wasmtime — expected HashMismatch, got {result:?}"
    );
}
