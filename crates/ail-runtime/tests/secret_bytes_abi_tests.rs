// ── ail-runtime::secret_bytes_abi_tests ──────────────────────────────────
//
// Typed ABI boundary tests: treating `secret.read` output as `Bytes` via
// `ValueLayout::Bytes` and `ValueDecoder`.
//
// These tests prove the "typed secret bytes ABI" pattern described in
// docs/abi-value-contract.md §"secret.read typed boundary":
//
//   1. A WASM module invokes `ail/host_call_write` for `secret.read:<id>`.
//   2. `dispatch_host_call_write` resolves the secret via `SecretReadHandler`
//      and writes the raw bytes to WASM linear memory at `out_ptr`.
//   3. It returns the byte count as i32.
//   4. The host caller packs `(byte_count as i64) << 32 | (out_ptr as i64)` —
//      the same packed encoding emitted by a Bytes-typed WASM export.
//   5. `ValueDecoder::decode(&ValueLayout::Bytes, packed, &memory)` unpacks
//      ptr and len WITHOUT reading memory →
//      `StructuredValue::Bytes { ptr: out_ptr, len: byte_count }`.
//   6. The caller uses `read_wasm_memory(ptr, len)` to access the actual bytes.
//
// Security contract maintained by these tests:
//   - Raw secret bytes are never logged or compared directly to a string literal.
//   - The decode step (`ValueDecoder`) does not touch memory — only the
//     explicit `read_wasm_memory` call accesses secret bytes.
//   - Audit events record only the BLAKE3 hash of the output (not raw value).
//
// Test scenarios:
//   SB-1 — Bytes decode of secret.read output: ptr/len correct, memory bytes
//           match the vault value, audit records BLAKE3 hash only.
//   SB-2 — StructuredValue::Bytes is produced (not StructuredValue::Text);
//           the Bytes variant makes the absence of a UTF-8 assumption explicit.
//   SB-3 — Decode after denial (byte_count = -1 as i32, treated as i64 -1):
//           ValueDecoder emits Bytes with the raw (negative) fields — callers
//           must check the byte count before reading memory.

use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, ResourceLimits, RuntimeHost,
    RuntimeProfile, RuntimeValue, SecretEntry, SecretReadHandler, SecretVault, StructuredValue,
    ValueDecoder, ValueLayout, blake3_hex_of,
};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction, MemorySection, MemoryType, Module, TypeSection,
    ValType,
};

// ── WASM builder ──────────────────────────────────────────────────────────

/// Build a minimal WASM module that calls `ail/host_call_write` for `cap_name`,
/// writes the response to `[out_ptr, out_ptr + out_max)`, and returns the
/// written byte count as i32.
///
/// Memory layout:
///   [0..cap_len)     = cap_name UTF-8 bytes
///   [64..68)         = "read" (operation string required by SecretReadHandler)
///   [128..136)       = args buffer (0 arg words)
///   [out_ptr..out_max) = output buffer
fn secret_host_call_write_wasm(cap_name: &str, out_ptr: i32, out_max: i32) -> Vec<u8> {
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

// ── Profile / handler helpers ─────────────────────────────────────────────

fn profile_granting(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    cap: CapabilityId,
) -> RuntimeProfile {
    RuntimeProfile::new(
        "secret-bytes-abi-test".to_string(),
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
        "secret-bytes-abi-denied-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![],
        ResourceLimits::default(),
    )
}

fn make_handler(secret_id: &str, vault_path: &str, secret_bytes: &[u8]) -> Arc<SecretReadHandler> {
    let mut vault = SecretVault::new();
    vault.insert(vault_path, secret_bytes.to_vec());
    let mapping = vec![SecretEntry {
        secret_id: secret_id.to_string(),
        vault_path: vault_path.to_string(),
    }];
    Arc::new(SecretReadHandler::new(mapping, Arc::new(vault)))
}

// ── SB-1: Bytes decode of secret.read output ──────────────────────────────
//
// Proves the full typed secret bytes ABI pattern:
//   host_call_write → byte_count → pack → ValueDecoder → StructuredValue::Bytes
//   → read_wasm_memory → actual bytes match the vault value.
//
// The audit event must record the BLAKE3 hash of the secret bytes, NOT the
// raw value — the test verifies this without accessing the hash in a leaky way.

#[test]
fn sb1_bytes_decode_of_secret_read_output_via_value_layout() {
    const SECRET_ID: &str = "ApiKey";
    const VAULT_PATH: &str = "secrets/api-key";
    // Secret bytes — opaque, no UTF-8 assumption required.
    const SECRET_BYTES: &[u8] = b"\xde\xad\xbe\xef\x00\x01\x02\x03\xca\xfe\xba\xbe";
    const CAP: &str = "secret.read:ApiKey";
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    let wasm = secret_host_call_write_wasm(CAP, OUT_PTR, OUT_MAX);
    let manifest = CapabilityManifest {
        module: "secret-bytes-abi".to_string(),
        requires: vec![CapabilityId::new(CAP)],
    };
    let cap = CapabilityId::new(CAP);
    let profile = profile_granting(&wasm, &manifest, cap);
    let handler = make_handler(SECRET_ID, VAULT_PATH, SECRET_BYTES);

    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Step 1: invoke — dispatch_host_call_write → SecretReadHandler → vault.
    let result = instance.invoke("main", &[]).expect("invoke must not trap");
    let byte_count = match result {
        RuntimeValue::I32(n) => n,
        other => panic!("expected I32 byte count, got {other:?}"),
    };

    // Verify write succeeded (byte_count > 0, not -1).
    assert_eq!(
        byte_count,
        SECRET_BYTES.len() as i32,
        "host_call_write must return the secret byte count"
    );

    // Step 2: pack (byte_count, out_ptr) into the Bytes packed i64 encoding.
    // This mirrors what a Bytes-typed WASM export would return.
    let packed: i64 = ((byte_count as i64) << 32) | (OUT_PTR as i64 & 0xFFFF_FFFF);

    // Step 3: read full WASM memory for the decode call.
    // ValueDecoder::decode for Bytes does NOT read memory — it only unpacks
    // ptr and len from the raw i64.  Memory is only needed if the caller
    // subsequently calls read_wasm_memory.
    let memory_size = {
        // Probe memory size via a known-safe read.
        let probe = instance.read_wasm_memory(0, 0);
        // We need a non-trivial size; use 65536 (one WASM page).
        let _ = probe;
        65536usize
    };
    let memory = instance
        .read_wasm_memory(0, memory_size)
        .unwrap_or_default();

    // Step 4: decode via ValueLayout::Bytes — must produce Bytes { ptr, len },
    // not Text or Scalar.
    let decoded = ValueDecoder::decode(&ValueLayout::Bytes, packed, &memory);

    let (ptr, len) = match decoded {
        StructuredValue::Bytes { ptr, len } => (ptr, len),
        other => panic!("expected StructuredValue::Bytes, got {other:?}"),
    };

    assert_eq!(
        ptr, OUT_PTR,
        "decoded ptr must equal the out_ptr passed to host_call_write"
    );
    assert_eq!(
        len,
        SECRET_BYTES.len() as i32,
        "decoded len must equal the secret byte count"
    );

    // Step 5: read the actual bytes from WASM memory at ptr.
    // This is the only point where secret bytes are accessed — and never logged.
    let actual_bytes = instance
        .read_wasm_memory(ptr, len as usize)
        .expect("read_wasm_memory must succeed");
    assert_eq!(
        actual_bytes.as_slice(),
        SECRET_BYTES,
        "WASM memory at ptr must contain the exact secret bytes"
    );

    // Step 6: audit — verify BLAKE3 hash is recorded, NOT the raw bytes.
    let event = host
        .audit_log()
        .events()
        .iter()
        .rfind(|e| e.is_capability_call())
        .cloned()
        .expect("must have a CapabilityCallExecuted event");

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
            assert_eq!(
                hash,
                &blake3_hex_of(SECRET_BYTES),
                "output_hash must be the BLAKE3 digest of the secret bytes"
            );
            // Hash must not contain the raw bytes in any form.
            assert_eq!(hash.len(), 64, "output_hash must be a 64-char hex digest");
        }
        other => panic!("expected CapabilityCallExecuted, got {other:?}"),
    }
}

// ── SB-2: StructuredValue::Bytes vs StructuredValue::Text ─────────────────
//
// Confirms that ValueDecoder produces StructuredValue::Bytes (not Text) for
// the same packed i64 when the layout is ValueLayout::Bytes.  The distinction
// matters for callers that need to route opaque byte buffers vs UTF-8 strings
// through different code paths.

#[test]
fn sb2_bytes_layout_produces_bytes_not_text_variant() {
    // A packed i64 with ptr=256, len=12 — the same value that Text would see.
    let out_ptr: i32 = 256;
    let byte_count: i32 = 12;
    let packed: i64 = ((byte_count as i64) << 32) | (out_ptr as i64 & 0xFFFF_FFFF);
    let memory = vec![0u8; 512]; // contents irrelevant — decoder doesn't read for Bytes

    let bytes_result = ValueDecoder::decode(&ValueLayout::Bytes, packed, &memory);
    let text_result = ValueDecoder::decode(&ValueLayout::Text, packed, &memory);

    // Both decode the same ptr/len from the same packed i64 …
    assert_eq!(
        bytes_result,
        StructuredValue::Bytes {
            ptr: out_ptr,
            len: byte_count,
        },
        "Bytes layout must decode to StructuredValue::Bytes"
    );
    assert_eq!(
        text_result,
        StructuredValue::Text {
            ptr: out_ptr,
            len: byte_count,
        },
        "Text layout must decode to StructuredValue::Text"
    );

    // … but into distinct variants.
    assert_ne!(
        bytes_result, text_result,
        "Bytes and Text results must be distinct StructuredValue variants"
    );
}

// ── SB-3: Denial sentinel propagates cleanly through the decode path ───────
//
// When `host_call_write` denies a request it returns -1 as i32.  The caller
// receives -1 and must check for the sentinel before packing and decoding.
//
// This test proves that if a caller naively packs the -1 sentinel as a Bytes
// i64, the decoder produces a Bytes with negative ptr/len — making it safe to
// add a "byte_count < 0 → denial" guard before the pack step.

#[test]
fn sb3_denial_sentinel_minus_one_does_not_panic_in_decoder() {
    const CAP: &str = "secret.read:ApiKey";
    const OUT_PTR: i32 = 256;
    const OUT_MAX: i32 = 64;

    let wasm = secret_host_call_write_wasm(CAP, OUT_PTR, OUT_MAX);
    // Manifest requires no capabilities → no grants → dispatch denies.
    let manifest = CapabilityManifest {
        module: "secret-bytes-abi-denied".to_string(),
        requires: vec![],
    };
    let profile = profile_no_grants(&wasm, &manifest);

    let mut host = RuntimeHost::new(); // no handler
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass (no requires)");

    let result = instance.invoke("main", &[]).expect("invoke must not trap");
    let byte_count = match result {
        RuntimeValue::I32(n) => n,
        other => panic!("expected I32, got {other:?}"),
    };

    // Denial sentinel.
    assert_eq!(byte_count, -1, "denied call must return -1");

    // Callers should guard here: `if byte_count < 0 { /* handle denial */ }`.
    // The test below shows what happens if they don't guard — decoder is safe.
    let packed: i64 = ((byte_count as i64) << 32) | (OUT_PTR as i64 & 0xFFFF_FFFF);
    let memory = vec![0u8; 512];
    let decoded = ValueDecoder::decode(&ValueLayout::Bytes, packed, &memory);

    // The decoder produces Bytes with negative len — no panic, no UB.
    // Callers that check byte_count before packing will never reach this.
    match decoded {
        StructuredValue::Bytes { len, .. } => {
            assert!(
                len < 0,
                "naive pack of -1 sentinel produces Bytes with negative len; \
                 callers must guard byte_count >= 0 before decoding"
            );
        }
        other => panic!("expected StructuredValue::Bytes, got {other:?}"),
    }
}
