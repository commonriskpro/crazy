use ail_core::semantic_graph::NodeRef;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::{CoreExpr, LiteralValue};

use super::super::{
    atomize, lower_core_binary_to_anf, lower_core_expr_to_anf_local, lower_core_unary_to_anf,
};
use super::lower_core_expr_to_anf;

pub(super) fn try_lower(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
    out: &mut Vec<AnfBinding>,
) -> Option<AnfExpr> {
    let result = match expr {
        // Atomic values — no sub-expressions to flatten.
        CoreExpr::Literal(v) => AnfExpr::Literal(v.clone()),
        CoreExpr::Var(n) => AnfExpr::Var(n.clone()),

        // Let: lower value and body recursively; no atomization needed.
        CoreExpr::Let { name, value, body } => {
            let anf_value = lower_core_expr_to_anf(value, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Let {
                name: name.clone(),
                value: Box::new(anf_value),
                body: Box::new(anf_body),
            }
        }

        // If: condition must be atomic (atomize if needed).
        CoreExpr::If { cond, then_, else_ } => {
            let cond_name = atomize(cond, fresh, source_ref, out);
            let anf_then = lower_core_expr_to_anf(then_, fresh, source_ref, out);
            let anf_else = lower_core_expr_to_anf(else_, fresh, source_ref, out);
            AnfExpr::If {
                cond: cond_name,
                then_branch: Box::new(anf_then),
                else_branch: Box::new(anf_else),
            }
        }

        // Call: all args must be atomic (atomize each non-Var arg).
        CoreExpr::Call { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: func.clone(),
                args: atomic_args,
            }
        }
        CoreExpr::Add(left, right) => {
            lower_core_binary_to_anf("add", left, right, fresh, source_ref)
        }
        CoreExpr::Sub(left, right) => {
            lower_core_binary_to_anf("sub", left, right, fresh, source_ref)
        }
        CoreExpr::Mul(left, right) => {
            lower_core_binary_to_anf("mul", left, right, fresh, source_ref)
        }
        CoreExpr::Div(left, right) => {
            lower_core_binary_to_anf("div", left, right, fresh, source_ref)
        }
        CoreExpr::Mod(left, right) => {
            lower_core_binary_to_anf("mod", left, right, fresh, source_ref)
        }
        CoreExpr::Eq(left, right) => lower_core_binary_to_anf("eq", left, right, fresh, source_ref),
        CoreExpr::Lt(left, right) => lower_core_binary_to_anf("lt", left, right, fresh, source_ref),
        CoreExpr::Gt(left, right) => lower_core_binary_to_anf("gt", left, right, fresh, source_ref),
        CoreExpr::Ne(left, right) => lower_core_binary_to_anf("ne", left, right, fresh, source_ref),
        CoreExpr::Le(left, right) => lower_core_binary_to_anf("le", left, right, fresh, source_ref),
        CoreExpr::Ge(left, right) => lower_core_binary_to_anf("ge", left, right, fresh, source_ref),
        CoreExpr::Not(operand) => lower_core_unary_to_anf("not", operand, fresh, source_ref),

        // FieldGet: record expression must be atomic.
        CoreExpr::FieldGet { record, field } => {
            let record_name = atomize(record, fresh, source_ref, out);
            AnfExpr::FieldGet {
                record: record_name,
                field: field.clone(),
            }
        }

        // ── G20: Expression body lowering ────────────────────────────────

        // Match: scrutinee must be atomic (atomize if non-Var).
        // Each arm body is lowered recursively.
        CoreExpr::Match { scrutinee, arms } => {
            let scrutinee_name = atomize(scrutinee, fresh, source_ref, out);
            let anf_arms = arms
                .iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern.clone(),
                    body: lower_core_expr_to_anf(&arm.body, fresh, source_ref, out),
                })
                .collect();
            AnfExpr::Match {
                scrutinee: scrutinee_name,
                arms: anf_arms,
            }
        }

        // Lambda: params are already names; lower body recursively.
        // After lowering the body, collect its free variables relative to
        // `params` — these become the explicit closure captures.
        CoreExpr::Lambda { params, body } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            // Compute captures: free vars in the lowered body minus the params.
            let mut bound: Vec<&str> = params.iter().map(String::as_str).collect();
            let mut free: Vec<&str> = Vec::new();
            crate::wasm_abi::collect_free_vars(&anf_body, &mut bound, &mut free);
            let captures: Vec<String> = free.into_iter().map(str::to_owned).collect();
            AnfExpr::Lambda {
                params: params.clone(),
                captures,
                body: Box::new(anf_body),
            }
        }

        // RecordNew: full ANF normalization — each field value is let-bound
        // so field construction arguments are always atomic.
        CoreExpr::RecordNew { fields } => {
            let anf_fields: Vec<(String, AnfExpr)> = fields
                .iter()
                .map(|(name, val)| {
                    let atom = atomize(val, fresh, source_ref, out);
                    (name.clone(), AnfExpr::Var(atom))
                })
                .collect();
            AnfExpr::RecordNew { fields: anf_fields }
        }

        // FieldUpdate: record expression must be atomic; value is also atomized
        // for full ANF normalization.
        CoreExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            let record_name = atomize(record, fresh, source_ref, out);
            let value_name = atomize(value, fresh, source_ref, out);
            AnfExpr::FieldUpdate {
                record: record_name,
                field: field.clone(),
                value: Box::new(AnfExpr::Var(value_name)),
            }
        }

        // TupleNew: full ANF normalization — each element is let-bound.
        CoreExpr::TupleNew(elems) => {
            let anf_elems: Vec<AnfExpr> = elems
                .iter()
                .map(|e| {
                    let name = atomize(e, fresh, source_ref, out);
                    AnfExpr::Var(name)
                })
                .collect();
            AnfExpr::TupleNew(anf_elems)
        }

        // VariantNew: payload is atomized for full ANF normalization.
        CoreExpr::VariantNew { tag, payload } => {
            let anf_payload = payload.as_ref().map(|p| {
                let name = atomize(p, fresh, source_ref, out);
                Box::new(AnfExpr::Var(name))
            });
            AnfExpr::VariantNew {
                tag: tag.clone(),
                payload: anf_payload,
            }
        }

        // ListNew: lower each element recursively; let-bind non-atomic elements
        // to enforce full ANF normalization.
        CoreExpr::ListNew(elems) => {
            let anf_elems: Vec<AnfExpr> = elems
                .iter()
                .map(|e| {
                    let name = atomize(e, fresh, source_ref, out);
                    AnfExpr::Var(name)
                })
                .collect();
            AnfExpr::ListNew(anf_elems)
        }

        // Loop: body is lowered recursively; exits through Break.
        // The termination field is not used during ANF lowering.
        CoreExpr::Loop { body, .. } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Loop {
                body: Box::new(anf_body),
            }
        }

        // Break: value is lowered recursively so it can be emitted before br.
        CoreExpr::Break { value } => {
            let anf_value = lower_core_expr_to_anf(value, fresh, source_ref, out);
            AnfExpr::Break {
                value: Box::new(anf_value),
            }
        }

        CoreExpr::Continue => AnfExpr::Continue,

        // WhileLoop: delegate to the local lowering path.
        //
        // The desugared form (Loop + If + Break/Continue) is self-contained —
        // the condition expression lives inside the Loop body node and must not
        // be pushed to the outer `out` bindings.  Delegating to
        // `lower_core_expr_to_anf_local` produces the fully nested Let form
        // with correct per-iteration condition re-evaluation.
        //
        // See the `lower_core_expr_to_anf_local` WhileLoop arm for the full
        // desugaring rationale and structure.
        CoreExpr::WhileLoop { .. } => lower_core_expr_to_anf_local(expr, fresh, source_ref),

        _ => return None,
    };
    Some(result)
}
