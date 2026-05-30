// ── ail-compiler::lower::lower_expr::local ─────────────────────────────────

use ail_core::semantic_graph::NodeRef;

use crate::anf::AnfExpr;
use crate::core_ir::{CoreExpr, LiteralValue};

use super::{
    atomize_local, lower_core_binary_to_anf, lower_core_call_to_anf, lower_core_expr_to_anf,
    lower_core_unary_to_anf, wrap_local_bindings,
};

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
        CoreExpr::EffectCall {
            capability,
            func,
            args,
        } => {
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
                AnfExpr::EffectCall {
                    capability: capability.clone(),
                    func: func.clone(),
                    args: arg_names,
                },
            )
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
        // AnfExpr::WhileLoop is retained for direct ANF construction
        // (backward compatibility) where the caller intentionally names a
        // stable immutable flag or an immutable binding.  In that variant the emitter
        // issues a single `local.get` each iteration and does NOT re-evaluate
        // a computed expression — see the `AnfExpr::WhileLoop` doc comment for
        // the stale-condition limitation.  Only the CoreExpr → ANF lowering
        // path is changed here: all source-level while loops now desugar to
        // Loop+If so computed conditions are re-evaluated inside the loop body.
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
