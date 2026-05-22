// ── ail-runtime::async_capability_tests ──────────────────────────────────
//
// Strict TDD — RED phase written BEFORE invoke_async and CapabilityCallMode exist.
//
// Spec scenarios:
//  ASYNC-1: invoke_async returns the correct RuntimeValue for a WASM export.
//  ASYNC-2: invoke_async with a unit-return export returns RuntimeValue::Unit.
//  ASYNC-3: CapabilityCallMode variants are distinct and debug-printable.
//  ASYNC-4: invoke_async propagates RuntimeError on missing export.

use std::sync::Arc;

use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction, Module,
    TypeSection, ValType,
};

use ail_runtime::{
    CapabilityCallMode, CapabilityManifest, ResourceLimits, RuntimeArg, RuntimeError, RuntimeHost,
    RuntimeProfile, RuntimeValue, blake3_hex_of,
};

// ── WASM builders ─────────────────────────────────────────────────────────

/// Build a WASM module that exports a no-arg function returning a constant i32.
fn wasm_const_i32(export_name: &str, value: i32) -> Vec<u8> {
    let mut module = Module::new();

    // Type: () -> i32
    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![ValType::I32]);
    module.section(&types);

    // Function declaration
    let mut funcs = FunctionSection::new();
    funcs.function(0);
    module.section(&funcs);

    // Export
    let mut exports = ExportSection::new();
    exports.export(export_name, ExportKind::Func, 0);
    module.section(&exports);

    // Code: i32.const VALUE; end
    let mut code = CodeSection::new();
    let mut func = Function::new(vec![]);
    func.instruction(&Instruction::I32Const(value));
    func.instruction(&Instruction::End);
    code.function(&func);
    module.section(&code);

    module.finish()
}

/// Build a WASM module that exports a no-arg function returning nothing (Unit).
fn wasm_unit_return(export_name: &str) -> Vec<u8> {
    let mut module = Module::new();

    // Type: () -> ()
    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![]);
    module.section(&types);

    let mut funcs = FunctionSection::new();
    funcs.function(0);
    module.section(&funcs);

    let mut exports = ExportSection::new();
    exports.export(export_name, ExportKind::Func, 0);
    module.section(&exports);

    let mut code = CodeSection::new();
    let mut func = Function::new(vec![]);
    func.instruction(&Instruction::End);
    code.function(&func);
    module.section(&code);

    module.finish()
}

/// Build a matching RuntimeProfile for the given wasm and manifest (no capabilities).
fn matching_profile(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "async-test-profile".to_string(),
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

/// Instantiate a WASM module and return a RuntimeInstance ready to invoke.
fn instantiate(wasm: &[u8]) -> ail_runtime::RuntimeInstance {
    let manifest = CapabilityManifest {
        module: "async-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(wasm, &manifest);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM module must instantiate for async tests")
}

// ── ASYNC-1: invoke_async returns the correct RuntimeValue ────────────────

#[tokio::test(flavor = "multi_thread")]
async fn invoke_async_returns_correct_value_from_wasm_export() {
    let wasm = wasm_const_i32("get_answer", 42);
    let mut instance = instantiate(&wasm);

    // RED: RuntimeInstance::invoke_async does not exist yet.
    let result = instance.invoke_async("get_answer", &[]).await;
    assert_eq!(
        result,
        Ok(RuntimeValue::I32(42)),
        "invoke_async must return the WASM-exported value"
    );
}

// ── ASYNC-2: invoke_async with unit-return export returns Unit ─────────────

#[tokio::test(flavor = "multi_thread")]
async fn invoke_async_unit_return_gives_runtime_value_unit() {
    let wasm = wasm_unit_return("do_nothing");
    let mut instance = instantiate(&wasm);

    let result = instance.invoke_async("do_nothing", &[]).await;
    assert_eq!(
        result,
        Ok(RuntimeValue::Unit),
        "invoke_async on a no-return export must yield RuntimeValue::Unit"
    );
}

// ── ASYNC-3: CapabilityCallMode variants are distinct ─────────────────────

#[test]
fn capability_call_mode_variants_are_distinct() {
    // RED: CapabilityCallMode does not exist yet.
    assert_ne!(
        CapabilityCallMode::Sync,
        CapabilityCallMode::Async,
        "Sync and Async variants must be distinct"
    );
}

#[test]
fn capability_call_mode_is_debug_printable() {
    let mode = CapabilityCallMode::Async;
    let repr = format!("{mode:?}");
    assert!(
        repr.contains("Async"),
        "Debug output must name the variant, got: {repr}"
    );
}

// ── ASYNC-4: invoke_async propagates RuntimeError on missing export ────────

#[tokio::test(flavor = "multi_thread")]
async fn invoke_async_propagates_error_on_missing_export() {
    let wasm = wasm_const_i32("real_export", 7);
    let mut instance = instantiate(&wasm);

    let result = instance.invoke_async("nonexistent_export", &[]).await;
    assert!(
        result.is_err(),
        "invoke_async must return Err when the export is not found"
    );
    match result {
        Err(RuntimeError::EncodingError(msg)) => {
            assert!(
                msg.contains("nonexistent_export"),
                "error message must name the missing export, got: {msg}"
            );
        }
        other => panic!("expected EncodingError, got {other:?}"),
    }
}
