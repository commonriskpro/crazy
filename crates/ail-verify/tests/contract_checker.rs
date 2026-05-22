// ── ail-verify::contract_checker tests ───────────────────────────────────
//
// Strict TDD — RED phase.  Written BEFORE src/contract_checker.rs exists.
// These tests encode the RCM spec scenarios verbatim.
//
// Spec domain: runtime-check-materialization
//   RCM-1  ContractChecker accepts &dyn Solver through its constructor.
//   RCM-2  SolverOutcome::Proven → VerificationState::RuntimeChecked.
//   RCM-3  SolverOutcome::Unsupported → VerificationState::Assumed + non-empty evidence.
//   RCM-4  Predicate "false" → VerificationState::Failed.
//   RCM-5  GraphNode with contract_clauses: Some(_) MUST NOT produce Unverified entries.
//   RCM-6  Assumed entries have evidence: Some(s) where s is non-empty.
//   RCM-7  VerificationReport::summary() returns Failed when any entry is Failed.

use ail_core::semantic_graph::{ContractClauses, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::contract_checker::ContractChecker;
use ail_verify::diagnostic::{DiagnosticSeverity, E_CONTRACT_VIOLATED};
use ail_verify::proof::ProofObligation;
use ail_verify::report::VerificationState;
use ail_verify::solver::{Solver, SolverOutcome};

// ── Test-double solver: always Proven ────────────────────────────────────

struct AlwaysProven;

impl Solver for AlwaysProven {
    fn solve(&self, _obligation: &ProofObligation) -> SolverOutcome {
        SolverOutcome::Proven
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn graph_with_clauses(requires: Vec<&str>, ensures: Vec<&str>) -> SemanticGraph {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "n");
    node.contract_clauses = Some(ContractClauses {
        requires: requires.into_iter().map(|s| s.to_string()).collect(),
        ensures: ensures.into_iter().map(|s| s.to_string()).collect(),
    });
    SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    }
}

// ── Scenario RCM-2: Tautology clause emits RuntimeChecked ────────────────

#[test]
fn tautology_clause_emits_runtime_checked() {
    // GIVEN a node with requires: ["true"], ensures: []
    // AND SimpleSolver injected into ContractChecker
    let graph = graph_with_clauses(vec!["true"], vec![]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    // WHEN ContractChecker::check processes the node
    let report = checker.check(&graph);

    // THEN the entry for the requires clause has state: RuntimeChecked
    assert_eq!(report.entries.len(), 1, "one clause → one entry");
    assert_eq!(
        report.entries[0].state,
        VerificationState::RuntimeChecked,
        "literal 'true' must map to RuntimeChecked"
    );
}

// ── Scenario RCM-3 + RCM-6: Unsupported clause emits Assumed with evidence

#[test]
fn unsupported_clause_emits_assumed_with_evidence() {
    // GIVEN a node with ensures: ["user.age >= 18"]
    // AND SimpleSolver injected
    let graph = graph_with_clauses(vec![], vec!["user.age >= 18"]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    // WHEN ContractChecker::check processes the node
    let report = checker.check(&graph);

    // THEN entry has state: Assumed AND evidence is Some(s) where s is non-empty
    assert_eq!(report.entries.len(), 1);
    let entry = &report.entries[0];
    assert_eq!(
        entry.state,
        VerificationState::Assumed,
        "unsupported predicate must map to Assumed"
    );
    match &entry.evidence {
        Some(s) => assert!(!s.is_empty(), "evidence must be non-empty"),
        None => panic!("Assumed entry must have evidence: Some(s), got None"),
    }
}

// ── Scenario RCM-4: Violated literal emits Failed ────────────────────────

#[test]
fn violated_literal_emits_failed() {
    // GIVEN a node with requires: ["false"]
    let graph = graph_with_clauses(vec!["false"], vec![]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    // WHEN ContractChecker::check runs
    let report = checker.check(&graph);

    // THEN the entry has state: Failed
    assert_eq!(report.entries.len(), 1);
    assert_eq!(
        report.entries[0].state,
        VerificationState::Failed,
        "literal 'false' must map to Failed"
    );
}

// ── Scenario RCM-5: No Unverified for contract nodes ─────────────────────

#[test]
fn no_unverified_for_contract_nodes() {
    // GIVEN a node with non-empty contract_clauses (mix of requires + ensures)
    let graph = graph_with_clauses(vec!["true", "false"], vec!["user.age >= 18"]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    // WHEN ContractChecker::check runs
    let report = checker.check(&graph);

    // THEN no emitted entry has state: Unverified
    for entry in &report.entries {
        assert_ne!(
            entry.state,
            VerificationState::Unverified,
            "contract checker must never emit Unverified; got {:?}",
            entry
        );
    }
    assert_eq!(report.entries.len(), 3, "3 clauses → 3 entries");
}

// ── Scenario RCM-7: Failed contract dominates report summary ─────────────

#[test]
fn failed_contract_dominates_report_summary() {
    // GIVEN a node with clauses yielding RuntimeChecked, Assumed, and Failed
    // "true" → RuntimeChecked, "user.age >= 18" → Assumed, "false" → Failed
    let graph = graph_with_clauses(vec!["true", "false"], vec!["user.age >= 18"]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    // THEN summary() returns Failed (dominates all others)
    assert_eq!(
        report.summary(),
        VerificationState::Failed,
        "Failed must dominate summary when any entry is Failed"
    );
}

// ── Scenario RCM-1: Solver swap requires no checker changes ──────────────

#[test]
fn solver_swap_requires_no_checker_changes() {
    // GIVEN a test-double AlwaysProven solver
    // WHEN injected into ContractChecker and check runs on a contract node
    let graph = graph_with_clauses(vec!["x + y < z"], vec!["user.age >= 18"]);
    let double = AlwaysProven;
    let checker = ContractChecker::new(&double);

    let report = checker.check(&graph);

    // THEN every entry has state: RuntimeChecked
    assert_eq!(report.entries.len(), 2);
    for entry in &report.entries {
        assert_eq!(
            entry.state,
            VerificationState::RuntimeChecked,
            "AlwaysProven double must yield RuntimeChecked for every clause"
        );
    }
}

// ── Triangulation: empty contract_clauses produces empty report ───────────

#[test]
fn empty_contract_clauses_produces_empty_report() {
    // GIVEN a node with contract_clauses: Some(empty requires, empty ensures)
    let graph = graph_with_clauses(vec![], vec![]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    // THEN no entries are produced (nothing to check)
    assert_eq!(report.entries.len(), 0, "no clauses → no entries");
    // AND summary is Proven (vacuous truth)
    assert_eq!(report.summary(), VerificationState::Proven);
}

// ── Triangulation: node without contract_clauses produces empty report ────

#[test]
fn node_without_contract_clauses_produces_empty_report() {
    // GIVEN a plain node with no contract_clauses (None)
    let node = GraphNode::new(NodeRef(0), NodeKind::Function, "plain");
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    // THEN no entries — ContractChecker only processes contract clauses
    assert_eq!(report.entries.len(), 0);
}

// ── Diagnostic: "false" clause emits E_CONTRACT_VIOLATED diagnostic ────────

#[test]
fn failed_clause_emits_contract_violated_diagnostic() {
    // GIVEN a node with requires: ["false"]
    let graph = graph_with_clauses(vec!["false"], vec![]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    // WHEN ContractChecker::check processes it
    let report = checker.check(&graph);

    // THEN exactly one diagnostic with code E_CONTRACT_VIOLATED
    assert_eq!(
        report.diagnostics.len(),
        1,
        "failed clause must produce exactly one diagnostic"
    );
    let diag = &report.diagnostics[0];
    assert_eq!(diag.code, E_CONTRACT_VIOLATED);
    assert_eq!(diag.severity, DiagnosticSeverity::Error);
    assert_eq!(diag.target, NodeRef(0));
    assert!(diag.blocking, "E_CONTRACT_VIOLATED must be blocking");
    assert!(
        diag.evidence.is_some(),
        "diagnostic must carry evidence text"
    );
}

// ── Diagnostic: passing clause emits no diagnostic ─────────────────────────

#[test]
fn passing_clause_emits_no_diagnostic() {
    // GIVEN a node with requires: ["true"] (passes → RuntimeChecked)
    let graph = graph_with_clauses(vec!["true"], vec![]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    assert!(
        report.diagnostics.is_empty(),
        "passing clause must not produce any diagnostics"
    );
}

// ── Diagnostic: multiple failed clauses emit one diagnostic each ───────────

#[test]
fn multiple_failed_clauses_emit_one_diagnostic_each() {
    // GIVEN a node with requires: ["false", "false"]
    let graph = graph_with_clauses(vec!["false", "false"], vec![]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    assert_eq!(
        report.diagnostics.len(),
        2,
        "two failed clauses must emit two diagnostics"
    );
    for diag in &report.diagnostics {
        assert_eq!(diag.code, E_CONTRACT_VIOLATED);
    }
}

// ── Diagnostic: target in diagnostic matches node id ──────────────────────

#[test]
fn contract_diagnostic_target_matches_node_id() {
    // GIVEN a node with NodeRef(77) and requires: ["false"]
    let mut node = GraphNode::new(NodeRef(77), NodeKind::Function, "fn_77");
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["false".into()],
        ensures: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == E_CONTRACT_VIOLATED)
        .expect("must have E_CONTRACT_VIOLATED diagnostic");
    assert_eq!(
        diag.target,
        NodeRef(77),
        "diagnostic target must match node's NodeRef"
    );
}

// ── TASK-27: ContractChecker invariant obligations ────────────────────────

// REQ-13: ContractChecker must also check invariant nodes.

#[test]
fn invariant_node_with_requires_generates_obligation() {
    // GIVEN a graph with a Invariant node that has a requires clause
    let mut node = GraphNode::new(NodeRef(10), NodeKind::Invariant, "balance_invariant");
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["amount > 0".to_string()],
        ensures: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    // WHEN check runs
    let report = checker.check(&graph);

    // THEN an entry is generated for the invariant requires clause
    assert!(
        !report.entries.is_empty(),
        "invariant node with requires must produce at least one entry"
    );
    let entry = report
        .entries
        .iter()
        .find(|e| e.scope == "balance_invariant")
        .expect("entry scoped to invariant node must exist");
    assert!(
        entry.claim.starts_with("invariant-requires:"),
        "invariant entry claim must use invariant-requires: prefix, got '{}'",
        entry.claim
    );
}

#[test]
fn invariant_node_with_ensures_generates_obligation() {
    // GIVEN a graph with an Invariant node that has an ensures clause
    let mut node = GraphNode::new(NodeRef(11), NodeKind::Invariant, "total_invariant");
    node.contract_clauses = Some(ContractClauses {
        requires: vec![],
        ensures: vec!["total >= 0".to_string()],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    assert!(
        !report.entries.is_empty(),
        "invariant node with ensures must produce at least one entry"
    );
    let entry = report
        .entries
        .iter()
        .find(|e| e.scope == "total_invariant")
        .expect("entry scoped to invariant node must exist");
    assert!(
        entry.claim.starts_with("invariant-ensures:"),
        "invariant entry claim must use invariant-ensures: prefix, got '{}'",
        entry.claim
    );
}

#[test]
fn regular_function_node_behavior_unchanged_by_invariant_support() {
    // GIVEN a Function node with contracts (existing behavior must be preserved)
    let graph = graph_with_clauses(vec!["true"], vec!["x > 0"]);
    let solver = ail_verify::solver::SimpleSolver;
    let checker = ContractChecker::new(&solver);

    let report = checker.check(&graph);

    // Regular function still produces entries (existing behavior unchanged)
    assert_eq!(
        report.entries.len(),
        2,
        "function node: one entry per clause (requires + ensures)"
    );
    // requires: "true" → RuntimeChecked (SimpleSolver proves it)
    assert_eq!(report.entries[0].state, VerificationState::RuntimeChecked);
    // ensures: "x > 0" → Assumed (SimpleSolver returns Unsupported)
    assert_eq!(report.entries[1].state, VerificationState::Assumed);
}
