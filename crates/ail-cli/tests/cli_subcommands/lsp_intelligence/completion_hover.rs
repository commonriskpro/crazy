// Mechanical phase 2 split from lsp_intelligence.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn lsp_completion_covers_source_typed_let_annotations() {
    let completion_output = ail()
        .args(["lsp", "--complete", "let", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "let"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("let ${1:name}: ${2:Int} = ${3:value}")),
        "completion must include typed let snippet; got: {items:?}"
    );
}
#[test]
fn lsp_completion_covers_source_match_expressions() {
    let completion_output = ail()
        .args(["lsp", "--complete", "match", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "match"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("match ${1:value}")),
        "completion must include match snippet; got: {items:?}"
    );
}
#[test]
fn lsp_completion_covers_source_consts() {
    let completion_output = ail()
        .args(["lsp", "--complete", "const", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "const"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("const ${1:name}: ${2:Int} = ${3:42}")),
        "completion must include const snippet; got: {items:?}"
    );
}
#[test]
fn lsp_completion_and_hover_cover_acl_test_authoring() {
    let completion_output = ail()
        .args(["lsp", "--complete", "create_test", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "op create_test"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("op create_test id=test.")),
        "completion must include create_test snippet; got: {items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "create_test", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let hover = parse_json_output(&hover_output);
    assert_eq!(hover["status"], "ok");
    assert!(
        hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("ail test"),
        "hover must explain create_test integration with ail test; got: {hover}"
    );
}
#[test]
fn lsp_completion_and_hover_cover_ail_source_authoring() {
    let completion_output = ail()
        .args(["lsp", "--complete", "fn", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "fn"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("fn ${1:name}")),
        "completion must include AIL source function snippet; got: {items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "fn", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let hover = parse_json_output(&hover_output);
    assert_eq!(hover["status"], "ok");
    assert!(
        hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("typed AIL source function"),
        "hover must explain AIL source functions; got: {hover}"
    );
}
#[test]
fn lsp_completion_and_hover_cover_ail_source_operators() {
    let completion_output = ail()
        .args(["lsp", "--complete", "+", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "+"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("${1:left} + ${2:right}")),
        "completion must include AIL source + operator snippet; got: {items:?}"
    );
    assert!(
        items.iter().any(|item| item["label"] == "++"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("${1:left} ++ ${2:right}")),
        "completion must include AIL source ++ operator snippet; got: {items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "&&", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let hover = parse_json_output(&hover_output);
    assert_eq!(hover["status"], "ok");
    assert!(
        hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("logical and"),
        "hover must explain AIL source && operator; got: {hover}"
    );

    let dot_completion_output = ail()
        .args(["lsp", "--complete", ".", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dot_completion = parse_json_output(&dot_completion_output);
    let dot_items = dot_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        dot_items.iter().any(
            |item| item["label"] == "." && item["detail"] == "AIL source Record field operator"
        ),
        "completion must include AIL source record dot operator; got: {dot_items:?}"
    );

    let record_literal_completion_output = ail()
        .args(["lsp", "--complete", "{", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let record_literal_completion = parse_json_output(&record_literal_completion_output);
    let record_literal_items = record_literal_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        record_literal_items
            .iter()
            .any(|item| item["label"] == "{" && item["detail"] == "AIL source Record literal"),
        "completion must include AIL source record literal; got: {record_literal_items:?}"
    );

    let record_update_completion_output = ail()
        .args(["lsp", "--complete", "...", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let record_update_completion = parse_json_output(&record_update_completion_output);
    let record_update_items = record_update_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        record_update_items
            .iter()
            .any(|item| item["label"] == "..."
                && item["detail"] == "AIL source Record update spread"),
        "completion must include AIL source record update spread; got: {record_update_items:?}"
    );
}
#[test]
fn lsp_stdio_hover_reports_source_operator_metadata() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 1+2\n")
        .expect("source fixture must be written so file URI can resolve");
    let uri = format!("file://{}", source.path().display());
    let open = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ail",
                "version": 1,
                "text": "fn main() -> Int = 1+2\n",
            }
        }
    })
    .to_string();
    let hover = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source.path().display()) },
            "position": { "line": 0, "character": 20 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open.len(),
        open,
        hover.len(),
        hover
    );

    ail()
        .args(["lsp", "--stdio"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("textDocument/publishDiagnostics"))
        .stdout(predicate::str::contains("Adds two Int expressions"));
}
