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
        "id": 34,
        "method": "ail/renameEdits",
        "params": {
            "textDocument": { "uri": uri.clone() },
            "position": { "line": 1, "character": 4 },
            "newName": "math.sum_pair"
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
        .find(|message| message["id"] == 34)
        .expect("renameEdits response must be emitted");

    assert_eq!(response["result"]["canRename"], false);
    assert_eq!(response["result"]["reason"], "invalid_new_name");
}
