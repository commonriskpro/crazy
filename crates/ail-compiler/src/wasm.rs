// ── ail-compiler::wasm ────────────────────────────────────────────────────
//
// WASM emission — the third and final pipeline stage.
//
// # Pre-condition
//
// `emit_wasm` MUST be called with an `AnfIr` produced by `lower_to_anf`.
// The `anf_ir_hash` field in `stage_hashes` must be `Some(...)`.
// If it is `None`, `Err(CompileError::EncodingError)` is returned.
//
// # What is emitted (G20 R2)
//
// Every `AnfBinding` becomes a WASM function with a real body emitted from
// the ANF IR.  The codegen uses WASM locals to represent ANF let-bindings:
//   - `AnfExpr::Literal(Int(n))` → `i64.const n` + `local.set`
//   - `AnfExpr::Literal(Bool(b))` → `i64.const 0|1` + `local.set`
//   - `AnfExpr::Literal(Float(f))` → `f64.const f`
//   - `AnfExpr::Literal(Text)` → packed `(ptr, len)` i64 into linear memory
//   - `AnfExpr::Literal(Unit)` → `i32.const 0`
//   - `AnfExpr::Var(n)` → `local.get <index>`
//   - `AnfExpr::Call { func, args }` → `call <func_ref>` (host import)
//   - `AnfExpr::If` → `block/if/else/end`
//   - `AnfExpr::Let` → let-bind value, then emit body
//   - Effect variants with real emit (`EffectCall`, `ResourceAcquire`,
//     `ResourceRelease`, `RuntimeCheck`) → host-import calls / conditional trap
//   - Concurrency variants (Task*/Channel*/Select/Timeout) and `Dispatch` →
//     `CompileError::UnsupportedWasmConstruct` at compile time (pre-flight gate
//     in `emit_wasm_with_profile`); `unreachable` in `emit_anf_expr` as a
//     defence-in-depth fallback for direct callers.
//   - `AnfExpr::Placeholder` → `unreachable`
//
// An `AnfIr` with zero bindings produces a minimal valid WASM module
// (magic + version only — no sections).
//
// # Hash chain contract
//
// `wasm_hash = blake3(anf_ir_hash || wasm_binary)`
//
// # Determinism contract
//
// `BTreeMap` for provenance.  `stable_cbor_bytes` + BLAKE3 for hashing.
// Same `AnfIr` → byte-identical `WasmArtifact` across any number of calls.
//
// # What this stage does NOT do
//
// - No optimization.
// - No runtime / Wasmtime dependency.
// - Concurrency primitives (Task*/Channel*/Select/Timeout/Dispatch) are
//   rejected at compile time via `emit_wasm_with_profile`; they are NOT
//   silently trapped at runtime.
// - `Fold` with a captured Lambda reducer (`FoldWithCapturedReducer`) is
//   rejected at compile time (Wave 13B).  The captured values are not
//   representable as `(i64, i64) → i64` WASM function parameters, so the
//   backend cannot hoist the Lambda into the function table.
//
// # Module layout
//
// The implementation is split across focused sibling modules:
//   - `wasm_sections.rs` — pure section builders (type/func/export/import/…)
//   - `wasm_emit.rs`     — ANF→instruction emitter and code-section builder
//
// Tests live in `wasm_tests.rs`.

use std::collections::{BTreeMap, HashSet};

use ail_core::semantic_graph::NodeRef;
use wasm_encoder::Module;

use crate::anf::{AnfBinding, AnfExpr, AnfIr, SourceMap, SourceMapEntry};
use crate::artifact_manifest::ArtifactManifest;
use crate::capabilities::CapabilitiesManifest;
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
// Public re-exports: maintain the pre-existing surface of `ail_compiler::wasm`.
pub use crate::wasm_abi::{
    ABI_VERSION, AbiDescriptor, WasmScalarType, WasmTypeDescriptor, derive_wasm_type,
};
pub use crate::wasm_artifact::WasmArtifact;

use crate::wasm_abi::{EffectDataLayout, binding_result, binding_signatures, export_name};
use crate::wasm_artifact::code_entry_offsets;
use crate::wasm_emit::build_code_section;
use crate::wasm_sections::{
    align_to_i64, build_data_section, build_element_section, build_export_section_with_memory,
    build_function_section, build_global_section, build_import_section, build_memory_section,
    build_table_section, build_type_section, build_type_section_with_host_call,
};

#[cfg(test)]
#[path = "wasm_tests.rs"]
mod tests;

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
fn has_fold_with_captured_reducer(bindings: &[AnfBinding]) -> bool {
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
            expr_has_fold_with_captured_reducer(then_branch, captured_names)
                || expr_has_fold_with_captured_reducer(else_branch, captured_names)
        }
        AnfExpr::Return(inner) => expr_has_fold_with_captured_reducer(inner, captured_names),
        AnfExpr::Seq(exprs) => exprs
            .iter()
            .any(|e| expr_has_fold_with_captured_reducer(e, captured_names)),
        AnfExpr::Match { arms, .. } => arms
            .iter()
            .any(|a| expr_has_fold_with_captured_reducer(&a.body, captured_names)),
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

// ── anf_has_fold ──────────────────────────────────────────────────────────

/// Returns `true` if any sub-expression in `expr` is `AnfExpr::Fold`.
///
/// Used by `emit_wasm_with_profile` to decide whether to add the function
/// table, element section, and fold-reducer type to the WASM module.
fn anf_has_fold(expr: &AnfExpr) -> bool {
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
fn first_unsupported_wasm_construct(expr: &AnfExpr) -> Option<&'static str> {
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

// ── emit_wasm ─────────────────────────────────────────────────────────────

/// Emit a structurally valid WASM module from an `AnfIr`.
///
/// # Pre-conditions
///
/// - `anf.stage_hashes.anf_ir_hash` must be `Some(...)`.  Call
///   `lower_to_anf` before `emit_wasm`.
///
/// # Hash chain
///
/// Extends the chain: `wasm_hash = blake3(anf_ir_hash || wasm_binary)`.
///
/// # Errors
///
/// - `CompileError::EncodingError` — `anf_ir_hash` is `None` (pre-condition
///   violated) or WASM binary assembly failed.
pub fn emit_wasm(anf: &AnfIr) -> Result<WasmArtifact, CompileError> {
    emit_wasm_with_profile(anf, "unspecified")
}

/// Emit a WASM module and bind the artifact manifest to `profile`.
pub fn emit_wasm_with_profile(anf: &AnfIr, profile: &str) -> Result<WasmArtifact, CompileError> {
    // Gate: anf_ir_hash must be sealed.
    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .ok_or_else(|| CompileError::EncodingError("anf_ir_hash not sealed".to_string()))?;

    // Gate: unsupported WASM constructs.
    // Walk every binding's expression before code generation so callers receive
    // a structured CompileError::UnsupportedWasmConstruct instead of a silent
    // unreachable trap at runtime.
    //
    // Unsupported: Dispatch, TaskSpawn/Await/Cancel/Group (async runtime),
    // ChannelNew/Send/Receive/Select/Timeout (channel runtime).
    // Note: Fold is now implemented via call_indirect + function table.
    for binding in &anf.bindings {
        if let Some(name) = first_unsupported_wasm_construct(&binding.expr) {
            return Err(CompileError::UnsupportedWasmConstruct(name.to_string()));
        }
    }

    // Gate: Fold reducer that is a captured Lambda with ≠ 2 params (Wave 13B,
    // updated in Wave 16A PR3).
    //
    // 2-param captured Lambdas are now supported via closure hoisting (PR3):
    // they are emitted as `(env_ptr: i64, acc: i64, elem: i64) → i64` WASM
    // functions with the real table index stored in the closure env's fn_idx.
    //
    // Non-2-param captured Lambdas (0, 1, 3+ params) cannot be Fold reducers
    // and still produce a runtime trap.  The gate preserves the compile-time
    // diagnostic for those shapes.
    if has_fold_with_captured_reducer(&anf.bindings) {
        return Err(CompileError::UnsupportedWasmConstruct(
            "FoldWithCapturedReducer".to_string(),
        ));
    }

    let signatures = binding_signatures(&anf.bindings);
    let effect_data = EffectDataLayout::for_bindings(&anf.bindings);
    let needs_host_call = effect_data.needs_host_call;
    let needs_host_call_write = effect_data.needs_host_call_write;
    let needs_resource_call = effect_data.needs_resource_call;
    // ResourceAcquire sets `needs_memory = true` directly in `collect_expr`
    // (it interns the resource name string and writes the args buffer in linear
    // memory).  ResourceRelease only passes an i64 handle — no memory access —
    // so `needs_resource_call` is NOT folded in here; doing so would
    // over-provision a memory section for ResourceRelease-only modules.
    let needs_memory = effect_data.needs_host_call || effect_data.needs_memory;
    // type_offset / function_offset: bindings start after all imported function entries.
    // Import order:
    //   [0]  ail/host_call          (if needs_host_call)
    //   [1]  ail/host_call_write    (if needs_host_call_write)
    //   [N]  ail/resource_acquire   (if needs_resource_call)
    //   [N+1] ail/resource_release  (if needs_resource_call)
    let type_offset =
        needs_host_call as u32 + needs_host_call_write as u32 + needs_resource_call as u32 * 2;
    let function_offset = type_offset;

    // Detect whether any binding contains a Fold expression.
    // When true: add a function table, element section, and fold-reducer type.
    let needs_fold = anf.bindings.iter().any(|b| anf_has_fold(&b.expr));

    // Collect nested Lambdas that qualify for hoisting (params == 2, no captures).
    // Only meaningful when needs_fold is true; empty otherwise.
    let hoisted_lambdas: Vec<(Vec<String>, AnfExpr)> = if needs_fold {
        collect_hoistable_lambdas(&anf.bindings)
    } else {
        vec![]
    };
    let n_hoisted = hoisted_lambdas.len() as u32;

    // Collect closure-hoistable Lambdas (params == 2, captures non-empty).
    // Only meaningful when needs_fold is true; empty otherwise.
    // (Wave 16A PR3: general closure dispatch for captured fold reducers.)
    let closure_hoistable_lambdas: Vec<(Vec<String>, Vec<String>, AnfExpr)> = if needs_fold {
        collect_closure_hoistable_lambdas(&anf.bindings)
    } else {
        vec![]
    };
    let n_closure_hoisted = closure_hoistable_lambdas.len() as u32;

    // Total functions in the module = bindings + hoisted + closure-hoisted.
    let n_bindings = anf.bindings.len() as u32;
    let n_functions = n_bindings + n_hoisted + n_closure_hoisted;

    // fold_reducer_type_idx: the type-section index of (i64, i64) → i64.
    // It is appended after all host-import types and binding signatures:
    //   index = type_offset + signatures.len()
    let fold_reducer_type_idx: Option<u32> = if needs_fold {
        Some(type_offset + signatures.len() as u32)
    } else {
        None
    };

    // closure_reducer_type_idx: the type-section index of (i64, i64, i64) → i64.
    // Appended immediately after fold_reducer_type when needs_fold is true.
    // Used by the Fold I32 dispatch path for captured Lambda reducers (PR3).
    //   index = type_offset + signatures.len() + 1   (when needs_fold)
    let closure_reducer_type_idx: Option<u32> = if needs_fold {
        Some(type_offset + signatures.len() as u32 + 1)
    } else {
        None
    };

    // Both needs_fold → needs_closure_reducer: add closure-reducer type whenever
    // there is a Fold, even if no captured Lambdas exist in this module.  The
    // type is unused in that case but its presence is harmless and avoids
    // conditional type-section layout changes that would complicate the index
    // arithmetic.
    let needs_closure_reducer = needs_fold;

    // Assemble WASM module first so we can compute byte offsets.
    // Section order follows the WASM binary format spec:
    //   Type(1) Import(2) Function(3) Table(4) Memory(5) Global(6)
    //   Export(7) Element(9) Code(10) Data(11)
    let mut module = Module::new();
    if needs_host_call || needs_resource_call {
        module.section(&build_type_section_with_host_call(
            &signatures,
            needs_host_call,
            needs_host_call_write,
            needs_resource_call,
            needs_fold,
            needs_closure_reducer,
        ));
    } else if let Some(types) = build_type_section(&signatures, needs_fold, needs_closure_reducer) {
        module.section(&types);
    }
    if let Some(imports) =
        build_import_section(needs_host_call, needs_host_call_write, needs_resource_call)
    {
        module.section(&imports);
    }
    if let Some(functions) = build_function_section(
        &signatures,
        type_offset,
        n_hoisted,
        fold_reducer_type_idx,
        n_closure_hoisted,
        closure_reducer_type_idx,
    ) {
        module.section(&functions);
    }
    // Table section (4): required for call_indirect when Fold is present.
    if needs_fold && let Some(table) = build_table_section(n_functions) {
        module.section(&table);
    }
    if let Some(memory) = build_memory_section(needs_memory) {
        module.section(&memory);
    }
    if let Some(globals) = build_global_section(needs_memory, align_to_i64(effect_data.next_offset))
    {
        module.section(&globals);
    }
    if let Some(exports) =
        build_export_section_with_memory(&anf.bindings, function_offset, needs_memory)
    {
        module.section(&exports);
    }
    // Element section (9): populates the function table for call_indirect.
    // Must appear after Export and before Code per WASM binary format.
    if needs_fold && let Some(elems) = build_element_section(function_offset, n_functions) {
        module.section(&elems);
    }
    if let Some(codes) = build_code_section(
        &anf.bindings,
        &effect_data,
        function_offset,
        fold_reducer_type_idx,
        closure_reducer_type_idx,
        &hoisted_lambdas,
        &closure_hoistable_lambdas,
    ) {
        module.section(&codes);
    }
    if let Some(data) = build_data_section(&effect_data) {
        module.section(&data);
    }
    let wasm = module.finish();

    // Build provenance map: NodeRef → WASM byte offset of the code entry.
    // `code_entry_offsets` scans the binary and returns the position of each
    // function's LEB128-encoded body-size prefix in the code section.
    let entry_offsets = code_entry_offsets(&wasm);
    let provenance: BTreeMap<NodeRef, u32> = anf
        .bindings
        .iter()
        .zip(entry_offsets.iter())
        .map(|(b, &offset)| (b.source_ref, offset))
        .collect();

    // Build semantic source map — clone ANF source map and populate wasm_offset.
    // Each binding entry gets wasm_offset = the byte offset from code_entry_offsets.
    let source_map_entries: Vec<SourceMapEntry> = anf
        .source_map
        .entries
        .iter()
        .zip(
            // Zip with entry_offsets; pad with None if offsets are shorter.
            entry_offsets
                .iter()
                .map(|&o| Some(o))
                .chain(std::iter::repeat(None)),
        )
        .map(|(entry, wasm_offset)| SourceMapEntry {
            wasm_offset,
            ..entry.clone()
        })
        .collect();
    let source_map = SourceMap {
        entries: source_map_entries,
    };
    source_map.validate_required_provenance(profile, &anf.bindings)?;

    // Seal: source_map_hash = blake3(source_map_cbor_bytes).
    let source_map_bytes = stable_cbor_bytes(&source_map)?;
    let source_map_hash = hash_with_parent(&[], &source_map_bytes);

    // Seal: wasm_hash = blake3(anf_ir_hash || wasm_binary).
    let wasm_hash = hash_with_parent(&anf_ir_hash, &wasm);

    // Extend the stage hashes from ANF.
    let mut hash_chain = anf.stage_hashes.clone();
    hash_chain.wasm_hash = Some(wasm_hash);
    hash_chain.source_map_hash = Some(source_map_hash);

    // Build capability manifest from bindings — one entry per AnfBinding.
    let capabilities_manifest = CapabilitiesManifest::from_bindings(&anf.bindings);

    // Seal: capabilities_manifest_hash = blake3(cbor(capabilities_manifest)).
    // Uses the same manifest bytes that are stored in WasmArtifact so the hash
    // is consistent with the real manifest (not a proxy over raw bindings).
    let capabilities_manifest_bytes = stable_cbor_bytes(&capabilities_manifest)?;
    let capabilities_manifest_hash = hash_with_parent(&[], &capabilities_manifest_bytes);

    // Build ArtifactManifest from the complete hash chain.
    let artifact_manifest = ArtifactManifest {
        profile: profile.to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        graph_snapshot_hash: hash_chain.graph_snapshot_hash,
        verification_report_hash: hash_chain.verification_report_hash,
        core_ir_hash: hash_chain.core_ir_hash,
        anf_ir_hash,
        wasm_hash: Some(wasm_hash),
        native_hash: None,
        source_map_hash: Some(source_map_hash),
        capabilities_manifest_hash: Some(capabilities_manifest_hash),
    };

    // Seal: artifact_manifest_hash = blake3(manifest_cbor_bytes).
    let manifest_cbor = stable_cbor_bytes(&artifact_manifest)?;
    let artifact_manifest_hash = hash_with_parent(&[], &manifest_cbor);
    hash_chain.artifact_manifest_hash = Some(artifact_manifest_hash);

    // Serialize JSON sidecars.
    // `program.source_map.json` — semantic source map for debugging/profiling.
    let source_map_json = serde_json::to_vec(&source_map)
        .map_err(|e| CompileError::EncodingError(format!("source_map JSON encode: {e}")))?;
    // `program.artifact.json` — profile-bound artifact manifest.
    let artifact_manifest_json = serde_json::to_vec(&artifact_manifest)
        .map_err(|e| CompileError::EncodingError(format!("artifact_manifest JSON encode: {e}")))?;

    // Populate export_types: for each exported binding, derive its descriptor.
    let mut export_types: BTreeMap<String, WasmTypeDescriptor> = BTreeMap::new();
    for binding in &anf.bindings {
        if binding_result(binding).is_some() {
            let name = export_name(&binding.name);
            let descriptor = derive_wasm_type(&binding.expr);
            export_types.insert(name, descriptor);
        }
    }

    let result_buffer_offset = if effect_data.needs_host_call_write {
        Some(effect_data.result_buffer_offset)
    } else {
        None
    };

    let abi_descriptor = AbiDescriptor::new(export_types.clone());

    Ok(WasmArtifact {
        wasm,
        source_map,
        provenance,
        capabilities_manifest,
        hash_chain,
        artifact_manifest,
        source_map_json,
        artifact_manifest_json,
        export_types,
        abi_descriptor,
        result_buffer_offset,
    })
}
