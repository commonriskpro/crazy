use super::*;

#[test]
fn source_ignored_expression_statement_diagnostics_ignore_effect_statements() {
    let warnings = source_ignored_expression_statement_diagnostics(
        r#"capability log.write
fn main() -> Unit {
  log.write("hi")
  return ()
}
grant main log.write
"#,
    );

    assert!(
        warnings.is_empty(),
        "effect statements must not produce ignored-expression warnings: {warnings:?}"
    );
}

#[test]
fn source_ignored_expression_statement_diagnostics_report_pure_statements() {
    let warnings = source_ignored_expression_statement_diagnostics(
        r#"fn main() -> Int {
  1 + 2
  if true {
    3 + 4
    return 1
  } else {
    return 0
  }
}
"#,
    );

    let lines = warnings
        .iter()
        .map(|warning| warning.line_num)
        .collect::<Vec<_>>();
    assert_eq!(lines, [2, 4]);
}

#[test]
fn source_unused_binding_diagnostics_report_unused_locals() {
    let warnings = source_unused_binding_diagnostics(
        r#"fn main() -> Int {
  let unused = 1
  let used: Int = 2
  return used
}
"#,
    );

    assert_eq!(
        warnings,
        vec![SourceUnusedBinding {
            name: "unused".to_string(),
            line_num: 2,
        }]
    );
}

#[test]
fn source_unused_binding_diagnostics_ignore_underscore_bindings() {
    let warnings = source_unused_binding_diagnostics(
        r#"fn main() -> Int {
  let _ignored = 1
  return 2
}
"#,
    );

    assert!(
        warnings.is_empty(),
        "underscore-prefixed bindings are intentional discards: {warnings:?}"
    );
}
