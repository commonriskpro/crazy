// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
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
#[test]
fn lsp_completion_and_hover_cover_ail_source_builtins() {
    let completion_output = ail()
        .args(["lsp", "--complete", "effect", "--json"])
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
        items.iter().any(|item| item["label"] == "effect_call"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("effect_call(${1:log.write}")),
        "completion must include AIL source effect_call snippet; got: {items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "effect_call", "--json"])
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
            .contains("explicit grant"),
        "hover must explain effect_call grants; got: {hover}"
    );

    let first_or_completion_output = ail()
        .args(["lsp", "--complete", "first", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let first_or_completion = parse_json_output(&first_or_completion_output);
    let first_or_items = first_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        first_or_items
            .iter()
            .any(|item| item["label"] == "first_or" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source first_or helper; got: {first_or_items:?}"
    );

    let last_or_completion_output = ail()
        .args(["lsp", "--complete", "last", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let last_or_completion = parse_json_output(&last_or_completion_output);
    let last_or_items = last_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        last_or_items
            .iter()
            .any(|item| item["label"] == "last_or" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source last_or helper; got: {last_or_items:?}"
    );

    let get_or_completion_output = ail()
        .args(["lsp", "--complete", "get", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let get_or_completion = parse_json_output(&get_or_completion_output);
    let get_or_items = get_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        get_or_items
            .iter()
            .any(|item| item["label"] == "get_or" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source get_or helper; got: {get_or_items:?}"
    );

    let is_empty_completion_output = ail()
        .args(["lsp", "--complete", "is_empty", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let is_empty_completion = parse_json_output(&is_empty_completion_output);
    let is_empty_items = is_empty_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        is_empty_items
            .iter()
            .any(|item| item["label"] == "is_empty"
                && item["detail"] == "AIL source sized predicate"),
        "completion must include AIL source is_empty helper; got: {is_empty_items:?}"
    );

    let text_eq_completion_output = ail()
        .args(["lsp", "--complete", "text_eq", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_eq_completion = parse_json_output(&text_eq_completion_output);
    let text_eq_items = text_eq_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_eq_items.iter().any(|item| item["label"] == "text_eq"
            && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_eq helper; got: {text_eq_items:?}"
    );

    let text_trim_completion_output = ail()
        .args(["lsp", "--complete", "text_trim", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_trim_completion = parse_json_output(&text_trim_completion_output);
    let text_trim_items = text_trim_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_trim_items
            .iter()
            .any(|item| item["label"] == "text_trim" && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_trim helper; got: {text_trim_items:?}"
    );

    let int_clamp_completion_output = ail()
        .args(["lsp", "--complete", "int_clamp", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_clamp_completion = parse_json_output(&int_clamp_completion_output);
    let int_clamp_items = int_clamp_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_clamp_items
            .iter()
            .any(|item| item["label"] == "int_clamp"
                && item["detail"] == "AIL source Int bounds helper"),
        "completion must include AIL source int_clamp helper; got: {int_clamp_items:?}"
    );

    let int_abs_or_completion_output = ail()
        .args(["lsp", "--complete", "int_abs_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_abs_or_completion = parse_json_output(&int_abs_or_completion_output);
    let int_abs_or_items = int_abs_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_abs_or_items
            .iter()
            .any(|item| item["label"] == "int_abs_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_abs_or helper; got: {int_abs_or_items:?}"
    );

    let int_neg_or_completion_output = ail()
        .args(["lsp", "--complete", "int_neg_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_neg_or_completion = parse_json_output(&int_neg_or_completion_output);
    let int_neg_or_items = int_neg_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_neg_or_items
            .iter()
            .any(|item| item["label"] == "int_neg_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_neg_or helper; got: {int_neg_or_items:?}"
    );

    let int_add_or_completion_output = ail()
        .args(["lsp", "--complete", "int_add_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_add_or_completion = parse_json_output(&int_add_or_completion_output);
    let int_add_or_items = int_add_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_add_or_items
            .iter()
            .any(|item| item["label"] == "int_add_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_add_or helper; got: {int_add_or_items:?}"
    );

    let int_sub_or_completion_output = ail()
        .args(["lsp", "--complete", "int_sub_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_sub_or_completion = parse_json_output(&int_sub_or_completion_output);
    let int_sub_or_items = int_sub_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_sub_or_items
            .iter()
            .any(|item| item["label"] == "int_sub_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_sub_or helper; got: {int_sub_or_items:?}"
    );

    let int_mul_or_completion_output = ail()
        .args(["lsp", "--complete", "int_mul_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_mul_or_completion = parse_json_output(&int_mul_or_completion_output);
    let int_mul_or_items = int_mul_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_mul_or_items
            .iter()
            .any(|item| item["label"] == "int_mul_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_mul_or helper; got: {int_mul_or_items:?}"
    );

    let int_saturating_add_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_add", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_add_completion = parse_json_output(&int_saturating_add_completion_output);
    let int_saturating_add_items = int_saturating_add_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_add_items
            .iter()
            .any(|item| item["label"] == "int_saturating_add"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_add helper; got: {int_saturating_add_items:?}"
    );

    let int_saturating_sub_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_sub", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_sub_completion = parse_json_output(&int_saturating_sub_completion_output);
    let int_saturating_sub_items = int_saturating_sub_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_sub_items
            .iter()
            .any(|item| item["label"] == "int_saturating_sub"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_sub helper; got: {int_saturating_sub_items:?}"
    );

    let int_saturating_mul_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_mul", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_mul_completion = parse_json_output(&int_saturating_mul_completion_output);
    let int_saturating_mul_items = int_saturating_mul_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_mul_items
            .iter()
            .any(|item| item["label"] == "int_saturating_mul"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_mul helper; got: {int_saturating_mul_items:?}"
    );

    let int_saturating_neg_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_neg", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_neg_completion = parse_json_output(&int_saturating_neg_completion_output);
    let int_saturating_neg_items = int_saturating_neg_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_neg_items
            .iter()
            .any(|item| item["label"] == "int_saturating_neg"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_neg helper; got: {int_saturating_neg_items:?}"
    );

    let int_wrapping_add_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_add", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_add_completion = parse_json_output(&int_wrapping_add_completion_output);
    let int_wrapping_add_items = int_wrapping_add_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_add_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_add"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_add helper; got: {int_wrapping_add_items:?}"
    );

    let int_wrapping_sub_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_sub", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_sub_completion = parse_json_output(&int_wrapping_sub_completion_output);
    let int_wrapping_sub_items = int_wrapping_sub_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_sub_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_sub"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_sub helper; got: {int_wrapping_sub_items:?}"
    );

    let int_wrapping_mul_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_mul", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_mul_completion = parse_json_output(&int_wrapping_mul_completion_output);
    let int_wrapping_mul_items = int_wrapping_mul_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_mul_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_mul"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_mul helper; got: {int_wrapping_mul_items:?}"
    );

    let int_wrapping_neg_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_neg", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_neg_completion = parse_json_output(&int_wrapping_neg_completion_output);
    let int_wrapping_neg_items = int_wrapping_neg_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_neg_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_neg"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_neg helper; got: {int_wrapping_neg_items:?}"
    );

    let int_bit_and_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_and", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_and_completion = parse_json_output(&int_bit_and_completion_output);
    let int_bit_and_items = int_bit_and_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_and_items
            .iter()
            .any(|item| item["label"] == "int_bit_and"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_and helper; got: {int_bit_and_items:?}"
    );

    let int_bit_or_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_or_completion = parse_json_output(&int_bit_or_completion_output);
    let int_bit_or_items = int_bit_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_or_items
            .iter()
            .any(|item| item["label"] == "int_bit_or"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_or helper; got: {int_bit_or_items:?}"
    );

    let int_bit_xor_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_xor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_xor_completion = parse_json_output(&int_bit_xor_completion_output);
    let int_bit_xor_items = int_bit_xor_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_xor_items
            .iter()
            .any(|item| item["label"] == "int_bit_xor"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_xor helper; got: {int_bit_xor_items:?}"
    );

    let int_bit_not_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_not", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_not_completion = parse_json_output(&int_bit_not_completion_output);
    let int_bit_not_items = int_bit_not_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_not_items
            .iter()
            .any(|item| item["label"] == "int_bit_not"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_not helper; got: {int_bit_not_items:?}"
    );

    let int_shift_left_completion_output = ail()
        .args(["lsp", "--complete", "int_shift_left", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_shift_left_completion = parse_json_output(&int_shift_left_completion_output);
    let int_shift_left_items = int_shift_left_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_shift_left_items
            .iter()
            .any(|item| item["label"] == "int_shift_left"
                && item["detail"] == "AIL source Int bit shift helper"),
        "completion must include AIL source int_shift_left helper; got: {int_shift_left_items:?}"
    );

    let int_shift_right_completion_output = ail()
        .args(["lsp", "--complete", "int_shift_right", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_shift_right_completion = parse_json_output(&int_shift_right_completion_output);
    let int_shift_right_items = int_shift_right_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_shift_right_items
            .iter()
            .any(|item| item["label"] == "int_shift_right"
                && item["detail"] == "AIL source Int bit shift helper"),
        "completion must include AIL source int_shift_right helper; got: {int_shift_right_items:?}"
    );

    let int_shift_right_unsigned_completion_output = ail()
        .args(["lsp", "--complete", "int_shift_right_unsigned", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_shift_right_unsigned_completion =
        parse_json_output(&int_shift_right_unsigned_completion_output);
    let int_shift_right_unsigned_items = int_shift_right_unsigned_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_shift_right_unsigned_items
            .iter()
            .any(|item| item["label"] == "int_shift_right_unsigned"
                && item["detail"] == "AIL source Int bit shift helper"),
        "completion must include AIL source int_shift_right_unsigned helper; got: {int_shift_right_unsigned_items:?}"
    );

    let int_div_or_completion_output = ail()
        .args(["lsp", "--complete", "int_div_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_div_or_completion = parse_json_output(&int_div_or_completion_output);
    let int_div_or_items = int_div_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_div_or_items
            .iter()
            .any(|item| item["label"] == "int_div_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_div_or helper; got: {int_div_or_items:?}"
    );

    let int_rem_or_completion_output = ail()
        .args(["lsp", "--complete", "int_rem_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_rem_or_completion = parse_json_output(&int_rem_or_completion_output);
    let int_rem_or_items = int_rem_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_rem_or_items
            .iter()
            .any(|item| item["label"] == "int_rem_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_rem_or helper; got: {int_rem_or_items:?}"
    );

    let text_contains_completion_output = ail()
        .args(["lsp", "--complete", "text_contains", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_contains_completion = parse_json_output(&text_contains_completion_output);
    let text_contains_items = text_contains_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_contains_items
            .iter()
            .any(|item| item["label"] == "text_contains"
                && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_contains helper; got: {text_contains_items:?}"
    );

    let text_index_of_completion_output = ail()
        .args(["lsp", "--complete", "text_index_of", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_index_of_completion = parse_json_output(&text_index_of_completion_output);
    let text_index_of_items = text_index_of_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_index_of_items
            .iter()
            .any(|item| item["label"] == "text_index_of"
                && item["detail"] == "AIL source Text search"),
        "completion must include AIL source text_index_of helper; got: {text_index_of_items:?}"
    );

    let text_parse_int_or_completion_output = ail()
        .args(["lsp", "--complete", "text_parse_int_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_parse_int_or_completion = parse_json_output(&text_parse_int_or_completion_output);
    let text_parse_int_or_items = text_parse_int_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_parse_int_or_items
            .iter()
            .any(|item| item["label"] == "text_parse_int_or"
                && item["detail"] == "AIL source Text parser"),
        "completion must include AIL source text_parse_int_or helper; got: {text_parse_int_or_items:?}"
    );

    let text_byte_at_or_completion_output = ail()
        .args(["lsp", "--complete", "text_byte_at_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_byte_at_or_completion = parse_json_output(&text_byte_at_or_completion_output);
    let text_byte_at_or_items = text_byte_at_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_byte_at_or_items
            .iter()
            .any(|item| item["label"] == "text_byte_at_or"
                && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_byte_at_or helper; got: {text_byte_at_or_items:?}"
    );

    let text_slice_completion_output = ail()
        .args(["lsp", "--complete", "text_slice", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_slice_completion = parse_json_output(&text_slice_completion_output);
    let text_slice_items = text_slice_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_slice_items
            .iter()
            .any(|item| item["label"] == "text_slice"
                && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_slice helper; got: {text_slice_items:?}"
    );

    let text_replace_first_completion_output = ail()
        .args(["lsp", "--complete", "text_replace_first", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_replace_first_completion = parse_json_output(&text_replace_first_completion_output);
    let text_replace_first_items = text_replace_first_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_replace_first_items
            .iter()
            .any(|item| item["label"] == "text_replace_first"
                && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_replace_first helper; got: {text_replace_first_items:?}"
    );

    let text_starts_with_completion_output = ail()
        .args(["lsp", "--complete", "text_starts_with", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_starts_with_completion = parse_json_output(&text_starts_with_completion_output);
    let text_starts_with_items = text_starts_with_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_starts_with_items
            .iter()
            .any(|item| item["label"] == "text_starts_with"
                && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_starts_with helper; got: {text_starts_with_items:?}"
    );

    let text_ends_with_completion_output = ail()
        .args(["lsp", "--complete", "text_ends_with", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_ends_with_completion = parse_json_output(&text_ends_with_completion_output);
    let text_ends_with_items = text_ends_with_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_ends_with_items
            .iter()
            .any(|item| item["label"] == "text_ends_with"
                && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_ends_with helper; got: {text_ends_with_items:?}"
    );

    let map_completion_output = ail()
        .args(["lsp", "--complete", "ma", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let map_completion = parse_json_output(&map_completion_output);
    let map_items = map_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        map_items
            .iter()
            .any(|item| item["label"] == "map" && item["detail"] == "AIL source Map builtin"),
        "completion must include AIL source map builtin; got: {map_items:?}"
    );

    let set_hover_output = ail()
        .args(["lsp", "--hover-token", "set", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let set_hover = parse_json_output(&set_hover_output);
    assert!(
        set_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Set<T>"),
        "hover must explain source Set builtin; got: {set_hover}"
    );

    let tuple_completion_output = ail()
        .args(["lsp", "--complete", "tu", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let tuple_completion = parse_json_output(&tuple_completion_output);
    let tuple_items = tuple_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        tuple_items
            .iter()
            .any(|item| item["label"] == "tuple" && item["detail"] == "AIL source Tuple builtin"),
        "completion must include AIL source tuple builtin; got: {tuple_items:?}"
    );

    let record_completion_output = ail()
        .args(["lsp", "--complete", "rec", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let record_completion = parse_json_output(&record_completion_output);
    let record_items = record_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        record_items
            .iter()
            .any(|item| item["label"] == "record" && item["detail"] == "AIL source Record builtin"),
        "completion must include AIL source record builtin; got: {record_items:?}"
    );

    let option_completion_output = ail()
        .args(["lsp", "--complete", "Som", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let option_completion = parse_json_output(&option_completion_output);
    let option_items = option_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        option_items.iter().any(
            |item| item["label"] == "Some" && item["detail"] == "AIL source Option constructor"
        ),
        "completion must include AIL source Option constructor; got: {option_items:?}"
    );

    let result_hover_output = ail()
        .args(["lsp", "--hover-token", "Err", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let result_hover = parse_json_output(&result_hover_output);
    assert!(
        result_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Result<T,E> error"),
        "hover must explain source Result constructor; got: {result_hover}"
    );

    let unwrap_or_completion_output = ail()
        .args(["lsp", "--complete", "unwrap", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let unwrap_or_completion = parse_json_output(&unwrap_or_completion_output);
    let unwrap_or_items = unwrap_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        unwrap_or_items.iter().any(
            |item| item["label"] == "unwrap_or" && item["detail"] == "AIL source Option helper"
        ),
        "completion must include AIL source unwrap_or helper; got: {unwrap_or_items:?}"
    );

    let option_predicate_completion_output = ail()
        .args(["lsp", "--complete", "is_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let option_predicate_completion = parse_json_output(&option_predicate_completion_output);
    let option_predicate_items = option_predicate_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        option_predicate_items
            .iter()
            .any(|item| item["label"] == "is_some"
                && item["detail"] == "AIL source Option predicate")
            && option_predicate_items
                .iter()
                .any(|item| item["label"] == "is_none"
                    && item["detail"] == "AIL source Option predicate"),
        "completion must include AIL source Option predicates; got: {option_predicate_items:?}"
    );

    let result_predicate_completion_output = ail()
        .args(["lsp", "--complete", "is_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let result_predicate_completion = parse_json_output(&result_predicate_completion_output);
    let result_predicate_items = result_predicate_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        result_predicate_items
            .iter()
            .any(|item| item["label"] == "is_ok" && item["detail"] == "AIL source Result predicate")
            && result_predicate_items
                .iter()
                .any(|item| item["label"] == "is_err"
                    && item["detail"] == "AIL source Result predicate"),
        "completion must include AIL source Result predicates; got: {result_predicate_items:?}"
    );
}
#[test]
fn lsp_definition_resolves_acl_target_to_id_location() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let acl = dir.child("defs.acl");
    acl.write_str(
        r#"change defs
author cli-test
description definition lookup
base 0
op create_function id=fn.main return=Int body=add(20, 22)
op grant target=fn.main capability=log.write
end
"#,
    )
    .expect("ACL fixture must be written");

    let output = ail()
        .args(["lsp", "--definition-token", "fn.main", "--definition-file"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "fn.main");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 4);
    assert!(
        v["data"]["definition"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("defs.acl")
    );
}
#[test]
fn lsp_definition_resolves_ail_source_imported_function() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("imported source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str("use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n")
        .expect("main source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--definition-token",
            "math.add_pair",
            "--definition-file",
        ])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "math.add_pair");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 1);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 3);
    assert!(
        v["data"]["definition"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("math.ail")
    );
}
#[test]
fn lsp_definition_resolves_ail_source_capability() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write\n\
grant main log.write\n\
fn main() -> Int = effect_call(log.write, write, \"hi\")\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--definition-token",
            "log.write",
            "--definition-file",
        ])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "log.write");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 0);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 11);
    assert!(
        v["data"]["definition"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("main.ail")
    );
}
#[test]
fn lsp_definition_resolves_ail_source_const() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("const answer: Int = 42\nfn main() -> Int = answer\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--definition-token", "answer", "--definition-file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "answer");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 0);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 6);
}
#[test]
fn lsp_definition_resolves_ail_source_test() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("test smoke = eq(add(20, 22), 42)\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--definition-token",
            "test.smoke",
            "--definition-file",
        ])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "test.smoke");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 0);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 5);
    assert!(
        v["data"]["definition"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("main.ail")
    );
}
#[test]
fn lsp_references_find_same_file_acl_identifier_uses() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let acl = dir.child("refs.acl");
    acl.write_str(
        r#"change refs
author cli-test
description reference lookup
base 0
op create_function id=fn.main return=Int body=add(20, 22)
op grant target=fn.main capability=log.write
op set_body target=fn.main body=add(1, 2)
end
"#,
    )
    .expect("ACL fixture must be written");

    let output = ail()
        .args(["lsp", "--references-token", "fn.main", "--references-file"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "fn.main");
    assert_eq!(v["data"]["reference_count"], 3);
    let refs = v["data"]["references"]
        .as_array()
        .expect("references must be an array");
    assert_eq!(refs[0]["range"]["start"]["line"], 4);
    assert_eq!(refs[1]["range"]["start"]["line"], 5);
    assert_eq!(refs[2]["range"]["start"]["line"], 6);
}
#[test]
fn lsp_references_resolve_ail_source_imported_function_uses() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("imported source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str(
        "use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\ntest add = eq(math.add_pair(1, 2), 3)\n",
    )
    .expect("main source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--references-token",
            "math.add_pair",
            "--references-file",
        ])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    let refs = v["data"]["references"]
        .as_array()
        .expect("references must be an array");

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "math.add_pair");
    assert_eq!(v["data"]["reference_count"], 3);
    assert_eq!(refs[0]["range"]["start"]["line"], 1);
    assert_eq!(refs[1]["range"]["start"]["line"], 2);
    assert!(
        refs[2]["uri"]
            .as_str()
            .expect("definition reference uri")
            .ends_with("math.ail")
    );
    assert_eq!(refs[2]["range"]["start"]["line"], 1);
}
#[test]
fn lsp_references_resolve_ail_source_prefixed_test_uses() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test smoke = eq(add(20, 22), 42)\ngrant test.smoke log.write\nfn main() -> Int = 0\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--references-token",
            "test.smoke",
            "--references-file",
        ])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    let refs = v["data"]["references"]
        .as_array()
        .expect("references must be an array");

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "test.smoke");
    assert_eq!(v["data"]["reference_count"], 2);
    assert_eq!(refs[0]["range"]["start"]["line"], 0);
    assert_eq!(refs[0]["range"]["start"]["character"], 5);
    assert_eq!(refs[1]["range"]["start"]["line"], 1);
    assert_eq!(refs[1]["range"]["start"]["character"], 6);
}
