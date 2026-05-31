use super::helpers::*;
use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;
use crate::native::{NativeAbiIssueCategory, NativeAbiIssueCode, validate_native_abi};

fn binding(name: &str, source_ref: u32, expr: AnfExpr) -> AnfBinding {
    AnfBinding {
        source_ref: NodeRef(source_ref),
        name: name.to_string(),
        expr,
    }
}

fn issue_codes(anf: &AnfIr) -> Vec<NativeAbiIssueCode> {
    validate_native_abi(anf)
        .issues
        .iter()
        .map(|issue| issue.code)
        .collect()
}

#[test]
fn native_abi_diagnostics_report_stable_codes_categories_and_order() {
    let anf = anf_for_bindings(vec![
        binding(
            "customer-secret-token",
            0,
            AnfExpr::RecordNew {
                fields: vec![(
                    "secret-field-name".to_string(),
                    AnfExpr::Literal(LiteralValue::Unit),
                )],
            },
        ),
        binding(
            "branch_mismatch",
            1,
            AnfExpr::Let {
                name: "flag".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                body: Box::new(AnfExpr::If {
                    cond: "flag".to_string(),
                    then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    else_branch: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                }),
            },
        ),
        binding(
            "call_mismatch",
            2,
            AnfExpr::Let {
                name: "flag".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                body: Box::new(AnfExpr::Call {
                    func: "add".to_string(),
                    args: vec!["flag".to_string()],
                }),
            },
        ),
    ]);

    let diagnostic = validate_native_abi(&anf);

    assert_eq!(
        diagnostic
            .issues
            .iter()
            .map(|issue| (issue.code, issue.category))
            .collect::<Vec<_>>(),
        vec![
            (
                NativeAbiIssueCode::InvalidSymbolShape,
                NativeAbiIssueCategory::SymbolNameShape,
            ),
            (
                NativeAbiIssueCode::UnsupportedTypeLayout,
                NativeAbiIssueCategory::UnsupportedTypeLayout,
            ),
            (
                NativeAbiIssueCode::CallArityMismatch,
                NativeAbiIssueCategory::ArgumentReturnMismatch,
            ),
            (
                NativeAbiIssueCode::ArgumentShapeMismatch,
                NativeAbiIssueCategory::ArgumentReturnMismatch,
            ),
            (
                NativeAbiIssueCode::ReturnShapeMismatch,
                NativeAbiIssueCategory::ArgumentReturnMismatch,
            ),
        ]
    );
}

#[test]
fn native_abi_diagnostics_redact_source_descriptors() {
    let anf = anf_for_bindings(vec![
        binding(
            "customer-secret-token",
            0,
            AnfExpr::Call {
                func: "private.secret.operation".to_string(),
                args: vec!["missing-private-value".to_string()],
            },
        ),
        binding(
            "customer.secret.token",
            1,
            AnfExpr::Literal(LiteralValue::Int(1)),
        ),
    ]);

    let diagnostic = validate_native_abi(&anf);
    let text = diagnostic.to_error_message();

    assert!(text.contains("AIL-NATIVE-ABI-SYMBOL-SHAPE"));
    assert!(text.contains("AIL-NATIVE-ABI-UNSUPPORTED-LAYOUT"));
    assert!(!text.contains("customer-secret-token"));
    assert!(!text.contains("customer.secret.token"));
    assert!(!text.contains("private.secret.operation"));
    assert!(!text.contains("missing-private-value"));
}

#[test]
fn emit_native_rejects_blocking_abi_issues_with_redacted_stable_codes() {
    let anf = anf_for_binding(binding(
        "fn_op",
        0,
        AnfExpr::RecordNew {
            fields: vec![(
                "secret-field-name".to_string(),
                AnfExpr::Literal(LiteralValue::Unit),
            )],
        },
    ));

    let err = emit_native(&anf).unwrap_err();
    let CompileError::NativeEncodingError(msg) = err else {
        panic!("expected NativeEncodingError for native ABI validation failure");
    };

    assert!(msg.contains("AIL-NATIVE-ABI-UNSUPPORTED-LAYOUT"));
    assert!(msg.contains("category=unsupported-type-layout"));
    assert!(!msg.contains("secret-field-name"));
}

#[test]
fn emit_native_keeps_sanitized_symbol_names_compatible_but_reports_diagnostic() {
    let anf = anf_for_binding(binding(
        "fn.add\0hot path",
        0,
        AnfExpr::Literal(LiteralValue::Int(42)),
    ));

    let diagnostic = validate_native_abi(&anf);
    assert_eq!(
        issue_codes(&anf),
        vec![NativeAbiIssueCode::InvalidSymbolShape]
    );

    let artifact = emit_native(&anf).unwrap();
    assert!(!artifact.native_bytes.is_empty());
    assert_eq!(
        artifact.source_map.entries[0].binding_name,
        "fn.add\0hot path"
    );
    assert_eq!(
        artifact.capabilities_manifest.entries[0].name,
        "fn.add\0hot path"
    );
    assert_eq!(
        diagnostic.issues[0].category,
        NativeAbiIssueCategory::SymbolNameShape
    );
}
