// ── ail-verify ────────────────────────────────────────────────────────────
#![allow(clippy::items_after_test_module)]
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
// # G25 scope (verification-pipeline)
//
// Adds resource lifecycle, concurrency safety, boundary/FFI trust, and
// codegen consistency checkers.  Wires them together via a canonical
// `VerificationPipeline` facade.  Extends the proof obligation pipeline with
// first-class ledger entries and degradation tracking.
//
// # Module layout
//
// - `report`             — `VerificationState`, `VerificationEntry`, `VerificationReport`,
//                          `ArtifactHash`, `DegradationEvent` (six-state + priority summary).
// - `checker`            — `Checker::check(&SemanticGraph) -> VerificationReport`
//                          (pure graph walker; no I/O, no mutation).
// - `proof`              — `ClauseRole`, `ProofObligation`, `ObligationLedgerEntry`
//                          (proof obligation value types + ledger).
// - `solver`             — `SolverOutcome`, `Solver` trait, `SimpleSolver`.
// - `resource_checker`   — `ResourceChecker` (affine/linear/shared lifecycle).
// - `concurrency_checker`— `ConcurrencyChecker` (task/channel/shared-state safety).
// - `boundary_checker`   — `BoundaryChecker` (FFI/boundary trust enforcement).
// - `codegen_checker`    — `CodegenChecker` (artifact hash + manifest consistency).
// - `pipeline`           — `VerificationPipeline` (canonical ordered facade).

pub mod boundary_checker;
pub mod checker;
pub mod codegen_checker;
pub mod concurrency_checker;
pub mod contract_checker;
pub mod diagnostic;
pub mod effect_checker;
pub mod package_checker;
pub mod pipeline;
pub mod policy;
pub mod proof;
pub mod report;
pub mod resource_checker;
pub mod solver;
pub mod translation_validator;
pub(crate) mod tv_obligations;
pub mod type_checker;
pub(crate) mod type_diagnostics;
pub(crate) mod type_obligations;
pub(crate) mod type_refinements;

#[cfg(feature = "z3-solver")]
pub mod z3_solver;

pub use package_checker::PackageTrustChecker;
pub use policy::{
    ApprovalRecord, ApprovalStrength, CapabilityGrant, POLICY_ASSUMED_UNAPPROVED,
    POLICY_PROFILE_GATE, POLICY_PUBLIC_API_CHANGED, POLICY_RUNTIME_CHECK_ADVISORY,
    POLICY_SOLVER_DIAGNOSTIC_BLOCKED, POLICY_UNSAFE_BLOCKED, POLICY_UNVERIFIED_PUBLIC_API,
    POLICY_WEAK_ASSUMPTION, PackageTrustEntry, PolicyAudit, PolicyAuditEntry, PolicyDecision,
    PolicyEngine, PolicyInput, PolicyRule, PolicyViolation, PolicyWarning, PublicApiChange,
    StructuralDiff,
};
pub use proof::{
    ClauseRole, ObligationAttempt, ObligationLedgerEntry, ObligationState, ProofObligation,
    ProofObligationPipeline,
};
pub use report::{
    ArtifactHash, DegradationEvent, SolverDiagnostic, SolverDiagnosticStatus, VerificationEntry,
    VerificationReport, VerificationState,
};
pub use translation_validator::{
    E_TV_EFFECT_MALFORMED, E_TV_EFFECT_UNDECLARED, E_TV_INSUFFICIENT_EVIDENCE,
    E_TV_SHAPE_NO_RETURN_TYPE, TranslationValidator,
};
