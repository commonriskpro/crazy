use std::collections::BTreeMap;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;

use super::{is_pure, uses_var};

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
        AnfExpr::Lambda {
            params,
            captures: _,
            body,
        } => {
            let mut nested_env = env.clone();
            for param in &params {
                nested_env.remove(param);
            }
            let optimized_body = optimize_expr(*body, &mut nested_env);
            // Recompute captures from the optimized body: constant-folding may
            // have eliminated references to captured vars, making the old list
            // stale and causing `uses_var` false positives in dead-let DCE.
            // This is a single traversal — cheap; no broad optimizer refactor.
            let mut bound: Vec<&str> = params.iter().map(String::as_str).collect();
            let mut free_in_body: Vec<&str> = Vec::new();
            crate::wasm_abi::collect_free_vars(&optimized_body, &mut bound, &mut free_in_body);
            let captures = free_in_body.into_iter().map(str::to_string).collect();
            AnfExpr::Lambda {
                params,
                captures,
                body: Box::new(optimized_body),
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
        ("int.min" | "int_min", [a, b]) => Some(LiteralValue::Int((*a).min(*b))),
        ("int.max" | "int_max", [a, b]) => Some(LiteralValue::Int((*a).max(*b))),
        ("int.abs_or" | "int_abs_or", [value, fallback]) => {
            Some(LiteralValue::Int(value.checked_abs().unwrap_or(*fallback)))
        }
        ("int.neg_or" | "int_neg_or", [value, fallback]) => {
            Some(LiteralValue::Int(value.checked_neg().unwrap_or(*fallback)))
        }
        ("int.add_or" | "int_add_or", [left, right, fallback]) => Some(LiteralValue::Int(
            left.checked_add(*right).unwrap_or(*fallback),
        )),
        ("int.sub_or" | "int_sub_or", [left, right, fallback]) => Some(LiteralValue::Int(
            left.checked_sub(*right).unwrap_or(*fallback),
        )),
        ("int.div_or" | "int_div_or", [value, divisor, fallback]) => Some(LiteralValue::Int(
            value.checked_div(*divisor).unwrap_or(*fallback),
        )),
        ("int.rem_or" | "int_rem_or", [value, divisor, fallback]) => Some(LiteralValue::Int(
            value.checked_rem(*divisor).unwrap_or(*fallback),
        )),
        ("int.clamp" | "int_clamp", [value, low, high]) => {
            Some(LiteralValue::Int(if value < low {
                *low
            } else if value > high {
                *high
            } else {
                *value
            }))
        }
        ("i64.eq" | "==" | "eq", [a, b]) => Some(LiteralValue::Bool(a == b)),
        ("i64.ne" | "!=" | "ne", [a, b]) => Some(LiteralValue::Bool(a != b)),
        ("i64.lt_s" | "<" | "lt", [a, b]) => Some(LiteralValue::Bool(a < b)),
        ("i64.le_s" | "<=" | "le", [a, b]) => Some(LiteralValue::Bool(a <= b)),
        ("i64.gt_s" | ">" | "gt", [a, b]) => Some(LiteralValue::Bool(a > b)),
        ("i64.ge_s" | ">=" | "ge", [a, b]) => Some(LiteralValue::Bool(a >= b)),
        _ => None,
    }
}
