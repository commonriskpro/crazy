// ── ail-verify::solver tests ──────────────────────────────────────────────
//
// Strict TDD — RED phase.  Written BEFORE src/proof.rs and src/solver.rs exist.
// These tests encode the POS spec scenarios verbatim.
//
// Spec domain: proof-obligation-solver
//   POS-1  ClauseRole must have exactly two variants: Requires and Ensures.
//   POS-2  ProofObligation is constructible from any predicate string.
//   POS-3  SolverOutcome has exactly three variants: Proven, Assumed(String), Unsupported.
//   POS-4  Solver trait exposes fn solve(&self, &ProofObligation) -> SolverOutcome.
//   POS-5  SimpleSolver::solve returns Proven for literal "true".
//   POS-6  SimpleSolver::solve returns Unsupported for any unknown predicate;
//          it MUST NOT return Proven for non-tautological predicates.
//   POS-7  SimpleSolver::solve is deterministic: same input → same output.

use ail_verify::proof::{ClauseRole, ProofObligation};
use ail_verify::solver::{SimpleSolver, Solver, SolverOutcome};

// ── Scenario POS-5: Literal "true" resolves to Proven ─────────────────────

#[test]
fn literal_true_resolves_to_proven() {
    let solver = SimpleSolver;
    let obligation = ProofObligation {
        predicate: "true".into(),
        role: ClauseRole::Requires,
        scope: String::new(),
    };
    assert_eq!(solver.solve(&obligation), SolverOutcome::Proven);
}

// ── Scenario POS-6: Non-trivial predicate is Unsupported ──────────────────

#[test]
fn non_trivial_predicate_is_unsupported() {
    let solver = SimpleSolver;
    let obligation = ProofObligation {
        predicate: "x + y < z".into(),
        role: ClauseRole::Ensures,
        scope: String::new(),
    };
    let outcome = solver.solve(&obligation);
    // MUST NOT be Proven
    assert_ne!(
        outcome,
        SolverOutcome::Proven,
        "SimpleSolver must not prove non-tautological predicates"
    );
    assert_eq!(outcome, SolverOutcome::Unsupported);
}

// ── Triangulation: "true" on Ensures role also Proven ─────────────────────

#[test]
fn literal_true_on_ensures_role_is_proven() {
    let solver = SimpleSolver;
    let obligation = ProofObligation {
        predicate: "true".into(),
        role: ClauseRole::Ensures,
        scope: String::new(),
    };
    assert_eq!(solver.solve(&obligation), SolverOutcome::Proven);
}

// ── Triangulation: complex Requires clause is Unsupported ─────────────────

#[test]
fn complex_requires_clause_is_unsupported() {
    let solver = SimpleSolver;
    let obligation = ProofObligation {
        predicate: "user.age >= 18".into(),
        role: ClauseRole::Requires,
        scope: String::new(),
    };
    let outcome = solver.solve(&obligation);
    assert_ne!(outcome, SolverOutcome::Proven);
    assert_eq!(outcome, SolverOutcome::Unsupported);
}

// ── Scenario POS-7: Repeated calls return the same outcome ────────────────

#[test]
fn repeated_calls_return_same_outcome() {
    let solver = SimpleSolver;

    let true_oblig = ProofObligation {
        predicate: "true".into(),
        role: ClauseRole::Requires,
        scope: String::new(),
    };
    let first = solver.solve(&true_oblig);
    let second = solver.solve(&true_oblig);
    assert_eq!(first, second, "solve must be deterministic for 'true'");

    let complex_oblig = ProofObligation {
        predicate: "result > 0".into(),
        role: ClauseRole::Ensures,
        scope: String::new(),
    };
    let first_c = solver.solve(&complex_oblig);
    let second_c = solver.solve(&complex_oblig);
    assert_eq!(
        first_c, second_c,
        "solve must be deterministic for complex predicates"
    );
}

// ── Scenario POS-4: Solver trait is injectable via &dyn Solver ────────────
//
// A test-double `AlwaysProven` that proves everything.  This verifies the
// trait boundary compiles with a foreign type — no SimpleSolver change needed.

struct AlwaysProven;

impl Solver for AlwaysProven {
    fn solve(&self, _obligation: &ProofObligation) -> SolverOutcome {
        SolverOutcome::Proven
    }
}

fn run_solver(solver: &dyn Solver, predicate: &str, role: ClauseRole) -> SolverOutcome {
    let oblig = ProofObligation {
        predicate: predicate.into(),
        role,
        scope: String::new(),
    };
    solver.solve(&oblig)
}

#[test]
fn solver_trait_is_injectable_with_foreign_type() {
    let double = AlwaysProven;
    // AlwaysProven proves everything — even a complex predicate
    let outcome = run_solver(&double, "x + y < z", ClauseRole::Requires);
    assert_eq!(
        outcome,
        SolverOutcome::Proven,
        "test-double solver must be usable via &dyn Solver"
    );

    // SimpleSolver passed through the same abstraction
    let simple = SimpleSolver;
    let outcome_simple = run_solver(&simple, "x + y < z", ClauseRole::Requires);
    assert_eq!(
        outcome_simple,
        SolverOutcome::Unsupported,
        "SimpleSolver injected via &dyn Solver must still return Unsupported"
    );
}

// ── POS-1: ClauseRole has exactly Requires and Ensures ────────────────────

#[test]
fn clause_role_has_requires_and_ensures_variants() {
    // Exhaustive match — if a variant is added this test fails to compile
    let roles = [ClauseRole::Requires, ClauseRole::Ensures];
    for role in &roles {
        let _desc = match role {
            ClauseRole::Requires => "requires",
            ClauseRole::Ensures => "ensures",
        };
    }
    assert_eq!(roles.len(), 2);
}

// ── POS-3: SolverOutcome has exactly Proven, Assumed(String), Unsupported ──

#[test]
fn solver_outcome_has_three_variants() {
    let outcomes: Vec<SolverOutcome> = vec![
        SolverOutcome::Proven,
        SolverOutcome::Assumed("degraded: no SMT solver".into()),
        SolverOutcome::Unsupported,
    ];
    assert_eq!(outcomes.len(), 3);
    // Exhaustive match ensures no missing arm at compile time
    for outcome in &outcomes {
        let _desc = match outcome {
            SolverOutcome::Proven => "proven",
            SolverOutcome::Assumed(_) => "assumed",
            SolverOutcome::Unsupported => "unsupported",
        };
    }
}

// ── TASK-23: SimpleSolver KnownRefinement normalization ───────────────────

fn prove(predicate: &str) -> SolverOutcome {
    let solver = SimpleSolver;
    solver.solve(&ProofObligation {
        predicate: predicate.into(),
        role: ClauseRole::Requires,
        scope: "test".into(),
    })
}

// ── Ola5: SimpleSolver arithmetic tautologies ─────────────────────────────

// Gap-1 RED: constant arithmetic tautologies

#[test]
fn simple_solver_proves_constant_ge_zero() {
    // "0 >= 0" is a constant arithmetic tautology → Proven
    assert_eq!(prove("0 >= 0"), SolverOutcome::Proven);
}

#[test]
fn simple_solver_proves_constant_gt() {
    // "5 > 3" is a constant arithmetic tautology → Proven
    assert_eq!(prove("5 > 3"), SolverOutcome::Proven);
}

#[test]
fn simple_solver_proves_constant_eq() {
    // "7 == 7" is a constant arithmetic tautology → Proven
    assert_eq!(prove("7 == 7"), SolverOutcome::Proven);
}

#[test]
fn simple_solver_proves_reflexive_ge() {
    // "x >= x" is reflexively true → Proven
    assert_eq!(prove("x >= x"), SolverOutcome::Proven);
}

#[test]
fn simple_solver_proves_reflexive_eq() {
    // "x == x" is reflexively true → Proven
    assert_eq!(prove("x == x"), SolverOutcome::Proven);
}

#[test]
fn simple_solver_unconstrained_ge_remains_unsupported() {
    // "x >= 0" without constraints → Unsupported (can't prove without bounds)
    assert_eq!(prove("x >= 0"), SolverOutcome::Unsupported);
}

// Gap-1 RED: solve_with_constraints

fn prove_with(predicate: &str, constraints: &[&str]) -> SolverOutcome {
    let solver = SimpleSolver;
    let oblig = ProofObligation {
        predicate: predicate.into(),
        role: ClauseRole::Ensures,
        scope: "test".into(),
    };
    solver.solve_with_constraints(&oblig, constraints)
}

#[test]
fn simple_solver_proves_result_ge_zero_from_sum_of_nonneg() {
    // Given a >= 0, b >= 0, result = a + b → prove result >= 0
    // Evidence: result = checked_add(a, b) where a >= 0, b >= 0
    let outcome = prove_with("result >= 0", &["a >= 0", "b >= 0", "result = a + b"]);
    assert_eq!(
        outcome,
        SolverOutcome::Proven,
        "result >= 0 must be Proven given a >= 0, b >= 0, result = a + b"
    );
}

#[test]
fn simple_solver_proves_via_transitivity() {
    // Given x >= 5 → prove x >= 0 (lb(x)=5 >= 0)
    let outcome = prove_with("x >= 0", &["x >= 5"]);
    assert_eq!(
        outcome,
        SolverOutcome::Proven,
        "x >= 0 must be Proven given x >= 5 (transitivity)"
    );
}

#[test]
fn simple_solver_cannot_prove_false_constraint() {
    // Given a >= 0 → cannot prove a >= 10 (lower bound is 0, not 10)
    let outcome = prove_with("a >= 10", &["a >= 0"]);
    assert_eq!(
        outcome,
        SolverOutcome::Unsupported,
        "a >= 10 must NOT be Proven when only a >= 0 is known"
    );
}

#[test]
fn known_refinement_positive_money_ident_is_proven() {
    // "PositiveMoney(amount)" is a known refinement tautology → Proven
    assert_eq!(prove("PositiveMoney(amount)"), SolverOutcome::Proven);
}

#[test]
fn known_refinement_non_empty_text_ident_is_proven() {
    assert_eq!(prove("NonEmptyText(description)"), SolverOutcome::Proven);
}

#[test]
fn known_refinement_email_ident_is_proven() {
    assert_eq!(prove("Email(recipient)"), SolverOutcome::Proven);
}

#[test]
fn unknown_predicate_remains_unsupported() {
    // Unknown predicates still return Unsupported (not affected by known refinements)
    assert_eq!(prove("amount > 0"), SolverOutcome::Unsupported);
    assert_eq!(prove("complex_predicate(x, y)"), SolverOutcome::Unsupported);
}

#[test]
fn known_refinement_requires_single_identifier_argument() {
    // Malformed pattern: no closing paren, or multiple args → Unsupported
    assert_eq!(prove("PositiveMoney"), SolverOutcome::Unsupported);
    assert_eq!(prove("PositiveMoney()"), SolverOutcome::Unsupported); // empty ident
}
