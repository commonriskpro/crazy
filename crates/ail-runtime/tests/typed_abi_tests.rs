// ── ail-runtime::typed_abi_tests ─────────────────────────────────────────
//
// TASK D-3: Tests for RuntimeInstance::invoke_typed.
// TASK F-3: Tests for dispatch_host_call_write.
//
// Spec scenarios:
//  D-3:
//  - invoke_typed_scalar_returns_structured_scalar
//  - invoke_typed_record_decodes_fields
//  - invoke_typed_variant_decodes_tag
//  - invoke_typed_list_decodes_elements
//
//  F-3:
//  - host_call_write_writes_handler_result_to_wasm_memory
//  - host_call_write_returns_bytes_written
//  - host_call_write_denied_capability_returns_minus_one

use std::sync::Arc;

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, LiteralValue, SourceMap, StageHashes, emit_wasm,
};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, InMemoryHandler, ResourceLimits,
    RuntimeHost, RuntimeProfile, StructuredValue, ValueDecoder, ValueLayout, blake3_hex_of,
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
        module: "typed-abi-test".to_string(),
        requires: vec![],
    };
    let profile = RuntimeProfile::new(
        "typed-abi-test".to_string(),
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

/// Instantiate a WASM module with a handler and grant for `cap_name`.
fn instantiate_with_handler(
    wasm: &[u8],
    cap_name: &str,
    handler: Arc<InMemoryHandler>,
) -> (RuntimeHost, ail_runtime::RuntimeInstance) {
    let manifest = CapabilityManifest {
        module: "typed-abi-test".to_string(),
        requires: vec![CapabilityId::new(cap_name)],
    };
    let profile = RuntimeProfile::new(
        "typed-abi-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![CapabilityGrant {
            module: "typed-abi-test".to_string(),
            capability: CapabilityId::new(cap_name),
        }],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );
    let mut host = RuntimeHost::new().with_handler(handler);
    let instance = host
        .validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM must instantiate");
    (host, instance)
}

// ── D-3 tests ─────────────────────────────────────────────────────────────

#[test]
fn invoke_typed_scalar_returns_structured_scalar() {
    let expr = AnfExpr::Literal(LiteralValue::Int(42));
    let wasm = compiler_wasm_for_expr(expr, "answer");
    let mut instance = instantiate(&wasm);

    let result = instance
        .invoke_typed("answer", &[], &ValueLayout::Scalar)
        .expect("invoke_typed must succeed");

    assert_eq!(result, StructuredValue::Scalar(42));
}

#[test]
fn invoke_typed_record_decodes_fields() {
    let expr = AnfExpr::RecordNew {
        fields: vec![
            ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
            ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(32))),
        ],
    };
    let wasm = compiler_wasm_for_expr(expr, "make_xy");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::Record {
        fields: vec!["x".to_string(), "y".to_string()],
    };
    let result = instance
        .invoke_typed("make_xy", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::Record(vec![
            ("x".to_string(), StructuredValue::Scalar(10)),
            ("y".to_string(), StructuredValue::Scalar(32)),
        ])
    );
}

#[test]
fn invoke_typed_variant_decodes_tag() {
    // VariantNew { tag: "Ok", payload: Some(Int(5)) }
    // The variant tag "Ok" will be assigned discriminant 0 (first encounter).
    let expr = AnfExpr::VariantNew {
        tag: "Ok".to_string(),
        payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(5)))),
    };
    let wasm = compiler_wasm_for_expr(expr, "make_ok");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::Variant {
        tags: vec!["Ok".to_string(), "Err".to_string()],
    };
    let result = instance
        .invoke_typed("make_ok", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::Variant {
            tag: "Ok".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(5))),
        }
    );
}

#[test]
fn invoke_typed_list_decodes_elements() {
    let expr = AnfExpr::ListNew(vec![
        AnfExpr::Literal(LiteralValue::Int(1)),
        AnfExpr::Literal(LiteralValue::Int(2)),
        AnfExpr::Literal(LiteralValue::Int(3)),
    ]);
    let wasm = compiler_wasm_for_expr(expr, "make_list");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::List(Box::new(ValueLayout::Scalar));
    let result = instance
        .invoke_typed("make_list", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::List(vec![
            StructuredValue::Scalar(1),
            StructuredValue::Scalar(2),
            StructuredValue::Scalar(3),
        ])
    );
}

// ── G-1: Record return across WASM boundary ───────────────────────────────

#[test]
fn invoke_typed_record_end_to_end() {
    // Full end-to-end: compile, instantiate, invoke_typed with Record layout.
    let expr = AnfExpr::RecordNew {
        fields: vec![
            (
                "name_len".to_string(),
                AnfExpr::Literal(LiteralValue::Int(3)),
            ),
            ("age".to_string(), AnfExpr::Literal(LiteralValue::Int(30))),
        ],
    };
    let wasm = compiler_wasm_for_expr(expr, "make_person");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::Record {
        fields: vec!["name_len".to_string(), "age".to_string()],
    };
    let result = instance
        .invoke_typed("make_person", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::Record(vec![
            ("name_len".to_string(), StructuredValue::Scalar(3)),
            ("age".to_string(), StructuredValue::Scalar(30)),
        ])
    );
}

// ── G-2: Variant/Result/Option across WASM boundary ──────────────────────

#[test]
fn invoke_typed_option_some_end_to_end() {
    // For Option(Scalar), None must get discriminant 0 and Some must get 1.
    // Achieve this by having the binding encounter "None" first, then "Some".
    // Let _none = VariantNew { "None" }  → disc("None") = 0
    // VariantNew { "Some", Int(7) }       → disc("Some") = 1
    let expr = AnfExpr::Let {
        name: "_none".to_string(),
        value: Box::new(AnfExpr::VariantNew {
            tag: "None".to_string(),
            payload: None,
        }),
        body: Box::new(AnfExpr::VariantNew {
            tag: "Some".to_string(),
            payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(7)))),
        }),
    };
    let wasm = compiler_wasm_for_expr(expr, "make_some");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::Option(Box::new(ValueLayout::Scalar));
    let result = instance
        .invoke_typed("make_some", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::Variant {
            tag: "Some".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(7))),
        }
    );
}

#[test]
fn invoke_typed_result_ok_end_to_end() {
    let expr = AnfExpr::VariantNew {
        tag: "Ok".to_string(),
        payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(0)))),
    };
    let wasm = compiler_wasm_for_expr(expr, "make_ok_val");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::Result {
        ok: Box::new(ValueLayout::Scalar),
        err: Box::new(ValueLayout::Scalar),
    };
    let result = instance
        .invoke_typed("make_ok_val", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::Variant {
            tag: "Ok".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(0))),
        }
    );
}

#[test]
fn invoke_typed_option_none_end_to_end() {
    let expr = AnfExpr::VariantNew {
        tag: "None".to_string(),
        payload: None,
    };
    let wasm = compiler_wasm_for_expr(expr, "make_none");
    let mut instance = instantiate(&wasm);

    let layout = ValueLayout::Option(Box::new(ValueLayout::Scalar));
    let result = instance
        .invoke_typed("make_none", &[], &layout)
        .expect("invoke_typed must succeed");

    assert_eq!(
        result,
        StructuredValue::Variant {
            tag: "None".to_string(),
            payload: None,
        }
    );
}

// ── G-3: EffectCall with structured result via host_call_write ─────────────

#[test]
fn invoke_typed_effect_call_structured_result_end_to_end() {
    // Handler returns 16 bytes: [10i64 LE | 20i64 LE] (a 2-field record).
    let handler_response: Vec<u8> = [10i64.to_le_bytes(), 20i64.to_le_bytes()].concat();
    let handler = Arc::new(InMemoryHandler::new(
        "data-handler",
        vec![CapabilityId::new("data")],
        handler_response.clone(),
    ));

    let (wasm, result_buffer_offset) = effect_call_in_record_anf("data", "fetch");
    let (_, mut instance) = instantiate_with_handler(&wasm, "data", handler);

    // Invoke — returns i32 ptr to RecordNew { val: bytes_written }
    let result = instance.invoke("fetch", &[]).expect("invoke must succeed");
    let _ = result; // We don't need the function's return value here.

    // Read the structured data from result_buffer_offset.
    let raw_data = instance
        .read_wasm_memory(result_buffer_offset, 16)
        .expect("read result buffer must succeed");

    // Decode as a 2-field record starting at offset 0 of the slice.
    let layout = ValueLayout::Record {
        fields: vec!["f1".to_string(), "f2".to_string()],
    };
    let decoded = ValueDecoder::decode(&layout, 0, &raw_data);
    assert_eq!(
        decoded,
        StructuredValue::Record(vec![
            ("f1".to_string(), StructuredValue::Scalar(10)),
            ("f2".to_string(), StructuredValue::Scalar(20)),
        ])
    );
}

// ── F-3 tests: dispatch_host_call_write ───────────────────────────────────
//
// These tests verify that dispatch_host_call_write writes handler response
// bytes into WASM memory at result_buffer_offset.
// They are RED until F-4 implements dispatch_host_call_write.

/// Build an ANF where an EffectCall result flows into a RecordNew.
///
/// Returns (wasm_bytes, result_buffer_offset).  The binding is named "fetch"
/// and returns an i32 pointer to a 1-field record whose "val" field holds
/// `bytes_written` (i64) from `host_call_write`.
fn effect_call_in_record_anf(cap: &str, func: &str) -> (Vec<u8>, i32) {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fetch".to_string(),
        expr: AnfExpr::Let {
            name: "effect_result".to_string(),
            value: Box::new(AnfExpr::EffectCall {
                capability: cap.to_string(),
                func: func.to_string(),
                args: vec![],
            }),
            body: Box::new(AnfExpr::RecordNew {
                fields: vec![("val".to_string(), AnfExpr::Var("effect_result".to_string()))],
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    let result_buffer_offset = artifact
        .result_buffer_offset
        .expect("structured EffectCall must have result_buffer_offset");
    (artifact.wasm, result_buffer_offset)
}

#[test]
fn host_call_write_writes_handler_result_to_wasm_memory() {
    // Handler returns 16 bytes encoding two i64 fields: 10 and 20.
    let handler_response: Vec<u8> = [10i64.to_le_bytes(), 20i64.to_le_bytes()].concat();
    let handler = Arc::new(InMemoryHandler::new(
        "data-handler",
        vec![CapabilityId::new("data")],
        handler_response.clone(),
    ));

    let (wasm, result_buffer_offset) = effect_call_in_record_anf("data", "fetch");
    let (_, mut instance) = instantiate_with_handler(&wasm, "data", handler);

    // invoke — returns bytes_written as i64
    let _result = instance.invoke("fetch", &[]).expect("invoke must succeed");

    // The handler's bytes should be in memory at result_buffer_offset.
    let bytes = instance
        .read_wasm_memory(result_buffer_offset, 16)
        .expect("read_wasm_memory must succeed");
    assert_eq!(
        bytes, handler_response,
        "memory at result_buffer_offset must contain the handler's response bytes"
    );
}

#[test]
fn host_call_write_returns_bytes_written() {
    // Handler returns exactly 16 bytes.
    let handler_response: Vec<u8> = [10i64.to_le_bytes(), 20i64.to_le_bytes()].concat();
    let handler = Arc::new(InMemoryHandler::new(
        "data-handler",
        vec![CapabilityId::new("data")],
        handler_response,
    ));

    let (wasm, result_buffer_offset) = effect_call_in_record_anf("data", "fetch");
    let (_, mut instance) = instantiate_with_handler(&wasm, "data", handler);

    // Invoke → returns i32 ptr to RecordNew { val: bytes_written }
    let result = instance.invoke("fetch", &[]).expect("invoke must succeed");
    let rec_ptr = match result {
        ail_runtime::RuntimeValue::I32(p) => p,
        other => panic!("expected I32 RecordNew ptr, got {other:?}"),
    };

    // The RecordNew "val" field (8 bytes at rec_ptr) holds bytes_written.
    let field_bytes = instance
        .read_wasm_memory(rec_ptr, 8)
        .expect("read record field must succeed");
    let bytes_written = i64::from_le_bytes(field_bytes.try_into().unwrap());
    assert_eq!(
        bytes_written, 16,
        "bytes_written (stored in RecordNew.val) must equal handler response length (got result_buffer_offset={result_buffer_offset})"
    );
}

#[test]
fn host_call_write_denied_capability_returns_minus_one() {
    // No handler registered → capability denied → -1 returned.
    let (wasm, _) = effect_call_in_record_anf("data", "fetch");
    // Instantiate WITHOUT a handler.
    let manifest = CapabilityManifest {
        module: "typed-abi-test".to_string(),
        requires: vec![], // no capability declared
    };
    let profile = RuntimeProfile::new(
        "typed-abi-test".to_string(),
        blake3_hex_of(&wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![], // no grants
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("WASM must instantiate");

    // Invoke → returns i32 ptr to RecordNew { val: bytes_written }
    let result = instance.invoke("fetch", &[]).expect("invoke must succeed");
    let rec_ptr = match result {
        ail_runtime::RuntimeValue::I32(p) => p,
        other => panic!("expected I32 RecordNew ptr, got {other:?}"),
    };

    // When no handler / not granted, host_call_write returns -1.
    // The "val" field of the RecordNew holds -1 (as i64 LE).
    let field_bytes = instance
        .read_wasm_memory(rec_ptr, 8)
        .expect("read record field must succeed");
    let bytes_written = i64::from_le_bytes(field_bytes.try_into().unwrap());
    assert_eq!(
        bytes_written, -1,
        "bytes_written must be -1 when capability is denied"
    );
}
