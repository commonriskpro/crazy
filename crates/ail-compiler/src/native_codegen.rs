// ── ail-compiler::native_codegen ─────────────────────────────────────────
//
// Cranelift expression lowering for the native backend.
//
// Extracted from `native.rs` to isolate expression codegen from
// module-building, data-layout, and artifact-sealing concerns.
//
// # Responsibilities
//
// - Per-function compilation context (`NativeCodegenCtx`)
// - ANF expression → Cranelift IR lowering (`lower_anf_expr_cranelift`,
//   delegated to `native_lower`)
// - Return-type inference (`infer_cranelift_return_type`)
//
// # Non-responsibilities
//
// - Object module creation       → `native::build_object_module`
// - String interning / data scan → `native_types::NativeDataLayout`
// - Artifact sealing, hash chain → `native::emit_native_with_profile`
// - Expression lowering logic    → `native_lower::lower_anf_expr_cranelift`

use std::collections::BTreeMap;

use cranelift_codegen::ir::{Block, Value, types};
use cranelift_module::{DataId, FuncId};

use crate::native_types::NativeDataLayout;

// ── LowerResult ───────────────────────────────────────────────────────────

/// Result of lowering one `AnfExpr` into Cranelift IR.
pub(crate) enum LowerResult {
    /// The expression produced a Cranelift SSA value; the current block is
    /// NOT yet terminated — caller must emit `return_(&[val])`.
    Value(cranelift_codegen::ir::Value),
    /// The expression produces no value (unit); the current block is NOT
    /// yet terminated — caller must emit `return_(&[])`.
    Unit,
    /// The expression emitted a terminating instruction (`trap`); the current
    /// block IS terminated — caller must NOT emit another terminator.
    Terminated,
}

// ── NativeLabelKind ───────────────────────────────────────────────────────

/// Kind of label pushed onto the label stack for Loop/Break/Continue resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeLabelKind {
    LoopBreak,
    LoopContinue,
}

// ── NativeCodegenCtx ──────────────────────────────────────────────────────

/// Per-function compilation context for `lower_anf_expr_cranelift`.
pub(crate) struct NativeCodegenCtx<'a> {
    /// Maps ANF let-binding names to their Cranelift SSA `Value` + type.
    /// Uses `String` keys to avoid lifetime complexity with nested expressions.
    pub(crate) locals: BTreeMap<String, (Value, types::Type)>,
    /// Label stack for Loop/Break/Continue resolution.
    pub(crate) labels: Vec<(NativeLabelKind, Block)>,
    /// Record layout map: binding name → ordered field names.
    pub(crate) record_layouts: BTreeMap<String, Vec<String>>,
    /// Tag discriminant table: tag string → u32 id (first-encounter order).
    variant_tags: BTreeMap<String, u32>,
    /// Interned data object IDs for text literals and EffectCall strings.
    pub(crate) data_ids: &'a [DataId],
    /// Layout describing which strings map to which data_ids index.
    pub(crate) data_layout: &'a NativeDataLayout,
    /// Interned data object IDs for `LiteralValue::Bytes` byte buffers.
    ///
    /// Index matches `NativeDataLayout::bytes_table` — entry `i` here is the
    /// `DataId` for `bytes_table[i]`.
    pub(crate) bytes_data_ids: &'a [DataId],
    /// Imported host_call FuncId if the program uses EffectCall.
    pub(crate) host_call_id: Option<FuncId>,
    /// Imported __ail_malloc FuncId for heap allocation of compound values.
    pub(crate) malloc_id: Option<FuncId>,
    /// Imported ail_runtime_call FuncId for concurrency/dispatch/resource ops.
    pub(crate) runtime_call_id: Option<FuncId>,
    /// Counter for generating unique lambda function names.
    pub(crate) next_lambda: u32,
}

impl<'a> NativeCodegenCtx<'a> {
    pub(crate) fn new(
        data_ids: &'a [DataId],
        data_layout: &'a NativeDataLayout,
        bytes_data_ids: &'a [DataId],
        host_call_id: Option<FuncId>,
    ) -> Self {
        Self {
            locals: BTreeMap::new(),
            labels: Vec::new(),
            record_layouts: BTreeMap::new(),
            variant_tags: BTreeMap::new(),
            data_ids,
            data_layout,
            bytes_data_ids,
            host_call_id,
            malloc_id: None,
            runtime_call_id: None,
            next_lambda: 0,
        }
    }

    pub(crate) fn bind(&mut self, name: &str, val: Value, ty: types::Type) {
        self.locals.insert(name.to_string(), (val, ty));
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<(Value, types::Type)> {
        self.locals.get(name).copied()
    }

    pub(crate) fn field_offset(&self, record: &str, field: &str) -> i32 {
        if let Some(fields) = self.record_layouts.get(record) {
            for (i, f) in fields.iter().enumerate() {
                if f == field {
                    return (i * 8) as i32;
                }
            }
        }
        0
    }

    pub(crate) fn assign_tag(&mut self, tag: &str) -> u32 {
        if let Some(&id) = self.variant_tags.get(tag) {
            return id;
        }
        // Use FNV-1a hash of the tag name for a stable, name-dependent discriminant.
        // This ensures the same tag always gets the same ID across compilation units.
        let mut h: u32 = 2166136261;
        for b in tag.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
        self.variant_tags.insert(tag.to_string(), h);
        h
    }

    pub(crate) fn push_label(&mut self, kind: NativeLabelKind, block: Block) {
        self.labels.push((kind, block));
    }

    pub(crate) fn pop_label(&mut self) {
        self.labels.pop();
    }

    pub(crate) fn find_label(&self, kind: NativeLabelKind) -> Option<Block> {
        self.labels
            .iter()
            .rev()
            .find(|(k, _)| *k == kind)
            .map(|(_, b)| *b)
    }
}

// ── infer_cranelift_return_type ───────────────────────────────────────────

/// Infer the Cranelift return type for an `AnfExpr` without compiling it.
///
/// Returns `None` when the expression produces no value (unit, trap stub,
/// or unsupported variant).
pub(crate) fn infer_cranelift_return_type(
    expr: &crate::anf::AnfExpr,
) -> Option<cranelift_codegen::ir::types::Type> {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::types;

    match expr {
        AnfExpr::Literal(LiteralValue::Int(_)) => Some(types::I64),
        AnfExpr::Literal(LiteralValue::Bool(_)) => Some(types::I8),
        AnfExpr::Literal(LiteralValue::Float(_)) => Some(types::F64),
        AnfExpr::Literal(LiteralValue::Text(_)) => Some(types::I64),
        AnfExpr::Literal(LiteralValue::Bytes(_)) => Some(types::I64),
        AnfExpr::Let { body, .. } => infer_cranelift_return_type(body),
        AnfExpr::Return(inner) => infer_cranelift_return_type(inner),
        AnfExpr::Call { func, .. } => match func.as_str() {
            "i64.add" | "+" | "add"
            | "i64.sub" | "-" | "sub"
            | "i64.mul" | "*" | "mul"
            | "i64.div_s" | "/" | "div"
            | "i64.rem_s" | "%" | "mod"
            | "i64.and" | "and"
            | "i64.or" | "or"
            | "i64.neg" | "neg" | "negate"
            | "int.min" | "int_min"
            | "int.max" | "int_max"
            | "int.clamp" | "int_clamp"
            | "int.abs_or" | "int_abs_or"
            | "int.neg_or" | "int_neg_or"
            | "int.add_or" | "int_add_or"
            | "int.sub_or" | "int_sub_or"
            | "int.mul_or" | "int_mul_or"
            | "int.saturating_add" | "int_saturating_add"
            | "int.div_or" | "int_div_or"
            | "int.rem_or" | "int_rem_or" => Some(types::I64),
            "i64.eq" | "==" | "eq"
            | "i64.ne" | "!=" | "ne"
            | "i64.lt_s" | "<" | "lt"
            | "i64.le_s" | "<=" | "le"
            | "i64.gt_s" | ">" | "gt"
            | "i64.ge_s" | ">=" | "ge"
            | "i64.eqz" | "not" | "!"
            => Some(types::I8),
            _ => None,
        },
        AnfExpr::If { then_branch, .. } => infer_cranelift_return_type(then_branch),
        AnfExpr::ShortCircuitAnd { .. } | AnfExpr::ShortCircuitOr { .. } => Some(types::I64),
        AnfExpr::Seq(exprs) => exprs.last().and_then(infer_cranelift_return_type),
        AnfExpr::Loop { body } => infer_cranelift_return_type(body),
        AnfExpr::Break { value } => infer_cranelift_return_type(value),
        AnfExpr::Match { arms, .. } => arms.first()
            .and_then(|a| infer_cranelift_return_type(&a.body)),
        AnfExpr::RecordNew { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::FieldUpdate { .. }
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::TupleNew(_)
        | AnfExpr::EffectCall { .. }
        // ola5 Gap 2/3 — heap-allocated compound types and runtime results
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Lambda { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. } => Some(types::I64),
        _ => None,
    }
}
