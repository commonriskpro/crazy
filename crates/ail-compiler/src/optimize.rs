// ── ail-compiler::optimize ────────────────────────────────────────────────
//
// Conservative ANF optimizations. These passes only rewrite pure local
// expressions and never remove top-level bindings or effect/resource nodes.

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
        | AnfExpr::CellNew { init: channel }
        | AnfExpr::Timeout {
            duration: channel, ..
        } => channel == name,
        AnfExpr::RuntimeCheck { cond, .. } => cond == name,
        AnfExpr::ShortCircuitAnd { left, .. } | AnfExpr::ShortCircuitOr { left, .. } => {
            left == name
        }
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
