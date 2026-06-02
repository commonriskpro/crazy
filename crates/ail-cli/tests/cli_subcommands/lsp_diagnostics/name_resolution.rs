// Mechanical phase 2 split from lsp_diagnostics.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

#[test]
fn lsp_diagnose_reports_ail_source_duplicate_function_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 0\nfn main() -> Int = 1\n")
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
    assert_eq!(v["data"]["diagnostics_status"], "error");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["warning_count"], 0);
    assert_eq!(
        v["data"]["diagnostic_codes"][0],
        "AIL_SOURCE_SYMBOL_DUPLICATE"
    );
    assert_eq!(
        v["data"]["diagnostic_categories"][0],
        "source.symbol.duplicate"
    );
    assert_eq!(v["data"]["repair_count"], 0);
    assert_eq!(v["data"]["repair_codes"], serde_json::Value::Array(vec![]));
    assert_eq!(
        v["data"]["repair_suggestions"],
        serde_json::Value::Array(vec![])
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("duplicate function declaration `fn.main`")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_SYMBOL_DUPLICATE"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.symbol.duplicate"
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_function_calls() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn ok() -> Int = 0\nfn main() -> Int = typo_add(20, 22)\n")
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
            .contains("line 2: unknown function call `typo_add`")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        19
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 27);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_NAME_UNKNOWN_FUNCTION"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.name.function"
    );
}
#[test]
fn lsp_diagnose_reports_ail_source_builtin_call_arity_errors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn ok() -> Int = 0\nfn main() -> Int = add(20)\n")
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
            .contains("line 2: function call `add` expects 2 argument(s), got 1")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        19
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 22);
    assert_eq!(v["data"]["diagnostics"][0]["code"], "AIL_SOURCE_CALL_ARITY");
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.call.arity"
    );
}
#[test]
fn lsp_diagnose_reports_ail_source_user_call_arity_errors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn add_pair(x: Int, y: Int) -> Int = x + y\nfn main() -> Int = add_pair(20)\n")
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
            .contains("function call `add_pair` expects 2 argument(s), got 1")
    );
}
#[test]
fn lsp_diagnose_reports_ail_source_unknown_variables() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn ok() -> Int = 0\nfn main() -> Int = add(x, 1)\n")
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
            .contains("line 2: unknown variable `x`")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["diagnostics"][0]["range"]["start"]["character"],
        23
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["end"]["character"], 24);
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_NAME_UNKNOWN_VARIABLE"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.name.variable"
    );
}
#[test]
fn lsp_diagnose_accepts_ail_source_params_and_let_variables() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn plus_one(x: Int) -> Int {\n  let base = add(x, 1)\n  return add(base, 1)\n}\n",
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
fn lsp_diagnose_accepts_source_block_tests_with_lets() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test math {\n  let actual: Int = 20 + 22\n  return actual == 42\n}\nfn main() -> Int = 0\n",
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
fn lsp_diagnose_accepts_source_unit_literal() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn noop() -> Unit = ()\nfn main() -> Int = 0\n")
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
fn lsp_diagnose_accepts_source_typed_let_annotations() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let base: Int = 20 + 20\n  return base + 2\n}\n")
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
fn lsp_diagnose_accepts_source_consts() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "const answer: Int = 40 + 2\nfn main() -> Int = answer\ntest answer = answer == 42\n",
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
fn lsp_diagnose_reports_source_typed_let_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let base: Bool = 20 + 20\n  return 0\n}\n")
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
            .contains("line 2: type mismatch in let binding base: expected Bool, got Int")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
}
