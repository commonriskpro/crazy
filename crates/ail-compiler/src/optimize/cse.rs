use crate::anf::{AnfBinding, AnfExpr};

use super::is_pure;

/// Common Subexpression Elimination.
///
/// Within each binding's let-chain, find pure sub-expressions that appear
/// more than once (structurally identical via `PartialEq`) and replace
/// subsequent occurrences with a `Var` reference to the first binding.
///
/// The resulting redundant alias lets (e.g. `let b = a in …`) are collapsed
/// by the subsequent `optimize_bindings` dead-let pass.
pub fn cse_bindings(bindings: Vec<AnfBinding>) -> Vec<AnfBinding> {
    bindings
        .into_iter()
        .map(|b| {
            let mut seen: Vec<(AnfExpr, String)> = Vec::new();
            AnfBinding {
                expr: cse_expr(b.expr, &mut seen),
                ..b
            }
        })
        .collect()
}

/// Recursively apply CSE within `expr`, threading the `seen` table.
///
/// `seen` maps a pure `AnfExpr` value to the name of the `Let` binding that
/// first computed it.  When the same pure value appears as the RHS of a later
/// `Let`, it is replaced with `Var(first_binding_name)`.
///
/// `If` branches clone the current `seen` table so that CSE hits inside one
/// branch are not visible to the sibling branch (distinct control-flow paths).
fn cse_expr(expr: AnfExpr, seen: &mut Vec<(AnfExpr, String)>) -> AnfExpr {
    match expr {
        AnfExpr::Let { name, value, body } => {
            let value = cse_expr(*value, seen);
            let new_value = if is_pure(&value) {
                // Check whether this pure expression was already computed.
                let existing = seen
                    .iter()
                    .find(|(e, _)| e == &value)
                    .map(|(_, n)| n.clone());
                if let Some(existing_name) = existing {
                    // CSE hit: alias to the first computation.
                    AnfExpr::Var(existing_name)
                } else {
                    // First occurrence: record it.
                    seen.push((value.clone(), name.clone()));
                    value
                }
            } else {
                value
            };
            let body = cse_expr(*body, seen);
            AnfExpr::Let {
                name,
                value: Box::new(new_value),
                body: Box::new(body),
            }
        }
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            // Clone `seen` for each branch: CSE across branch boundaries is
            // unsound (only one branch executes).
            let then_branch = cse_expr(*then_branch, &mut seen.clone());
            let else_branch = cse_expr(*else_branch, &mut seen.clone());
            AnfExpr::If {
                cond,
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            }
        }
        // For all other variants, return as-is (CSE focuses on Let-chains).
        other => other,
    }
}
