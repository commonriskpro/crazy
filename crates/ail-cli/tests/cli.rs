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
        .stdout(predicate::str::contains("parse"));
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

/// Scenario: Parse stub with path — exits 0 and prints "not yet implemented".
#[test]
fn parse_with_path_exits_zero_and_prints_stub() {
    ail()
        .args(["parse", "some/file.atl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not yet implemented"));
}

/// Scenario: Parse stub missing path argument — exits non-zero with diagnostic on stderr.
#[test]
fn parse_without_path_exits_nonzero() {
    ail()
        .arg("parse")
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not());
}

/// Scenario: Unknown subcommand — exits non-zero and stderr names available subcommands.
#[test]
fn unknown_subcommand_exits_nonzero() {
    ail()
        .arg("unknowncmd")
        .assert()
        .failure()
        .stderr(predicate::str::contains("parse"));
}
