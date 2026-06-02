// Mechanical phase 2 split from lsp_intelligence.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

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
fn lsp_references_use_utf16_source_reference_ranges() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "module main\nfn main() -> Text = \"🔥\" ++ helper()\nfn helper() -> Text = \"x\"\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--references-token", "helper", "--references-file"])
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
    assert_eq!(v["data"]["reference_count"], 2);
    assert_eq!(refs[0]["range"]["start"]["line"], 1);
    assert_eq!(
        refs[0]["range"]["start"]["character"], 28,
        "reference after emoji must use UTF-16 offset, not byte offset"
    );
    assert_eq!(refs[0]["range"]["end"]["character"], 34);
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
    assert_eq!(v["data"]["references_found"], true);
    assert_eq!(v["data"]["reference_count"], 3);
    let reference_uris = v["data"]["reference_uris"]
        .as_array()
        .expect("reference uris must be an array");
    assert_eq!(reference_uris.len(), 2);
    assert!(
        reference_uris[0]
            .as_str()
            .expect("first reference uri")
            .ends_with("main.ail")
    );
    assert!(
        reference_uris[1]
            .as_str()
            .expect("second reference uri")
            .ends_with("math.ail")
    );
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

#[test]
fn lsp_references_resolve_ail_source_block_test_uses() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "test smoke {\n  let actual: Int = 20 + 22\n  return actual == 42\n}\ngrant test.smoke log.write\nfn main() -> Int = 0\n",
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
    assert_eq!(refs[1]["range"]["start"]["line"], 4);
    assert_eq!(refs[1]["range"]["start"]["character"], 6);
}

#[test]
fn lsp_references_scope_kind_qualified_source_test_without_function_collision() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn smoke() -> Int = 0\n\
test smoke = eq(smoke(), 0)\n\
grant test.smoke log.write\n\
fn main() -> Int = smoke()\n",
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
    assert_eq!(refs[0]["range"]["start"]["line"], 1);
    assert_eq!(refs[0]["range"]["start"]["character"], 5);
    assert_eq!(refs[1]["range"]["start"]["line"], 2);
    assert_eq!(refs[1]["range"]["start"]["character"], 6);
}

#[test]
fn lsp_references_return_empty_for_ambiguous_imported_source_symbols() {
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
        .args(["lsp", "--references-token", "helper", "--references-file"])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "helper");
    assert_eq!(v["data"]["references_found"], false);
    assert_eq!(v["data"]["reference_count"], 0);
    assert_eq!(
        v["data"]["reference_uris"]
            .as_array()
            .expect("empty reference uris must be an array")
            .len(),
        0
    );
    assert_eq!(
        v["data"]["references"]
            .as_array()
            .expect("ambiguous references result must be an array")
            .len(),
        0
    );
}

#[test]
fn lsp_stdio_references_use_open_workspace_import_text() {
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
    let references = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "textDocument/references",
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
        references.len(),
        references
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
        .find(|message| message["id"] == 9)
        .expect("references response must be emitted");
    let refs = response["result"]
        .as_array()
        .expect("references result must be an array");

    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0]["range"]["start"]["line"], 1);
    assert!(
        refs[1]["uri"]
            .as_str()
            .expect("imported reference uri")
            .ends_with("math.ail")
    );
    assert_eq!(refs[1]["range"]["start"]["line"], 1);
    assert_eq!(refs[1]["range"]["start"]["character"], 3);
}

#[test]
fn lsp_references_diagnostics_report_missing_document() {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "ail/references",
        "params": {
            "textDocument": { "uri": "file:///redacted/missing.ail" },
            "position": { "line": 0, "character": 0 }
        }
    });

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(lsp_frame(&request))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == 41)
        .expect("references diagnostic response must be emitted");

    assert_eq!(response["result"]["ok"], false);
    assert_eq!(
        response["result"]["diagnostics"][0]["reason"],
        "missing_document"
    );
    assert_eq!(
        response["result"]["diagnostics"][0]["code"],
        "AIL_REFERENCES_MISSING_DOCUMENT"
    );
    assert_eq!(
        response["result"]["diagnostics"][0]["descriptor"]["documentUriRedacted"],
        true
    );
}

#[test]
fn lsp_references_diagnostics_redact_unresolved_symbol() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let main = dir.child("main.ail");
    let main_text = "fn main() -> Int = secret_symbol()\n";
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
    });
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "ail/references",
        "params": {
            "textDocument": { "uri": main_uri },
            "position": { "line": 0, "character": main_text.find("secret_symbol").unwrap() + 1 }
        }
    });

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(format!("{}{}", lsp_frame(&open_main), lsp_frame(&request)))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == 42)
        .expect("references diagnostic response must be emitted");
    let rendered = response.to_string();

    assert_eq!(response["result"]["ok"], false);
    assert_eq!(
        response["result"]["diagnostics"][0]["reason"],
        "unresolved_symbol"
    );
    assert_eq!(
        response["result"]["diagnostics"][0]["code"],
        "AIL_REFERENCES_UNRESOLVED_SYMBOL"
    );
    assert!(!rendered.contains("secret_symbol"));
    assert_eq!(
        response["result"]["diagnostics"][0]["descriptor"]["token"]["tokenLength"],
        13
    );
}

#[test]
fn lsp_references_diagnostics_report_ambiguous_imported_symbols() {
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
    let main_text = "use \"./right.ail\"\nuse \"./left.ail\"\nfn main() -> Int = helper()\n";
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
    });
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 43,
        "method": "ail/references",
        "params": {
            "textDocument": { "uri": main_uri },
            "position": { "line": 2, "character": main_text.lines().nth(2).unwrap().find("helper").unwrap() + 1 }
        }
    });

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(format!("{}{}", lsp_frame(&open_main), lsp_frame(&request)))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == 43)
        .expect("references diagnostic response must be emitted");

    assert_eq!(response["result"]["ok"], false);
    assert_eq!(response["result"]["referenceCount"], 0);
    assert_eq!(
        response["result"]["diagnostics"][0]["reason"],
        "ambiguous_symbol"
    );
    assert_eq!(
        response["result"]["diagnostics"][0]["descriptor"]["candidateCount"],
        2
    );
    assert_eq!(
        response["result"]["diagnostics"][0]["descriptor"]["candidateLocationsRedacted"],
        true
    );
}

#[test]
fn lsp_references_diagnostics_warn_for_skipped_unreadable_import() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    std::fs::create_dir(dir.child("bad.ail").path())
        .expect("unreadable import directory must be created");
    let main = dir.child("main.ail");
    let main_text = "use \"./bad.ail\"\nfn main() -> Int = main()\n";
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
    });
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 44,
        "method": "ail/references",
        "params": {
            "textDocument": { "uri": main_uri },
            "position": { "line": 1, "character": main_text.lines().nth(1).unwrap().find("main").unwrap() + 1 }
        }
    });

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(format!("{}{}", lsp_frame(&open_main), lsp_frame(&request)))
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let response = messages
        .iter()
        .find(|message| message["id"] == 44)
        .expect("references diagnostic response must be emitted");

    assert_eq!(response["result"]["ok"], true);
    assert_eq!(response["result"]["diagnosticCount"], 1);
    assert_eq!(
        response["result"]["diagnostics"][0]["reason"],
        "skipped_import"
    );
    assert_eq!(response["result"]["diagnostics"][0]["severity"], "warning");
    assert_eq!(
        response["result"]["diagnostics"][0]["descriptor"]["importState"],
        "unreadable_import"
    );
    assert_eq!(
        response["result"]["diagnostics"][0]["descriptor"]["importPathRedacted"],
        true
    );
}

#[test]
fn lsp_references_order_imported_locations_deterministically_by_uri() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let imported = dir.child("aaa.ail");
    imported
        .write_str("module aaa\nfn target() -> Int = 1\n")
        .expect("imported source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str("use \"./aaa.ail\"\nfn main() -> Int = aaa.target()\n")
        .expect("main source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--references-token",
            "aaa.target",
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
    assert_eq!(v["data"]["reference_count"], 2);
    assert!(
        refs[0]["uri"]
            .as_str()
            .expect("first reference uri")
            .ends_with("aaa.ail")
    );
    assert!(
        refs[1]["uri"]
            .as_str()
            .expect("second reference uri")
            .ends_with("main.ail")
    );
}

fn lsp_frame(message: &serde_json::Value) -> String {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
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
