use crate::common::ail;

#[test]
fn lsp_rename_candidates_reports_symbol_kind_and_open_document_references() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    let main = dir.child("main.ail");
    math.write_str("module math\nfn stale() -> Int = 0\n")
        .expect("stale math fixture must be written");
    main.write_str("use \"./math.ail\"\nfn main() -> Int = math.stale()\n")
        .expect("stale main fixture must be written");

    let math_uri = format!("file://{}", math.path().display());
    let main_uri = format!("file://{}", main.path().display());
    let main_text = "use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n";
    let math_text = "module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n";
    let open_main = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": main_uri.clone(),
                "languageId": "ail",
                "version": 2,
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
                "uri": math_uri.clone(),
                "languageId": "ail",
                "version": 2,
                "text": math_text,
            }
        }
    })
    .to_string();
    let candidates = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "ail/renameCandidates",
        "params": {
            "textDocument": { "uri": main_uri.clone() },
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
        candidates.len(),
        candidates
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
        .find(|message| message["id"] == 31)
        .expect("rename candidates response must be emitted");
    let candidate = &response["result"];
    let references = candidate["references"]
        .as_array()
        .expect("rename references must be an array");

    assert_eq!(candidate["canRename"], true);
    assert_eq!(candidate["token"], "math.add_pair");
    assert_eq!(candidate["referenceToken"], "math.add_pair");
    assert_eq!(candidate["symbolKind"], 12);
    assert_eq!(candidate["range"]["start"]["line"], 1);
    assert_eq!(candidate["range"]["start"]["character"], 19);
    assert_eq!(candidate["range"]["end"]["character"], 32);
    assert_eq!(candidate["referenceCount"], 2);
    let reference_uris = candidate["referenceUris"]
        .as_array()
        .expect("rename reference uris must be an array");
    assert_eq!(reference_uris.len(), 2);
    assert!(
        reference_uris[0]
            .as_str()
            .expect("first rename reference uri")
            .ends_with("main.ail")
    );
    assert!(
        reference_uris[1]
            .as_str()
            .expect("second rename reference uri")
            .ends_with("math.ail")
    );
    assert_eq!(references.len(), 2);
    assert!(references.iter().any(|reference| {
        reference["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("main.ail"))
            && reference["range"]["start"]["line"] == 1
            && reference["range"]["start"]["character"] == 19
    }));
    assert!(references.iter().any(|reference| {
        reference["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("math.ail"))
            && reference["range"]["start"]["line"] == 1
            && reference["range"]["start"]["character"] == 3
    }));
}

#[test]
fn lsp_prepare_rename_returns_standard_range_placeholder() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("math.ail");
    let uri = format!("file://{}", source.path().display());
    source
        .write_str("module math\nfn stale() -> Int = 0\n")
        .expect("source fixture must be written");
    let open_source = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri.clone(),
                "languageId": "ail",
                "version": 1,
                "text": "module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n",
            }
        }
    })
    .to_string();
    let prepare_rename = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "textDocument/prepareRename",
        "params": {
            "textDocument": { "uri": uri.clone() },
            "position": { "line": 1, "character": 4 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_source.len(),
        open_source,
        prepare_rename.len(),
        prepare_rename
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
        .find(|message| message["id"] == 32)
        .expect("prepareRename response must be emitted");

    assert_eq!(response["result"]["placeholder"], "add_pair");
    assert_eq!(response["result"]["range"]["start"]["line"], 1);
    assert_eq!(response["result"]["range"]["start"]["character"], 3);
    assert_eq!(response["result"]["range"]["end"]["character"], 11);
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

fn lsp_input(messages: Vec<String>) -> String {
    messages
        .into_iter()
        .map(|message| format!("Content-Length: {}\r\n\r\n{}", message.len(), message))
        .collect::<Vec<_>>()
        .join("")
}

fn rename_candidates_result_for_documents(
    documents: Vec<(&str, &str)>,
    target_document: &str,
    line: u64,
    character: u64,
    request_id: u64,
) -> serde_json::Value {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let mut messages = Vec::new();
    let mut target_uri = None;
    for (index, (name, text)) in documents.into_iter().enumerate() {
        let file = dir.child(name);
        file.write_str(text)
            .expect("source fixture must be written");
        let uri = format!("file://{}", file.path().display());
        if name == target_document {
            target_uri = Some(uri.clone());
        }
        messages.push(did_open_message(&uri, text, index as u64 + 1));
    }
    let target_uri = target_uri.expect("target document must be in fixture set");
    messages.push(
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "ail/renameCandidates",
            "params": {
                "textDocument": { "uri": target_uri },
                "position": { "line": line, "character": character }
            }
        })
        .to_string(),
    );
    let input = lsp_input(messages);

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
        .find(|message| message["id"] == request_id)
        .expect("renameCandidates response must be emitted");
    response["result"].clone()
}

#[test]
fn lsp_text_document_rename_returns_workspace_edit_for_open_documents() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    let main = dir.child("main.ail");
    math.write_str("module math\nfn stale() -> Int = 0\n")
        .expect("stale math fixture must be written");
    main.write_str("use \"./math.ail\"\nfn main() -> Int = math.stale()\n")
        .expect("stale main fixture must be written");

    let math_uri = format!("file://{}", math.path().display());
    let main_uri = format!("file://{}", main.path().display());
    let main_text = "use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n";
    let math_text = "module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n";
    let open_main = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": main_uri.clone(),
                "languageId": "ail",
                "version": 2,
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
                "uri": math_uri.clone(),
                "languageId": "ail",
                "version": 2,
                "text": math_text,
            }
        }
    })
    .to_string();
    let rename = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 33,
        "method": "textDocument/rename",
        "params": {
            "textDocument": { "uri": main_uri.clone() },
            "position": { "line": 1, "character": 24 },
            "newName": "sum_pair"
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_main.len(),
        open_main,
        open_math.len(),
        open_math,
        rename.len(),
        rename
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
        .find(|message| message["id"] == 33)
        .expect("rename response must be emitted");
    let changes = response["result"]["changes"]
        .as_object()
        .expect("rename must return workspace changes");
    let main_edits = changes
        .get(&main_uri)
        .and_then(|value| value.as_array())
        .expect("main document must receive an edit");
    let math_edits = changes
        .get(&math_uri)
        .and_then(|value| value.as_array())
        .expect("math document must receive an edit");

    assert_eq!(changes.len(), 2);
    assert_eq!(main_edits.len(), 1);
    assert_eq!(main_edits[0]["newText"], "math.sum_pair");
    assert_eq!(main_edits[0]["range"]["start"]["line"], 1);
    assert_eq!(main_edits[0]["range"]["start"]["character"], 19);
    assert_eq!(main_edits[0]["range"]["end"]["character"], 32);
    assert_eq!(math_edits.len(), 1);
    assert_eq!(math_edits[0]["newText"], "sum_pair");
    assert_eq!(math_edits[0]["range"]["start"]["line"], 1);
    assert_eq!(math_edits[0]["range"]["start"]["character"], 3);
    assert_eq!(math_edits[0]["range"]["end"]["character"], 11);
}

#[test]
fn lsp_rename_edits_rejects_qualified_new_names() {
    let result = rename_edits_result_for_new_name("math.sum_pair", 34);

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "qualified_new_name");
}

#[test]
fn lsp_rename_edits_rejects_invalid_identifiers() {
    let result = rename_edits_result_for_new_name("1sum_pair", 35);

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "invalid_identifier");
}

#[test]
fn lsp_rename_edits_rejects_reserved_keywords() {
    let result = rename_edits_result_for_new_name("fn", 36);

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "reserved_keyword");
}

#[test]
fn lsp_rename_edits_rejects_same_name_no_ops() {
    let result = rename_edits_result_for_new_name("add_pair", 37);

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "same_name");
    assert_eq!(result["diagnostic"]["code"], "AIL_RENAME_SAME_NAME");
    assert_eq!(result["diagnostic"]["category"], "invalid_new_name");
}

#[test]
fn lsp_rename_edits_reports_redacted_invalid_new_name_diagnostic() {
    let result = rename_edits_result_for_new_name("customer.secret", 38);
    let diagnostic = &result["diagnostic"];

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "qualified_new_name");
    assert_eq!(diagnostic["code"], "AIL_RENAME_QUALIFIED_NEW_NAME");
    assert_eq!(diagnostic["category"], "invalid_new_name");
    assert_eq!(diagnostic["descriptor"]["containsQualifier"], true);
    assert!(!diagnostic.to_string().contains("customer.secret"));
}

#[test]
fn lsp_rename_candidates_reports_unresolved_symbol_diagnostic() {
    let source = "module math\nfn main() -> Int = customer_private_value()\n";
    let result =
        rename_candidates_result_for_documents(vec![("main.ail", source)], "main.ail", 1, 21, 39);
    let diagnostic = &result["diagnostic"];

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "unresolved_symbol");
    assert_eq!(diagnostic["code"], "AIL_RENAME_UNRESOLVED_SYMBOL");
    assert_eq!(diagnostic["category"], "symbol_resolution");
    assert_eq!(diagnostic["descriptor"]["token"]["tokenLength"], 22);
    assert!(!diagnostic.to_string().contains("customer_private_value"));
}

#[test]
fn lsp_rename_candidates_reports_ambiguous_symbol_diagnostic() {
    let alpha = "module alpha\nfn add_pair(x: Int, y: Int) -> Int = x + y\n";
    let beta = "module beta\nfn add_pair(x: Int, y: Int) -> Int = x + y\n";
    let result = rename_candidates_result_for_documents(
        vec![("alpha.ail", alpha), ("beta.ail", beta)],
        "alpha.ail",
        1,
        4,
        40,
    );

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "ambiguous_symbol");
    assert_eq!(result["diagnostic"]["code"], "AIL_RENAME_AMBIGUOUS_SYMBOL");
    assert_eq!(result["diagnostic"]["category"], "symbol_resolution");
    assert_eq!(result["diagnostic"]["descriptor"]["candidateCount"], 2);
    assert_eq!(
        result["diagnostic"]["descriptor"]["symbolKinds"],
        serde_json::json!([12])
    );
    assert!(!result["diagnostic"].to_string().contains("alpha.add_pair"));
    assert!(!result["diagnostic"].to_string().contains("beta.add_pair"));
}

#[test]
fn lsp_rename_edits_reports_unopened_import_references_as_unsupported() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    let consumer = dir.child("consumer.ail");
    let math_text =
        "module math\nuse \"./consumer.ail\"\nfn add_pair(x: Int, y: Int) -> Int = x + y\n";
    let consumer_text = "use \"./math.ail\"\nfn main() -> Int = math.add_pair(1, 2)\n";
    math.write_str(math_text)
        .expect("math fixture must be written");
    consumer
        .write_str(consumer_text)
        .expect("consumer fixture must be written");

    let math_uri = format!("file://{}", math.path().display());
    let open_math = did_open_message(&math_uri, math_text, 1);
    let rename_edits = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "ail/renameEdits",
        "params": {
            "textDocument": { "uri": math_uri.clone() },
            "position": { "line": 2, "character": 4 },
            "newName": "sum_pair"
        }
    })
    .to_string();
    let input = lsp_input(vec![open_math, rename_edits]);

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
        .find(|message| message["id"] == 41)
        .expect("renameEdits response must be emitted");
    let result = &response["result"];

    assert_eq!(result["canRename"], false);
    assert_eq!(result["reason"], "cross_file_import_unsupported");
    assert_eq!(
        result["diagnostic"]["code"],
        "AIL_RENAME_CROSS_FILE_IMPORT_UNSUPPORTED"
    );
    assert_eq!(result["diagnostic"]["category"], "unsupported");
    assert_eq!(
        result["diagnostic"]["descriptor"]["missingDocumentCount"],
        1
    );
    assert!(!result["diagnostic"].to_string().contains("consumer.ail"));
}

#[test]
fn lsp_rename_edits_orders_document_edits_by_range() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("math.ail");
    let text = concat!(
        "module math\n",
        "fn add_pair(x: Int, y: Int) -> Int = x + y\n",
        "fn first() -> Int = add_pair(1, 2)\n",
        "fn second() -> Int = math.add_pair(3, 4)\n",
    );
    source
        .write_str(text)
        .expect("source fixture must be written");
    let uri = format!("file://{}", source.path().display());
    let open_source = did_open_message(&uri, text, 1);
    let rename_edits = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "ail/renameEdits",
        "params": {
            "textDocument": { "uri": uri.clone() },
            "position": { "line": 1, "character": 4 },
            "newName": "sum_pair"
        }
    })
    .to_string();
    let input = lsp_input(vec![open_source, rename_edits]);

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
        .find(|message| message["id"] == 42)
        .expect("renameEdits response must be emitted");
    let changes = response["result"]["workspaceEdit"]["changes"]
        .as_object()
        .expect("workspace changes must be returned");
    let edits = changes
        .get(&uri)
        .and_then(|value| value.as_array())
        .expect("source document edits must be returned");

    assert_eq!(edits.len(), 3);
    assert_eq!(edits[0]["range"]["start"]["line"], 1);
    assert_eq!(edits[0]["range"]["start"]["character"], 3);
    assert_eq!(edits[1]["range"]["start"]["line"], 2);
    assert_eq!(edits[1]["range"]["start"]["character"], 20);
    assert_eq!(edits[2]["range"]["start"]["line"], 3);
    assert_eq!(edits[2]["range"]["start"]["character"], 21);
}

#[test]
fn lsp_rename_edits_cover_source_block_tests() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let text = concat!(
        "test smoke {\n",
        "  let actual: Int = 20 + 22\n",
        "  return actual == 42\n",
        "}\n",
        "grant test.smoke log.write\n",
        "fn main() -> Int = 0\n",
    );
    source
        .write_str(text)
        .expect("source fixture must be written");
    let uri = format!("file://{}", source.path().display());
    let open_source = did_open_message(&uri, text, 1);
    let rename_edits = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 43,
        "method": "ail/renameEdits",
        "params": {
            "textDocument": { "uri": uri.clone() },
            "position": { "line": 0, "character": 6 },
            "newName": "addition"
        }
    })
    .to_string();
    let input = lsp_input(vec![open_source, rename_edits]);

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
        .find(|message| message["id"] == 43)
        .expect("renameEdits response must be emitted");
    let result = &response["result"];
    let changes = result["workspaceEdit"]["changes"]
        .as_object()
        .expect("workspace changes must be returned");
    let edits = changes
        .get(&uri)
        .and_then(|value| value.as_array())
        .expect("source document edits must be returned");

    assert_eq!(result["canRename"], true);
    assert_eq!(result["referenceToken"], "test.smoke");
    assert_eq!(result["documentCount"], 1);
    let document_uris = result["documentUris"]
        .as_array()
        .expect("rename document uris must be an array");
    assert_eq!(document_uris.len(), 1);
    assert!(
        document_uris[0]
            .as_str()
            .expect("rename document uri")
            .ends_with("main.ail")
    );
    assert_eq!(result["editCount"], 2);
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0]["newText"], "addition");
    assert_eq!(edits[0]["range"]["start"]["line"], 0);
    assert_eq!(edits[0]["range"]["start"]["character"], 5);
    assert_eq!(edits[1]["newText"], "test.addition");
    assert_eq!(edits[1]["range"]["start"]["line"], 4);
    assert_eq!(edits[1]["range"]["start"]["character"], 6);
}

fn rename_edits_result_for_new_name(new_name: &str, request_id: u64) -> serde_json::Value {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("math.ail");
    let uri = format!("file://{}", source.path().display());
    source
        .write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("source fixture must be written");
    let open_source = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri.clone(),
                "languageId": "ail",
                "version": 1,
                "text": "module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n",
            }
        }
    })
    .to_string();
    let rename_edits = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "ail/renameEdits",
        "params": {
            "textDocument": { "uri": uri.clone() },
            "position": { "line": 1, "character": 4 },
            "newName": new_name
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_source.len(),
        open_source,
        rename_edits.len(),
        rename_edits
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
        .find(|message| message["id"] == request_id)
        .expect("renameEdits response must be emitted");
    response["result"].clone()
}
