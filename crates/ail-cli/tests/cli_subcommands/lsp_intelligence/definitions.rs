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
