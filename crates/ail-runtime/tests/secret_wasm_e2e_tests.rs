// ── ail-runtime::secret_wasm_e2e_tests ───────────────────────────────────
//
// End-to-end WASM host-import tests for the `secret.read` capability path.
//
// These tests exercise the full dispatch pipeline:
//   WASM module → ail/host_call or ail/host_call_write import
//     → dispatch_host_call / dispatch_host_call_write
//       → SecretReadHandler
//         → SecretVault
//
// Gaps closed (Wave 11D):
//   The existing `secret_vault_tests.rs` covers `RuntimeHost::call_capability`
//   (host-side path) only.  This file adds coverage of the WASM dispatch path,
//   where a compiled module invokes `secret.read:<id>` through the registered
//   linker import.  Both `host_call` (scalar i64 result) and `host_call_write`
//   (bytes written into WASM memory) paths are exercised.
//
// Test scenarios:
//   WE-1 — `host_call_write` granted with handler → bytes land in WASM memory,
//           byte count returned, audit records output_hash (not raw secret).
//   WE-2 — `host_call_write` denied without grant → returns -1, failed audit.
//   WE-3 — `host_call` granted with handler → returns non-(-1) result, audit
//           records success.
//   WE-4 — `host_call_write` grant present but no handler bound → returns -1,
//           failed audit.

use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, ResourceLimits, RuntimeHost,
    RuntimeProfile, RuntimeValue, SecretEntry, SecretProvider, SecretProviderError,
    SecretReadHandler, SecretVault, blake3_hex_of,
};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction, MemorySection, MemoryType, Module, TypeSection,
    ValType,
};

// ── WASM builders ────────────────────────────────────────────────────────

/// Build a WASM module that calls `ail/host_call_write` for `cap_name`,
/// writes the response into memory at `out_ptr` (up to `out_max` bytes),
/// and returns the bytes-written count as i32.
///
/// Memory layout:
///   [0..cap_len)    = cap_name bytes
///   [64..68)        = "read"  (the SecretReadHandler operation)
///   [128..)         = args (0 words — empty)
///   [out_ptr..)     = output buffer (up to out_max bytes)
///
/// Note: the operation is "read" (not the generic "op") because
/// `SecretReadHandler::handle` explicitly rejects any operation other than
/// "read" with `CapabilityDenied`.
fn host_call_write_wasm(cap_name: &str, out_ptr: i32, out_max: i32) -> Vec<u8> {
    const CAP_PTR: i32 = 0;
    const OP_PTR: i32 = 64;
    const ARGS_PTR: i32 = 128;

    let mut module = Module::new();

    // Type 0: host_call_write (8 x i32) -> i32
    let mut types = TypeSection::new();
    types.ty().function(
        vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        vec![ValType::I32],
    );
    // Type 1: main() -> i32
    types.ty().function(vec![], vec![ValType::I32]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("ail", "host_call_write", EntityType::Function(0));
    module.section(&imports);

    let mut functions = FunctionSection::new();
    functions.function(1);
    module.section(&functions);

    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("main", ExportKind::Func, 1);
    module.section(&exports);

    let mut codes = CodeSection::new();
    let mut function = Function::new(vec![]);
    function.instruction(&Instruction::I32Const(CAP_PTR));
    function.instruction(&Instruction::I32Const(cap_name.len() as i32));
    function.instruction(&Instruction::I32Const(OP_PTR));
    function.instruction(&Instruction::I32Const(4)); // "read".len()
    function.instruction(&Instruction::I32Const(ARGS_PTR));
    function.instruction(&Instruction::I32Const(0)); // 0 arg words
    function.instruction(&Instruction::I32Const(out_ptr));
    function.instruction(&Instruction::I32Const(out_max));
    function.instruction(&Instruction::Call(0));
    function.instruction(&Instruction::End);
    codes.function(&function);
    module.section(&codes);

    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(CAP_PTR), cap_name.bytes());
    data.active(0, &ConstExpr::i32_const(OP_PTR), "read".bytes());
    module.section(&data);

    module.finish()
}

/// Build a WASM module that calls `ail/host_call` for `cap_name`
/// and returns the i64 result directly.
///
/// Memory layout:
///   [0..cap_len)  = cap_name bytes
///   [64..68)      = "read"  (SecretReadHandler operation)
///   [128..)       = args (0 words — empty)
///
/// Note: the operation is "read" — same reason as `host_call_write_wasm`.
fn host_call_wasm(cap_name: &str) -> Vec<u8> {
    const CAP_PTR: i32 = 0;
    const OP_PTR: i32 = 64;
    const ARGS_PTR: i32 = 128;

    let mut module = Module::new();

    // Type 0: host_call (6 x i32) -> i64
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
    // Type 1: main() -> i64
    types.ty().function(vec![], vec![ValType::I64]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("ail", "host_call", EntityType::Function(0));
    module.section(&imports);

    let mut functions = FunctionSection::new();
    functions.function(1);
    module.section(&functions);

    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("main", ExportKind::Func, 1);
    module.section(&exports);

    let mut codes = CodeSection::new();
    let mut function = Function::new(vec![]);
    function.instruction(&Instruction::I32Const(CAP_PTR));
    function.instruction(&Instruction::I32Const(cap_name.len() as i32));
    function.instruction(&Instruction::I32Const(OP_PTR));
    function.instruction(&Instruction::I32Const(4)); // "read".len()
    function.instruction(&Instruction::I32Const(ARGS_PTR));
    function.instruction(&Instruction::I32Const(0)); // 0 arg words
    function.instruction(&Instruction::Call(0));
    function.instruction(&Instruction::End);
    codes.function(&function);
    module.section(&codes);

    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(CAP_PTR), cap_name.bytes());
    data.active(0, &ConstExpr::i32_const(OP_PTR), "read".bytes());
    module.section(&data);

    module.finish()
}

// ── Profile helpers ───────────────────────────────────────────────────────

fn profile_granting(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    cap: CapabilityId,
) -> RuntimeProfile {
    RuntimeProfile::new(
        "secret-wasm-e2e-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![CapabilityGrant {
            module: manifest.module.clone(),
            capability: cap,
        }],
        ResourceLimits::default(),
    )
}

fn profile_no_grants(wasm: &[u8], manifest: &CapabilityManifest) -> RuntimeProfile {
    RuntimeProfile::new(
        "secret-wasm-e2e-denied-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![],
        ResourceLimits::default(),
    )
}

// ── Handler factory ───────────────────────────────────────────────────────

fn make_secret_handler(
    secret_id: &str,
    vault_path: &str,
    secret_bytes: &[u8],
) -> Arc<SecretReadHandler> {
    let mut vault = SecretVault::new();
    vault.insert(vault_path, secret_bytes.to_vec());
    let mapping = vec![SecretEntry {
        secret_id: secret_id.to_string(),
        vault_path: vault_path.to_string(),
    }];
    Arc::new(SecretReadHandler::new(mapping, Arc::new(vault)))
}

// ── Audit helpers ─────────────────────────────────────────────────────────

fn last_capability_event(host: &RuntimeHost) -> AuditEvent {
    host.audit_log()
        .events()
        .iter()
        .rfind(|e| e.is_capability_call())
        .cloned()
        .expect("at least one CapabilityCallExecuted event")
}

// ── WE-1: host_call_write — bytes land in WASM memory ────────────────────
//
// Scenario: a WASM module invokes `ail/host_call_write` for
// `secret.read:ApiKey`.  The SecretReadHandler resolves the secret from the
// vault and writes bytes into WASM linear memory at the out_ptr offset.
//
// Assertions:
//   - invoke returns the secret byte count (not -1).
//   - `read_wasm_memory(out_ptr, len)` returns the exact secret bytes.
//   - Audit event records `succeeded=true`.
//   - Audit output_hash == blake3_hex_of(secret_bytes) — not the raw value.

#[test]
fn we1_host_call_write_secret_bytes_land_in_wasm_memory() {
    const SECRET_ID: &str = "ApiKey";
    const VAULT_PATH: &str = "secrets/api";
    const SECRET_BYTES: &[u8] = b"hunter2_secret"; // 14 bytes
    const CAP: &str = "secret.read:ApiKey";
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    let wasm = host_call_write_wasm(CAP, OUT_PTR, OUT_MAX);
    let manifest = CapabilityManifest {
        module: "secret-wasm-e2e".to_string(),
        requires: vec![CapabilityId::new(CAP)],
    };
    let cap = CapabilityId::new(CAP);
    let profile = profile_granting(&wasm, &manifest, cap);
    let handler = make_secret_handler(SECRET_ID, VAULT_PATH, SECRET_BYTES);

    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Invoke: dispatch_host_call_write → SecretReadHandler → vault
    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // The return value is the number of bytes written (14).
    assert_eq!(
        result,
        RuntimeValue::I32(SECRET_BYTES.len() as i32),
        "host_call_write must return the secret byte count"
    );

    // Read the secret bytes back from WASM linear memory.
    let written = instance
        .read_wasm_memory(OUT_PTR, SECRET_BYTES.len())
        .expect("read_wasm_memory must succeed");
    assert_eq!(
        written, SECRET_BYTES,
        "WASM memory at out_ptr must contain the exact secret bytes"
    );

    // Audit: the event must record success and the BLAKE3 hash — NOT the raw bytes.
    let event = last_capability_event(&host);
    match &event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            ..
        } => {
            assert!(succeeded, "audit must record success");

            let hash = output_hash
                .as_deref()
                .expect("output_hash must be present on success");

            // Hash must be the BLAKE3 digest of the secret bytes.
            let expected_hash = blake3_hex_of(SECRET_BYTES);
            assert_eq!(
                hash, expected_hash,
                "output_hash must equal blake3_hex_of(secret_bytes)"
            );

            // The raw secret value must NOT appear in the hash string.
            assert!(
                !hash.contains("hunter2"),
                "output_hash must not contain the raw secret value"
            );

            // Must be a 64-char hex string (BLAKE3 256-bit output).
            assert_eq!(
                hash.len(),
                64,
                "output_hash must be a 64-char BLAKE3 hex digest"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── WE-2: host_call_write — denied without grant ──────────────────────────
//
// Scenario: the profile grants NO capabilities.  The WASM call for
// `secret.read:ApiKey` must be denied by the grant check before the handler
// is reached.
//
// Assertions:
//   - invoke returns -1.
//   - Audit event records `succeeded=false`.

#[test]
fn we2_host_call_write_denied_without_grant_returns_minus_one() {
    const CAP: &str = "secret.read:ApiKey";
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    let wasm = host_call_write_wasm(CAP, OUT_PTR, OUT_MAX);
    // Manifest requires the cap but the profile grants nothing →
    // but preflight checks requires ⊆ grants, so we must declare no requires.
    let manifest = CapabilityManifest {
        module: "secret-wasm-e2e-denied".to_string(),
        requires: vec![], // no requires → no grants needed for preflight
    };
    let profile = profile_no_grants(&wasm, &manifest);
    let handler = make_secret_handler("ApiKey", "secrets/api", b"hunter2_secret");

    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass (no requires)");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // dispatch_host_call_write: grant check fails → returns -1.
    assert_eq!(
        result,
        RuntimeValue::I32(-1),
        "denied capability must return -1"
    );

    // Audit: the event must record failure.
    let event = last_capability_event(&host);
    match &event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            ..
        } => {
            assert!(!succeeded, "audit must record failure");
            assert!(
                output_hash.is_none(),
                "output_hash must be absent on denial"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── WE-3: host_call — granted, handler dispatched ────────────────────────
//
// Scenario: a WASM module invokes `ail/host_call` for `secret.read:DbPass`.
// `dispatch_host_call` interprets the first 8 bytes of the secret as a
// little-endian i64 and returns it.  The test verifies the call succeeds
// (result != -1) and the audit event records success.
//
// Note: the exact i64 value depends on the secret bytes; we assert !=-1
// to avoid brittle byte-packing arithmetic in the test.

#[test]
fn we3_host_call_secret_read_succeeds_and_audit_records_success() {
    const SECRET_ID: &str = "DbPass";
    const VAULT_PATH: &str = "secrets/db";
    // 8+ bytes so dispatch_host_call takes the first 8 as i64 (not zero-branch).
    const SECRET_BYTES: &[u8] = b"pg_pass_xyz12345";
    const CAP: &str = "secret.read:DbPass";

    let wasm = host_call_wasm(CAP);
    let manifest = CapabilityManifest {
        module: "secret-wasm-e2e-hc".to_string(),
        requires: vec![CapabilityId::new(CAP)],
    };
    let cap = CapabilityId::new(CAP);
    let profile = profile_granting(&wasm, &manifest, cap);
    let handler = make_secret_handler(SECRET_ID, VAULT_PATH, SECRET_BYTES);

    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // dispatch_host_call returns first 8 bytes as LE i64; -1 signals error.
    assert_ne!(
        result,
        RuntimeValue::I64(-1),
        "granted secret read must not return the error sentinel"
    );

    // Audit: the event must record success with a non-None output_hash.
    let event = last_capability_event(&host);
    match &event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            ..
        } => {
            assert!(succeeded, "audit must record success");
            assert!(
                output_hash.is_some(),
                "output_hash must be present on success"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── WE-5: host_call_write — handler bound, provider returns NotFound ──────
//
// Scenario: the profile grants `secret.read:ProviderKey` and the handler IS
// bound, but the backing provider always returns `SecretProviderError::NotFound`.
// The dispatch path must:
//   - return -1 (denial sentinel) to the WASM guest
//   - record `succeeded=false` in the audit log
//   - record `denial_category == Some("secret.not_found")` in the audit log
//   - NOT include the raw secret ID or vault path in the denial_category string
//
// This exercises the WASM dispatch branch that extracts `audit_category` from
// `HostError::CapabilityDeniedCategorized` (host_dispatch.rs dispatch_host_call_write).

#[test]
fn we5_host_call_write_provider_not_found_denial_category_in_audit() {
    const SECRET_ID: &str = "ProviderKey";
    const VAULT_PATH: &str = "vault/provider-key";
    const CAP: &str = "secret.read:ProviderKey";
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    // Provider that always signals NotFound — simulates a key absent in the vault.
    struct NotFoundProvider;
    impl SecretProvider for NotFoundProvider {
        fn resolve(&self, _vault_path: &str) -> Result<Vec<u8>, SecretProviderError> {
            Err(SecretProviderError::NotFound)
        }
    }

    let wasm = host_call_write_wasm(CAP, OUT_PTR, OUT_MAX);
    let manifest = CapabilityManifest {
        module: "secret-wasm-dc-not-found".to_string(),
        requires: vec![CapabilityId::new(CAP)],
    };
    let cap = CapabilityId::new(CAP);
    let profile = profile_granting(&wasm, &manifest, cap);

    // Handler is bound — grant check passes, but provider returns NotFound.
    let mapping = vec![SecretEntry {
        secret_id: SECRET_ID.to_string(),
        vault_path: VAULT_PATH.to_string(),
    }];
    let handler = SecretReadHandler::new(mapping, std::sync::Arc::new(NotFoundProvider));

    let mut host = RuntimeHost::new().with_handler(std::sync::Arc::new(handler));
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // Handler error → dispatch returns -1 (denial sentinel).
    assert_eq!(
        result,
        RuntimeValue::I32(-1),
        "provider NotFound must return -1 to WASM guest"
    );

    // Audit: failure recorded with the correct category.
    let event = last_capability_event(&host);
    match &event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            denial_category,
            ..
        } => {
            assert!(!succeeded, "audit must record failure");
            assert!(
                output_hash.is_none(),
                "output_hash must be absent on denial"
            );
            assert_eq!(
                denial_category.as_deref(),
                Some("secret.not_found"),
                "audit denial_category must be secret.not_found for NotFound provider"
            );
            // Category must contain no secret data.
            let cat = denial_category.as_deref().unwrap_or("");
            assert!(
                !cat.contains(SECRET_ID),
                "denial_category must not contain the secret ID"
            );
            assert!(
                !cat.contains(VAULT_PATH),
                "denial_category must not contain the vault path"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── WE-6: host_call — handler bound, provider returns Unavailable ─────────
//
// Scenario: the profile grants `secret.read:SvcToken` and the handler IS bound,
// but the backing provider always returns `SecretProviderError::Unavailable`
// (simulates a transient vault outage or network timeout).
// The dispatch path must:
//   - return -1 (i64 sentinel) to the WASM guest
//   - record `succeeded=false` in the audit log
//   - record `denial_category == Some("secret.provider_unavailable")`
//   - NOT include the raw secret ID or vault path in the category string
//
// This exercises the WASM dispatch branch for `dispatch_host_call`
// (i64-returning variant), which has a separate audit-push path.

#[test]
fn we6_host_call_provider_unavailable_denial_category_in_audit() {
    const SECRET_ID: &str = "SvcToken";
    const VAULT_PATH: &str = "infra/svc-token";
    const CAP: &str = "secret.read:SvcToken";

    // Provider that always signals Unavailable — simulates a vault outage.
    struct DownProvider;
    impl SecretProvider for DownProvider {
        fn resolve(&self, _vault_path: &str) -> Result<Vec<u8>, SecretProviderError> {
            Err(SecretProviderError::Unavailable)
        }
    }

    let wasm = host_call_wasm(CAP);
    let manifest = CapabilityManifest {
        module: "secret-wasm-dc-unavail".to_string(),
        requires: vec![CapabilityId::new(CAP)],
    };
    let cap = CapabilityId::new(CAP);
    let profile = profile_granting(&wasm, &manifest, cap);

    // Handler is bound — grant check passes, but provider returns Unavailable.
    let mapping = vec![SecretEntry {
        secret_id: SECRET_ID.to_string(),
        vault_path: VAULT_PATH.to_string(),
    }];
    let handler = SecretReadHandler::new(mapping, std::sync::Arc::new(DownProvider));

    let mut host = RuntimeHost::new().with_handler(std::sync::Arc::new(handler));
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    // Handler error → dispatch returns -1 (i64 sentinel).
    assert_eq!(
        result,
        RuntimeValue::I64(-1),
        "provider Unavailable must return -1 (i64) to WASM guest"
    );

    // Audit: failure recorded with the correct category.
    let event = last_capability_event(&host);
    match &event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            denial_category,
            ..
        } => {
            assert!(!succeeded, "audit must record failure");
            assert!(
                output_hash.is_none(),
                "output_hash must be absent on denial"
            );
            assert_eq!(
                denial_category.as_deref(),
                Some("secret.provider_unavailable"),
                "audit denial_category must be secret.provider_unavailable for Unavailable provider"
            );
            // Category must contain no secret data.
            let cat = denial_category.as_deref().unwrap_or("");
            assert!(
                !cat.contains(SECRET_ID),
                "denial_category must not contain the secret ID"
            );
            assert!(
                !cat.contains(VAULT_PATH),
                "denial_category must not contain the vault path"
            );
            // Category string must not contain the secret ID, vault path, or
            // any provider-internal detail (error message, URL, stack trace).
            // The documented category "secret.provider_unavailable" is intentional.
            assert!(
                !cat.contains(VAULT_PATH),
                "denial_category must not contain the vault path"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── WE-4: host_call_write — grant present, no handler bound ──────────────
//
// Scenario: the profile grants `secret.read:ApiKey` to the module, and the
// cap appears in the manifest, but no `SecretReadHandler` is registered on
// the host.  The dispatch must return -1 and emit a failed audit event.
//
// This completes the denial-case coverage alongside WE-2 (no grant) by
// also covering the "granted-but-unbound" path through the WASM-side
// dispatch.

#[test]
fn we4_host_call_write_no_handler_bound_returns_minus_one() {
    const CAP: &str = "secret.read:ApiKey";
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    let wasm = host_call_write_wasm(CAP, OUT_PTR, OUT_MAX);
    let manifest = CapabilityManifest {
        module: "secret-wasm-e2e-unbound".to_string(),
        requires: vec![CapabilityId::new(CAP)],
    };
    let cap = CapabilityId::new(CAP);
    let profile = profile_granting(&wasm, &manifest, cap);

    // No handler registered → granted-but-unbound.
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");

    assert_eq!(
        result,
        RuntimeValue::I32(-1),
        "granted-but-unbound secret capability must return -1"
    );

    let event = last_capability_event(&host);
    match &event {
        AuditEvent::CapabilityCallExecuted {
            succeeded,
            output_hash,
            ..
        } => {
            assert!(!succeeded, "audit must record failure for unbound cap");
            assert!(
                output_hash.is_none(),
                "output_hash must be absent on unbound denial"
            );
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}
