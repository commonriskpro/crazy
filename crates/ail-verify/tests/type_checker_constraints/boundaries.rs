use super::helpers::*;

#[test]
fn fn_with_params_but_no_return_type_emits_unverified_boundary() {
    use ail_verify::type_checker::E_BOUNDARY_NOT_MATERIALIZED;

    let mut node = fn_node(0, "checkout");
    node.params = Some(vec![ParamDecl {
        name: "cartId".into(),
        ty: "CartId".into(),
    }]);
    // return_type intentionally absent

    let report = TypeChecker::check(&graph_from(vec![node]));

    let boundary_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "boundary-materialization")
        .collect();
    assert!(
        !boundary_entries.is_empty(),
        "expected 'boundary-materialization' entry for fn with params but no return_type"
    );
    let unverified = boundary_entries
        .iter()
        .any(|e| e.state == VerificationState::Unverified);
    assert!(
        unverified,
        "missing return_type must produce Unverified boundary-materialization"
    );
    let has_code = boundary_entries.iter().any(|e| {
        e.evidence
            .as_deref()
            .map(|ev| ev.contains(E_BOUNDARY_NOT_MATERIALIZED))
            .unwrap_or(false)
    });
    assert!(
        has_code,
        "evidence must contain {E_BOUNDARY_NOT_MATERIALIZED}"
    );
}

// TRIANGULATE: Function with both params and return_type → Proven boundary
#[test]
fn fn_with_params_and_return_type_emits_proven_boundary() {
    let mut node = fn_node(0, "load_user");
    node.params = Some(vec![ParamDecl {
        name: "id".into(),
        ty: "UserId".into(),
    }]);
    node.return_type = Some("User".into());

    let report = TypeChecker::check(&graph_from(vec![node]));

    let proven = report
        .entries
        .iter()
        .filter(|e| e.claim == "boundary-materialization")
        .any(|e| e.state == VerificationState::Proven);
    assert!(
        proven,
        "fn with params and return_type must emit Proven boundary-materialization"
    );
}

// TRIANGULATE: Function with no params skips boundary check (no entry)
#[test]
fn fn_with_no_params_skips_boundary_check() {
    let node = fn_node(0, "pure_fn");
    // no params, no return_type

    let report = TypeChecker::check(&graph_from(vec![node]));

    let boundary_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "boundary-materialization")
        .collect();
    assert!(
        boundary_entries.is_empty(),
        "fn with no params must not emit boundary-materialization entries"
    );
}

// ── Subpass 9: Null/absence policy ────────────────────────────────────────

// Spec requirement (Absence, failure, and partial updates):
//   "No null/nil/undefined in Core IR."
//   If a node's return_type is "null", "nil", "undefined", or "void",
//   fail with E_NULL_IN_CORE_IR.
//
// GIVEN a Function node with return_type = "null"
// THEN a "null-policy" entry with state Failed is emitted

#[test]
fn boundary_inference_matching_return_type_is_proven() {
    use ail_verify::type_checker::E_BOUNDARY_INFERENCE_MISMATCH;

    let mut node = fn_node(0, "charge_order");
    node.return_type = Some("Result<OrderId>".into());
    node.inferred = vec![InferredFact {
        kind: "boundary".into(),
        value: "return:Result<OrderId>".into(),
    }];
    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report.entries.iter().any(|e| {
        e.claim == "boundary-inference"
            && e.state == VerificationState::Failed
            && e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_BOUNDARY_INFERENCE_MISMATCH))
                .unwrap_or(false)
    });
    assert!(
        !failed,
        "matching boundary inference must not produce E_BOUNDARY_INFERENCE_MISMATCH"
    );
    let proven = report
        .entries
        .iter()
        .any(|e| e.claim == "boundary-inference" && e.state == VerificationState::Proven);
    assert!(
        proven,
        "matching boundary inference must produce Proven boundary-inference entry"
    );
}

// S-E5b: Function with mismatching boundary inferred_fact → Failed with E_BOUNDARY_INFERENCE_MISMATCH.
#[test]
fn boundary_inference_mismatching_return_type_fails() {
    use ail_verify::type_checker::E_BOUNDARY_INFERENCE_MISMATCH;

    let mut node = fn_node(0, "charge_order");
    node.return_type = Some("Result<PaymentReceipt>".into());
    node.inferred = vec![InferredFact {
        kind: "boundary".into(),
        value: "return:Result<OrderId>".into(),
    }];
    let report = TypeChecker::check(&graph_from(vec![node]));

    let entry = report
        .entries
        .iter()
        .find(|e| e.claim == "boundary-inference" && e.state == VerificationState::Failed);
    assert!(
        entry.is_some(),
        "boundary inference mismatch must produce Failed entry"
    );
    assert!(
        entry
            .unwrap()
            .evidence
            .as_deref()
            .map(|ev| ev.contains(E_BOUNDARY_INFERENCE_MISMATCH))
            .unwrap_or(false),
        "evidence must contain E_BOUNDARY_INFERENCE_MISMATCH"
    );
}

// S-E5c: Function with no inferred_facts → no boundary-inference entry.
// Triangulation: absence of inferred facts means the subpass is skipped.
#[test]
fn function_with_no_inferred_facts_has_no_boundary_inference_entry() {
    let node = fn_node(0, "pure_fn");
    let report = TypeChecker::check(&graph_from(vec![node]));
    let has_entry = report
        .entries
        .iter()
        .any(|e| e.claim == "boundary-inference");
    assert!(
        !has_entry,
        "function with no inferred_facts must not have boundary-inference entry"
    );
}

// ── Task F1: Combined integration scenario ────────────────────────────────
//
// Builds a SemanticGraph with all structural additions from ola3-core-ir-types:
//   - NodeKind::Interface node ("PaymentGateway")
//   - Function with EffectParam generic + effect_row + params + return_type + boundary inferred fact
//   - Type node with RefinementRef (Proven/true) + PatchField<Text> return type
//   - Handler node (Function) with HandlerMeta
//
// Asserts that TypeChecker::check() produces entries covering refinement,
// boundary-materialization, generic-param-kind, patchfield, and boundary-inference
// claims, all with the expected states.
