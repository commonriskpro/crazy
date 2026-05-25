// ── ail-compiler::wasm_abi ────────────────────────────────────────────────
//
// WASM ABI and value-layout helpers.
//
// This module contains:
//   - `WasmScalarType` / `WasmTypeDescriptor` — structured return-type
//     descriptors used by the runtime's `invoke_typed` decoder.
//   - `derive_wasm_type` — derives a descriptor from an `AnfExpr`.
//   - `WasmSignature` — (param_count, result) pairs for module assembly.
//   - Type-inference helpers (`literal_type`, `infer_expr_type`).
//   - Binding analysis (`collect_free_vars`, `binding_params`,
//     `binding_result`, `binding_signatures`).
//   - Export name derivation (`export_name`).
//   - Record/variant layout helpers (`well_known_variant_tag`,
//     `record_layout_fields`).
//   - Effect-data layout analysis (`EffectDataLayout`, `has_effect_call`,
//     `is_structured_descriptor`, `RESULT_BUFFER_MAX`, `MAX_ARGS_BYTES`).
//
// None of these items emit WASM instructions or access `wasm_encoder` types
// beyond `ValType`.

use std::collections::BTreeMap;

use wasm_encoder::ValType;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;
use crate::pattern_string::arm_payload_binding;

// ── ABI versioning ────────────────────────────────────────────────────────

/// Current WASM ABI version.  Increment this when the typed-value layout
/// contract changes in a backward-incompatible way.
pub const ABI_VERSION: u32 = 1;

/// A versioned envelope for the per-export type descriptors emitted by the
/// compiler.  Callers that own a `WasmArtifact` can construct an
/// `AbiDescriptor` from `export_types` and pass it across a process boundary
/// (e.g. serialise to JSON) so the runtime can check compatibility before
/// invoking typed exports.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AbiDescriptor {
    /// Layout version.  Must equal [`ABI_VERSION`] for the current runtime to
    /// decode the exports without an upgrade path.
    pub abi_version: u32,
    /// Maps each exported function name to its [`WasmTypeDescriptor`].
    pub exports: BTreeMap<String, WasmTypeDescriptor>,
}

impl AbiDescriptor {
    /// Wrap `exports` with the current [`ABI_VERSION`].
    pub fn new(exports: BTreeMap<String, WasmTypeDescriptor>) -> Self {
        Self {
            abi_version: ABI_VERSION,
            exports,
        }
    }

    /// Returns `true` when this descriptor's version matches the current
    /// runtime's expected ABI version.
    pub fn is_compatible(&self) -> bool {
        self.abi_version == ABI_VERSION
    }
}

// ── WasmTypeDescriptor ───────────────────────────────────────────────────

/// Scalar WASM primitive types used in the type descriptor.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WasmScalarType {
    I64,
    F64,
    I32,
}

/// Describes the return type of an exported WASM function for use by the
/// runtime decoder when reconstructing a `StructuredValue` from linear memory.
///
/// Populated by `emit_wasm` into `WasmArtifact::export_types`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WasmTypeDescriptor {
    Scalar(WasmScalarType),
    /// A UTF-8 text value packed as `(len as i64) << 32 | (ptr as i64)` in
    /// the raw i64 WASM return slot.  The runtime unpacks this into
    /// `StructuredValue::Text { ptr, len }` without a separate memory read.
    Text,
    /// A raw byte buffer packed as `(len as i64) << 32 | (ptr as i64)` in
    /// the raw i64 WASM return slot.  Decoded to
    /// `StructuredValue::Bytes { ptr, len }` without a memory read.
    ///
    /// Unlike [`WasmTypeDescriptor::Text`], no UTF-8 assumption is made —
    /// the bytes are treated as opaque.  Used for capability operations that
    /// return binary payloads (e.g. serialised CBOR, cryptographic digests).
    Bytes,
    Record {
        fields: Vec<String>,
    },
    Variant {
        tags: Vec<String>,
    },
    Tuple(Vec<WasmTypeDescriptor>),
    List(Box<WasmTypeDescriptor>),
    Option(Box<WasmTypeDescriptor>),
    Result {
        ok: Box<WasmTypeDescriptor>,
        err: Box<WasmTypeDescriptor>,
    },
    Handle,
}

/// Derive the `WasmTypeDescriptor` for an `AnfExpr` by recursively inspecting
/// the expression tree.  Used to populate `WasmArtifact::export_types`.
///
/// # Coverage of `Option`, `Result`, `Bytes`, and `Handle`
///
/// `Handle` is determinable when the top-level expression is
/// `ResourceAcquire` — that node is defined as yielding a resource handle.
///
/// `Option` and `Result` are NOT derivable from current ANF shapes because
/// there are no dedicated `AnfExpr::OptionNew` or `AnfExpr::ResultNew`
/// constructors.  A `VariantNew { tag: "None" | "Some" | "Ok" | "Err" }`
/// cannot be reliably distinguished from a user-defined enum with those tag
/// names without type-checker annotations in the ANF nodes.  Until such
/// annotations are propagated, callers that require Option/Result descriptors
/// must construct them from an external type-descriptor table.
///
/// `Bytes` IS derivable when the top-level expression is
/// `AnfExpr::Literal(LiteralValue::Bytes(_))`.  The packed `(len << 32) | ptr`
/// i64 encoding mirrors the `Text` layout; the runtime decodes it via
/// `ValueLayout::Bytes` → `StructuredValue::Bytes { ptr, len }`.
pub fn derive_wasm_type(expr: &AnfExpr) -> WasmTypeDescriptor {
    match expr {
        AnfExpr::RecordNew { fields } => WasmTypeDescriptor::Record {
            fields: fields.iter().map(|(f, _)| f.clone()).collect(),
        },
        AnfExpr::VariantNew { tag, .. } => WasmTypeDescriptor::Variant {
            tags: vec![tag.clone()],
        },
        AnfExpr::TupleNew(elems) => {
            WasmTypeDescriptor::Tuple(elems.iter().map(derive_wasm_type).collect())
        }
        AnfExpr::ListNew(_) => {
            WasmTypeDescriptor::List(Box::new(WasmTypeDescriptor::Scalar(WasmScalarType::I64)))
        }
        AnfExpr::Let { body, .. } => derive_wasm_type(body),
        // ── Literal arms — explicit to avoid relying on the wildcard ──────
        AnfExpr::Literal(LiteralValue::Float(_)) => WasmTypeDescriptor::Scalar(WasmScalarType::F64),
        AnfExpr::Literal(LiteralValue::Unit) => WasmTypeDescriptor::Scalar(WasmScalarType::I32),
        AnfExpr::Literal(LiteralValue::Text(_)) => WasmTypeDescriptor::Text,
        // Bytes literal — packed ptr/len i64, decoded as opaque byte buffer.
        AnfExpr::Literal(LiteralValue::Bytes(_)) => WasmTypeDescriptor::Bytes,
        // Int and Bool both inhabit the i64 WASM slot (see `literal_type`).
        AnfExpr::Literal(LiteralValue::Int(_)) | AnfExpr::Literal(LiteralValue::Bool(_)) => {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        // ── ResourceAcquire → Handle ──────────────────────────────────────
        //
        // `ResourceAcquire` is the only ANF node whose semantic contract
        // guarantees a handle return: the expression yields an opaque resource
        // handle packed into the i64 return slot as a u64 ID.
        //
        // Other concurrency/cell primitives (ChannelNew, CellNew, TaskSpawn)
        // also produce handle-like values at the language level, but their
        // ABI representation is still evolving; they remain in the wildcard
        // fallback until their return layout is stabilised.
        AnfExpr::ResourceAcquire { .. } => WasmTypeDescriptor::Handle,
        // ── EffectCall limitation ─────────────────────────────────────────
        //
        // `EffectCall` return types cannot be structurally derived at this
        // compilation stage.  ANF expressions carry no return-type annotation
        // and there are no handler descriptors available here, so the compiler
        // has no information about what concrete type a capability operation
        // actually produces.
        //
        // We therefore always return `Scalar(I64)`, which is the raw value
        // placed in the WASM return slot by the `ail/host_call` import (the
        // host packs the result handle or small integer into that slot).
        //
        // Resolving this limitation requires one of:
        //   - ANF return-type annotations propagated from the type-checker, or
        //   - A handler-descriptor table passed into `derive_wasm_type` so it
        //     can look up the declared return type of the effect operation.
        //
        // Until then, callers that need structured EffectCall return descriptors
        // (e.g. `is_structured_descriptor` + `needs_host_call_write`) must be
        // driven by the surrounding expression context (e.g. the binding body
        // being a `RecordNew` that consumes the effect result) rather than by
        // the `EffectCall` node itself.
        AnfExpr::EffectCall { .. } => WasmTypeDescriptor::Scalar(WasmScalarType::I64),
        _ => WasmTypeDescriptor::Scalar(WasmScalarType::I64),
    }
}

// ── WasmSignature ─────────────────────────────────────────────────────────

/// (param_count, result) descriptor used by the type and function sections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WasmSignature {
    pub(crate) param_count: usize,
    pub(crate) result: Option<ValType>,
}

// ── Type inference ────────────────────────────────────────────────────────

pub(crate) fn literal_type(lit: &LiteralValue) -> ValType {
    match lit {
        // Text and Bytes both use the packed ptr/len i64 encoding.
        LiteralValue::Int(_)
        | LiteralValue::Bool(_)
        | LiteralValue::Text(_)
        | LiteralValue::Bytes(_) => ValType::I64,
        LiteralValue::Unit => ValType::I32,
        LiteralValue::Float(_) => ValType::F64,
    }
}

pub(crate) fn infer_expr_type(
    expr: &AnfExpr,
    locals: &mut Vec<(String, ValType)>,
) -> Option<ValType> {
    match expr {
        AnfExpr::Literal(lit) => Some(literal_type(lit)),
        AnfExpr::Var(name) => locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, ty)| *ty),
        AnfExpr::Let { name, value, body } => {
            let value_ty = infer_expr_type(value, locals).unwrap_or(ValType::I32);
            locals.push((name.clone(), value_ty));
            let body_ty = infer_expr_type(body, locals);
            locals.pop();
            body_ty
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            let then_ty = infer_expr_type(then_branch, locals);
            let else_ty = infer_expr_type(else_branch, locals);
            if then_ty == else_ty { then_ty } else { None }
        }
        AnfExpr::Match { arms, .. } => {
            // Infer each arm's body type, temporarily adding the payload binding
            // variable (e.g. `x` from `"Ok(x)"`) to locals so that references to
            // it in the body resolve as I64 rather than returning None.
            let mut unanimous: Option<Option<ValType>> = None;
            for arm in arms {
                let payload = arm_payload_binding(&arm.pattern);
                if let Some(name) = payload {
                    locals.push((name.to_string(), ValType::I64));
                }
                let ty = infer_expr_type(&arm.body, locals);
                if payload.is_some() {
                    locals.pop();
                }
                match unanimous {
                    None => unanimous = Some(ty),
                    Some(prev) if prev != ty => return None,
                    Some(_) => {}
                }
            }
            unanimous.flatten()
        }
        AnfExpr::Return(inner) => infer_expr_type(inner, locals),
        AnfExpr::ShortCircuitAnd { .. } | AnfExpr::ShortCircuitOr { .. } => Some(ValType::I64),
        AnfExpr::Loop { body } => infer_expr_type(body, locals),
        AnfExpr::Break { value } => infer_expr_type(value, locals),
        AnfExpr::Continue => None,
        // WhileLoop pushes a unit (I32 0) after the loop ends so it can appear
        // as the value in a Let binding without causing a stack-underflow error.
        // Mirrors the ForEach fix from Wave 18B.
        AnfExpr::WhileLoop { .. } => Some(ValType::I32),
        AnfExpr::RecordNew { .. }
        | AnfExpr::TupleNew(_)
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::Lambda { .. }
        | AnfExpr::Seq(_) => Some(ValType::I32),
        AnfExpr::FieldGet { .. } | AnfExpr::Call { .. } => Some(ValType::I64),
        AnfExpr::EffectCall { .. } => Some(ValType::I64),
        // ── Cell primitives ───────────────────────────────────────────────
        // CellNew returns an I32 pointer; CellGet returns the I64 value;
        // CellSet is a write that returns unit (I32 0), consistent with
        // the unit-as-I32(0) pattern used throughout the emit layer.
        AnfExpr::CellNew { .. } => Some(ValType::I32),
        AnfExpr::CellGet { .. } => Some(ValType::I64),
        AnfExpr::CellSet { .. } => Some(ValType::I32),
        // ── Collection constructors ───────────────────────────────────────
        // MapNew and SetNew return I32 pointers into linear memory.
        // IndexGet reads an element and returns I64.
        AnfExpr::MapNew { .. } | AnfExpr::SetNew { .. } => Some(ValType::I32),
        AnfExpr::IndexGet { .. } => Some(ValType::I64),
        // ForEach produces a unit (I32 0) so it can appear as the value in
        // a `Let` binding or as an intermediate element in a `Seq` without
        // causing a WASM stack-underflow validation error.
        AnfExpr::ForEach { .. } => Some(ValType::I32),
        // Fold reduces a list to an I64 accumulator via call_indirect.
        // emit_anf_expr returns Some(ValType::I64) for Fold; this must match.
        AnfExpr::Fold { .. } => Some(ValType::I64),
        // ResourceAcquire returns an opaque resource handle packed as i64.
        AnfExpr::ResourceAcquire { .. } => Some(ValType::I64),
        // ResourceRelease is a side-effect with no return value.
        AnfExpr::Placeholder
        | AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Select { .. }
        | AnfExpr::Timeout { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceRelease { .. }
        // ola5 Gap 2 — remaining stubs
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. } => None,
        AnfExpr::FieldUpdate { value, .. } => infer_expr_type(value, locals).or(Some(ValType::I32)),
    }
}

// ── Binding analysis ──────────────────────────────────────────────────────

pub(crate) fn collect_free_vars<'a>(
    expr: &'a AnfExpr,
    bound: &mut Vec<&'a str>,
    out: &mut Vec<&'a str>,
) {
    match expr {
        AnfExpr::Var(name)
            if !bound.iter().rev().any(|bound_name| *bound_name == name)
                && !out.iter().any(|existing| *existing == name) =>
        {
            out.push(name);
        }
        AnfExpr::Let { name, value, body } => {
            collect_free_vars(value, bound, out);
            bound.push(name);
            collect_free_vars(body, bound, out);
            bound.pop();
        }
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if !bound.iter().rev().any(|bound_name| *bound_name == cond)
                && !out.iter().any(|existing| *existing == cond)
            {
                out.push(cond);
            }
            collect_free_vars(then_branch, bound, out);
            collect_free_vars(else_branch, bound, out);
        }
        AnfExpr::Call { args, .. } => {
            for arg in args {
                if !bound.iter().rev().any(|bound_name| *bound_name == arg)
                    && !out.iter().any(|existing| *existing == arg)
                {
                    out.push(arg);
                }
            }
        }
        AnfExpr::EffectCall { args, .. } => {
            for arg in args {
                if !bound.iter().rev().any(|bound_name| *bound_name == arg)
                    && !out.iter().any(|existing| *existing == arg)
                {
                    out.push(arg);
                }
            }
        }
        AnfExpr::Return(inner)
        | AnfExpr::ShortCircuitAnd { right: inner, .. }
        | AnfExpr::ShortCircuitOr { right: inner, .. }
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::FieldUpdate { value: inner, .. } => collect_free_vars(inner, bound, out),
        AnfExpr::WhileLoop { cond, body } => {
            if !bound.iter().rev().any(|bound_name| *bound_name == cond)
                && !out.iter().any(|existing| *existing == cond)
            {
                out.push(cond);
            }
            collect_free_vars(body, bound, out);
        }
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            for expr in exprs {
                collect_free_vars(expr, bound, out);
            }
        }
        AnfExpr::Match { arms, .. } => {
            for arm in arms {
                // A single-binding constructor pattern like "Ok(x)" introduces
                // a payload variable that is locally bound within the arm body.
                // Add it to `bound` so it is not reported as a free variable.
                let payload = arm_payload_binding(&arm.pattern);
                if let Some(name) = payload {
                    bound.push(name);
                }
                collect_free_vars(&arm.body, bound, out);
                if payload.is_some() {
                    bound.pop();
                }
            }
        }
        // Lambda: the `captures` field explicitly names the free variables this
        // lambda needs from the enclosing scope.  Propagate each capture to
        // `out` if it is not already bound — this is more efficient than
        // re-scanning the body and produces the same set as long as captures
        // were populated correctly by the lowering pass.
        //
        // An empty captures list has two meanings: the lambda genuinely closes
        // over nothing, or it is a hand-built fixture that did not populate the
        // field.  Both cases are handled by the body-scan fallback below.
        AnfExpr::Lambda {
            params,
            body,
            captures,
        } => {
            if captures.is_empty() {
                // Fallback: re-scan the body for free vars — handles lambdas
                // that capture nothing, including hand-built fixtures that omit
                // the captures field.
                let original_len = bound.len();
                bound.extend(params.iter().map(String::as_str));
                collect_free_vars(body, bound, out);
                bound.truncate(original_len);
            } else {
                // Fast path: use the explicit capture list.
                for cap in captures {
                    if !bound.iter().rev().any(|b| *b == cap) && !out.iter().any(|e| *e == cap) {
                        out.push(cap);
                    }
                }
            }
        }
        AnfExpr::RecordNew { fields } => {
            for (_, expr) in fields {
                collect_free_vars(expr, bound, out);
            }
        }
        AnfExpr::VariantNew {
            payload: Some(payload),
            ..
        } => collect_free_vars(payload, bound, out),
        _ => {}
    }
}

pub(crate) fn binding_params(binding: &AnfBinding) -> Vec<&str> {
    let mut params = Vec::new();
    collect_free_vars(&binding.expr, &mut Vec::new(), &mut params);
    params
}

/// Returns the `params` field of a top-level `Lambda` expression, or `&[]`
/// for non-Lambda expressions.
///
/// Used by `binding_signatures` and `build_code_section` to include the
/// Lambda's own call parameters (distinct from captures) in the WASM function
/// signature.  For a top-level Lambda binding the WASM function emits the
/// Lambda body directly, so its params are additional WASM function locals
/// beyond the captured-variable locals that come from `binding_params`.
pub(crate) fn lambda_body_params(expr: &AnfExpr) -> &[String] {
    match expr {
        AnfExpr::Lambda { params, .. } => params,
        _ => &[],
    }
}

pub(crate) fn binding_result(binding: &AnfBinding) -> Option<ValType> {
    match &binding.expr {
        // For a top-level Lambda binding the WASM function emits the Lambda
        // body directly (captures + Lambda params in scope).  Infer the
        // result type from the body, not from the Lambda node itself (which
        // would always return I32 for the nested-closure-ptr path).
        AnfExpr::Lambda { params, body, .. } => {
            let mut locals: Vec<(String, ValType)> = binding_params(binding)
                .into_iter()
                .map(|name| (name.to_string(), ValType::I64))
                .collect();
            // Add the Lambda's own params after the captured-variable locals.
            locals.extend(params.iter().map(|p| (p.clone(), ValType::I64)));
            infer_expr_type(body, &mut locals)
                .filter(|ty| matches!(ty, ValType::I64 | ValType::I32))
        }
        expr => {
            let mut locals = binding_params(binding)
                .into_iter()
                .map(|name| (name.to_string(), ValType::I64))
                .collect();
            infer_expr_type(expr, &mut locals)
                .filter(|ty| matches!(ty, ValType::I64 | ValType::I32))
        }
    }
}

pub(crate) fn binding_signatures(bindings: &[AnfBinding]) -> Vec<WasmSignature> {
    bindings
        .iter()
        .map(|binding| {
            // For Lambda bindings: WASM params = captures + Lambda's own params.
            let capture_count = binding_params(binding).len();
            let lambda_param_count = lambda_body_params(&binding.expr).len();
            WasmSignature {
                param_count: capture_count + lambda_param_count,
                result: binding_result(binding),
            }
        })
        .collect()
}

// ── Export naming ─────────────────────────────────────────────────────────

pub(crate) fn export_name(binding_name: &str) -> String {
    let local = binding_name.rsplit('.').next().unwrap_or(binding_name);
    local
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

// ── Record/variant layout helpers ─────────────────────────────────────────

pub(crate) fn well_known_variant_tag(tag: &str) -> Option<u32> {
    match tag {
        "None" | "Ok" => Some(0),
        "Some" | "Err" => Some(1),
        _ => None,
    }
}

pub(crate) fn record_layout_fields(expr: &AnfExpr) -> Option<Vec<String>> {
    match expr {
        AnfExpr::RecordNew { fields } => {
            Some(fields.iter().map(|(field, _)| field.clone()).collect())
        }
        AnfExpr::Let { body, .. } => record_layout_fields(body),
        _ => None,
    }
}

// ── Effect data layout ────────────────────────────────────────────────────

/// Maximum bytes the host may write into the result buffer.
pub(crate) const RESULT_BUFFER_MAX: i32 = 1024;

/// Maximum args slots reserved in the args buffer (8 args × 8 bytes = 64).
pub(crate) const MAX_ARGS_BYTES: i32 = 64;

/// Returns true if `expr` or any sub-expression is an `EffectCall`.
pub(crate) fn has_effect_call(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::EffectCall { .. } => true,
        AnfExpr::Let { value, body, .. } => has_effect_call(value) || has_effect_call(body),
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => has_effect_call(then_branch) || has_effect_call(else_branch),
        AnfExpr::Return(inner)
        | AnfExpr::ShortCircuitAnd { right: inner, .. }
        | AnfExpr::ShortCircuitOr { right: inner, .. }
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::FieldUpdate { value: inner, .. } => has_effect_call(inner),
        AnfExpr::WhileLoop { body, .. } => has_effect_call(body),
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            exprs.iter().any(has_effect_call)
        }
        AnfExpr::RecordNew { fields } => fields.iter().any(|(_, e)| has_effect_call(e)),
        AnfExpr::VariantNew { payload, .. } => payload.as_deref().is_some_and(has_effect_call),
        AnfExpr::Match { arms, .. } => arms.iter().any(|arm| has_effect_call(&arm.body)),
        AnfExpr::Lambda { body, .. } => has_effect_call(body),
        _ => false,
    }
}

/// Returns true when `desc` is a compound/structured type (not a plain scalar).
pub(crate) fn is_structured_descriptor(desc: &WasmTypeDescriptor) -> bool {
    matches!(
        desc,
        WasmTypeDescriptor::Record { .. }
            | WasmTypeDescriptor::Variant { .. }
            | WasmTypeDescriptor::Tuple(_)
            | WasmTypeDescriptor::List(_)
            | WasmTypeDescriptor::Option(_)
            | WasmTypeDescriptor::Result { .. }
    )
}

/// String-interning layout table for effect data (capability/op strings +
/// args buffer) in WASM linear memory.
#[derive(Clone, Debug, Default)]
pub(crate) struct EffectDataLayout {
    pub(crate) strings: BTreeMap<String, (i32, i32)>,
    /// Raw byte-buffer entries interned from `LiteralValue::Bytes` literals.
    ///
    /// Each entry is `(data, ptr)`: the byte slice that was interned and the
    /// linear-memory offset at which it was placed.  Length is `data.len()`.
    /// Stored as a `Vec` (not a `BTreeMap`) because byte slices have no
    /// canonical string key; linear scan is acceptable for the small numbers
    /// of compile-time byte literals expected in practice.
    pub(crate) bytes_entries: Vec<(Vec<u8>, i32)>,
    pub(crate) next_offset: i32,
    pub(crate) args_offset: i32,
    /// Offset of the structured result buffer in WASM linear memory.
    /// Set when `needs_host_call_write` is true; placed after the args area.
    pub(crate) result_buffer_offset: i32,
    pub(crate) needs_host_call: bool,
    /// True when at least one EffectCall in a binding has a structured return type
    /// (Record, Variant, List, Option, or Result). Causes `ail/host_call_write`
    /// to be imported and used in place of `ail/host_call` for those calls.
    pub(crate) needs_host_call_write: bool,
    /// True when any binding contains `ResourceAcquire` or `ResourceRelease`.
    /// Causes `ail/resource_acquire` and `ail/resource_release` to be imported.
    pub(crate) needs_resource_call: bool,
    pub(crate) needs_memory: bool,
}

impl EffectDataLayout {
    /// Function index of `ail/resource_acquire` within the import table.
    ///
    /// Resource imports are placed after `ail/host_call[_write]` imports:
    /// - index 0: `ail/host_call`         (if `needs_host_call`)
    /// - index 1: `ail/host_call_write`   (if `needs_host_call_write`)
    /// - index N: `ail/resource_acquire`  (if `needs_resource_call`)
    /// - index N+1: `ail/resource_release`
    pub(crate) fn resource_acquire_func_index(&self) -> u32 {
        self.needs_host_call as u32 + self.needs_host_call_write as u32
    }

    /// Function index of `ail/resource_release` within the import table.
    pub(crate) fn resource_release_func_index(&self) -> u32 {
        self.resource_acquire_func_index() + 1
    }

    pub(crate) fn for_bindings(bindings: &[AnfBinding]) -> Self {
        let mut layout = Self::default();
        for binding in bindings {
            layout.collect_expr(&binding.expr);
        }
        if layout.needs_host_call || layout.needs_resource_call {
            layout.args_offset = layout.next_offset.max(1);
        }
        // Detect structured EffectCall: any binding that both (a) contains an
        // EffectCall and (b) has a structured return type needs host_call_write.
        if layout.needs_host_call {
            for binding in bindings {
                if has_effect_call(&binding.expr)
                    && is_structured_descriptor(&derive_wasm_type(&binding.expr))
                {
                    layout.needs_host_call_write = true;
                    break;
                }
            }
        }
        if layout.needs_host_call_write {
            // Reserve the result buffer after the args area.
            layout.result_buffer_offset = layout.args_offset + MAX_ARGS_BYTES;
        }
        layout
    }

    pub(crate) fn collect_expr(&mut self, expr: &AnfExpr) {
        match expr {
            AnfExpr::Literal(LiteralValue::Text(s)) => {
                self.intern(s);
                self.needs_memory = true;
            }
            AnfExpr::Literal(LiteralValue::Bytes(data)) => {
                self.intern_bytes(data);
                self.needs_memory = true;
            }
            AnfExpr::EffectCall {
                capability, func, ..
            } => {
                self.needs_host_call = true;
                self.intern(capability);
                self.intern(func);
            }
            AnfExpr::Let { value, body, .. } => {
                self.collect_expr(value);
                self.collect_expr(body);
            }
            AnfExpr::FieldGet { .. } => {
                self.needs_memory = true;
            }
            AnfExpr::FieldUpdate { value, .. } => {
                self.needs_memory = true;
                self.collect_expr(value);
            }
            AnfExpr::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_expr(then_branch);
                self.collect_expr(else_branch);
            }
            AnfExpr::Return(inner)
            | AnfExpr::ShortCircuitAnd { right: inner, .. }
            | AnfExpr::ShortCircuitOr { right: inner, .. }
            | AnfExpr::Loop { body: inner }
            | AnfExpr::Break { value: inner } => self.collect_expr(inner),
            AnfExpr::WhileLoop { body, .. } => self.collect_expr(body),
            AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
                if !matches!(expr, AnfExpr::Seq(_)) {
                    self.needs_memory = true;
                }
                for expr in exprs {
                    self.collect_expr(expr);
                }
            }
            AnfExpr::Match { arms, .. } => {
                for arm in arms {
                    self.collect_expr(&arm.body);
                }
            }
            AnfExpr::Lambda { captures, body, .. } => {
                // A Lambda sub-expression with captures will emit a closure env
                // struct in linear memory (via emit_alloc).  Mark needs_memory
                // so the WASM module includes the linear-memory and bump-
                // allocator-global sections required by emit_alloc.
                if !captures.is_empty() {
                    self.needs_memory = true;
                }
                self.collect_expr(body);
            }
            AnfExpr::RecordNew { fields } => {
                self.needs_memory = true;
                for (_, expr) in fields {
                    self.collect_expr(expr);
                }
            }
            AnfExpr::VariantNew { payload, .. } => {
                self.needs_memory = true;
                if let Some(payload) = payload {
                    self.collect_expr(payload);
                }
            }
            // ── Collection and cell primitives need linear memory ─────────
            // emit_alloc is called for CellNew/MapNew/SetNew; CellGet and
            // CellSet issue I64Load/I64Store; IndexGet issues I64Load at a
            // dynamic offset.  ForEach issues I64Load to read list elements.
            // All require the memory and bump-allocator-global sections.
            AnfExpr::CellNew { .. }
            | AnfExpr::CellGet { .. }
            | AnfExpr::CellSet { .. }
            | AnfExpr::MapNew { .. }
            | AnfExpr::SetNew { .. }
            | AnfExpr::IndexGet { .. } => {
                self.needs_memory = true;
            }
            AnfExpr::ForEach { body, .. } => {
                self.needs_memory = true;
                self.collect_expr(body);
            }
            // ── Resource primitives need the import table + linear memory ──
            // `ResourceAcquire` interns the resource name string (data section)
            // and uses the shared args buffer, both of which live in linear memory.
            // `ResourceRelease` only passes an i64 handle — no memory needed —
            // but it still requires the `ail/resource_release` import.
            AnfExpr::ResourceAcquire { resource, .. } => {
                self.needs_resource_call = true;
                self.needs_memory = true;
                self.intern(resource);
            }
            AnfExpr::ResourceRelease { .. } => {
                self.needs_resource_call = true;
            }
            _ => {}
        }
    }

    pub(crate) fn intern(&mut self, value: &str) {
        if self.strings.contains_key(value) {
            return;
        }
        let ptr = self.next_offset;
        let len = value.len() as i32;
        self.strings.insert(value.to_string(), (ptr, len));
        self.next_offset += len.max(1);
    }

    pub(crate) fn string(&self, value: &str) -> (i32, i32) {
        self.strings[value]
    }

    /// Intern a raw byte buffer into the linear-memory data section.
    ///
    /// Byte-identical slices are deduplicated — the same `(ptr, len)` is
    /// returned for equal content.  An empty slice occupies 1 byte so that
    /// its pointer is always distinct from `ptr == 0` (which is the
    /// bump-allocator base and reserved for the null-address convention).
    pub(crate) fn intern_bytes(&mut self, data: &[u8]) -> (i32, i32) {
        // Linear dedup — acceptable for compile-time byte literals.
        if let Some((_, ptr)) = self
            .bytes_entries
            .iter()
            .find(|(d, _)| d.as_slice() == data)
        {
            return (*ptr, data.len() as i32);
        }
        let ptr = self.next_offset;
        let len = data.len() as i32;
        self.bytes_entries.push((data.to_vec(), ptr));
        self.next_offset += len.max(1);
        (ptr, len)
    }

    /// Return the `(ptr, len)` previously interned for `data`.
    ///
    /// Panics if `data` was not interned — callers must call `intern_bytes`
    /// during the layout-collection phase before calling `bytes` during emit.
    pub(crate) fn bytes(&self, data: &[u8]) -> (i32, i32) {
        self.bytes_entries
            .iter()
            .find(|(d, _)| d.as_slice() == data)
            .map(|(d, ptr)| (*ptr, d.len() as i32))
            .expect("byte literal not interned; call intern_bytes first")
    }
}

// ── Tests are in pattern_string.rs (canonical location) ──────────────────
//
// The arm_payload_binding function is imported from pattern_string and
// fully tested there. No duplicate tests are kept here.
