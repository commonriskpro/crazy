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
