// ── ail-compiler::lower::lower_expr ──────────────────────────────────────
//
// Expression lowering helpers: CoreExpr → AnfExpr.
//
// Responsibility split:
// - atomize: synthetic binding/atomic-name helpers.
// - local: nested local ANF lowering used by expression bodies.
// - global: public CoreExpr → ANF lowering entry point.

mod atomize;
mod global;
mod local;

pub(super) use atomize::{
    atomize, atomize_local, lower_core_binary_to_anf, lower_core_call_to_anf,
    lower_core_unary_to_anf, wrap_local_bindings,
};
pub use global::lower_core_expr_to_anf;
pub(super) use local::lower_core_expr_to_anf_local;
