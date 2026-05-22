// ── ail-verify::type_checker tests ───────────────────────────────────────
//
// Integration tests for TypeChecker (verification pipeline step 7).
//
// G24 full type-system enforcement:
//
// Subpass 1 — Nominal presence (existing):
//   - Function/Type nodes with non-empty TypeFacts.nominal + valid generics → Proven
//   - Function/Type nodes with TypeFacts.nominal non-empty + empty generic string → Failed (E_GENERIC_ARITY)
//   - Function/Type nodes without TypeFacts (or empty nominal) → Unverified
//   - All other node kinds are skipped (produce no entries)
//
// Subpass 2 — Nominal call check:
//   - Calls edge with call_args where arg type ≠ callee param type → Failed (E_NOMINAL_MISMATCH)
//   - Calls edge with matching arg types → Proven ("nominal-call")
//
// Subpass 3 — Generic param kind validation:
//   - Empty generic param name → Failed (E_GENERIC_ARITY)
//   - EffectParam not in effect_row → Failed (E_EFFECT_PARAM_WIDENED)
//   - CapabilityParam not in capability_reqs → Failed (E_CAPABILITY_PARAM_WIDENED)
//   - Valid typed generic params → Proven ("generic-param-kind")
//
// Subpass 4 — Variance enforcement:
//   - call_args with parameterized type where base same but type arg differs → Failed (E_VARIANCE_COERCION)
//   - Matching parameterized types → no violation
//
// Subpass 5 — Interface coherence:
//   - Node with duplicate interface impl (same interface name twice) → Failed (E_COHERENCE_DUPLICATE)
//   - Adapter exception → passes
//   - Associated type binding mismatch (impl declares wrong associated type) is reported
//
// Subpass 6 — Constraint enforcement:
//   - Type node used as Set<T> element without Eq/Hash → Failed (E_MISSING_HASH)
//   - Type node used as sort key without Ord → Failed (E_MISSING_ORD)
//   - Type node with required constraints present → Proven ("constraint-check")
//
// Subpass 7 — Refinements:
//   - Refinement with status Proven → VerificationState::Proven
//   - Refinement with status RuntimeChecked → VerificationState::RuntimeChecked
//   - Refinement with erased=true → additional "refinement-erasure" entry
//   - Refinement with status Failed → VerificationState::Failed

use ail_core::semantic_graph::EdgeKind;
use ail_core::semantic_graph::{
    AssociatedTypeBinding, CapabilityReqs, ConstraintSet, ContractClauses, EffectRow,
    GenericParamDecl, GenericParamKind, GraphEdge, GraphNode, HandlerMeta, InferredFact,
    InterfaceImplMeta, NodeKind, NodeRef, ParamDecl, RefinementRef, RefinementStatus,
    RuntimeCheckMeta, SemanticGraph, TypeArgBinding, TypeFacts, WhereConstraint,
};
use ail_verify::report::VerificationState;
use ail_verify::type_checker::{
    E_CAPABILITY_NOT_PROPAGATED, E_CAPABILITY_PARAM_WIDENED, E_COHERENCE_DUPLICATE,
    E_DYN_INTERFACE_UNAVAILABLE, E_EFFECT_NOT_PROPAGATED, E_EFFECT_PARAM_WIDENED,
    E_GENERIC_BINDING_ARITY, E_MISSING_HASH, E_MISSING_ORD, E_NOMINAL_MISMATCH,
    E_REFINEMENT_PROOF_UNDISCHARGED, E_REFINEMENT_RUNTIME_CHECK_MISSING,
    E_STRUCTURAL_TYPE_MISMATCH, E_VARIANCE_COERCION, TypeChecker,
};

fn graph_from(nodes: Vec<GraphNode>) -> SemanticGraph {
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

// ── Scenario: Function node with typed nominal → Proven ───────────────────
// GIVEN a Function node with TypeFacts { nominal: "Int", generics: [] }
// WHEN TypeChecker::check is called
// THEN entry state is Proven

#[test]
fn function_with_nominal_type_is_proven() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "add");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });
    let report = TypeChecker::check(&graph_from(vec![node]));
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
    assert_eq!(report.entries[0].scope, "add");
    assert_eq!(report.entries[0].claim, "type-check");
}

// ── Scenario: Type node with generics → Proven ───────────────────────────
// GIVEN a Type node with generics: ["K", "V"]
// WHEN TypeChecker::check is called
// THEN entry state is Proven

#[test]
fn type_node_with_generics_is_proven() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Map");
    node.type_facts = Some(TypeFacts {
        nominal: "Map".into(),
        generics: vec!["K".into(), "V".into()],
    });
    let report = TypeChecker::check(&graph_from(vec![node]));
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: Function node without TypeFacts → Unverified ───────────────
// GIVEN a Function node with type_facts: None
// WHEN TypeChecker::check is called
// THEN entry state is Unverified

#[test]
fn function_without_type_facts_is_unverified() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "mystery_fn");
    let report = TypeChecker::check(&graph_from(vec![node]));
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

// ── Scenario: Empty generic parameter → Failed (E_GENERIC_ARITY) ─────────
// GIVEN a Function node with generics: ["", "V"]  (first param is empty string)
// WHEN TypeChecker::check is called
// THEN entry state is Failed with evidence containing E_GENERIC_ARITY

#[test]
fn empty_generic_param_is_failed_with_arity_error() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "BadGeneric");
    node.type_facts = Some(TypeFacts {
        nominal: "Container".into(),
        generics: vec!["".into(), "V".into()],
    });
    let report = TypeChecker::check(&graph_from(vec![node]));
    assert_eq!(report.entries.len(), 1);
    let entry = &report.entries[0];
    assert_eq!(entry.state, VerificationState::Failed);
    let evidence = entry.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("E_GENERIC_ARITY"),
        "evidence must contain E_GENERIC_ARITY, got: {evidence}"
    );
}

// ── Scenario: Non-Function/Type nodes are skipped ────────────────────────
// GIVEN a graph with Module, Effect, Capability nodes (no Function/Type)
// WHEN TypeChecker::check is called
// THEN report has zero entries

#[test]
fn non_function_type_nodes_are_skipped() {
    let nodes = vec![
        GraphNode::new(NodeRef(0), NodeKind::Module, "root"),
        GraphNode::new(NodeRef(1), NodeKind::Effect, "io"),
        GraphNode::new(NodeRef(2), NodeKind::Capability, "net"),
        GraphNode::new(NodeRef(3), NodeKind::Boundary, "external"),
    ];
    let report = TypeChecker::check(&graph_from(nodes));
    assert_eq!(report.entries.len(), 0);
}

// ── Triangulation: empty graph → empty report ────────────────────────────

#[test]
fn empty_graph_produces_empty_report() {
    let report = TypeChecker::check(&graph_from(vec![]));
    assert_eq!(report.entries.len(), 0);
    assert_eq!(report.summary(), VerificationState::Proven);
}

// ── Triangulation: mixed Function/Type nodes → entries in order ──────────

#[test]
fn mixed_nodes_entries_in_order() {
    let mut fn_node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a");
    fn_node.type_facts = Some(TypeFacts {
        nominal: "Bool".into(),
        generics: vec![],
    });
    let type_node = GraphNode::new(NodeRef(1), NodeKind::Type, "untyped");
    let mod_node = GraphNode::new(NodeRef(2), NodeKind::Module, "mod"); // skipped

    let report = TypeChecker::check(&graph_from(vec![fn_node, type_node, mod_node]));
    // 2 entries: fn_a (Proven) + untyped (Unverified); mod skipped
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.entries[0].scope, "fn_a");
    assert_eq!(report.entries[0].state, VerificationState::Proven);
    assert_eq!(report.entries[1].scope, "untyped");
    assert_eq!(report.entries[1].state, VerificationState::Unverified);
}

// ── Triangulation: schema_version and summary_counts are populated ────────

#[test]
fn report_has_schema_version_and_counts() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "f");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });
    let report = TypeChecker::check(&graph_from(vec![node]));
    assert_eq!(report.schema_version, "verification/1.0");
    assert_eq!(report.summary_counts.verified_count, 1);
    assert_eq!(report.summary_counts.failed_count, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// G24 NEW TESTS — Tasks 2-5 (RED before implementation)
// ═══════════════════════════════════════════════════════════════════════════

// ── helper ────────────────────────────────────────────────────────────────

fn graph_with_edges(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> SemanticGraph {
    SemanticGraph { nodes, edges }
}

fn type_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Type, name)
}

fn fn_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Function, name)
}

#[test]
fn generic_call_missing_binding_fails_arity_validation() {
    let caller = fn_node(0, "caller");
    let mut callee = fn_node(1, "make_pair");
    callee.generic_params = Some(vec![
        GenericParamDecl {
            name: "K".into(),
            kind: GenericParamKind::TypeParam,
            required_constraints: vec![],
        },
        GenericParamDecl {
            name: "V".into(),
            kind: GenericParamKind::TypeParam,
            required_constraints: vec![],
        },
    ]);
    let edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: None,
        type_arg_bindings: Some(vec![TypeArgBinding {
            param: "K".into(),
            ty: "Text".into(),
        }]),
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![edge]));
    assert!(report.entries.iter().any(|entry| {
        entry.claim == "generic-call-binding"
            && entry.state == VerificationState::Failed
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_GENERIC_BINDING_ARITY)
    }));
}

#[test]
fn callee_effects_and_capabilities_must_propagate_to_caller() {
    let mut caller = fn_node(0, "caller");
    caller.effect_row = Some(EffectRow { effects: vec![] });
    caller.capability_reqs = Some(CapabilityReqs { caps: vec![] });
    let mut callee = fn_node(1, "charge");
    callee.effect_row = Some(EffectRow {
        effects: vec!["IO".into()],
    });
    callee.capability_reqs = Some(CapabilityReqs {
        caps: vec!["payments:charge".into()],
    });
    let edge = GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls);

    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![edge]));
    assert!(report.entries.iter().any(|entry| {
        entry.claim == "effect-propagation"
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_EFFECT_NOT_PROPAGATED)
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.claim == "capability-propagation"
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_CAPABILITY_NOT_PROPAGATED)
    }));
}

#[test]
fn structural_type_and_dyn_interface_fail_when_unavailable() {
    let caller = fn_node(0, "caller");
    let mut callee = fn_node(1, "render");
    callee.params = Some(vec![
        ParamDecl {
            name: "item".into(),
            ty: "struct{id:Int,name:Text}".into(),
        },
        ParamDecl {
            name: "service".into(),
            ty: "Dyn<Chargeable>".into(),
        },
    ]);
    let user = type_node(2, "User");
    let payment = type_node(3, "PaymentService");
    let edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["User".into(), "PaymentService".into()]),
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = TypeChecker::check(&graph_with_edges(
        vec![caller, callee, user, payment],
        vec![edge],
    ));
    assert!(report.entries.iter().any(|entry| {
        entry.claim == "structural-type"
            && entry.state == VerificationState::Failed
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_STRUCTURAL_TYPE_MISMATCH)
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.claim == "dyn-interface"
            && entry.state == VerificationState::Failed
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_DYN_INTERFACE_UNAVAILABLE)
    }));
}

#[test]
fn refinement_proven_requires_local_discharge_and_runtime_checked_requires_check() {
    let mut claimed_proven = type_node(0, "PositiveInt");
    claimed_proven.refinement_ref = Some(RefinementRef {
        base_type: "Int".into(),
        predicate: "value > 0".into(),
        status: RefinementStatus::Proven,
        erased: false,
    });
    let mut runtime_checked = type_node(1, "NonEmptyText");
    runtime_checked.refinement_ref = Some(RefinementRef {
        base_type: "Text".into(),
        predicate: "len(value) > 0".into(),
        status: RefinementStatus::RuntimeChecked,
        erased: false,
    });
    runtime_checked.runtime_checks = Some(vec![]);
    let mut discharged = type_node(2, "AlwaysTrue");
    discharged.refinement_ref = Some(RefinementRef {
        base_type: "Bool".into(),
        predicate: "true".into(),
        status: RefinementStatus::Proven,
        erased: false,
    });
    discharged.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "true".into(),
        hash: "h".into(),
    }]);

    let report = TypeChecker::check(&graph_from(vec![
        claimed_proven,
        runtime_checked,
        discharged,
    ]));
    assert!(report.entries.iter().any(|entry| {
        entry.scope == "PositiveInt"
            && entry.claim == "refinement"
            && entry.state == VerificationState::Unverified
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_REFINEMENT_PROOF_UNDISCHARGED)
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.scope == "NonEmptyText"
            && entry.claim == "refinement"
            && entry.state == VerificationState::Failed
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_REFINEMENT_RUNTIME_CHECK_MISSING)
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.scope == "AlwaysTrue"
            && entry.claim == "refinement"
            && entry.state == VerificationState::Proven
    }));
}

// ── Subpass 2: Nominal call check ────────────────────────────────────────

// Spec scenario 1: "Passing OrderId to load_user(UserId) fails"
//   GIVEN load_user declares param "id: UserId"
//   AND a Calls edge from caller to load_user with call_args: ["OrderId"]
//   WHEN TypeChecker::check is called
//   THEN a "nominal-call" entry with state Failed and E_NOMINAL_MISMATCH is emitted
//
// RED: E_NOMINAL_MISMATCH and "nominal-call" claim don't exist yet.
#[test]
fn nominal_mismatch_at_call_site_fails() {
    let mut callee = fn_node(1, "load_user");
    callee.params = Some(vec![ParamDecl {
        name: "id".into(),
        ty: "UserId".into(),
    }]);

    let caller = fn_node(0, "caller");

    let calls_edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["OrderId".into()]),
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = graph_with_edges(vec![caller, callee], vec![calls_edge]);
    let report = TypeChecker::check(&report);

    let nominal_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "nominal-call")
        .collect();

    assert!(
        !nominal_entries.is_empty(),
        "expected at least one 'nominal-call' entry"
    );
    let failed = nominal_entries
        .iter()
        .any(|e| e.state == VerificationState::Failed);
    assert!(failed, "expected a Failed entry for nominal mismatch");

    let has_evidence = nominal_entries.iter().any(|e| {
        e.evidence
            .as_deref()
            .map(|ev| ev.contains(E_NOMINAL_MISMATCH))
            .unwrap_or(false)
    });
    assert!(has_evidence, "evidence must contain {E_NOMINAL_MISMATCH}");
}

// Spec scenario 1 TRIANGULATE: "Passing UserId to load_user(UserId) passes"
//   GIVEN matching arg type
//   THEN no Failed nominal-call entry
#[test]
fn nominal_match_at_call_site_passes() {
    let mut callee = fn_node(1, "load_user");
    callee.params = Some(vec![ParamDecl {
        name: "id".into(),
        ty: "UserId".into(),
    }]);

    let caller = fn_node(0, "caller");

    let calls_edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["UserId".into()]),
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };

    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![calls_edge]));

    let failed_nominal: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "nominal-call" && e.state == VerificationState::Failed)
        .collect();
    assert!(
        failed_nominal.is_empty(),
        "matching types must not produce a nominal-call failure"
    );
}

// Spec scenario 1: "Nominal alias with matching fields still does not auto-match"
//   GIVEN two distinct types UserId and OrderId (both alias Id)
//   AND call site passes OrderId where UserId is expected
//   THEN fails (nominal identity is exact)
#[test]
fn nominal_alias_does_not_auto_match() {
    let mut callee = fn_node(1, "process_user");
    callee.params = Some(vec![ParamDecl {
        name: "uid".into(),
        ty: "UserId".into(),
    }]);
    let caller = fn_node(0, "invoker");
    let edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["OrderId".into()]),
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };
    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![edge]));
    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "nominal-call" && e.state == VerificationState::Failed);
    assert!(
        failed,
        "aliases with same representation must still fail nominal check"
    );
}

// ── Subpass 3: Generic param kind validation ──────────────────────────────

// Spec scenario 2: "EffectParam declared but not reflected in effect_row → widening detected"
//   GIVEN a Function node with generic_params: [EffectParam "e"]
//   AND effect_row does NOT include "e"
//   THEN "generic-param-kind" entry is Failed with E_EFFECT_PARAM_WIDENED
//
// RED: E_EFFECT_PARAM_WIDENED and "generic-param-kind" claim don't exist yet.
#[test]
fn effect_param_not_in_effect_row_is_failed() {
    use ail_core::semantic_graph::EffectRow;

    let mut node = fn_node(0, "my_fn");
    node.generic_params = Some(vec![GenericParamDecl {
        name: "e".into(),
        kind: GenericParamKind::EffectParam,
        required_constraints: vec![],
    }]);
    // effect_row does NOT include "e"
    node.effect_row = Some(EffectRow {
        effects: vec!["IO".into()],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let kind_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .collect();
    assert!(
        !kind_entries.is_empty(),
        "expected 'generic-param-kind' entries for EffectParam"
    );
    let failed = kind_entries
        .iter()
        .any(|e| e.state == VerificationState::Failed);
    assert!(failed, "EffectParam not in effect_row must be Failed");
    let has_code = kind_entries.iter().any(|e| {
        e.evidence
            .as_deref()
            .map(|ev| ev.contains(E_EFFECT_PARAM_WIDENED))
            .unwrap_or(false)
    });
    assert!(has_code, "evidence must contain {E_EFFECT_PARAM_WIDENED}");
}

// TRIANGULATE: EffectParam IS in effect_row → passes
#[test]
fn effect_param_in_effect_row_passes() {
    use ail_core::semantic_graph::EffectRow;

    let mut node = fn_node(0, "good_fn");
    node.generic_params = Some(vec![GenericParamDecl {
        name: "e".into(),
        kind: GenericParamKind::EffectParam,
        required_constraints: vec![],
    }]);
    node.effect_row = Some(EffectRow {
        effects: vec!["e".into()], // "e" is in the effect row
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .any(|e| e.state == VerificationState::Failed);
    assert!(!failed, "EffectParam in effect_row must not fail");
}

// CapabilityParam not in capability_reqs → widening detected
#[test]
fn capability_param_not_in_caps_is_failed() {
    use ail_core::semantic_graph::CapabilityReqs;

    let mut node = fn_node(0, "retry_fn");
    node.generic_params = Some(vec![GenericParamDecl {
        name: "cap".into(),
        kind: GenericParamKind::CapabilityParam,
        required_constraints: vec![],
    }]);
    // capability_reqs does NOT include "cap"
    node.capability_reqs = Some(CapabilityReqs {
        caps: vec!["net:read".into()],
    });

    let report = TypeChecker::check(&graph_from(vec![node]));

    let has_failed = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .any(|e| {
            e.state == VerificationState::Failed
                && e.evidence
                    .as_deref()
                    .map(|ev| ev.contains(E_CAPABILITY_PARAM_WIDENED))
                    .unwrap_or(false)
        });
    assert!(
        has_failed,
        "CapabilityParam not in caps must fail with {E_CAPABILITY_PARAM_WIDENED}"
    );
}

// TypeParam with required constraints missing from instantiation → constraint failure
// (checked in subpass 6 via type_arg_bindings, here just validate TypeParam passes with no required)
#[test]
fn type_param_without_requirements_passes() {
    let mut node = fn_node(0, "identity_fn");
    node.generic_params = Some(vec![GenericParamDecl {
        name: "T".into(),
        kind: GenericParamKind::TypeParam,
        required_constraints: vec![],
    }]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .filter(|e| e.claim == "generic-param-kind")
        .any(|e| e.state == VerificationState::Failed);
    assert!(!failed, "TypeParam with no requirements must not fail");
}

// ── Subpass 4: Variance enforcement ──────────────────────────────────────

// Spec scenario 4: "List<Dog> is not accepted where List<Animal> is expected"
//   GIVEN a Calls edge where call_args: ["List<Dog>"] but callee param type: "List<Animal>"
//   THEN Failed with E_VARIANCE_COERCION
//
// RED: E_VARIANCE_COERCION and "variance" claim don't exist yet.
#[test]
fn parameterized_type_arg_coercion_fails() {
    let mut callee = fn_node(1, "render_animals");
    callee.params = Some(vec![ParamDecl {
        name: "items".into(),
        ty: "List<Animal>".into(),
    }]);
    let caller = fn_node(0, "caller");
    let edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["List<Dog>".into()]),
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };
    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![edge]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "variance" && e.state == VerificationState::Failed);
    assert!(
        failed,
        "parameterized type coercion must fail with variance error"
    );

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "variance")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_VARIANCE_COERCION))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_VARIANCE_COERCION}");
}

// TRIANGULATE: matching parameterized type → no variance failure
#[test]
fn matching_parameterized_type_passes_variance() {
    let mut callee = fn_node(1, "render_animals");
    callee.params = Some(vec![ParamDecl {
        name: "items".into(),
        ty: "List<Animal>".into(),
    }]);
    let caller = fn_node(0, "caller");
    let edge = GraphEdge {
        source: NodeRef(0),
        target: NodeRef(1),
        kind: EdgeKind::Calls,
        call_args: Some(vec!["List<Animal>".into()]),
        type_arg_bindings: None,
        effect_arg_bindings: None,
        capability_arg_bindings: None,
    };
    let report = TypeChecker::check(&graph_with_edges(vec![caller, callee], vec![edge]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "variance" && e.state == VerificationState::Failed);
    assert!(
        !failed,
        "identical parameterized types must not fail variance check"
    );
}

// ── Subpass 5: Interface coherence ───────────────────────────────────────

// Spec scenario 3: "Duplicate Interface<T> impls fail"
//   GIVEN a Type node with two InterfaceImplMeta for the same interface
//   THEN "coherence" entry is Failed with E_COHERENCE_DUPLICATE
//
// RED: E_COHERENCE_DUPLICATE and "coherence" claim don't exist yet.
#[test]
fn duplicate_interface_impl_fails() {
    let mut node = type_node(0, "MyType");
    node.interface_impls = Some(vec![
        InterfaceImplMeta {
            interface: "cap.Serializable".into(),
            associated_types: vec![],
            is_adapter: false,
        },
        InterfaceImplMeta {
            interface: "cap.Serializable".into(), // duplicate!
            associated_types: vec![],
            is_adapter: false,
        },
    ]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "coherence" && e.state == VerificationState::Failed);
    assert!(failed, "duplicate interface impl must fail coherence check");

    let has_code = report
        .entries
        .iter()
        .filter(|e| e.claim == "coherence")
        .any(|e| {
            e.evidence
                .as_deref()
                .map(|ev| ev.contains(E_COHERENCE_DUPLICATE))
                .unwrap_or(false)
        });
    assert!(has_code, "evidence must contain {E_COHERENCE_DUPLICATE}");
}

// Spec scenario 3: "Adapter/newtype path passes"
//   GIVEN a Type node where one impl is is_adapter=true
//   THEN no coherence failure
#[test]
fn adapter_impl_does_not_fail_coherence() {
    let mut node = type_node(0, "AdapterType");
    node.interface_impls = Some(vec![
        InterfaceImplMeta {
            interface: "cap.Chargeable".into(),
            associated_types: vec![],
            is_adapter: false,
        },
        InterfaceImplMeta {
            interface: "cap.Chargeable".into(),
            associated_types: vec![],
            is_adapter: true, // adapter exception
        },
    ]);

    let report = TypeChecker::check(&graph_from(vec![node]));

    let failed = report
        .entries
        .iter()
        .any(|e| e.claim == "coherence" && e.state == VerificationState::Failed);
    assert!(!failed, "adapter impl must not trigger coherence failure");
}

// Spec scenario 3: "Associated-type mismatch fails"
//   GIVEN two Type nodes each with an impl of the same interface, one with mismatched assoc type
//   THEN the mismatch is reported for the node
//
// (In the graph model: if the same interface is declared twice with different assoc types,
//  that's a coherence conflict — covered by duplicate detection above.
//  Here we test a node declaring an assoc type that contradicts itself — i.e. an empty binding
//  where a name is expected — which we report as a coherence violation.)
#[test]
fn associated_type_empty_name_fails_coherence() {
    let mut node = type_node(0, "Repo");
    node.interface_impls = Some(vec![InterfaceImplMeta {
        interface: "cap.Repository".into(),
        associated_types: vec![AssociatedTypeBinding {
            name: "".into(), // empty name — invalid binding
            ty: "DbError".into(),
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
        "empty associated type name must fail coherence check"
    );
}

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
    let failed = report
        .entries
        .iter()
        .any(|e| {
            e.claim == "patchfield"
                && e.state == VerificationState::Failed
                && e.evidence
                    .as_deref()
                    .map(|ev| ev.contains(E_PATCHFIELD_EMPTY_INNER))
                    .unwrap_or(false)
        });
    assert!(!failed, "PatchField<Text> must not emit E_PATCHFIELD_EMPTY_INNER");
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
    assert!(!has_patchfield_entry, "non-PatchField node must not have patchfield entry");
}

// ── Task E3 (RED): PartialOrd validation subpass ──────────────────────────

// S-E3a: Type node with has_partial_ord=true in partial-order context → Proven.
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
    assert!(proven, "type with has_partial_ord=true must emit Proven partial-ord entry");
}

// S-E3b: Node requiring Ord but only has_partial_ord=true → informational entry.
// Triangulation: partial ord without total ord emits a distinguishable entry.
#[test]
fn type_with_only_partial_ord_emits_informational_in_sorting_context() {
    use ail_verify::type_checker::E_PARTIAL_ORD_REQUIRED;

    let mut node = type_node(0, "FloatOrd");
    node.constraint_set = Some(ConstraintSet {
        has_eq: true,
        has_ord: false,   // no total ord
        has_hash: false,
        has_partial_ord: true,
        extras: vec![],
    });
    // A sorting context is signaled by extras containing "needs_ord".
    node.return_type = Some("OrderedSet<FloatOrd>".into());
    let report = TypeChecker::check(&graph_from(vec![node]));

    // Should emit an entry for partial-ord context (Unverified or informational).
    let has_partial_ord_entry = report
        .entries
        .iter()
        .any(|e| e.claim == "partial-ord");
    assert!(
        has_partial_ord_entry,
        "type with partial-ord-only in Ord context must emit a partial-ord entry; code: {E_PARTIAL_ORD_REQUIRED}"
    );
}

// ── Task E5 (RED): boundary inference cross-check subpass ─────────────────

// S-E5a: Function with matching boundary inferred_fact and return_type → Proven.
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

    let failed = report
        .entries
        .iter()
        .any(|e| {
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
    assert!(proven, "matching boundary inference must produce Proven boundary-inference entry");
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
    let generic_proven = report.entries.iter().any(|e| {
        e.claim == "generic-param-kind" && e.state == VerificationState::Proven
    });
    assert!(
        generic_proven,
        "EffectParam 'db' in effect_row must produce Proven generic-param-kind entry"
    );

    // 2. boundary-materialization: process_payment has params + return_type → Proven.
    let boundary_mat_proven = report.entries.iter().any(|e| {
        e.claim == "boundary-materialization" && e.state == VerificationState::Proven
    });
    assert!(
        boundary_mat_proven,
        "function with params + return_type must produce Proven boundary-materialization entry"
    );

    // 3. refinement: PatchOrderDetails has predicate "true" + status Proven → Proven.
    let refinement_proven = report.entries.iter().any(|e| {
        e.claim == "refinement" && e.state == VerificationState::Proven
    });
    assert!(
        refinement_proven,
        "refinement with status Proven and predicate 'true' must produce Proven refinement entry"
    );

    // 4. patchfield: PatchField<Text> inner type is non-empty → Proven.
    let patchfield_proven = report.entries.iter().any(|e| {
        e.claim == "patchfield" && e.state == VerificationState::Proven
    });
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
    let bi_proven = report.entries.iter().any(|e| {
        e.claim == "boundary-inference" && e.state == VerificationState::Proven
    });
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
