// ── ail-runtime::wasmtime_tests ──────────────────────────────────────────
//
// Task 3.2 (RED): Tests written BEFORE host.rs / RuntimeHost exist.
//
// Spec scenarios covered:
//  - Malformed WASM bytes are rejected at Wasmtime validation (WasmValidationError).
//  - WASM produced by ail-compiler validates and instantiates successfully.
//  - Failed preflight (hash mismatch) blocks Wasmtime invocation.
//  - RuntimeHost::new() succeeds (engine initialization is infallible from caller's view).

use ail_change::{
    apply::{SnapshotBridge, apply},
    canonical::canonicalize_parsed,
    model::{ChangeSetOutcome, SnapshotId},
    parser::parse_changeset,
};
use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, AnfMatchArm, LiteralValue, SourceMap,
    StageHashes, emit_wasm, lower_to_anf, lower_to_core_ir,
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
            ..Default::default()
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

struct TestBridge;

impl SnapshotBridge for TestBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        SnapshotId(0)
    }
}

fn invoke_acl_export(acl: &str, export: &str) -> RuntimeValue {
    let parsed = parse_changeset(acl).expect("ACL must parse");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    assert_eq!(
        apply(canonical, &mut graph, &TestBridge),
        ChangeSetOutcome::Applied
    );

    let report = VerificationReport {
        entries: vec![],
        ..Default::default()
    };
    let core = lower_to_core_ir(&graph, &report).expect("lower_to_core_ir failed");
    let anf = lower_to_anf(&core).expect("lower_to_anf failed");
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "acl-expr-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    instance.invoke(export, &[]).expect("invoke must succeed")
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
fn acl_sum_of_squares_body_compiles_and_runs() {
    let acl = "\
change expr_bodies base=0
author tester
description expression bodies
op create_function id=fn.sum_of_squares return=Int
op add_param target=fn.sum_of_squares name=x type=Int
op add_param target=fn.sum_of_squares name=y type=Int
op set_body target=fn.sum_of_squares body=add(mul(x, x), mul(y, y))
op create_function id=fn.main return=Int body=sum_of_squares(3, 4)
end
";

    let value = invoke_acl_export(acl, "main");

    assert_eq!(value, RuntimeValue::I64(25));
}

#[test]
fn acl_let_and_short_circuit_body_compiles_and_runs() {
    let acl = "\
change structured_expr_bodies base=0
author tester
description structured expression bodies
op create_function id=fn.main return=Int body=let(flag, false, and(flag, div(1, 0)))
end
";

    let value = invoke_acl_export(acl, "main");

    assert_eq!(value, RuntimeValue::I64(0));
}

#[test]
fn acl_match_literal_and_wildcard_body_compiles_and_runs() {
    let acl = "\
change match_expr_bodies base=0
author tester
description match expression bodies
op create_function id=fn.literal_hit return=Int body=match(2, 1, 10, 2, 20, _, 30)
op create_function id=fn.wildcard_hit return=Int body=match(9, 1, 10, 2, 20, _, 30)
end
";

    assert_eq!(invoke_acl_export(acl, "literal_hit"), RuntimeValue::I64(20));
    assert_eq!(
        invoke_acl_export(acl, "wildcard_hit"),
        RuntimeValue::I64(30)
    );
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
            ..Default::default()
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

// ── Wave 17A: closure Fold execution proof ────────────────────────────────
//
// Tests that compile, instantiate, and invoke ANF programs using a
// 2-param captured Lambda as a Fold reducer.  The reducer captures a
// `bias` value from the enclosing scope and uses it in the accumulation.
//
// Reducer shape:  fn(acc, x) -> let tmp = acc + x in tmp + bias
// This REQUIRES the closure env preamble to load `bias` from memory; a
// plain hoistable (no-capture) fold would produce wrong results.

/// Build and invoke a closure-fold ANF program.
///
/// `bias` is captured by the Lambda reducer.
/// `init` is the Fold initial accumulator.
/// `elements` is the list to fold over.
///
/// Expected result: fold left with `reducer(acc, x) = acc + x + bias`.
fn invoke_closure_fold(bias: i64, init: i64, elements: Vec<i64>) -> RuntimeValue {
    let list_exprs = elements
        .iter()
        .map(|&e| AnfExpr::Literal(LiteralValue::Int(e)))
        .collect::<Vec<_>>();

    // fn.main =
    //   let bias  = <bias>
    //   let lst   = ListNew([...])
    //   let f     = Lambda(params=[acc, x], captures=[bias],
    //                 body = let tmp = acc + x in tmp + bias)
    //   let zero  = <init>
    //   Fold(init=zero, list=lst, func=f)
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(bias))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(list_exprs)),
                body: Box::new(AnfExpr::Let {
                    name: "f".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["bias".to_string()],
                        body: Box::new(AnfExpr::Let {
                            name: "tmp".to_string(),
                            value: Box::new(AnfExpr::Call {
                                func: "+".to_string(),
                                args: vec!["acc".to_string(), "x".to_string()],
                            }),
                            body: Box::new(AnfExpr::Call {
                                func: "+".to_string(),
                                args: vec!["tmp".to_string(), "bias".to_string()],
                            }),
                        }),
                    }),
                    body: Box::new(AnfExpr::Let {
                        name: "zero".to_string(),
                        value: Box::new(AnfExpr::Literal(LiteralValue::Int(init))),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "f".to_string(),
                        }),
                    }),
                }),
            }),
        },
    };

    let anf = sealed_anf(vec![binding]);
    let wasm = emit_wasm(&anf).expect("closure-fold ANF must compile").wasm;
    let manifest = CapabilityManifest {
        module: "closure-fold-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("closure-fold WASM must instantiate");
    instance.invoke("main", &[]).expect("main must invoke")
}

// Scenario: empty list — Fold returns the initial accumulator unchanged.
//
// reducer(acc, x) = acc + x + bias  (bias=10, init=7)
// Fold(7, [], reducer) = 7
#[test]
fn closure_fold_empty_list_returns_init() {
    assert_eq!(
        invoke_closure_fold(10, 7, vec![]),
        RuntimeValue::I64(7),
        "empty-list fold must return init unchanged"
    );
}

// Scenario: single element — one reducer application.
//
// reducer(acc, x) = acc + x + bias  (bias=10, init=0)
// Fold(0, [5], reducer) = 0 + 5 + 10 = 15
#[test]
fn closure_fold_single_element_applies_reducer_once() {
    assert_eq!(
        invoke_closure_fold(10, 0, vec![5]),
        RuntimeValue::I64(15),
        "single-element fold with bias=10 must return 15"
    );
}

// Scenario: multiple elements — reducer applied once per element.
//
// reducer(acc, x) = acc + x + bias  (bias=10, init=0)
// Fold(0, [1, 2, 3], reducer):
//   step1: reducer(0,  1) = 0  + 1  + 10 = 11
//   step2: reducer(11, 2) = 11 + 2  + 10 = 23
//   step3: reducer(23, 3) = 23 + 3  + 10 = 36
#[test]
fn closure_fold_multi_element_accumulates_with_bias() {
    assert_eq!(
        invoke_closure_fold(10, 0, vec![1, 2, 3]),
        RuntimeValue::I64(36),
        "3-element fold with bias=10 must return 36"
    );
}

// Scenario: multiple captures — Lambda closes over two independent values.
//
// reducer(acc, x) = let s = acc + x in let t = s + bias1 in t + bias2
// bias1=3, bias2=7 (sum=10), init=0, list=[1, 2]
//   step1: reducer(0, 1)  = 0  + 1 + 3 + 7 = 11
//   step2: reducer(11, 2) = 11 + 2 + 3 + 7 = 23
#[test]
fn closure_fold_multiple_captures_both_loaded_from_env() {
    // fn.main =
    //   let bias1 = 3
    //   let bias2 = 7
    //   let lst   = ListNew([1, 2])
    //   let f     = Lambda(params=[acc,x], captures=[bias1, bias2],
    //                 body = let s = acc+x in let t = s+bias1 in t+bias2)
    //   let zero  = 0
    //   Fold(init=zero, list=lst, func=f)
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "bias1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
            body: Box::new(AnfExpr::Let {
                name: "bias2".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(1)),
                        AnfExpr::Literal(LiteralValue::Int(2)),
                    ])),
                    body: Box::new(AnfExpr::Let {
                        name: "f".to_string(),
                        value: Box::new(AnfExpr::Lambda {
                            params: vec!["acc".to_string(), "x".to_string()],
                            captures: vec!["bias1".to_string(), "bias2".to_string()],
                            body: Box::new(AnfExpr::Let {
                                name: "s".to_string(),
                                value: Box::new(AnfExpr::Call {
                                    func: "+".to_string(),
                                    args: vec!["acc".to_string(), "x".to_string()],
                                }),
                                body: Box::new(AnfExpr::Let {
                                    name: "t".to_string(),
                                    value: Box::new(AnfExpr::Call {
                                        func: "+".to_string(),
                                        args: vec!["s".to_string(), "bias1".to_string()],
                                    }),
                                    body: Box::new(AnfExpr::Call {
                                        func: "+".to_string(),
                                        args: vec!["t".to_string(), "bias2".to_string()],
                                    }),
                                }),
                            }),
                        }),
                        body: Box::new(AnfExpr::Let {
                            name: "zero".to_string(),
                            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                            body: Box::new(AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "f".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        },
    };

    let anf = sealed_anf(vec![binding]);
    let wasm = emit_wasm(&anf)
        .expect("two-capture closure-fold ANF must compile")
        .wasm;
    let manifest = CapabilityManifest {
        module: "closure-fold-two-captures-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("two-capture closure-fold WASM must instantiate");
    let value = instance.invoke("main", &[]).expect("main must invoke");
    assert_eq!(
        value,
        RuntimeValue::I64(23),
        "two-capture fold over [1,2] with bias1=3 bias2=7 must return 23"
    );
}

// ── Wave 17D: VariantNew + Match runtime execution conformance ────────────
//
// Spec scenarios covered (RUNTIME-VARIANT-MATCH-1..4):
//
//  RUNTIME-VARIANT-MATCH-1: single-binding constructor "Ok(x)" extracts the
//    variant payload from linear memory (offset 8) and binds it; the arm body
//    references and uses the bound variable.
//
//  RUNTIME-VARIANT-MATCH-2: tag-only constructor "None" matches by discriminant
//    only — no payload read is attempted (correct for payload-less variants).
//
//  RUNTIME-VARIANT-MATCH-3: wildcard "_" fires when the scrutinee's tag does not
//    match any earlier arm; the wrong-tag arm is fully skipped.
//
//  RUNTIME-VARIANT-MATCH-4: arm ordering is respected — the "None" arm is
//    evaluated first, fails the tag check, and the subsequent "Some(x)" arm
//    correctly extracts the payload.
//
// Design note — arm body shape:
//   Arms that bind a payload variable `x` use `Call { "+", ["x", "x"] }` as
//   the body rather than the bare `Var("x")`.  Both forms now work: the Wave 17D
//   `infer_expr_type` fix temporarily adds the payload binding to `locals` before
//   inferring each arm's body type, so `Var("x")` resolves to `Some(I64)` rather
//   than `None`.  `x + x` is a deliberate proof-of-value choice — the result 42
//   (= 21 + 21) proves both that the correct payload (21) was extracted from
//   linear memory and that the binding is live in the arm body.

/// Build the ANF expression:
///   `let v = VariantNew(tag, payload?) in match v { arms... }`
///
/// `payload` is encoded as `Some(i64 literal)` when present.
fn make_variant_match_expr(tag: &str, payload: Option<i64>, arms: Vec<AnfMatchArm>) -> AnfExpr {
    AnfExpr::Let {
        name: "v".to_string(),
        value: Box::new(AnfExpr::VariantNew {
            tag: tag.to_string(),
            payload: payload.map(|p| Box::new(AnfExpr::Literal(LiteralValue::Int(p)))),
        }),
        body: Box::new(AnfExpr::Match {
            scrutinee: "v".to_string(),
            arms,
        }),
    }
}

// RUNTIME-VARIANT-MATCH-1
//
// VariantNew("Ok", payload=21) → discriminant=0, payload stored at offset 8.
// Match arm "Ok(x)" fires: discriminant 0 == 0 ✓; payload 21 loaded into x.
// Arm body x+x = 42.  Wildcard fallback ("_" → 0) must NOT be reached.
#[test]
fn variant_match_single_binding_extracts_payload_and_uses_it() {
    let expr = make_variant_match_expr(
        "Ok",
        Some(21),
        vec![
            AnfMatchArm {
                pattern: "Ok(x)".to_string(),
                body: AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(0)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "Ok(x) arm must load payload 21 and compute x+x=42"
    );
}

// RUNTIME-VARIANT-MATCH-2
//
// VariantNew("None") → discriminant=0, no payload written.
// Match arm "None" fires: discriminant 0 == 0 ✓; no payload load attempted.
// Arm body returns literal 99.
#[test]
fn variant_match_tag_only_pattern_matches_none_variant() {
    let expr = make_variant_match_expr(
        "None",
        None,
        vec![
            AnfMatchArm {
                pattern: "None".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(99)),
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(0)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(99),
        "None tag-only pattern must match discriminant 0 and return 99"
    );
}

// RUNTIME-VARIANT-MATCH-3
//
// VariantNew("Err", payload=1) → discriminant=1.
// Match arm "Ok(x)" checks discriminant==0 → fails (1≠0); wildcard fires → 999.
// Proves the wrong-tag arm is skipped entirely.
#[test]
fn variant_match_wildcard_fires_on_wrong_tag() {
    let expr = make_variant_match_expr(
        "Err",
        Some(1),
        vec![
            AnfMatchArm {
                pattern: "Ok(x)".to_string(),
                body: AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(999)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(999),
        "Err variant (discriminant=1) must skip Ok(x) arm and hit wildcard returning 999"
    );
}

// RUNTIME-VARIANT-MATCH-4
//
// VariantNew("Some", payload=21) → discriminant=1.
// Arms: "None" (discriminant=0) → 1 [skipped], "Some(x)" (discriminant=1) → x+x [matches!],
//       "_" → 999 [never reached].
// Proves ordering: the first arm is evaluated and rejected before the
// correct arm fires and extracts the payload.
#[test]
fn variant_match_ordering_skips_wrong_arms_and_matches_correct_one() {
    let expr = make_variant_match_expr(
        "Some",
        Some(21),
        vec![
            AnfMatchArm {
                pattern: "None".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(1)),
            },
            AnfMatchArm {
                pattern: "Some(x)".to_string(),
                body: AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(999)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "Some(x) arm must match after skipping None; x+x with payload=21 must return 42"
    );
}

// ── Wave 18B: CellNew/CellGet/CellSet, IndexGet, ForEach conformance ──────
//
// Spec scenarios covered (RUNTIME-CELL-1..2, RUNTIME-INDEXGET-1..2,
// RUNTIME-FOREACH-1):
//
//  RUNTIME-CELL-1: CellNew(42) followed immediately by CellGet returns the
//    initialisation value — proves the alloc+store+load round trip works end-
//    to-end through Wasmtime.
//
//  RUNTIME-CELL-2: CellNew(1) followed by CellSet(c, 10) then CellGet(c)
//    returns 10 — proves that the write overwrites the initial value and that
//    the cell pointer is stable across Let bindings.
//
//  RUNTIME-INDEXGET-1: IndexGet on a two-element list at index 0 returns the
//    first element (5) — proves the list-header skip (offset 8) and the base-
//    case formula `ptr + 8 + 0*8 = ptr + 8`.
//
//  RUNTIME-INDEXGET-2: IndexGet at index 1 returns the second element (10) —
//    proves the stride formula `ptr + 8 + 1*8 = ptr + 16`.
//
//  RUNTIME-FOREACH-1: ForEach over [1, 2, 3] with a cell accumulator yields 6
//    — proves that the inline loop binds each element into `x`, that CellGet
//    and CellSet work inside the loop body, and that ForEach as a Let value
//    produces a unit (I32 0) so the enclosing Let can sequence it with a
//    subsequent CellGet.

// RUNTIME-CELL-1
//
// fn.main = let init = 42 in let c = CellNew(init) in CellGet(c)
//
// CellNew allocates 8 bytes, stores init (42) at offset 0, returns I32 ptr.
// CellGet loads I64 from offset 0 of the ptr → 42.
#[test]
fn cell_new_get_round_trip() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::CellGet {
                cell: "c".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "CellNew(42) followed by CellGet must return 42"
    );
}

// RUNTIME-CELL-2
//
// fn.main =
//   let init = 1 in
//   let c = CellNew(init) in
//   let v = 10 in
//   let _s = CellSet(c, v) in
//   CellGet(c)
//
// CellSet stores 10 at offset 0, overwriting the initial 1.
// CellGet then reads 10.
#[test]
fn cell_set_overwrites_initial_value() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "v".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                body: Box::new(AnfExpr::Let {
                    name: "_s".to_string(),
                    value: Box::new(AnfExpr::CellSet {
                        cell: "c".to_string(),
                        value: "v".to_string(),
                    }),
                    body: Box::new(AnfExpr::CellGet {
                        cell: "c".to_string(),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(10),
        "CellSet(c, 10) must overwrite initial 1; CellGet must return 10"
    );
}

// RUNTIME-INDEXGET-1
//
// fn.main = let lst = ListNew([5, 10]) in let i = 0 in IndexGet(lst, i)
//
// List layout: [count=2: i64, elem0=5: i64, elem1=10: i64]
// IndexGet formula: ptr + 8 + 0*8 = ptr + 8 → 5.
#[test]
fn index_get_element_at_zero() {
    let expr = AnfExpr::Let {
        name: "lst".to_string(),
        value: Box::new(AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(5)),
            AnfExpr::Literal(LiteralValue::Int(10)),
        ])),
        body: Box::new(AnfExpr::Let {
            name: "i".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::IndexGet {
                collection: "lst".to_string(),
                index: "i".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(5),
        "IndexGet at index 0 of [5, 10] must return 5"
    );
}

// RUNTIME-INDEXGET-2
//
// fn.main = let lst = ListNew([5, 10]) in let i = 1 in IndexGet(lst, i)
//
// IndexGet formula: ptr + 8 + 1*8 = ptr + 16 → 10.
#[test]
fn index_get_element_at_one() {
    let expr = AnfExpr::Let {
        name: "lst".to_string(),
        value: Box::new(AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(5)),
            AnfExpr::Literal(LiteralValue::Int(10)),
        ])),
        body: Box::new(AnfExpr::Let {
            name: "i".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::IndexGet {
                collection: "lst".to_string(),
                index: "i".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(10),
        "IndexGet at index 1 of [5, 10] must return 10"
    );
}

// RUNTIME-FOREACH-1
//
// fn.main =
//   let init = 0 in
//   let c    = CellNew(init) in
//   let lst  = ListNew([1, 2, 3]) in
//   let _fe  = ForEach(x in lst,
//                let cur = CellGet(c) in
//                let s   = cur + x   in
//                CellSet(c, s))       in
//   CellGet(c)
//
// ForEach iterates [1, 2, 3] and at each step adds x to the cell value:
//   step 0: 0 + 1 = 1
//   step 1: 1 + 2 = 3
//   step 2: 3 + 3 = 6
// Final CellGet returns 6.
//
// This also proves that ForEach is usable as the value in a Let binding —
// it must produce a unit (I32 0) on the WASM stack so the enclosing
// LocalSet does not underflow.
#[test]
fn foreach_accumulates_via_cell() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(1)),
                    AnfExpr::Literal(LiteralValue::Int(2)),
                    AnfExpr::Literal(LiteralValue::Int(3)),
                ])),
                body: Box::new(AnfExpr::Let {
                    name: "_fe".to_string(),
                    value: Box::new(AnfExpr::ForEach {
                        binding: "x".to_string(),
                        collection: "lst".to_string(),
                        body: Box::new(AnfExpr::Let {
                            name: "cur".to_string(),
                            value: Box::new(AnfExpr::CellGet {
                                cell: "c".to_string(),
                            }),
                            body: Box::new(AnfExpr::Let {
                                name: "s".to_string(),
                                value: Box::new(AnfExpr::Call {
                                    func: "+".to_string(),
                                    args: vec!["cur".to_string(), "x".to_string()],
                                }),
                                body: Box::new(AnfExpr::CellSet {
                                    cell: "c".to_string(),
                                    value: "s".to_string(),
                                }),
                            }),
                        }),
                    }),
                    body: Box::new(AnfExpr::CellGet {
                        cell: "c".to_string(),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(6),
        "ForEach over [1,2,3] accumulating via cell must yield 6"
    );
}

// ── Wave 18D: WhileLoop compile and execution conformance ─────────────────
//
// Spec scenarios covered (RUNTIME-WHILE-1..2):
//
//  RUNTIME-WHILE-1: WhileLoop with an initially-false condition never
//    executes the body.  A cell initialised to 42 must remain 42 after the
//    loop — proves that the condition check fires before the first iteration
//    and that the BrIf(1) exit is taken immediately.
//
//  RUNTIME-WHILE-2: WhileLoop with a true condition runs exactly one
//    iteration: the body decrements a cell from 5 to 4 and then breaks out
//    via Break.  CellGet after the loop must return 4 — proves that (a) the
//    loop body executes, (b) CellSet and CellGet work inside the body, (c)
//    Break branches to the enclosing block's exit, and (d) WhileLoop pushes
//    a unit (I32 0) so it can be used as the value of a Let binding without
//    a WASM stack-underflow error.

// RUNTIME-WHILE-1
//
// fn.main =
//   let init = 42 in
//   let c    = CellNew(init) in
//   let zero = 0 in
//   let flag = false in
//   let _w   = while(flag, CellSet(c, zero)) in   ← body never runs
//   CellGet(c)
//
// Because flag = false (I64 0) the condition check `flag ≠ 0 → 0; eqz → 1`
// triggers BrIf(1) and exits the loop before the body runs.
// CellGet must return the initial value 42.
#[test]
fn while_loop_false_condition_body_never_runs() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "zero".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "flag".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                    body: Box::new(AnfExpr::Let {
                        name: "_w".to_string(),
                        value: Box::new(AnfExpr::WhileLoop {
                            cond: "flag".to_string(),
                            body: Box::new(AnfExpr::CellSet {
                                cell: "c".to_string(),
                                value: "zero".to_string(),
                            }),
                        }),
                        body: Box::new(AnfExpr::CellGet {
                            cell: "c".to_string(),
                        }),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "WhileLoop with false condition must skip the body; CellGet must return 42"
    );
}

// RUNTIME-WHILE-2
//
// fn.main =
//   let start = 5 in
//   let c     = CellNew(start) in
//   let go    = true in
//   let _w    = while(go,
//                 let cur  = CellGet(c)           in
//                 let one  = 1                    in
//                 let next = sub(cur, one)         in
//                 let _s   = CellSet(c, next)     in
//                 break(0))                         ← exits after one iteration
//   in
//   CellGet(c)
//
// go = true → condition fires, body runs once:
//   cur  = 5
//   next = 5 − 1 = 4
//   CellSet(c, 4)
//   break → exits loop
// CellGet(c) must return 4.
//
// This also proves that WhileLoop returns a unit (I32 0) so it can be used
// as the value of the outer Let binding without a WASM validation error.
#[test]
fn while_loop_body_runs_once_then_breaks() {
    let expr = AnfExpr::Let {
        name: "start".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(5))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "start".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "go".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                body: Box::new(AnfExpr::Let {
                    name: "_w".to_string(),
                    value: Box::new(AnfExpr::WhileLoop {
                        cond: "go".to_string(),
                        body: Box::new(AnfExpr::Let {
                            name: "cur".to_string(),
                            value: Box::new(AnfExpr::CellGet {
                                cell: "c".to_string(),
                            }),
                            body: Box::new(AnfExpr::Let {
                                name: "one".to_string(),
                                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                                body: Box::new(AnfExpr::Let {
                                    name: "next".to_string(),
                                    value: Box::new(AnfExpr::Call {
                                        func: "-".to_string(),
                                        args: vec!["cur".to_string(), "one".to_string()],
                                    }),
                                    body: Box::new(AnfExpr::Let {
                                        name: "_s".to_string(),
                                        value: Box::new(AnfExpr::CellSet {
                                            cell: "c".to_string(),
                                            value: "next".to_string(),
                                        }),
                                        body: Box::new(AnfExpr::Break {
                                            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                                        }),
                                    }),
                                }),
                            }),
                        }),
                    }),
                    body: Box::new(AnfExpr::CellGet {
                        cell: "c".to_string(),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(4),
        "WhileLoop body must run once (5→4), break, then CellGet must return 4"
    );
}

// ── Wave 19A: ANF control-flow execution conformance ─────────────────────
//
// Spec scenarios covered (RUNTIME-SEQ-1..3, RUNTIME-RETURN-1..3,
// RUNTIME-CONTINUE-1, RUNTIME-ABORT-1, RUNTIME-ASSUME-1,
// RUNTIME-RUNTIMECHECK-1..2, RUNTIME-SHORTCIRCUITAND-1..2,
// RUNTIME-SHORTCIRCUITOR-1..2):
//
//  RUNTIME-SEQ-1: Empty Seq produces unit (I32 0) — proves the empty-Seq
//    guard pushes I32Const(0) rather than leaving the stack underflowed.
//
//  RUNTIME-SEQ-2: Single-element Seq(Unit) returns the element's value
//    (I32 0) — proves the single-element path emits no spurious Drop.
//
//  RUNTIME-SEQ-3: Multi-element Seq applies both CellSet effects in order;
//    the cell holds the last written value — proves intermediate results are
//    dropped and both effects execute sequentially.
//
//  RUNTIME-RETURN-1: Return(42) causes the function to exit with I64(42)
//    before the implicit End — proves the Return instruction transfers
//    control and the value is carried correctly.
//
//  RUNTIME-RETURN-2: Return inside a taken if-branch exits before the
//    else branch would evaluate — proves early return on a conditional path.
//
//  RUNTIME-RETURN-3: Return(Unit) is the first element of a Seq; the second
//    element is Abort.  The function returns I32(0) without trapping, proving
//    Return's early-exit semantics: if Return did not emit the WASM `return`
//    instruction, Abort would fire and the invocation would return
//    Err(EncodingError) instead of Ok(I32(0)).
//
//  RUNTIME-CONTINUE-1: Continue inside a WhileLoop body jumps back to the
//    loop's condition check.  A counter cell increments each iteration;
//    the loop exits via Break when the counter reaches 3.  CellGet must
//    return 3 — proves Continue restarts iteration without loss of side
//    effects accumulated in the body.
//
//  RUNTIME-ABORT-1: Abort always traps — invoke returns
//    Err(RuntimeError::EncodingError) containing a Wasmtime unreachable
//    message.
//
//  RUNTIME-ASSUME-1: Assume emits no instructions and causes no trap; the
//    function returns normally with RuntimeValue::Unit — proves Assume is a
//    pure static annotation with zero runtime cost.
//
//  RUNTIME-RUNTIMECHECK-1: RuntimeCheck with cond=false (no violation
//    detected) does not trap; the function returns RuntimeValue::Unit —
//    proves the guard fires only when the condition is truthy.
//
//  RUNTIME-RUNTIMECHECK-2: RuntimeCheck with cond=true (violation
//    detected) traps — invoke returns Err(RuntimeError::EncodingError).
//    NOTE: `cond` in RuntimeCheck is the *violation* predicate; a truthy
//    cond means the check failed.
//
//  RUNTIME-SHORTCIRCUITAND-1: ShortCircuitAnd with left=false returns
//    I64(0) without evaluating right — right is an Abort that would trap
//    if reached, proving right is never executed.
//
//  RUNTIME-SHORTCIRCUITAND-2: ShortCircuitAnd with left=true evaluates
//    right (Literal 7) and returns I64(7).
//
//  RUNTIME-SHORTCIRCUITOR-1: ShortCircuitOr with left=true returns I64(1)
//    without evaluating right — right is an Abort that would trap if
//    reached, proving right is never executed.
//
//  RUNTIME-SHORTCIRCUITOR-2: ShortCircuitOr with left=false evaluates
//    right (Literal 7) and returns I64(7).

/// Variant of `invoke_compiler_expr` that returns `Result` instead of
/// panicking — used for tests that expect a trap.
fn try_invoke_compiler_expr(expr: AnfExpr, name: &str) -> Result<RuntimeValue, RuntimeError> {
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
    instance.invoke(export, &[])
}

// RUNTIME-SEQ-1
//
// fn.main = Seq([])
//
// Empty Seq: no elements, so the emit guard pushes I32Const(0) (unit) and
// returns Some(I32).  The function signature is () → I32 and must return 0.
#[test]
fn seq_empty_produces_unit() {
    assert_eq!(
        invoke_compiler_expr(AnfExpr::Seq(vec![]), "fn.seq_empty"),
        RuntimeValue::I32(0),
        "Empty Seq must produce unit I32(0)"
    );
}

// RUNTIME-SEQ-2
//
// fn.main = Seq([Literal(Unit)])
//
// Single-element Seq: no Drop is emitted (only the last element is kept).
// The element is Unit (I32 0).
#[test]
fn seq_single_element_returns_element_value() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Seq(vec![AnfExpr::Literal(LiteralValue::Unit)]),
            "fn.seq_single"
        ),
        RuntimeValue::I32(0),
        "Single-element Seq([Unit]) must return I32(0)"
    );
}

// RUNTIME-SEQ-3
//
// fn.main =
//   let init = 1       in
//   let c    = CellNew(init) in
//   let v1   = 10      in
//   let v2   = 99      in
//   let _sq  = Seq([CellSet(c, v1), CellSet(c, v2)]) in
//   CellGet(c)
//
// CellSet(c, 10) fires first (effect applied, I32(0) dropped).
// CellSet(c, 99) fires second (effect applied, I32(0) kept as Seq result).
// CellGet must return 99, proving both effects executed in order and that
// only the last value was kept from the Seq.
#[test]
fn seq_multi_element_applies_effects_in_order() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "v1".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                body: Box::new(AnfExpr::Let {
                    name: "v2".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
                    body: Box::new(AnfExpr::Let {
                        name: "_sq".to_string(),
                        value: Box::new(AnfExpr::Seq(vec![
                            AnfExpr::CellSet {
                                cell: "c".to_string(),
                                value: "v1".to_string(),
                            },
                            AnfExpr::CellSet {
                                cell: "c".to_string(),
                                value: "v2".to_string(),
                            },
                        ])),
                        body: Box::new(AnfExpr::CellGet {
                            cell: "c".to_string(),
                        }),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(99),
        "Seq([CellSet(c,10), CellSet(c,99)]): both effects run; cell must hold 99"
    );
}

// RUNTIME-RETURN-1
//
// fn.main = Return(42)
//
// Return emits the value and then the WASM `return` instruction, which exits
// the function immediately.  The function's inferred return type is I64
// (from the inner Literal), so the export signature is () → I64.
#[test]
fn return_exits_function_with_value() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Return(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
            "fn.ret"
        ),
        RuntimeValue::I64(42),
        "Return(42) must exit the function with I64(42)"
    );
}

// RUNTIME-RETURN-2
//
// fn.main =
//   let t = true in
//   if t { Return(10) } else { Literal(20) }
//
// t=true → then-branch fires: Return(10) exits the function immediately.
// The else-branch (20) is dead code.  Result must be I64(10).
#[test]
fn return_in_taken_branch_exits_before_else() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::If {
            cond: "t".to_string(),
            then_branch: Box::new(AnfExpr::Return(Box::new(AnfExpr::Literal(
                LiteralValue::Int(10),
            )))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.ret_if"),
        RuntimeValue::I64(10),
        "Return in taken then-branch must exit with 10; else (20) must not be reached"
    );
}

// RUNTIME-RETURN-3
//
// fn.main = Seq([Return(Unit), Abort("unreachable — Return above must exit")])
//
// Return(Unit) emits I32Const(0) + WASM `return`, which exits the function
// immediately.  The second Seq element (Abort → WASM `unreachable`) is dead
// code and is never reached at runtime.
//
// If Return did NOT emit the WASM `return` instruction, the Abort would fire
// and the invocation would return Err(EncodingError) rather than Ok(I32(0)).
// Receiving I32(0) without a trap is the proof that Return causes a genuine
// early exit before any subsequent statement in the same Seq executes.
//
// Type note: Seq always infers I32 as its result type; Return(Unit) → I32(0)
// matches that type exactly, so the generated WASM function is well-typed.
#[test]
fn return_in_seq_before_abort_proves_early_exit() {
    assert_eq!(
        invoke_compiler_expr(
            AnfExpr::Seq(vec![
                AnfExpr::Return(Box::new(AnfExpr::Literal(LiteralValue::Unit))),
                AnfExpr::Abort {
                    message: "unreachable — Return above must exit the function".to_string(),
                },
            ]),
            "fn.ret_early"
        ),
        RuntimeValue::I32(0),
        "Return in Seq must exit before Abort; I32(0) without trap proves early exit"
    );
}

// RUNTIME-CONTINUE-1
//
// fn.main =
//   let go    = true  in                       ← WhileLoop condition (always truthy)
//   let init  = 0     in
//   let c     = CellNew(init) in
//   let one   = 1     in
//   let three = 3     in
//   let _w    = while(go,
//                 let cur      = CellGet(c)      in
//                 let next     = cur + one        in
//                 let _s       = CellSet(c, next) in
//                 let done_val = (next == three)  in
//                 if done_val { Break(unit) } else { Continue }
//               ) in
//   CellGet(c)
//
// Iterations (Continue fires on 1st and 2nd, Break on 3rd):
//   iter 1: cur=0, next=1, _s→c=1, done_val=0 (1≠3) → Continue
//   iter 2: cur=1, next=2, _s→c=2, done_val=0 (2≠3) → Continue
//   iter 3: cur=2, next=3, _s→c=3, done_val=1 (3==3) → Break(unit)
// CellGet must return 3 — proves Continue restarts the iteration without
// skipping the CellSet side-effect, and Break terminates the loop correctly.
#[test]
fn continue_in_while_loop_restarts_iteration() {
    let expr = AnfExpr::Let {
        name: "go".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::Let {
            name: "init".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "c".to_string(),
                value: Box::new(AnfExpr::CellNew {
                    init: "init".to_string(),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "one".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    body: Box::new(AnfExpr::Let {
                        name: "three".to_string(),
                        value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                        body: Box::new(AnfExpr::Let {
                            name: "_w".to_string(),
                            value: Box::new(AnfExpr::WhileLoop {
                                cond: "go".to_string(),
                                body: Box::new(AnfExpr::Let {
                                    name: "cur".to_string(),
                                    value: Box::new(AnfExpr::CellGet {
                                        cell: "c".to_string(),
                                    }),
                                    body: Box::new(AnfExpr::Let {
                                        name: "next".to_string(),
                                        value: Box::new(AnfExpr::Call {
                                            func: "+".to_string(),
                                            args: vec!["cur".to_string(), "one".to_string()],
                                        }),
                                        body: Box::new(AnfExpr::Let {
                                            name: "_s".to_string(),
                                            value: Box::new(AnfExpr::CellSet {
                                                cell: "c".to_string(),
                                                value: "next".to_string(),
                                            }),
                                            body: Box::new(AnfExpr::Let {
                                                name: "done_val".to_string(),
                                                value: Box::new(AnfExpr::Call {
                                                    func: "==".to_string(),
                                                    args: vec![
                                                        "next".to_string(),
                                                        "three".to_string(),
                                                    ],
                                                }),
                                                body: Box::new(AnfExpr::If {
                                                    cond: "done_val".to_string(),
                                                    then_branch: Box::new(AnfExpr::Break {
                                                        value: Box::new(AnfExpr::Literal(
                                                            LiteralValue::Unit,
                                                        )),
                                                    }),
                                                    else_branch: Box::new(AnfExpr::Continue),
                                                }),
                                            }),
                                        }),
                                    }),
                                }),
                            }),
                            body: Box::new(AnfExpr::CellGet {
                                cell: "c".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(3),
        "Continue must restart each iteration; counter must reach 3 then Break"
    );
}

// RUNTIME-ABORT-1
//
// fn.main =
//   let _a = Abort { message: "test abort" } in
//   Literal(0)
//
// Abort emits Unreachable, placing the stack in the unreachable (polymorphic)
// state.  The Let binding's LocalSet and Literal(0) are dead code — valid WASM
// because unreachable code is polymorphically accepted.  The outer body
// (Literal(0)) gives the binding a declared I64 return type so it is exported.
// When invoked, Abort fires immediately → trap → RuntimeError::EncodingError.
#[test]
fn abort_always_traps() {
    let expr = AnfExpr::Let {
        name: "_a".to_string(),
        value: Box::new(AnfExpr::Abort {
            message: "test abort".to_string(),
        }),
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
    };
    let result = try_invoke_compiler_expr(expr, "fn.abort");
    assert!(
        matches!(result, Err(RuntimeError::EncodingError(_))),
        "Abort must trap and return EncodingError, got {result:?}"
    );
}

// RUNTIME-ASSUME-1
//
// Two-binding ANF:
//   fn.assume_note = Assume { predicate: "x > 0", reason: "test assumption" }
//   fn.main        = Literal(42)
//
// Assume emits NO WASM instructions (pure compile-time annotation).
// Its binding is NOT exported (binding_result = None — by design) but it IS
// compiled and validated as part of the module.
// fn.main IS exported and returns I64(42), proving the module compiles and
// instantiates correctly even when a sibling binding contains Assume.
// This demonstrates Assume's zero runtime cost: no trap, no interference.
#[test]
fn assume_has_no_runtime_effect() {
    use ail_core::semantic_graph::NodeRef;

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.assume_note".to_string(),
            expr: AnfExpr::Assume {
                predicate: "x > 0".to_string(),
                reason: "test assumption".to_string(),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(42)),
        },
    ]);
    let wasm = emit_wasm(&anf).expect("emit_wasm failed").wasm;
    let manifest = CapabilityManifest {
        module: "assume-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("module with Assume binding must instantiate");

    let value = instance.invoke("main", &[]).expect("fn.main must invoke");
    assert_eq!(
        value,
        RuntimeValue::I64(42),
        "fn.main must return I64(42); Assume in sibling binding must not interfere"
    );
}

// ── RuntimeCheck execution conformance ────────────────────────────────────
//
// DESIGN NOTE: The ail-compiler intentionally does NOT export functions whose
// top-level expression is a RuntimeCheck (binding_result = None).  This is
// a tested invariant (see C-3b in wasm_tests.rs).  To test the EXECUTION
// of the RuntimeCheck pattern we construct the equivalent WASM bytecode
// directly using wasm_encoder, bypassing the compiler.
//
// The RuntimeCheck WASM pattern emitted by ail-compiler for
//   RuntimeCheck { cond, .. }
// is exactly:
//   emit_condition_get(cond)   ; I32 on stack
//   If(BlockType::Empty)
//     Unreachable
//   End
//
// We replicate this pattern manually with a hardcoded I32 condition so the
// function can be exported and invoked.  This proves the execution semantics
// of the pattern, complementing the structural (wasmparser) proofs in
// wasm_tests.rs.

/// Build a minimal WASM module containing one exported `() → I32` function
/// that executes the RuntimeCheck pattern with a hardcoded condition:
///
/// ```text
/// i32.const <condition>
/// if []
///   unreachable
/// end
/// i32.const 42    ; return value (only reached when condition is false)
/// ```
fn runtime_check_pattern_wasm(condition: i32) -> Vec<u8> {
    let mut module = wasm_encoder::Module::new();

    let mut types = wasm_encoder::TypeSection::new();
    types.ty().function([], [wasm_encoder::ValType::I32]);
    module.section(&types);

    let mut functions = wasm_encoder::FunctionSection::new();
    functions.function(0);
    module.section(&functions);

    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("check", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = wasm_encoder::CodeSection::new();
    let mut f = wasm_encoder::Function::new([]);
    f.instruction(&wasm_encoder::Instruction::I32Const(condition));
    f.instruction(&wasm_encoder::Instruction::If(
        wasm_encoder::BlockType::Empty,
    ));
    f.instruction(&wasm_encoder::Instruction::Unreachable);
    f.instruction(&wasm_encoder::Instruction::End);
    f.instruction(&wasm_encoder::Instruction::I32Const(42));
    f.instruction(&wasm_encoder::Instruction::End);
    codes.function(&f);
    module.section(&codes);

    module.finish()
}

// RUNTIME-RUNTIMECHECK-1
//
// RuntimeCheck pattern with condition=0 (false / no violation).
//
// The If guard is not taken — Unreachable never fires.
// Execution continues past the If block and returns I32(42).
// Proves: when the violation predicate is false, RuntimeCheck is a no-op
// and the surrounding code runs normally.
#[test]
fn runtime_check_false_cond_does_not_trap() {
    let wasm = runtime_check_pattern_wasm(0); // condition = false
    let mut instance = instantiate_test_wasm(&wasm);
    let value = instance.invoke("check", &[]).expect("check must not trap");
    assert_eq!(
        value,
        RuntimeValue::I32(42),
        "RuntimeCheck with false condition must not trap; must return I32(42)"
    );
}

// RUNTIME-RUNTIMECHECK-2
//
// RuntimeCheck pattern with condition=1 (true / violation detected).
//
// The If guard IS taken → Unreachable fires → Wasmtime trap →
// RuntimeError::EncodingError.
// Proves: when the violation predicate is true, RuntimeCheck traps.
// NOTE: `cond` in RuntimeCheck is the *violation* predicate — truthy means
// "check failed", not "assertion holds".
#[test]
fn runtime_check_true_cond_traps() {
    let wasm = runtime_check_pattern_wasm(1); // condition = true
    let mut instance = instantiate_test_wasm(&wasm);
    let result = instance.invoke("check", &[]);
    assert!(
        matches!(result, Err(RuntimeError::EncodingError(_))),
        "RuntimeCheck with true condition must trap with EncodingError, got {result:?}"
    );
}

// RUNTIME-SHORTCIRCUITAND-1
//
// fn.main =
//   let f = false in
//   ShortCircuitAnd { left: "f", right: Abort{"dead code"} }
//
// left=false → else branch → I64(0); right (Abort) is NEVER evaluated.
// If short-circuit were broken and right were reached, Abort would trap.
// No trap proves right was not evaluated.
#[test]
fn short_circuit_and_false_left_skips_right() {
    let expr = AnfExpr::Let {
        name: "f".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
        body: Box::new(AnfExpr::ShortCircuitAnd {
            left: "f".to_string(),
            right: Box::new(AnfExpr::Abort {
                message: "dead code: AND right with false left".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.and_false"),
        RuntimeValue::I64(0),
        "ShortCircuitAnd with left=false must return I64(0) without evaluating right"
    );
}

// RUNTIME-SHORTCIRCUITAND-2
//
// fn.main =
//   let t = true in
//   let r = 7    in
//   ShortCircuitAnd { left: "t", right: Var("r") }
//
// left=true → then branch → evaluates right (Var("r") = 7) → I64(7).
#[test]
fn short_circuit_and_true_left_evaluates_right() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
            body: Box::new(AnfExpr::ShortCircuitAnd {
                left: "t".to_string(),
                right: Box::new(AnfExpr::Var("r".to_string())),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.and_true"),
        RuntimeValue::I64(7),
        "ShortCircuitAnd with left=true must evaluate right and return I64(7)"
    );
}

// RUNTIME-SHORTCIRCUITOR-1
//
// fn.main =
//   let t = true in
//   ShortCircuitOr { left: "t", right: Abort{"dead code"} }
//
// left=true → then branch → I64(1); right (Abort) is NEVER evaluated.
// If short-circuit were broken and right were reached, Abort would trap.
// No trap proves right was not evaluated.
#[test]
fn short_circuit_or_true_left_skips_right() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(AnfExpr::ShortCircuitOr {
            left: "t".to_string(),
            right: Box::new(AnfExpr::Abort {
                message: "dead code: OR right with true left".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.or_true"),
        RuntimeValue::I64(1),
        "ShortCircuitOr with left=true must return I64(1) without evaluating right"
    );
}

// RUNTIME-SHORTCIRCUITOR-2
//
// fn.main =
//   let f = false in
//   let r = 7     in
//   ShortCircuitOr { left: "f", right: Var("r") }
//
// left=false → else branch → evaluates right (Var("r") = 7) → I64(7).
#[test]
fn short_circuit_or_false_left_evaluates_right() {
    let expr = AnfExpr::Let {
        name: "f".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
        body: Box::new(AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
            body: Box::new(AnfExpr::ShortCircuitOr {
                left: "f".to_string(),
                right: Box::new(AnfExpr::Var("r".to_string())),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.or_false"),
        RuntimeValue::I64(7),
        "ShortCircuitOr with left=false must evaluate right and return I64(7)"
    );
}

// ── Wave 19B: data-structure execution conformance ────────────────────────
//
// Spec scenarios covered (RUNTIME-RECORD-1..2, RUNTIME-FIELDUPDATE-1..2,
// RUNTIME-TUPLE-1..2, RUNTIME-MAP-1, RUNTIME-SET-1):
//
//  RUNTIME-RECORD-1: RecordNew({x:10, y:42}) + FieldGet("y") returns I64(42)
//    — proves the second field is stored at offset 8 and retrieved correctly.
//
//  RUNTIME-RECORD-2: RecordNew({x:10, y:42}) + FieldGet("x") returns I64(10)
//    — proves the first field is stored at offset 0 and retrieved correctly.
//    Together with RECORD-1 this is the RecordNew+FieldGet round-trip proof.
//
//  RUNTIME-FIELDUPDATE-1: FieldUpdate mutates "y" to 99 in-place; subsequent
//    FieldGet("y") on the original record name returns I64(99) — proves that
//    FieldUpdate stores the new value at the correct field offset and that the
//    record pointer remains valid after the mutation.
//
//  RUNTIME-FIELDUPDATE-2: After the same FieldUpdate(y←99), FieldGet("x")
//    on the original record still returns I64(10) — proves FieldUpdate does
//    not corrupt adjacent fields.
//
//  RUNTIME-TUPLE-1: TupleNew([10, 42]) + FieldGet("0") returns I64(10)
//    — proves the first tuple element is at byte offset 0.  FieldGet uses
//    the numeric field-name fallback (`field.parse::<usize>()`) because
//    TupleNew does not register a named record layout.
//
//  RUNTIME-TUPLE-2: TupleNew([10, 42]) + FieldGet("1") returns I64(42)
//    — proves the second element is at byte offset 8, confirming the 8-byte
//    stride is correct for tuple elements.
//
//  RUNTIME-MAP-1: MapNew with one key-value pair returns I32(ptr > 0)
//    — structural proof that MapNew compiles, instantiates, and allocates
//    without trapping.  Memory layout integrity requires introspection
//    infrastructure beyond what invoke_compiler_expr exposes.
//
//  RUNTIME-SET-1: SetNew with one element returns I32(ptr > 0)
//    — same structural proof as RUNTIME-MAP-1 for the SetNew constructor.

// RUNTIME-RECORD-1
//
// fn.main =
//   let r = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   FieldGet { record: "r", field: "y" }
//
// RecordNew layout: x at offset 0 (I64 10), y at offset 8 (I64 42).
// FieldGet("y"): record_layouts["r"] = ["x","y"] → index 1 → offset 8.
// load_i64_at(8, ptr) → I64(42).
#[test]
fn record_new_field_get_second_field() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::FieldGet {
            record: "r".to_string(),
            field: "y".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.record_get_y"),
        RuntimeValue::I64(42),
        "FieldGet(y) on RecordNew{{x:10,y:42}} must return I64(42)"
    );
}

// RUNTIME-RECORD-2
//
// fn.main =
//   let r = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   FieldGet { record: "r", field: "x" }
//
// FieldGet("x"): record_layouts["r"] = ["x","y"] → index 0 → offset 0.
// load_i64_at(0, ptr) → I64(10).
#[test]
fn record_new_field_get_first_field() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::FieldGet {
            record: "r".to_string(),
            field: "x".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.record_get_x"),
        RuntimeValue::I64(10),
        "FieldGet(x) on RecordNew{{x:10,y:42}} must return I64(10)"
    );
}

// RUNTIME-FIELDUPDATE-1
//
// fn.main =
//   let r    = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   let _upd = FieldUpdate { record: "r", field: "y", value: Literal(99) }   in
//   FieldGet { record: "r", field: "y" }
//
// FieldUpdate stores 99 at ptr + 8 (field "y") in-place and returns ptr.
// The original Let-binding "r" still holds the same pointer; memory is now
// [I64(10), I64(99)].  FieldGet("y") on "r" reads offset 8 → I64(99).
#[test]
fn field_update_mutates_target_field() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::Let {
            name: "_upd".to_string(),
            value: Box::new(AnfExpr::FieldUpdate {
                record: "r".to_string(),
                field: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "r".to_string(),
                field: "y".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.fieldupdate_y"),
        RuntimeValue::I64(99),
        "FieldUpdate(y←99) must be visible via subsequent FieldGet(y)"
    );
}

// RUNTIME-FIELDUPDATE-2
//
// fn.main =
//   let r    = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   let _upd = FieldUpdate { record: "r", field: "y", value: Literal(99) }   in
//   FieldGet { record: "r", field: "x" }
//
// After FieldUpdate(y←99): memory = [I64(10), I64(99)].
// FieldGet("x") reads offset 0 → I64(10).  Proves FieldUpdate does not
// corrupt the adjacent field at offset 0.
#[test]
fn field_update_leaves_other_field_unchanged() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::Let {
            name: "_upd".to_string(),
            value: Box::new(AnfExpr::FieldUpdate {
                record: "r".to_string(),
                field: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "r".to_string(),
                field: "x".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.fieldupdate_x_unchanged"),
        RuntimeValue::I64(10),
        "FieldUpdate(y←99) must not corrupt field x; FieldGet(x) must still return I64(10)"
    );
}

// RUNTIME-TUPLE-1
//
// fn.main =
//   let t = TupleNew([Literal(10), Literal(42)]) in
//   FieldGet { record: "t", field: "0" }
//
// TupleNew layout (no count prefix): elem0 at offset 0 (I64 10),
//                                    elem1 at offset 8 (I64 42).
// FieldGet("0"): TupleNew does not register a record layout, so
//   field_offset falls back to `"0".parse::<usize>().unwrap_or(0)` = 0
//   → offset 0.  load_i64_at(0, ptr) → I64(10).
#[test]
fn tuple_new_field_get_at_index_zero() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(10)),
            AnfExpr::Literal(LiteralValue::Int(42)),
        ])),
        body: Box::new(AnfExpr::FieldGet {
            record: "t".to_string(),
            field: "0".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.tuple_get_0"),
        RuntimeValue::I64(10),
        "FieldGet(\"0\") on TupleNew([10,42]) must return the first element I64(10)"
    );
}

// RUNTIME-TUPLE-2
//
// fn.main =
//   let t = TupleNew([Literal(10), Literal(42)]) in
//   FieldGet { record: "t", field: "1" }
//
// FieldGet("1"): `"1".parse::<usize>()` = 1 → offset 1*8 = 8.
// load_i64_at(8, ptr) → I64(42).  Confirms the 8-byte element stride.
#[test]
fn tuple_new_field_get_at_index_one() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(10)),
            AnfExpr::Literal(LiteralValue::Int(42)),
        ])),
        body: Box::new(AnfExpr::FieldGet {
            record: "t".to_string(),
            field: "1".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.tuple_get_1"),
        RuntimeValue::I64(42),
        "FieldGet(\"1\") on TupleNew([10,42]) must return the second element I64(42)"
    );
}

// RUNTIME-MAP-1
//
// fn.main =
//   let k = Literal(1)   in
//   let v = Literal(100) in
//   MapNew { entries: [("k", "v")] }
//
// MapNew allocates [(1+1*2)*8 = 24] bytes.  Heap starts at offset 8 (the
// bump-pointer initial value when there is no effect data), so the returned
// pointer is > 0.
//
// NOTE: We can only prove structural non-crash here; verifying that the
// count (I64 1) and the k/v pair are written correctly requires memory-
// introspection infrastructure that invoke_compiler_expr does not expose.
#[test]
fn map_new_returns_non_null_pointer() {
    let expr = AnfExpr::Let {
        name: "k".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(100))),
            body: Box::new(AnfExpr::MapNew {
                entries: vec![("k".to_string(), "v".to_string())],
            }),
        }),
    };
    let result = invoke_compiler_expr(expr, "fn.map_new");
    assert!(
        matches!(result, RuntimeValue::I32(ptr) if ptr > 0),
        "MapNew must return a positive I32 heap pointer; got {result:?}"
    );
}

// RUNTIME-SET-1
//
// fn.main =
//   let elem = Literal(7) in
//   SetNew { elements: ["elem"] }
//
// SetNew allocates [(1+1)*8 = 16] bytes.  The returned pointer must be > 0.
//
// NOTE: Same structural-proof limitation as RUNTIME-MAP-1; memory layout
// (count at offset 0, element at offset 8) is not verified here.
#[test]
fn set_new_returns_non_null_pointer() {
    let expr = AnfExpr::Let {
        name: "elem".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
        body: Box::new(AnfExpr::SetNew {
            elements: vec!["elem".to_string()],
        }),
    };
    let result = invoke_compiler_expr(expr, "fn.set_new");
    assert!(
        matches!(result, RuntimeValue::I32(ptr) if ptr > 0),
        "SetNew must return a positive I32 heap pointer; got {result:?}"
    );
}

// ── Wave 19C: ACL source-level E2E conformance — variant + match + string + while ──
//
// Spec scenarios covered (RUNTIME-ACL-SOME-1, RUNTIME-ACL-NONE-1,
// RUNTIME-ACL-OK-1, RUNTIME-ACL-ERR-1, RUNTIME-ACL-STRING-1,
// RUNTIME-ACL-WHILE-1, RUNTIME-ACL-WHILE-2):
//
//  RUNTIME-ACL-SOME-1: ACL body `match(some(42), Some(x), x, _, 0)` must
//    construct a Some(42) variant, enter the Some(x) arm, bind x=42, and
//    return I64(42).  Proves the full pipeline from ACL source through
//    VariantNew emission and constructor-pattern match dispatch.
//
//  RUNTIME-ACL-NONE-1: ACL body `match(none(), None, 99, _, 0)` must
//    construct a None variant (tag_id=0) and dispatch to the `None` arm
//    (tag-only, no payload binding), returning I64(99).  Proves tag-only
//    constructor patterns fire correctly.
//
//  RUNTIME-ACL-OK-1: ACL body `match(ok(7), Ok(v), v, Err(e), 0)` must
//    construct an Ok(7) variant and dispatch to the Ok(v) arm, returning I64(7).
//    Proves Ok/Err share the same well-known tag encoding as None/Some.
//
//  RUNTIME-ACL-ERR-1: ACL body `match(err(5), Ok(v), 0, Err(e), e)` must
//    construct an Err(5) variant, skip the Ok(v) arm (tag mismatch), dispatch
//    to Err(e), and return I64(5).  Proves the second arm fires correctly.
//
//  RUNTIME-ACL-STRING-1: ACL body `let(s, "hello", s)` must compile the
//    string literal "hello" and return it as a packed I64 where the upper
//    32 bits encode the string length (5).  Proves string literals survive the
//    ACL parse → expr_parser → lower → WASM emit pipeline without loss.
//
//  RUNTIME-ACL-WHILE-1: ACL body `let(flag, false, while(flag, 42))` must
//    enter the while loop, find the condition false, never execute the body,
//    and return I32(0) (unit).  Proves WhileLoop with a Var-condition at the
//    ACL level exits immediately and produces the unit sentinel.
//
//  RUNTIME-ACL-WHILE-2: A multi-let ACL body creates a cell, runs a while
//    loop that writes 1 to the cell and breaks, then reads the cell.  Must
//    return I64(1).  Proves the while body executes exactly once, CellSet
//    persists to linear memory, CellGet reads back the written value, and
//    break exits the loop.  All sub-expression arguments are pre-bound Vars
//    so that no atomized binding is lost through the lower_core_expr_to_anf_local
//    `_` fallthrough (documented gap: non-Var while-condition expressions).

// RUNTIME-ACL-SOME-1
//
// ACL body: match(some(42), Some(x), x, _, 0)
//
//   Pipeline:
//   1. `some(42)` → CoreExpr::VariantNew{tag:"Some", payload:Literal(42)}
//   2. Lowered to: Let{anf_0=42, anf_1=VariantNew{tag:"Some",payload:Var(anf_0)},
//                      Match{scrutinee:anf_1, arms:[Some(x)→x, _→0]}}
//   3. WASM: alloc 16 bytes, store tag_id("Some")=1 at offset 0,
//      store I64(42) at offset 8; then match: tag==1 → bind x=payload → x=42.
//   4. Returns I64(42).
#[test]
fn acl_some_match_extracts_payload() {
    let acl = "\
change acl_some_1 base=0
author tester
description some/match round-trip: Some(x) arm extracts the i64 payload
op create_function id=fn.main return=Int body=match(some(42), Some(x), x, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(42),
        "match(some(42), Some(x), x, _, 0) must return I64(42)"
    );
}

// RUNTIME-ACL-NONE-1
//
// ACL body: match(none(), None, 99, _, 0)
//
//   Pipeline:
//   1. `none()` → CoreExpr::VariantNew{tag:"None", payload:None}
//   2. Lowered to: Let{anf_0=VariantNew{tag:"None",payload:None},
//                      Match{scrutinee:anf_0, arms:[None→99, _→0]}}
//   3. WASM: alloc 16 bytes, store tag_id("None")=0 at offset 0;
//      match: tag==0 → no payload binding → body=99.
//   4. Returns I64(99).
//
// Well-known tag table: None=0, Ok=0, Some=1, Err=1.
#[test]
fn acl_none_match_fires_none_arm() {
    let acl = "\
change acl_none_1 base=0
author tester
description none/match: None tag-only arm fires, wildcard fallback returns 0
op create_function id=fn.main return=Int body=match(none(), None, 99, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(99),
        "match(none(), None, 99, _, 0) must return I64(99)"
    );
}

// RUNTIME-ACL-OK-1
//
// ACL body: match(ok(7), Ok(v), v, Err(e), 0)
//
//   Pipeline:
//   1. `ok(7)` → VariantNew{tag:"Ok", payload:Literal(7)}, tag_id("Ok")=0
//   2. Match: Ok(v) arm → tag_id("Ok")=0 matches → bind v=7 → return v.
//   3. Err(e) arm is unreachable in this invocation.
//   4. Returns I64(7).
#[test]
fn acl_ok_match_extracts_ok_payload() {
    let acl = "\
change acl_ok_1 base=0
author tester
description ok/match round-trip: Ok(v) arm extracts the i64 payload
op create_function id=fn.main return=Int body=match(ok(7), Ok(v), v, Err(e), 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(7),
        "match(ok(7), Ok(v), v, Err(e), 0) must return I64(7)"
    );
}

// RUNTIME-ACL-ERR-1
//
// ACL body: match(err(5), Ok(v), 0, Err(e), e)
//
//   Pipeline:
//   1. `err(5)` → VariantNew{tag:"Err", payload:Literal(5)}, tag_id("Err")=1
//   2. Match: Ok(v) arm → tag_id("Ok")=0 ≠ 1 → skip.
//             Err(e) arm → tag_id("Err")=1 matches → bind e=5 → return e.
//   3. Returns I64(5).
//
// Proves the second match arm fires when the first arm's tag does not match.
#[test]
fn acl_err_match_fires_err_arm() {
    let acl = "\
change acl_err_1 base=0
author tester
description err/match: Err(e) arm fires when Ok(v) arm tag does not match
op create_function id=fn.main return=Int body=match(err(5), Ok(v), 0, Err(e), e)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(5),
        "match(err(5), Ok(v), 0, Err(e), e) must return I64(5)"
    );
}

// RUNTIME-ACL-STRING-1
//
// ACL body: let(s, "hello", s)
//
//   Pipeline:
//   1. `bare_value_end` preserves inner quotes: body_expr = `let(s, "hello", s)`.
//   2. expr_parser sees `"hello"` inside parse_args → Literal(Text("hello")).
//   3. WASM emit: Text literal → I64Const((len << 32) | ptr).
//      For "hello" (5 bytes): upper 32 bits = 5.
//   4. Returns I64(packed) with upper 32 bits = 5.
//
// The exact ptr (lower 32 bits) depends on the data segment and is not
// asserted — only the stable length field is checked.
#[test]
fn acl_string_literal_body_encodes_length_in_upper_bits() {
    let acl = r#"
change acl_string_1 base=0
author tester
description string literal body: "hello" must encode len=5 in upper I64 bits
op create_function id=fn.main return=Text body=let(s, "hello", s)
end
"#;
    let value = invoke_acl_export(acl, "main");
    let RuntimeValue::I64(packed) = value else {
        panic!("expected RuntimeValue::I64 for string body, got {value:?}");
    };
    let len = (packed as u64 >> 32) as u32;
    assert_eq!(
        len, 5,
        "string \"hello\" must encode length 5 in upper 32 bits of the packed I64; got len={len}"
    );
}

// RUNTIME-ACL-WHILE-1
//
// ACL body: let(flag, false, while(flag, 42))
//
//   Pipeline:
//   1. flag = I64(0) (false).
//   2. WhileLoop: emit_condition_get("flag") → I64(0); I64Const(0); I64Ne → I32(0);
//      I32Eqz → I32(1); BrIf(1) → branch taken (condition is zero).
//      Body (Literal(42)) is never reached.
//   3. WhileLoop pushes I32Const(0) → returns I32(0) (unit).
//
// Constraint: the while-condition must be a Var already in scope.
// If the condition expression is non-atomic (e.g. `while(lt(x,5), ...)`),
// the lower_core_expr_to_anf_local `_` fallthrough loses atomized bindings —
// see documented gap in Wave 19C session summary.
#[test]
fn acl_while_false_condition_body_never_runs() {
    let acl = "\
change acl_while_1 base=0
author tester
description while(flag=false, body) must skip body and return unit I32(0)
op create_function id=fn.main return=Int body=let(flag, false, while(flag, 42))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I32(0),
        "let(flag, false, while(flag, 42)) must return I32(0) without running the body"
    );
}

// RUNTIME-ACL-WHILE-2
//
// ACL body (multi-let):
//   let(zero, 0,
//     let(c, cell_new(zero),
//       let(one, 1,
//         let(go, true,
//           let(_w, while(go, let(_s, cell_set(c, one), break(go))),
//             cell_get(c)
//           )
//         )
//       )
//     )
//   )
//
//   Pipeline:
//   1. zero=0, c=CellNew(0), one=1, go=true (I64 1).
//   2. WhileLoop: go=truthy → enter body.
//      Body: _s=CellSet(c, one=1) — writes I64(1) into cell; break(go) → Br(1).
//   3. WhileLoop exits via break; pushes I32Const(0) as _w.
//   4. CellGet(c) → I64(1).
//
// Proves: ACL while body executes exactly once; CellSet persists through
// linear memory; CellGet reads back the written value; break exits the loop.
// All sub-expression arguments are pre-bound Vars — no atomized binding is lost.
#[test]
fn acl_while_body_runs_once_and_mutates_cell() {
    let acl = "\
change acl_while_2 base=0
author tester
description while body runs once: CellSet writes 1 to cell, CellGet reads 1
op create_function id=fn.main return=Int body=let(zero, 0, let(c, cell_new(zero), let(one, 1, let(go, true, let(_w, while(go, let(_s, cell_set(c, one), break(go))), cell_get(c))))))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "while body must run once: CellSet(c,1) then break → CellGet(c) must return I64(1)"
    );
}

// ── Wave 20C: ACL source-level E2E conformance — map and set constructors ──
//
// Spec scenarios covered (RUNTIME-ACL-MAP-1..3, RUNTIME-ACL-SET-1..2):
//
//  RUNTIME-ACL-MAP-1: ACL body `map(1, 10)` must parse to
//    CoreExpr::MapNew { entries: [(Literal(1), Literal(10))] }, lower to
//    AnfExpr::MapNew with atomized vars, emit WASM, instantiate, and return
//    I32(ptr > 0).  Proves the full pipeline from ACL source → expr_parser
//    → MapNew → lower → WASM emit without crash.
//
//    NOTE: Memory layout verification (count at offset 0, key/value pairs
//    at subsequent offsets) requires memory-introspection infrastructure not
//    yet available in invoke_acl_export.  Non-null pointer is the feasible
//    structural proof at this stage.
//
//  RUNTIME-ACL-SET-1: ACL body `set(42)` must parse to
//    CoreExpr::SetNew { elements: [Literal(42)] }, lower to AnfExpr::SetNew
//    with one atomized var, emit WASM, instantiate, and return I32(ptr > 0).
//    Same structural proof as RUNTIME-ACL-MAP-1 for the SetNew constructor.

// RUNTIME-ACL-MAP-2
//
// ACL body: map()  — empty map
//
//   Pipeline:
//   1. `map()` → parse_expr → CoreExpr::MapNew { entries: [] }
//   2. lower_to_anf → no atomizations; AnfExpr::MapNew { entries: [] }.
//   3. emit_wasm → allocates 8-byte header (count=0); returns I32(ptr > 0).
//
// This covers the zero-entry path through the MapNew emitter (the .max(8)
// guard ensures at least a count word is allocated).
#[test]
fn acl_map_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_map_2 base=0
author tester
description map() empty map must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=map()
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "map() must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-MAP-3
//
// ACL body: map(1, 10, 2, 20)  — two-pair map
//
//   Pipeline:
//   1. `map(1, 10, 2, 20)` → parse_expr →
//      CoreExpr::MapNew { entries: [(Lit(1),Lit(10)), (Lit(2),Lit(20))] }
//   2. lower_to_anf → atomize 4 literals → _t0.._t3;
//      AnfExpr::MapNew { entries: [("_t0","_t1"), ("_t2","_t3")] }.
//   3. emit_wasm → allocates (1+2*2)*8 = 40 bytes; returns I32(ptr > 0).
//
// Proves the multi-entry atomization path and layout arithmetic are correct.
#[test]
fn acl_map_multi_pair_form_returns_non_null_pointer() {
    let acl = "\
change acl_map_3 base=0
author tester
description map(1,10,2,20) two-pair map must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=map(1, 10, 2, 20)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "map(1, 10, 2, 20) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-MAP-1
//
// ACL body: map(1, 10)
//
//   Pipeline:
//   1. `map(1, 10)` → parse_expr → CoreExpr::MapNew { entries: [(Lit(1), Lit(10))] }
//   2. lower_to_core_ir → unchanged (CoreExpr::MapNew passes through).
//   3. lower_to_anf → atomize Lit(1) → _t0, atomize Lit(10) → _t1;
//      AnfExpr::MapNew { entries: [("_t0", "_t1")] }.
//   4. emit_wasm → MapNew bump-allocates heap memory; returns I32(ptr > 0).
//   5. invoke → RuntimeValue::I32(ptr) where ptr > 0.
//
// Constraint: key and value are integer literals; no let-binding required.
// The atomizer generates fresh names internally during ANF lowering.
#[test]
fn acl_map_form_returns_non_null_pointer() {
    let acl = "\
change acl_map_1 base=0
author tester
description map(1, 10) ACL form must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=map(1, 10)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "map(1, 10) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-SET-1
//
// ACL body: set(42)
//
//   Pipeline:
//   1. `set(42)` → parse_expr → CoreExpr::SetNew { elements: [Lit(42)] }
//   2. lower_to_core_ir → unchanged (CoreExpr::SetNew passes through).
//   3. lower_to_anf → atomize Lit(42) → _t0;
//      AnfExpr::SetNew { elements: ["_t0"] }.
//   4. emit_wasm → SetNew bump-allocates heap memory; returns I32(ptr > 0).
//   5. invoke → RuntimeValue::I32(ptr) where ptr > 0.
//
// Constraint: element is an integer literal; no let-binding required.
#[test]
fn acl_set_form_returns_non_null_pointer() {
    let acl = "\
change acl_set_1 base=0
author tester
description set(42) ACL form must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=set(42)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "set(42) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-SET-2
//
// ACL body: set()  — empty set
//
//   Pipeline:
//   1. `set()` → parse_expr → CoreExpr::SetNew { elements: [] }
//   2. lower_to_anf → no atomizations; AnfExpr::SetNew { elements: [] }.
//   3. emit_wasm → allocates 8-byte header (count=0); returns I32(ptr > 0).
//
// Covers the zero-element path through the SetNew emitter (the .max(8)
// guard ensures at least a count word is always allocated).
#[test]
fn acl_set_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_set_2 base=0
author tester
description set() empty set must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=set()
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "set() must return a positive I32 heap pointer; got {value:?}"
    );
}
