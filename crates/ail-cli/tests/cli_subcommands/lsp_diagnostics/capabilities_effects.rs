// Mechanical phase 2 split from lsp_diagnostics.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

#[test]
fn lsp_diagnose_reports_ail_source_unknown_grant_capability() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 0\ngrant main log.write\n")
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
            .contains("grant capability `log.write` is not declared")
    );
}
#[test]
fn lsp_diagnose_accepts_module_test_capability_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "module app
capability log.write
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant smoke log.write
fn main() -> Int = 0
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
fn lsp_diagnose_reports_ambiguous_source_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write
fn smoke() -> Int = 0
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant smoke log.write
fn main() -> Int = 0
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
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("grant target `smoke` is ambiguous; use `fn.smoke` or `test.smoke`")
    );
}
#[test]
fn lsp_diagnose_reports_ungranted_source_effect_call() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write\nfn main() -> Int = effect_call(log.write, write, \"hi\")\n",
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
            .contains("line 2: source item `fn.main` uses capability `log.write` without a grant")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
}

#[test]
fn lsp_diagnose_accepts_source_log_write_alias_with_grant() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write\n\
fn main() -> Int = log.write(\"hi\")\n\
fn alias() -> Int = log_write(\"hi\")\n\
grant main log.write\n\
grant alias log.write\n",
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
fn lsp_diagnose_reports_source_log_write_alias_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("capability log.write\nfn main() -> Int = log.write(42)\ngrant main log.write\n")
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
            .contains("type mismatch in log.write argument 1: expected Text, got Int")
    );
}

#[test]
fn lsp_diagnose_reports_source_log_write_arity_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write\nfn main() -> Int = log.write(\"hi\", \"extra\")\ngrant main log.write\n",
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
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LOWER_EXPRESSION"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lower.expression"
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("log.write requires `log.write(message)`")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_effect_call_capability() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn ok() -> Int = 0\nfn main() -> Int = effect_call(log.write, write, \"hi\")\n")
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
            .contains("line 2: effect_call capability `log.write` is not declared")
    );
    assert_eq!(v["data"]["diagnostics"][0]["range"]["start"]["line"], 1);
}
#[test]
fn lsp_diagnose_reports_ail_source_unknown_grant_target() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("capability log.write\ngrant missing log.write\n")
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
            .contains("grant target `fn.missing` is not declared")
    );
}
