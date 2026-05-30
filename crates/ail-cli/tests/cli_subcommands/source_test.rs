// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn test_file_runs_ail_source_tests_without_acl_authoring() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main() -> Int = add(20, 22)\n\
fn add_pair(x: Int, y: Int) -> Int = x + y\n\
test main_addition = eq(add_pair(20, 22), 42)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "test",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS test.main_addition"))
        .stdout(predicate::str::contains("test result: 1 passed; 0 failed"));
}
#[test]
fn test_file_runs_module_qualified_ail_source_tests() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "module app\n\
fn main() -> Int = add(20, 22)\n\
test main_addition = eq(main(), 42)\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args([
            "test",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["passed"], 1);
    assert_eq!(v["data"]["failed"], 0);
    assert_eq!(v["data"]["tests"][0]["name"], "test.app.main_addition");
    assert_eq!(v["data"]["tests"][0]["export"], "app_main_addition");
}
#[test]
fn test_command_runs_graph_test_nodes() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let acl = dir.child("test-runner.acl");
    acl.write_str(
        r#"change test_runner
author cli-test
description add user-facing test runner target
base 0
op create_test id=test.addition body=eq(add(20, 22), 42)
end
"#,
    )
    .expect("ACL fixture must be written");

    let change_output = ail()
        .args([
            "change",
            "--file",
            acl.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change output must include a change_id");

    ail()
        .args(["verify", change_id])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["apply", change_id, "--yes"])
        .current_dir(dir.path())
        .assert()
        .success();

    ail()
        .args(["test"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS test.addition"))
        .stdout(predicate::str::contains("test result: 1 passed; 0 failed"));

    let test_output = ail()
        .args(["test", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let test_json = parse_json_output(&test_output);
    assert_eq!(test_json["status"], "ok");
    assert_eq!(test_json["data"]["total"], 1);
    assert_eq!(test_json["data"]["passed"], 1);
    assert_eq!(test_json["data"]["failed"], 0);
    assert_eq!(test_json["data"]["tests"][0]["name"], "test.addition");
    assert_eq!(test_json["data"]["tests"][0]["status"], "passed");
}
