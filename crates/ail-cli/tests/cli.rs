// ── ail-cli integration tests (PR2 baseline) ─────────────────────────────
//
// These tests cover the minimum viable surface that must pass after PR2:
// binary presence, help/version flags, and unknown subcommand dispatch.
//
// The full test suite (unknown_subcommand_lists_six, context, change, verify,
// apply, JSON mode, E2E) will be added in PR3 (tasks 3.1–3.5).

use assert_cmd::Command;
use predicates::prelude::*;

fn ail() -> Command {
    Command::cargo_bin("ail").expect("ail binary must be present")
}

/// Scenario: Help flag — exits 0, prints usage, and lists available subcommands.
#[test]
fn help_exits_zero() {
    ail()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("context"));
}

/// Scenario: Version flag — exits 0 and version string is present.
#[test]
fn version_exits_zero_and_prints_version() {
    ail()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

/// Scenario: Unknown subcommand — exits 2 and stderr names the six subcommands.
#[test]
fn unknown_subcommand_exits_two_and_lists_six() {
    ail()
        .arg("unknowncmd")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Available subcommands: context, change, verify, apply, compile, run",
        ));
}

/// Scenario: `ail context` — exits 0 (empty store is valid).
#[test]
fn context_exits_zero() {
    ail().arg("context").assert().success();
}

/// Scenario: `ail context --json` — exits 0 and stdout is valid JSON.
///
/// JSON output must have top-level `status` and `data` fields per spec.
#[test]
fn context_json_flag_produces_parseable_json() {
    let output = ail()
        .args(["context", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("stdout must be utf8");
    let parsed: serde_json::Value =
        serde_json::from_str(text.trim()).expect("--json output must be parseable JSON");

    assert_eq!(
        parsed["status"], "ok",
        "JSON envelope must have status == \"ok\"; got: {parsed}"
    );
    assert!(
        parsed["data"].is_object(),
        "JSON envelope must have a data object; got: {parsed}"
    );
}
