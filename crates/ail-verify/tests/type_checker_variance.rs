// ── ail-verify::type_checker — variance and coherence tests ───────────────
//
// Integration tests for TypeChecker:
//   Subpass 3 — Generic param kind validation (EffectParam, CapabilityParam,
//               TypeParam, ConstParam decidability)
//   Subpass 4 — Variance enforcement (parameterized type coercion)
//   Subpass 5 — Interface coherence (duplicate impls, adapter exception,
//               associated type binding)

use ail_core::semantic_graph::EdgeKind;
use ail_core::semantic_graph::{
    AssociatedTypeBinding, GenericParamDecl, GenericParamKind, GraphEdge, GraphNode,
    InterfaceImplMeta, NodeKind, NodeRef, ParamDecl, SemanticGraph,
};
use ail_verify::report::VerificationState;
use ail_verify::type_checker::{
    E_CAPABILITY_PARAM_WIDENED, E_COHERENCE_DUPLICATE, E_EFFECT_PARAM_WIDENED, E_VARIANCE_COERCION,
    TypeChecker,
};

fn graph_from(nodes: Vec<GraphNode>) -> SemanticGraph {
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

fn graph_with_edges(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> SemanticGraph {
    SemanticGraph { nodes, edges }
}

fn type_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Type, name)
}

fn fn_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Function, name)
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
