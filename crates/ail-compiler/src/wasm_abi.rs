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
        AnfExpr::Literal(LiteralValue::Float(_)) => WasmTypeDescriptor::Scalar(WasmScalarType::F64),
        AnfExpr::Literal(LiteralValue::Unit) => WasmTypeDescriptor::Scalar(WasmScalarType::I32),
        AnfExpr::Literal(LiteralValue::Text(_)) => WasmTypeDescriptor::Text,
        // LIMITATION: `EffectCall` return types cannot be structurally derived
        // at this compilation stage.  ANF expressions carry no return-type
        // annotation and there are no handler descriptors available here, so
        // the compiler has no information about what concrete type a capability
        // operation actually produces.
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
        LiteralValue::Int(_) | LiteralValue::Bool(_) | LiteralValue::Text(_) => ValType::I64,
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
            let first_ty = arms
                .first()
                .and_then(|arm| infer_expr_type(&arm.body, locals));
            if arms
                .iter()
                .all(|arm| infer_expr_type(&arm.body, locals) == first_ty)
            {
                first_ty
            } else {
                None
            }
        }
        AnfExpr::Return(inner) => infer_expr_type(inner, locals),
        AnfExpr::ShortCircuitAnd { .. } | AnfExpr::ShortCircuitOr { .. } => Some(ValType::I64),
        AnfExpr::Loop { body } => infer_expr_type(body, locals),
        AnfExpr::Break { value } => infer_expr_type(value, locals),
        AnfExpr::Continue | AnfExpr::WhileLoop { .. } => None,
        AnfExpr::RecordNew { .. }
        | AnfExpr::TupleNew(_)
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::Lambda { .. }
        | AnfExpr::Seq(_) => Some(ValType::I32),
        AnfExpr::FieldGet { .. } | AnfExpr::Call { .. } => Some(ValType::I64),
        AnfExpr::EffectCall { .. } => Some(ValType::I64),
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
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        // ola5 Gap 2 — new primitives
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::ForEach { .. }
        | AnfExpr::Fold { .. } => None,
        AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::IndexGet { .. } => Some(ValType::I64),
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
                collect_free_vars(&arm.body, bound, out);
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

pub(crate) fn binding_result(binding: &AnfBinding) -> Option<ValType> {
    let mut locals = binding_params(binding)
        .into_iter()
        .map(|name| (name.to_string(), ValType::I64))
        .collect();
    infer_expr_type(&binding.expr, &mut locals)
        .filter(|ty| matches!(ty, ValType::I64 | ValType::I32))
}

pub(crate) fn binding_signatures(bindings: &[AnfBinding]) -> Vec<WasmSignature> {
    bindings
        .iter()
        .map(|binding| WasmSignature {
            param_count: binding_params(binding).len(),
            result: binding_result(binding),
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
    pub(crate) needs_memory: bool,
}

impl EffectDataLayout {
    pub(crate) fn for_bindings(bindings: &[AnfBinding]) -> Self {
        let mut layout = Self::default();
        for binding in bindings {
            layout.collect_expr(&binding.expr);
        }
        if layout.needs_host_call {
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
            AnfExpr::Lambda { body, .. } => self.collect_expr(body),
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
}
