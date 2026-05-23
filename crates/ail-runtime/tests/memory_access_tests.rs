// ── ail-runtime::memory_access_tests ─────────────────────────────────────
//
// TASK D-1 (TDD RED): Tests for RuntimeInstance::read_wasm_memory and
// write_wasm_memory — written before the methods exist.
//
// Spec scenarios:
//  - read_wasm_memory_after_record_construction: compile a RecordNew with two
//    int fields, invoke, read memory at the returned pointer → correct i64 LE values.
//  - write_then_read_wasm_memory: write bytes to address 0, read back → same bytes.
//  - read_wasm_memory_negative_ptr_returns_none: negative ptr → None.
//  - read_wasm_memory_past_end_returns_none: ptr + len > mem size → None.

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, LiteralValue, SourceMap, StageHashes, emit_wasm,
};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

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

fn instantiate(wasm: &[u8]) -> ail_runtime::RuntimeInstance {
    let manifest = CapabilityManifest {
        module: "memory-test".to_string(),
        requires: vec![],
    };
    let profile = RuntimeProfile::new(
        "memory-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM must instantiate")
}

// ── D-1 tests ─────────────────────────────────────────────────────────────

#[test]
fn read_wasm_memory_after_record_construction() {
    // RecordNew { a: Int(10), b: Int(20) } — the record fields are written to
    // WASM linear memory; the function returns a pointer (i32) to the record.
    let expr = AnfExpr::RecordNew {
        fields: vec![
            ("a".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
            ("b".to_string(), AnfExpr::Literal(LiteralValue::Int(20))),
        ],
    };
    let wasm = compiler_wasm_for_expr(expr, "make_rec");
    let mut instance = instantiate(&wasm);

    // invoke returns I32(ptr)
    let result = instance
        .invoke("make_rec", &[])
        .expect("invoke must succeed");
    let ptr = match result {
        RuntimeValue::I32(p) => p,
        other => panic!("expected RuntimeValue::I32 pointer, got {other:?}"),
    };

    // read 16 bytes (2 fields × 8 bytes each) at the record base pointer
    let bytes = instance
        .read_wasm_memory(ptr, 16)
        .expect("read_wasm_memory must succeed for valid ptr");
    assert_eq!(bytes.len(), 16, "must return exactly 16 bytes");

    let a_val = i64::from_le_bytes(bytes[0..8].try_into().unwrap());
    let b_val = i64::from_le_bytes(bytes[8..16].try_into().unwrap());
    assert_eq!(a_val, 10, "field a must equal 10");
    assert_eq!(b_val, 20, "field b must equal 20");
}

#[test]
fn write_then_read_wasm_memory() {
    // Use any WASM with exported memory.  RecordNew ensures needs_memory = true.
    let expr = AnfExpr::RecordNew {
        fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(0)))],
    };
    let wasm = compiler_wasm_for_expr(expr, "rec");
    let mut instance = instantiate(&wasm);

    // Write 8 bytes to address 0 (before the heap start, safe to overwrite).
    let payload: [u8; 8] = [0xCA, 0xFE, 0xBA, 0xBE, 0x01, 0x02, 0x03, 0x04];
    let wrote = instance.write_wasm_memory(0, &payload);
    assert!(wrote, "write_wasm_memory must return true for valid ptr");

    // Read them back.
    let read_back = instance
        .read_wasm_memory(0, 8)
        .expect("read_wasm_memory must succeed");
    assert_eq!(
        read_back.as_slice(),
        &payload,
        "read-back bytes must match written bytes"
    );
}

#[test]
fn read_wasm_memory_negative_ptr_returns_none() {
    let expr = AnfExpr::RecordNew {
        fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(0)))],
    };
    let wasm = compiler_wasm_for_expr(expr, "rec");
    let mut instance = instantiate(&wasm);

    let result = instance.read_wasm_memory(-1, 8);
    assert!(result.is_none(), "negative ptr must return None");
}

#[test]
fn read_wasm_memory_past_end_returns_none() {
    let expr = AnfExpr::RecordNew {
        fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(0)))],
    };
    let wasm = compiler_wasm_for_expr(expr, "rec");
    let mut instance = instantiate(&wasm);

    // WASM memory minimum = 1 page = 65536 bytes.
    // Reading 100 bytes starting at offset 65530 exceeds the page boundary.
    let result = instance.read_wasm_memory(65530, 100);
    assert!(result.is_none(), "read past end of memory must return None");
}
