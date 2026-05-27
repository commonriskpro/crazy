use super::helpers::*;

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
