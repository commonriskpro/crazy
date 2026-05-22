// ── ail-runtime::typed_abi_tests ─────────────────────────────────────────
//
// TASK D-3 (TDD RED): Tests for RuntimeInstance::invoke_typed — written
// before the method exists.
//
// Spec scenarios:
//  - invoke_typed_scalar_returns_structured_scalar
//  - invoke_typed_record_decodes_fields
//  - invoke_typed_variant_decodes_tag
//  - invoke_typed_list_decodes_elements

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, LiteralValue, SourceMap, StageHashes,
    emit_wasm,
};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, StructuredValue, ValueLayout,
    blake3_hex_of,
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
        },
    );
    let mut host = RuntimeHost::new();
    host.validate_and_instantiate(wasm, &manifest, &profile)
        .expect("WASM must instantiate")
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
