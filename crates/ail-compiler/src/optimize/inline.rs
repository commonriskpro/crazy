use std::collections::BTreeMap;

use crate::anf::{AnfBinding, AnfExpr};

use super::{anf_node_count, is_pure};

/// Inline calls to small pure functions (Lambda bindings with ≤3 ANF nodes
/// in the body and no effects).
///
/// A binding qualifies for inlining when its `expr` is an `AnfExpr::Lambda`
/// whose body is `is_pure` and `anf_node_count(body) <= 3`.  All `Call`
/// expressions in other bindings whose `func` matches a qualifying lambda
/// name are replaced with the beta-reduced body.
pub fn inline_small_pure(bindings: Vec<AnfBinding>) -> Vec<AnfBinding> {
    // Collect small pure lambdas: name → (params, body).
    let small_fns: BTreeMap<String, (Vec<String>, AnfExpr)> = bindings
        .iter()
        .filter_map(|b| {
            if let AnfExpr::Lambda { params, body, .. } = &b.expr
                && is_pure(body)
                && anf_node_count(body) <= 3
            {
                return Some((b.name.clone(), (params.clone(), *body.clone())));
            }
            None
        })
        .collect();

    if small_fns.is_empty() {
        return bindings;
    }

    bindings
        .into_iter()
        .map(|b| AnfBinding {
            expr: inline_calls_in_expr(b.expr, &small_fns),
            ..b
        })
        .collect()
}

fn inline_calls_in_expr(
    expr: AnfExpr,
    small_fns: &BTreeMap<String, (Vec<String>, AnfExpr)>,
) -> AnfExpr {
    match expr {
        AnfExpr::Call { ref func, ref args } => {
            if let Some((params, body)) = small_fns.get(func)
                && params.len() == args.len()
            {
                let subst: BTreeMap<String, String> = params
                    .iter()
                    .zip(args.iter())
                    .map(|(p, a)| (p.clone(), a.clone()))
                    .collect();
                return substitute_vars(body.clone(), &subst);
            }
            expr
        }
        AnfExpr::Let { name, value, body } => AnfExpr::Let {
            name,
            value: Box::new(inline_calls_in_expr(*value, small_fns)),
            body: Box::new(inline_calls_in_expr(*body, small_fns)),
        },
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => AnfExpr::If {
            cond,
            then_branch: Box::new(inline_calls_in_expr(*then_branch, small_fns)),
            else_branch: Box::new(inline_calls_in_expr(*else_branch, small_fns)),
        },
        AnfExpr::Return(inner) => {
            AnfExpr::Return(Box::new(inline_calls_in_expr(*inner, small_fns)))
        }
        AnfExpr::Seq(exprs) => AnfExpr::Seq(
            exprs
                .into_iter()
                .map(|e| inline_calls_in_expr(e, small_fns))
                .collect(),
        ),
        AnfExpr::Match { scrutinee, arms } => AnfExpr::Match {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern,
                    body: inline_calls_in_expr(arm.body, small_fns),
                })
                .collect(),
        },
        AnfExpr::Lambda {
            params,
            captures: _,
            body,
        } => {
            let inlined_body = inline_calls_in_expr(*body, small_fns);
            // Recompute captures: inlining a call inside the body may have
            // replaced a captured-var argument, leaving the old list stale.
            let mut bound: Vec<&str> = params.iter().map(String::as_str).collect();
            let mut free_in_body: Vec<&str> = Vec::new();
            crate::wasm_abi::collect_free_vars(&inlined_body, &mut bound, &mut free_in_body);
            let captures = free_in_body.into_iter().map(str::to_string).collect();
            AnfExpr::Lambda {
                params,
                captures,
                body: Box::new(inlined_body),
            }
        }
        AnfExpr::RecordNew { fields } => AnfExpr::RecordNew {
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, inline_calls_in_expr(e, small_fns)))
                .collect(),
        },
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => AnfExpr::FieldUpdate {
            record,
            field,
            value: Box::new(inline_calls_in_expr(*value, small_fns)),
        },
        AnfExpr::TupleNew(elems) => AnfExpr::TupleNew(
            elems
                .into_iter()
                .map(|e| inline_calls_in_expr(e, small_fns))
                .collect(),
        ),
        AnfExpr::VariantNew { tag, payload } => AnfExpr::VariantNew {
            tag,
            payload: payload.map(|p| Box::new(inline_calls_in_expr(*p, small_fns))),
        },
        AnfExpr::ListNew(elems) => AnfExpr::ListNew(
            elems
                .into_iter()
                .map(|e| inline_calls_in_expr(e, small_fns))
                .collect(),
        ),
        AnfExpr::Loop { body } => AnfExpr::Loop {
            body: Box::new(inline_calls_in_expr(*body, small_fns)),
        },
        AnfExpr::Break { value } => AnfExpr::Break {
            value: Box::new(inline_calls_in_expr(*value, small_fns)),
        },
        AnfExpr::TaskGroup { body } => AnfExpr::TaskGroup {
            body: Box::new(inline_calls_in_expr(*body, small_fns)),
        },
        AnfExpr::Timeout { duration, body } => AnfExpr::Timeout {
            duration,
            body: Box::new(inline_calls_in_expr(*body, small_fns)),
        },
        other => other,
    }
}

/// Substitute variable names in `expr`: every `Var(k)` where `k` is a key in
/// `subst` is replaced by `Var(subst[k])`.  Atomic string fields in ANF
/// expressions (call args, record names, scrutinees, etc.) are also
/// substituted.  `Call.func` is NOT substituted — it is a function name, not
/// a variable reference.  Shadowing by inner `Let`/`Lambda` params is
/// respected.
fn substitute_vars(expr: AnfExpr, subst: &BTreeMap<String, String>) -> AnfExpr {
    if subst.is_empty() {
        return expr;
    }
    let sub = |s: String| subst.get(&s).cloned().unwrap_or(s);
    match expr {
        AnfExpr::Var(name) => AnfExpr::Var(sub(name)),
        AnfExpr::Call { func, args } => AnfExpr::Call {
            func,
            args: args.into_iter().map(sub).collect(),
        },
        AnfExpr::FieldGet { record, field } => AnfExpr::FieldGet {
            record: sub(record),
            field,
        },
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => AnfExpr::If {
            cond: sub(cond),
            then_branch: Box::new(substitute_vars(*then_branch, subst)),
            else_branch: Box::new(substitute_vars(*else_branch, subst)),
        },
        AnfExpr::Let { name, value, body } => {
            let value = substitute_vars(*value, subst);
            // Inner binding shadows the substitution.
            let mut inner = subst.clone();
            inner.remove(&name);
            AnfExpr::Let {
                name,
                value: Box::new(value),
                body: Box::new(substitute_vars(*body, &inner)),
            }
        }
        AnfExpr::Return(inner) => AnfExpr::Return(Box::new(substitute_vars(*inner, subst))),
        AnfExpr::Seq(exprs) => AnfExpr::Seq(
            exprs
                .into_iter()
                .map(|e| substitute_vars(e, subst))
                .collect(),
        ),
        AnfExpr::RecordNew { fields } => AnfExpr::RecordNew {
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, substitute_vars(e, subst)))
                .collect(),
        },
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => AnfExpr::FieldUpdate {
            record: sub(record),
            field,
            value: Box::new(substitute_vars(*value, subst)),
        },
        AnfExpr::TupleNew(elems) => AnfExpr::TupleNew(
            elems
                .into_iter()
                .map(|e| substitute_vars(e, subst))
                .collect(),
        ),
        AnfExpr::VariantNew { tag, payload } => AnfExpr::VariantNew {
            tag,
            payload: payload.map(|p| Box::new(substitute_vars(*p, subst))),
        },
        AnfExpr::ListNew(elems) => AnfExpr::ListNew(
            elems
                .into_iter()
                .map(|e| substitute_vars(e, subst))
                .collect(),
        ),
        AnfExpr::Lambda {
            params,
            captures,
            body,
        } => {
            let mut inner = subst.clone();
            for p in &params {
                inner.remove(p);
            }
            // Apply substitution to captures — they reference outer-scope names.
            let new_captures = captures
                .into_iter()
                .map(|c| inner.get(&c).cloned().unwrap_or(c))
                .collect();
            AnfExpr::Lambda {
                params,
                captures: new_captures,
                body: Box::new(substitute_vars(*body, &inner)),
            }
        }
        AnfExpr::Match { scrutinee, arms } => AnfExpr::Match {
            scrutinee: sub(scrutinee),
            arms: arms
                .into_iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern,
                    body: substitute_vars(arm.body, subst),
                })
                .collect(),
        },
        AnfExpr::Loop { body } => AnfExpr::Loop {
            body: Box::new(substitute_vars(*body, subst)),
        },
        AnfExpr::Break { value } => AnfExpr::Break {
            value: Box::new(substitute_vars(*value, subst)),
        },
        AnfExpr::TaskGroup { body } => AnfExpr::TaskGroup {
            body: Box::new(substitute_vars(*body, subst)),
        },
        AnfExpr::Timeout { duration, body } => AnfExpr::Timeout {
            duration: sub(duration),
            body: Box::new(substitute_vars(*body, subst)),
        },
        other => other,
    }
}
