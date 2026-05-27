use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;

use super::is_pure;

/// Remove pure `AnfExpr` sub-expressions whose results are never referenced.
///
/// Specifically: within `AnfExpr::Seq`, any non-final element that `is_pure`
/// can be dropped — its result is discarded and it has no observable effects.
/// The pass recurses into all composite expression variants.
pub fn eliminate_dead_pure(bindings: Vec<AnfBinding>) -> Vec<AnfBinding> {
    bindings
        .into_iter()
        .map(|b| AnfBinding {
            expr: elim_dead_expr(b.expr),
            ..b
        })
        .collect()
}

fn elim_dead_expr(expr: AnfExpr) -> AnfExpr {
    match expr {
        AnfExpr::Seq(exprs) => {
            let n = exprs.len();
            let mut filtered: Vec<AnfExpr> = exprs
                .into_iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    let e = elim_dead_expr(e);
                    // Non-final pure elements have no observable effect — drop.
                    if i < n.saturating_sub(1) && is_pure(&e) {
                        None
                    } else {
                        Some(e)
                    }
                })
                .collect();
            match filtered.len() {
                0 => AnfExpr::Literal(LiteralValue::Unit),
                1 => filtered.remove(0),
                _ => AnfExpr::Seq(filtered),
            }
        }
        AnfExpr::Let { name, value, body } => AnfExpr::Let {
            name,
            value: Box::new(elim_dead_expr(*value)),
            body: Box::new(elim_dead_expr(*body)),
        },
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => AnfExpr::If {
            cond,
            then_branch: Box::new(elim_dead_expr(*then_branch)),
            else_branch: Box::new(elim_dead_expr(*else_branch)),
        },
        AnfExpr::Return(inner) => AnfExpr::Return(Box::new(elim_dead_expr(*inner))),
        AnfExpr::Loop { body } => AnfExpr::Loop {
            body: Box::new(elim_dead_expr(*body)),
        },
        AnfExpr::Break { value } => AnfExpr::Break {
            value: Box::new(elim_dead_expr(*value)),
        },
        AnfExpr::Lambda {
            params,
            captures,
            body,
        } => AnfExpr::Lambda {
            params,
            captures,
            body: Box::new(elim_dead_expr(*body)),
        },
        AnfExpr::TaskGroup { body } => AnfExpr::TaskGroup {
            body: Box::new(elim_dead_expr(*body)),
        },
        AnfExpr::Timeout { duration, body } => AnfExpr::Timeout {
            duration,
            body: Box::new(elim_dead_expr(*body)),
        },
        AnfExpr::Match { scrutinee, arms } => AnfExpr::Match {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern,
                    body: elim_dead_expr(arm.body),
                })
                .collect(),
        },
        AnfExpr::RecordNew { fields } => AnfExpr::RecordNew {
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, elim_dead_expr(e)))
                .collect(),
        },
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => AnfExpr::FieldUpdate {
            record,
            field,
            value: Box::new(elim_dead_expr(*value)),
        },
        AnfExpr::TupleNew(elems) => {
            AnfExpr::TupleNew(elems.into_iter().map(elim_dead_expr).collect())
        }
        AnfExpr::VariantNew { tag, payload } => AnfExpr::VariantNew {
            tag,
            payload: payload.map(|p| Box::new(elim_dead_expr(*p))),
        },
        AnfExpr::ListNew(elems) => {
            AnfExpr::ListNew(elems.into_iter().map(elim_dead_expr).collect())
        }
        other => other,
    }
}
