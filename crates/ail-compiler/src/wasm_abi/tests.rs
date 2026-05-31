use std::collections::BTreeMap;

use super::{
    AbiDescriptor, AbiDescriptorIssue, WasmScalarType, WasmTypeDescriptor, WasmWireShape,
    derive_wasm_type, export_name,
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

#[test]
fn abi_descriptor_validation_accepts_current_canonical_descriptor() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([
        (
            "main".to_string(),
            WasmTypeDescriptor::Scalar(WasmScalarType::I64),
        ),
        (
            "profile".to_string(),
            WasmTypeDescriptor::Record {
                fields: vec!["id".to_string(), "name".to_string()],
            },
        ),
    ]));

    assert!(descriptor.validation_issues().is_empty());
    assert!(descriptor.is_valid_for_runtime());
}

#[test]
fn abi_descriptor_validation_reports_version_and_export_name_issues() {
    let descriptor = AbiDescriptor {
        abi_version: super::ABI_VERSION + 1,
        exports: BTreeMap::from([
            ("".to_string(), WasmTypeDescriptor::Text),
            ("fn.main".to_string(), WasmTypeDescriptor::Bytes),
        ]),
    };

    let issues = descriptor.validation_issues();
    assert!(issues.contains(&AbiDescriptorIssue::IncompatibleVersion {
        expected: super::ABI_VERSION,
        actual: super::ABI_VERSION + 1,
    }));
    assert!(issues.contains(&AbiDescriptorIssue::EmptyExportName));
    assert!(issues.contains(&AbiDescriptorIssue::LegacyGraphExportName {
        export: "fn.main".to_string(),
    }));
    assert!(!descriptor.is_valid_for_runtime());
}

#[test]
fn abi_descriptor_validation_reports_ambiguous_structured_shapes() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([
        (
            "empty_record".to_string(),
            WasmTypeDescriptor::Record { fields: vec![] },
        ),
        (
            "duplicate_field".to_string(),
            WasmTypeDescriptor::Record {
                fields: vec!["id".to_string(), "id".to_string()],
            },
        ),
        (
            "empty_variant".to_string(),
            WasmTypeDescriptor::Variant { tags: vec![] },
        ),
        (
            "duplicate_tag".to_string(),
            WasmTypeDescriptor::Variant {
                tags: vec!["Ok".to_string(), "Ok".to_string()],
            },
        ),
    ]));

    let issues = descriptor.validation_issues();
    assert!(issues.contains(&AbiDescriptorIssue::EmptyRecordFields {
        export: "empty_record".to_string(),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::DuplicateRecordField {
        export: "duplicate_field".to_string(),
        field: "id".to_string(),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::EmptyVariantTags {
        export: "empty_variant".to_string(),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::DuplicateVariantTag {
        export: "duplicate_tag".to_string(),
        tag: "Ok".to_string(),
    }));
}

#[test]
fn abi_descriptor_validation_reports_unstable_identifiers() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([
        (
            "bad-export".to_string(),
            WasmTypeDescriptor::Record {
                fields: vec!["ok".to_string(), "bad-field".to_string()],
            },
        ),
        (
            "variant".to_string(),
            WasmTypeDescriptor::Variant {
                tags: vec!["Some".to_string(), "Bad Tag".to_string()],
            },
        ),
    ]));

    let issues = descriptor.validation_issues();
    assert!(issues.contains(&AbiDescriptorIssue::InvalidExportName {
        export: "bad-export".to_string(),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::InvalidRecordField {
        export: "bad-export".to_string(),
        field: "bad-field".to_string(),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::InvalidVariantTag {
        export: "variant".to_string(),
        tag: "Bad Tag".to_string(),
    }));
}

#[test]
fn abi_descriptor_validation_returns_issues_in_canonical_order() {
    let descriptor = AbiDescriptor {
        abi_version: super::ABI_VERSION + 1,
        exports: BTreeMap::from([(
            "z_export".to_string(),
            WasmTypeDescriptor::Tuple(vec![
                WasmTypeDescriptor::Variant {
                    tags: vec!["bad tag z".to_string(), "bad tag a".to_string()],
                },
                WasmTypeDescriptor::Record {
                    fields: vec!["bad-field-z".to_string(), "bad-field-a".to_string()],
                },
            ]),
        )]),
    };

    assert_eq!(
        descriptor.validation_issues(),
        vec![
            AbiDescriptorIssue::IncompatibleVersion {
                expected: super::ABI_VERSION,
                actual: super::ABI_VERSION + 1,
            },
            AbiDescriptorIssue::InvalidRecordField {
                export: "z_export".to_string(),
                field: "bad-field-a".to_string(),
            },
            AbiDescriptorIssue::InvalidRecordField {
                export: "z_export".to_string(),
                field: "bad-field-z".to_string(),
            },
            AbiDescriptorIssue::InvalidVariantTag {
                export: "z_export".to_string(),
                tag: "bad tag a".to_string(),
            },
            AbiDescriptorIssue::InvalidVariantTag {
                export: "z_export".to_string(),
                tag: "bad tag z".to_string(),
            },
        ]
    );
}

#[test]
fn abi_descriptor_validation_reports_module_boundary_mismatches() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([
        ("text_result".to_string(), WasmTypeDescriptor::Text),
        (
            "float_result".to_string(),
            WasmTypeDescriptor::Scalar(WasmScalarType::F64),
        ),
        (
            "record_result".to_string(),
            WasmTypeDescriptor::Record {
                fields: vec!["id".to_string()],
            },
        ),
        ("missing_result".to_string(), WasmTypeDescriptor::Handle),
    ]));
    let mut module = super::AbiModuleShape::new(BTreeMap::from([
        (
            "text_result".to_string(),
            super::AbiFunctionSignature::new(vec![WasmScalarType::I32], Some(WasmScalarType::I32)),
        ),
        (
            "float_result".to_string(),
            super::AbiFunctionSignature::new(vec![], Some(WasmScalarType::F64)),
        ),
        (
            "record_result".to_string(),
            super::AbiFunctionSignature::new(vec![], Some(WasmScalarType::I64)),
        ),
    ]));
    module.imports.push(super::AbiImportShape::new(
        "ail host",
        "host.call",
        super::AbiFunctionSignature::new(vec![WasmScalarType::I32], Some(WasmScalarType::I64)),
    ));

    let issues = descriptor.validation_issues_for_module(&module);
    assert!(issues.contains(&AbiDescriptorIssue::UnsupportedTypeLayout {
        export: "float_result".to_string(),
        layout: super::AbiTypeLayout::F64Scalar,
    }));
    assert!(issues.contains(&AbiDescriptorIssue::InvalidImportName {
        module: "ail host".to_string(),
        name: "host.call".to_string(),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::ArgumentTypeMismatch {
        export: "text_result".to_string(),
        index: 0,
        expected: WasmScalarType::I64,
        actual: WasmScalarType::I32,
    }));
    assert!(issues.contains(&AbiDescriptorIssue::ReturnTypeMismatch {
        export: "text_result".to_string(),
        expected: Some(WasmScalarType::I64),
        actual: Some(WasmScalarType::I32),
    }));
    assert!(issues.contains(&AbiDescriptorIssue::ReturnTypeMismatch {
        export: "record_result".to_string(),
        expected: Some(WasmScalarType::I32),
        actual: Some(WasmScalarType::I64),
    }));
    assert!(
        issues.contains(&AbiDescriptorIssue::MemoryBoundaryMismatch {
            export: "text_result".to_string(),
        })
    );
    assert!(
        issues.contains(&AbiDescriptorIssue::MemoryBoundaryMismatch {
            export: "record_result".to_string(),
        })
    );
    assert!(issues.contains(&AbiDescriptorIssue::MissingExportFunction {
        export: "missing_result".to_string(),
    }));
}

#[test]
fn abi_descriptor_validation_diagnostics_are_redacted_and_stable() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([(
        "customer-secret-token".to_string(),
        WasmTypeDescriptor::Record {
            fields: vec!["raw-email-address".to_string()],
        },
    )]));
    let mut module = super::AbiModuleShape::new(BTreeMap::from([(
        "customer-secret-token".to_string(),
        super::AbiFunctionSignature::new(vec![WasmScalarType::F64], Some(WasmScalarType::I64)),
    )]));
    module.imports.push(super::AbiImportShape::new(
        "private module",
        "token.reader",
        super::AbiFunctionSignature::new(vec![], Some(WasmScalarType::I64)),
    ));

    let diagnostics = descriptor.validation_diagnostics_for_module(&module);
    let rendered = format!("{diagnostics:?}");
    assert!(!rendered.contains("customer-secret-token"));
    assert!(!rendered.contains("raw-email-address"));
    assert!(!rendered.contains("private module"));
    assert!(!rendered.contains("token.reader"));
    assert!(rendered.contains("export:21:"));
    assert!(rendered.contains("field:17:"));
    assert!(rendered.contains("import_module:14:"));
    assert!(rendered.contains("import_name:12:"));

    assert_eq!(
        diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![
            super::AbiDiagnosticCode::InvalidExportName,
            super::AbiDiagnosticCode::InvalidRecordField,
            super::AbiDiagnosticCode::InvalidImportName,
            super::AbiDiagnosticCode::ArgumentTypeMismatch,
            super::AbiDiagnosticCode::ReturnTypeMismatch,
            super::AbiDiagnosticCode::MemoryBoundaryMismatch,
        ]
    );
    assert_eq!(
        diagnostics,
        descriptor.validation_diagnostics_for_module(&module),
        "diagnostic order and redaction must be deterministic"
    );
}

#[test]
fn abi_descriptor_validation_reports_empty_tuple_layout_as_unsupported() {
    let descriptor = AbiDescriptor::new(BTreeMap::from([(
        "unit_tuple".to_string(),
        WasmTypeDescriptor::Tuple(vec![]),
    )]));

    assert_eq!(
        descriptor.validation_issues(),
        vec![AbiDescriptorIssue::UnsupportedTypeLayout {
            export: "unit_tuple".to_string(),
            layout: super::AbiTypeLayout::EmptyTuple,
        }]
    );
}
