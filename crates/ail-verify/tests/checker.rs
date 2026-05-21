// ── ail-verify::checker tests + diagnostic assertions ─────────────────────
//
// Strict TDD — RED phase.  Written BEFORE src/checker.rs is implemented.
// These tests encode the spec scenarios verbatim.
//
// Checker model (per design):
//   For each GraphNode the checker produces three VerificationEntry items
//   in order: (type_fact, effect_fact, capability_fact).
//   - type_fact  : Proven if type_facts.nominal is non-empty; else Unverified.
//   - effect_fact: Assumed if effect_row is Some(..); else Unverified.
//   - cap_fact   : Assumed if capability_reqs is Some(..); else Unverified.
//
//   Empty graph ⇒ 0 entries, summary = Proven.

use ail_core::semantic_graph::{
    CapabilityReqs, EffectRow, GraphNode, NodeKind, NodeRef, SemanticGraph, TypeFacts,
};
use ail_verify::checker::Checker;
use ail_verify::diagnostic::{DiagnosticSeverity, E_TYPE_MISMATCH};
use ail_verify::report::VerificationState;

// Helper: build a minimal SemanticGraph from a list of nodes.
fn graph_from(nodes: Vec<GraphNode>) -> SemanticGraph {
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

// Helper: find the first entry whose claim equals `claim_kind`.
fn entry_state(
    report: &ail_verify::report::VerificationReport,
    scope: &str,
    claim_kind: &str,
) -> VerificationState {
    report
        .entries
        .iter()
        .find(|e| e.scope == scope && e.claim == claim_kind)
        .unwrap_or_else(|| panic!("no entry for scope={scope} claim={claim_kind}"))
        .state
}

// ── Scenario: Empty graph yields 0 entries and Proven summary ─────────────

#[test]
fn empty_graph_has_no_entries_and_proven_summary() {
    let graph = graph_from(vec![]);
    let report = Checker::check(&graph);
    assert_eq!(
        report.entries.len(),
        0,
        "empty graph must produce zero entries"
    );
    assert_eq!(
        report.summary(),
        VerificationState::Proven,
        "vacuous summary must be Proven"
    );
}

// ── Scenario: Node with non-empty nominal → Proven type entry ─────────────

#[test]
fn node_with_nominal_type_yields_proven() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Bool");
    node.type_facts = Some(TypeFacts {
        nominal: "Bool".into(),
        generics: vec![],
    });
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);
    let state = entry_state(&report, "Bool", "type");
    assert_eq!(state, VerificationState::Proven);
}

// ── Scenario: Node with type_facts: None → Unverified type entry ──────────

#[test]
fn node_without_type_facts_yields_unverified() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "my_fn");
    // type_facts is None by default
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);
    let state = entry_state(&report, "my_fn", "type");
    assert_eq!(state, VerificationState::Unverified);
}

// ── Scenario: Declared effect row yields Assumed ──────────────────────────

#[test]
fn node_with_effect_row_yields_assumed() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Effect, "io_effect");
    node.effect_row = Some(EffectRow {
        effects: vec!["IO".into()],
    });
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);
    let state = entry_state(&report, "io_effect", "effect");
    assert_eq!(state, VerificationState::Assumed);
}

// ── Scenario: effect_row: None → Unverified effect entry ──────────────────

#[test]
fn node_without_effect_row_yields_unverified() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "pure_fn");
    // effect_row is None by default
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);
    let state = entry_state(&report, "pure_fn", "effect");
    assert_eq!(state, VerificationState::Unverified);
}

// ── Scenario: Declared capability reqs yields Assumed ─────────────────────

#[test]
fn node_with_capability_reqs_yields_assumed() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Capability, "net_reader");
    node.capability_reqs = Some(CapabilityReqs {
        caps: vec!["net:read".into()],
    });
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);
    let state = entry_state(&report, "net_reader", "capability");
    assert_eq!(state, VerificationState::Assumed);
}

// ── Scenario: capability_reqs: None → Unverified capability entry ──────────

#[test]
fn node_without_capability_reqs_yields_unverified() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "isolated_fn");
    // capability_reqs is None by default
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);
    let state = entry_state(&report, "isolated_fn", "capability");
    assert_eq!(state, VerificationState::Unverified);
}

// ── Triangulation: multi-node graph — deterministic entry order ────────────
//
// Two nodes → 6 entries (3 per node).  Verify the entries appear in node
// order and that scope names are correctly assigned.

#[test]
fn multi_node_graph_entries_are_in_node_order() {
    let mut node_a = GraphNode::new(NodeRef(0), NodeKind::Type, "TypeA");
    node_a.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });

    let mut node_b = GraphNode::new(NodeRef(1), NodeKind::Effect, "EffectB");
    node_b.effect_row = Some(EffectRow {
        effects: vec!["State".into()],
    });

    let graph = graph_from(vec![node_a, node_b]);
    let report = Checker::check(&graph);

    // 3 entries per node = 6 total
    assert_eq!(report.entries.len(), 6);

    // First 3 entries belong to TypeA
    assert_eq!(report.entries[0].scope, "TypeA");
    assert_eq!(report.entries[1].scope, "TypeA");
    assert_eq!(report.entries[2].scope, "TypeA");

    // Next 3 entries belong to EffectB
    assert_eq!(report.entries[3].scope, "EffectB");
    assert_eq!(report.entries[4].scope, "EffectB");
    assert_eq!(report.entries[5].scope, "EffectB");

    // TypeA has Proven type, Unverified effect, Unverified cap
    assert_eq!(report.entries[0].state, VerificationState::Proven); // type
    assert_eq!(report.entries[1].state, VerificationState::Unverified); // effect
    assert_eq!(report.entries[2].state, VerificationState::Unverified); // cap

    // EffectB has Unverified type, Assumed effect, Unverified cap
    assert_eq!(report.entries[3].state, VerificationState::Unverified); // type
    assert_eq!(report.entries[4].state, VerificationState::Assumed); // effect
    assert_eq!(report.entries[5].state, VerificationState::Unverified); // cap
}

// ── Diagnostic: node without type facts emits E_TYPE_MISMATCH ─────────────

#[test]
fn node_without_type_facts_emits_type_mismatch_diagnostic() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "untyped_fn");
    // type_facts is None → Unverified type entry → diagnostic
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);

    assert!(
        !report.diagnostics.is_empty(),
        "must emit at least one diagnostic for untyped node"
    );
    let diag = &report.diagnostics[0];
    assert_eq!(diag.code, E_TYPE_MISMATCH);
    assert_eq!(diag.severity, DiagnosticSeverity::Error);
    assert_eq!(diag.target, NodeRef(0));
    assert!(diag.blocking, "E_TYPE_MISMATCH must be blocking");
    assert!(
        diag.evidence.is_some(),
        "diagnostic must carry evidence text"
    );
}

// ── Diagnostic: node WITH type facts emits no type diagnostic ─────────────

#[test]
fn node_with_type_facts_emits_no_type_mismatch_diagnostic() {
    let mut node = GraphNode::new(NodeRef(1), NodeKind::Type, "TypedNode");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);

    let type_mismatch_count = report
        .diagnostics
        .iter()
        .filter(|d| d.code == E_TYPE_MISMATCH)
        .count();
    assert_eq!(
        type_mismatch_count, 0,
        "typed node must not produce E_TYPE_MISMATCH"
    );
}

// ── Diagnostic: empty graph produces no diagnostics ───────────────────────

#[test]
fn empty_graph_produces_no_diagnostics() {
    let graph = graph_from(vec![]);
    let report = Checker::check(&graph);
    assert!(
        report.diagnostics.is_empty(),
        "empty graph must produce zero diagnostics"
    );
}

// ── Diagnostic: diagnostic target matches node id ─────────────────────────

#[test]
fn diagnostic_target_matches_node_id() {
    let node = GraphNode::new(NodeRef(99), NodeKind::Function, "fn_99");
    let graph = graph_from(vec![node]);
    let report = Checker::check(&graph);

    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == E_TYPE_MISMATCH)
        .expect("must have E_TYPE_MISMATCH diagnostic");
    assert_eq!(
        diag.target,
        NodeRef(99),
        "diagnostic target must match the node's NodeRef"
    );
}
