// ── ail-compiler::lower::lower_expr ──────────────────────────────────────
//
// Expression lowering helpers: CoreExpr → AnfExpr.
//
// All functions in this module convert individual `CoreExpr` nodes to their
// ANF counterparts.  They are re-exported through `lower.rs` and form part of
// the public compiler API (`lower_core_expr_to_anf`).

use ail_core::semantic_graph::NodeRef;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::CoreExpr;

use super::{lower_core_expr_to_anf, lower_core_expr_to_anf_local};

// ── atomize ───────────────────────────────────────────────────────────────

/// Ensure `expr` is atomic (a variable name).
///
/// If `expr` is already `CoreExpr::Var(n)`, returns `n` without emitting any
/// binding.  Otherwise lowers `expr` to an `AnfExpr`, pushes a synthetic
/// `AnfBinding` with a fresh name, and returns that fresh name.
///
/// The pushed binding carries the same `source_ref` as the enclosing node
/// (provenance is preserved for synthetic temporaries).
pub(crate) fn atomize(
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

pub(crate) fn atomize_local(
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

pub(crate) fn wrap_local_bindings(mut bindings: Vec<(String, AnfExpr)>, body: AnfExpr) -> AnfExpr {
    bindings.reverse();
    bindings
        .into_iter()
        .fold(body, |body, (name, value)| AnfExpr::Let {
            name,
            value: Box::new(value),
            body: Box::new(body),
        })
}

pub(crate) fn lower_core_call_to_anf(
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

pub(crate) fn lower_core_binary_to_anf(
    func: &str,
    left: &CoreExpr,
    right: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
) -> AnfExpr {
    lower_core_call_to_anf(func, &[left.clone(), right.clone()], fresh, source_ref)
}

pub(crate) fn lower_core_unary_to_anf(
    func: &str,
    operand: &CoreExpr,
    fresh: &mut u32,
    source_ref: NodeRef,
) -> AnfExpr {
    lower_core_call_to_anf(func, std::slice::from_ref(operand), fresh, source_ref)
}
