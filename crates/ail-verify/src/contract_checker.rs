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

// ── Stable contract diagnostic categories ─────────────────────────────────

/// Stable category for a failed `requires` contract clause.
pub const CONTRACT_DIAGNOSTIC_CATEGORY_PRECONDITION_FAILED: &str = "contract.precondition_failed";

/// Stable category for a failed `ensures` contract clause.
pub const CONTRACT_DIAGNOSTIC_CATEGORY_POSTCONDITION_FAILED: &str = "contract.postcondition_failed";

/// Stable category for a failed invariant `requires` contract clause.
pub const CONTRACT_DIAGNOSTIC_CATEGORY_INVARIANT_PRECONDITION_FAILED: &str =
    "contract.invariant_precondition_failed";

/// Stable category for a failed invariant `ensures` contract clause.
pub const CONTRACT_DIAGNOSTIC_CATEGORY_INVARIANT_POSTCONDITION_FAILED: &str =
    "contract.invariant_postcondition_failed";

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
        use ail_core::semantic_graph::NodeKind;
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        for node in &graph.nodes {
            if let Some(clauses) = &node.contract_clauses {
                let scope = node.name.clone();
                let is_invariant = node.kind == NodeKind::Invariant;

                // Requires first, then Ensures — declaration order preserved.
                // Invariant nodes use "invariant-requires:" / "invariant-ensures:" claim prefixes.
                for predicate in &clauses.requires {
                    let entry = if is_invariant {
                        self.evaluate_invariant(predicate, ClauseRole::Requires, &scope)
                    } else {
                        self.evaluate(predicate, ClauseRole::Requires, &scope)
                    };
                    if entry.state == VerificationState::Failed {
                        diagnostics.push(contract_failure_diagnostic(
                            node.id,
                            &scope,
                            predicate,
                            ClauseRole::Requires,
                            is_invariant,
                        ));
                    }
                    entries.push(entry);
                }
                for predicate in &clauses.ensures {
                    let entry = if is_invariant {
                        self.evaluate_invariant(predicate, ClauseRole::Ensures, &scope)
                    } else {
                        self.evaluate(predicate, ClauseRole::Ensures, &scope)
                    };
                    if entry.state == VerificationState::Failed {
                        diagnostics.push(contract_failure_diagnostic(
                            node.id,
                            &scope,
                            predicate,
                            ClauseRole::Ensures,
                            is_invariant,
                        ));
                    }
                    entries.push(entry);
                }
            }
        }
        canonicalize_contract_diagnostics(&mut diagnostics);
        VerificationReport {
            entries,
            diagnostics,
            ..Default::default()
        }
    }

    /// Evaluate one clause predicate for an invariant node.
    ///
    /// Same logic as `evaluate` but uses `"invariant-requires:"` /
    /// `"invariant-ensures:"` claim prefixes to distinguish invariant obligations
    /// from regular function contract clauses (REQ-13).
    fn evaluate_invariant(
        &self,
        predicate: &str,
        role: ClauseRole,
        scope: &str,
    ) -> VerificationEntry {
        let prefix = match role {
            ClauseRole::Requires => "invariant-requires:",
            ClauseRole::Ensures => "invariant-ensures:",
        };
        if predicate.trim() == "false" {
            return VerificationEntry {
                claim: format!("{prefix} {predicate}"),
                state: VerificationState::Failed,
                scope: scope.to_string(),
                evidence: None,
                blocking: true,
                repair_options: vec![],
            };
        }
        let obligation = ProofObligation {
            predicate: predicate.to_string(),
            role,
            scope: String::new(),
        };
        let (state, evidence) = match self.solver.solve(&obligation) {
            SolverOutcome::Proven => (VerificationState::RuntimeChecked, None),
            SolverOutcome::Unsupported => (
                VerificationState::Assumed,
                Some(format!(
                    "solver cannot evaluate invariant predicate: {predicate}"
                )),
            ),
            SolverOutcome::Assumed(reason) => (VerificationState::Assumed, Some(reason)),
        };
        let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
        VerificationEntry {
            claim: format!("{prefix} {predicate}"),
            state,
            scope: scope.to_string(),
            evidence,
            blocking,
            repair_options: vec![],
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
                blocking: true,
                repair_options: vec![],
            };
        }

        let obligation = ProofObligation {
            predicate: predicate.to_string(),
            role,
            scope: String::new(),
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

        let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
        VerificationEntry {
            claim: format!("{}: {}", role_label(role), predicate),
            state,
            scope: scope.to_string(),
            evidence,
            blocking,
            repair_options: vec![],
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

fn contract_failure_category(role: ClauseRole, is_invariant: bool) -> &'static str {
    match (role, is_invariant) {
        (ClauseRole::Requires, false) => CONTRACT_DIAGNOSTIC_CATEGORY_PRECONDITION_FAILED,
        (ClauseRole::Ensures, false) => CONTRACT_DIAGNOSTIC_CATEGORY_POSTCONDITION_FAILED,
        (ClauseRole::Requires, true) => CONTRACT_DIAGNOSTIC_CATEGORY_INVARIANT_PRECONDITION_FAILED,
        (ClauseRole::Ensures, true) => CONTRACT_DIAGNOSTIC_CATEGORY_INVARIANT_POSTCONDITION_FAILED,
    }
}

fn contract_failure_diagnostic(
    target: ail_core::semantic_graph::NodeRef,
    scope: &str,
    predicate: &str,
    role: ClauseRole,
    is_invariant: bool,
) -> Diagnostic {
    let category = contract_failure_category(role, is_invariant);
    let predicate = predicate.trim();
    Diagnostic::error(E_CONTRACT_VIOLATED, target).with_evidence(format!(
        "category={category}; role={}; scope='{scope}'; predicate={predicate}",
        role_label(role),
    ))
}

fn canonicalize_contract_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        contract_diagnostic_category_rank(a)
            .cmp(&contract_diagnostic_category_rank(b))
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.code.cmp(&b.code))
            .then_with(|| a.evidence.cmp(&b.evidence))
    });
    diagnostics.dedup();
}

fn contract_diagnostic_category_rank(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.evidence.as_deref().and_then(diagnostic_category) {
        Some(CONTRACT_DIAGNOSTIC_CATEGORY_PRECONDITION_FAILED) => 0,
        Some(CONTRACT_DIAGNOSTIC_CATEGORY_POSTCONDITION_FAILED) => 1,
        Some(CONTRACT_DIAGNOSTIC_CATEGORY_INVARIANT_PRECONDITION_FAILED) => 2,
        Some(CONTRACT_DIAGNOSTIC_CATEGORY_INVARIANT_POSTCONDITION_FAILED) => 3,
        _ => 4,
    }
}

fn diagnostic_category(evidence: &str) -> Option<&str> {
    evidence
        .strip_prefix("category=")
        .and_then(|rest| rest.split_once(';').map(|(category, _)| category))
}
