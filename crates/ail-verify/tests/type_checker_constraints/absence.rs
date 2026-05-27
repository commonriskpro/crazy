use super::helpers::*;

#[test]
fn return_type_null_fails_null_policy() {
    use ail_verify::type_checker::E_NULL_IN_CORE_IR;

    let mut node = fn_node(0, "bad_fn");
    node.params = Some(vec![]);
    node.return_type = Some("null".into());

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "null-policy" && e.state == VerificationState::Failed);
    assert!(failed, "return_type 'null' must fail null-policy check");

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "null-policy")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_NULL_IN_CORE_IR))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_NULL_IN_CORE_IR}");
}

// TRIANGULATE: return_type = "nil" also fails
#[test]
fn return_type_nil_fails_null_policy() {
    let mut node = fn_node(0, "legacy_nil_fn");
    node.params = Some(vec![]);
    node.return_type = Some("nil".into());

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "null-policy" && e.state == VerificationState::Failed);
    assert!(failed, "return_type 'nil' must fail null-policy check");
}

// TRIANGULATE: return_type = "Option<Text>" does NOT fail null-policy
#[test]
fn return_type_option_passes_null_policy() {
    let mut node = fn_node(0, "good_fn");
    node.params = Some(vec![]);
    node.return_type = Some("Option<Text>".into());

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "null-policy" && e.state == VerificationState::Failed);
    assert!(
        !failed,
        "return_type 'Option<Text>' must NOT fail null-policy check"
    );
}

// ── Subpass 10: Float equality/ordering policy ────────────────────────────

// Spec requirement (Equality and ordering):
//   "Float equality requires explicit approximate/bitwise/domain comparator."
//   "Float has no default Ord."
//   If a Type node with nominal "Float" has has_eq=true, that is a violation
//   of the no-implicit-equality rule.
//
// GIVEN a Type node with type_facts.nominal = "Float" and has_eq = true
// THEN a "float-policy" entry with state Failed is emitted (E_FLOAT_EQ_IMPLICIT)
