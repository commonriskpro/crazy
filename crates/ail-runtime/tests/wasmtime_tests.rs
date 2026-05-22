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
    CapabilityManifest, PreflightFailure, ResourceLimits, RuntimeArg, RuntimeError, RuntimeHost,
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

fn instantiate_test_wasm(wasm: &[u8]) -> ail_runtime::RuntimeInstance {
    let manifest = CapabilityManifest {
        module: "invoke-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(wasm, &manifest);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM must instantiate")
}

fn sum_wasm(param_count: u32) -> Vec<u8> {
    let mut module = wasm_encoder::Module::new();
    let mut types = wasm_encoder::TypeSection::new();
    types.ty().function(
        vec![wasm_encoder::ValType::I64; param_count as usize],
        [wasm_encoder::ValType::I64],
    );
    module.section(&types);
    let mut functions = wasm_encoder::FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("sum", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);
    let mut codes = wasm_encoder::CodeSection::new();
    let mut function = wasm_encoder::Function::new([]);
    if param_count == 0 {
        function.instruction(&wasm_encoder::Instruction::I64Const(42));
    } else {
        function.instruction(&wasm_encoder::Instruction::LocalGet(0));
        for index in 1..param_count {
            function.instruction(&wasm_encoder::Instruction::LocalGet(index));
            function.instruction(&wasm_encoder::Instruction::I64Add);
        }
    }
    function.instruction(&wasm_encoder::Instruction::End);
    codes.function(&function);
    module.section(&codes);
    module.finish()
}

fn invoke_compiler_expr(expr: AnfExpr, name: &str) -> RuntimeValue {
    let wasm = compiler_wasm_for_expr(expr, name);
    let manifest = CapabilityManifest {
        module: format!("{name}-test"),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let export = name.rsplit('.').next().unwrap_or(name);
    instance.invoke(export, &[]).expect("invoke must succeed")
}

fn binary_i64_call(func: &str, left: i64, right: i64) -> AnfExpr {
    AnfExpr::Let {
        name: "a".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(left))),
        body: Box::new(AnfExpr::Let {
            name: "b".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(right))),
            body: Box::new(AnfExpr::Call {
                func: func.to_string(),
                args: vec!["a".to_string(), "b".to_string()],
            }),
        }),
    }
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
fn invoke_export_with_zero_args_still_works() {
    let wasm = sum_wasm(0);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance.invoke("sum", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_one_arg() {
    let wasm = sum_wasm(1);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance
        .invoke("sum", &[RuntimeArg::I64(42)])
        .expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_two_args() {
    let wasm = sum_wasm(2);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance
        .invoke("sum", &[RuntimeArg::I64(20), RuntimeArg::I64(22)])
        .expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn invoke_export_with_three_args() {
    let wasm = sum_wasm(3);
    let mut instance = instantiate_test_wasm(&wasm);

    let value = instance
        .invoke(
            "sum",
            &[
                RuntimeArg::I64(10),
                RuntimeArg::I64(12),
                RuntimeArg::I64(20),
            ],
        )
        .expect("invoke must succeed");

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
fn compiler_bool_literal_function_returns_i64_boolean() {
    assert_eq!(
        invoke_compiler_expr(AnfExpr::Literal(LiteralValue::Bool(true)), "fn.flag"),
        RuntimeValue::I64(1)
    );
}

#[test]
fn compiler_loop_break_with_value_returns_value() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Loop {
                body: Box::new(AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                }),
            },
            "fn.count_to_ten",
        ),
        RuntimeValue::I64(10)
    );
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

#[test]
fn compiler_function_with_param_invokes_with_runtime_arg() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.double".to_string(),
        expr: AnfExpr::Call {
            func: "i64.add".to_string(),
            args: vec!["x".to_string(), "x".to_string()],
        },
    }]);
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "param-function-call-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    let value = instance
        .invoke("double", &[RuntimeArg::I64(21)])
        .expect("double must invoke");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn compiler_arithmetic_ops_invoke_with_i64_results() {
    let cases = [
        ("add", binary_i64_call("i64.add", 3, 4), 7),
        ("mul", binary_i64_call("i64.mul", 6, 7), 42),
        ("sub", binary_i64_call("i64.sub", 50, 8), 42),
        ("div", binary_i64_call("i64.div_s", 84, 2), 42),
        ("mod", binary_i64_call("i64.rem_s", 85, 43), 42),
    ];

    for (name, expr, expected) in cases {
        assert_eq!(
            invoke_compiler_expr(expr, &format!("fn.{name}")),
            RuntimeValue::I64(expected),
            "{name} should evaluate to {expected}"
        );
    }
}

#[test]
fn compiler_comparison_ops_invoke_with_i64_boolean_results() {
    let cases = [
        ("eq", binary_i64_call("i64.eq", 42, 42), 1),
        ("lt", binary_i64_call("i64.lt_s", 3, 5), 1),
        ("ne", binary_i64_call("i64.ne", 42, 7), 1),
        ("le", binary_i64_call("i64.le_s", 42, 42), 1),
        ("gt", binary_i64_call("i64.gt_s", 9, 5), 1),
        ("ge", binary_i64_call("i64.ge_s", 42, 42), 1),
    ];

    for (name, expr, expected) in cases {
        assert_eq!(
            invoke_compiler_expr(expr, &format!("fn.{name}")),
            RuntimeValue::I64(expected),
            "{name} should evaluate to {expected}"
        );
    }
}

#[test]
fn compiler_unary_ops_invoke_with_i64_results() {
    let neg = AnfExpr::Let {
        name: "x".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(-42))),
        body: Box::new(AnfExpr::Call {
            func: "i64.neg".to_string(),
            args: vec!["x".to_string()],
        }),
    };
    let not = AnfExpr::Let {
        name: "x".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::Call {
            func: "i64.eqz".to_string(),
            args: vec!["x".to_string()],
        }),
    };

    assert_eq!(invoke_compiler_expr(neg, "fn.neg"), RuntimeValue::I64(42));
    assert_eq!(invoke_compiler_expr(not, "fn.not"), RuntimeValue::I64(1));
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
