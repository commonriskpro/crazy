use ail_stdlib::diagnostics::{Diagnostic, DiagnosticSeverity, ProofObligation};
use ail_stdlib::verify::{
    PolicyReport, RuntimeCheck, VerificationEntry, VerificationReport, VerificationStatus,
    extract_repair_ops, format_report, group_obligations,
};

#[test]
fn verification_report_counts_and_coverage() {
    let mut report = VerificationReport::new();
    report.add_entry(VerificationEntry::new(
        "R1",
        "must pass",
        VerificationStatus::Pass,
    ));
    report.add_entry(VerificationEntry::new(
        "R2",
        "must fail",
        VerificationStatus::Fail,
    ));

    assert_eq!(report.pass_count(), 1);
    assert_eq!(report.fail_count(), 1);
    assert_eq!(report.coverage(), 0.5);
}

#[test]
fn policy_report_tracks_violations() {
    let mut report = PolicyReport::new("prod");
    assert!(report.passed);

    report.add_violation(Diagnostic::new("P001", DiagnosticSeverity::Error, "denied"));

    assert!(!report.passed);
    assert_eq!(report.violations.len(), 1);
}

#[test]
fn runtime_check_pass_and_fail_constructors() {
    let ok = RuntimeCheck::pass("C1", "safe");
    let err = RuntimeCheck::fail("C2", "unsafe", "boom");

    assert!(ok.passed);
    assert!(!err.passed);
    assert_eq!(err.error.as_deref(), Some("boom"));
}

#[test]
fn verify_extract_repair_ops_uses_failed_entries() {
    let mut report = VerificationReport::new();
    report.add_entry(VerificationEntry::new(
        "R1",
        "passes",
        VerificationStatus::Pass,
    ));
    report.add_entry(VerificationEntry::new(
        "R2",
        "fails",
        VerificationStatus::Fail,
    ));

    let repairs = extract_repair_ops(&report);

    assert_eq!(repairs.len(), 1);
    assert_eq!(repairs[0].id, "repair-R2");
}

#[test]
fn verify_group_obligations_splits_by_satisfaction() {
    let mut report = VerificationReport::new();
    let mut satisfied = ProofObligation::new("P1", "done", "std.verify");
    satisfied.satisfied = true;
    report.add_obligation(satisfied);
    report.add_obligation(ProofObligation::new("P2", "todo", "std.verify"));

    let (done, todo) = group_obligations(&report);

    assert_eq!(done.len(), 1);
    assert_eq!(todo.len(), 1);
}

#[test]
fn format_report_includes_summary_and_failures() {
    let mut report = VerificationReport::new();
    report.add_entry(VerificationEntry::new(
        "R1",
        "must hold",
        VerificationStatus::Fail,
    ));

    let formatted = format_report(&report);

    assert!(formatted.contains("0/1 passed"));
    assert!(formatted.contains("FAIL: 1 requirement(s) not met"));
}
