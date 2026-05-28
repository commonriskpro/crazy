#![allow(dead_code)]

pub mod package_helpers;
use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;
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

pub fn string_ops_acl_path() -> std::path::PathBuf {
    let manifest =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set during tests");
    std::path::Path::new(&manifest)
        .join("tests")
        .join("fixtures")
        .join("string_ops.acl")
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

/// Write a hand-crafted `VerificationReport` sidecar into the file-backed project
/// store at `project_dir`.
///
/// The report contains a single entry with the given `state`.  The CBOR-encoded
/// object is written to `.ail/store/objects/<blake3-hex>` and the sidecar index
/// pointer is written to `.ail/reports/<change_id>`.
///
/// Used by verification-gate tests (VG-2, VG-3) that need a persisted `Failed`
/// or `Unsafe` report without going through a real verification pipeline.
pub fn write_blocked_report_sidecar(
    project_dir: &Path,
    change_id: &str,
    state: ail_verify::report::VerificationState,
) {
    use ail_verify::report::{VerificationEntry, VerificationReport};

    let blocking = matches!(
        state,
        ail_verify::report::VerificationState::Failed
            | ail_verify::report::VerificationState::Unsafe
    );
    let report = VerificationReport {
        entries: vec![VerificationEntry {
            claim: "gate-test synthetic entry".to_string(),
            state,
            scope: "test".to_string(),
            evidence: None,
            blocking,
            repair_options: vec![],
        }],
        ..Default::default()
    };

    let mut bytes = Vec::new();
    ciborium::into_writer(&report, &mut bytes).expect("CBOR encoding must succeed");

    let hash = blake3::hash(&bytes);
    let hex = hash.to_hex();
    let hex_str = hex.as_str();

    // Write content-addressed object to .ail/store/objects/<hex>.
    let objects_dir = project_dir.join(".ail").join("store").join("objects");
    std::fs::create_dir_all(&objects_dir).expect("objects dir must be creatable");
    std::fs::write(objects_dir.join(hex_str), &bytes).expect("object write must succeed");

    // Write sidecar pointer to .ail/reports/<change_id>.
    let reports_dir = project_dir.join(".ail").join("reports");
    std::fs::create_dir_all(&reports_dir).expect("reports dir must be creatable");
    std::fs::write(reports_dir.join(change_id), format!("{hex_str}\n"))
        .expect("sidecar write must succeed");
}
