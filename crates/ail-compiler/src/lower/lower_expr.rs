// ── ail-compiler::lower::lower_expr ──────────────────────────────────────
//
// Expression lowering helpers: CoreExpr → AnfExpr.
//
// All functions in this module convert individual `CoreExpr` nodes to their
// ANF counterparts.  They are re-exported through `lower.rs` and form part of
// the public compiler API (`lower_core_expr_to_anf`).

use ail_core::semantic_graph::NodeRef;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::{CoreExpr, LiteralValue};

// ── atomize ───────────────────────────────────────────────────────────────

/// Ensure `expr` is atomic (a variable name).
///
/// If `expr` is already `CoreExpr::Var(n)`, returns `n` without emitting any
/// binding.  Otherwise lowers `expr` to an `AnfExpr`, pushes a synthetic
/// `AnfBinding` with a fresh name, and returns that fresh name.
///
/// The pushed binding carries the same `source_ref` as the enclosing node
/// (provenance is preserved for synthetic temporaries).
pub(super) fn atomize(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
    out: &mut Vec<AnfBinding>,
) -> String {
    if let CoreExpr::Var(name) = expr {
        return name.clone();
    }
    let anf_expr = lower_core_expr_to_anf(expr, fresh, source_ref, out);
    let name = format!("anf_{}", *fresh);
    *fresh += 1;
    out.push(AnfBinding {
        source_ref,
        name: name.clone(),
        expr: anf_expr,
    });
    name
}

pub(super) fn atomize_local(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
) -> (String, Option<(String, AnfExpr)>) {
    if let CoreExpr::Var(name) = expr {
        return (name.clone(), None);
    }
    let value = lower_core_expr_to_anf_local(expr, fresh, source_ref);
    let name = format!("anf_{}", *fresh);
    *fresh += 1;
    (name.clone(), Some((name, value)))
}

pub(super) fn wrap_local_bindings(mut bindings: Vec<(String, AnfExpr)>, body: AnfExpr) -> AnfExpr {
    bindings.reverse();
    bindings
        .into_iter()
        .fold(body, |body, (name, value)| AnfExpr::Let {
            name,
            value: Box::new(value),
            body: Box::new(body),
        })
}

pub(super) fn lower_core_call_to_anf(
    func: &str,
    args: &[CoreExpr],
    fresh: &mut u32,
    source_ref: NodeRef,
) -> AnfExpr {
    let mut bindings = Vec::new();
    let mut arg_names = Vec::with_capacity(args.len());
    for arg in args {
        let (name, binding) = atomize_local(arg, fresh, source_ref);
        if let Some(binding) = binding {
            bindings.push(binding);
        }
        arg_names.push(name);
    }
    wrap_local_bindings(
        bindings,
        AnfExpr::Call {
            func: func.to_string(),
            args: arg_names,
        },
    )
}

pub(super) fn lower_core_binary_to_anf(
    func: &str,
    left: &CoreExpr,
    right: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
) -> AnfExpr {
    lower_core_call_to_anf(func, &[left.clone(), right.clone()], fresh, source_ref)
}

pub(super) fn lower_core_unary_to_anf(
    func: &str,
    operand: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
) -> AnfExpr {
    lower_core_call_to_anf(func, std::slice::from_ref(operand), fresh, source_ref)
}

pub(super) fn lower_core_expr_to_anf_local(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
) -> AnfExpr {
    match expr {
        CoreExpr::Let { name, value, body } => AnfExpr::Let {
            name: name.clone(),
            value: Box::new(lower_core_expr_to_anf_local(value, fresh, source_ref)),
            body: Box::new(lower_core_expr_to_anf_local(body, fresh, source_ref)),
        },
        CoreExpr::If { cond, then_, else_ } => {
            let (cond_name, cond_binding) = atomize_local(cond, fresh, source_ref);
            let if_expr = AnfExpr::If {
                cond: cond_name,
                then_branch: Box::new(lower_core_expr_to_anf_local(then_, fresh, source_ref)),
                else_branch: Box::new(lower_core_expr_to_anf_local(else_, fresh, source_ref)),
            };
            if let Some(binding) = cond_binding {
                wrap_local_bindings(vec![binding], if_expr)
            } else {
                if_expr
            }
        }
        CoreExpr::Match { scrutinee, arms } => {
            let (scrutinee_name, scrutinee_binding) = atomize_local(scrutinee, fresh, source_ref);
            let match_expr = AnfExpr::Match {
                scrutinee: scrutinee_name,
                arms: arms
                    .iter()
                    .map(|arm| crate::anf::AnfMatchArm {
                        pattern: arm.pattern.clone(),
                        body: lower_core_expr_to_anf_local(&arm.body, fresh, source_ref),
                    })
                    .collect(),
            };
            if let Some(binding) = scrutinee_binding {
                wrap_local_bindings(vec![binding], match_expr)
            } else {
                match_expr
            }
        }
        CoreExpr::Call { func, args } => lower_core_call_to_anf(func, args, fresh, source_ref),
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
        CoreExpr::And { left, right } => {
            let (left_name, left_binding) = atomize_local(left, fresh, source_ref);
            let and_expr = AnfExpr::ShortCircuitAnd {
                left: left_name,
                right: Box::new(lower_core_expr_to_anf_local(right, fresh, source_ref)),
            };
            if let Some(binding) = left_binding {
                wrap_local_bindings(vec![binding], and_expr)
            } else {
                and_expr
            }
        }
        CoreExpr::Or { left, right } => {
            let (left_name, left_binding) = atomize_local(left, fresh, source_ref);
            let or_expr = AnfExpr::ShortCircuitOr {
                left: left_name,
                right: Box::new(lower_core_expr_to_anf_local(right, fresh, source_ref)),
            };
            if let Some(binding) = left_binding {
                wrap_local_bindings(vec![binding], or_expr)
            } else {
                or_expr
            }
        }
        CoreExpr::FieldGet { record, field } => {
            let (record_name, record_binding) = atomize_local(record, fresh, source_ref);
            let field_expr = AnfExpr::FieldGet {
                record: record_name,
                field: field.clone(),
            };
            if let Some(binding) = record_binding {
                wrap_local_bindings(vec![binding], field_expr)
            } else {
                field_expr
            }
        }
        CoreExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            let (record_name, record_binding) = atomize_local(record, fresh, source_ref);
            let (value_name, value_binding) = atomize_local(value, fresh, source_ref);
            let update_expr = AnfExpr::FieldUpdate {
                record: record_name,
                field: field.clone(),
                value: Box::new(AnfExpr::Var(value_name)),
            };
            let bindings = [record_binding, value_binding]
                .into_iter()
                .flatten()
                .collect();
            wrap_local_bindings(bindings, update_expr)
        }
        CoreExpr::RecordNew { fields } => {
            let mut bindings = Vec::new();
            let mut anf_fields = Vec::with_capacity(fields.len());
            for (field, value) in fields {
                let (name, binding) = atomize_local(value, fresh, source_ref);
                if let Some(binding) = binding {
                    bindings.push(binding);
                }
                anf_fields.push((field.clone(), AnfExpr::Var(name)));
            }
            wrap_local_bindings(bindings, AnfExpr::RecordNew { fields: anf_fields })
        }
        CoreExpr::TupleNew(elems) => {
            let mut bindings = Vec::new();
            let mut anf_elems = Vec::with_capacity(elems.len());
            for elem in elems {
                let (name, binding) = atomize_local(elem, fresh, source_ref);
                if let Some(binding) = binding {
                    bindings.push(binding);
                }
                anf_elems.push(AnfExpr::Var(name));
            }
            wrap_local_bindings(bindings, AnfExpr::TupleNew(anf_elems))
        }
        CoreExpr::VariantNew { tag, payload } => {
            let mut bindings = Vec::new();
            let anf_payload = if let Some(payload) = payload {
                let (name, binding) = atomize_local(payload, fresh, source_ref);
                if let Some(binding) = binding {
                    bindings.push(binding);
                }
                Some(Box::new(AnfExpr::Var(name)))
            } else {
                None
            };
            wrap_local_bindings(
                bindings,
                AnfExpr::VariantNew {
                    tag: tag.clone(),
                    payload: anf_payload,
                },
            )
        }
        CoreExpr::ListNew(elems) => {
            let mut bindings = Vec::new();
            let mut anf_elems = Vec::with_capacity(elems.len());
            for elem in elems {
                let (name, binding) = atomize_local(elem, fresh, source_ref);
                if let Some(binding) = binding {
                    bindings.push(binding);
                }
                anf_elems.push(AnfExpr::Var(name));
            }
            wrap_local_bindings(bindings, AnfExpr::ListNew(anf_elems))
        }
        // ── Loop constructs ───────────────────────────────────────────────
        //
        // These variants were previously handled by the `_` fallthrough which
        // called `lower_core_expr_to_anf(..., &mut Vec::new())`.  That discards
        // any atomized bindings, leaving the synthesised names undefined at
        // runtime.  Each arm below uses `atomize_local` + `wrap_local_bindings`
        // so no binding is lost.

        // Loop: body is lowered recursively; no sub-expression needs atomizing.
        CoreExpr::Loop { body, .. } => AnfExpr::Loop {
            body: Box::new(lower_core_expr_to_anf_local(body, fresh, source_ref)),
        },

        // Break: value is lowered recursively; no atomization required at this
        // level (Break's value is not required to be atomic in ANF).
        CoreExpr::Break { value } => AnfExpr::Break {
            value: Box::new(lower_core_expr_to_anf_local(value, fresh, source_ref)),
        },

        // Continue: no sub-expressions.
        CoreExpr::Continue => AnfExpr::Continue,

        // WhileLoop: desugar into Loop + If + Break/Continue so the condition
        // expression is re-evaluated inside the loop body on every iteration.
        //
        // CoreExpr::WhileLoop { cond, body }
        // ↦
        // Seq([
        //   Loop {
        //     body: Let { anf_N = lower(cond),   ← re-evaluated each iteration
        //             If { cond: anf_N,
        //                  then_: Let { anf_M = lower(body), Continue },
        //                  else_: Break { Literal(Unit) } } }
        //   },
        //   Literal(Unit),   ← unit sentinel produced after the loop exits
        // ])
        //
        // The condition is lowered into the Loop body (not hoisted outside),
        // which fixes the stale-condition bug: previously `atomize_local`
        // hoisted the condition outside the AnfExpr::WhileLoop, so a computed
        // expression like `lt(cell_get(c), 3)` was evaluated exactly once and
        // subsequent iterations re-used the stale binding value.
        //
        // AnfExpr::WhileLoop is preserved for direct ANF construction
        // (backward compatibility).  Only the CoreExpr → ANF lowering path
        // is changed here.
        CoreExpr::WhileLoop { cond, body, .. } => {
            let cond_lowered = lower_core_expr_to_anf_local(cond, fresh, source_ref);
            let cond_tmp = format!("anf_{}", *fresh);
            *fresh += 1;
            let body_lowered = lower_core_expr_to_anf_local(body, fresh, source_ref);
            let body_tmp = format!("anf_{}", *fresh);
            *fresh += 1;
            let if_expr = AnfExpr::If {
                cond: cond_tmp.clone(),
                then_branch: Box::new(AnfExpr::Let {
                    name: body_tmp,
                    value: Box::new(body_lowered),
                    body: Box::new(AnfExpr::Continue),
                }),
                else_branch: Box::new(AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                }),
            };
            let loop_body = AnfExpr::Let {
                name: cond_tmp,
                value: Box::new(cond_lowered),
                body: Box::new(if_expr),
            };
            AnfExpr::Seq(vec![
                AnfExpr::Loop {
                    body: Box::new(loop_body),
                },
                AnfExpr::Literal(LiteralValue::Unit),
            ])
        }

        // ForEach: collection must be atomic; body is lowered recursively.
        CoreExpr::ForEach {
            binding,
            collection,
            body,
        } => {
            let (col_name, col_binding) = atomize_local(collection, fresh, source_ref);
            let anf_body = lower_core_expr_to_anf_local(body, fresh, source_ref);
            let for_expr = AnfExpr::ForEach {
                binding: binding.clone(),
                collection: col_name,
                body: Box::new(anf_body),
            };
            if let Some(b) = col_binding {
                wrap_local_bindings(vec![b], for_expr)
            } else {
                for_expr
            }
        }

        // Fold: init, list, and func must all be atomic.
        CoreExpr::Fold { init, list, func } => {
            let (init_name, init_binding) = atomize_local(init, fresh, source_ref);
            let (list_name, list_binding) = atomize_local(list, fresh, source_ref);
            let (func_name, func_binding) = atomize_local(func, fresh, source_ref);
            let fold_expr = AnfExpr::Fold {
                init: init_name,
                list: list_name,
                func: func_name,
            };
            let bindings = [init_binding, list_binding, func_binding]
                .into_iter()
                .flatten()
                .collect();
            wrap_local_bindings(bindings, fold_expr)
        }

        // CellNew: init must be atomic.
        CoreExpr::CellNew { init } => {
            let (init_name, init_binding) = atomize_local(init, fresh, source_ref);
            let cell_expr = AnfExpr::CellNew { init: init_name };
            if let Some(binding) = init_binding {
                wrap_local_bindings(vec![binding], cell_expr)
            } else {
                cell_expr
            }
        }

        // CellGet: cell pointer must be atomic.
        CoreExpr::CellGet { cell } => {
            let (cell_name, cell_binding) = atomize_local(cell, fresh, source_ref);
            let cell_expr = AnfExpr::CellGet { cell: cell_name };
            if let Some(binding) = cell_binding {
                wrap_local_bindings(vec![binding], cell_expr)
            } else {
                cell_expr
            }
        }

        // CellSet: both cell pointer and value must be atomic.
        CoreExpr::CellSet { cell, value } => {
            let (cell_name, cell_binding) = atomize_local(cell, fresh, source_ref);
            let (value_name, value_binding) = atomize_local(value, fresh, source_ref);
            let cell_expr = AnfExpr::CellSet {
                cell: cell_name,
                value: value_name,
            };
            let bindings = [cell_binding, value_binding]
                .into_iter()
                .flatten()
                .collect();
            wrap_local_bindings(bindings, cell_expr)
        }

        // MapNew: atomize each key and value into local let-bindings.
        // Uses atomize_local (not atomize) so synthetic temporaries are scoped
        // as inline AnfExpr::Let nodes visible to the WASM emitter.
        CoreExpr::MapNew { entries } => {
            let mut bindings = Vec::new();
            let mut anf_entries = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let (k_name, k_binding) = atomize_local(k, fresh, source_ref);
                let (v_name, v_binding) = atomize_local(v, fresh, source_ref);
                if let Some(binding) = k_binding {
                    bindings.push(binding);
                }
                if let Some(binding) = v_binding {
                    bindings.push(binding);
                }
                anf_entries.push((k_name, v_name));
            }
            wrap_local_bindings(
                bindings,
                AnfExpr::MapNew {
                    entries: anf_entries,
                },
            )
        }

        // SetNew: atomize each element into a local let-binding.
        CoreExpr::SetNew { elements } => {
            let mut bindings = Vec::new();
            let mut anf_elements = Vec::with_capacity(elements.len());
            for elem in elements {
                let (name, binding) = atomize_local(elem, fresh, source_ref);
                if let Some(binding) = binding {
                    bindings.push(binding);
                }
                anf_elements.push(name);
            }
            wrap_local_bindings(
                bindings,
                AnfExpr::SetNew {
                    elements: anf_elements,
                },
            )
        }

        // IndexGet: atomize collection and index into local let-bindings.
        // Uses atomize_local so synthetic temporaries (e.g. the integer index
        // literal) are emitted as inline AnfExpr::Let nodes visible to the
        // WASM emitter.  Falling through to the `_` arm would discard those
        // bindings, leaving the IndexGet referencing an undefined local →
        // runtime trap.
        CoreExpr::IndexGet { collection, index } => {
            let (col_name, col_binding) = atomize_local(collection, fresh, source_ref);
            let (idx_name, idx_binding) = atomize_local(index, fresh, source_ref);
            let mut bindings = Vec::new();
            if let Some(b) = col_binding {
                bindings.push(b);
            }
            if let Some(b) = idx_binding {
                bindings.push(b);
            }
            wrap_local_bindings(
                bindings,
                AnfExpr::IndexGet {
                    collection: col_name,
                    index: idx_name,
                },
            )
        }

        // ── Fallthrough gap ───────────────────────────────────────────────
        //
        // Variants without an explicit local arm fall here.
        // `lower_core_expr_to_anf` is called with an immediately discarded
        // `Vec::new()`, so any synthetic bindings it pushes are silently lost.
        //
        // Safe to fall through (produce no atomized bindings):
        //   Literal, Var, ChannelNew, Continue, Placeholder, Assume, Abort
        //
        // UNSAFE if reached from a local-lowering context before an explicit
        // arm is added — sub-expression bindings would be discarded and the
        // resulting ANF would reference undefined names:
        //   EffectCall, Dispatch, TaskSpawn, ChannelSend, ChannelReceive,
        //   Select, Timeout, ResourceUsing, Lambda, BoundaryCall,
        //   DynCall, CapabilityUse, ...
        //
        // Add an explicit arm in this function before the parser exposes any
        // of the unsafe variants in a local expression position.
        _ => lower_core_expr_to_anf(expr, fresh, source_ref, &mut Vec::new()),
    }
}

// ── lower_core_expr_to_anf ────────────────────────────────────────────────

/// Recursively lower a `CoreExpr` to an `AnfExpr`.
///
/// Non-atomic sub-expressions (nested calls, non-trivial conditions, etc.)
/// are atomized: a synthetic `AnfBinding` is pushed to `out` and the
/// sub-expression is replaced by a `Var` reference to that binding.
///
/// All synthetic bindings carry `source_ref` for end-to-end provenance.
pub fn lower_core_expr_to_anf(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
    out: &mut Vec<AnfBinding>,
) -> AnfExpr {
    match expr {
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

        // ── G20 R2: new semantic variants ────────────────────────────────

        // And: short-circuit — lower left; result is Var(left_name); right
        // is wrapped in the ANF ShortCircuitAnd so it is only evaluated when
        // the condition demands it.
        CoreExpr::And { left, right } => {
            let left_name = atomize(left, fresh, source_ref, out);
            let anf_right = lower_core_expr_to_anf(right, fresh, source_ref, out);
            AnfExpr::ShortCircuitAnd {
                left: left_name,
                right: Box::new(anf_right),
            }
        }

        // Or: symmetric short-circuit — left is atomized; right is wrapped.
        CoreExpr::Or { left, right } => {
            let left_name = atomize(left, fresh, source_ref, out);
            let anf_right = lower_core_expr_to_anf(right, fresh, source_ref, out);
            AnfExpr::ShortCircuitOr {
                left: left_name,
                right: Box::new(anf_right),
            }
        }

        // EffectCall: atomize all args; effect ordering is structural.
        CoreExpr::EffectCall {
            capability,
            func,
            args,
        } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::EffectCall {
                capability: capability.clone(),
                func: func.clone(),
                args: atomic_args,
            }
        }

        // Dispatch: dynamic handler dispatch — atomize all args.
        CoreExpr::Dispatch {
            handler,
            method,
            args,
        } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Dispatch {
                handler: handler.clone(),
                method: method.clone(),
                args: atomic_args,
            }
        }

        // TaskSpawn: atomize all args; explicit ordering via ANF let-chain.
        CoreExpr::TaskSpawn { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::TaskSpawn {
                func: func.clone(),
                args: atomic_args,
            }
        }

        // ChannelSend: both channel and value must be atomic.
        CoreExpr::ChannelSend { channel, value } => {
            let channel_name = atomize(channel, fresh, source_ref, out);
            let value_name = atomize(value, fresh, source_ref, out);
            AnfExpr::ChannelSend {
                channel: channel_name,
                value: value_name,
            }
        }

        // ChannelReceive: channel must be atomic.
        CoreExpr::ChannelReceive { channel } => {
            let channel_name = atomize(channel, fresh, source_ref, out);
            AnfExpr::ChannelReceive {
                channel: channel_name,
            }
        }

        // RuntimeCheck: condition must be atomic; check_ref and msg are preserved.
        CoreExpr::RuntimeCheck {
            check_ref,
            cond,
            msg,
        } => {
            let cond_name = atomize(cond, fresh, source_ref, out);
            AnfExpr::RuntimeCheck {
                check_ref: check_ref.clone(),
                cond: cond_name,
                msg: msg.clone(),
            }
        }

        // ResourceAcquire: atomize all args; acquisition ordering is structural.
        CoreExpr::ResourceAcquire { resource, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::ResourceAcquire {
                resource: resource.clone(),
                args: atomic_args,
            }
        }

        // ResourceRelease: handle must be atomic.
        CoreExpr::ResourceRelease { handle } => {
            let handle_name = atomize(handle, fresh, source_ref, out);
            AnfExpr::ResourceRelease {
                handle: handle_name,
            }
        }

        // ── G23: new concurrency and cell primitives ─────────────────────

        // TaskAwait: task handle must be atomic.
        CoreExpr::TaskAwait { task } => {
            let task_name = atomize(task, fresh, source_ref, out);
            AnfExpr::TaskAwait { task: task_name }
        }

        // TaskCancel: task handle must be atomic.
        CoreExpr::TaskCancel { task } => {
            let task_name = atomize(task, fresh, source_ref, out);
            AnfExpr::TaskCancel { task: task_name }
        }

        // TaskGroup: body is lowered recursively (may contain TaskSpawn calls).
        CoreExpr::TaskGroup { body } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::TaskGroup {
                body: Box::new(anf_body),
            }
        }

        // ChannelNew: capacity is a primitive scalar — no sub-expression to lower.
        CoreExpr::ChannelNew { capacity } => AnfExpr::ChannelNew {
            capacity: *capacity,
        },

        // Select: each branch channel must be atomic; body is lowered recursively.
        CoreExpr::Select { branches } => {
            let anf_branches = branches
                .iter()
                .map(|clause| {
                    let channel_name = atomize(&clause.channel, fresh, source_ref, out);
                    let anf_body = lower_core_expr_to_anf(&clause.body, fresh, source_ref, out);
                    crate::anf::AnfSelectClause {
                        channel: channel_name,
                        binding: clause.binding.clone(),
                        body: anf_body,
                    }
                })
                .collect();
            AnfExpr::Select {
                branches: anf_branches,
            }
        }

        // Timeout: duration must be atomic; body is lowered recursively.
        CoreExpr::Timeout { duration, body } => {
            let duration_name = atomize(duration, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Timeout {
                duration: duration_name,
                body: Box::new(anf_body),
            }
        }

        // CellNew: init is atomized (must be atomic in ANF).
        CoreExpr::CellNew { init } => {
            let init_name = atomize(init, fresh, source_ref, out);
            AnfExpr::CellNew { init: init_name }
        }

        // CellGet: cell must be atomic.
        CoreExpr::CellGet { cell } => {
            let cell_name = atomize(cell, fresh, source_ref, out);
            AnfExpr::CellGet { cell: cell_name }
        }

        // CellSet: both cell and value must be atomic.
        CoreExpr::CellSet { cell, value } => {
            let cell_name = atomize(cell, fresh, source_ref, out);
            let value_name = atomize(value, fresh, source_ref, out);
            AnfExpr::CellSet {
                cell: cell_name,
                value: value_name,
            }
        }

        // CoreExpr::Placeholder → AnfExpr::Placeholder (no expression body).
        CoreExpr::Placeholder => AnfExpr::Placeholder,

        // ── ola5-compiler-core: Gap 2 — real ANF lowering ────────────────

        // Return: atomize the value, wrap in AnfExpr::Return.
        CoreExpr::Return { value } => {
            let val_name = atomize(value, fresh, source_ref, out);
            AnfExpr::Return(Box::new(AnfExpr::Var(val_name)))
        }

        // Assume: no runtime effect — produces unit.  Predicate and reason
        // are preserved for static analysis / documentation purposes.
        CoreExpr::Assume { predicate, reason } => AnfExpr::Assume {
            predicate: predicate.clone(),
            reason: reason.clone(),
        },

        // Abort: always traps — diagnostic message preserved.
        CoreExpr::Abort { message } => AnfExpr::Abort {
            message: message.clone(),
        },

        // BoundaryCall: atomize all args; encode boundary + func as a
        // namespaced call so backends can route to the trust boundary.
        CoreExpr::BoundaryCall {
            boundary,
            func,
            args,
        } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: format!("{boundary}::{func}"),
                args: atomic_args,
            }
        }

        // DynCall: atomize all args; encode interface + method as a
        // namespaced call for dynamic dispatch through Dyn<Interface>.
        CoreExpr::DynCall {
            interface,
            method,
            args,
        } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: format!("dyn::{interface}::{method}"),
                args: atomic_args,
            }
        }

        // ── doc-alignment: new CoreExpr variant lowering ─────────────────────

        // CapabilityUse: lower as an EffectCall-like call.
        CoreExpr::CapabilityUse { capability, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::EffectCall {
                capability: capability.clone(),
                func: capability.clone(),
                args: atomic_args,
            }
        }

        // ResourceUse: atomize handle, lower body.
        CoreExpr::ResourceUse { handle, body } => {
            let _h = atomize(handle, fresh, source_ref, out);
            lower_core_expr_to_anf(body, fresh, source_ref, out)
        }

        // ResourceUsing: acquire, bind, use, release — lowered as let + body.
        CoreExpr::ResourceUsing {
            resource,
            binding,
            body,
        } => {
            let res_name = atomize(resource, fresh, source_ref, out);
            // Emit acquire binding.
            out.push(AnfBinding {
                source_ref,
                name: binding.clone(),
                expr: AnfExpr::ResourceAcquire {
                    resource: res_name,
                    args: vec![],
                },
            });
            let result = lower_core_expr_to_anf(body, fresh, source_ref, out);
            // Emit implicit release after body.
            let release_tmp = format!("anf_{}", *fresh);
            *fresh += 1;
            out.push(AnfBinding {
                source_ref,
                name: release_tmp,
                expr: AnfExpr::ResourceRelease {
                    handle: binding.clone(),
                },
            });
            result
        }

        // ResourceTransfer: atomize both operands.
        CoreExpr::ResourceTransfer { handle, target } => {
            let h = atomize(handle, fresh, source_ref, out);
            let t = atomize(target, fresh, source_ref, out);
            AnfExpr::Call {
                func: "__resource_transfer".to_string(),
                args: vec![h, t],
            }
        }

        // ForeignFunctionCall: lower as a boundary-style call.
        CoreExpr::ForeignFunctionCall { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: format!("__foreign::{func}"),
                args: atomic_args,
            }
        }

        // PatchFieldConstruct: lower as a variant construction.
        CoreExpr::PatchFieldConstruct { state, value } => {
            let payload = value.as_ref().map(|v| {
                let name = atomize(v, fresh, source_ref, out);
                Box::new(AnfExpr::Var(name))
            });
            AnfExpr::VariantNew {
                tag: state.clone(),
                payload,
            }
        }

        // PatchFieldMatch: lower as a standard match.
        CoreExpr::PatchFieldMatch { scrutinee, arms } => {
            let scrutinee_name = atomize(scrutinee, fresh, source_ref, out);
            let anf_arms: Vec<crate::anf::AnfMatchArm> = arms
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

        // IndexGet: atomize both collection and index.
        CoreExpr::IndexGet { collection, index } => {
            let col_name = atomize(collection, fresh, source_ref, out);
            let idx_name = atomize(index, fresh, source_ref, out);
            AnfExpr::IndexGet {
                collection: col_name,
                index: idx_name,
            }
        }

        // MapNew: atomize all keys and values; preserve declaration order.
        CoreExpr::MapNew { entries } => {
            let atomic_entries: Vec<(String, String)> = entries
                .iter()
                .map(|(k, v)| {
                    let k_name = atomize(k, fresh, source_ref, out);
                    let v_name = atomize(v, fresh, source_ref, out);
                    (k_name, v_name)
                })
                .collect();
            AnfExpr::MapNew {
                entries: atomic_entries,
            }
        }

        // SetNew: atomize all elements; preserve declaration order.
        CoreExpr::SetNew { elements } => {
            let atomic_elems: Vec<String> = elements
                .iter()
                .map(|e| atomize(e, fresh, source_ref, out))
                .collect();
            AnfExpr::SetNew {
                elements: atomic_elems,
            }
        }

        // ForEach: atomize the collection; body is lowered recursively.
        CoreExpr::ForEach {
            binding,
            collection,
            body,
        } => {
            let col_name = atomize(collection, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::ForEach {
                binding: binding.clone(),
                collection: col_name,
                body: Box::new(anf_body),
            }
        }

        // Fold: atomize init, list, and func; all three become atomic names.
        CoreExpr::Fold { init, list, func } => {
            let init_name = atomize(init, fresh, source_ref, out);
            let list_name = atomize(list, fresh, source_ref, out);
            let func_name = atomize(func, fresh, source_ref, out);
            AnfExpr::Fold {
                init: init_name,
                list: list_name,
                func: func_name,
            }
        } // CoreExpr::Placeholder → AnfExpr::Placeholder (no expression body).
          // (kept last for clarity — already handled above)
    }
}
