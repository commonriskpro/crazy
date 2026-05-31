use crate::common::ail;

#[test]
fn lsp_workspace_symbols_indexes_open_ail_documents_deterministically() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    let app = dir.child("app.ail");
    let math_uri = format!("file://{}", math.path().display());
    let app_uri = format!("file://{}", app.path().display());
    math.write_str("module math\nfn stale() -> Int = 0\n")
        .expect("math fixture must be written");
    app.write_str("module app\nfn stale() -> Int = 0\n")
        .expect("app fixture must be written");

    let open_math = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": math_uri,
                "languageId": "ail",
                "version": 1,
                "text": "module math\nconst answer: Int = 42\nfn add_pair(x: Int, y: Int) -> Int = x + y\ntest add = eq(add_pair(20, 22), 42)\n",
            }
        }
    })
    .to_string();
    let open_app = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": app_uri,
                "languageId": "ail",
                "version": 1,
                "text": "module app\ncapability log.write\nfn main() -> Int = math.add_pair(20, 22)\n",
            }
        }
    })
    .to_string();
    let workspace_symbols = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "workspace/symbol",
        "params": { "query": "" }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_math.len(),
        open_math,
        open_app.len(),
        open_app,
        workspace_symbols.len(),
        workspace_symbols
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
        .find(|message| message["id"] == 11)
        .expect("workspace/symbol response must be emitted");
    let symbols = response["result"]
        .as_array()
        .expect("workspace/symbol result must be an array");
    let names = symbols
        .iter()
        .filter_map(|symbol| symbol["name"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "app",
            "app.main",
            "log.write",
            "math",
            "math.add_pair",
            "math.answer",
            "test.add",
        ]
    );
    let add_pair = symbols
        .iter()
        .find(|symbol| symbol["name"] == "math.add_pair")
        .expect("math.add_pair symbol must be indexed");
    assert_eq!(add_pair["kind"], 12);
    assert_eq!(add_pair["containerName"], "math");
    assert!(
        add_pair["location"]["uri"]
            .as_str()
            .expect("symbol uri")
            .ends_with("math.ail")
    );
    assert_eq!(add_pair["location"]["range"]["start"]["line"], 2);
    assert_eq!(add_pair["location"]["range"]["start"]["character"], 3);
}

#[test]
fn lsp_workspace_symbols_filters_query_against_open_document_index() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("math.ail");
    let source_uri = format!("file://{}", source.path().display());
    source
        .write_str("module math\nfn stale() -> Int = 0\n")
        .expect("source fixture must be written");
    let open_source = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "ail",
                "version": 1,
                "text": "module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\nfn subtract(x: Int, y: Int) -> Int = x - y\n",
            }
        }
    })
    .to_string();
    let workspace_symbols = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "workspace/symbol",
        "params": { "query": "pair" }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_source.len(),
        open_source,
        workspace_symbols.len(),
        workspace_symbols
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
        .find(|message| message["id"] == 12)
        .expect("workspace/symbol response must be emitted");
    let symbols = response["result"]
        .as_array()
        .expect("workspace/symbol result must be an array");

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "math.add_pair");
    assert_eq!(symbols[0]["kind"], 12);
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
