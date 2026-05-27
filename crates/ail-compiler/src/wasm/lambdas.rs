use std::collections::HashSet;

use crate::anf::{AnfBinding, AnfExpr};

// ── collect_hoistable_lambdas ─────────────────────────────────────────────

/// Collect all nested Lambda sub-expressions that qualify for hoisting.
///
/// A Lambda is hoistable when it has exactly 2 parameters and no captures
/// (fold-reducer shape `(i64, i64) → i64`).  These are the only Lambdas
/// whose bodies can be safely emitted as standalone WASM functions and
/// referenced by `call_indirect` in a Fold loop.
///
/// The order of collection matches the DFS traversal order in
/// `emit_anf_expr`, so the sequential index assigned here and the
/// `next_hoisted_table_idx` counter advanced during emission are always
/// consistent.
///
/// Top-level Lambda bindings are not collected — `build_code_section` emits
/// their bodies directly as regular binding functions.  Only Lambdas that
/// appear *inside* a binding's expression are collected.
pub(crate) fn collect_hoistable_lambdas(bindings: &[AnfBinding]) -> Vec<(Vec<String>, AnfExpr)> {
    let mut out = Vec::new();
    for binding in bindings {
        // Mirror the body-selection logic in `build_code_section`.
        let body_to_scan = match &binding.expr {
            AnfExpr::Lambda { body, .. } => body.as_ref(),
            other => other,
        };
        collect_in_expr(body_to_scan, &mut out);
    }
    out
}

/// DFS helper for `collect_hoistable_lambdas`.
///
/// Traverses `expr` in the same order as `emit_anf_expr` and appends
/// hoistable Lambdas to `out`.  The traversal does NOT recurse into
/// Lambda bodies — those bodies become separate functions and are not
/// visited inline during binding emission.
fn collect_in_expr(expr: &AnfExpr, out: &mut Vec<(Vec<String>, AnfExpr)>) {
    match expr {
        AnfExpr::Lambda {
            params,
            captures,
            body,
        } if params.len() == 2 && captures.is_empty() => {
            out.push((params.clone(), *body.clone()));
            // Do NOT recurse into body: it will be emitted as a separate
            // standalone function, not visited inline.
        }
        AnfExpr::Lambda { .. } => {
            // Non-hoistable or closure-hoistable Lambda — do not recurse.
        }
        AnfExpr::Let { value, body, .. } => {
            collect_in_expr(value, out);
            collect_in_expr(body, out);
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_in_expr(then_branch, out);
            collect_in_expr(else_branch, out);
        }
        AnfExpr::Return(inner) => collect_in_expr(inner, out),
        AnfExpr::Seq(exprs) => exprs.iter().for_each(|e| collect_in_expr(e, out)),
        AnfExpr::Match { arms, .. } => {
            arms.iter().for_each(|a| collect_in_expr(&a.body, out));
        }
        AnfExpr::Loop { body } => collect_in_expr(body, out),
        AnfExpr::Break { value } => collect_in_expr(value, out),
        AnfExpr::WhileLoop { body, .. } => collect_in_expr(body, out),
        AnfExpr::ForEach { body, .. } => collect_in_expr(body, out),
        AnfExpr::RecordNew { fields } => {
            fields.iter().for_each(|(_, v)| collect_in_expr(v, out));
        }
        AnfExpr::FieldUpdate { value, .. } => collect_in_expr(value, out),
        AnfExpr::TupleNew(elems) | AnfExpr::ListNew(elems) => {
            elems.iter().for_each(|e| collect_in_expr(e, out));
        }
        AnfExpr::VariantNew {
            payload: Some(p), ..
        } => {
            collect_in_expr(p, out);
        }
        AnfExpr::VariantNew { payload: None, .. } => {}
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            collect_in_expr(right, out);
        }
        // Atomic or non-recursive variants — no nested Lambdas.
        _ => {}
    }
}

// ── collect_closure_hoistable_lambdas ─────────────────────────────────────

/// Collect all nested Lambda sub-expressions that qualify for closure hoisting
/// (Wave 16A PR3).
///
/// A Lambda is closure-hoistable when it has exactly 2 parameters and at least
/// one capture.  Its body is emitted as a 3-param WASM function
/// `(env_ptr: i64, acc: i64, elem: i64) → i64` that loads captures from the
/// env pointer before executing the Lambda body.  The Lambda node itself writes
/// the real table index into the closure env's `fn_idx` slot, enabling Fold to
/// dispatch via `call_indirect` with the closure-reducer type.
///
/// The collection order matches the DFS traversal order in `emit_anf_expr`,
/// ensuring that the sequential `next_closure_hoisted_table_idx` counter
/// advanced during emission is consistent with the body indices assigned here.
pub(crate) fn collect_closure_hoistable_lambdas(
    bindings: &[AnfBinding],
) -> Vec<(Vec<String>, Vec<String>, AnfExpr)> {
    let mut out = Vec::new();
    for binding in bindings {
        let body_to_scan = match &binding.expr {
            AnfExpr::Lambda { body, .. } => body.as_ref(),
            other => other,
        };
        collect_closure_in_expr(body_to_scan, &mut out);
    }
    out
}

/// DFS helper for `collect_closure_hoistable_lambdas`.
///
/// Follows the same traversal order as `collect_in_expr` and `emit_anf_expr`.
/// Does NOT recurse into any Lambda body — those become separate functions.
fn collect_closure_in_expr(expr: &AnfExpr, out: &mut Vec<(Vec<String>, Vec<String>, AnfExpr)>) {
    match expr {
        AnfExpr::Lambda {
            params,
            captures,
            body,
        } if params.len() == 2 && !captures.is_empty() => {
            out.push((params.clone(), captures.clone(), *body.clone()));
            // Do NOT recurse: body becomes a separate standalone function.
        }
        AnfExpr::Lambda { .. } => {
            // Hoistable (capture-free) or non-2-param Lambda — skip body.
        }
        AnfExpr::Let { value, body, .. } => {
            collect_closure_in_expr(value, out);
            collect_closure_in_expr(body, out);
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_closure_in_expr(then_branch, out);
            collect_closure_in_expr(else_branch, out);
        }
        AnfExpr::Return(inner) => collect_closure_in_expr(inner, out),
        AnfExpr::Seq(exprs) => exprs.iter().for_each(|e| collect_closure_in_expr(e, out)),
        AnfExpr::Match { arms, .. } => {
            arms.iter()
                .for_each(|a| collect_closure_in_expr(&a.body, out));
        }
        AnfExpr::Loop { body } => collect_closure_in_expr(body, out),
        AnfExpr::Break { value } => collect_closure_in_expr(value, out),
        AnfExpr::WhileLoop { body, .. } => collect_closure_in_expr(body, out),
        AnfExpr::ForEach { body, .. } => collect_closure_in_expr(body, out),
        AnfExpr::RecordNew { fields } => {
            fields
                .iter()
                .for_each(|(_, v)| collect_closure_in_expr(v, out));
        }
        AnfExpr::FieldUpdate { value, .. } => collect_closure_in_expr(value, out),
        AnfExpr::TupleNew(elems) | AnfExpr::ListNew(elems) => {
            elems.iter().for_each(|e| collect_closure_in_expr(e, out));
        }
        AnfExpr::VariantNew {
            payload: Some(p), ..
        } => {
            collect_closure_in_expr(p, out);
        }
        AnfExpr::VariantNew { payload: None, .. } => {}
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            collect_closure_in_expr(right, out);
        }
        _ => {}
    }
}

// ── expr_contains_2param_lambda ───────────────────────────────────────────

/// Returns `true` when `expr` contains — directly or through recursive
/// sub-expressions — any `Lambda` node with exactly 2 parameters.
///
/// Used by the nested-hoistable-Lambda gate in `emit_wasm_with_profile`:
/// both `collect_hoistable_lambdas` and `collect_closure_hoistable_lambdas`
/// do NOT recurse into Lambda bodies.  A 2-param Lambda nested inside a
/// hoisted or closure-hoisted Lambda body would consume a table index that
/// was never allocated, silently writing an out-of-range index into linear
/// memory (not caught by `wasmparser::validate`).
///
/// The gate rejects such programs at compile time with
/// `CompileError::UnsupportedWasmConstruct` until recursive
/// collection/indexing is implemented.
pub(super) fn expr_contains_2param_lambda(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Lambda { params, body, .. } => {
            // If this Lambda itself has 2 params it IS a hoistable/closure-hoistable
            // nested Lambda — the problem case.  Otherwise recurse into its body.
            params.len() == 2 || expr_contains_2param_lambda(body)
        }
        AnfExpr::Let { value, body, .. } => {
            expr_contains_2param_lambda(value) || expr_contains_2param_lambda(body)
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => expr_contains_2param_lambda(then_branch) || expr_contains_2param_lambda(else_branch),
        AnfExpr::Return(inner) => expr_contains_2param_lambda(inner),
        AnfExpr::Seq(exprs) => exprs.iter().any(expr_contains_2param_lambda),
        AnfExpr::Match { arms, .. } => arms.iter().any(|a| expr_contains_2param_lambda(&a.body)),
        AnfExpr::Loop { body } => expr_contains_2param_lambda(body),
        AnfExpr::Break { value } => expr_contains_2param_lambda(value),
        AnfExpr::WhileLoop { body, .. } | AnfExpr::ForEach { body, .. } => {
            expr_contains_2param_lambda(body)
        }
        AnfExpr::RecordNew { fields } => fields.iter().any(|(_, v)| expr_contains_2param_lambda(v)),
        AnfExpr::FieldUpdate { value, .. } => expr_contains_2param_lambda(value),
        AnfExpr::TupleNew(elems) | AnfExpr::ListNew(elems) => {
            elems.iter().any(expr_contains_2param_lambda)
        }
        AnfExpr::VariantNew {
            payload: Some(p), ..
        } => expr_contains_2param_lambda(p),
        AnfExpr::VariantNew { payload: None, .. } => false,
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            expr_contains_2param_lambda(right)
        }
        // Atomic or non-recursive variants — no nested Lambdas.
        _ => false,
    }
}

// ── has_fold_with_captured_reducer ───────────────────────────────────────

/// Returns `true` when any `Fold` in `bindings` has a `func` that is
/// let-bound to a `Lambda` with non-empty captures AND **not** exactly 2
/// parameters.
///
/// Wave 16A PR3 implements general closure hoisting for 2-param captured
/// Lambdas: they are emitted as `(env_ptr: i64, acc: i64, elem: i64) → i64`
/// WASM functions and the closure env receives a real table index.  These no
/// longer need this diagnostic gate.
///
/// Lambdas with captures and **≠ 2 params** cannot be Fold reducers (Fold
/// expects `(i64, i64) → i64`).  Using them as such would write `fn_idx = 0`
/// (placeholder) into the closure env, causing a runtime type-mismatch trap.
/// This gate preserves the compile-time diagnostic for those non-reducible
/// shapes.
///
/// Top-level Lambda bindings are not checked here — they are always emitted as
/// proper WASM functions with captures as explicit I64 parameters.
pub(super) fn has_fold_with_captured_reducer(bindings: &[AnfBinding]) -> bool {
    for binding in bindings {
        let body_to_scan = match &binding.expr {
            AnfExpr::Lambda { body, .. } => body.as_ref(),
            other => other,
        };
        let mut captured_names: HashSet<&str> = HashSet::new();
        if expr_has_fold_with_captured_reducer(body_to_scan, &mut captured_names) {
            return true;
        }
    }
    false
}

/// DFS helper for `has_fold_with_captured_reducer`.
///
/// Tracks let-bound names whose values are Lambdas with non-empty captures AND
/// **≠ 2 params** (`captured_names`).  Returns `true` when a `Fold` node is
/// found whose `func` is in that set.
///
/// 2-param captured Lambdas are excluded because they are now supported via
/// closure hoisting (Wave 16A PR3) and no longer need the diagnostic.
fn expr_has_fold_with_captured_reducer<'a>(
    expr: &'a AnfExpr,
    captured_names: &mut HashSet<&'a str>,
) -> bool {
    match expr {
        AnfExpr::Let { name, value, body } => {
            if let AnfExpr::Lambda {
                captures, params, ..
            } = value.as_ref()
                && !captures.is_empty()
                && params.len() != 2
            {
                // Only flag non-2-param captured Lambdas: 2-param captured
                // Lambdas are now supported via closure hoisting (Wave 16A PR3).
                captured_names.insert(name.as_str());
            } else if let AnfExpr::Var(v) = value.as_ref()
                && captured_names.contains(v.as_str())
            {
                // Transitive alias: propagate captured-name membership.
                captured_names.insert(name.as_str());
            }
            expr_has_fold_with_captured_reducer(value, captured_names)
                || expr_has_fold_with_captured_reducer(body, captured_names)
        }
        AnfExpr::Fold { func, .. } => captured_names.contains(func.as_str()),
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            // Clone before each branch so names introduced in one branch cannot
            // leak into the sibling branch and cause false positives.
            let mut then_names = captured_names.clone();
            let mut else_names = captured_names.clone();
            expr_has_fold_with_captured_reducer(then_branch, &mut then_names)
                || expr_has_fold_with_captured_reducer(else_branch, &mut else_names)
        }
        AnfExpr::Return(inner) => expr_has_fold_with_captured_reducer(inner, captured_names),
        AnfExpr::Seq(exprs) => exprs
            .iter()
            .any(|e| expr_has_fold_with_captured_reducer(e, captured_names)),
        AnfExpr::Match { arms, .. } => arms.iter().any(|a| {
            // Clone per arm: names from one arm must not contaminate sibling arms.
            let mut arm_names = captured_names.clone();
            expr_has_fold_with_captured_reducer(&a.body, &mut arm_names)
        }),
        AnfExpr::Lambda { body, .. } => expr_has_fold_with_captured_reducer(body, captured_names),
        AnfExpr::Loop { body }
        | AnfExpr::WhileLoop { body, .. }
        | AnfExpr::ForEach { body, .. } => {
            expr_has_fold_with_captured_reducer(body, captured_names)
        }
        AnfExpr::Break { value } => expr_has_fold_with_captured_reducer(value, captured_names),
        AnfExpr::RecordNew { fields } => fields
            .iter()
            .any(|(_, v)| expr_has_fold_with_captured_reducer(v, captured_names)),
        AnfExpr::FieldUpdate { value, .. } => {
            expr_has_fold_with_captured_reducer(value, captured_names)
        }
        AnfExpr::TupleNew(elems) | AnfExpr::ListNew(elems) => elems
            .iter()
            .any(|e| expr_has_fold_with_captured_reducer(e, captured_names)),
        AnfExpr::VariantNew { payload, .. } => payload
            .as_deref()
            .is_some_and(|p| expr_has_fold_with_captured_reducer(p, captured_names)),
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            expr_has_fold_with_captured_reducer(right, captured_names)
        }
        _ => false,
    }
}

// ── has_fold_with_uncaptured_wrong_arity_reducer ──────────────────────────

/// Returns `true` when any `Fold` in `bindings` has a `func` that is
/// let-bound to a `Lambda` with **empty captures** and **≠ 2 params**.
///
/// Such Lambdas fall into the non-hoistable `else` branch in `emit_anf_expr`:
/// they emit a closure env with `fn_idx = 0` (placeholder).  When a `Fold`
/// uses the resulting I32 pointer as its reducer, the Fold I32 dispatch path
/// reads `fn_idx = 0` and dispatches `call_indirect(closure-reducer type)`
/// to `table[0]` — a silent wrong-function dispatch or a runtime type-mismatch
/// trap rather than a compile-time diagnostic.
///
/// This gate returns `CompileError::UnsupportedWasmConstruct` before code
/// generation, ensuring callers receive a deterministic, structured error
/// instead of silent bad runtime behaviour.
///
/// Note: captures-non-empty + params ≠ 2 is handled by
/// `has_fold_with_captured_reducer` and returns `FoldWithCapturedReducer`.
/// This function covers the complementary case: no captures, wrong arity.
pub(super) fn has_fold_with_uncaptured_wrong_arity_reducer(bindings: &[AnfBinding]) -> bool {
    for binding in bindings {
        let body_to_scan = match &binding.expr {
            AnfExpr::Lambda { body, .. } => body.as_ref(),
            other => other,
        };
        let mut names: HashSet<&str> = HashSet::new();
        if expr_has_fold_with_uncaptured_wrong_arity(body_to_scan, &mut names) {
            return true;
        }
    }
    false
}

/// DFS helper for `has_fold_with_uncaptured_wrong_arity_reducer`.
///
/// Tracks let-bound names whose values are Lambdas with **empty captures** and
/// **≠ 2 params**.  Returns `true` when a `Fold` node is found whose `func`
/// is in that set.
fn expr_has_fold_with_uncaptured_wrong_arity<'a>(
    expr: &'a AnfExpr,
    names: &mut HashSet<&'a str>,
) -> bool {
    match expr {
        AnfExpr::Let { name, value, body } => {
            if let AnfExpr::Lambda {
                captures, params, ..
            } = value.as_ref()
                && captures.is_empty()
                && params.len() != 2
            {
                // Capture-free, wrong-arity Lambda: non-hoistable, cannot be a
                // valid Fold reducer — would emit fn_idx=0 placeholder.
                names.insert(name.as_str());
            } else if let AnfExpr::Var(v) = value.as_ref()
                && names.contains(v.as_str())
            {
                // Transitive alias: propagate membership.
                names.insert(name.as_str());
            }
            expr_has_fold_with_uncaptured_wrong_arity(value, names)
                || expr_has_fold_with_uncaptured_wrong_arity(body, names)
        }
        AnfExpr::Fold { func, .. } => names.contains(func.as_str()),
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            // Clone before each branch so names introduced in one branch cannot
            // leak into the sibling branch and cause false positives.
            let mut then_names = names.clone();
            let mut else_names = names.clone();
            expr_has_fold_with_uncaptured_wrong_arity(then_branch, &mut then_names)
                || expr_has_fold_with_uncaptured_wrong_arity(else_branch, &mut else_names)
        }
        AnfExpr::Return(inner) => expr_has_fold_with_uncaptured_wrong_arity(inner, names),
        AnfExpr::Seq(exprs) => exprs
            .iter()
            .any(|e| expr_has_fold_with_uncaptured_wrong_arity(e, names)),
        AnfExpr::Match { arms, .. } => arms.iter().any(|a| {
            // Clone per arm: names from one arm must not contaminate sibling arms.
            let mut arm_names = names.clone();
            expr_has_fold_with_uncaptured_wrong_arity(&a.body, &mut arm_names)
        }),
        AnfExpr::Lambda { body, .. } => expr_has_fold_with_uncaptured_wrong_arity(body, names),
        AnfExpr::Loop { body }
        | AnfExpr::WhileLoop { body, .. }
        | AnfExpr::ForEach { body, .. } => expr_has_fold_with_uncaptured_wrong_arity(body, names),
        AnfExpr::Break { value } => expr_has_fold_with_uncaptured_wrong_arity(value, names),
        AnfExpr::RecordNew { fields } => fields
            .iter()
            .any(|(_, v)| expr_has_fold_with_uncaptured_wrong_arity(v, names)),
        AnfExpr::FieldUpdate { value, .. } => {
            expr_has_fold_with_uncaptured_wrong_arity(value, names)
        }
        AnfExpr::TupleNew(elems) | AnfExpr::ListNew(elems) => elems
            .iter()
            .any(|e| expr_has_fold_with_uncaptured_wrong_arity(e, names)),
        AnfExpr::VariantNew { payload, .. } => payload
            .as_deref()
            .is_some_and(|p| expr_has_fold_with_uncaptured_wrong_arity(p, names)),
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            expr_has_fold_with_uncaptured_wrong_arity(right, names)
        }
        _ => false,
    }
}

// ── anf_has_fold ──────────────────────────────────────────────────────────

/// Returns `true` if any sub-expression in `expr` is `AnfExpr::Fold`.
///
/// Used by `emit_wasm_with_profile` to decide whether to add the function
/// table, element section, and fold-reducer type to the WASM module.
pub(super) fn anf_has_fold(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Fold { .. } => true,
        AnfExpr::Let { value, body, .. } => anf_has_fold(value) || anf_has_fold(body),
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => anf_has_fold(then_branch) || anf_has_fold(else_branch),
        AnfExpr::Return(inner) => anf_has_fold(inner),
        AnfExpr::Seq(exprs) => exprs.iter().any(anf_has_fold),
        AnfExpr::Match { arms, .. } => arms.iter().any(|a| anf_has_fold(&a.body)),
        AnfExpr::Lambda { body, .. } => anf_has_fold(body),
        AnfExpr::Loop { body } | AnfExpr::TaskGroup { body } => anf_has_fold(body),
        AnfExpr::Timeout { body, .. } => anf_has_fold(body),
        AnfExpr::Break { value } => anf_has_fold(value),
        AnfExpr::WhileLoop { body, .. } | AnfExpr::ForEach { body, .. } => anf_has_fold(body),
        AnfExpr::RecordNew { fields } => fields.iter().any(|(_, v)| anf_has_fold(v)),
        AnfExpr::FieldUpdate { value, .. } => anf_has_fold(value),
        AnfExpr::TupleNew(elems) | AnfExpr::ListNew(elems) => elems.iter().any(anf_has_fold),
        AnfExpr::VariantNew { payload, .. } => payload.as_deref().is_some_and(anf_has_fold),
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            anf_has_fold(right)
        }
        // Atomic or unimplemented variants — no Fold sub-expression.
        _ => false,
    }
}

// ── first_unsupported_wasm_construct ──────────────────────────────────────

/// Returns the name of the first WASM-unsupported construct found in `expr`,
/// or `None` if every sub-expression is supported by the current backend.
///
/// Used by the WASM pre-flight gate in `emit_wasm_with_profile`: detecting
/// unsupported constructs before code generation lets us return a structured
/// `CompileError::UnsupportedWasmConstruct` instead of emitting silent
/// `unreachable` traps at runtime.
///
/// **Unsupported constructs (compile-time diagnostic gate):**
/// - `"Dispatch"` — dynamic dispatch requires `call_indirect` + vtable
/// - `"TaskSpawn"`, `"TaskAwait"`, `"TaskCancel"`, `"TaskGroup"` — require async runtime
/// - `"ChannelNew"`, `"ChannelSend"`, `"ChannelReceive"`, `"Select"`, `"Timeout"` — require channel runtime
///
/// Note: `Fold` is now implemented via `call_indirect` + function table and is
/// NOT listed here.
///
/// All other variants are either implemented or are atomic (no sub-expressions).
pub(super) fn first_unsupported_wasm_construct(expr: &AnfExpr) -> Option<&'static str> {
    match expr {
        // ── Unsupported constructs — return diagnostic name immediately ───
        AnfExpr::Dispatch { .. } => Some("Dispatch"),
        AnfExpr::TaskSpawn { .. } => Some("TaskSpawn"),
        AnfExpr::TaskAwait { .. } => Some("TaskAwait"),
        AnfExpr::TaskCancel { .. } => Some("TaskCancel"),
        AnfExpr::TaskGroup { .. } => Some("TaskGroup"),
        AnfExpr::ChannelNew { .. } => Some("ChannelNew"),
        AnfExpr::ChannelSend { .. } => Some("ChannelSend"),
        AnfExpr::ChannelReceive { .. } => Some("ChannelReceive"),
        AnfExpr::Select { .. } => Some("Select"),
        AnfExpr::Timeout { .. } => Some("Timeout"),

        // ── Recursive variants — walk all sub-expressions ─────────────────
        AnfExpr::Let { value, body, .. } => first_unsupported_wasm_construct(value)
            .or_else(|| first_unsupported_wasm_construct(body)),
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => first_unsupported_wasm_construct(then_branch)
            .or_else(|| first_unsupported_wasm_construct(else_branch)),
        AnfExpr::Return(inner) => first_unsupported_wasm_construct(inner),
        AnfExpr::Seq(exprs) => exprs.iter().find_map(first_unsupported_wasm_construct),
        AnfExpr::Match { arms, .. } => arms
            .iter()
            .find_map(|a| first_unsupported_wasm_construct(&a.body)),
        AnfExpr::Lambda { body, .. } => first_unsupported_wasm_construct(body),
        AnfExpr::RecordNew { fields } => fields
            .iter()
            .find_map(|(_, v)| first_unsupported_wasm_construct(v)),
        AnfExpr::FieldUpdate { value, .. } => first_unsupported_wasm_construct(value),
        AnfExpr::TupleNew(elems) => elems.iter().find_map(first_unsupported_wasm_construct),
        AnfExpr::VariantNew { payload, .. } => payload
            .as_deref()
            .and_then(first_unsupported_wasm_construct),
        AnfExpr::ListNew(elems) => elems.iter().find_map(first_unsupported_wasm_construct),
        AnfExpr::Loop { body } => first_unsupported_wasm_construct(body),
        AnfExpr::Break { value } => first_unsupported_wasm_construct(value),
        AnfExpr::WhileLoop { body, .. } => first_unsupported_wasm_construct(body),
        AnfExpr::ShortCircuitAnd { right, .. } => first_unsupported_wasm_construct(right),
        AnfExpr::ShortCircuitOr { right, .. } => first_unsupported_wasm_construct(right),
        AnfExpr::ForEach { body, .. } => first_unsupported_wasm_construct(body),

        // ── Atomic or implemented variants — no sub-expressions to inspect ──
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::Call { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::EffectCall { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::Continue
        | AnfExpr::Placeholder
        // Fold is now implemented via call_indirect + function table.
        | AnfExpr::Fold { .. } => None,
    }
}
