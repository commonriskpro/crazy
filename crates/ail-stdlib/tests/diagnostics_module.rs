use ail_stdlib::diagnostics::{
    Diagnostic, DiagnosticSeverity, ProofObligation, RepairOption, extract_repair_ops,
    format_diagnostic, group_obligations,
};

#[test]
fn diagnostic_format_includes_location_and_notes() {
    let d = Diagnostic::new("E001", DiagnosticSeverity::Error, "bad type")
        .with_location("main.ail", 3, 9)
        .with_note("expected Int");

    let formatted = format_diagnostic(&d);

    assert!(formatted.contains("[error] E001 at main.ail:3:9"));
    assert!(formatted.contains("expected Int"));
}

#[test]
fn repair_option_can_carry_patch() {
    let repair = RepairOption::new("R001", "replace type", 80).with_patch("-Text\n+Int");

    assert_eq!(repair.confidence, 80);
    assert_eq!(repair.patch.as_deref(), Some("-Text\n+Int"));
}

#[test]
fn extract_repair_ops_only_returns_error_level_diagnostics() {
    let diagnostics = vec![
        Diagnostic::new("W001", DiagnosticSeverity::Warning, "unused"),
        Diagnostic::new("E001", DiagnosticSeverity::Error, "bad type"),
        Diagnostic::new("F001", DiagnosticSeverity::Fatal, "cannot continue"),
    ];

    let repairs = extract_repair_ops(&diagnostics);

    assert_eq!(repairs.len(), 2);
    assert_eq!(repairs[0].id, "repair-E001");
    assert_eq!(repairs[1].id, "repair-F001");
}

#[test]
fn group_obligations_orders_by_module() {
    let obligations = vec![
        ProofObligation::new("P2", "prove text", "std.text"),
        ProofObligation::new("P1", "prove numeric", "std.numeric"),
    ];

    let grouped = group_obligations(&obligations);

    assert_eq!(grouped[0].0, "std.numeric");
    assert_eq!(grouped[1].0, "std.text");
}
