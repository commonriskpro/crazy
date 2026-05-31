use std::collections::BTreeMap;

use super::{
    AbiDescriptor, WasmScalarType, WasmTypeDescriptor, WasmWireShape, derive_wasm_type, export_name,
};
use crate::{anf::AnfExpr, core_ir::LiteralValue};

//
// The arm_payload_binding function is imported from pattern_string and
// fully tested there. No duplicate tests are kept here.

#[test]
fn export_name_preserves_module_namespace_without_legacy_prefix() {
    assert_eq!(export_name("fn.main"), "main");
    assert_eq!(export_name("fn.app.main"), "app_main");
    assert_eq!(export_name("test.math.addition"), "math_addition");
}

#[test]
fn abi_wire_shape_classifies_scalar_packed_structured_and_handle_values() {
    assert_eq!(
        WasmTypeDescriptor::Scalar(WasmScalarType::I64).wire_shape(),
        WasmWireShape::ScalarSlot
    );
    assert_eq!(
        WasmTypeDescriptor::Text.wire_shape(),
        WasmWireShape::PackedPtrLen
    );
    assert_eq!(
        WasmTypeDescriptor::Bytes.wire_shape(),
        WasmWireShape::PackedPtrLen
    );
    assert_eq!(
        WasmTypeDescriptor::Record {
            fields: vec!["id".to_string()]
        }
        .wire_shape(),
        WasmWireShape::StructuredResultBuffer
    );
    assert_eq!(
        WasmTypeDescriptor::Option(Box::new(WasmTypeDescriptor::Scalar(WasmScalarType::I64)))
            .wire_shape(),
        WasmWireShape::StructuredResultBuffer
    );
    assert_eq!(
        WasmTypeDescriptor::Result {
            ok: Box::new(WasmTypeDescriptor::Text),
            err: Box::new(WasmTypeDescriptor::Text),
        }
        .wire_shape(),
        WasmWireShape::StructuredResultBuffer
    );
    assert_eq!(
        WasmTypeDescriptor::Handle.wire_shape(),
        WasmWireShape::HandleSlot
    );
}

#[test]
fn abi_descriptor_exports_wire_shapes_in_canonical_order() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([
        ("text".to_string(), WasmTypeDescriptor::Text),
        (
            "count".to_string(),
            WasmTypeDescriptor::Scalar(WasmScalarType::I64),
        ),
        (
            "record".to_string(),
            WasmTypeDescriptor::Record {
                fields: vec!["name".to_string()],
            },
        ),
    ]));

    let shapes = descriptor.export_wire_shapes();
    assert_eq!(
        shapes.keys().cloned().collect::<Vec<_>>(),
        vec![
            "count".to_string(),
            "record".to_string(),
            "text".to_string()
        ]
    );
    assert_eq!(shapes["count"], WasmWireShape::ScalarSlot);
    assert_eq!(shapes["record"], WasmWireShape::StructuredResultBuffer);
    assert_eq!(shapes["text"], WasmWireShape::PackedPtrLen);
}

#[test]
fn bytes_and_text_literals_share_the_packed_pointer_length_wire_shape() {
    let text = derive_wasm_type(&AnfExpr::Literal(LiteralValue::Text("hello".to_string())));
    let bytes = derive_wasm_type(&AnfExpr::Literal(LiteralValue::Bytes(vec![1, 2, 3])));

    assert_eq!(text, WasmTypeDescriptor::Text);
    assert_eq!(bytes, WasmTypeDescriptor::Bytes);
    assert_eq!(text.wire_shape(), WasmWireShape::PackedPtrLen);
    assert_eq!(bytes.wire_shape(), WasmWireShape::PackedPtrLen);
}
