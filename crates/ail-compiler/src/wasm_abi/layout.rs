use super::*;

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
                if effect_call_returns_bytes(capability, func) {
                    self.needs_host_call_write = true;
                    self.needs_memory = true;
                }
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "concat" | "text.concat") && args.len() == 2 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.parse_int_or" | "text_parse_int_or")
                    && args.len() == 2 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.byte_at_or" | "text_byte_at_or")
                    && args.len() == 3 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.trim" | "text_trim") && args.len() == 1 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.slice" | "text_slice") && args.len() == 3 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.replace_first" | "text_replace_first")
                    && args.len() == 3 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.eq" | "text_eq") && args.len() == 2 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.contains" | "text_contains")
                    && args.len() == 2 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "text.index_of" | "text_index_of")
                    && args.len() == 2 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(
                    func.as_str(),
                    "text.starts_with" | "text_starts_with" | "text.ends_with" | "text_ends_with"
                ) && args.len() == 2 =>
            {
                self.needs_memory = true;
            }
            AnfExpr::Call { func, args }
                if matches!(func.as_str(), "bytes.at" | "bytes_at" | "std.bytes.at")
                    && args.len() == 2 =>
            {
                self.needs_memory = true;
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

pub(crate) fn effect_call_returns_bytes(capability: &str, func: &str) -> bool {
    capability == "file.read" && func == "read"
}
