use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ail_testkit::{
    DiagnosticOrder, RunnerAssertion, RunnerCase, RunnerFixture, RunnerIssueKind,
    run_runner_diagnostics,
};

static CASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_case_dir(name: &str) -> PathBuf {
    let id = CASE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ail-testkit-runner-diagnostics-{}-{id}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("test temp dir must be created");
    path
}

fn write_fixture(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("fixture must be written");
    path
}

#[test]
fn missing_fixture_reports_stable_redacted_issue() {
    let dir = temp_case_dir("missing-fixture");
    let missing = dir.join("missing.atl");

    let report = run_runner_diagnostics(
        RunnerCase::new().with_fixture(RunnerFixture::path(missing.clone())),
    );

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, RunnerIssueKind::FixtureMissing);

    let diagnostics = report.stable_diagnostics();
    assert!(diagnostics.contains("kind: fixture-missing"));
    assert!(diagnostics.contains("fixture: <fixture:missing.atl>"));
    assert!(
        !diagnostics.contains(dir.to_string_lossy().as_ref()),
        "diagnostics must not leak absolute fixture directories: {diagnostics}"
    );
}

#[test]
fn empty_fixture_reports_invalid_fixture() {
    let dir = temp_case_dir("invalid-fixture");
    let fixture = write_fixture(&dir, "empty.atl", "\n\t  ");

    let report =
        run_runner_diagnostics(RunnerCase::new().with_fixture(RunnerFixture::path(fixture)));

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, RunnerIssueKind::FixtureInvalid);
    assert!(
        report
            .stable_diagnostics()
            .contains("fixture exists but is empty")
    );
}

#[test]
fn assertion_mismatch_redacts_unstable_fixture_paths() {
    let dir = temp_case_dir("assertion-mismatch");
    let fixture = write_fixture(&dir, "case.atl", "module case\n");
    let absolute_output = format!("wrote artifact to {}/artifact.wasm", dir.display());

    let report = run_runner_diagnostics(
        RunnerCase::new()
            .with_fixture(RunnerFixture::path(fixture))
            .with_assertion(RunnerAssertion::eq(
                "artifact path",
                "wrote artifact to <fixture-dir>/expected.wasm",
                absolute_output,
            )),
    );

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, RunnerIssueKind::AssertionMismatch);

    let diagnostics = report.stable_diagnostics();
    assert!(diagnostics.contains("kind: assertion-mismatch"));
    assert!(diagnostics.contains("actual: wrote artifact to <fixture-dir>/artifact.wasm"));
    assert!(
        !diagnostics.contains(dir.to_string_lossy().as_ref()),
        "diagnostics must redact unstable paths: {diagnostics}"
    );
}

#[test]
fn absent_expected_diagnostic_is_reported() {
    let report = run_runner_diagnostics(
        RunnerCase::new()
            .with_fixture(RunnerFixture::inline("valid.atl", "module valid\n"))
            .with_actual_diagnostic("parsed fixture successfully")
            .with_expected_diagnostic("type error"),
    );

    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        report.issues[0].kind,
        RunnerIssueKind::ExpectedDiagnosticAbsent
    );
    assert!(
        report
            .stable_diagnostics()
            .contains("kind: expected-diagnostic-absent")
    );
}

#[test]
fn nondeterministic_diagnostic_order_is_reported_when_represented() {
    let report = run_runner_diagnostics(
        RunnerCase::new()
            .with_fixture(RunnerFixture::inline("valid.atl", "module valid\n"))
            .with_diagnostic_order(DiagnosticOrder::Nondeterministic),
    );

    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        report.issues[0].kind,
        RunnerIssueKind::NondeterministicDiagnosticOrder
    );
    assert!(
        report
            .stable_diagnostics()
            .contains("kind: nondeterministic-diagnostic-order")
    );
}

#[test]
fn reports_issue_kinds_in_deterministic_order() {
    let report = run_runner_diagnostics(
        RunnerCase::new()
            .with_fixture(RunnerFixture::inline("empty.atl", ""))
            .with_diagnostic_order(DiagnosticOrder::Nondeterministic)
            .with_expected_diagnostic("later diagnostic")
            .with_assertion(RunnerAssertion::eq("mismatch", "expected", "actual")),
    );

    let kinds: Vec<RunnerIssueKind> = report.issues.iter().map(|issue| issue.kind).collect();
    assert_eq!(
        kinds,
        vec![
            RunnerIssueKind::FixtureInvalid,
            RunnerIssueKind::AssertionMismatch,
            RunnerIssueKind::ExpectedDiagnosticAbsent,
            RunnerIssueKind::NondeterministicDiagnosticOrder,
        ]
    );
}
