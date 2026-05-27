use super::helpers::*;

#[test]
fn associated_type_binding_with_empty_ty_fails_coherence() {
    use ail_verify::type_checker::E_ASSOC_TYPE_EMPTY_BINDING;

    let mut node = type_node(0, "BadRepo");
    node.interface_impls = Some(vec![InterfaceImplMeta {
        interface: "cap.Repository".into(),
        associated_types: vec![AssociatedTypeBinding {
            name: "Error".into(),
            ty: "".into(), // empty ty — concrete type not resolved
        }],
        is_adapter: false,
    }]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "coherence" && e.state == VerificationState::Failed);
    assert!(
        failed,
        "empty associated type binding ty must fail coherence"
    );

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "coherence")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_ASSOC_TYPE_EMPTY_BINDING))
                .unwrap_or(false)
        });
    assert!(
        has_code,
        "evidence must contain {E_ASSOC_TYPE_EMPTY_BINDING}"
    );
}

// TRIANGULATE: binding with both name and ty present passes
#[test]
fn associated_type_binding_with_valid_ty_passes() {
    let mut node = type_node(0, "GoodRepo");
    node.interface_impls = Some(vec![InterfaceImplMeta {
        interface: "cap.Repository".into(),
        associated_types: vec![AssociatedTypeBinding {
            name: "Error".into(),
            ty: "DbError".into(), // valid concrete type
        }],
        is_adapter: false,
    }]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "coherence" && e.state == VerificationState::Failed);
    assert!(
        !failed,
        "binding with valid name and ty must not fail coherence"
    );
}

// TRIANGULATE: all-Proven report flows to PolicyEngine as Accept
#[test]
fn all_proven_report_is_accepted_by_policy_engine() {
    use ail_verify::{ApprovalRecord, PolicyEngine, PolicyInput, PolicyRule};

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "safe_fn");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    // Only Proven entries → no unsafe/failed → Accept
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
    assert!(
        matches!(decision, ail_verify::policy::PolicyDecision::Passed),
        "all-Proven report must be Passed by PolicyEngine; got: {decision:?}"
    );
}

// ── Subpass 7: Refinements ───────────────────────────────────────────────

// Spec scenario 6: "Positive-money style predicates can be proven locally"
//   GIVEN a Type node with refinement_ref { status: Proven }
//   THEN "refinement" entry has state Proven
//
// RED: "refinement" claim doesn't exist yet.
#[test]
fn refinement_proven_emits_proven_entry() {
    let mut node = type_node(0, "PositiveMoney");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Decimal".into(),
        predicate: "value > Decimal.zero".into(),
        status: RefinementStatus::Proven,
        erased: false,
    });
    node.contract_clauses = Some(ContractClauses {
        requires: vec![],
        ensures: vec!["value > Decimal.zero".into()],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let refinement_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "refinement")
        .collect();
    assert!(
        !refinement_entries.is_empty(),
        "expected 'refinement' entries"
    );
    assert_eq!(
        refinement_entries[0].state,
        VerificationState::Proven,
        "Proven refinement must produce Proven entry"
    );
}

// Spec scenario 6: "Boundary email decoding is runtime_checked only when a real check exists"
//   GIVEN a Type node with refinement_ref { status: RuntimeChecked }
//   THEN "refinement" entry has state RuntimeChecked
#[test]
fn refinement_runtime_checked_emits_runtime_checked_entry() {
    let mut node = type_node(0, "Email");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Text".into(),
        predicate: "matches_email(value)".into(),
        status: RefinementStatus::RuntimeChecked,
        erased: false,
    });
    node.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "matches_email(value)".into(),
        hash: "email-check".into(),
    }]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let state = report
        .entries
        .iter()
        .filter(|e| e.claim == "refinement")
        .map(|e| e.state)
        .next();
    assert_eq!(
        state,
        Some(VerificationState::RuntimeChecked),
        "RuntimeChecked refinement must produce RuntimeChecked entry"
    );
}

// Spec scenario 6: "Erasure is explicitly reported, never hidden"
//   GIVEN a Type node with refinement_ref { erased: true }
//   THEN an additional "refinement-erasure" entry is emitted
#[test]
fn refinement_erasure_emits_erasure_entry() {
    let mut node = type_node(0, "Email");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Text".into(),
        predicate: "matches_email(value)".into(),
        status: RefinementStatus::Assumed,
        erased: true,
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let erasure_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "refinement-erasure")
        .collect();
    assert!(
        !erasure_entries.is_empty(),
        "erased refinement must emit a 'refinement-erasure' entry"
    );
}

// TRIANGULATE: no erasure → no erasure entry
#[test]
fn no_erasure_produces_no_erasure_entry() {
    let mut node = type_node(0, "PositiveInt");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "value > 0".into(),
        status: RefinementStatus::Proven,
        erased: false,
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let erasure_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "refinement-erasure")
        .collect();
    assert!(
        erasure_entries.is_empty(),
        "non-erased refinement must not emit 'refinement-erasure' entry"
    );
}

// Spec scenario 6: "Unproven refinements fail or remain unverified per policy"
//   GIVEN a Type node with refinement_ref { status: Failed }
//   THEN "refinement" entry has state Failed
#[test]
fn refinement_failed_emits_failed_entry() {
    let mut node = type_node(0, "BadRefinement");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "contradictory_predicate(value)".into(),
        status: RefinementStatus::Failed,
        erased: false,
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let state = report
        .entries
        .iter()
        .filter(|e| e.claim == "refinement")
        .map(|e| e.state)
        .next();
    assert_eq!(
        state,
        Some(VerificationState::Failed),
        "Failed refinement must produce Failed entry"
    );
}

// ── Task E1 (RED): PatchField validation subpass ──────────────────────────

// S-E1a: Type node with valid PatchField<Text> return type → Proven.
#[test]
fn patch_field_with_non_empty_inner_type_is_proven() {
    use ail_verify::type_checker::E_PATCHFIELD_EMPTY_INNER;

    let mut node = type_node(0, "UpdateUsername");
    node.return_type = Some("PatchField<Text>".into());
    let report = TypeChecker::check(&graph_from(vec![node]));

    // The patchfield subpass must not produce a Failed entry for a valid PatchField.
    let failed = report.entries.iter().any(|e| {
        e.claim == "patchfield"
            && e.state == VerificationState::Failed
            && e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_PATCHFIELD_EMPTY_INNER))
                .unwrap_or(false)
    });
    assert!(
        !failed,
        "PatchField<Text> must not emit E_PATCHFIELD_EMPTY_INNER"
    );
}

// S-E1b: Type node with empty inner PatchField<> → Failed with E_PATCHFIELD_EMPTY_INNER.
#[test]
fn patch_field_with_empty_inner_type_fails() {
    use ail_verify::type_checker::E_PATCHFIELD_EMPTY_INNER;

    let mut node = type_node(0, "BadPatch");
    node.return_type = Some("PatchField<>".into());
    let report = TypeChecker::check(&graph_from(vec![node]));

    let entry = report
        .entries
        .iter()
        .find(|e| e.claim == "patchfield" && e.state == VerificationState::Failed);
    assert!(
        entry.is_some(),
        "PatchField<> must produce a Failed patchfield entry"
    );
    assert!(
        entry
            .unwrap()
            .evidence
            .as_deref()
            .map(|ev| ev.contains(E_PATCHFIELD_EMPTY_INNER))
            .unwrap_or(false),
        "evidence must contain E_PATCHFIELD_EMPTY_INNER"
    );
}

// S-E1c: Nodes without PatchField return type are not affected.
// Triangulation: no false positives.
#[test]
fn non_patchfield_return_type_is_not_checked() {
    let mut node = fn_node(0, "load_user");
    node.return_type = Some("Result<User, DbError>".into());
    let report = TypeChecker::check(&graph_from(vec![node]));
    let has_patchfield_entry = report.entries.iter().any(|e| e.claim == "patchfield");
    assert!(
        !has_patchfield_entry,
        "non-PatchField node must not have patchfield entry"
    );
}

// ── Task E3 (RED): PartialOrd validation subpass ──────────────────────────

// S-E3a: Type node with has_partial_ord=true in partial-order context → Proven.

#[test]
fn combined_scenario_all_structural_additions_produce_expected_entries() {
    use ail_verify::type_checker::{E_BOUNDARY_INFERENCE_MISMATCH, E_PATCHFIELD_EMPTY_INNER};

    // Node 0 — Interface node: no type-check entries expected (not Function/Type).
    let interface_node = GraphNode::new(NodeRef(0), NodeKind::Interface, "PaymentGateway");

    // Node 1 — Function with EffectParam generic, effect_row, params, return_type,
    //           and a matching boundary inferred fact.
    let mut fn_generic = GraphNode::new(NodeRef(1), NodeKind::Function, "process_payment");
    fn_generic.generic_params = Some(vec![GenericParamDecl {
        name: "db".into(),
        kind: GenericParamKind::EffectParam,
        required_constraints: vec![],
    }]);
    fn_generic.effect_row = Some(EffectRow {
        effects: vec!["db".into()],
    });
    fn_generic.params = Some(vec![ParamDecl {
        name: "order_id".into(),
        ty: "OrderId".into(),
    }]);
    fn_generic.return_type = Some("Result<OrderId>".into());
    fn_generic.inferred = vec![InferredFact {
        kind: "boundary".into(),
        value: "return:Result<OrderId>".into(),
    }];

    // Node 2 — Type with Proven refinement (predicate "true") and PatchField<Text> return.
    let mut patch_type = GraphNode::new(NodeRef(2), NodeKind::Type, "PatchOrderDetails");
    patch_type.refinement_ref = Some(RefinementRef {
        base_type: "Text".into(),
        predicate: "true".into(),
        status: RefinementStatus::Proven,
        erased: false,
    });
    patch_type.return_type = Some("PatchField<Text>".into());

    // Node 3 — Function acting as a capability handler with HandlerMeta.
    let mut handler = GraphNode::new(NodeRef(3), NodeKind::Function, "StripeHandler");
    handler.handler_meta = Some(HandlerMeta {
        handled_caps: vec!["database.read".into()],
        internal_effects: vec![],
        satisfies_contract: None,
    });

    let graph = SemanticGraph {
        nodes: vec![interface_node, fn_generic, patch_type, handler],
        edges: vec![],
    };

    let report = TypeChecker::check(&graph);

    // 1. generic-param-kind: EffectParam "db" is in the effect_row → Proven.
    let generic_proven = report
        .entries
        .iter()
        .any(|e| e.claim == "generic-param-kind" && e.state == VerificationState::Proven);
    assert!(
        generic_proven,
        "EffectParam 'db' in effect_row must produce Proven generic-param-kind entry"
    );

    // 2. boundary-materialization: process_payment has params + return_type → Proven.
    let boundary_mat_proven = report
        .entries
        .iter()
        .any(|e| e.claim == "boundary-materialization" && e.state == VerificationState::Proven);
    assert!(
        boundary_mat_proven,
        "function with params + return_type must produce Proven boundary-materialization entry"
    );

    // 3. refinement: PatchOrderDetails has predicate "true" + status Proven → Proven.
    let refinement_proven = report
        .entries
        .iter()
        .any(|e| e.claim == "refinement" && e.state == VerificationState::Proven);
    assert!(
        refinement_proven,
        "refinement with status Proven and predicate 'true' must produce Proven refinement entry"
    );

    // 4. patchfield: PatchField<Text> inner type is non-empty → Proven.
    let patchfield_proven = report
        .entries
        .iter()
        .any(|e| e.claim == "patchfield" && e.state == VerificationState::Proven);
    assert!(
        patchfield_proven,
        "PatchField<Text> must produce Proven patchfield entry"
    );
    let patchfield_failed = report.entries.iter().any(|e| {
        e.claim == "patchfield"
            && e.state == VerificationState::Failed
            && e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_PATCHFIELD_EMPTY_INNER))
                .unwrap_or(false)
    });
    assert!(
        !patchfield_failed,
        "PatchField<Text> must not produce E_PATCHFIELD_EMPTY_INNER"
    );

    // 5. boundary-inference: inferred "return:Result<OrderId>" matches declared return_type → Proven.
    let bi_proven = report
        .entries
        .iter()
        .any(|e| e.claim == "boundary-inference" && e.state == VerificationState::Proven);
    assert!(
        bi_proven,
        "matching boundary inferred fact must produce Proven boundary-inference entry"
    );
    let bi_failed = report.entries.iter().any(|e| {
        e.claim == "boundary-inference"
            && e.state == VerificationState::Failed
            && e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_BOUNDARY_INFERENCE_MISMATCH))
                .unwrap_or(false)
    });
    assert!(
        !bi_failed,
        "matching boundary inference must not produce E_BOUNDARY_INFERENCE_MISMATCH"
    );
}
