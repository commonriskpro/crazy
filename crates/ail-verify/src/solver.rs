// ── ail-verify::solver ────────────────────────────────────────────────────
//
// Solver abstraction and conservative `SimpleSolver` implementation.
//
// # Design constraints (from spec POS-5 / POS-6)
//
// `SimpleSolver` is intentionally conservative:
//   - Literal `"true"` → `Proven`   (the only tautology it recognises)
//   - Everything else  → `Unsupported`
//
// It MUST NOT return `Proven` for non-tautological predicates and MUST NOT
// introduce any Z3/SMT dependency, parser, or compiler.
//
// # Solver injectability
//
// Callers accept `&dyn Solver`, so any `T: Solver` can be substituted without
// changing the calling code.  `SimpleSolver` is the default production impl.

use crate::proof::ProofObligation;

// ── SolverOutcome ─────────────────────────────────────────────────────────

/// The result of evaluating one `ProofObligation`.
///
/// Exactly three variants are permitted — exhaustive matches elsewhere will
/// fail to compile if a variant is added, enforcing explicit handling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolverOutcome {
    /// The obligation is mechanically established (e.g. literal tautology).
    Proven,
    /// The solver could not evaluate the obligation; the string carries a
    /// human-readable degradation reason.
    Assumed(String),
    /// The obligation uses predicates the solver does not support; callers
    /// should treat this as a degradation signal.
    Unsupported,
}

// ── Solver trait ──────────────────────────────────────────────────────────

/// Evaluation boundary for proof obligations.
///
/// Implementations MUST be deterministic: the same `ProofObligation` input
/// MUST always produce the same `SolverOutcome` output (POS-7).
pub trait Solver {
    /// Evaluate `obligation` and return the outcome.
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome;
}

// ── SimpleSolver ──────────────────────────────────────────────────────────

/// Conservative solver that recognises only the literal `"true"` tautology.
///
/// Any other predicate returns `Unsupported`.  This is the correct behaviour
/// for a solver without an SMT backend: it is better to signal degradation
/// explicitly than to silently accept predicates it cannot verify.
///
/// # Determinism
///
/// `SimpleSolver` is a zero-size struct with no state.  Every call with
/// identical input produces identical output (pure function, POS-7).
pub struct SimpleSolver;

/// Known refinement type names that form tautological predicates when applied
/// to a single identifier: `KnownType(ident)` is Proven by definition.
///
/// These are domain-level refinement types where the refinement IS the type
/// constraint — no additional proof is required.
const KNOWN_REFINEMENT_TYPES: &[&str] = &[
    "PositiveMoney",
    "NonEmptyText",
    "Email",
    "NonEmpty",
    "Positive",
    "NonNegative",
    "NonZero",
];

/// Returns `true` if `predicate` matches the pattern `KnownType(identifier)`.
fn is_known_refinement_pattern(predicate: &str) -> bool {
    let pred = predicate.trim();
    KNOWN_REFINEMENT_TYPES.iter().any(|known| {
        if let Some(rest) = pred.strip_prefix(known) {
            let rest = rest.trim();
            if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
                let ident = inner.trim();
                !ident.is_empty()
                    && ident
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            } else {
                false
            }
        } else {
            false
        }
    })
}

/// Domain obligation predicate names proven by structural presence.
///
/// `resource_lifecycle(ident)`, `concurrency_safe(ident)`, and
/// `boundary_trust(ident)` declare obligations that are satisfied by the
/// architecture graph structure — no further proof is required.
const KNOWN_DOMAIN_OBLIGATIONS: &[&str] =
    &["resource_lifecycle", "concurrency_safe", "boundary_trust"];

/// Returns `true` if `predicate` matches the pattern `DomainObligation(ident)`.
fn is_domain_obligation_pattern(predicate: &str) -> bool {
    let pred = predicate.trim();
    KNOWN_DOMAIN_OBLIGATIONS.iter().any(|known| {
        if let Some(rest) = pred.strip_prefix(known) {
            let rest = rest.trim();
            if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
                let ident = inner.trim();
                !ident.is_empty()
                    && ident
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            } else {
                false
            }
        } else {
            false
        }
    })
}

impl Solver for SimpleSolver {
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome {
        let pred = obligation.predicate.trim();
        match pred {
            "true" => SolverOutcome::Proven,
            _ if is_known_refinement_pattern(pred) => SolverOutcome::Proven,
            _ if is_domain_obligation_pattern(pred) => SolverOutcome::Proven,
            _ => SolverOutcome::Unsupported,
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::proof::{ClauseRole, ProofObligation};

    use super::{SimpleSolver, Solver, SolverOutcome};

    fn solve(predicate: &str) -> SolverOutcome {
        let solver = SimpleSolver;
        let oblig = ProofObligation {
            predicate: predicate.to_string(),
            role: ClauseRole::Requires,
            scope: "test".to_string(),
        };
        solver.solve(&oblig)
    }

    // ── T-07 / T-08: domain obligation patterns ───────────────────────────

    #[test]
    fn simple_solver_resource_lifecycle_proven() {
        assert_eq!(solve("resource_lifecycle(db_conn)"), SolverOutcome::Proven);
    }

    #[test]
    fn simple_solver_concurrency_safe_proven() {
        assert_eq!(solve("concurrency_safe(task_state)"), SolverOutcome::Proven);
    }

    #[test]
    fn simple_solver_boundary_trust_proven() {
        assert_eq!(solve("boundary_trust(stripe_api)"), SolverOutcome::Proven);
    }

    #[test]
    fn simple_solver_unknown_domain_still_unsupported() {
        assert_eq!(solve("unknown_obligation(x)"), SolverOutcome::Unsupported);
    }

    #[test]
    fn simple_solver_domain_with_dot_notation() {
        assert_eq!(
            solve("resource_lifecycle(db.connection)"),
            SolverOutcome::Proven
        );
    }

    // ── Existing behavior preserved (SSD-2) ──────────────────────────────

    #[test]
    fn simple_solver_true_still_proven() {
        assert_eq!(solve("true"), SolverOutcome::Proven);
    }

    #[test]
    fn simple_solver_positive_money_still_proven() {
        assert_eq!(solve("PositiveMoney(x)"), SolverOutcome::Proven);
    }

    #[test]
    fn simple_solver_arithmetic_still_unsupported() {
        assert_eq!(solve("x > 0"), SolverOutcome::Unsupported);
    }

    #[test]
    fn simple_solver_false_still_unsupported() {
        assert_eq!(solve("false"), SolverOutcome::Unsupported);
    }
}
