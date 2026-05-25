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
///
/// Also interns raw byte-buffer literals (`LiteralValue::Bytes`) in a
/// separate `bytes_table` so they are stored as plain `__ail_bytes_N`
/// data objects (no UTF-8 assumption, no NUL terminator).
#[derive(Default)]
pub struct NativeDataLayout {
    /// Interned strings: value → (index into `ordered`, byte length).
    strings: BTreeMap<String, (usize, usize)>,
    /// Ordered list of all interned strings (index = position in this vec).
    pub ordered: Vec<String>,
    /// Interned byte buffers from `LiteralValue::Bytes` literals.
    ///
    /// Stored as an ordered `Vec` because `Vec<u8>` has no canonical key
    /// suitable for `BTreeMap`; linear dedup is fine for small literal counts.
    /// Each entry is the raw byte slice; index is the position in this vec.
    pub bytes_table: Vec<Vec<u8>>,
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

    /// Intern a raw byte buffer, returning its index into `bytes_table`.
    ///
    /// Deduplicates by value: identical byte slices share a single data object.
    pub fn intern_bytes(&mut self, data: &[u8]) -> usize {
        if let Some(pos) = self.bytes_table.iter().position(|b| b.as_slice() == data) {
            return pos;
        }
        let idx = self.bytes_table.len();
        self.bytes_table.push(data.to_vec());
        idx
    }

    /// Return `(index, byte_len)` for a previously interned byte slice.
    ///
    /// Returns `(0, 0)` when the slice was not interned (defensive fallback).
    pub fn get_bytes(&self, data: &[u8]) -> (usize, usize) {
        self.bytes_table
            .iter()
            .enumerate()
            .find(|(_, b)| b.as_slice() == data)
            .map(|(i, b)| (i, b.len()))
            .unwrap_or((0, 0))
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
            AnfExpr::Literal(LiteralValue::Bytes(data)) => {
                self.intern_bytes(data);
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

    /// Declare + define all interned byte-buffer data objects in the `ObjectModule`.
    ///
    /// Each entry in `bytes_table` becomes a `__ail_bytes_N` local data symbol.
    /// The returned `Vec` maps index → `DataId` with the same indexing as
    /// `bytes_table`.
    pub fn define_all_bytes(&self, module: &mut ObjectModule) -> Result<Vec<DataId>, CompileError> {
        let mut bytes_data_ids = Vec::with_capacity(self.bytes_table.len());
        for (i, data) in self.bytes_table.iter().enumerate() {
            let name = format!("__ail_bytes_{i}");
            let data_id = module
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|e| {
                    CompileError::NativeEncodingError(format!("declare_data({name}): {e}"))
                })?;
            let mut desc = DataDescription::new();
            let raw: Box<[u8]> = data.clone().into_boxed_slice();
            // Empty byte slices need at least one byte to be a valid data object.
            if raw.is_empty() {
                desc.define(Box::new([0u8]) as Box<[u8]>);
            } else {
                desc.define(raw);
            }
            module.define_data(data_id, &desc).map_err(|e| {
                CompileError::NativeEncodingError(format!("define_data({name}): {e}"))
            })?;
            bytes_data_ids.push(data_id);
        }
        Ok(bytes_data_ids)
    }
}
