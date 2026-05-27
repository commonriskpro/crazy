pub(super) use ail_change::{
    apply::{SnapshotBridge, apply},
    canonical::canonicalize_parsed,
    model::{ChangeSetOutcome, SnapshotId},
    parser::parse_changeset,
};
pub(super) use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, AnfMatchArm, LiteralValue, SourceMap,
    StageHashes, emit_wasm, lower_to_anf, lower_to_core_ir,
};
pub(super) use ail_core::semantic_graph::{NodeRef, SemanticGraph};
pub(super) use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, ClockHandler, PreflightFailure,
    ResourceLimits, RuntimeArg, RuntimeError, RuntimeHost, RuntimeProfile, RuntimeValue,
    blake3_hex_of,
};
pub(super) use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

/// Compile a minimal SemanticGraph through ail-compiler and return the WASM bytes.
pub(super) fn compiler_wasm() -> Vec<u8> {
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
pub(super) fn matching_profile(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
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

pub(super) fn sealed_anf(bindings: Vec<AnfBinding>) -> AnfIr {
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

pub(super) fn compiler_wasm_for_expr(expr: AnfExpr, name: &str) -> Vec<u8> {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: name.to_string(),
        expr,
    };
    let anf = sealed_anf(vec![binding]);
    emit_wasm(&anf).expect("emit_wasm failed").wasm
}

pub(super) fn instantiate_test_wasm(wasm: &[u8]) -> ail_runtime::RuntimeInstance {
    let manifest = CapabilityManifest {
        module: "invoke-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(wasm, &manifest);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM must instantiate")
}

pub(super) fn sum_wasm(param_count: u32) -> Vec<u8> {
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

pub(super) fn invoke_compiler_expr(expr: AnfExpr, name: &str) -> RuntimeValue {
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

pub(super) fn binary_i64_call(func: &str, left: i64, right: i64) -> AnfExpr {
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

pub(super) struct TestBridge;

impl SnapshotBridge for TestBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        SnapshotId(0)
    }
}

pub(super) fn invoke_acl_export(acl: &str, export: &str) -> RuntimeValue {
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
pub(super) fn invoke_closure_fold(bias: i64, init: i64, elements: Vec<i64>) -> RuntimeValue {
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

/// Build the ANF expression:
///   `let v = VariantNew(tag, payload?) in match v { arms... }`
///
/// `payload` is encoded as `Some(i64 literal)` when present.
pub(super) fn make_variant_match_expr(
    tag: &str,
    payload: Option<i64>,
    arms: Vec<AnfMatchArm>,
) -> AnfExpr {
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

/// Variant of `invoke_compiler_expr` that returns `Result` instead of
/// panicking — used for tests that expect a trap.
pub(super) fn try_invoke_compiler_expr(
    expr: AnfExpr,
    name: &str,
) -> Result<RuntimeValue, RuntimeError> {
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
pub(super) fn runtime_check_pattern_wasm(condition: i32) -> Vec<u8> {
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

/// Compile `expr` to WASM and return the live `RuntimeInstance` **without**
/// invoking any export.
///
/// Use this (instead of [`invoke_compiler_expr`]) when the test needs to keep
/// the instance alive after invocation so it can call
/// [`RuntimeInstance::read_memory_i64`] to inspect linear memory.
pub(super) fn compile_and_instantiate_expr(
    expr: AnfExpr,
    name: &str,
) -> ail_runtime::RuntimeInstance {
    let wasm = compiler_wasm_for_expr(expr, name);
    let manifest = CapabilityManifest {
        module: format!("{name}-test"),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate")
}
