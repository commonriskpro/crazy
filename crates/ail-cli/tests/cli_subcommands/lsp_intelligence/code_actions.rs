use crate::common::ail;

#[test]
fn lsp_initialize_advertises_quickfix_code_actions() {
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    .to_string();
    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_input(&[initialize]))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == 1)
        .expect("initialize response must be emitted");

    assert_eq!(
        response["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
        serde_json::json!(["quickfix"])
    );
}

#[test]
fn lsp_code_action_reports_missing_document_with_redacted_diagnostic() {
    let uri = "file:///private/customer_secret.ail";
    let code_action =
        code_action_message(uri, 20, vec![diagnostic_with_code("AIL_SOURCE_PARSER", 2)]);

    let result = code_action_result(vec![code_action], 20);
    let failure = &result[0]["data"]["diagnostic"];

    assert_eq!(failure["code"], "AIL_CODE_ACTION_MISSING_DOCUMENT");
    assert_eq!(failure["category"], "document_state");
    assert_eq!(failure["descriptor"]["documentState"], "not_open");
    assert!(!failure.to_string().contains("customer_secret"));
}

#[test]
fn lsp_code_action_reports_unsupported_diagnostic_code_without_echoing_sensitive_code() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "module main\n");
    let mut diagnostic = diagnostic_with_code("AIL_SOURCE_PARSE_SECRET_CUSTOMER", 3);
    diagnostic["source"] = serde_json::json!("/tmp/customer.ail");
    let code_action = code_action_message(uri, 21, vec![diagnostic]);

    let result = code_action_result(vec![open, code_action], 21);
    let failure = &result[0]["data"]["diagnostic"];

    assert_eq!(failure["code"], "AIL_CODE_ACTION_UNSUPPORTED_DIAGNOSTIC");
    assert_eq!(failure["reason"], "unsupported_diagnostic_code");
    assert_eq!(failure["descriptor"]["diagnosticCode"], "unsupported");
    assert!(!failure.to_string().contains("customer.ail"));
    assert!(!failure.to_string().contains("SECRET_CUSTOMER"));
}

#[test]
fn lsp_code_action_reports_supported_diagnostic_with_no_repair() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "module main\n");
    let code_action =
        code_action_message(uri, 22, vec![diagnostic_with_code("AIL_SOURCE_PARSER", 4)]);

    let result = code_action_result(vec![open, code_action], 22);
    let failure = &result[0]["data"]["diagnostic"];

    assert_eq!(failure["code"], "AIL_CODE_ACTION_NO_REPAIR_AVAILABLE");
    assert_eq!(failure["reason"], "no_repair_available");
    assert_eq!(failure["descriptor"]["diagnosticCode"], "AIL_SOURCE_PARSER");
}

#[test]
fn lsp_code_action_accepts_specific_source_diagnostic_codes() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "module main\n");
    let code_action = code_action_message(
        uri,
        24,
        vec![diagnostic_with_code("AIL_SOURCE_PARSE_INVALID_NAME", 4)],
    );

    let result = code_action_result(vec![open, code_action], 24);
    let failure = &result[0]["data"]["diagnostic"];

    assert_eq!(failure["code"], "AIL_CODE_ACTION_NO_REPAIR_AVAILABLE");
    assert_eq!(
        failure["descriptor"]["diagnosticCode"],
        "AIL_SOURCE_PARSE_INVALID_NAME"
    );
}

#[test]
fn lsp_code_action_reports_ambiguous_repair_edits() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "module main\n");
    let mut diagnostic = diagnostic_with_code("AIL_SOURCE_PARSER", 5);
    diagnostic["data"] = serde_json::json!({
        "ailRepair": {
            "code": "replace.module",
            "edits": [workspace_edit(uri, "module one\n"), workspace_edit(uri, "module two\n")]
        }
    });
    let code_action = code_action_message(uri, 23, vec![diagnostic]);

    let result = code_action_result(vec![open, code_action], 23);
    let failure = &result[0]["data"]["diagnostic"];

    assert_eq!(failure["code"], "AIL_CODE_ACTION_AMBIGUOUS_REPAIR_EDIT");
    assert_eq!(failure["reason"], "ambiguous_repair_edit");
    assert_eq!(failure["descriptor"]["repairEditCount"], 2);
}

#[test]
fn lsp_code_actions_are_deterministically_ordered_and_emit_single_repairs() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "module main\n");
    let mut later = diagnostic_with_code("AIL_SOURCE_PARSER", 10);
    later["data"] = serde_json::json!({
        "ailRepair": { "code": "replace.later", "edit": workspace_edit(uri, "module later\n") }
    });
    let mut earlier = diagnostic_with_code("AIL_SOURCE_PARSER", 1);
    earlier["data"] = serde_json::json!({
        "ailRepair": { "code": "replace.earlier", "edit": workspace_edit(uri, "module earlier\n") }
    });
    let code_action = code_action_message(uri, 24, vec![later, earlier]);

    let result = code_action_result(vec![open, code_action], 24);
    let repair_codes = result
        .as_array()
        .expect("code actions must be an array")
        .iter()
        .map(|action| action["data"]["repairCode"].as_str().expect("repair code"))
        .collect::<Vec<_>>();

    assert_eq!(repair_codes, ["replace.earlier", "replace.later"]);
    assert_eq!(
        result[0]["title"],
        "AIL: apply replace.earlier for AIL_SOURCE_PARSER"
    );
    assert_eq!(
        result[1]["title"],
        "AIL: apply replace.later for AIL_SOURCE_PARSER"
    );
    assert_eq!(
        result[0]["edit"]["changes"][uri][0]["newText"],
        "module earlier\n"
    );
    assert_eq!(
        result[1]["edit"]["changes"][uri][0]["newText"],
        "module later\n"
    );
}

#[test]
fn lsp_code_action_removes_ignored_expression_statement() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "fn main() -> Int {\n  1 + 2\n  return 0\n}\n");
    let code_action = code_action_message(uri, 25, vec![ignored_expression_diagnostic(uri)]);

    let result = code_action_result(vec![open, code_action], 25);

    assert_eq!(result[0]["kind"], "quickfix");
    assert_eq!(
        result[0]["title"],
        "AIL: remove ignored expression statement"
    );
    assert_eq!(
        result[0]["data"]["diagnosticCode"],
        "AIL_SOURCE_LSP_IGNORED_EXPRESSION"
    );
    assert_eq!(
        result[0]["data"]["repairCode"],
        "remove.ignored_expression_statement"
    );
    assert_eq!(result[0]["edit"]["changes"][uri][0]["newText"], "");
    assert_eq!(
        result[0]["edit"]["changes"][uri][0]["range"]["start"]["line"],
        1
    );
    assert_eq!(
        result[0]["edit"]["changes"][uri][0]["range"]["end"]["line"],
        2
    );
}

#[test]
fn lsp_code_action_prefixes_unused_binding_with_underscore() {
    let uri = "file:///workspace/main.ail";
    let open = did_open_message(uri, "fn main() -> Int {\n  let unused = 1\n  return 0\n}\n");
    let code_action = code_action_message(uri, 26, vec![unused_binding_diagnostic(uri)]);

    let result = code_action_result(vec![open, code_action], 26);

    assert_eq!(result[0]["kind"], "quickfix");
    assert_eq!(result[0]["title"], "AIL: prefix unused binding with `_`");
    assert_eq!(
        result[0]["data"]["diagnosticCode"],
        "AIL_SOURCE_LSP_UNUSED_BINDING"
    );
    assert_eq!(
        result[0]["data"]["repairCode"],
        "prefix.unused_binding_with_underscore"
    );
    assert_eq!(result[0]["edit"]["changes"][uri][0]["newText"], "_");
    assert_eq!(
        result[0]["edit"]["changes"][uri][0]["range"]["start"]["line"],
        1
    );
    assert_eq!(
        result[0]["edit"]["changes"][uri][0]["range"]["start"]["character"],
        6
    );
}

#[test]
fn lsp_publish_diagnostics_handles_unsaved_source_documents() {
    let uri = "file:///workspace/unsaved.ail";
    let open = did_open_message(uri, "fn main() -> Int {\n  1 + 2\n  return 0\n}\n");

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_input(&[open]))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let notification = messages
        .iter()
        .find(|message| message["method"] == "textDocument/publishDiagnostics")
        .expect("publishDiagnostics notification must be emitted");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "AIL_SOURCE_LSP_IGNORED_EXPRESSION");
    assert_eq!(diagnostics[0]["severity"], 2);
    assert_eq!(
        diagnostics[0]["data"]["ailRepair"]["code"],
        "remove.ignored_expression_statement"
    );
    assert!(
        !diagnostics[0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("failed to resolve AIL source"),
        "unsaved source diagnostics must not require canonical file resolution"
    );
}

#[test]
fn lsp_did_change_uses_utf16_range_offsets() {
    let uri = "file:///workspace/utf16-incremental.ail";
    let open = did_open_message(uri, "fn main() -> Int {\n  \"🔥\" ++ x\n  return 0\n}\n");
    let change = did_change_message(
        uri,
        serde_json::json!([{
            "range": {
                "start": { "line": 1, "character": 10 },
                "end": { "line": 1, "character": 11 }
            },
            "text": "\"x\""
        }]),
    );

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_input(&[open, change]))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let notification = messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .last()
        .expect("didChange must publish diagnostics for UTF-16 ranged text");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "AIL_SOURCE_LSP_IGNORED_EXPRESSION");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 1);
    assert_eq!(
        diagnostics[0]["data"]["ailRepair"]["code"],
        "remove.ignored_expression_statement"
    );
}

#[test]
fn lsp_did_change_applies_incremental_source_document_edits() {
    let uri = "file:///workspace/incremental.ail";
    let open = did_open_message(uri, "fn main() -> Int {\n  return 0\n}\n");
    let change = did_change_message(
        uri,
        serde_json::json!([{
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 0 }
            },
            "text": "  1 + 2\n"
        }]),
    );

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_input(&[open, change]))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let notification = messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .last()
        .expect("didChange must publish diagnostics for incrementally updated text");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "AIL_SOURCE_LSP_IGNORED_EXPRESSION");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 1);
    assert_eq!(
        diagnostics[0]["data"]["ailRepair"]["code"],
        "remove.ignored_expression_statement"
    );
}

fn code_action_result(messages: Vec<String>, request_id: u64) -> serde_json::Value {
    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_input(&messages))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == request_id)
        .expect("codeAction response must be emitted");
    response["result"].clone()
}

fn did_change_message(uri: &str, content_changes: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": uri,
                "version": 2,
            },
            "contentChanges": content_changes
        }
    })
    .to_string()
}

fn ignored_expression_diagnostic(uri: &str) -> serde_json::Value {
    serde_json::json!({
        "range": {
            "start": { "line": 1, "character": 2 },
            "end": { "line": 1, "character": 7 }
        },
        "severity": 2,
        "source": "ail-source-lint",
        "code": "AIL_SOURCE_LSP_IGNORED_EXPRESSION",
        "message": "ignored expression statement has no direct effect",
        "data": {
            "ailRepair": {
                "code": "remove.ignored_expression_statement",
                "edit": delete_line_edit(uri)
            }
        }
    })
}

fn unused_binding_diagnostic(uri: &str) -> serde_json::Value {
    serde_json::json!({
        "range": {
            "start": { "line": 1, "character": 2 },
            "end": { "line": 1, "character": 16 }
        },
        "severity": 2,
        "source": "ail-source-lint",
        "code": "AIL_SOURCE_LSP_UNUSED_BINDING",
        "message": "unused local binding `unused`",
        "data": {
            "ailRepair": {
                "code": "prefix.unused_binding_with_underscore",
                "edit": insert_underscore_edit(uri)
            }
        }
    })
}

fn insert_underscore_edit(uri: &str) -> serde_json::Value {
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri.to_string(),
        serde_json::json!([{
            "range": {
                "start": { "line": 1, "character": 6 },
                "end": { "line": 1, "character": 6 }
            },
            "newText": "_"
        }]),
    );
    serde_json::json!({ "changes": changes })
}

fn delete_line_edit(uri: &str) -> serde_json::Value {
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri.to_string(),
        serde_json::json!([{
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 2, "character": 0 }
            },
            "newText": ""
        }]),
    );
    serde_json::json!({ "changes": changes })
}

fn did_open_message(uri: &str, text: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ail",
                "version": 1,
                "text": text,
            }
        }
    })
    .to_string()
}

fn code_action_message(uri: &str, request_id: u64, diagnostics: Vec<serde_json::Value>) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 0 }
            },
            "context": { "diagnostics": diagnostics }
        }
    })
    .to_string()
}

fn diagnostic_with_code(code: &str, character: u64) -> serde_json::Value {
    serde_json::json!({
        "range": {
            "start": { "line": 0, "character": character },
            "end": { "line": 0, "character": character + 1 }
        },
        "severity": 1,
        "source": "ail-source-parser",
        "code": code,
        "message": "redacted by code-action failure diagnostics"
    })
}

fn workspace_edit(uri: &str, new_text: &str) -> serde_json::Value {
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri.to_string(),
        serde_json::json!([{
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 0 }
            },
            "newText": new_text
        }]),
    );
    serde_json::json!({ "changes": changes })
}

fn lsp_input(messages: &[String]) -> String {
    messages
        .iter()
        .map(|message| format!("Content-Length: {}\r\n\r\n{}", message.len(), message))
        .collect::<Vec<_>>()
        .join("")
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
