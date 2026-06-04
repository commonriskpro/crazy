use super::*;

// ── Solver selection ──────────────────────────────────────────────────────

/// Concrete solver backend selected at CLI dispatch time.
///
/// `Simple` is always available; `Z3` is only present when the `z3-solver`
/// cargo feature is compiled in.  The enum implements `Solver` by delegating
/// to the inner type, so it coerces directly to `&dyn Solver`.
pub(super) enum AnySolver {
    Simple(SimpleSolver),
    #[cfg(feature = "z3-solver")]
    Z3(ail_verify::z3_solver::Z3Solver),
}

impl std::fmt::Debug for AnySolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnySolver::Simple(_) => write!(f, "AnySolver::Simple"),
            #[cfg(feature = "z3-solver")]
            AnySolver::Z3(_) => write!(f, "AnySolver::Z3"),
        }
    }
}

impl Solver for AnySolver {
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome {
        match self {
            AnySolver::Simple(s) => s.solve(obligation),
            #[cfg(feature = "z3-solver")]
            AnySolver::Z3(s) => s.solve(obligation),
        }
    }

    fn solve_with_constraints(
        &self,
        obligation: &ProofObligation,
        constraints: &[&str],
    ) -> SolverOutcome {
        match self {
            AnySolver::Simple(s) => s.solve_with_constraints(obligation, constraints),
            #[cfg(feature = "z3-solver")]
            AnySolver::Z3(s) => s.solve_with_constraints(obligation, constraints),
        }
    }
}

/// Build the solver requested by `name`.
///
/// - `"simple"` or `""` → `SimpleSolver` (always available).
/// - `"z3"` → `Z3Solver` when `z3-solver` feature is compiled in; otherwise
///   returns a deterministic `CliError::Domain` explaining how to recompile.
/// - Any other name → `CliError::Domain` listing the valid options.
pub(super) fn build_solver(name: &str) -> Result<AnySolver, CliError> {
    match name {
        "simple" | "" => Ok(AnySolver::Simple(SimpleSolver)),
        "z3" => {
            #[cfg(feature = "z3-solver")]
            return Ok(AnySolver::Z3(ail_verify::z3_solver::Z3Solver::new()));
            #[cfg(not(feature = "z3-solver"))]
            Err(CliError::Domain(
                "solver 'z3' requires the z3-solver cargo feature; \
                 recompile ail-cli with --features z3-solver"
                    .to_string(),
            ))
        }
        other => Err(CliError::Domain(format!(
            "unknown solver '{other}'; supported values: simple, z3"
        ))),
    }
}
