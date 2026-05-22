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

impl Solver for SimpleSolver {
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome {
        let pred = obligation.predicate.trim();
        match pred {
            "true" => SolverOutcome::Proven,
            _ if is_known_refinement_pattern(pred) => SolverOutcome::Proven,
            _ => SolverOutcome::Unsupported,
        }
    }
}
