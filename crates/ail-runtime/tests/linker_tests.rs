// ── ail-runtime::linker_tests ────────────────────────────────────────────
//
// Integration tests for the Wasmtime Linker wiring (PR2, task 2.7).
//
// Verifies that:
//  L1 — A WASM module declaring `(import "ail" "host_call" ...)` instantiates
//       via the RuntimeHost linker without error.
//  L2 — Existing WASM modules with NO host imports still instantiate
//       (regression guard for Store<HostState> migration).
//  L3 — The linker stub allows instantiation even when no handlers are registered.

use std::sync::Arc;

use wasm_encoder::{EntityType, FunctionSection, ImportSection, Module, TypeSection, ValType};

use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, InMemoryHandler, ResourceLimits,
    RuntimeHost, RuntimeProfile, blake3_hex_of,
};

// ── WASM builders ────────────────────────────────────────────────────────

/// Build a WASM module that declares the `ail/host_call` import.
///
/// Signature: (i32, i32, i32, i32, i32, i32) -> i64
/// This matches the stub registered by `RuntimeHost::new`.
fn wasm_with_host_call_import() -> Vec<u8> {
    let mut module = Module::new();

    // Type section: define the function type (i32 × 6) -> i64
    let mut types = TypeSection::new();
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
    module.section(&types);

    // Import section: import `ail`/`host_call` with the type above (index 0)
    let mut imports = ImportSection::new();
    imports.import("ail", "host_call", EntityType::Function(0));
    module.section(&imports);

    // Function section: declare one function (body will be in code section)
    // — not strictly needed for this test; the module is valid with just the import.
    let func_section = FunctionSection::new();
    module.section(&func_section);

    module.finish()
}

/// Minimal valid WASM with no imports: magic + version only.
fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

/// Build a matching RuntimeProfile for given wasm and manifest.
fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "linker-test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        grants,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
}

// ── L1 — WASM with ail/host_call import instantiates ─────────────────────

#[test]
fn wasm_with_host_call_import_instantiates() {
    let wasm = wasm_with_host_call_import();
    let manifest = CapabilityManifest {
        module: "linker-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest, vec![]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "WASM with ail/host_call import must instantiate via linker: {result:?}"
    );
    assert!(host.audit_log().events()[0].is_passed());
}

// ── L2 — Existing WASM with no imports still instantiates (regression) ───

#[test]
fn minimal_wasm_still_instantiates_after_store_migration() {
    let wasm = minimal_wasm();
    let manifest = CapabilityManifest {
        module: "regression-test".to_string(),
        requires: vec![],
    };
    let profile = matching_profile(&wasm, &manifest, vec![]);

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "minimal WASM must still work after Store<HostState> migration: {result:?}"
    );
}

// ── L3 — Linker stub works without registered handlers ───────────────────

#[test]
fn linker_stub_works_without_handlers() {
    let wasm = wasm_with_host_call_import();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "no-handler-test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "no-handler-test".to_string(),
        capability: cap,
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    // No handlers — but handler binding check is NOT enabled (default false).
    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "linker stub must allow instantiation even without handlers: {result:?}"
    );
}

// ── L4 — WASM with import + handler registration + call_capability ────────

#[test]
fn wasm_with_import_and_handler_dispatches_capability() {
    let wasm = wasm_with_host_call_import();
    let cap = CapabilityId::new("FileRead");
    let manifest = CapabilityManifest {
        module: "full-test".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "full-test".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]);

    let handler = Arc::new(InMemoryHandler::new(
        "file-handler",
        vec![cap.clone()],
        b"result-bytes".to_vec(),
    ));

    let mut host = RuntimeHost::new().with_handler(handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // call_capability dispatches to the handler via the host side API.
    let result = host.call_capability(&cap, "read", &[]);
    assert_eq!(result, Ok(b"result-bytes".to_vec()));

    let events = host.audit_log().events();
    assert_eq!(events.len(), 2);
    assert!(events[0].is_passed());
    assert!(events[1].is_capability_call());
}
