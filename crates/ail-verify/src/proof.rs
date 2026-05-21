// ── ail-verify::proof ─────────────────────────────────────────────────────
//
// Proof obligation value types and the `ProofObligationPipeline`.
//
// # Types
//
// - `ClauseRole`             — precondition or postcondition.
// - `ProofObligation`        — predicate string tagged with its role.
// - `ObligationState`        — resolved state of one obligation.
// - `ObligationResult`       — obligation + resolved state.
// - `ProofObligationPipeline`— five-stage pipeline: generate → simplify →
//                              solve → compose → degrade.
//
// # Pipeline stages
//
// 1. **Generate** — extract obligations from `ContractClauses` in graph nodes.
// 2. **Simplify** — resolve literal `"true"` → Proven, `"false"` → Failed
//    immediately (skips solver).
// 3. **Solve**    — dispatch remaining obligations to a `&dyn Solver`.
// 4. **Compose**  — if a node's ensures-proven peers cover the predicate,
//    upgrade `Assumed` → `RuntimeChecked`.
// 5. **Degrade**  — `Unsupported` solver outcomes → `Assumed` with reason.

use ail_core::semantic_graph::SemanticGraph;

use crate::solver::{Solver, SolverOutcome};

// ── ClauseRole ────────────────────────────────────────────────────────────

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

// ── ProofObligation ───────────────────────────────────────────────────────

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
    /// The name of the graph node this obligation came from.
    pub scope: String,
}

// ── ObligationState ───────────────────────────────────────────────────────

/// The resolved state of one `ProofObligation` after the full pipeline.
///
/// Mirrors the six-state model from `verification.md` but limited to the
/// states that an obligation can reach through the proof pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObligationState {
    /// Obligation is mechanically proven (tautology or literal `"true"`).
    Proven,
    /// Obligation was upgraded by contract composition (ensures of a called fn).
    RuntimeChecked,
    /// Obligation could not be proven; accepted with a degradation reason.
    Assumed(String),
    /// Obligation is known to be violated (literal `"false"`).
    Failed,
}

// ── ObligationResult ──────────────────────────────────────────────────────

/// One proof obligation paired with its resolved state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObligationResult {
    /// The obligation that was evaluated.
    pub obligation: ProofObligation,
    /// The state reached after running the full pipeline.
    pub state: ObligationState,
}

// ── ProofObligationPipeline ───────────────────────────────────────────────

/// Five-stage proof obligation pipeline.
///
/// Stages run in order: generate → simplify → solve → compose → degrade.
/// All stages are pure — no I/O, no mutation of the graph.
pub struct ProofObligationPipeline;

impl ProofObligationPipeline {
    /// Run the full pipeline over `graph` using `solver` for SMT-style checks.
    ///
    /// Returns one `ObligationResult` per contract clause found in the graph.
    /// Nodes without `contract_clauses` produce no results.
    ///
    /// # Example
    ///
    /// ```rust
    /// use ail_verify::proof::ProofObligationPipeline;
    /// use ail_verify::solver::SimpleSolver;
    /// use ail_core::semantic_graph::SemanticGraph;
    ///
    /// let graph = SemanticGraph { nodes: vec![], edges: vec![] };
    /// let solver = SimpleSolver;
    /// let results = ProofObligationPipeline::run(&graph, &solver);
    /// assert!(results.is_empty());
    /// ```
    pub fn run(graph: &SemanticGraph, solver: &dyn Solver) -> Vec<ObligationResult> {
        // Stage 1: Generate
        let obligations = Self::generate(graph);

        // Stage 2 + 3 + 4 + 5 (chain per obligation)
        obligations
            .into_iter()
            .map(|ob| Self::resolve(ob, graph, solver))
            .collect()
    }

    // ── Stage 1: Generate ─────────────────────────────────────────────────

    fn generate(graph: &SemanticGraph) -> Vec<ProofObligation> {
        let mut obligations = Vec::new();
        for node in &graph.nodes {
            if let Some(clauses) = &node.contract_clauses {
                let scope = node.name.clone();
                for predicate in &clauses.requires {
                    obligations.push(ProofObligation {
                        predicate: predicate.clone(),
                        role: ClauseRole::Requires,
                        scope: scope.clone(),
                    });
                }
                for predicate in &clauses.ensures {
                    obligations.push(ProofObligation {
                        predicate: predicate.clone(),
                        role: ClauseRole::Ensures,
                        scope: scope.clone(),
                    });
                }
            }
        }
        obligations
    }

    // ── Stages 2–5 for one obligation ────────────────────────────────────

    fn resolve(
        obligation: ProofObligation,
        graph: &SemanticGraph,
        solver: &dyn Solver,
    ) -> ObligationResult {
        let predicate = obligation.predicate.trim();

        // Stage 2: Simplify — literal shortcuts (no solver needed).
        if predicate == "true" {
            return ObligationResult {
                obligation,
                state: ObligationState::Proven,
            };
        }
        if predicate == "false" {
            return ObligationResult {
                obligation,
                state: ObligationState::Failed,
            };
        }

        // Stage 3: Solve — dispatch to solver.
        let outcome = solver.solve(&obligation);

        match outcome {
            SolverOutcome::Proven => ObligationResult {
                obligation,
                state: ObligationState::Proven,
            },
            SolverOutcome::Assumed(reason) => {
                // Stage 4: Compose — check if an ensures clause in the graph
                // covers this predicate, upgrading Assumed → RuntimeChecked.
                if Self::compose_check(predicate, graph) {
                    ObligationResult {
                        obligation,
                        state: ObligationState::RuntimeChecked,
                    }
                } else {
                    // Stage 5: Degrade — keep as Assumed with reason.
                    ObligationResult {
                        obligation,
                        state: ObligationState::Assumed(reason),
                    }
                }
            }
            SolverOutcome::Unsupported => {
                // Stage 4: Compose check before degrading.
                if Self::compose_check(predicate, graph) {
                    ObligationResult {
                        obligation,
                        state: ObligationState::RuntimeChecked,
                    }
                } else {
                    // Stage 5: Degrade — unsupported → Assumed.
                    ObligationResult {
                        obligation,
                        state: ObligationState::Assumed(
                            "solver cannot evaluate predicate; accepted by policy".into(),
                        ),
                    }
                }
            }
        }
    }

    // ── Stage 4: Compose ──────────────────────────────────────────────────

    /// Return `true` if any node in `graph` has an `ensures` clause whose
    /// predicate text exactly matches `predicate`.
    ///
    /// Contract composition: if a called function *ensures* the same predicate
    /// the current obligation requires, the obligation is covered without an
    /// independent proof.
    fn compose_check(predicate: &str, graph: &SemanticGraph) -> bool {
        for node in &graph.nodes {
            if let Some(clauses) = &node.contract_clauses {
                for ensures in &clauses.ensures {
                    if ensures.trim() == predicate {
                        return true;
                    }
                }
            }
        }
        false
    }
}
