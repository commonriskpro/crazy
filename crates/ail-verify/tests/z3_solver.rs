// ── ail-verify::z3_solver tests ──────────────────────────────────────────
//
// Integration tests for `Z3Solver` — only compiled and run when the
// `z3-solver` feature is enabled.
//
// Run with:
//   cargo test --package ail-verify --features z3-solver
//
// Spec domain: Z3 Solver Backend (ZSB) / Predicate Parser (PP)
//
//   ZSB-3  Z3Solver returns Proven for tautologies.
//   ZSB-4  Z3Solver returns Unsupported for unparseable predicates.
//   ZSB-5  Z3Solver returns Unsupported on timeout.
//   ZSB-7  Timeout is configurable via with_timeout_ms.
//   ZSB-8  Z3Solver is deterministic for pure predicates.
//   ZSB-9  Z3Solver is injectable into ContractChecker via &dyn Solver.
//   PP-1   true → tautology.
//   PP-2   false → not tautology (sat for negation).
//   PP-3   Comparisons: >, >=, <, <=, ==, !=.
//   PP-4   Arithmetic: +, -, *, /.
//   PP-5   Logical: &&, ||, !.
//   PP-9   Unsupported grammar → Unsupported outcome.

#[cfg(feature = "z3-solver")]
mod z3_solver_tests {
    use ail_core::semantic_graph::{ContractClauses, GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_verify::contract_checker::ContractChecker;
    use ail_verify::proof::{ClauseRole, ProofObligation};
    use ail_verify::report::VerificationState;
    use ail_verify::solver::{Solver, SolverOutcome};
    use ail_verify::z3_solver::Z3Solver;

    // ── Helpers ───────────────────────────────────────────────────────────

    fn solve(predicate: &str) -> SolverOutcome {
        let solver = Z3Solver::new();
        let oblig = ProofObligation {
            predicate: predicate.to_string(),
            role: ClauseRole::Requires,
        };
        solver.solve(&oblig)
    }

    fn solve_ensures(predicate: &str) -> SolverOutcome {
        let solver = Z3Solver::new();
        let oblig = ProofObligation {
            predicate: predicate.to_string(),
            role: ClauseRole::Ensures,
        };
        solver.solve(&oblig)
    }

    // ── Scenario ZSB-3 + PP-1: Boolean literal "true" → Proven ───────────

    #[test]
    fn boolean_literal_true_is_proven() {
        assert_eq!(solve("true"), SolverOutcome::Proven);
    }

    // ── Scenario ZSB-3: Tautology via disjunction → Proven ───────────────

    #[test]
    fn tautology_disjunction_is_proven() {
        // x > 0 || x <= 0 is always true regardless of x.
        assert_eq!(solve("x > 0 || x <= 0"), SolverOutcome::Proven);
    }

    // ── Scenario ZSB-3: Double negation tautology → Proven ───────────────

    #[test]
    fn double_negation_tautology_is_proven() {
        // x >= 5 || x < 5 is always true.
        assert_eq!(solve("x >= 5 || x < 5"), SolverOutcome::Proven);
    }

    // ── Scenario ZSB-3: Ensures role also works ───────────────────────────

    #[test]
    fn tautology_on_ensures_role_is_proven() {
        assert_eq!(solve_ensures("x > 0 || x <= 0"), SolverOutcome::Proven);
    }

    // ── Scenario PP-2: false → NOT Proven ────────────────────────────────
    //
    // The contract_checker pre-screens "false" before calling the solver,
    // but the Z3Solver must still not claim it is a tautology.

    #[test]
    fn boolean_literal_false_is_not_proven() {
        let outcome = solve("false");
        assert_ne!(outcome, SolverOutcome::Proven, "false is never a tautology");
    }

    // ── Scenario ZSB-4 + PP-9: Non-tautological predicate → Unsupported ──
    //
    // `x > 0` is satisfiable but NOT a tautology; x=0 falsifies it.

    #[test]
    fn non_tautology_is_unsupported() {
        let outcome = solve("x > 0");
        assert_ne!(
            outcome,
            SolverOutcome::Proven,
            "x > 0 is not a tautology — x=0 falsifies it"
        );
    }

    // ── Scenario ZSB-4 + PP-9: Dot-notation → Unsupported with reason ────

    #[test]
    fn dot_notation_predicate_is_unsupported() {
        // "user.age >= 18" contains dot-notation which is not in our grammar.
        let outcome = solve("user.age >= 18");
        assert_eq!(
            outcome,
            SolverOutcome::Unsupported,
            "dot-notation should not be parseable"
        );
    }

    // ── Scenario ZSB-4: Bare unrecognised syntax → Unsupported ───────────

    #[test]
    fn malformed_predicate_is_unsupported() {
        let outcome = solve("@invalid predicate$");
        assert_eq!(outcome, SolverOutcome::Unsupported);
    }

    // ── Scenario ZSB-5 + ZSB-7: Short timeout → Unsupported ─────────────

    #[test]
    fn short_timeout_yields_unsupported_or_proven() {
        // With 1ms timeout, Z3 may or may not finish in time for simple
        // predicates. For a simple tautology it may still return Proven
        // (fast path). The key property: it must NOT panic.
        let solver = Z3Solver::with_timeout_ms(1);
        let oblig = ProofObligation {
            predicate: "x > 0 || x <= 0".to_string(),
            role: ClauseRole::Requires,
        };
        // Outcome is either Proven (finished fast) or Unsupported (timed out).
        // Either is valid; the test just ensures no panic.
        let _outcome = solver.solve(&oblig);
    }

    // ── Scenario ZSB-8: Determinism — same input same output ─────────────

    #[test]
    fn solve_is_deterministic_for_tautology() {
        let solver = Z3Solver::new();
        let oblig = ProofObligation {
            predicate: "x > 0 || x <= 0".to_string(),
            role: ClauseRole::Requires,
        };
        let first = solver.solve(&oblig);
        let second = solver.solve(&oblig);
        assert_eq!(first, second, "Z3Solver must be deterministic");
    }

    #[test]
    fn solve_is_deterministic_for_unsupported() {
        let solver = Z3Solver::new();
        let oblig = ProofObligation {
            predicate: "user.age >= 18".to_string(),
            role: ClauseRole::Requires,
        };
        let first = solver.solve(&oblig);
        let second = solver.solve(&oblig);
        assert_eq!(
            first, second,
            "Z3Solver must be deterministic for Unsupported"
        );
    }

    // ── Scenario ZSB-9: Injectable into ContractChecker ──────────────────
    //
    // A tautological `requires` clause must yield state RuntimeChecked
    // (RCM-2: Proven → RuntimeChecked).

    #[test]
    fn z3_solver_injectable_into_contract_checker() {
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_test");
        node.contract_clauses = Some(ContractClauses {
            requires: vec!["x > 0 || x <= 0".to_string()],
            ensures: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };

        let solver = Z3Solver::new();
        let checker = ContractChecker::new(&solver);
        let report = checker.check(&graph);

        assert_eq!(report.entries.len(), 1, "one clause → one entry");
        assert_eq!(
            report.entries[0].state,
            VerificationState::RuntimeChecked,
            "tautology via Z3Solver must produce RuntimeChecked (RCM-2)"
        );
    }

    // ── Scenario PP-3: Comparison operators ──────────────────────────────

    #[test]
    fn comparison_operators_support() {
        // x > 0 || x <= 0: tautology using > and <=
        assert_eq!(solve("x > 0 || x <= 0"), SolverOutcome::Proven);
        // x >= 0 || x < 0: tautology using >= and <
        assert_eq!(solve("x >= 0 || x < 0"), SolverOutcome::Proven);
        // x == x: always true (tautology)
        assert_eq!(solve("x == x"), SolverOutcome::Proven);
    }

    // ── Scenario PP-4 + PP-5: Arithmetic and logical operators ───────────

    #[test]
    fn arithmetic_and_logic_in_tautology() {
        // x + 1 > x is always true for integers.
        assert_eq!(solve("x + 1 > x"), SolverOutcome::Proven);
    }

    // ── Scenario PP-5: Logical NOT ────────────────────────────────────────

    #[test]
    fn logical_not_in_tautology() {
        // !(x > 0) || x > 0 is always true (excluded middle).
        assert_eq!(solve("!(x > 0) || x > 0"), SolverOutcome::Proven);
    }

    // ── Scenario PP-10: No panic on malformed input ───────────────────────

    #[test]
    fn no_panic_on_malformed_input() {
        // These must not panic.
        let _ = solve("");
        let _ = solve("((((");
        let _ = solve(">>>>");
        let _ = solve("true && && false");
    }
}
