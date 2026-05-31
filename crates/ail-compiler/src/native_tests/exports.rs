use super::helpers::*;
use crate::anf::{AnfBinding, AnfExpr};
use crate::native_binding::native_export_name;

fn int_binding(name: &str, source_ref: u32, value: i64) -> AnfBinding {
    AnfBinding {
        source_ref: NodeRef(source_ref),
        name: name.to_string(),
        expr: AnfExpr::Literal(crate::core_ir::LiteralValue::Int(value)),
    }
}

#[test]
fn native_export_name_sanitizes_binding_name_deterministically() {
    assert_eq!(native_export_name("fn.add-hot path"), "fn_add_hot_path");
    assert_eq!(native_export_name("9lives"), "ail_9lives");
    assert_eq!(native_export_name(""), "ail_binding");
    assert_eq!(native_export_name("host_call"), "ail_host_call");
}

#[test]
fn emit_native_preserves_source_names_when_native_export_is_sanitized() {
    let anf = anf_for_binding(int_binding("fn.add\0hot path", 0, 42));

    let artifact = emit_native(&anf).unwrap();

    assert!(
        !artifact.native_bytes.is_empty(),
        "native emission must succeed with a sanitized object export"
    );
    assert_eq!(
        artifact.source_map.entries[0].binding_name,
        "fn.add\0hot path"
    );
    assert_eq!(
        artifact.capabilities_manifest.entries[0].name,
        "fn.add\0hot path"
    );
}

#[test]
fn emit_native_rejects_duplicate_sanitized_export_names() {
    let anf = anf_for_bindings(vec![
        int_binding("fn.add", 0, 1),
        int_binding("fn-add", 1, 2),
    ]);

    let err = emit_native(&anf).unwrap_err();
    let CompileError::NativeEncodingError(msg) = err else {
        panic!("expected NativeEncodingError for duplicate native export name");
    };

    assert!(
        msg.contains("AIL-NATIVE-ABI-SYMBOL-DUPLICATE"),
        "diagnostic must use stable duplicate symbol code, got: {msg}"
    );
    assert!(
        msg.contains("category=symbol-name-shape"),
        "diagnostic must carry the stable symbol category, got: {msg}"
    );
    assert!(
        !msg.contains("fn.add") && !msg.contains("fn-add"),
        "diagnostic must redact raw source binding names, got: {msg}"
    );
}
