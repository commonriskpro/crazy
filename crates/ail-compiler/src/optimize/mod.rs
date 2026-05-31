// ── ail-compiler::optimize ────────────────────────────────────────────────
//
// Conservative ANF optimizations. These passes only rewrite pure local
// expressions and never remove top-level bindings or effect/resource nodes.
//
// # Passes (in pipeline order)
//
// 1. `eliminate_dead_pure`  — Remove pure non-final Seq elements whose
//    results are discarded (extends existing dead-let elimination).
// 2. `inline_small_pure`    — Inline Lambda bindings with ≤3 ANF nodes and
//    no effects at all call sites.
// 3. `cse_bindings`         — Common Subexpression Elimination: within each
//    binding's let-chain, replace duplicate pure sub-expressions with a Var
//    reference to the first binding.
// 4. `optimize_bindings`    — Constant folding + dead-let elimination
//    (existing pass; runs last to clean up aliases introduced by CSE).

mod const_fold;
mod cse;
mod dce;
mod diagnostics;
mod inline;
mod purity;

pub use const_fold::optimize_bindings;
pub use cse::cse_bindings;
pub use dce::eliminate_dead_pure;
pub use diagnostics::{
    OptimizerDiagnostic, OptimizerDiagnosticConfig, OptimizerDiagnostics, OptimizerIssueKind,
    OptimizerPass, OptimizerSeverity, diagnose_optimizer, diagnose_optimizer_with_config,
    optimize_bindings_with_diagnostics, redacted_binding_descriptor, redacted_function_descriptor,
    redacted_node_descriptor,
};
pub use inline::inline_small_pure;

pub(crate) use purity::{anf_node_count, is_pure, purity_blocking_reason, uses_var};

#[cfg(test)]
mod tests;
