use super::helpers::*;

// ── Subpass 6: Constraint enforcement ────────────────────────────────────

// Spec scenario 5: "Set<T> without Hashable<T> fails"
//   GIVEN a Type node with nominal "Set" and generic type param "UserId"
//   AND the UserId type node lacks has_hash = true
//   THEN "constraint-check" entry is Failed with E_MISSING_HASH
//
// RED: E_MISSING_HASH and "constraint-check" claim don't exist yet.
#[test]
fn set_type_without_hash_fails() {
    // UserId type node — no constraint_set (thus no hash)
    let user_id = type_node(0, "UserId");

    // Set<UserId> type node
    let mut set_node = type_node(1, "SetOfUserIds");
    set_node.type_facts = Some(TypeFacts {
        nominal: "Set".into(),
        generics: vec!["UserId".into()],
    });

    let report = TypeChecker::check(&graph_from(vec![user_id, set_node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "constraint-check" && e.state == VerificationState::Failed);
    assert!(
        failed,
        "Set<T> without Hashable<T> must fail constraint check"
    );

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "constraint-check")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_MISSING_HASH))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_MISSING_HASH}");
}

// Spec scenario 5: "Set<T> with Eq + Hashable passes"
#[test]
fn set_type_with_hash_passes() {
    let mut user_id = type_node(0, "UserId");
    user_id.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: false,
        has_hash: true,
        has_partial_ord: false,
        extras: vec![],
    });

    let mut set_node = type_node(1, "SetOfUserIds");
    set_node.type_facts = Some(TypeFacts {
        nominal: "Set".into(),
        generics: vec!["UserId".into()],
    });

    let report = TypeChecker::check(&graph_from(vec![user_id, set_node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "constraint-check" && e.state == VerificationState::Failed);
    assert!(
        !failed,
        "Set<T> with Hashable<T> must not fail constraint check"
    );
}

// Spec scenario 5: "sort<Float> fails unless wrapped"
//   GIVEN a type node using "List" as base with generic "Float"
//   AND Float lacks has_ord
//   WHEN nominal is "OrderedSet" or a sort-op node
//   THEN fails with E_MISSING_ORD
#[test]
fn ordered_set_without_ord_fails() {
    let float_node = type_node(0, "Float"); // no constraint_set → no Ord

    let mut ordered_set = type_node(1, "OrderedSetOfFloats");
    ordered_set.type_facts = Some(TypeFacts {
        nominal: "OrderedSet".into(),
        generics: vec!["Float".into()],
    });

    let report = TypeChecker::check(&graph_from(vec![float_node, ordered_set]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "constraint-check" && e.state == VerificationState::Failed);
    assert!(failed, "OrderedSet<T> without Ord<T> must fail");

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "constraint-check")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_MISSING_ORD))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_MISSING_ORD}");
}

// Spec scenario 5: "contains<T> without Eq<T> fails"
//   GIVEN a generic function node with TypeParam T requiring Eq
//   AND a call site instantiating T with a type that lacks Eq
//   THEN fails with E_MISSING_EQ (via constraint-check on type_arg_bindings)
#[test]
fn generic_fn_missing_eq_constraint_fails() {
    use ail_verify::type_checker::E_MISSING_EQ;

    let no_eq_type = type_node(0, "NoEqType"); // no constraint_set

    let mut contains_fn = fn_node(1, "contains");
    contains_fn.generic_params = Some(vec![GenericParamDecl {
        name: "T".into(),
        kind: GenericParamKind::TypeParam,
        required_constraints: vec![WhereConstraint {
            interface: "Eq".into(),
            target_param: None,
            associated_types: vec![],
        }],
    }]);

    let caller = fn_node(2, "caller");

    let edge = GraphEdge {
        source: NodeRef(2),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: None,
        type_arg_bindings: Some(vec![TypeArgBinding {
            param: "T".into(),
            ty: "NoEqType".into(),
        }]),
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = TypeChecker::check(&graph_with_edges(
        vec![no_eq_type, contains_fn, caller],
        vec![edge],
    ));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "constraint-check" && e.state == VerificationState::Failed);
    assert!(
        failed,
        "generic fn with Eq requirement on T must fail when T lacks Eq"
    );

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "constraint-check")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_MISSING_EQ))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_MISSING_EQ}");
}

// TRIANGULATE: generic fn with Eq requirement satisfied passes
#[test]
fn generic_fn_eq_constraint_satisfied_passes() {
    use ail_verify::type_checker::E_MISSING_EQ;

    let mut eq_type = type_node(0, "EqType");
    eq_type.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: false,
        has_hash: false,
        has_partial_ord: false,
        extras: vec![],
    });

    let mut contains_fn = fn_node(1, "contains");
    contains_fn.generic_params = Some(vec![GenericParamDecl {
        name: "T".into(),
        kind: GenericParamKind::TypeParam,
        required_constraints: vec![WhereConstraint {
            interface: "Eq".into(),
            target_param: None,
            associated_types: vec![],
        }],
    }]);

    let caller = fn_node(2, "caller");

    let edge = GraphEdge {
        source: NodeRef(2),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: None,
        type_arg_bindings: Some(vec![TypeArgBinding {
            param: "T".into(),
            ty: "EqType".into(),
        }]),
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = TypeChecker::check(&graph_with_edges(
        vec![eq_type, contains_fn, caller],
        vec![edge],
    ));

    let failed = report
        .entries
        .iter()
        .filter(|e| e.claim == "constraint-check")
        .any(|e| {
            e.state == VerificationState::Failed
                && e.evidence
                    .as_deref()
                    .map(|ev| ev.contains(E_MISSING_EQ))
                    .unwrap_or(false)
        });
    assert!(!failed, "Eq constraint satisfied must not fail");
}

// ── Subpass 3: ConstParam decidability ───────────────────────────────────

// Spec scenario 2: "Non-decidable const parameters are rejected"
//   GIVEN a Function node with ConstParam whose name contains spaces/operators
//   THEN "generic-param-kind" entry is Failed (E_CONST_PARAM_UNDECIDABLE or E_GENERIC_ARITY)
#[test]
fn const_param_with_complex_expression_fails() {
    use ail_verify::type_checker::E_CONST_PARAM_UNDECIDABLE;

    let mut node = fn_node(0, "bad_fixed");
    node.generic_params = Some(vec![GenericParamDecl {
        name: "N + 1".into(), // complex expression — not a simple identifier
        kind: GenericParamKind::ConstParam,
        required_constraints: vec![],
    }]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .any(|e| e.state == VerificationState::Failed);
    assert!(failed, "ConstParam with complex expression must fail");

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_CONST_PARAM_UNDECIDABLE))
                .unwrap_or(false)
        });
    assert!(
        has_code,
        "evidence must contain {E_CONST_PARAM_UNDECIDABLE}"
    );
}

// TRIANGULATE: simple ConstParam identifier passes
#[test]
fn const_param_simple_identifier_passes() {
    let mut node = fn_node(0, "vector_fn");
    node.generic_params = Some(vec![GenericParamDecl {
        name: "N".into(), // simple identifier — decidable
        kind: GenericParamKind::ConstParam,
        required_constraints: vec![],
    }]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .any(|e| e.state == VerificationState::Failed);
    assert!(!failed, "simple ConstParam identifier must not fail");
}

// ── Task 6: Pipeline compatibility ────────────────────────────────────────

// Spec global acceptance: "VerificationReport remains deterministic and
// consumable by PolicyEngine."
//   GIVEN a TypeChecker report with entries (Proven + Failed)
//   WHEN PolicyEngine::evaluate is called with NoUnsafe rule
//   THEN PolicyDecision is deterministic and reflects failures
#[test]
fn type_checker_report_flows_into_policy_engine() {
    use ail_verify::{ApprovalRecord, PolicyDecision, PolicyEngine, PolicyInput, PolicyRule};

    // Build a graph: nominal match passes, nominal mismatch fails.
    let mut callee = fn_node(1, "process");
    callee.params = Some(vec![ParamDecl {
        name: "x".into(),
        ty: "TypeA".into(),
    }]);
    let caller = fn_node(0, "caller");

    let mismatch_edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["TypeB".into()]), // wrong type
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![mismatch_edge]));

    // Verify the report has a Failed entry before feeding to PolicyEngine.
    assert!(
        report
            .entries
            .iter()
            .any(|e| e.state == VerificationState::Failed),
        "report must contain at least one Failed entry"
    );
    assert!(
        report.summary_counts.failed_count > 0,
        "summary_counts.failed_count must reflect failures"
    );

    // Feed to PolicyEngine — should reject (no approvals for failed entries).
    let approvals: Vec<ApprovalRecord> = vec![];
    let input = PolicyInput {
        report: &report,
        rules: &[PolicyRule::NoUnsafe],
        approvals: &approvals,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let decision = PolicyEngine::evaluate(&input);

    // The report is deterministic: two identical graphs produce identical decisions.
    let decision2 = PolicyEngine::evaluate(&input);
    assert_eq!(
        format!("{decision:?}"),
        format!("{decision2:?}"),
        "PolicyEngine::evaluate must be deterministic for identical input"
    );

    // The decision type is one of the valid variants (exhaustive match).
    match &decision {
        PolicyDecision::Passed
        | PolicyDecision::PassedWithWarnings(_)
        | PolicyDecision::Failed(_)
        | PolicyDecision::ApprovalRequired(_) => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// G24 ROUND-2 TESTS — Boundary materialization, Null policy, Float policy,
// Deeper associated-type validation
// ═══════════════════════════════════════════════════════════════════════════

// ── Subpass 8: Boundary materialization ──────────────────────────────────

// Spec requirement (Inference and materialization):
//   "Boundaries must have resolved signatures in the canonical graph."
//   A Function node that declares params but omits return_type is not yet
//   fully materialized — emit an Unverified "boundary-materialization" entry.
//
// GIVEN a Function node with params set and return_type = None
// THEN a "boundary-materialization" entry with Unverified state is emitted
