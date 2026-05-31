use crate::anf::AnfExpr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PurityBlockReason {
    pub shape: &'static str,
    pub reason: &'static str,
}

pub(crate) fn is_pure(expr: &AnfExpr) -> bool {
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

pub(crate) fn purity_blocking_reason(expr: &AnfExpr) -> Option<PurityBlockReason> {
    match expr {
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::Call { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::RecordNew { .. }
        | AnfExpr::TupleNew(_)
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::Lambda { .. } => None,
        AnfExpr::Let { value, body, .. } => {
            purity_blocking_reason(value).or_else(|| purity_blocking_reason(body))
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => purity_blocking_reason(then_branch).or_else(|| purity_blocking_reason(else_branch)),
        AnfExpr::Seq(exprs) => exprs.iter().find_map(purity_blocking_reason),
        AnfExpr::Match { arms, .. } => arms
            .iter()
            .find_map(|arm| purity_blocking_reason(&arm.body)),
        AnfExpr::Return(_) => Some(PurityBlockReason {
            shape: "Return",
            reason: "alters control flow",
        }),
        AnfExpr::FieldUpdate { .. } => Some(PurityBlockReason {
            shape: "FieldUpdate",
            reason: "writes derived state",
        }),
        AnfExpr::Loop { .. } | AnfExpr::WhileLoop { .. } => Some(PurityBlockReason {
            shape: "Loop",
            reason: "may not terminate",
        }),
        AnfExpr::Break { .. } | AnfExpr::Continue => Some(PurityBlockReason {
            shape: "LoopControl",
            reason: "alters loop control flow",
        }),
        AnfExpr::ShortCircuitAnd { .. } | AnfExpr::ShortCircuitOr { .. } => {
            Some(PurityBlockReason {
                shape: "ShortCircuit",
                reason: "conditional evaluation order is observable",
            })
        }
        AnfExpr::EffectCall { .. } | AnfExpr::Dispatch { .. } => Some(PurityBlockReason {
            shape: "EffectCall",
            reason: "external effect",
        }),
        AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. } => Some(PurityBlockReason {
            shape: "Task",
            reason: "scheduler interaction",
        }),
        AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::Select { .. } => Some(PurityBlockReason {
            shape: "Channel",
            reason: "communication effect",
        }),
        AnfExpr::RuntimeCheck { .. }
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::Placeholder => Some(PurityBlockReason {
            shape: "RuntimeCheck",
            reason: "runtime validation or trap",
        }),
        AnfExpr::ResourceAcquire { .. } | AnfExpr::ResourceRelease { .. } => {
            Some(PurityBlockReason {
                shape: "Resource",
                reason: "resource lifetime effect",
            })
        }
        AnfExpr::Timeout { .. } => Some(PurityBlockReason {
            shape: "Timeout",
            reason: "time-dependent control flow",
        }),
        AnfExpr::CellNew { .. } | AnfExpr::CellGet { .. } | AnfExpr::CellSet { .. } => {
            Some(PurityBlockReason {
                shape: "Cell",
                reason: "mutable cell access",
            })
        }
        AnfExpr::IndexGet { .. } => Some(PurityBlockReason {
            shape: "IndexGet",
            reason: "bounds-sensitive access",
        }),
        AnfExpr::MapNew { .. } | AnfExpr::SetNew { .. } => Some(PurityBlockReason {
            shape: "CollectionNew",
            reason: "collection allocation semantics not modeled as pure",
        }),
        AnfExpr::ForEach { .. } | AnfExpr::Fold { .. } => Some(PurityBlockReason {
            shape: "Iterator",
            reason: "iterator evaluation semantics not modeled as pure",
        }),
    }
}

/// Count the total number of `AnfExpr` nodes in `expr` (recursive).
///
/// Atomic leaf nodes (`Literal`, `Var`, `Placeholder`, `Continue`, `Call`,
/// `FieldGet`, and other flat impure primitives) each count as 1.  Composite
/// nodes count as 1 plus the sum of their sub-expressions.
pub(crate) fn anf_node_count(expr: &AnfExpr) -> usize {
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
        AnfExpr::Let { value, body, .. } => 1 + anf_node_count(value) + anf_node_count(body),
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
            1 + arms
                .iter()
                .map(|arm| anf_node_count(&arm.body))
                .sum::<usize>()
        }
        AnfExpr::Select { branches } => {
            1 + branches
                .iter()
                .map(|b| anf_node_count(&b.body))
                .sum::<usize>()
        }
    }
}

pub(crate) fn uses_var(expr: &AnfExpr, name: &str) -> bool {
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
        | AnfExpr::TaskGroup { body: inner }
        | AnfExpr::Timeout { body: inner, .. } => uses_var(inner, name),
        AnfExpr::ShortCircuitAnd { left, right } => left == name || uses_var(right, name),
        AnfExpr::ShortCircuitOr { left, right } => left == name || uses_var(right, name),
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            exprs.iter().any(|expr| uses_var(expr, name))
        }
        AnfExpr::Match { scrutinee, arms } => {
            scrutinee == name || arms.iter().any(|arm| uses_var(&arm.body, name))
        }
        AnfExpr::Lambda { params, captures, body } => {
            // Check captures first (fast path), then fall back to body scan.
            captures.iter().any(|c| c == name)
                || (!params.iter().any(|param| param == name) && uses_var(body, name))
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
        | AnfExpr::Placeholder => false,
        // Fold references its init, list, and func atoms by name — all three
        // must be checked so the dead-let pass does not eliminate the
        // let-bindings that supply them.
        AnfExpr::Fold { init, list, func } => {
            init == name || list == name || func == name
        }
        AnfExpr::IndexGet { collection, index } => collection == name || index == name,
        AnfExpr::MapNew { entries } => entries.iter().any(|(k, v)| k == name || v == name),
        AnfExpr::SetNew { elements } => elements.iter().any(|e| e == name),
        AnfExpr::ForEach { collection, body, binding } => {
            // `binding` is the loop variable; it shadows the outer `name` when
            // they are equal.  If binding is empty (no loop variable) the outer
            // `name` is still freely accessible inside the body, so we must
            // not gate the body scan on `!binding.is_empty()`.
            collection == name || (binding != name && uses_var(body, name))
        }
    }
}
