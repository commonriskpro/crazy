// Mechanical phase 2 split from lsp_diagnostics.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

#[test]
fn lsp_diagnose_accepts_source_option_result_constructors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn maybe(flag: Bool) -> Option<Int> = if flag { Some(42) } else { None }\n\
fn ok_value() -> Result<Int, Text> = Ok(42)\n\
fn err_value() -> Result<Int, Text> = Err(\"boom\")\n",
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
fn lsp_diagnose_accepts_source_type_aliases() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn text_len(value: String) -> int = len(value)\n\
fn first(values: List<i64>) -> i64 = values[0]\n\
fn maybe(flag: bool) -> Option<i32> = if flag { Some(42) } else { None }\n\
fn result() -> Result<i64, String> = Ok(42)\n",
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
fn lsp_diagnose_accepts_source_match_expressions() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn unwrap_or_zero(value: Option<Int>) -> Int = match value { Some(v) => v, None => 0 }\n\
fn result_or_zero(value: Result<Int, Text>) -> Int = match value { Ok(v) => v, Err(e) => 0 }\n",
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
fn lsp_diagnose_accepts_source_unwrap_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn value(input: Option<Int>) -> Int = unwrap_or(input, 0)\n")
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
fn lsp_diagnose_accepts_source_option_predicate_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn has_value(input: Option<Int>) -> Bool = is_some(input)\n\
fn missing(input: Option<Int>) -> Bool = is_none(input)\n",
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
fn lsp_diagnose_accepts_source_result_predicate_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn succeeded(input: Result<Int, Text>) -> Bool = is_ok(input)\n\
fn failed(input: Result<Int, Text>) -> Bool = is_err(input)\n",
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
fn lsp_diagnose_reports_nested_source_match_pattern() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main(value: Option<Result<Int, Text>>) -> Int = match value { Some(Ok(v)) => v, None => 0 }\n",
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
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported nested source match pattern `Some(Ok(v))`")
    );
}
#[test]
fn lsp_diagnose_reports_non_exhaustive_source_match() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(value: Option<Int>) -> Int = match value { Some(v) => v }\n")
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
            .contains("non-exhaustive match for Option<Int>")
    );
}
#[test]
fn lsp_diagnose_reports_unreachable_source_match_arm() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(value: Option<Int>) -> Int = match value { _ => 0, Some(v) => v }\n")
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
            .contains("unreachable match arm `Some(v)`")
    );
}
#[test]
fn lsp_diagnose_reports_ail_source_return_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn ok() -> Int = 0\nfn main() -> Int = true\n")
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
            .contains("line 2: type mismatch in fn.main: expected Int, got Bool")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
}
#[test]
fn lsp_diagnose_reports_ail_source_float_literal_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 1.5\n")
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
            .contains("type mismatch in fn.main: expected Int, got Float")
    );
}
#[test]
fn lsp_diagnose_reports_ail_source_builtin_argument_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn ok() -> Int = 0\nfn main() -> Int = add(\"one\", 1)\n")
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
            .contains("line 2: type mismatch in add argument 1: expected Int, got Text")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
}
