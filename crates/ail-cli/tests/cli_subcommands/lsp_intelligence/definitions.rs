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
