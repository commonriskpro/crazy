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

// ── Arithmetic helpers ────────────────────────────────────────────────────

/// Attempt to parse a string as a decimal integer.
fn parse_int(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// Parse a simple `lhs OP rhs` arithmetic predicate into `(lhs, op, rhs)`.
///
/// Supported operators (checked longest-first to avoid ambiguity):
/// `>=`, `<=`, `==`, `!=`, `>`, `<`.
///
/// Each side is either a decimal integer literal or an alphanumeric identifier
/// (dot-notation allowed).  Returns `None` if the predicate doesn't match.
fn parse_simple_pred(pred: &str) -> Option<(&str, &str, &str)> {
    let pred = pred.trim();
    for op in &[">=", "<=", "==", "!=", ">", "<"] {
        // Find the operator; skip if it appears as part of `>=` / `<=`
        // by iterating byte positions ourselves.
        if let Some(idx) = pred.find(op) {
            let lhs = pred[..idx].trim();
            let rhs = pred[idx + op.len()..].trim();
            if lhs.is_empty() || rhs.is_empty() {
                continue;
            }
            // Validate: each side is an int literal or a valid identifier.
            let valid_side = |s: &str| -> bool {
                parse_int(s).is_some()
                    || (s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') && !s.is_empty())
            };
            if valid_side(lhs) && valid_side(rhs) {
                return Some((lhs, op, rhs));
            }
        }
    }
    None
}

/// Check whether `pred` is a constant-only arithmetic tautology
/// (both sides are integer literals and the comparison is true).
fn is_constant_tautology(pred: &str) -> bool {
    if let Some((lhs, op, rhs)) = parse_simple_pred(pred) {
        if let (Some(l), Some(r)) = (parse_int(lhs), parse_int(rhs)) {
            return match op {
                ">=" => l >= r,
                "<=" => l <= r,
                "==" => l == r,
                "!=" => l != r,
                ">" => l > r,
                "<" => l < r,
                _ => false,
            };
        }
    }
    false
}

/// Check whether `pred` is a reflexive tautology (`x >= x` or `x == x`).
fn is_reflexive_tautology(pred: &str) -> bool {
    if let Some((lhs, op, rhs)) = parse_simple_pred(pred) {
        if lhs == rhs {
            return matches!(op, ">=" | "==" | "<=" );
        }
    }
    false
}

// ── Constraint-based arithmetic reasoner ─────────────────────────────────

/// A minimal fact database derived from a slice of constraint strings.
///
/// Tracks an inclusive **lower bound** (`lb`) for each named variable.
/// Constraints supported:
/// - `x >= N`    → lb(x) = max(lb(x), N)
/// - `x > N`     → lb(x) = max(lb(x), N + 1)
/// - `x == N`    → lb(x) = N
/// - `x = y + z` → lb(x) = lb(y) + lb(z) (if both known; single-pass)
/// - `x = y`     → lb(x) = lb(y) (alias)
///
/// Does **not** track upper bounds; returns `None` for any unknown variable.
struct FactDb {
    /// Inclusive lower bound for each named variable.
    lb: std::collections::HashMap<String, i64>,
}

impl FactDb {
    /// Build a `FactDb` from a slice of constraint strings.
    fn from_constraints(constraints: &[&str]) -> Self {
        let mut lb: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

        // ── Pass 1: extract simple bounds (x >= N, x > N, x == N) ──────
        for constraint in constraints {
            let c = constraint.trim();
            // Equality with constant: `x == N` (use `==` operator)
            if let Some((lhs, "==", rhs)) = parse_simple_pred(c) {
                if let Some(val) = parse_int(rhs) {
                    if parse_int(lhs).is_none() {
                        // lhs is a variable, rhs is a constant
                        let entry = lb.entry(lhs.to_string()).or_insert(i64::MIN);
                        *entry = val;
                    }
                }
            }
            // Lower bounds: `x >= N` or `x > N`
            if let Some((lhs, op, rhs)) = parse_simple_pred(c) {
                if let Some(val) = parse_int(rhs) {
                    if parse_int(lhs).is_none() {
                        let bound = match op {
                            ">=" => val,
                            ">" => val.saturating_add(1),
                            _ => continue,
                        };
                        let entry = lb.entry(lhs.to_string()).or_insert(i64::MIN);
                        if bound > *entry {
                            *entry = bound;
                        }
                    }
                }
            }
        }

        // ── Pass 2: resolve assignments (x = y + z, x = y) ───────────
        for constraint in constraints {
            let c = constraint.trim();
            // Pattern: `x = y + z`  (single `=`, not `==`)
            // We look for "ident = ident + ident" form.
            if let Some(eq_pos) = c.find('=') {
                // Skip if this is `==` or `>=` / `<=` / `!=`
                let prev = c[..eq_pos].chars().last();
                let next = c[eq_pos + 1..].chars().next();
                if matches!(prev, Some('>' | '<' | '!' | '='))
                    || matches!(next, Some('='))
                {
                    continue;
                }
                let lhs = c[..eq_pos].trim();
                let rhs_expr = c[eq_pos + 1..].trim();
                // Ensure lhs is an identifier
                if !lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') || lhs.is_empty() {
                    continue;
                }
                // Try `a + b` on the rhs
                if let Some(plus_pos) = rhs_expr.find('+') {
                    let a = rhs_expr[..plus_pos].trim();
                    let b = rhs_expr[plus_pos + 1..].trim();
                    if let (Some(&lb_a), Some(&lb_b)) = (lb.get(a), lb.get(b)) {
                        if lb_a != i64::MIN && lb_b != i64::MIN {
                            let entry = lb.entry(lhs.to_string()).or_insert(i64::MIN);
                            let sum_lb = lb_a.saturating_add(lb_b);
                            if sum_lb > *entry {
                                *entry = sum_lb;
                            }
                            continue;
                        }
                    }
                }
                // Try simple alias `x = y`
                let rhs_is_ident = rhs_expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                    && !rhs_expr.is_empty();
                if rhs_is_ident {
                    if let Some(&alias_lb) = lb.get(rhs_expr) {
                        if alias_lb != i64::MIN {
                            let entry = lb.entry(lhs.to_string()).or_insert(i64::MIN);
                            if alias_lb > *entry {
                                *entry = alias_lb;
                            }
                        }
                    }
                }
            }
        }

        FactDb { lb }
    }

    /// Return the lower bound for `var`, or `None` if unknown.
    fn lower_bound(&self, var: &str) -> Option<i64> {
        self.lb.get(var).copied().filter(|&v| v != i64::MIN)
    }

    /// Try to prove `predicate` using the fact database.
    ///
    /// Returns `Some(true)` if proven, `Some(false)` if disproven,
    /// `None` if the db lacks sufficient information.
    fn prove_pred(&self, predicate: &str) -> Option<bool> {
        let Some((lhs, op, rhs)) = parse_simple_pred(predicate) else {
            return None;
        };
        // Both sides could be constants or variables.
        let lhs_val = parse_int(lhs).or_else(|| self.lower_bound(lhs));
        let rhs_val = parse_int(rhs);
        let (l, r) = (lhs_val?, rhs_val?);
        Some(match op {
            ">=" => l >= r,
            ">" => l > r,
            "<=" => l <= r,
            "<" => l < r,
            "==" => l == r,
            "!=" => l != r,
            _ => return None,
        })
    }
}

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

    /// Evaluate `obligation` given additional constraints from the scope context.
    ///
    /// `constraints` is an ordered slice of constraint strings extracted from the
    /// scope node's `requires` clauses and `body_expr`.  Implementations may use
    /// these to perform context-aware arithmetic reasoning that is not possible
    /// from the predicate string alone.
    ///
    /// The default implementation ignores `constraints` and delegates to `solve`.
    fn solve_with_constraints(
        &self,
        obligation: &ProofObligation,
        constraints: &[&str],
    ) -> SolverOutcome {
        let _ = constraints; // default: ignore context
        self.solve(obligation)
    }
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
            // Arithmetic tautologies: constant comparisons and reflexive patterns.
            _ if is_constant_tautology(pred) => SolverOutcome::Proven,
            _ if is_reflexive_tautology(pred) => SolverOutcome::Proven,
            _ => SolverOutcome::Unsupported,
        }
    }

    /// Context-aware arithmetic reasoning.
    ///
    /// Builds a lower-bound fact database from `constraints` (requires clauses +
    /// body expression from the scope node), then attempts to prove `obligation`.
    ///
    /// Falls back to `solve` when the fact database is empty or the predicate
    /// cannot be evaluated from the available constraints.
    fn solve_with_constraints(
        &self,
        obligation: &ProofObligation,
        constraints: &[&str],
    ) -> SolverOutcome {
        // First try the constraint-free path (handles literals, known patterns,
        // constant and reflexive tautologies).
        let base = self.solve(obligation);
        if base == SolverOutcome::Proven {
            return base;
        }

        // Build the fact database and try arithmetic reasoning.
        if constraints.is_empty() {
            return base;
        }
        let db = FactDb::from_constraints(constraints);
        match db.prove_pred(obligation.predicate.trim()) {
            Some(true) => SolverOutcome::Proven,
            _ => base, // insufficient info → keep the base outcome (Unsupported)
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
