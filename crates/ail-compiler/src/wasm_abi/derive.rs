use super::*;

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
    derive_wasm_type_with_locals(expr, &mut BTreeMap::new())
}

fn derive_wasm_type_with_locals(
    expr: &AnfExpr,
    locals: &mut BTreeMap<String, WasmTypeDescriptor>,
) -> WasmTypeDescriptor {
    match expr {
        AnfExpr::RecordNew { fields } => WasmTypeDescriptor::Record {
            fields: fields.iter().map(|(f, _)| f.clone()).collect(),
        },
        AnfExpr::VariantNew { tag, .. } => WasmTypeDescriptor::Variant {
            tags: vec![tag.clone()],
        },
        AnfExpr::TupleNew(elems) => WasmTypeDescriptor::Tuple(
            elems
                .iter()
                .map(|elem| derive_wasm_type_with_locals(elem, locals))
                .collect(),
        ),
        AnfExpr::ListNew(_) => {
            WasmTypeDescriptor::List(Box::new(WasmTypeDescriptor::Scalar(WasmScalarType::I64)))
        }
        AnfExpr::Let { name, value, body } => {
            let value_ty = derive_wasm_type_with_locals(value, locals);
            let previous = locals.insert(name.clone(), value_ty);
            let body_ty = derive_wasm_type_with_locals(body, locals);
            match previous {
                Some(prev) => {
                    locals.insert(name.clone(), prev);
                }
                None => {
                    locals.remove(name);
                }
            }
            body_ty
        }
        AnfExpr::Var(name) => locals
            .get(name)
            .cloned()
            .unwrap_or(WasmTypeDescriptor::Scalar(WasmScalarType::I64)),
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "concat" | "text.concat") && args.len() == 2 =>
        {
            WasmTypeDescriptor::Text
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.trim" | "text_trim") && args.len() == 1 =>
        {
            WasmTypeDescriptor::Text
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.slice" | "text_slice") && args.len() == 3 =>
        {
            WasmTypeDescriptor::Text
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.replace_first" | "text_replace_first")
                && args.len() == 3 =>
        {
            WasmTypeDescriptor::Text
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.eq" | "text_eq") && args.len() == 2 =>
        {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.contains" | "text_contains") && args.len() == 2 =>
        {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.index_of" | "text_index_of") && args.len() == 2 =>
        {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.parse_int_or" | "text_parse_int_or")
                && args.len() == 2 =>
        {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "text.byte_at_or" | "text_byte_at_or")
                && args.len() == 3 =>
        {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        AnfExpr::Call { func, args }
            if matches!(
                func.as_str(),
                "text.starts_with" | "text_starts_with" | "text.ends_with" | "text_ends_with"
            ) && args.len() == 2 =>
        {
            WasmTypeDescriptor::Scalar(WasmScalarType::I64)
        }
        AnfExpr::Return(expr) => derive_wasm_type_with_locals(expr, locals),
        AnfExpr::Lambda { body, params, .. } => {
            let mut scoped = locals.clone();
            for param in params {
                scoped.remove(param);
            }
            derive_wasm_type_with_locals(body, &mut scoped)
        }
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
