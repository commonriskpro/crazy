// ── ail-compiler::lower::lower_expr::global ────────────────────────────────

use ail_core::semantic_graph::NodeRef;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::{CoreExpr, LiteralValue};

mod base;
mod concurrency;
mod effects;
mod extended;

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
    base::try_lower(expr, fresh, source_ref, out)
        .or_else(|| effects::try_lower(expr, fresh, source_ref, out))
        .or_else(|| concurrency::try_lower(expr, fresh, source_ref, out))
        .or_else(|| extended::try_lower(expr, fresh, source_ref, out))
        .unwrap_or_else(|| unreachable!("all CoreExpr variants must be handled by ANF lowering"))
}
