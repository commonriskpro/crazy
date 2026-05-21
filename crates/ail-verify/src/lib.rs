// ── ail-verify ────────────────────────────────────────────────────────────
//
// Pure type/effect/capability verification layer for the AIL semantic graph.
//
// # Phase 5 scope
//
// `ail-verify` reads a `&SemanticGraph` (from `ail-core`) and emits a
// `VerificationReport` without performing any I/O, mutations, or external
// solver calls.  It does NOT wire into `ChangeSetOp::Verify` — that remains
// a no-op placeholder.
//
// # Module layout
//
// - `report`  — `VerificationState`, `VerificationEntry`, `VerificationReport`
//               (six-state enum + priority-based summary).
// - `checker` — `Checker::check(&SemanticGraph) -> VerificationReport`
//               (pure graph walker; no I/O, no mutation).
// - `proof`   — `ClauseRole`, `ProofObligation`
//               (proof obligation value types consumed by solvers).
// - `solver`  — `SolverOutcome`, `Solver` trait, `SimpleSolver`
//               (conservative literal-only solver; injectable via `&dyn Solver`).

pub mod checker;
pub mod contract_checker;
pub mod diagnostic;
pub mod effect_checker;
pub mod package_checker;
pub mod proof;
pub mod report;
pub mod solver;
pub mod type_checker;

#[cfg(feature = "z3-solver")]
pub mod z3_solver;

pub use package_checker::PackageTrustChecker;
