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
                "text": "module math\nconst answer: Int = 42\nfn add_pair(x: Int, y: Int) -> Int = x + y\ntest add {\n  let actual: Int = add_pair(20, 22)\n  return actual == 42\n}\n",
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
    let add_test = symbols
        .iter()
        .find(|symbol| symbol["name"] == "test.add")
        .expect("test.add symbol must be indexed");
    assert_eq!(add_test["location"]["range"]["start"]["line"], 3);
    assert_eq!(add_test["location"]["range"]["start"]["character"], 5);
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

#[test]
fn lsp_workspace_symbol_diagnostics_reports_missing_workspace_root() {
    let workspace_symbols = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "ail/workspaceSymbols",
        "params": { "query": "" }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}",
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
        .find(|message| message["id"] == 31)
        .expect("ail/workspaceSymbols response must be emitted");
    let result = &response["result"];

    assert_eq!(result["ok"], false);
    assert_eq!(result["diagnosticCount"], 1);
    assert_eq!(
        result["diagnostics"][0]["code"],
        "AIL_WORKSPACE_SYMBOL_MISSING_ROOT"
    );
    assert_eq!(result["diagnostics"][0]["category"], "document_state");
    assert_eq!(
        result["diagnostics"][0]["descriptor"]["workspaceRoot"],
        "uninitialized"
    );
}

#[test]
fn lsp_workspace_symbol_diagnostics_rejects_unsupported_query_shape_redacted() {
    let workspace_symbols = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "ail/workspaceSymbols",
        "params": { "query": { "pattern": "customer.secret" } }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}",
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
        .find(|message| message["id"] == 32)
        .expect("ail/workspaceSymbols response must be emitted");
    let result = &response["result"];

    assert_eq!(result["ok"], false);
    assert_eq!(
        result["diagnostics"][0]["code"],
        "AIL_WORKSPACE_SYMBOL_UNSUPPORTED_QUERY_SHAPE"
    );
    assert_eq!(
        result["diagnostics"][0]["descriptor"]["queryShape"],
        "object"
    );
    assert!(
        !result["diagnostics"][0]
            .to_string()
            .contains("customer.secret")
    );
}

#[test]
fn lsp_workspace_symbol_diagnostics_indexes_root_with_stable_order_and_warnings() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let alpha = dir.child("alpha.ail");
    let beta = dir.child("beta.ail");
    let unreadable = dir.child("customer.secret.ail");
    alpha
        .write_str("fn shared() -> Int = 1\nfn zzz() -> Int = 9\n")
        .expect("alpha fixture must be written");
    beta.write_str("fn aaa() -> Int = 0\nfn shared() -> Int = 2\n")
        .expect("beta fixture must be written");
    std::fs::create_dir(unreadable.path()).expect("unreadable fixture directory must be created");

    let root_uri = format!("file://{}", dir.path().display());
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "rootUri": root_uri }
    })
    .to_string();
    let diagnostic_symbols = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 33,
        "method": "ail/workspaceSymbols",
        "params": { "query": "" }
    })
    .to_string();
    let workspace_symbols = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 34,
        "method": "workspace/symbol",
        "params": { "query": "" }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        initialize.len(),
        initialize,
        diagnostic_symbols.len(),
        diagnostic_symbols,
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
    let diagnostic_response = messages
        .iter()
        .find(|message| message["id"] == 33)
        .expect("ail/workspaceSymbols response must be emitted");
    let standard_response = messages
        .iter()
        .find(|message| message["id"] == 34)
        .expect("workspace/symbol response must be emitted");
    let result = &diagnostic_response["result"];
    let symbols = result["symbols"]
        .as_array()
        .expect("symbols result must be an array");
    let names = symbols
        .iter()
        .filter_map(|symbol| symbol["name"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(result["ok"], true);
    assert_eq!(names, vec!["aaa", "shared", "shared", "zzz"]);
    assert_eq!(
        standard_response["result"]
            .as_array()
            .expect("standard workspace symbols must be an array")
            .iter()
            .filter_map(|symbol| symbol["name"].as_str())
            .collect::<Vec<_>>(),
        names
    );
    assert_eq!(result["diagnosticCount"], 2);
    assert!(
        result["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(
                |diagnostic| diagnostic["code"] == "AIL_WORKSPACE_SYMBOL_AMBIGUOUS_SYMBOL"
                    && diagnostic["descriptor"]["candidateCount"] == 2
            )
    );
    assert!(
        result["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(
                |diagnostic| diagnostic["code"] == "AIL_WORKSPACE_SYMBOL_SKIPPED_UNREADABLE_FILE"
                    && diagnostic["descriptor"]["pathRedacted"] == true
            )
    );
    assert!(
        !result["diagnostics"]
            .to_string()
            .contains("customer.secret")
    );
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
