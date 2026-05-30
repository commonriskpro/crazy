use super::*;

// ── Binding analysis ──────────────────────────────────────────────────────

pub(crate) fn collect_free_vars<'a>(
    expr: &'a AnfExpr,
    bound: &mut Vec<&'a str>,
    out: &mut Vec<&'a str>,
) {
    match expr {
        AnfExpr::Var(name)
            if !bound.iter().rev().any(|bound_name| *bound_name == name)
                && !out.iter().any(|existing| *existing == name) =>
        {
            out.push(name);
        }
        AnfExpr::Let { name, value, body } => {
            collect_free_vars(value, bound, out);
            bound.push(name);
            collect_free_vars(body, bound, out);
            bound.pop();
        }
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if !bound.iter().rev().any(|bound_name| *bound_name == cond)
                && !out.iter().any(|existing| *existing == cond)
            {
                out.push(cond);
            }
            collect_free_vars(then_branch, bound, out);
            collect_free_vars(else_branch, bound, out);
        }
        AnfExpr::Call { args, .. } => {
            for arg in args {
                if !bound.iter().rev().any(|bound_name| *bound_name == arg)
                    && !out.iter().any(|existing| *existing == arg)
                {
                    out.push(arg);
                }
            }
        }
        AnfExpr::EffectCall { args, .. } => {
            for arg in args {
                if !bound.iter().rev().any(|bound_name| *bound_name == arg)
                    && !out.iter().any(|existing| *existing == arg)
                {
                    out.push(arg);
                }
            }
        }
        AnfExpr::Return(inner)
        | AnfExpr::ShortCircuitAnd { right: inner, .. }
        | AnfExpr::ShortCircuitOr { right: inner, .. }
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::FieldUpdate { value: inner, .. } => collect_free_vars(inner, bound, out),
        AnfExpr::WhileLoop { cond, body } => {
            if !bound.iter().rev().any(|bound_name| *bound_name == cond)
                && !out.iter().any(|existing| *existing == cond)
            {
                out.push(cond);
            }
            collect_free_vars(body, bound, out);
        }
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            for expr in exprs {
                collect_free_vars(expr, bound, out);
            }
        }
        AnfExpr::Match { arms, .. } => {
            for arm in arms {
                // A single-binding constructor pattern like "Ok(x)" introduces
                // a payload variable that is locally bound within the arm body.
                // Add it to `bound` so it is not reported as a free variable.
                let payload = arm_payload_binding(&arm.pattern);
                if let Some(name) = payload {
                    bound.push(name);
                }
                collect_free_vars(&arm.body, bound, out);
                if payload.is_some() {
                    bound.pop();
                }
            }
        }
        // Lambda: the `captures` field explicitly names the free variables this
        // lambda needs from the enclosing scope.  Propagate each capture to
        // `out` if it is not already bound — this is more efficient than
        // re-scanning the body and produces the same set as long as captures
        // were populated correctly by the lowering pass.
        //
        // An empty captures list has two meanings: the lambda genuinely closes
        // over nothing, or it is a hand-built fixture that did not populate the
        // field.  Both cases are handled by the body-scan fallback below.
        AnfExpr::Lambda {
            params,
            body,
            captures,
        } => {
            if captures.is_empty() {
                // Fallback: re-scan the body for free vars — handles lambdas
                // that capture nothing, including hand-built fixtures that omit
                // the captures field.
                let original_len = bound.len();
                bound.extend(params.iter().map(String::as_str));
                collect_free_vars(body, bound, out);
                bound.truncate(original_len);
            } else {
                // Fast path: use the explicit capture list.
                for cap in captures {
                    if !bound.iter().rev().any(|b| *b == cap) && !out.iter().any(|e| *e == cap) {
                        out.push(cap);
                    }
                }
            }
        }
        AnfExpr::RecordNew { fields } => {
            for (_, expr) in fields {
                collect_free_vars(expr, bound, out);
            }
        }
        AnfExpr::VariantNew {
            payload: Some(payload),
            ..
        } => collect_free_vars(payload, bound, out),
        _ => {}
    }
}

pub(crate) fn binding_params(binding: &AnfBinding) -> Vec<&str> {
    let mut params = Vec::new();
    collect_free_vars(&binding.expr, &mut Vec::new(), &mut params);
    params
}

/// Returns the `params` field of a top-level `Lambda` expression, or `&[]`
/// for non-Lambda expressions.
///
/// Used by `binding_signatures` and `build_code_section` to include the
/// Lambda's own call parameters (distinct from captures) in the WASM function
/// signature.  For a top-level Lambda binding the WASM function emits the
/// Lambda body directly, so its params are additional WASM function locals
/// beyond the captured-variable locals that come from `binding_params`.
pub(crate) fn lambda_body_params(expr: &AnfExpr) -> &[String] {
    match expr {
        AnfExpr::Lambda { params, .. } => params,
        _ => &[],
    }
}

pub(crate) fn binding_result(binding: &AnfBinding) -> Option<ValType> {
    match &binding.expr {
        // For a top-level Lambda binding the WASM function emits the Lambda
        // body directly (captures + Lambda params in scope).  Infer the
        // result type from the body, not from the Lambda node itself (which
        // would always return I32 for the nested-closure-ptr path).
        AnfExpr::Lambda { params, body, .. } => {
            let mut locals: Vec<(String, ValType)> = binding_params(binding)
                .into_iter()
                .map(|name| (name.to_string(), ValType::I64))
                .collect();
            // Add the Lambda's own params after the captured-variable locals.
            locals.extend(params.iter().map(|p| (p.clone(), ValType::I64)));
            infer_expr_type(body, &mut locals)
                .filter(|ty| matches!(ty, ValType::I64 | ValType::I32))
        }
        expr => {
            let mut locals = binding_params(binding)
                .into_iter()
                .map(|name| (name.to_string(), ValType::I64))
                .collect();
            infer_expr_type(expr, &mut locals)
                .filter(|ty| matches!(ty, ValType::I64 | ValType::I32))
        }
    }
}

pub(crate) fn binding_signatures(bindings: &[AnfBinding]) -> Vec<WasmSignature> {
    bindings
        .iter()
        .map(|binding| {
            // For Lambda bindings: WASM params = captures + Lambda's own params.
            let capture_count = binding_params(binding).len();
            let lambda_param_count = lambda_body_params(&binding.expr).len();
            WasmSignature {
                param_count: capture_count + lambda_param_count,
                result: binding_result(binding),
            }
        })
        .collect()
}
