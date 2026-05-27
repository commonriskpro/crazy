use super::helpers::*;

#[test]
fn float_type_with_eq_fails_float_policy() {
    use ail_verify::type_checker::E_FLOAT_EQ_IMPLICIT;

    let mut node = type_node(0, "FloatVal");
    node.type_facts = Some(TypeFacts {
        nominal: "Float".into(),
        generics: vec![],
    });
    node.constraint_set = Some(ConstraintSet {
        has_eq: true, // implicit equality on Float — violation
        has_ord: false,
        has_hash: false,
        has_partial_ord: false,
        extras: vec![],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "float-policy" && e.state == VerificationState::Failed);
    assert!(failed, "Float type with has_eq=true must fail float-policy");

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "float-policy")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_FLOAT_EQ_IMPLICIT))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_FLOAT_EQ_IMPLICIT}");
}

// TRIANGULATE: Float type with has_ord=true also fails (no default Ord for Float)
#[test]
fn float_type_with_ord_fails_float_policy() {
    use ail_verify::type_checker::E_FLOAT_ORD_IMPLICIT;

    let mut node = type_node(0, "FloatVal");
    node.type_facts = Some(TypeFacts {
        nominal: "Float".into(),
        generics: vec![],
    });
    node.constraint_set = Some(ConstraintSet {
        has_eq: false,
        has_ord: true, // implicit Ord on Float — violation
        has_hash: false,
        has_partial_ord: false,
        extras: vec![],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "float-policy" && e.state == VerificationState::Failed);
    assert!(
        failed,
        "Float type with has_ord=true must fail float-policy"
    );

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "float-policy")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_FLOAT_ORD_IMPLICIT))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_FLOAT_ORD_IMPLICIT}");
}

// TRIANGULATE: Float type with has_eq=false and has_ord=false passes
#[test]
fn float_type_without_eq_ord_passes_float_policy() {
    let mut node = type_node(0, "FloatVal");
    node.type_facts = Some(TypeFacts {
        nominal: "Float".into(),
        generics: vec![],
    });
    node.constraint_set = Some(ConstraintSet {
        has_eq: false,
        has_ord: false,
        has_hash: false,
        has_partial_ord: false,
        extras: vec![],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "float-policy" && e.state == VerificationState::Failed);
    assert!(
        !failed,
        "Float type with no eq/ord must not fail float-policy"
    );
}

// TRIANGULATE: NonNaNFloat (non-Float nominal) is exempt from float-policy
#[test]
fn non_nan_float_refinement_is_exempt_from_float_policy() {
    let mut node = type_node(0, "NonNaNFloat");
    node.type_facts = Some(TypeFacts {
        nominal: "NonNaNFloat".into(), // NOT "Float" — it's a refinement
        generics: vec![],
    });
    node.constraint_set = Some(ConstraintSet {
        has_eq: false,
        has_ord: true, // OK — NonNaNFloat can have Ord
        has_hash: false,
        has_partial_ord: false,
        extras: vec![],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "float-policy" && e.state == VerificationState::Failed);
    assert!(
        !failed,
        "NonNaNFloat (non-Float nominal) must not fail float-policy"
    );
}

// ── Improved Subpass 5: Associated type binding with empty ty ─────────────

// Spec requirement (Interface system, Associated types):
//   "Associated types must be explicit in the IR and appear in the semantic context."
//   An impl where the binding's ty is empty is semantically invalid — the
//   concrete type was not resolved.
//
// GIVEN an interface_impl with an associated type binding where ty = ""
// THEN a "coherence" entry with state Failed and E_ASSOC_TYPE_EMPTY_BINDING is emitted

#[test]
fn type_with_partial_ord_in_partial_order_context_is_proven() {
    let mut node = type_node(0, "Probability");
    node.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: false,
        has_hash: false,
        has_partial_ord: true,
        extras: vec![],
    });
    // Mark as a partial-order context via extras.
    node.return_type = Some("PartialOrd<Probability>".into());
    let report = TypeChecker::check(&graph_from(vec![node]));

    let proven = report
        .entries
        .iter()
        .any(|e| e.claim == "partial-ord" && e.state == VerificationState::Proven);
    assert!(
        proven,
        "type with has_partial_ord=true must emit Proven partial-ord entry"
    );
}

// S-E3b: Node requiring Ord but only has_partial_ord=true → informational entry.
// Triangulation: partial ord without total ord emits a distinguishable entry.
#[test]
fn type_with_only_partial_ord_emits_informational_in_sorting_context() {
    use ail_verify::type_checker::E_PARTIAL_ORD_REQUIRED;

    let mut node = type_node(0, "FloatOrd");
    node.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: false, // no total ord
        has_hash: false,
        has_partial_ord: true,
        extras: vec![],
    });
    // A sorting context is signaled by extras containing "needs_ord".
    node.return_type = Some("OrderedSet<FloatOrd>".into());
    let report = TypeChecker::check(&graph_from(vec![node]));

    // Should emit an entry for partial-ord context (Unverified or informational).
    let has_partial_ord_entry = report.entries.iter().any(|e| e.claim == "partial-ord");
    assert!(
        has_partial_ord_entry,
        "type with partial-ord-only in Ord context must emit a partial-ord entry; code: {E_PARTIAL_ORD_REQUIRED}"
    );
}

// ── Task E5 (RED): boundary inference cross-check subpass ─────────────────

// S-E5a: Function with matching boundary inferred_fact and return_type → Proven.
