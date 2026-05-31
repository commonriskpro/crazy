// ── ail-verify::proof ─────────────────────────────────────────────────────
//
// Proof obligation value types and the `ProofObligationPipeline`.
//
// # Types
//
// - `ClauseRole`              — precondition or postcondition.
// - `ProofObligation`         — predicate string tagged with its role.
// - `ObligationState`         — resolved state of one obligation.
// - `ObligationResult`        — obligation + resolved state.
// - `ObligationAttempt`       — one resolution attempt in the ledger.
// - `ObligationLedgerEntry`   — full first-class obligation ledger entry.
// - `ProofObligationPipeline` — five-stage pipeline: generate → simplify →
//                               solve → compose → degrade.
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
//
// # G25 extensions (verification-pipeline)
//
// `ObligationLedgerEntry` wraps `ObligationResult` and adds:
// - `id` — stable per-run identifier (sequential within one pipeline call)
// - `source_stage` — which checker generated the obligation (e.g. "contract")
// - `attempts` — ordered list of resolution steps taken
// - `degradation_reason` — why the final state is lower-confidence, if applicable
// - `repair_options` — suggested repairs, if any
//
// `ProofObligationPipeline::run_with_ledger` returns `Vec<ObligationLedgerEntry>`.
// The original `run` method is preserved unchanged for backward compatibility.

use serde::{Deserialize, Serialize};

use ail_core::semantic_graph::{NodeKind, RefinementStatus, SemanticGraph};

use crate::solver::{Solver, SolverOutcome};

// ── ClauseRole ────────────────────────────────────────────────────────────

/// Whether a contract clause is a precondition (`Requires`) or a
/// postcondition (`Ensures`).
///
/// Exactly two variants are permitted — exhaustive matches elsewhere in the
/// codebase will fail to compile if a variant is added, which is intentional.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

// ── ObligationAttempt ─────────────────────────────────────────────────────

/// One resolution step attempted during proof obligation evaluation.
///
/// The proof pipeline may attempt multiple strategies in order
/// (simplify → solver → compose → degrade).  Each step is recorded as
/// an `ObligationAttempt` so that tooling can explain why an obligation
/// ended in a given state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationAttempt {
    /// The pipeline stage that made this attempt (e.g. `"simplify"`, `"solver"`,
    /// `"compose"`, `"degrade"`).
    pub stage: String,
    /// The outcome of this attempt: `"proven"`, `"failed"`, `"unsupported"`,
    /// `"assumed"`, `"composed"`, or `"degraded"`.
    pub outcome: String,
    /// Optional supporting evidence for this attempt (e.g. the degradation reason).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

// ── ObligationLedgerEntry ─────────────────────────────────────────────────

/// A first-class proof obligation entry in the verification report's
/// obligation ledger.
///
/// Extends `ObligationResult` with tracking fields introduced in G25
/// (verification-pipeline):
/// - `id` — unique identifier within one pipeline run (sequential string).
/// - `source_stage` — which verification stage generated this obligation.
/// - `attempts` — ordered list of resolution strategies attempted.
/// - `degradation_reason` — human-readable explanation of state downgrade, if any.
/// - `repair_options` — suggested fixes the toolchain or user can apply.
///
/// Implements `Serialize`/`Deserialize` so it can be stored in `VerificationReport`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationLedgerEntry {
    /// Unique identifier within one pipeline run (e.g. `"po_1"`, `"po_2"`).
    pub id: String,
    /// The proof obligation this entry tracks.
    pub obligation: ProofObligation,
    /// The final resolved state after all pipeline stages.
    pub state: ObligationState,
    /// Which verification stage generated this obligation
    /// (e.g. `"contract"`, `"resource"`, `"boundary"`, `"concurrency"`, `"policy"`).
    pub source_stage: String,
    /// Ordered list of resolution attempts made during the pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ObligationAttempt>,
    /// Human-readable explanation of why the obligation was downgraded, if it was.
    ///
    /// `None` for `Proven` and `RuntimeChecked` obligations (no degradation).
    /// `Some(_)` for `Assumed`, `Unverified`, and `Failed` obligations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degradation_reason: Option<String>,
    /// Actionable repair suggestions.
    ///
    /// Empty if no automated repairs are available.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_options: Vec<String>,
}

// ── ProofObligationPipeline ───────────────────────────────────────────────

/// Five-stage proof obligation pipeline.
///
/// Stages run in order: generate → simplify → solve → compose → degrade.
/// All stages are pure — no I/O, no mutation of the graph.
pub struct ProofObligationPipeline;

struct GeneratedObligation {
    obligation: ProofObligation,
    source_stage: String,
}

impl ProofObligationPipeline {
    /// Run the full pipeline over `graph` using `solver` for SMT-style checks.
    ///
    /// Returns one `ObligationResult` per contract clause found in the graph.
    /// Nodes without `contract_clauses` produce no results.
    ///
    /// This method is preserved unchanged for backward compatibility.
    /// Use [`run_with_ledger`](Self::run_with_ledger) to get first-class
    /// ledger entries with identity and attempt tracking.
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

    /// Run the full pipeline and return first-class `ObligationLedgerEntry` items.
    ///
    /// Each entry carries identity (`id`), source stage, resolution attempts,
    /// and a degradation reason if the obligation was downgraded.  The `id`
    /// values are sequential strings (`"po_1"`, `"po_2"`, …) assigned after
    /// canonical ordering and deduplication, so equivalent graph content
    /// produces a stable ledger independent of graph insertion order.
    ///
    /// Use this method when you need to store the obligation ledger in a
    /// `VerificationReport` or explain obligation resolution to tooling.
    pub fn run_with_ledger(
        graph: &SemanticGraph,
        solver: &dyn Solver,
    ) -> Vec<ObligationLedgerEntry> {
        let obligations = Self::canonicalize_ledger_obligations(Self::generate_with_sources(graph));
        obligations
            .into_iter()
            .enumerate()
            .map(|(idx, ob)| {
                let id = format!("po_{}", idx + 1);
                Self::resolve_to_ledger(id, ob.obligation, ob.source_stage, graph, solver)
            })
            .collect()
    }

    // ── Stage 1: Generate ─────────────────────────────────────────────────

    fn generate(graph: &SemanticGraph) -> Vec<ProofObligation> {
        Self::generate_with_sources(graph)
            .into_iter()
            .filter(|generated| generated.source_stage == "contract")
            .map(|generated| generated.obligation)
            .collect()
    }

    fn generate_with_sources(graph: &SemanticGraph) -> Vec<GeneratedObligation> {
        let mut obligations = Vec::new();
        for node in &graph.nodes {
            if let Some(clauses) = &node.contract_clauses {
                let scope = node.name.clone();
                for predicate in &clauses.requires {
                    obligations.push(GeneratedObligation {
                        obligation: ProofObligation {
                            predicate: predicate.clone(),
                            role: ClauseRole::Requires,
                            scope: scope.clone(),
                        },
                        source_stage: "contract".into(),
                    });
                }
                for predicate in &clauses.ensures {
                    obligations.push(GeneratedObligation {
                        obligation: ProofObligation {
                            predicate: predicate.clone(),
                            role: ClauseRole::Ensures,
                            scope: scope.clone(),
                        },
                        source_stage: "contract".into(),
                    });
                }
            }
            if let Some(refinement) = &node.refinement_ref
                && !matches!(refinement.status, RefinementStatus::Proven)
            {
                obligations.push(GeneratedObligation {
                    obligation: ProofObligation {
                        predicate: refinement.predicate.clone(),
                        role: ClauseRole::Ensures,
                        scope: node.name.clone(),
                    },
                    source_stage: "refinement".into(),
                });
            }
            if node
                .trust_metadata
                .as_ref()
                .map(|trust| trust.level.as_str().starts_with("resource:"))
                .unwrap_or(false)
            {
                obligations.push(GeneratedObligation {
                    obligation: ProofObligation {
                        predicate: format!("resource_lifecycle({})", node.name),
                        role: ClauseRole::Ensures,
                        scope: node.name.clone(),
                    },
                    source_stage: "resource".into(),
                });
            }
            if node
                .trust_metadata
                .as_ref()
                .map(|trust| {
                    trust
                        .tags
                        .iter()
                        .any(|tag| tag == "concurrent" || tag == "shared")
                })
                .unwrap_or(false)
            {
                obligations.push(GeneratedObligation {
                    obligation: ProofObligation {
                        predicate: format!("concurrency_safe({})", node.name),
                        role: ClauseRole::Ensures,
                        scope: node.name.clone(),
                    },
                    source_stage: "concurrency".into(),
                });
            }
            if node.kind == NodeKind::Boundary {
                obligations.push(GeneratedObligation {
                    obligation: ProofObligation {
                        predicate: format!("boundary_trust({})", node.name),
                        role: ClauseRole::Requires,
                        scope: node.name.clone(),
                    },
                    source_stage: "boundary".into(),
                });
            }
        }
        obligations
    }

    fn canonicalize_ledger_obligations(
        mut obligations: Vec<GeneratedObligation>,
    ) -> Vec<GeneratedObligation> {
        obligations.sort_by(|a, b| {
            Self::source_stage_rank(&a.source_stage)
                .cmp(&Self::source_stage_rank(&b.source_stage))
                .then_with(|| a.source_stage.cmp(&b.source_stage))
                .then_with(|| a.obligation.scope.cmp(&b.obligation.scope))
                .then_with(|| a.obligation.role.cmp(&b.obligation.role))
                .then_with(|| a.obligation.predicate.cmp(&b.obligation.predicate))
        });
        obligations.dedup_by(|a, b| {
            a.source_stage == b.source_stage
                && a.obligation.scope == b.obligation.scope
                && a.obligation.role == b.obligation.role
                && a.obligation.predicate == b.obligation.predicate
        });
        obligations
    }

    fn source_stage_rank(stage: &str) -> u8 {
        match stage {
            "contract" => 0,
            "refinement" => 1,
            "resource" => 2,
            "concurrency" => 3,
            "boundary" => 4,
            _ => u8::MAX,
        }
    }

    // ── Stages 2–5 for one obligation (original path) ────────────────────

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

        // Stage 3: Solve — dispatch to solver with scope constraints.
        //
        // Extract requires clauses + body_expr from the scope node so that
        // SimpleSolver can apply arithmetic reasoning when SimpleSolver alone
        // cannot determine the outcome.
        //
        // IMPORTANT: requires clauses and body_expr are only valid constraints
        // for *Ensures* obligations (postconditions are proven under the
        // assumption that preconditions hold).  Using them for *Requires*
        // obligations would be circular — a precondition cannot prove itself.
        let scope_node = graph.nodes.iter().find(|n| n.name == obligation.scope);
        let constraint_strings: Vec<String> = if obligation.role == ClauseRole::Ensures {
            let requires = scope_node
                .and_then(|n| n.contract_clauses.as_ref())
                .map(|clauses| clauses.requires.clone())
                .unwrap_or_default();
            let mut v = requires;
            if let Some(body) = scope_node.and_then(|n| n.body_expr.as_ref()) {
                v.push(body.clone());
            }
            v
        } else {
            vec![]
        };
        let constraint_refs: Vec<&str> = constraint_strings.iter().map(String::as_str).collect();

        let outcome = solver.solve_with_constraints(&obligation, &constraint_refs);

        // Z3 fallback: when SimpleSolver returns Unsupported and the z3-solver
        // feature is enabled, try Z3 as an SMT tautology check.
        #[cfg(feature = "z3-solver")]
        let outcome = if outcome == crate::solver::SolverOutcome::Unsupported {
            crate::z3_solver::Z3Solver::new().solve(&obligation)
        } else {
            outcome
        };

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

    // ── Stages 2–5 for one obligation (ledger path) ───────────────────────

    fn resolve_to_ledger(
        id: String,
        obligation: ProofObligation,
        source_stage: String,
        graph: &SemanticGraph,
        solver: &dyn Solver,
    ) -> ObligationLedgerEntry {
        let predicate = obligation.predicate.trim();
        let mut attempts = Vec::new();

        // Extract scope constraints for context-aware solving (same as resolve).
        // Only Ensures obligations can use requires clauses as context — using
        // them for Requires obligations is circular reasoning.
        let scope_node = graph.nodes.iter().find(|n| n.name == obligation.scope);
        let constraint_strings: Vec<String> = if obligation.role == ClauseRole::Ensures {
            let requires = scope_node
                .and_then(|n| n.contract_clauses.as_ref())
                .map(|clauses| clauses.requires.clone())
                .unwrap_or_default();
            let mut v = requires;
            if let Some(body) = scope_node.and_then(|n| n.body_expr.as_ref()) {
                v.push(body.clone());
            }
            v
        } else {
            vec![]
        };
        let constraint_refs: Vec<&str> = constraint_strings.iter().map(String::as_str).collect();

        // Stage 2: Simplify
        if predicate == "true" {
            attempts.push(ObligationAttempt {
                stage: "simplify".into(),
                outcome: "proven".into(),
                evidence: None,
            });
            return ObligationLedgerEntry {
                id,
                obligation,
                state: ObligationState::Proven,
                source_stage,
                attempts,
                degradation_reason: None,
                repair_options: vec![],
            };
        }
        if predicate == "false" {
            attempts.push(ObligationAttempt {
                stage: "simplify".into(),
                outcome: "failed".into(),
                evidence: Some("literal false — obligation is trivially violated".into()),
            });
            return ObligationLedgerEntry {
                id,
                obligation,
                state: ObligationState::Failed,
                source_stage,
                attempts,
                degradation_reason: Some("literal false clause".into()),
                repair_options: vec![
                    "remove or correct the false precondition".into(),
                    "add a guard that makes the clause reachable only when satisfiable".into(),
                ],
            };
        }

        // Stage 3: Solve — with context constraints, then optional Z3 fallback.
        let raw_outcome = solver.solve_with_constraints(&obligation, &constraint_refs);

        #[cfg(feature = "z3-solver")]
        let raw_outcome = if raw_outcome == SolverOutcome::Unsupported {
            crate::z3_solver::Z3Solver::new().solve(&obligation)
        } else {
            raw_outcome
        };

        let outcome = raw_outcome;

        match outcome {
            SolverOutcome::Proven => {
                attempts.push(ObligationAttempt {
                    stage: "solver".into(),
                    outcome: "proven".into(),
                    evidence: None,
                });
                ObligationLedgerEntry {
                    id,
                    obligation,
                    state: ObligationState::Proven,
                    source_stage,
                    attempts,
                    degradation_reason: None,
                    repair_options: vec![],
                }
            }
            SolverOutcome::Assumed(reason) => {
                attempts.push(ObligationAttempt {
                    stage: "solver".into(),
                    outcome: "assumed".into(),
                    evidence: Some(reason.clone()),
                });

                // Stage 4: Compose
                if Self::compose_check(predicate, graph) {
                    attempts.push(ObligationAttempt {
                        stage: "compose".into(),
                        outcome: "composed".into(),
                        evidence: Some("peer node ensures clause covers this predicate".into()),
                    });
                    ObligationLedgerEntry {
                        id,
                        obligation,
                        state: ObligationState::RuntimeChecked,
                        source_stage,
                        attempts,
                        degradation_reason: None,
                        repair_options: vec![],
                    }
                } else {
                    // Stage 5: Degrade
                    attempts.push(ObligationAttempt {
                        stage: "degrade".into(),
                        outcome: "degraded".into(),
                        evidence: Some(reason.clone()),
                    });
                    ObligationLedgerEntry {
                        id,
                        obligation,
                        state: ObligationState::Assumed(reason.clone()),
                        source_stage,
                        attempts,
                        degradation_reason: Some(reason),
                        repair_options: vec![
                            "add a requires guard proven by the caller".into(),
                            "add a runtime check at the call site".into(),
                        ],
                    }
                }
            }
            SolverOutcome::Unsupported => {
                attempts.push(ObligationAttempt {
                    stage: "solver".into(),
                    outcome: "unsupported".into(),
                    evidence: Some("predicate not supported by SimpleSolver".into()),
                });

                // Stage 4: Compose
                if Self::compose_check(predicate, graph) {
                    attempts.push(ObligationAttempt {
                        stage: "compose".into(),
                        outcome: "composed".into(),
                        evidence: Some("peer node ensures clause covers this predicate".into()),
                    });
                    ObligationLedgerEntry {
                        id,
                        obligation,
                        state: ObligationState::RuntimeChecked,
                        source_stage,
                        attempts,
                        degradation_reason: None,
                        repair_options: vec![],
                    }
                } else {
                    // Stage 5: Degrade — unsupported → Assumed
                    let reason = "solver cannot evaluate predicate; accepted by policy".to_string();
                    attempts.push(ObligationAttempt {
                        stage: "degrade".into(),
                        outcome: "degraded".into(),
                        evidence: Some(reason.clone()),
                    });
                    ObligationLedgerEntry {
                        id,
                        obligation,
                        state: ObligationState::Assumed(reason.clone()),
                        source_stage,
                        attempts,
                        degradation_reason: Some(reason),
                        repair_options: vec![
                            "add a requires guard proven by the caller".into(),
                            "add a runtime check at the call site".into(),
                        ],
                    }
                }
            }
        }
    }

    // ── Stage 4: Compose ──────────────────────────────────────────────────

    /// Return `true` if any node in `graph` has an `ensures` clause whose
    /// predicate text exactly matches `predicate` OR semantically implies it.
    ///
    /// Contract composition: if a called function *ensures* the same predicate
    /// (or a stronger one) the current obligation requires, the obligation is
    /// covered without an independent proof.
    fn compose_check(predicate: &str, graph: &SemanticGraph) -> bool {
        for node in &graph.nodes {
            if let Some(clauses) = &node.contract_clauses {
                for ensures in &clauses.ensures {
                    let candidate = ensures.trim();
                    if candidate == predicate || semantic_implies(predicate, candidate) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// ── Semantic implication for simple comparison predicates ─────────────────

/// Parse a simple `<ident> <op> <int>` predicate into its components.
///
/// Returns `(ident, op, value)` if the pattern matches, otherwise `None`.
/// Supported operators: `>=`, `<=`, `>`, `<`.
fn parse_simple_cmp(s: &str) -> Option<(String, &'static str, i64)> {
    let s = s.trim();
    // Check longer operators first to avoid ambiguity (>= before >)
    for op in &[">=", "<=", ">", "<"] {
        if let Some(idx) = s.find(op) {
            let ident = s[..idx].trim();
            let val_str = s[idx + op.len()..].trim();
            if ident.is_empty() {
                continue;
            }
            if !ident
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                continue;
            }
            if let Ok(val) = val_str.parse::<i64>() {
                return Some((ident.to_string(), op, val));
            }
        }
    }
    None
}

/// Convert a comparison operator + value to an inclusive lower bound.
///
/// `x > N` → lower bound is `N + 1` (integer semantics).
/// `x >= N` → lower bound is `N`.
/// Upper bound operators return `None`.
fn to_lower_bound(op: &str, val: i64) -> Option<i64> {
    match op {
        ">" => Some(val.saturating_add(1)),
        ">=" => Some(val),
        _ => None,
    }
}

/// Return `true` if `candidate` semantically implies `required` for simple
/// integer comparison predicates of the form `<ident> <op> <int>`.
///
/// Semantic implication: candidate's lower bound ≥ required's lower bound.
///
/// # Examples
///
/// ```text
/// semantic_implies("x > 0", "x >= 1")  // true:  x>=1 → x>0 (lb 1 ≥ lb 1)
/// semantic_implies("x >= 0", "x > 0")  // true:  x>0  → x≥0 (lb 1 ≥ lb 0)
/// semantic_implies("x > 5", "x > 3")   // false: x>3 does not imply x>5
/// ```
pub fn semantic_implies(required: &str, candidate: &str) -> bool {
    if required.trim() == candidate.trim() {
        return true;
    }
    let Some((r_ident, r_op, r_val)) = parse_simple_cmp(required) else {
        return false;
    };
    let Some((c_ident, c_op, c_val)) = parse_simple_cmp(candidate) else {
        return false;
    };
    if r_ident != c_ident {
        return false;
    }
    let Some(r_lb) = to_lower_bound(r_op, r_val) else {
        return false;
    };
    let Some(c_lb) = to_lower_bound(c_op, c_val) else {
        return false;
    };
    c_lb >= r_lb
}
