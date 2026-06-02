// Mechanical phase 2 split from lsp_intelligence.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

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
fn lsp_definition_uses_utf16_source_definition_ranges() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("module main\n\u{2003}fn helper() -> Int = 1\nfn main() -> Int = helper()\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--definition-token", "helper", "--definition-file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 1);
    assert_eq!(
        v["data"]["definition"]["range"]["start"]["character"], 4,
        "definition range must count the em-space indentation as one UTF-16 unit"
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
    assert_eq!(v["data"]["definition_found"], true);
    assert_eq!(v["data"]["definition_line"], 1);
    assert_eq!(v["data"]["definition_character"], 3);
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 1);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 3);
    assert!(
        v["data"]["definition_uri"]
            .as_str()
            .expect("summary definition uri")
            .ends_with("math.ail")
    );
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
fn lsp_definition_resolves_ail_source_block_test() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test smoke {\n  let actual: Int = 20 + 22\n  return actual == 42\n}\nfn main() -> Int = 0\n",
        )
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
}

#[test]
fn lsp_definition_resolves_kind_qualified_source_test_without_function_collision() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn smoke() -> Int = 0\n\
test smoke = eq(smoke(), 0)\n\
fn main() -> Int = smoke()\n",
        )
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
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 1);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 5);
}

#[test]
fn lsp_definition_returns_empty_result_for_ambiguous_imported_source_symbols() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let left = dir.child("left.ail");
    left.write_str("module left\nfn helper() -> Int = 1\n")
        .expect("left source fixture must be written");
    let right = dir.child("right.ail");
    right
        .write_str("module right\nfn helper() -> Int = 2\n")
        .expect("right source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str(
        "use \"./left.ail\"\n\
use \"./right.ail\"\n\
fn main() -> Int = helper()\n",
    )
    .expect("main source fixture must be written");

    let output = ail()
        .args(["lsp", "--definition-token", "helper", "--definition-file"])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "helper");
    assert_eq!(v["data"]["definition_found"], false);
    assert_eq!(v["data"]["definition_uri"], serde_json::Value::Null);
    assert_eq!(v["data"]["definition_line"], serde_json::Value::Null);
    assert_eq!(v["data"]["definition_character"], serde_json::Value::Null);
    assert_eq!(
        v["data"]["definition"]
            .as_array()
            .expect("ambiguous definition result must be an empty array")
            .len(),
        0
    );
}

#[test]
fn lsp_stdio_definition_uses_open_workspace_import_text() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn stale() -> Int = 0\n")
        .expect("stale imported source fixture must be written");
    let main = dir.child("main.ail");
    let main_text = "use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n";
    main.write_str(main_text)
        .expect("main source fixture must be written");
    let math_uri = format!("file://{}", math.path().display());
    let main_uri = format!("file://{}", main.path().display());
    let open_main = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": main_uri,
                "languageId": "ail",
                "version": 1,
                "text": main_text,
            }
        }
    })
    .to_string();
    let open_math = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": math_uri,
                "languageId": "ail",
                "version": 2,
                "text": "module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n",
            }
        }
    })
    .to_string();
    let definition = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", main.path().display()) },
            "position": { "line": 1, "character": 24 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_main.len(),
        open_main,
        open_math.len(),
        open_math,
        definition.len(),
        definition
    );

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == 7)
        .expect("definition response must be emitted");

    assert!(
        response["result"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("math.ail")
    );
    assert_eq!(response["result"]["range"]["start"]["line"], 1);
    assert_eq!(response["result"]["range"]["start"]["character"], 3);
}

fn lsp_json_messages(stdout: &[u8]) -> Vec<serde_json::Value> {
    let text = std::str::from_utf8(stdout).expect("LSP stdout must be UTF-8");
    let mut messages = Vec::new();
    let mut rest = text;
    while let Some(header_start) = rest.find("Content-Length: ") {
        rest = &rest[header_start + "Content-Length: ".len()..];
        let Some((len, after_len)) = rest.split_once("\r\n\r\n") else {
            break;
        };
        let len = len
            .trim()
            .parse::<usize>()
            .expect("Content-Length must be numeric");
        let body = &after_len[..len];
        messages.push(serde_json::from_str(body).expect("LSP body must be JSON"));
        rest = &after_len[len..];
    }
    messages
}

#[test]
fn lsp_definition_diagnostics_reports_missing_document_redacted() {
    let uri = "file:///private/customer_secret.ail";
    let result =
        definition_diagnostic_result(vec![definition_diagnostic_message(uri, 41, 0, 0)], 41);

    assert_eq!(result["ok"], false);
    assert_eq!(result["diagnosticCount"], 1);
    assert_eq!(
        result["diagnostics"][0]["code"],
        "AIL_DEFINITION_MISSING_DOCUMENT"
    );
    assert_eq!(result["diagnostics"][0]["category"], "document_state");
    assert_eq!(
        result["diagnostics"][0]["descriptor"]["documentState"],
        "not_open"
    );
    assert!(!result.to_string().contains("customer_secret"));
}

#[test]
fn lsp_definition_diagnostics_reports_unresolved_symbol_redacted() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(
        uri,
        "module main\nfn main() -> Int = customer_private_value\n",
        1,
    );
    let result = definition_diagnostic_result(
        vec![open, definition_diagnostic_message(uri, 42, 1, 22)],
        42,
    );

    assert_eq!(result["ok"], false);
    assert_eq!(
        result["diagnostics"][0]["code"],
        "AIL_DEFINITION_UNRESOLVED_SYMBOL"
    );
    assert_eq!(result["diagnostics"][0]["reason"], "unresolved_symbol");
    assert_eq!(
        result["diagnostics"][0]["descriptor"]["token"]["tokenLength"],
        22
    );
    assert!(!result.to_string().contains("customer_private_value"));
}

#[test]
fn lsp_definition_diagnostics_reports_ambiguous_symbol_redacted() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(
        uri,
        "module main\nfn helper() -> Int = 1\nfn helper() -> Int = 2\nfn main() -> Int = helper()\n",
        1,
    );
    let result = definition_diagnostic_result(
        vec![open, definition_diagnostic_message(uri, 43, 3, 20)],
        43,
    );

    assert_eq!(result["ok"], false);
    assert_eq!(
        result["diagnostics"][0]["code"],
        "AIL_DEFINITION_AMBIGUOUS_SYMBOL"
    );
    assert_eq!(result["diagnostics"][0]["reason"], "ambiguous_symbol");
    assert_eq!(result["diagnostics"][0]["descriptor"]["candidateCount"], 2);
    assert_eq!(
        result["diagnostics"][0]["descriptor"]["candidateLocationsRedacted"],
        true
    );
    assert!(!result.to_string().contains("helper()"));
}

#[test]
fn lsp_definition_diagnostics_reports_unsupported_target_redacted() {
    let uri = "file:///workspace/customer_secret.acl";
    let open = did_open_message(
        uri,
        "op grant target=customer.secret capability=log.write\n",
        1,
    );
    let result = definition_diagnostic_result(
        vec![open, definition_diagnostic_message(uri, 44, 0, 17)],
        44,
    );

    assert_eq!(result["ok"], false);
    assert_eq!(
        result["diagnostics"][0]["code"],
        "AIL_DEFINITION_UNSUPPORTED_TARGET"
    );
    assert_eq!(result["diagnostics"][0]["category"], "unsupported");
    assert_eq!(
        result["diagnostics"][0]["descriptor"]["documentUriRedacted"],
        true
    );
    assert!(!result.to_string().contains("customer_secret"));
    assert!(!result.to_string().contains("customer.secret"));
}

#[test]
fn lsp_definition_diagnostics_reports_unsupported_import_with_stable_order_redacted() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let text =
        "module main\nuse \"./customer_secret.ail\"\nfn main() -> Int = customer_private_value\n";
    source
        .write_str(text)
        .expect("source fixture must be written");
    let uri = format!("file://{}", source.path().display());
    let open = did_open_message(&uri, text, 1);
    let result = definition_diagnostic_result(
        vec![open, definition_diagnostic_message(&uri, 45, 2, 22)],
        45,
    );
    let diagnostics = result["diagnostics"].as_array().expect("diagnostics");
    let codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().expect("diagnostic code"))
        .collect::<Vec<_>>();

    assert_eq!(
        codes,
        vec![
            "AIL_DEFINITION_UNRESOLVED_SYMBOL",
            "AIL_DEFINITION_UNSUPPORTED_IMPORT"
        ]
    );
    assert_eq!(result["ok"], false);
    assert_eq!(result["diagnosticCount"], 2);
    assert_eq!(diagnostics[1]["reason"], "unsupported_import");
    assert_eq!(
        diagnostics[1]["descriptor"]["importState"],
        "unresolved_import"
    );
    assert_eq!(diagnostics[1]["descriptor"]["importPathRedacted"], true);
    assert!(!result.to_string().contains("customer_secret"));
    assert!(!result.to_string().contains("customer_private_value"));
}

#[test]
fn lsp_definition_diagnostics_preserves_successful_definition_result() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(
        uri,
        "module main\nfn add_pair(x: Int, y: Int) -> Int = x + y\nfn main() -> Int = add_pair(20, 22)\n",
        1,
    );
    let result = definition_diagnostic_result(
        vec![open, definition_diagnostic_message(uri, 46, 2, 20)],
        46,
    );

    assert_eq!(result["ok"], true);
    assert_eq!(result["diagnosticCount"], 0);
    assert_eq!(result["definition"]["range"]["start"]["line"], 1);
    assert_eq!(result["definition"]["range"]["start"]["character"], 3);
}

fn did_open_message(uri: &str, text: &str, version: u64) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ail",
                "version": version,
                "text": text,
            }
        }
    })
    .to_string()
}

fn definition_diagnostic_message(uri: &str, request_id: u64, line: u64, character: u64) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "ail/definition",
        "params": {
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        }
    })
    .to_string()
}

fn definition_diagnostic_result(messages: Vec<String>, request_id: u64) -> serde_json::Value {
    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_input(messages))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == request_id)
        .expect("ail/definition response must be emitted");
    response["result"].clone()
}

fn lsp_input(messages: Vec<String>) -> String {
    messages
        .into_iter()
        .map(|message| format!("Content-Length: {}\r\n\r\n{}", message.len(), message))
        .collect::<Vec<_>>()
        .join("")
}
