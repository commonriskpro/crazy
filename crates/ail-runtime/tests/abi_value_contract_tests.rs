// ── ail-runtime::abi_value_contract_tests ─────────────────────────────────
//
// These tests lock the current compiler/runtime ABI value contract without
// expanding the language surface. Runtime production code stays independent of
// ail-compiler; this test crate uses ail-compiler as a dev-dependency to prove
// the descriptor shapes map cleanly to runtime layouts.

use ail_compiler::wasm::{WasmScalarType, WasmTypeDescriptor, derive_wasm_type};
use ail_compiler::{AnfExpr, LiteralValue};
use ail_runtime::{HandleId, StructuredValue, ValueDecoder, ValueLayout};

fn descriptor_to_layout(descriptor: &WasmTypeDescriptor) -> ValueLayout {
    match descriptor {
        WasmTypeDescriptor::Scalar(_) => ValueLayout::Scalar,
        WasmTypeDescriptor::Text => ValueLayout::Text,
        WasmTypeDescriptor::Bytes => ValueLayout::Bytes,
        WasmTypeDescriptor::Record { fields } => ValueLayout::Record {
            fields: fields.clone(),
        },
        WasmTypeDescriptor::Variant { tags } => ValueLayout::Variant { tags: tags.clone() },
        WasmTypeDescriptor::Tuple(elems) => {
            ValueLayout::Tuple(elems.iter().map(descriptor_to_layout).collect())
        }
        WasmTypeDescriptor::List(inner) => ValueLayout::List(Box::new(descriptor_to_layout(inner))),
        WasmTypeDescriptor::Option(inner) => {
            ValueLayout::Option(Box::new(descriptor_to_layout(inner)))
        }
        WasmTypeDescriptor::Result { ok, err } => ValueLayout::Result {
            ok: Box::new(descriptor_to_layout(ok)),
            err: Box::new(descriptor_to_layout(err)),
        },
        WasmTypeDescriptor::Handle => ValueLayout::Handle,
    }
}

fn write_i32(memory: &mut [u8], offset: usize, value: i32) {
    memory[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_i64(memory: &mut [u8], offset: usize, value: i64) {
    memory[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn compiler_tuple_descriptor_maps_to_runtime_tuple_layout() {
    let descriptor = derive_wasm_type(&AnfExpr::TupleNew(vec![
        AnfExpr::Literal(LiteralValue::Int(3)),
        AnfExpr::Literal(LiteralValue::Int(5)),
    ]));

    let layout = descriptor_to_layout(&descriptor);
    assert_eq!(
        layout,
        ValueLayout::Tuple(vec![ValueLayout::Scalar, ValueLayout::Scalar])
    );

    let mut memory = vec![0; 16];
    write_i64(&mut memory, 0, 3);
    write_i64(&mut memory, 8, 5);
    assert_eq!(
        ValueDecoder::decode(&layout, 0, &memory),
        StructuredValue::List(vec![StructuredValue::Scalar(3), StructuredValue::Scalar(5),])
    );
}

#[test]
fn compiler_record_descriptor_maps_to_runtime_record_layout() {
    let descriptor = derive_wasm_type(&AnfExpr::RecordNew {
        fields: vec![
            ("id".to_string(), AnfExpr::Literal(LiteralValue::Int(7))),
            ("age".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
        ],
    });

    let layout = descriptor_to_layout(&descriptor);
    assert_eq!(
        layout,
        ValueLayout::Record {
            fields: vec!["id".to_string(), "age".to_string()],
        }
    );

    let mut memory = vec![0; 16];
    write_i64(&mut memory, 0, 7);
    write_i64(&mut memory, 8, 42);
    assert_eq!(
        ValueDecoder::decode(&layout, 0, &memory),
        StructuredValue::Record(vec![
            ("id".to_string(), StructuredValue::Scalar(7)),
            ("age".to_string(), StructuredValue::Scalar(42)),
        ])
    );
}

#[test]
fn compiler_variant_descriptor_maps_to_runtime_variant_layout() {
    let descriptor = derive_wasm_type(&AnfExpr::VariantNew {
        tag: "Ready".to_string(),
        payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(9)))),
    });

    let layout = descriptor_to_layout(&descriptor);
    assert_eq!(
        layout,
        ValueLayout::Variant {
            tags: vec!["Ready".to_string()],
        }
    );

    let mut memory = vec![0; 16];
    write_i32(&mut memory, 0, 0);
    write_i64(&mut memory, 8, 9);
    assert_eq!(
        ValueDecoder::decode(&layout, 0, &memory),
        StructuredValue::Variant {
            tag: "Ready".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(9))),
        }
    );
}

#[test]
fn compiler_option_result_and_handle_descriptors_map_to_runtime_layouts() {
    let scalar = WasmTypeDescriptor::Scalar(WasmScalarType::I64);

    assert_eq!(
        descriptor_to_layout(&WasmTypeDescriptor::Option(Box::new(scalar.clone()))),
        ValueLayout::Option(Box::new(ValueLayout::Scalar))
    );
    assert_eq!(
        descriptor_to_layout(&WasmTypeDescriptor::Result {
            ok: Box::new(scalar.clone()),
            err: Box::new(scalar),
        }),
        ValueLayout::Result {
            ok: Box::new(ValueLayout::Scalar),
            err: Box::new(ValueLayout::Scalar),
        }
    );
    assert_eq!(
        descriptor_to_layout(&WasmTypeDescriptor::Handle),
        ValueLayout::Handle
    );
}

#[test]
fn runtime_decodes_option_result_and_handle_contract_shapes() {
    let option_layout = descriptor_to_layout(&WasmTypeDescriptor::Option(Box::new(
        WasmTypeDescriptor::Scalar(WasmScalarType::I64),
    )));
    let mut some_memory = vec![0; 16];
    write_i32(&mut some_memory, 0, 1);
    write_i64(&mut some_memory, 8, 123);
    assert_eq!(
        ValueDecoder::decode(&option_layout, 0, &some_memory),
        StructuredValue::Variant {
            tag: "Some".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(123))),
        }
    );

    let result_layout = descriptor_to_layout(&WasmTypeDescriptor::Result {
        ok: Box::new(WasmTypeDescriptor::Scalar(WasmScalarType::I64)),
        err: Box::new(WasmTypeDescriptor::Scalar(WasmScalarType::I64)),
    });
    let mut err_memory = vec![0; 16];
    write_i32(&mut err_memory, 0, 1);
    write_i64(&mut err_memory, 8, -5);
    assert_eq!(
        ValueDecoder::decode(&result_layout, 0, &err_memory),
        StructuredValue::Variant {
            tag: "Err".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(-5))),
        }
    );

    let handle_layout = descriptor_to_layout(&WasmTypeDescriptor::Handle);
    assert_eq!(
        ValueDecoder::decode(&handle_layout, 77, &[]),
        StructuredValue::Handle(HandleId(77))
    );
}

#[test]
fn text_unit_and_bytes_descriptors_map_to_runtime_layouts() {
    // Text: compiler emits WasmTypeDescriptor::Text (not Scalar).
    // The runtime decodes packed (len << 32 | ptr) into StructuredValue::Text.
    let text_descriptor = derive_wasm_type(&AnfExpr::Literal(LiteralValue::Text("hi".into())));
    assert_eq!(text_descriptor, WasmTypeDescriptor::Text);
    assert_eq!(descriptor_to_layout(&text_descriptor), ValueLayout::Text);
    // ptr=0x40, len=2 → packed i64 = (2 << 32) | 0x40
    let packed = (2i64 << 32) | 0x40i64;
    assert_eq!(
        ValueDecoder::decode(&ValueLayout::Text, packed, &[]),
        StructuredValue::Text { ptr: 0x40, len: 2 }
    );

    let unit_descriptor = derive_wasm_type(&AnfExpr::Literal(LiteralValue::Unit));
    assert_eq!(
        unit_descriptor,
        WasmTypeDescriptor::Scalar(WasmScalarType::I32)
    );
    assert_eq!(descriptor_to_layout(&unit_descriptor), ValueLayout::Scalar);
    assert_eq!(
        ValueDecoder::decode(&ValueLayout::Scalar, 0, &[]),
        StructuredValue::Scalar(0)
    );

    // Bytes: WasmTypeDescriptor::Bytes maps to ValueLayout::Bytes.
    // Decoded from the same packed (len << 32 | ptr) encoding as Text,
    // but produces StructuredValue::Bytes — no UTF-8 assumption.
    assert_eq!(
        descriptor_to_layout(&WasmTypeDescriptor::Bytes),
        ValueLayout::Bytes
    );
    // ptr=0x80, len=16 → packed i64 = (16 << 32) | 0x80
    let bytes_packed = (16i64 << 32) | 0x80i64;
    assert_eq!(
        ValueDecoder::decode(&ValueLayout::Bytes, bytes_packed, &[]),
        StructuredValue::Bytes { ptr: 0x80, len: 16 }
    );
}
