// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn check_file_validates_ail_source_without_execution() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("imported source fixture must be written");
    let source = dir.child("main.ail");
    source
        .write_str(
            "use \"./math.ail\"\ncapability log.write\nconst answer: Int = 42\nfn main() -> Int = math.add_pair(answer, 0)\ntest smoke = eq(main(), 42)\ngrant main log.write\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["check", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["functions"], 2);
    assert_eq!(
        v["data"]["function_names"],
        serde_json::json!(["fn.math.add_pair", "fn.main"])
    );
    assert_eq!(v["data"]["imports"], 1);
    assert_eq!(v["data"]["import_paths"], serde_json::json!(["./math.ail"]));
    assert_eq!(v["data"]["capabilities"], 1);
    assert_eq!(
        v["data"]["capability_names"],
        serde_json::json!(["log.write"])
    );
    assert_eq!(v["data"]["constants"], 1);
    assert_eq!(
        v["data"]["constant_names"],
        serde_json::json!(["fn.answer"])
    );
    assert_eq!(v["data"]["tests"], 1);
    assert_eq!(v["data"]["test_names"], serde_json::json!(["test.smoke"]));
    assert_eq!(v["data"]["grants"], 1);
    assert_eq!(v["data"]["grant_targets"], serde_json::json!(["fn.main"]));
    assert_eq!(
        v["data"]["granted_capabilities"],
        serde_json::json!(["log.write"])
    );
    assert_eq!(v["data"]["default_entry"], "fn.main");
    assert_eq!(v["data"]["default_entry_exists"], true);
    assert_eq!(
        v["data"]["entrypoint_candidates"],
        serde_json::json!(["fn.main"])
    );
    assert!(
        v["data"]["graph_nodes"].as_u64().unwrap() >= 2,
        "check must materialize source into a semantic graph"
    );
    assert!(
        v["data"]["graph_edges"].as_u64().is_some(),
        "check must report graph edge materialization"
    );
}

#[test]
fn check_file_reports_missing_default_entrypoint() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("library.ail");
    source
        .write_str("fn helper() -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["check", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["default_entry"], "fn.main");
    assert_eq!(v["data"]["default_entry_exists"], false);
    assert_eq!(
        v["data"]["entrypoint_candidates"],
        serde_json::Value::Array(vec![])
    );
}
#[test]
fn check_file_rejects_invalid_ail_source_without_execution() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad.ail");
    source
        .write_str("fn main() -> Int = add(x, 1)\n")
        .expect("source fixture must be written");

    ail()
        .args(["check", "--file"])
        .arg(source.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 1: unknown variable `x`"));
}
