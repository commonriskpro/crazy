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

        _ => return None,
    };
    Some(result)
}
