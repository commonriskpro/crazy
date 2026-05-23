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

use std::collections::BTreeMap;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;

pub fn optimize_bindings(bindings: Vec<AnfBinding>) -> Vec<AnfBinding> {
    bindings
        .into_iter()
        .map(|binding| AnfBinding {
            expr: optimize_expr(binding.expr, &mut BTreeMap::new()),
            ..binding
        })
        .collect()
}

fn optimize_expr(expr: AnfExpr, env: &mut BTreeMap<String, LiteralValue>) -> AnfExpr {
    match expr {
        AnfExpr::Let { name, value, body } => {
            let value = optimize_expr(*value, env);
            let previous = match &value {
                AnfExpr::Literal(lit) => env.insert(name.clone(), lit.clone()),
                _ => env.remove(&name),
            };
            let body = optimize_expr(*body, env);
            restore_env(env, &name, previous);
            if is_pure(&value) && !uses_var(&body, &name) {
                body
            } else {
                AnfExpr::Let {
                    name,
                    value: Box::new(value),
                    body: Box::new(body),
                }
            }
        }
        AnfExpr::Call { func, args } => fold_call(&func, &args, env)
            .map(AnfExpr::Literal)
            .unwrap_or(AnfExpr::Call { func, args }),
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => match env.get(&cond) {
            Some(LiteralValue::Bool(true)) | Some(LiteralValue::Int(1)) => {
                optimize_expr(*then_branch, env)
            }
            Some(LiteralValue::Bool(false)) | Some(LiteralValue::Int(0)) => {
                optimize_expr(*else_branch, env)
            }
            _ => AnfExpr::If {
                cond,
                then_branch: Box::new(optimize_expr(*then_branch, env)),
                else_branch: Box::new(optimize_expr(*else_branch, env)),
            },
        },
        AnfExpr::Return(inner) => AnfExpr::Return(Box::new(optimize_expr(*inner, env))),
        AnfExpr::Seq(exprs) => AnfExpr::Seq(
            exprs
                .into_iter()
                .map(|expr| optimize_expr(expr, env))
                .collect(),
        ),
        AnfExpr::RecordNew { fields } => AnfExpr::RecordNew {
            fields: fields
                .into_iter()
                .map(|(field, expr)| (field, optimize_expr(expr, env)))
                .collect(),
        },
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => AnfExpr::FieldUpdate {
            record,
            field,
            value: Box::new(optimize_expr(*value, env)),
        },
        AnfExpr::TupleNew(elems) => AnfExpr::TupleNew(
            elems
                .into_iter()
                .map(|expr| optimize_expr(expr, env))
                .collect(),
        ),
        AnfExpr::VariantNew { tag, payload } => AnfExpr::VariantNew {
            tag,
            payload: payload.map(|expr| Box::new(optimize_expr(*expr, env))),
        },
        AnfExpr::ListNew(elems) => AnfExpr::ListNew(
            elems
                .into_iter()
                .map(|expr| optimize_expr(expr, env))
                .collect(),
        ),
        AnfExpr::Loop { body } => AnfExpr::Loop {
            body: Box::new(optimize_expr(*body, env)),
        },
        AnfExpr::Break { value } => AnfExpr::Break {
            value: Box::new(optimize_expr(*value, env)),
        },
        AnfExpr::ShortCircuitAnd { left, right } => AnfExpr::ShortCircuitAnd {
            left,
            right: Box::new(optimize_expr(*right, env)),
        },
        AnfExpr::ShortCircuitOr { left, right } => AnfExpr::ShortCircuitOr {
            left,
            right: Box::new(optimize_expr(*right, env)),
        },
        AnfExpr::Match { scrutinee, arms } => AnfExpr::Match {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern,
                    body: optimize_expr(arm.body, env),
                })
                .collect(),
        },
        AnfExpr::Lambda { params, body } => {
            let mut nested_env = env.clone();
            for param in &params {
                nested_env.remove(param);
            }
            AnfExpr::Lambda {
                params,
                body: Box::new(optimize_expr(*body, &mut nested_env)),
            }
        }
        AnfExpr::TaskGroup { body } => AnfExpr::TaskGroup {
            body: Box::new(optimize_expr(*body, env)),
        },
        AnfExpr::Timeout { duration, body } => AnfExpr::Timeout {
            duration,
            body: Box::new(optimize_expr(*body, env)),
        },
        other => other,
    }
}

fn restore_env(
    env: &mut BTreeMap<String, LiteralValue>,
    name: &str,
    previous: Option<LiteralValue>,
) {
    if let Some(previous) = previous {
        env.insert(name.to_string(), previous);
    } else {
        env.remove(name);
    }
}

fn fold_call(
    func: &str,
    args: &[String],
    env: &BTreeMap<String, LiteralValue>,
) -> Option<LiteralValue> {
    let ints = args
        .iter()
        .map(|arg| match env.get(arg) {
            Some(LiteralValue::Int(value)) => Some(*value),
            Some(LiteralValue::Bool(value)) => Some(i64::from(*value)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    match (func, ints.as_slice()) {
        ("i64.add" | "+" | "add", [a, b]) => Some(LiteralValue::Int(a + b)),
        ("i64.sub" | "-" | "sub", [a, b]) => Some(LiteralValue::Int(a - b)),
        ("i64.mul" | "*" | "mul", [a, b]) => Some(LiteralValue::Int(a * b)),
        ("i64.div_s" | "/" | "div", [_, 0]) => None,
        ("i64.div_s" | "/" | "div", [a, b]) => Some(LiteralValue::Int(a / b)),
        ("i64.rem_s" | "%" | "mod", [_, 0]) => None,
        ("i64.rem_s" | "%" | "mod", [a, b]) => Some(LiteralValue::Int(a % b)),
        ("i64.eq" | "==" | "eq", [a, b]) => Some(LiteralValue::Bool(a == b)),
        ("i64.ne" | "!=" | "ne", [a, b]) => Some(LiteralValue::Bool(a != b)),
        ("i64.lt_s" | "<" | "lt", [a, b]) => Some(LiteralValue::Bool(a < b)),
        ("i64.le_s" | "<=" | "le", [a, b]) => Some(LiteralValue::Bool(a <= b)),
        ("i64.gt_s" | ">" | "gt", [a, b]) => Some(LiteralValue::Bool(a > b)),
        ("i64.ge_s" | ">=" | "ge", [a, b]) => Some(LiteralValue::Bool(a >= b)),
        _ => None,
    }
}

fn is_pure(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::Call { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::RecordNew { .. }
        | AnfExpr::TupleNew(_)
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::Lambda { .. } => true,
        AnfExpr::Let { value, body, .. } => is_pure(value) && is_pure(body),
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => is_pure(then_branch) && is_pure(else_branch),
        AnfExpr::Seq(exprs) => exprs.iter().all(is_pure),
        AnfExpr::Match { arms, .. } => arms.iter().all(|arm| is_pure(&arm.body)),
        AnfExpr::Return(_)
        | AnfExpr::FieldUpdate { .. }
        | AnfExpr::Loop { .. }
        | AnfExpr::Break { .. }
        | AnfExpr::Continue
        | AnfExpr::WhileLoop { .. }
        | AnfExpr::ShortCircuitAnd { .. }
        | AnfExpr::ShortCircuitOr { .. }
        | AnfExpr::EffectCall { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::Select { .. }
        | AnfExpr::Timeout { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        // ola5 Gap 2 — new primitives
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::ForEach { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Placeholder => false,
    }
}

// ── anf_node_count ────────────────────────────────────────────────────────

/// Count the total number of `AnfExpr` nodes in `expr` (recursive).
///
/// Atomic leaf nodes (`Literal`, `Var`, `Placeholder`, `Continue`, `Call`,
/// `FieldGet`, and other flat impure primitives) each count as 1.  Composite
/// nodes count as 1 plus the sum of their sub-expressions.
fn anf_node_count(expr: &AnfExpr) -> usize {
    match expr {
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::Placeholder
        | AnfExpr::Continue
        | AnfExpr::Call { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::EffectCall { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::WhileLoop { .. }
        | AnfExpr::ShortCircuitAnd { .. }
        | AnfExpr::ShortCircuitOr { .. }
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::ForEach { .. }
        | AnfExpr::Fold { .. } => 1,
        AnfExpr::Let { value, body, .. } => {
            1 + anf_node_count(value) + anf_node_count(body)
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => 1 + anf_node_count(then_branch) + anf_node_count(else_branch),
        AnfExpr::Return(inner)
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::TaskGroup { body: inner }
        | AnfExpr::Timeout { body: inner, .. }
        | AnfExpr::Lambda { body: inner, .. } => 1 + anf_node_count(inner),
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            1 + exprs.iter().map(anf_node_count).sum::<usize>()
        }
        AnfExpr::RecordNew { fields } => {
            1 + fields.iter().map(|(_, e)| anf_node_count(e)).sum::<usize>()
        }
        AnfExpr::FieldUpdate { value, .. } => 1 + anf_node_count(value),
        AnfExpr::VariantNew { payload, .. } => {
            1 + payload.as_ref().map_or(0, |p| anf_node_count(p))
        }
        AnfExpr::Match { arms, .. } => {
            1 + arms.iter().map(|arm| anf_node_count(&arm.body)).sum::<usize>()
        }
        AnfExpr::Select { branches } => {
            1 + branches.iter().map(|b| anf_node_count(&b.body)).sum::<usize>()
        }
    }
}

// ── eliminate_dead_pure ───────────────────────────────────────────────────

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
        AnfExpr::Lambda { params, body } => AnfExpr::Lambda {
            params,
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
        AnfExpr::FieldUpdate { record, field, value } => AnfExpr::FieldUpdate {
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

// ── inline_small_pure ─────────────────────────────────────────────────────

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
            if let AnfExpr::Lambda { params, body } = &b.expr {
                if is_pure(body) && anf_node_count(body) <= 3 {
                    return Some((b.name.clone(), (params.clone(), *body.clone())));
                }
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
            if let Some((params, body)) = small_fns.get(func) {
                if params.len() == args.len() {
                    let subst: BTreeMap<String, String> = params
                        .iter()
                        .zip(args.iter())
                        .map(|(p, a)| (p.clone(), a.clone()))
                        .collect();
                    return substitute_vars(body.clone(), &subst);
                }
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
        AnfExpr::Lambda { params, body } => AnfExpr::Lambda {
            params,
            body: Box::new(inline_calls_in_expr(*body, small_fns)),
        },
        AnfExpr::RecordNew { fields } => AnfExpr::RecordNew {
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, inline_calls_in_expr(e, small_fns)))
                .collect(),
        },
        AnfExpr::FieldUpdate { record, field, value } => AnfExpr::FieldUpdate {
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
        AnfExpr::Seq(exprs) => {
            AnfExpr::Seq(exprs.into_iter().map(|e| substitute_vars(e, subst)).collect())
        }
        AnfExpr::RecordNew { fields } => AnfExpr::RecordNew {
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, substitute_vars(e, subst)))
                .collect(),
        },
        AnfExpr::FieldUpdate { record, field, value } => AnfExpr::FieldUpdate {
            record: sub(record),
            field,
            value: Box::new(substitute_vars(*value, subst)),
        },
        AnfExpr::TupleNew(elems) => {
            AnfExpr::TupleNew(elems.into_iter().map(|e| substitute_vars(e, subst)).collect())
        }
        AnfExpr::VariantNew { tag, payload } => AnfExpr::VariantNew {
            tag,
            payload: payload.map(|p| Box::new(substitute_vars(*p, subst))),
        },
        AnfExpr::ListNew(elems) => {
            AnfExpr::ListNew(elems.into_iter().map(|e| substitute_vars(e, subst)).collect())
        }
        AnfExpr::Lambda { params, body } => {
            let mut inner = subst.clone();
            for p in &params {
                inner.remove(p);
            }
            AnfExpr::Lambda {
                params,
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

// ── cse_bindings ──────────────────────────────────────────────────────────

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

fn uses_var(expr: &AnfExpr, name: &str) -> bool {
    match expr {
        AnfExpr::Var(var) => var == name,
        AnfExpr::Let {
            name: binding,
            value,
            body,
        } => uses_var(value, name) || (binding != name && uses_var(body, name)),
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => cond == name || uses_var(then_branch, name) || uses_var(else_branch, name),
        AnfExpr::Call { args, .. } => args.iter().any(|arg| arg == name),
        AnfExpr::FieldGet { record, .. } => record == name,
        AnfExpr::Return(inner)
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::ShortCircuitAnd { right: inner, .. }
        | AnfExpr::ShortCircuitOr { right: inner, .. }
        | AnfExpr::TaskGroup { body: inner }
        | AnfExpr::Timeout { body: inner, .. } => uses_var(inner, name),
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            exprs.iter().any(|expr| uses_var(expr, name))
        }
        AnfExpr::Match { scrutinee, arms } => {
            scrutinee == name || arms.iter().any(|arm| uses_var(&arm.body, name))
        }
        AnfExpr::Lambda { params, body } => {
            !params.iter().any(|param| param == name) && uses_var(body, name)
        }
        AnfExpr::RecordNew { fields } => fields.iter().any(|(_, expr)| uses_var(expr, name)),
        AnfExpr::FieldUpdate { record, value, .. } => record == name || uses_var(value, name),
        AnfExpr::VariantNew { payload, .. } => payload
            .as_ref()
            .is_some_and(|payload| uses_var(payload, name)),
        AnfExpr::WhileLoop { cond, body } => cond == name || uses_var(body, name),
        AnfExpr::EffectCall { args, .. }
        | AnfExpr::Dispatch { args, .. }
        | AnfExpr::TaskSpawn { args, .. }
        | AnfExpr::ResourceAcquire { args, .. } => args.iter().any(|arg| arg == name),
        AnfExpr::ChannelSend { channel, value }
        | AnfExpr::CellSet {
            cell: channel,
            value,
        } => channel == name || value == name,
        AnfExpr::ChannelReceive { channel }
        | AnfExpr::ResourceRelease { handle: channel }
        | AnfExpr::TaskAwait { task: channel }
        | AnfExpr::TaskCancel { task: channel }
        | AnfExpr::CellGet { cell: channel }
        | AnfExpr::CellNew { init: channel } => channel == name,
        AnfExpr::RuntimeCheck { cond, .. } => cond == name,
        AnfExpr::Select { branches } => branches
            .iter()
            .any(|branch| branch.channel == name || uses_var(&branch.body, name)),
        AnfExpr::Literal(_)
        | AnfExpr::Continue
        | AnfExpr::ChannelNew { .. }
        // ola5 Gap 2 — new primitives
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Placeholder => false,
        AnfExpr::IndexGet { collection, index } => collection == name || index == name,
        AnfExpr::MapNew { entries } => entries.iter().any(|(k, v)| k == name || v == name),
        AnfExpr::SetNew { elements } => elements.iter().any(|e| e == name),
        AnfExpr::ForEach { collection, body, binding } => {
            collection == name
                || (!binding.is_empty() && binding != name && uses_var(body, name))
        }
    }
}

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::NodeRef;

    use super::*;

    fn binding(expr: AnfExpr) -> AnfBinding {
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.main".to_string(),
            expr,
        }
    }

    #[test]
    fn constant_folding_rewrites_integer_primitive_calls() {
        let optimized = optimize_bindings(vec![binding(AnfExpr::Let {
            name: "a".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
            body: Box::new(AnfExpr::Let {
                name: "b".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(22))),
                body: Box::new(AnfExpr::Call {
                    func: "add".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
        })]);

        assert_eq!(optimized[0].expr, AnfExpr::Literal(LiteralValue::Int(42)));
    }

    #[test]
    fn dead_code_elimination_removes_unused_pure_lets() {
        let optimized = optimize_bindings(vec![binding(AnfExpr::Let {
            name: "unused".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
        })]);

        assert_eq!(optimized[0].expr, AnfExpr::Literal(LiteralValue::Int(2)));
    }

    // ── eliminate_dead_pure ───────────────────────────────────────────────

    #[test]
    fn eliminate_dead_pure_removes_pure_non_final_seq_element() {
        // Seq: [pure_expr, effect_call]
        // The first (pure) element should be removed; the effect call kept.
        let seq = AnfExpr::Seq(vec![
            AnfExpr::Literal(LiteralValue::Int(42)), // pure — dead
            AnfExpr::EffectCall {
                capability: "clock".to_string(),
                func: "now".to_string(),
                args: vec![],
            }, // effectful — kept
        ]);
        let result = eliminate_dead_pure(vec![binding(seq)]);
        // After elimination the Seq collapses to the single EffectCall.
        assert_eq!(
            result[0].expr,
            AnfExpr::EffectCall {
                capability: "clock".to_string(),
                func: "now".to_string(),
                args: vec![],
            }
        );
    }

    #[test]
    fn eliminate_dead_pure_keeps_all_effects_in_seq() {
        // Seq: [effect1, effect2]  — both effectful, neither removed.
        let seq = AnfExpr::Seq(vec![
            AnfExpr::EffectCall {
                capability: "db".to_string(),
                func: "write".to_string(),
                args: vec![],
            },
            AnfExpr::EffectCall {
                capability: "log".to_string(),
                func: "info".to_string(),
                args: vec![],
            },
        ]);
        let input = seq.clone();
        let result = eliminate_dead_pure(vec![binding(seq)]);
        assert_eq!(result[0].expr, input, "both effectful — seq must be unchanged");
    }

    #[test]
    fn eliminate_dead_pure_empty_seq_becomes_unit() {
        // A Seq with a single pure element that is NOT the final element
        // (edge case: Seq with one element total is the final element, kept).
        // Instead test the degenerate case where all non-final elements are pure
        // and the final element is also pure — nothing to drop but the seq collapses.
        let seq = AnfExpr::Seq(vec![AnfExpr::Literal(LiteralValue::Int(1))]);
        let result = eliminate_dead_pure(vec![binding(seq)]);
        // Single-element Seq collapses to the element.
        assert_eq!(result[0].expr, AnfExpr::Literal(LiteralValue::Int(1)));
    }

    // ── inline_small_pure ─────────────────────────────────────────────────

    #[test]
    fn inline_small_pure_inlines_single_arg_lambda() {
        // Binding A: fn.double = Lambda { params: ["x"], body: Call("mul", ["x", "two"]) }
        // Binding B: fn.main  = Let { a = Literal(2); b = Call("fn.double", ["a"]); b }
        // After inline: b = Call("mul", ["a", "two"])
        let lambda_binding = AnfBinding {
            source_ref: ail_core::semantic_graph::NodeRef(0),
            name: "fn.double".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["x".to_string()],
                body: Box::new(AnfExpr::Call {
                    func: "mul".to_string(),
                    args: vec!["x".to_string(), "two".to_string()],
                }),
            },
        };
        let main_binding = AnfBinding {
            source_ref: ail_core::semantic_graph::NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Let {
                name: "a".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
                body: Box::new(AnfExpr::Let {
                    name: "b".to_string(),
                    value: Box::new(AnfExpr::Call {
                        func: "fn.double".to_string(),
                        args: vec!["a".to_string()],
                    }),
                    body: Box::new(AnfExpr::Var("b".to_string())),
                }),
            },
        };

        let result = inline_small_pure(vec![lambda_binding, main_binding]);
        // The call to fn.double should be replaced by mul(a, two)
        let expected_b_value = AnfExpr::Call {
            func: "mul".to_string(),
            args: vec!["a".to_string(), "two".to_string()],
        };
        if let AnfExpr::Let { body, .. } = &result[1].expr {
            if let AnfExpr::Let { value, .. } = body.as_ref() {
                assert_eq!(
                    value.as_ref(),
                    &expected_b_value,
                    "call to fn.double must be inlined"
                );
            } else {
                panic!("expected inner Let");
            }
        } else {
            panic!("expected outer Let");
        }
    }

    #[test]
    fn inline_small_pure_does_not_inline_large_lambda() {
        // A lambda with 4+ nodes must NOT be inlined.
        let large_body = AnfExpr::Let {
            name: "t1".to_string(),
            value: Box::new(AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["x".to_string(), "y".to_string()],
            }),
            body: Box::new(AnfExpr::Let {
                name: "t2".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "add".to_string(),
                    args: vec!["t1".to_string(), "z".to_string()],
                }),
                body: Box::new(AnfExpr::Var("t2".to_string())),
            }),
        }; // 7 nodes — over the 3-node limit

        let lambda_binding = AnfBinding {
            source_ref: ail_core::semantic_graph::NodeRef(0),
            name: "fn.big".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["x".to_string(), "y".to_string(), "z".to_string()],
                body: Box::new(large_body),
            },
        };
        let call_binding = binding(AnfExpr::Call {
            func: "fn.big".to_string(),
            args: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        });

        let result = inline_small_pure(vec![lambda_binding, call_binding]);
        // Call must remain unchanged.
        assert_eq!(
            result[1].expr,
            AnfExpr::Call {
                func: "fn.big".to_string(),
                args: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            }
        );
    }

    // ── cse_bindings ──────────────────────────────────────────────────────

    #[test]
    fn cse_replaces_duplicate_pure_call_with_var_reference() {
        // let a = add(x, y) in
        // let b = add(x, y) in   ← same as a — should become: let b = a
        // pair(a, b)
        let expr = AnfExpr::Let {
            name: "a".to_string(),
            value: Box::new(AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["x".to_string(), "y".to_string()],
            }),
            body: Box::new(AnfExpr::Let {
                name: "b".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
                body: Box::new(AnfExpr::Call {
                    func: "pair".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
        };

        let result = cse_bindings(vec![binding(expr)]);

        // The value of the second Let should now be Var("a").
        if let AnfExpr::Let { body, .. } = &result[0].expr {
            if let AnfExpr::Let { value, .. } = body.as_ref() {
                assert_eq!(
                    value.as_ref(),
                    &AnfExpr::Var("a".to_string()),
                    "duplicate pure expression must be aliased to first occurrence"
                );
            } else {
                panic!("expected inner Let");
            }
        } else {
            panic!("expected outer Let");
        }
    }

    #[test]
    fn cse_does_not_share_across_if_branches() {
        // let cond = true in
        // if cond {
        //   let a = add(x, y) in a
        // } else {
        //   let b = add(x, y) in b
        // }
        // The two `add(x, y)` expressions are in separate branches — no CSE.
        let branch_expr = |name: &str| AnfExpr::Let {
            name: name.to_string(),
            value: Box::new(AnfExpr::Call {
                func: "add".to_string(),
                args: vec!["x".to_string(), "y".to_string()],
            }),
            body: Box::new(AnfExpr::Var(name.to_string())),
        };
        let expr = AnfExpr::Let {
            name: "cond".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::If {
                cond: "cond".to_string(),
                then_branch: Box::new(branch_expr("a")),
                else_branch: Box::new(branch_expr("b")),
            }),
        };

        let result = cse_bindings(vec![binding(expr)]);

        // Both branches should keep their original add(x, y) — not CSE'd.
        if let AnfExpr::Let { body, .. } = &result[0].expr {
            if let AnfExpr::If {
                then_branch,
                else_branch,
                ..
            } = body.as_ref()
            {
                let expected_call = AnfExpr::Call {
                    func: "add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                };
                if let AnfExpr::Let { value: tv, .. } = then_branch.as_ref() {
                    assert_eq!(tv.as_ref(), &expected_call, "then branch must not be CSE'd");
                }
                if let AnfExpr::Let { value: ev, .. } = else_branch.as_ref() {
                    assert_eq!(ev.as_ref(), &expected_call, "else branch must not be CSE'd");
                }
            } else {
                panic!("expected If");
            }
        } else {
            panic!("expected outer Let");
        }
    }

    #[test]
    fn dead_code_elimination_keeps_effects() {
        let expr = AnfExpr::Let {
            name: "unused".to_string(),
            value: Box::new(AnfExpr::EffectCall {
                capability: "clock.now".to_string(),
                func: "now".to_string(),
                args: vec![],
            }),
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
        };
        let optimized = optimize_bindings(vec![binding(expr.clone())]);

        assert_eq!(optimized[0].expr, expr);
    }
}
