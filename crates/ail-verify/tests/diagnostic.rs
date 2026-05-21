// ── ail-verify::diagnostic tests ─────────────────────────────────────────
//
// Covers:
//   - Diagnostic construction (Diagnostic::error, Diagnostic::warning)
//   - Builder helpers (with_evidence, with_expected, with_actual, with_repair)
//   - RepairOption variants: DirectOp, Choice, Migration, Approval, Explanation
//   - All seven error code constants
//   - DiagnosticSeverity variants
//   - CBOR round-trip for Diagnostic and RepairOption

use ail_change::model::ChangeSetOp;
use ail_core::semantic_graph::NodeRef;
use ail_verify::diagnostic::{
    Diagnostic, DiagnosticSeverity, E_CAPABILITY_DENIED, E_CONTRACT_VIOLATED, E_EFFECT_UNDECLARED,
    E_EFFECT_UNUSED, E_REFINEMENT_NOT_PROVEN, E_STALE_BASE, E_TYPE_MISMATCH, RepairOption,
};

// ── Error code constants ───────────────────────────────────────────────────

#[test]
fn error_code_constants_have_expected_values() {
    assert_eq!(E_TYPE_MISMATCH, "E_TYPE_MISMATCH");
    assert_eq!(E_EFFECT_UNDECLARED, "E_EFFECT_UNDECLARED");
    assert_eq!(E_EFFECT_UNUSED, "E_EFFECT_UNUSED");
    assert_eq!(E_REFINEMENT_NOT_PROVEN, "E_REFINEMENT_NOT_PROVEN");
    assert_eq!(E_CONTRACT_VIOLATED, "E_CONTRACT_VIOLATED");
    assert_eq!(E_CAPABILITY_DENIED, "E_CAPABILITY_DENIED");
    assert_eq!(E_STALE_BASE, "E_STALE_BASE");
}

// ── DiagnosticSeverity ────────────────────────────────────────────────────

#[test]
fn severity_variants_are_distinct() {
    assert_ne!(DiagnosticSeverity::Error, DiagnosticSeverity::Warning);
    assert_ne!(DiagnosticSeverity::Error, DiagnosticSeverity::Info);
    assert_ne!(DiagnosticSeverity::Warning, DiagnosticSeverity::Info);
}

// ── Diagnostic::error constructor ─────────────────────────────────────────

#[test]
fn error_constructor_sets_blocking_true_and_error_severity() {
    let diag = Diagnostic::error(E_TYPE_MISMATCH, NodeRef(0));
    assert_eq!(diag.code, E_TYPE_MISMATCH);
    assert_eq!(diag.severity, DiagnosticSeverity::Error);
    assert_eq!(diag.target, NodeRef(0));
    assert!(diag.blocking, "error diagnostics must be blocking");
    assert!(diag.evidence.is_none());
    assert!(diag.expected.is_none());
    assert!(diag.actual.is_none());
    assert!(diag.repair_options.is_empty());
}

// ── Diagnostic::warning constructor ───────────────────────────────────────

#[test]
fn warning_constructor_sets_blocking_false_and_warning_severity() {
    let diag = Diagnostic::warning(E_EFFECT_UNUSED, NodeRef(7));
    assert_eq!(diag.code, E_EFFECT_UNUSED);
    assert_eq!(diag.severity, DiagnosticSeverity::Warning);
    assert_eq!(diag.target, NodeRef(7));
    assert!(
        !diag.blocking,
        "warning diagnostics must not be blocking by default"
    );
}

// ── Builder helpers ───────────────────────────────────────────────────────

#[test]
fn with_evidence_attaches_evidence_string() {
    let diag = Diagnostic::error(E_EFFECT_UNDECLARED, NodeRef(1))
        .with_evidence("effect 'database.write' used but not declared");
    assert_eq!(
        diag.evidence.as_deref(),
        Some("effect 'database.write' used but not declared")
    );
}

#[test]
fn with_expected_attaches_expected_string() {
    let diag = Diagnostic::error(E_TYPE_MISMATCH, NodeRef(2))
        .with_expected("String")
        .with_actual("Int");
    assert_eq!(diag.expected.as_deref(), Some("String"));
    assert_eq!(diag.actual.as_deref(), Some("Int"));
}

#[test]
fn with_repair_appends_repair_option() {
    let diag = Diagnostic::error(E_CONTRACT_VIOLATED, NodeRef(3))
        .with_repair(RepairOption::Explanation("add a requires guard".into()));
    assert_eq!(diag.repair_options.len(), 1);
    assert_eq!(
        diag.repair_options[0],
        RepairOption::Explanation("add a requires guard".into())
    );
}

#[test]
fn multiple_with_repair_calls_accumulate() {
    let diag = Diagnostic::error(E_REFINEMENT_NOT_PROVEN, NodeRef(4))
        .with_repair(RepairOption::DirectOp(ChangeSetOp::Add))
        .with_repair(RepairOption::Explanation("or add a runtime check".into()));
    assert_eq!(diag.repair_options.len(), 2);
}

// ── RepairOption variants ─────────────────────────────────────────────────

#[test]
fn repair_option_direct_op_holds_changeset_op() {
    let r = RepairOption::DirectOp(ChangeSetOp::Set);
    assert_eq!(r, RepairOption::DirectOp(ChangeSetOp::Set));
    assert_ne!(r, RepairOption::DirectOp(ChangeSetOp::Add));
}

#[test]
fn repair_option_choice_holds_vec_of_ops() {
    let ops = vec![ChangeSetOp::Create, ChangeSetOp::Remove];
    let r = RepairOption::Choice(ops.clone());
    assert_eq!(r, RepairOption::Choice(ops));
}

#[test]
fn repair_option_migration_holds_string() {
    let r = RepairOption::Migration("run migration step 3".into());
    assert_eq!(r, RepairOption::Migration("run migration step 3".into()));
}

#[test]
fn repair_option_approval_holds_string() {
    let r = RepairOption::Approval("security team sign-off required".into());
    assert_eq!(
        r,
        RepairOption::Approval("security team sign-off required".into())
    );
}

#[test]
fn repair_option_explanation_holds_string() {
    let r = RepairOption::Explanation("no automated fix available".into());
    assert_eq!(
        r,
        RepairOption::Explanation("no automated fix available".into())
    );
}

// ── CBOR round-trip ───────────────────────────────────────────────────────

#[test]
fn diagnostic_round_trips_via_cbor_minimal() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let diag = Diagnostic::error(E_STALE_BASE, NodeRef(42));
    let mut buf = Vec::new();
    into_writer(&diag, &mut buf).expect("CBOR serialize must succeed");
    let decoded: Diagnostic = from_reader(buf.as_slice()).expect("CBOR deserialize must succeed");

    assert_eq!(decoded.code, E_STALE_BASE);
    assert_eq!(decoded.target, NodeRef(42));
    assert_eq!(decoded.severity, DiagnosticSeverity::Error);
    assert!(decoded.blocking);
    assert!(decoded.evidence.is_none());
    assert!(decoded.repair_options.is_empty());
}

#[test]
fn diagnostic_round_trips_via_cbor_full() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let diag = Diagnostic::error(E_CAPABILITY_DENIED, NodeRef(5))
        .with_evidence("capability 'fs.write' not granted")
        .with_expected("granted in profile")
        .with_actual("not present in capability registry")
        .with_repair(RepairOption::DirectOp(ChangeSetOp::Verify))
        .with_repair(RepairOption::Approval("ops team approval required".into()));

    let mut buf = Vec::new();
    into_writer(&diag, &mut buf).expect("CBOR serialize");
    let decoded: Diagnostic = from_reader(buf.as_slice()).expect("CBOR deserialize");

    assert_eq!(decoded.code, E_CAPABILITY_DENIED);
    assert_eq!(decoded.target, NodeRef(5));
    assert_eq!(
        decoded.evidence.as_deref(),
        Some("capability 'fs.write' not granted")
    );
    assert_eq!(decoded.expected.as_deref(), Some("granted in profile"));
    assert_eq!(
        decoded.actual.as_deref(),
        Some("not present in capability registry")
    );
    assert_eq!(decoded.repair_options.len(), 2);
    assert_eq!(
        decoded.repair_options[0],
        RepairOption::DirectOp(ChangeSetOp::Verify)
    );
    assert_eq!(
        decoded.repair_options[1],
        RepairOption::Approval("ops team approval required".into())
    );
}

#[test]
fn repair_option_choice_round_trips_via_cbor() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let diag =
        Diagnostic::warning(E_EFFECT_UNUSED, NodeRef(9)).with_repair(RepairOption::Choice(vec![
            ChangeSetOp::Remove,
            ChangeSetOp::Set,
        ]));

    let mut buf = Vec::new();
    into_writer(&diag, &mut buf).expect("CBOR serialize");
    let decoded: Diagnostic = from_reader(buf.as_slice()).expect("CBOR deserialize");

    assert_eq!(decoded.repair_options.len(), 1);
    assert_eq!(
        decoded.repair_options[0],
        RepairOption::Choice(vec![ChangeSetOp::Remove, ChangeSetOp::Set])
    );
}
