// ── ail-runtime::abi_typed_tests ─────────────────────────────────────────
//
// TDD RED phase — written before I32/F64 variants exist in RuntimeArg/RuntimeValue.
//
// Spec scenarios covered (R-1a, R-1b, R-1c):
//  - RuntimeArg::I32 and RuntimeArg::F64 are constructible.
//  - RuntimeArg::I32(0) and RuntimeArg::I64(0) are distinct.
//  - RuntimeInstance::invoke succeeds with an i32 arg and returns RuntimeValue::I32.
//  - RuntimeInstance::invoke succeeds with an f64 arg and returns RuntimeValue::F64.

use ail_runtime::{
    CapabilityManifest, ResourceLimits, RuntimeArg, RuntimeHost, RuntimeProfile, RuntimeValue,
    blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

fn matching_profile(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    RuntimeProfile::new(
        "abi-typed-test".to_string(),
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

/// Build a WASM module: (param i32) -> (result i32) — returns its argument.
fn i32_identity_wasm() -> Vec<u8> {
    let mut module = wasm_encoder::Module::new();

    let mut types = wasm_encoder::TypeSection::new();
    types
        .ty()
        .function([wasm_encoder::ValType::I32], [wasm_encoder::ValType::I32]);
    module.section(&types);

    let mut functions = wasm_encoder::FunctionSection::new();
    functions.function(0);
    module.section(&functions);

    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("identity", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = wasm_encoder::CodeSection::new();
    let mut function = wasm_encoder::Function::new([]);
    function.instruction(&wasm_encoder::Instruction::LocalGet(0));
    function.instruction(&wasm_encoder::Instruction::End);
    codes.function(&function);
    module.section(&codes);

    module.finish()
}

/// Build a WASM module: (param f64) -> (result f64) — returns its argument.
fn f64_identity_wasm() -> Vec<u8> {
    let mut module = wasm_encoder::Module::new();

    let mut types = wasm_encoder::TypeSection::new();
    types
        .ty()
        .function([wasm_encoder::ValType::F64], [wasm_encoder::ValType::F64]);
    module.section(&types);

    let mut functions = wasm_encoder::FunctionSection::new();
    functions.function(0);
    module.section(&functions);

    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("identity", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = wasm_encoder::CodeSection::new();
    let mut function = wasm_encoder::Function::new([]);
    function.instruction(&wasm_encoder::Instruction::LocalGet(0));
    function.instruction(&wasm_encoder::Instruction::End);
    codes.function(&function);
    module.section(&codes);

    module.finish()
}

fn instantiate(wasm: &[u8]) -> (RuntimeHost, ail_runtime::RuntimeInstance) {
    let manifest = CapabilityManifest {
        module: "abi-typed-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(wasm, &manifest);
    let mut host = RuntimeHost::new();
    let instance = host
        .validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM must instantiate");
    (host, instance)
}

// ── Scenario R-1c: I32(0) and I64(0) are distinct ────────────────────────

#[test]
fn runtime_arg_i32_and_i64_are_distinct() {
    let i32_zero = RuntimeArg::I32(0);
    let i64_zero = RuntimeArg::I64(0);
    assert_ne!(
        i32_zero, i64_zero,
        "I32(0) and I64(0) must be distinct variants"
    );
}

// ── Scenario R-1a: invoke with i32 arg succeeds ───────────────────────────

#[test]
fn runtime_arg_i32_variant_is_constructible() {
    let arg = RuntimeArg::I32(42_i32);
    match &arg {
        RuntimeArg::I32(v) => assert_eq!(*v, 42_i32),
        _ => panic!("expected RuntimeArg::I32"),
    }
}

// ── Scenario R-1b: invoke with f64 arg succeeds ───────────────────────────

#[test]
fn runtime_arg_f64_variant_is_constructible() {
    let arg = RuntimeArg::F64(3.125_f64);
    match &arg {
        RuntimeArg::F64(v) => assert!((*v - 3.125_f64).abs() < 1e-10),
        _ => panic!("expected RuntimeArg::F64"),
    }
}

#[test]
fn invoke_with_i32_arg_succeeds() {
    let wasm = i32_identity_wasm();
    let (_, mut instance) = instantiate(&wasm);

    let result = instance
        .invoke("identity", &[RuntimeArg::I32(5)])
        .expect("invoke with i32 arg must succeed");

    assert_eq!(
        result,
        RuntimeValue::I32(5),
        "i32 identity must return I32(5)"
    );
}

#[test]
fn invoke_with_f64_arg_succeeds() {
    let wasm = f64_identity_wasm();
    let (_, mut instance) = instantiate(&wasm);

    let result = instance
        .invoke("identity", &[RuntimeArg::F64(2.75_f64)])
        .expect("invoke with f64 arg must succeed");

    match result {
        RuntimeValue::F64(v) => assert!(
            (v - 2.75_f64).abs() < 1e-10,
            "f64 identity must return approximately 2.75, got {v}"
        ),
        other => panic!("expected RuntimeValue::F64, got {other:?}"),
    }
}
