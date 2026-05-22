// ── ail-verify::proof — ProofObligationPipeline::run_with_ledger tests ───
//
// Strict TDD — RED phase.
// Tests for the new `run_with_ledger` method that returns first-class
// ObligationLedgerEntry items with identity, attempts, and degradation tracking.
//
// Spec: verification-pipeline/spec §6 (proof obligation pipeline with
// degradation tracking).

use ail_core::semantic_graph::{ContractClauses, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::proof::{ObligationState, ProofObligationPipeline};
use ail_verify::solver::SimpleSolver;

fn graph_with_clauses(node_name: &str, requires: Vec<&str>, ensures: Vec<&str>) -> SemanticGraph {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, node_name);
    node.contract_clauses = Some(ContractClauses {
        requires: requires.into_iter().map(String::from).collect(),
        ensures: ensures.into_iter().map(String::from).collect(),
    });
    SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    }
}

// ── Scenario: one entry per clause ────────────────────────────────────────
// GIVEN a graph with 2 requires + 1 ensures
// WHEN run_with_ledger is called
// THEN 3 ObligationLedgerEntry items are returned
#[test]
fn run_with_ledger_returns_entry_per_clause() {
    let graph = graph_with_clauses("fn_a", vec!["true", "x > 0"], vec!["result >= 0"]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);
    assert_eq!(entries.len(), 3, "2 requires + 1 ensures = 3 entries");
}

// ── Scenario: literal "true" → Proven with no degradation ────────────────
// GIVEN a requires clause "true" (simplified by stage 2)
// WHEN run_with_ledger runs
// THEN state is Proven, degradation_reason is None
#[test]
fn ledger_entry_for_true_has_proven_state_and_no_degradation() {
    let graph = graph_with_clauses("fn_b", vec!["true"], vec![]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, ObligationState::Proven);
    assert!(
        entries[0].degradation_reason.is_none(),
        "Proven obligations have no degradation reason"
    );
}

// ── Scenario: non-trivial predicate → Assumed with degradation_reason ─────
// GIVEN a requires clause "x > 0" (solver returns Unsupported → Assumed)
// WHEN run_with_ledger runs
// THEN state is Assumed(_) and degradation_reason is Some(_)
#[test]
fn ledger_entry_for_assumed_has_degradation_reason() {
    let graph = graph_with_clauses("fn_c", vec!["x > 0"], vec![]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);

    assert_eq!(entries.len(), 1);
    assert!(
        matches!(entries[0].state, ObligationState::Assumed(_)),
        "non-trivial predicate must degrade to Assumed"
    );
    assert!(
        entries[0].degradation_reason.is_some(),
        "Assumed obligations must have a degradation_reason"
    );
}

// ── Scenario: each entry has a unique id ─────────────────────────────────
// GIVEN two clauses in the same node
// WHEN run_with_ledger runs
// THEN each entry has a distinct id string
#[test]
fn ledger_entries_have_unique_ids() {
    let graph = graph_with_clauses("fn_d", vec!["x > 0", "y > 0"], vec![]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);

    assert_eq!(entries.len(), 2);
    assert_ne!(
        entries[0].id, entries[1].id,
        "each obligation must have a unique id"
    );
    // IDs must be non-empty
    assert!(!entries[0].id.is_empty());
    assert!(!entries[1].id.is_empty());
}

// ── Scenario: source_stage is "contract" for clauses from contract_clauses ─
// GIVEN a requires clause from a node's contract_clauses
// WHEN run_with_ledger runs
// THEN source_stage is "contract"
#[test]
fn ledger_entry_source_stage_is_contract() {
    let graph = graph_with_clauses("fn_e", vec!["amount > 0"], vec![]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);

    assert_eq!(entries[0].source_stage, "contract");
}

// ── Scenario: "false" → Failed, with attempt recorded ────────────────────
// GIVEN a requires clause "false" (literal fail by stage 2)
// WHEN run_with_ledger runs
// THEN state is Failed and at least one attempt is recorded
#[test]
fn ledger_entry_for_false_has_failed_state_and_attempt() {
    let graph = graph_with_clauses("fn_f", vec!["false"], vec![]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, ObligationState::Failed);
    assert!(
        !entries[0].attempts.is_empty(),
        "Failed obligations must record at least one attempt"
    );
}

// ── Scenario: empty graph → empty ledger ─────────────────────────────────
#[test]
fn empty_graph_produces_empty_ledger() {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);
    assert!(entries.is_empty());
}

// ── TRIANGULATE: obligation scope matches node name ───────────────────────
#[test]
fn ledger_entry_obligation_scope_matches_node_name() {
    let graph = graph_with_clauses("checkout_fn", vec!["amount > 0"], vec![]);
    let solver = SimpleSolver;
    let entries = ProofObligationPipeline::run_with_ledger(&graph, &solver);

    assert_eq!(entries[0].obligation.scope, "checkout_fn");
}

// ── TRIANGULATE: existing run() method still works after extension ────────
// Ensure backward compat: run() continues to return Vec<ObligationResult>
#[test]
fn original_run_method_still_works() {
    use ail_verify::proof::ProofObligationPipeline;

    let graph = graph_with_clauses("fn_compat", vec!["true"], vec![]);
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].state, ObligationState::Proven);
}
