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
    let wasm = emit_wasm(&anf)
        .expect("closure-fold ANF must compile")
        .wasm;
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
fn make_variant_match_expr(
    tag: &str,
    payload: Option<i64>,
    arms: Vec<AnfMatchArm>,
) -> AnfExpr {
    AnfExpr::Let {
        name: "v".to_string(),
        value: Box::new(AnfExpr::VariantNew {
            tag: tag.to_string(),
            payload: payload
                .map(|p| Box::new(AnfExpr::Literal(LiteralValue::Int(p)))),
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
