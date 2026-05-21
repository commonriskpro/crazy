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
// - `report`          — `VerificationState`, `VerificationEntry`,
//                       `VerificationReport`, `SummaryCounts`
//                       (six-state enum + priority-based summary + schema fields).
// - `checker`         — `Checker::check(&SemanticGraph) -> VerificationReport`
//                       (shallow type/effect/capability classification).
// - `type_checker`    — `TypeChecker::check` — step 7 of pipeline: validate
//                       TypeFacts, generics arity, emit E_GENERIC_ARITY.
// - `effect_checker`  — `EffectChecker::check` — step 8 of pipeline: compare
//                       declared vs inferred effects, E_EFFECT_UNDECLARED /
//                       E_EFFECT_UNUSED.
// - `proof`           — `ClauseRole`, `ProofObligation`, `ObligationState`,
//                       `ObligationResult`, `ProofObligationPipeline`
//                       (five-stage pipeline: generate→simplify→solve→compose→degrade).
// - `contract_checker`— `ContractChecker` backed by `&dyn Solver`.
// - `solver`          — `SolverOutcome`, `Solver` trait, `SimpleSolver`
//                       (conservative literal-only solver; injectable via `&dyn Solver`).

pub mod checker;
pub mod contract_checker;
pub mod effect_checker;
pub mod package_checker;
pub mod proof;
pub mod report;
pub mod solver;
pub mod type_checker;

#[cfg(feature = "z3-solver")]
pub mod z3_solver;

pub use effect_checker::EffectChecker;
pub use package_checker::PackageTrustChecker;
pub use type_checker::TypeChecker;
