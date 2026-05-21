// ── ail-verify::proof ─────────────────────────────────────────────────────
//
// Proof obligation value types consumed by the `Solver` trait and
// `ContractChecker`.
//
// # Types
//
// - `ClauseRole`        — whether a clause is a precondition or postcondition.
// - `ProofObligation`   — a single predicate string tagged with its role.
//
// These are plain data types with no behaviour; all logic lives in the solver.

/// Whether a contract clause is a precondition (`Requires`) or a
/// postcondition (`Ensures`).
///
/// Exactly two variants are permitted — exhaustive matches elsewhere in the
/// codebase will fail to compile if a variant is added, which is intentional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClauseRole {
    /// A precondition: the caller is responsible for making this hold.
    Requires,
    /// A postcondition: the implementation promises this holds on return.
    Ensures,
}

/// One proof obligation produced from a contract clause.
///
/// `predicate` is the raw clause string as extracted from `ContractClauses`.
/// `role` indicates whether it came from `requires` or `ensures`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofObligation {
    /// The raw predicate expression, e.g. `"x > 0"` or `"true"`.
    pub predicate: String,
    /// Whether this is a precondition or postcondition.
    pub role: ClauseRole,
}
