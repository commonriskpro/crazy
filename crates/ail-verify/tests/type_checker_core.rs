// ── ail-verify::type_checker — core tests ────────────────────────────────
//
// Integration tests for TypeChecker: Subpass 1 (nominal presence),
// Subpass 2 (nominal call check), and G24 multi-subpass integration
// scenarios (generic-call binding, effect/capability propagation,
// structural types, and refinement proof obligations).

use ail_core::semantic_graph::EdgeKind;
use ail_core::semantic_graph::{
    CapabilityReqs, EffectRow, GenericParamDecl, GenericParamKind, GraphEdge, GraphNode, NodeKind,
    NodeRef, ParamDecl, RefinementRef, RefinementStatus, RuntimeCheckMeta, SemanticGraph,
    TypeArgBinding, TypeFacts,
};
use ail_verify::diagnostic::DiagnosticSeverity;
use ail_verify::report::VerificationState;
use ail_verify::type_checker::{
    E_CAPABILITY_NOT_PROPAGATED, E_DYN_INTERFACE_UNAVAILABLE, E_EFFECT_NOT_PROPAGATED,
    E_GENERIC_BINDING_ARITY, E_NOMINAL_MISMATCH, E_REFINEMENT_PROOF_UNDISCHARGED,
    E_REFINEMENT_RUNTIME_CHECK_MISSING, E_STRUCTURAL_TYPE_MISMATCH,
    TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING, TypeChecker,
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
        entry.claim == TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING
            && entry.state == VerificationState::Failed
            && entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_GENERIC_BINDING_ARITY)
    }));

    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == E_GENERIC_BINDING_ARITY)
        .expect("generic call binding failure must produce a structured diagnostic");
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.target, NodeRef(0));
    assert!(diagnostic.blocking);
    assert!(
        diagnostic
            .evidence
            .as_deref()
            .unwrap_or("")
            .contains(TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING),
        "diagnostic evidence must carry the stable type category"
    );
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
