use super::helpers::*;

fn stage_hashes() -> StageHashes {
    StageHashes {
        graph_snapshot_hash: [0; 32],
        verification_report_hash: [0; 32],
        core_ir_hash: [0; 32],
        anf_ir_hash: None,
        wasm_hash: None,
        native_hash: None,
        source_map_hash: None,
        artifact_manifest_hash: None,
    }
}

fn node(index: u32, kind: CoreNodeKind, name: &str) -> CoreNode {
    CoreNode {
        source_ref: NodeRef(index),
        kind,
        name: name.to_string(),
        ty: None,
        expr: None,
    }
}

#[test]
fn core_ir_diagnostics_report_redacted_production_issue_shapes() {
    let mut first = node(0, CoreNodeKind::Function, "main");
    first.ty = Some(CoreType::Generic(None));
    first.expr = Some(CoreExpr::Lambda {
        params: vec!["secret_binding".to_string(), "secret_binding".to_string()],
        body: Box::new(CoreExpr::TupleNew(vec![
            CoreExpr::Call {
                func: "secret_missing_function".to_string(),
                args: vec![],
            },
            CoreExpr::Var("secret_missing_function".to_string()),
        ])),
    });

    let mut duplicate = node(1, CoreNodeKind::Function, "main");
    duplicate.expr = Some(CoreExpr::Placeholder);

    let mut invalid_type = node(2, CoreNodeKind::Type, "");
    invalid_type.ty = Some(CoreType::NormalizedText("SECRET_FORM".to_string()));

    let ir = CoreIr {
        nodes: vec![invalid_type, duplicate, first],
        stage_hashes: stage_hashes(),
    };

    let issues = ir.diagnostic_issues_for_entry("main");
    let mut sorted = issues.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(issues, sorted, "diagnostic ordering/dedup must be stable");

    let codes = issues.iter().map(|issue| issue.code).collect::<Vec<_>>();
    assert!(codes.contains(&CoreIrIssueCode::InvalidNodeShape));
    assert!(codes.contains(&CoreIrIssueCode::InvalidTypeShape));
    assert!(codes.contains(&CoreIrIssueCode::DuplicateSymbol));
    assert!(codes.contains(&CoreIrIssueCode::DuplicateBinding));
    assert!(codes.contains(&CoreIrIssueCode::MissingReference));
    assert!(codes.contains(&CoreIrIssueCode::UnsupportedPrimitive));

    let rendered = format!("{issues:?}");
    assert!(
        !rendered.contains("secret"),
        "diagnostics must redact raw names"
    );
    assert!(
        !rendered.contains("SECRET"),
        "diagnostics must redact raw names"
    );
    assert!(
        rendered.contains("hash:"),
        "redacted diagnostics should retain stable shape hashes"
    );
}

#[test]
fn core_ir_diagnostics_report_missing_entry_without_leaking_entry_name() {
    let ir = CoreIr {
        nodes: vec![node(7, CoreNodeKind::Function, "helper")],
        stage_hashes: stage_hashes(),
    };

    let issues = ir.diagnostic_issues_for_entry("secret_entrypoint");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, CoreIrIssueCode::MissingEntry);
    assert_eq!(issues[0].category_str(), "missing-reference");

    let rendered = issues[0].detail.clone();
    assert!(!rendered.contains("secret_entrypoint"));
    assert!(rendered.contains("entry/len:17/hash:"));
}

#[test]
fn core_ir_diagnostics_deduplicate_repeated_missing_references_per_node() {
    let mut main = node(0, CoreNodeKind::Function, "main");
    main.expr = Some(CoreExpr::TupleNew(vec![
        CoreExpr::Var("private_missing".to_string()),
        CoreExpr::Var("private_missing".to_string()),
    ]));
    let ir = CoreIr {
        nodes: vec![main],
        stage_hashes: stage_hashes(),
    };

    let issues = ir.diagnostic_issues();
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue.code == CoreIrIssueCode::MissingReference)
            .count(),
        1,
        "repeated references with the same redacted shape must be deduplicated"
    );
}
