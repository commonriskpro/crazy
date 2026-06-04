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
        }

        // CoreExpr::Placeholder → AnfExpr::Placeholder (no expression body).
        // (kept last for clarity — already handled above)
        _ => return None,
    };
    Some(result)
}
