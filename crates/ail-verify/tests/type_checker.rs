// ── ail-verify::type_checker tests ───────────────────────────────────────
//
// Integration tests for TypeChecker (verification pipeline step 7).
//
// TypeChecker rules:
//   - Function/Type nodes with non-empty TypeFacts.nominal + valid generics → Proven
//   - Function/Type nodes with TypeFacts.nominal non-empty + empty generic string → Failed (E_GENERIC_ARITY)
//   - Function/Type nodes without TypeFacts (or empty nominal) → Unverified
//   - All other node kinds are skipped (produce no entries)

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph, TypeFacts};
use ail_verify::report::VerificationState;
use ail_verify::type_checker::TypeChecker;

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
