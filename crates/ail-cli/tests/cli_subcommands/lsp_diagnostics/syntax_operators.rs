// Mechanical phase 2 split from lsp_diagnostics.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn lsp_diagnose_reports_ail_source_missing_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./missing.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-import");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("failed to resolve AIL source")
    );
}
#[test]
fn lsp_diagnose_reports_ail_source_cyclic_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let main = dir.child("main.ail");
    main.write_str("use \"./dep.ail\"\nfn main() -> Int = dep()\n")
        .expect("main fixture must be written");
    let dep = dir.child("dep.ail");
    dep.write_str("use \"./main.ail\"\nfn dep() -> Int = 42\n")
        .expect("dep fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-import");
    let message = v["data"]["diagnostics"][0]["message"]
        .as_str()
        .expect("diagnostic message");
    assert!(message.contains("cyclic AIL source import detected:"));
    assert!(message.contains("main.ail ->"));
    assert!(message.contains("dep.ail ->"));
}

#[test]
fn lsp_diagnose_accepts_inline_return_markers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = return add(20, 22)\ntest add = return eq(main(), 42)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}

#[test]
fn lsp_diagnose_reports_numeric_leading_source_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn 1bad() -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_PARSE_INVALID_NAME"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.parse.name"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["family"],
        "AIL_SOURCE_PARSER"
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 0);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        3
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 7);
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["span"],
        v["data"]["diagnostics"][0]["range"]
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("declaration name `1bad` segment `1bad` must start with a letter or `_`")
    );
}

#[test]
fn lsp_diagnose_reports_reserved_generated_statement_binding_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let __ail_stmt_2 = 1\n  return __ail_stmt_2\n}\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_PARSE_INVALID_NAME"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.parse.name"
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        6
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 18);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("uses reserved compiler-generated prefix `__ail_stmt_`")
    );
}

#[test]
fn lsp_diagnose_warns_for_ignored_pure_expression_statements() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  1 + 2\n  return 0\n}\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["severity"], 2);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LSP_IGNORED_EXPRESSION"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailRepair"]["code"],
        "remove.ignored_expression_statement"
    );
    let uri = format!("file://{}", source.path().display());
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailRepair"]["edit"]["changes"][uri.as_str()][0]["newText"],
        ""
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lsp.ignored_expression"
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        2
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("ignored expression statement has no direct effect")
    );
}

#[test]
fn lsp_diagnose_warns_for_unused_source_bindings() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let unused = 1\n  let used: Int = 2\n  return used\n}\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["severity"], 2);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LSP_UNUSED_BINDING"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lsp.unused_binding"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailRepair"]["code"],
        "prefix.unused_binding_with_underscore"
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unused local binding `unused`")
    );
}

#[test]
fn lsp_diagnose_reports_source_match_expression_shape_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = match { _ => 1 }\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LOWER_MATCH_EXPRESSION"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lower.match"
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("match expression requires a scrutinee")
    );
}

#[test]
fn lsp_diagnose_reports_source_if_expression_shape_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = if { 1 } else { 2 }\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LOWER_CONTROL_EXPRESSION"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lower.control"
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("if expression requires a condition")
    );
}

#[test]
fn lsp_diagnose_accepts_multiline_source_if_else_if_blocks() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            r#"
fn bucket(x: Int) -> Int {
  if gt(x, 10) {
    3
  } else if gt(x, 0) {
    2
  } else {
    1
  }
}
test bucket_negative = eq(bucket(-5), 1)
"#,
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}

#[test]
fn lsp_diagnose_accepts_return_markers_inside_source_if_branches() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            r#"
fn bucket(x: Int) -> Int {
  if gt(x, 10) {
    return 3
  } else if gt(x, 0) {
    return 2
  } else {
    return 1
  }
}
test bucket_negative = eq(bucket(-5), 1)
"#,
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}

#[test]
fn lsp_diagnose_reports_malformed_source_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn bad..name() -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("declaration name `bad..name` contains an empty path segment")
    );
}

#[test]
fn lsp_diagnose_reports_precise_module_name_spans() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("module 1bad\nfn main() -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_PARSE_INVALID_NAME"
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 0);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        7
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 11);
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["span"],
        v["data"]["diagnostics"][0]["range"]
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_parameter_type() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(x: Mystery) -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 0);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        11
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 18);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source type `Mystery`")
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_return_type_span() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Mystery = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 0);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        13
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 20);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source type `Mystery`")
    );
}

#[test]
fn lsp_diagnose_reports_source_functions_that_shadow_builtins() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn add(x: Int) -> Int = x\nfn main() -> Int = add(1)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("function declaration `fn.add` shadows builtin `add`")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_SYMBOL_BUILTIN_SHADOW"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.symbol.builtin_shadow"
    );
}
#[test]
fn lsp_diagnose_reports_duplicate_source_parameters() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(x: Int, x: Int) -> Int = x\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 0);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        16
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 17);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("duplicate parameter `x`")
    );
}
#[test]
fn lsp_diagnose_reports_dotted_source_parameter_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(x.y: Int) -> Int = x.y\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 0);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        8
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 0);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 11);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("local binding name `x.y` must not contain `.`")
    );
}
#[test]
fn lsp_diagnose_reports_dotted_source_let_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let x.y = 1\n  return x.y\n}\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        6
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 9);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("local binding name `x.y` must not contain `.`")
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_let_type_span() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let value: Mystery = 1\n  return value\n}\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        13
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["line"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 20);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source type `Mystery`")
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_type_annotation_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = let_typed(value, Mystery, 1, 1, value)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source type annotation `Mystery`")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_TYPE_UNSUPPORTED_ANNOTATION"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.type.annotation"
    );
}

#[test]
fn lsp_diagnose_reports_typed_let_line_marker_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = let_typed(value, Int, nope, 1, value)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("invalid typed let source line marker `nope`")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LET_LINE_MARKER"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.let.line_marker"
    );
}

#[test]
fn lsp_diagnose_reports_non_finite_source_numeric_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Float = NaN\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source numeric literal `NaN`")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_EXPR_UNSUPPORTED_NUMERIC"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.expr.literal"
    );
}
#[test]
fn lsp_diagnose_accepts_source_infix_addition() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 1 + 2\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_accepts_source_infix_arithmetic_precedence() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("test math = 10 - 2 * 3 + (8 / 4 + 7 % 4) == 9\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_accepts_source_unary_minus() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn negated(x: Int) -> Int = -x
test grouped = -(1 + 2) == -3
fn main() -> Int = negated(3)
",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_accepts_source_infix_equality() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test addition = 1 + 2 == 3\ntest different = 1 + 2 != 4\nfn main() -> Int = 0\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_accepts_source_infix_ordering() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test gt_case = 3 > 2\n\
test ge_case = 3 >= 3\n\
test lt_case = 2 < 3\n\
test le_case = 2 <= 2\n\
fn main() -> Int = 0\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_accepts_source_infix_boolean_logic() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test combined = 3 > 2 && 2 < 3 || false\n\
fn main() -> Int = 0\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_accepts_source_unary_not_and_grouping() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test grouped = !(3 > 2 && false) == true\n\
fn main() -> Int = 0\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}
#[test]
fn lsp_diagnose_reports_unsupported_source_expression_syntax() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 1 ** 2\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source expression `1 ** 2`")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_EXPR_UNSUPPORTED"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.expr.unsupported"
    );
}
#[test]
fn lsp_diagnose_reports_unsupported_source_string_escapes() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Text = \"bad \\q escape\"\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("malformed string literal")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_EXPR_MALFORMED_STRING"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.expr.literal"
    );
}
#[test]
fn lsp_diagnose_reports_malformed_source_string_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Text = \"unterminated\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("malformed string literal")
    );
}
#[test]
fn lsp_diagnose_reports_untyped_source_builtins() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = fold(0, [1], 0)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source builtin `fold` has no type inference")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_BUILTIN_UNTYPED"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.builtin.untyped"
    );
}
#[test]
fn lsp_stdio_publish_diagnostics_reports_ail_source_missing_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 0\n")
        .expect("source fixture must be written so file URI can resolve");
    let text = "use \"./missing.ail\"\nfn main() -> Int = 0\n";
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": format!("file://{}", source.path().display()),
                "languageId": "ail",
                "version": 1,
                "text": text,
            }
        }
    })
    .to_string();
    let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

    ail()
        .args(["lsp", "--stdio"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("textDocument/publishDiagnostics"))
        .stdout(predicate::str::contains("ail-source-import"))
        .stdout(predicate::str::contains("failed to resolve AIL source"));
}
