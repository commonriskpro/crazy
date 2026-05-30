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

        _ => return None,
    };
    Some(result)
}
