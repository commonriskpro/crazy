// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

/// Spec scenario: ail link --help exits 0 and mentions native/linker.
///   GIVEN `ail link --help` is invoked
///   WHEN dispatch runs
///   THEN exit 0; output describes the profile flag and native object linking
#[test]
fn link_help_exits_zero_and_describes_profile_flag() {
    ail()
        .args(["link", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("profile"));
}
/// Spec scenario: ail link with no persisted artifact returns non-zero exit.
///   GIVEN an initialized project with no prior ail compile --target native
///   WHEN `ail link` runs
///   THEN exit code is 1; stderr contains 'no native artifact'
#[test]
fn link_without_compile_exits_nonzero_with_clear_error() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    dir.child(".ail").assert(predicate::path::is_dir());

    ail()
        .args(["link", "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no native artifact"));
}
/// Spec scenario: ail link --json with no artifact emits structured JSON error on stdout.
///   GIVEN an initialized project with no prior native compilation
///   WHEN `ail link --json` runs
///   THEN exit code is 1; stdout is valid JSON with status="error"; stderr has plain message
#[test]
fn link_json_no_artifact_emits_structured_error() {
    use assert_fs::TempDir;

    let dir = TempDir::new().expect("temp dir");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let output = ail()
        .args(["link", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no native artifact"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(
        v["status"], "error",
        "JSON status must be 'error' when no artifact; got: {v}"
    );
    assert_eq!(
        v["data"]["error"], "no native artifact",
        "JSON data.error must be 'no native artifact'; got: {v}"
    );
    assert!(
        v["data"]["next_action"].is_string(),
        "JSON data.next_action must be present to guide the user; got: {v}"
    );
}
