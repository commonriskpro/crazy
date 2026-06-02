use super::*;

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
        // WhileLoop always produces I32 0 (unit) after the loop: the outer WASM
        // block has arity 0, so no value is threaded through Break; after the
        // block exits, I32Const 0 is pushed unconditionally.  This allows
        // WhileLoop to appear in a Let binding or Seq without a stack-underflow
        // validation error.  Mirrors the ForEach fix from Wave 18B.
        AnfExpr::WhileLoop { .. } => Some(ValType::I32),
        AnfExpr::RecordNew { .. }
        | AnfExpr::TupleNew(_)
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::Lambda { .. }
        | AnfExpr::Seq(_) => Some(ValType::I32),
        AnfExpr::FieldGet { .. } => Some(ValType::I64),
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "bytes.at" | "bytes_at" | "std.bytes.at")
                && args.len() == 2 =>
        {
            Some(ValType::I32)
        }
        AnfExpr::Call { func, args }
            if matches!(func.as_str(), "bytes.slice" | "bytes_slice" | "std.bytes.slice")
                && args.len() == 3 =>
        {
            Some(ValType::I32)
        }
        AnfExpr::Call { .. } => Some(ValType::I64),
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
