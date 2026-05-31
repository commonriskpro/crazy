// ── ail-runtime::wasm_bridge_diagnostics_tests ───────────────────────────
//
// Stable redacted diagnostics for the production WASM ↔ host bridge.

use ail_runtime::{
    CapabilityManifest, ResourceLimits, RuntimeError, RuntimeHost, RuntimeProfile,
    WasmBridgeDiagnosticKind, blake3_hex_of,
};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, Module, TypeSection, ValType,
};

fn matching_profile(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    RuntimeProfile::new(
        "wasm-bridge-diagnostics-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash must succeed"),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    )
}

fn instantiate(wasm: &[u8]) -> ail_runtime::RuntimeInstance {
    let manifest = CapabilityManifest {
        module: "wasm-bridge-diagnostics".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(wasm, &manifest);
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(wasm, &manifest, &profile)
        .expect("test WASM must instantiate")
}

fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn wasm_with_unbound_and_mismatched_imports() -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I64]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("private_env", "secret_api_key", EntityType::Function(0));
    imports.import("ail", "host_call", EntityType::Function(1));
    module.section(&imports);

    module.finish()
}

fn one_arg_export_wasm() -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function([ValType::I64], [ValType::I64]);
    module.section(&types);

    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);

    let mut exports = ExportSection::new();
    exports.export("sum", ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = CodeSection::new();
    let mut function = Function::new([]);
    function.instruction(&Instruction::LocalGet(0));
    function.instruction(&Instruction::End);
    codes.function(&function);
    module.section(&codes);

    module.finish()
}

fn trapping_export_wasm() -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function([], []);
    module.section(&types);

    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);

    let mut exports = ExportSection::new();
    exports.export("main", ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = CodeSection::new();
    let mut function = Function::new([]);
    function.instruction(&Instruction::Unreachable);
    function.instruction(&Instruction::End);
    codes.function(&function);
    module.section(&codes);

    module.finish()
}

#[test]
fn module_diagnostics_classify_instantiation_failure_without_raw_error() {
    let host = RuntimeHost::new();
    let diagnostics = host.wasm_bridge_diagnostics(&[0xde, 0xad, 0xbe, 0xef]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        WasmBridgeDiagnosticKind::InstantiationFailure
    );
    assert_eq!(diagnostics[0].classification, "wasm.validation");
    assert!(diagnostics[0].detail.starts_with("error_shape=h"));
}

#[test]
fn module_diagnostics_redact_missing_imports_and_host_import_abi_mismatch() {
    let host = RuntimeHost::new();
    let diagnostics = host.wasm_bridge_diagnostics(&wasm_with_unbound_and_mismatched_imports());

    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == WasmBridgeDiagnosticKind::MissingImport
            && diagnostic.classification == "import.unbound"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == WasmBridgeDiagnosticKind::AbiMismatch
            && diagnostic.classification == "import.signature"
    }));

    let keys = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.diagnostic_key.as_str())
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(
        keys, sorted,
        "diagnostics must be deterministically ordered"
    );

    let rendered = format!("{diagnostics:?}");
    assert!(!rendered.contains("private_env"));
    assert!(!rendered.contains("secret_api_key"));
}

#[test]
fn call_diagnostics_classify_missing_export_without_changing_invoke_behavior() {
    let mut instance = instantiate(&minimal_wasm());

    let diagnostics = instance.wasm_bridge_diagnostics_for_call("very_sensitive_export", &[]);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, WasmBridgeDiagnosticKind::MissingExport);
    assert_eq!(diagnostics[0].classification, "export.missing");
    assert!(!format!("{diagnostics:?}").contains("very_sensitive_export"));

    let existing = instance.invoke("very_sensitive_export", &[]);
    assert!(matches!(existing, Err(RuntimeError::EncodingError(_))));
}

#[test]
fn diagnosed_invoke_classifies_export_abi_mismatch() {
    let mut instance = instantiate(&one_arg_export_wasm());

    let error = instance
        .invoke_with_bridge_diagnostics("sum", &[])
        .expect_err("arity mismatch must be diagnosed");

    assert!(matches!(error.source, RuntimeError::EncodingError(_)));
    assert_eq!(error.diagnostics.len(), 1);
    assert_eq!(
        error.diagnostics[0].kind,
        WasmBridgeDiagnosticKind::AbiMismatch
    );
    assert_eq!(error.diagnostics[0].classification, "export.arity");
}

#[test]
fn diagnosed_invoke_classifies_wasm_traps() {
    let mut instance = instantiate(&trapping_export_wasm());

    let existing = instance.invoke("main", &[]);
    assert!(matches!(existing, Err(RuntimeError::EncodingError(_))));

    let mut instance = instantiate(&trapping_export_wasm());
    let error = instance
        .invoke_with_bridge_diagnostics("main", &[])
        .expect_err("trap must be diagnosed");

    assert!(matches!(error.source, RuntimeError::EncodingError(_)));
    assert_eq!(error.diagnostics.len(), 1);
    assert_eq!(error.diagnostics[0].kind, WasmBridgeDiagnosticKind::Trap);
    assert_eq!(error.diagnostics[0].classification, "trap.unreachable");
}

#[test]
fn valid_bridge_module_has_no_diagnostics() {
    let host = RuntimeHost::new();
    assert!(host.wasm_bridge_diagnostics(&minimal_wasm()).is_empty());
}
