// ── ail-verify::contract_checker ─────────────────────────────────────────
//
// Pure contract materialization pipeline for Phase 6.
//
// `ContractChecker` traverses a `&SemanticGraph`, processes each node's
// `contract_clauses` through an injected `&dyn Solver`, and emits
// `VerificationEntry` items into a `VerificationReport`.
//
// # Mapping rules (spec RCM-2 / RCM-3 / RCM-4)
//
// | Solver outcome      | VerificationState | Evidence             |
// |---------------------|-------------------|----------------------|
// | Proven              | RuntimeChecked    | None                 |
// | Unsupported         | Assumed           | Some(degradation msg)|
// | literal "false"     | Failed            | None                 |
//
// "false" is handled BEFORE solver dispatch — it is a conservative literal
// violation check, not an SMT query.
//
// # Exclusions
//
// - No runtime host execution
// - No Z3/SMT dependency, parser, or compiler
// - No side effects; every call is pure given the same graph and solver

use ail_core::semantic_graph::SemanticGraph;

use crate::diagnostic::{Diagnostic, E_CONTRACT_VIOLATED};
use crate::proof::{ClauseRole, ProofObligation};
use crate::report::{VerificationEntry, VerificationReport, VerificationState};
use crate::solver::{Solver, SolverOutcome};

// ── ContractChecker ───────────────────────────────────────────────────────

/// Pure contract clause evaluator backed by an injected `Solver`.
///
/// Constructed with a `&dyn Solver` so any implementation can be substituted
/// without modifying this type (RCM-1).
pub struct ContractChecker<'s> {
    solver: &'s dyn Solver,
}

impl<'s> ContractChecker<'s> {
    /// Create a new `ContractChecker` backed by `solver`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use ail_verify::contract_checker::ContractChecker;
    /// use ail_verify::solver::SimpleSolver;
    ///
    /// let solver = SimpleSolver;
    /// let checker = ContractChecker::new(&solver);
    /// ```
    pub fn new(solver: &'s dyn Solver) -> Self {
        Self { solver }
    }

    /// Walk `graph` and materialize `VerificationEntry` items for every
    /// contract clause found in node `contract_clauses`.
    ///
    /// Nodes without `contract_clauses` are silently skipped; they produce
    /// no entries (RCM-5: no `Unverified` in contract reports).
    ///
    /// Requires clauses are emitted before ensures clauses within each node.
    pub fn check(&self, graph: &SemanticGraph) -> VerificationReport {
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        for node in &graph.nodes {
            if let Some(clauses) = &node.contract_clauses {
                let scope = node.name.clone();

                // Requires first, then Ensures — declaration order preserved.
                for predicate in &clauses.requires {
                    let entry = self.evaluate(predicate, ClauseRole::Requires, &scope);
                    if entry.state == VerificationState::Failed {
                        diagnostics.push(
                            Diagnostic::error(E_CONTRACT_VIOLATED, node.id).with_evidence(format!(
                                "requires clause failed for '{scope}': {predicate}"
                            )),
                        );
                    }
                    entries.push(entry);
                }
                for predicate in &clauses.ensures {
                    let entry = self.evaluate(predicate, ClauseRole::Ensures, &scope);
                    if entry.state == VerificationState::Failed {
                        diagnostics.push(
                            Diagnostic::error(E_CONTRACT_VIOLATED, node.id).with_evidence(format!(
                                "ensures clause failed for '{scope}': {predicate}"
                            )),
                        );
                    }
                    entries.push(entry);
                }
            }
        }
        VerificationReport {
            entries,
            diagnostics,
        }
    }

    /// Evaluate one clause predicate and return the corresponding entry.
    ///
    /// Conservative literal check for `"false"` is applied first to avoid
    /// invoking the solver on a known-violated predicate.
    fn evaluate(&self, predicate: &str, role: ClauseRole, scope: &str) -> VerificationEntry {
        // Conservative literal violation (RCM-4): "false" → Failed immediately.
        if predicate.trim() == "false" {
            return VerificationEntry {
                claim: format!("{}: {}", role_label(role), predicate),
                state: VerificationState::Failed,
                scope: scope.to_string(),
                evidence: None,
            };
        }

        let obligation = ProofObligation {
            predicate: predicate.to_string(),
            role,
        };

        let (state, evidence) = match self.solver.solve(&obligation) {
            // RCM-2: Proven → RuntimeChecked; no evidence needed
            SolverOutcome::Proven => (VerificationState::RuntimeChecked, None),
            // RCM-3 + RCM-6: Unsupported → Assumed with non-empty degradation message
            SolverOutcome::Unsupported => (
                VerificationState::Assumed,
                Some(format!("solver cannot evaluate predicate: {predicate}")),
            ),
            // Assumed(reason) from solver: carry the degradation message through
            SolverOutcome::Assumed(reason) => (VerificationState::Assumed, Some(reason)),
        };

        VerificationEntry {
            claim: format!("{}: {}", role_label(role), predicate),
            state,
            scope: scope.to_string(),
            evidence,
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn role_label(role: ClauseRole) -> &'static str {
    match role {
        ClauseRole::Requires => "requires",
        ClauseRole::Ensures => "ensures",
    }
}
