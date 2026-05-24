#![allow(dead_code)]
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
