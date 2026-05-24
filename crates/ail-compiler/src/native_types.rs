// ── ail-compiler::native_types ────────────────────────────────────────────
//
// Shared data types for the native backend.
//
// Extracted from `native.rs` to break the bidirectional dependency that arose
// when `native_codegen.rs` needed `NativeDataLayout` (defined in `native.rs`)
// while `native.rs` simultaneously imported from `native_codegen.rs`.
//
// # Dependency contract
//
// This module MUST NOT import from `native` or `native_codegen`.
// Both of those modules may freely import from here.

use std::collections::BTreeMap;

use cranelift_module::{DataDescription, DataId, Linkage, Module};
use cranelift_object::ObjectModule;

use crate::error::CompileError;

// ── NativeDataLayout ──────────────────────────────────────────────────────

/// Pre-scan of all string literals and EffectCall names in an `AnfIr`.
///
/// Interns each unique string exactly once so that multiple bindings
/// using the same literal share a single data object.
#[derive(Default)]
pub struct NativeDataLayout {
    /// Interned strings: value → (index into `ordered`, byte length).
    strings: BTreeMap<String, (usize, usize)>,
    /// Ordered list of all interned strings (index = position in this vec).
    pub ordered: Vec<String>,
    /// Set when any binding contains an `EffectCall` — triggers host_call import.
    pub needs_host_call: bool,
    /// Set when any binding allocates compound values (RecordNew, ListNew, etc.)
    /// that must survive function boundaries — triggers __ail_malloc import.
    pub needs_heap_alloc: bool,
    /// Set when any binding uses runtime services (concurrency, dispatch, resources)
    /// — triggers ail_runtime_call import.
    pub needs_runtime_call: bool,
}

impl NativeDataLayout {
    /// Intern a string, returning its index into `ordered`.
    pub fn intern(&mut self, s: &str) -> usize {
        if let Some(&(idx, _)) = self.strings.get(s) {
            return idx;
        }
        let idx = self.ordered.len();
        let len = s.len();
        self.ordered.push(s.to_string());
        self.strings.insert(s.to_string(), (idx, len));
        idx
    }

    /// Return `(index, byte_len)` for a previously interned string.
    pub fn get(&self, s: &str) -> (usize, usize) {
        self.strings.get(s).copied().unwrap_or((0, 0))
    }

    /// Pre-scan all bindings to intern strings and detect EffectCall usage.
    pub fn for_bindings(bindings: &[crate::anf::AnfBinding]) -> Self {
        let mut layout = Self::default();
        for b in bindings {
            layout.scan_expr(&b.expr);
        }
        layout
    }

    fn scan_expr(&mut self, expr: &crate::anf::AnfExpr) {
        use crate::anf::AnfExpr;
        use crate::core_ir::LiteralValue;
        match expr {
            AnfExpr::Literal(LiteralValue::Text(s)) => {
                self.intern(s);
            }
            AnfExpr::EffectCall {
                capability, func, ..
            } => {
                self.needs_host_call = true;
                self.intern(capability);
                self.intern(func);
            }
            AnfExpr::Let { value, body, .. } => {
                self.scan_expr(value);
                self.scan_expr(body);
            }
            AnfExpr::Return(inner) => self.scan_expr(inner),
            AnfExpr::Seq(exprs) => exprs.iter().for_each(|e| self.scan_expr(e)),
            AnfExpr::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.scan_expr(then_branch);
                self.scan_expr(else_branch);
            }
            AnfExpr::Loop { body } | AnfExpr::Break { value: body } => self.scan_expr(body),
            AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
                self.scan_expr(right);
            }
            AnfExpr::Match { arms, .. } => {
                for arm in arms {
                    self.scan_expr(&arm.body);
                }
            }
            AnfExpr::RecordNew { fields } => {
                for (_, e) in fields {
                    self.scan_expr(e);
                }
                self.needs_heap_alloc = true;
            }
            AnfExpr::FieldUpdate { value, .. } => self.scan_expr(value),
            AnfExpr::TupleNew(elems) => {
                for e in elems {
                    self.scan_expr(e);
                }
                self.needs_heap_alloc = true;
            }
            AnfExpr::ListNew(elems) => {
                for e in elems {
                    self.scan_expr(e);
                }
                self.needs_heap_alloc = true;
            }
            AnfExpr::VariantNew { payload, .. } => {
                if let Some(p) = payload {
                    self.scan_expr(p);
                }
                self.needs_heap_alloc = true;
            }
            // ola5 Gap 2/3 — new variants
            AnfExpr::ForEach { body, .. } => self.scan_expr(body),
            AnfExpr::Lambda { body, captures, .. } => {
                self.scan_expr(body);
                // A lambda that closes over variables needs a heap-allocated
                // closure env struct to carry the captured values.
                if !captures.is_empty() {
                    self.needs_heap_alloc = true;
                }
            }
            AnfExpr::TaskGroup { body } => {
                self.scan_expr(body);
                self.needs_runtime_call = true;
            }
            AnfExpr::Timeout { body, .. } => {
                self.scan_expr(body);
                self.needs_runtime_call = true;
            }
            AnfExpr::TaskSpawn { .. }
            | AnfExpr::TaskAwait { .. }
            | AnfExpr::TaskCancel { .. }
            | AnfExpr::ChannelNew { .. }
            | AnfExpr::ChannelSend { .. }
            | AnfExpr::ChannelReceive { .. }
            | AnfExpr::Select { .. }
            | AnfExpr::Dispatch { .. }
            | AnfExpr::ResourceAcquire { .. }
            | AnfExpr::ResourceRelease { .. } => {
                self.needs_runtime_call = true;
            }
            AnfExpr::CellNew { .. }
            | AnfExpr::CellGet { .. }
            | AnfExpr::CellSet { .. }
            | AnfExpr::MapNew { .. }
            | AnfExpr::SetNew { .. } => {
                self.needs_heap_alloc = true;
            }
            _ => {}
        }
    }

    /// Declare + define all interned string data objects in the `ObjectModule`.
    /// Returns a `Vec` mapping index → `DataId`.
    pub fn define_all(&self, module: &mut ObjectModule) -> Result<Vec<DataId>, CompileError> {
        let mut data_ids = Vec::with_capacity(self.ordered.len());
        for (i, s) in self.ordered.iter().enumerate() {
            let name = format!("__ail_str_{i}");
            let data_id = module
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|e| {
                    CompileError::NativeEncodingError(format!("declare_data({name}): {e}"))
                })?;
            let mut desc = DataDescription::new();
            let bytes: Box<[u8]> = s.as_bytes().to_vec().into_boxed_slice();
            desc.define(bytes);
            module.define_data(data_id, &desc).map_err(|e| {
                CompileError::NativeEncodingError(format!("define_data({name}): {e}"))
            })?;
            data_ids.push(data_id);
        }
        Ok(data_ids)
    }
}
