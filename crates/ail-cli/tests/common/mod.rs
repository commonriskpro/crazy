#![allow(dead_code)]

pub mod package_helpers;
use assert_cmd::Command;
use serde_json::Value;
use std::process::Output;

pub fn ail() -> Command {
    Command::cargo_bin("ail").expect("ail binary must be present")
}

/// Return the path to the sample fixture file (relative to CARGO_MANIFEST_DIR).
pub fn sample_acl_path() -> std::path::PathBuf {
    let manifest =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set during tests");
    std::path::Path::new(&manifest)
        .join("tests")
        .join("fixtures")
        .join("sample.acl")
}

/// Parse stdout bytes as JSON; panic with context on failure.
pub fn parse_json_output(output: &Output) -> Value {
    let text = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("--json output must parse as JSON: {e}\nRaw: {text}"))
}

/// Extract the change-id value from human-readable `ail change` output.
pub fn extract_change_id(stdout: &str) -> String {
    stdout
        .lines()
        .find(|l| l.contains("change-id:"))
        .expect("change-id line must be present")
        .split("change-id:")
        .nth(1)
        .expect("change-id value must follow colon")
        .trim()
        .to_string()
}

/// Run `ail change --file sample.acl` and return the deterministic change-id.
///
/// Used by tests that need a valid 64-char hex change-id to pass to verify/apply.
pub fn compute_sample_change_id() -> String {
    let path = sample_acl_path();
    let output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    extract_change_id(std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8"))
}

/// Run `ail change --file sample.acl --json` in an initialized project and
/// return the persisted ChangeSet id that `ail apply` can load later.
pub fn create_sample_change(project_dir: &std::path::Path) -> String {
    let path = sample_acl_path();
    let output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(project_dir)
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    v["data"]["change_id"]
        .as_str()
        .or_else(|| v["data"]["canonical_change"]["change_id"].as_str())
        .expect("change output must include a change_id")
        .to_string()
}
