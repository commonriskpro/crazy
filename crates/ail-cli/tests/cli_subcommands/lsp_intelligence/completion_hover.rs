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
                .contains("match ${1:value}")
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("return ${3:v}")
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("${4:None} => {\n        return ${5:fallback}\n    }")),
        "completion must include match snippet; got: {items:?}"
    );
}

#[test]
fn lsp_completion_covers_source_block_control_flow() {
    let completion_output = ail()
        .args(["lsp", "--complete", "return", "--json"])
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
        items.iter().any(|item| item["label"] == "return"
            && item["insertText"].as_str().expect("insertText") == "return ${1:value}"),
        "completion must include return marker snippet; got: {items:?}"
    );

    let else_completion_output = ail()
        .args(["lsp", "--complete", "else", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let else_completion = parse_json_output(&else_completion_output);
    assert_eq!(else_completion["status"], "ok");
    let else_items = else_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        else_items.iter().any(|item| item["label"] == "else if"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("else if ${1:condition}")),
        "completion must include else-if block snippet; got: {else_items:?}"
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
fn lsp_completion_covers_source_unit_type() {
    let completion_output = ail()
        .args(["lsp", "--complete", "Unit", "--json"])
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
        items.iter().any(|item| item["label"] == "Unit"
            && item["insertText"].as_str().expect("insertText") == "Unit"
            && item["documentation"]["value"]
                .as_str()
                .expect("documentation")
                .contains("without a meaningful value")),
        "completion must include Unit type snippet; got: {items:?}"
    );
}

#[test]
fn lsp_completion_and_hover_cover_source_unit_literal() {
    let completion_output = ail()
        .args(["lsp", "--complete", "()", "--json"])
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
        items.iter().any(|item| item["label"] == "()"
            && item["insertText"].as_str().expect("insertText") == "()"
            && item["detail"] == "AIL source Unit literal"),
        "completion must include Unit literal snippet; got: {items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "()", "--json"])
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
            .contains("Unit literal value"),
        "hover must explain source Unit literal; got: {hover}"
    );
}

#[test]
fn lsp_completion_ranks_exact_matches_before_contains() {
    let completion_output = ail()
        .args(["lsp", "--complete", "test", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");

    assert_eq!(
        items.first().expect("test completions must not be empty")["label"],
        "test",
        "exact source test snippet must rank before ACL create_test; got: {items:?}"
    );
    assert!(
        items.iter().any(|item| item["label"] == "op create_test"),
        "substring matches must still be available after exact matches; got: {items:?}"
    );
    assert!(
        items.windows(2).all(
            |window| window[0]["sortText"].as_str().expect("left sortText")
                <= window[1]["sortText"].as_str().expect("right sortText")
        ),
        "completion sortText must preserve deterministic server order; got: {items:?}"
    );
}

#[test]
fn lsp_completion_covers_source_block_tests() {
    let completion_output = ail()
        .args(["lsp", "--complete", "test", "--json"])
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
        items.iter().any(|item| item["label"] == "test"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("test ${1:name} {\n    let ${2:actual}: ${3:Int}")
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("return ${5:eq(actual, 42)}")),
        "completion must include block source test snippet; got: {items:?}"
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
fn lsp_hover_reports_source_type_metadata() {
    let hover_output = ail()
        .args(["lsp", "--hover-token", "Int", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let hover = parse_json_output(&hover_output);
    assert_eq!(hover["status"], "ok");
    assert_eq!(hover["data"]["hover"]["contents"]["kind"], "markdown");
    assert!(
        hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Signed integer type"),
        "hover must explain AIL source builtin types; got: {hover}"
    );
}

#[test]
fn lsp_stdio_hover_reports_source_function_detail_from_workspace_import() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn stale() -> Int = 0\n")
        .expect("stale source fixture must be written");
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
    let hover = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", main.path().display()) },
            "position": { "line": 1, "character": 25 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_main.len(),
        open_main,
        open_math.len(),
        open_math,
        hover.len(),
        hover
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
        .find(|message| message["id"] == 21)
        .expect("hover response must be emitted");

    assert_eq!(response["result"]["contents"]["kind"], "markdown");
    assert!(
        response["result"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("fn add_pair(x: Int, y: Int) -> Int"),
        "hover must show imported source function signature; got: {response}"
    );
    assert_eq!(response["result"]["data"]["kind"], "function");
    assert_eq!(response["result"]["data"]["label"], "math.add_pair");
}

#[test]
fn lsp_stdio_hover_reports_source_import_detail() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let text = "use \"./math.ail\"\nfn main() -> Int = 0\n";
    source
        .write_str(text)
        .expect("source fixture must be written");
    let uri = format!("file://{}", source.path().display());
    let open = serde_json::json!({
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
    .to_string();
    let hover = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source.path().display()) },
            "position": { "line": 0, "character": 8 }
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
        .find(|message| message["id"] == 22)
        .expect("hover response must be emitted");

    assert_eq!(response["result"]["data"]["kind"], "import");
    assert!(
        response["result"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("use \"./math.ail\""),
        "hover must show deterministic import markdown; got: {response}"
    );
}

#[test]
fn lsp_stdio_hover_returns_null_for_ambiguous_imported_source_symbols() {
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
    let main_text = "use \"./left.ail\"\nuse \"./right.ail\"\nfn main() -> Int = helper()\n";
    main.write_str(main_text)
        .expect("main source fixture must be written");
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
    let hover = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 23,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", main.path().display()) },
            "position": { "line": 2, "character": 21 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open_main.len(),
        open_main,
        hover.len(),
        hover
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
        .find(|message| message["id"] == 23)
        .expect("hover response must be emitted");

    assert!(
        response["result"].is_null(),
        "ambiguous imported source hover must stay empty; got: {response}"
    );
}

#[test]
fn lsp_stdio_completion_uses_cursor_prefix_to_filter_items() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let text = "fn main() -> Int = ma\n";
    source
        .write_str(text)
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
                "text": text,
            }
        }
    })
    .to_string();
    let completion = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", source.path().display()) },
            "position": { "line": 0, "character": 21 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open.len(),
        open,
        completion.len(),
        completion
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
        .find(|message| message["id"] == 2)
        .expect("completion response must be emitted");
    let items = response["result"]["items"]
        .as_array()
        .expect("completion result items must be an array");

    assert!(
        items.iter().any(|item| item["label"] == "map"),
        "cursor prefix `ma` must keep matching builtins; got: {items:?}"
    );
    assert!(
        items.iter().any(|item| item["label"] == "match"),
        "cursor prefix `ma` must keep matching syntax snippets; got: {items:?}"
    );
    assert!(
        !items.iter().any(|item| item["label"] == "fn"),
        "cursor prefix `ma` must filter unrelated snippets; got: {items:?}"
    );
}

#[test]
fn lsp_stdio_completion_uses_utf16_cursor_positions() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let text = "// 🔥 reX\n";
    source
        .write_str(text)
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
                "text": text,
            }
        }
    })
    .to_string();
    let completion = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", source.path().display()) },
            "position": { "line": 0, "character": 8 }
        }
    })
    .to_string();
    let input = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        open.len(),
        open,
        completion.len(),
        completion
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
        .find(|message| message["id"] == 3)
        .expect("completion response must be emitted");
    let items = response["result"]["items"]
        .as_array()
        .expect("completion result items must be an array");

    assert!(
        items.iter().any(|item| item["label"] == "Record"),
        "UTF-16 cursor after `re` must keep `Record`; got: {items:?}"
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
