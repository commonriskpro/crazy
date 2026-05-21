// ── ail-verify::proof — ProofObligationPipeline tests ────────────────────
//
// Integration tests for the five-stage proof obligation pipeline:
//   Generate → Simplify → Solve → Compose → Degrade

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

// ── Stage 1: Generate — extract obligations from contract clauses ─────────
// GIVEN a graph with one Function node with requires + ensures clauses
// WHEN pipeline runs
// THEN one ObligationResult per clause

#[test]
fn pipeline_generates_one_result_per_clause() {
    let graph = graph_with_clauses("fn_a", vec!["true", "x > 0"], vec!["result >= 0"]);
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert_eq!(results.len(), 3, "2 requires + 1 ensures = 3 results");
}

// ── Stage 2: Simplify — literal "true" → Proven ──────────────────────────
// GIVEN a requires clause with predicate "true"
// WHEN pipeline runs (SimpleSolver injected)
// THEN state is Proven (simplified before solver dispatch)

#[test]
fn literal_true_clause_is_proven_by_simplify() {
    let graph = graph_with_clauses("fn_b", vec!["true"], vec![]);
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].state, ObligationState::Proven);
}

// ── Stage 2: Simplify — literal "false" → Failed ─────────────────────────
// GIVEN a requires clause with predicate "false"
// WHEN pipeline runs
// THEN state is Failed (simplified before solver dispatch)

#[test]
fn literal_false_clause_is_failed_by_simplify() {
    let graph = graph_with_clauses("fn_c", vec!["false"], vec![]);
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].state, ObligationState::Failed);
}

// ── Stage 3+5: Solve → Degrade — non-trivial predicate → Assumed ─────────
// GIVEN a requires clause with predicate "x > 0"
// AND SimpleSolver (returns Unsupported for non-trivial)
// WHEN pipeline runs
// THEN state is Assumed(reason) after degrade stage

#[test]
fn unsupported_predicate_degrades_to_assumed() {
    let graph = graph_with_clauses("fn_d", vec!["x > 0"], vec![]);
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0].state, ObligationState::Assumed(_)),
        "non-trivial predicate must degrade to Assumed, got {:?}",
        results[0].state
    );
}

// ── Stage 4: Compose — shared ensures predicate → RuntimeChecked ──────────
// GIVEN TWO nodes: node A has requires ["x > 0"], node B has ensures ["x > 0"]
// WHEN pipeline runs
// THEN A's obligation is upgraded to RuntimeChecked via compose_check

#[test]
fn compose_upgrades_assumed_when_peer_ensures_matches() {
    let mut node_a = GraphNode::new(NodeRef(0), NodeKind::Function, "caller");
    node_a.contract_clauses = Some(ContractClauses {
        requires: vec!["x > 0".into()],
        ensures: vec![],
    });
    let mut node_b = GraphNode::new(NodeRef(1), NodeKind::Function, "callee");
    node_b.contract_clauses = Some(ContractClauses {
        requires: vec![],
        ensures: vec!["x > 0".into()], // ensures matches caller's requires
    });
    let graph = SemanticGraph {
        nodes: vec![node_a, node_b],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    // results[0] = caller requires "x > 0"; results[1] = callee ensures "x > 0"
    assert_eq!(results.len(), 2);
    // caller's requires "x > 0" should be RuntimeChecked (composed from callee's ensures)
    assert_eq!(
        results[0].state,
        ObligationState::RuntimeChecked,
        "caller's requires must be RuntimeChecked via composition"
    );
    // callee's ensures "x > 0" — compose_check finds its own ensures in the graph,
    // so it is also upgraded to RuntimeChecked (graph-wide composition scan).
    assert_eq!(
        results[1].state,
        ObligationState::RuntimeChecked,
        "callee's own ensures is found by compose_check (graph-wide scan) → RuntimeChecked"
    );
}

// ── Triangulation: empty graph → empty results ───────────────────────────

#[test]
fn empty_graph_produces_no_results() {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert!(results.is_empty());
}

// ── Triangulation: node without contract_clauses produces no results ───────

#[test]
fn node_without_contract_clauses_produces_no_results() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "plain");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert!(results.is_empty());
}

// ── Triangulation: obligation carries scope name from node ────────────────

#[test]
fn obligation_result_carries_scope_name() {
    let graph = graph_with_clauses("checkout_fn", vec!["amount > 0"], vec![]);
    let solver = SimpleSolver;
    let results = ProofObligationPipeline::run(&graph, &solver);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].obligation.scope, "checkout_fn");
}
