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
// - ANF expression → Cranelift IR lowering (`lower_anf_expr_cranelift`)
// - Return-type inference (`infer_cranelift_return_type`)
// - Runtime call emission (`emit_runtime_call`, private)
//
// # Non-responsibilities
//
// - Object module creation       → `native::build_object_module`
// - String interning / data scan → `native_types::NativeDataLayout`
// - Artifact sealing, hash chain → `native::emit_native_with_profile`

use std::collections::BTreeMap;

use cranelift_codegen::{
    Context,
    ir::{
        AbiParam, Function, InstBuilder, MemFlags, Signature, StackSlotData, UserFuncName,
        condcodes::IntCC, stackslot::StackSlotKind,
    },
    isa::CallConv,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;

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
enum NativeLabelKind {
    LoopBreak,
    LoopContinue,
}

// ── NativeCodegenCtx ──────────────────────────────────────────────────────

/// Per-function compilation context for `lower_anf_expr_cranelift`.
pub(crate) struct NativeCodegenCtx<'a> {
    /// Maps ANF let-binding names to their Cranelift SSA `Value` + type.
    /// Uses `String` keys to avoid lifetime complexity with nested expressions.
    pub(crate) locals: BTreeMap<
        String,
        (
            cranelift_codegen::ir::Value,
            cranelift_codegen::ir::types::Type,
        ),
    >,
    /// Label stack for Loop/Break/Continue resolution.
    labels: Vec<(NativeLabelKind, cranelift_codegen::ir::Block)>,
    /// Record layout map: binding name → ordered field names.
    record_layouts: BTreeMap<String, Vec<String>>,
    /// Tag discriminant table: tag string → u32 id (first-encounter order).
    variant_tags: BTreeMap<String, u32>,
    /// Interned data object IDs for text literals and EffectCall strings.
    pub(crate) data_ids: &'a [DataId],
    /// Layout describing which strings map to which data_ids index.
    pub(crate) data_layout: &'a NativeDataLayout,
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
        host_call_id: Option<FuncId>,
    ) -> Self {
        Self {
            locals: BTreeMap::new(),
            labels: Vec::new(),
            record_layouts: BTreeMap::new(),
            variant_tags: BTreeMap::new(),
            data_ids,
            data_layout,
            host_call_id,
            malloc_id: None,
            runtime_call_id: None,
            next_lambda: 0,
        }
    }

    pub(crate) fn bind(
        &mut self,
        name: &str,
        val: cranelift_codegen::ir::Value,
        ty: cranelift_codegen::ir::types::Type,
    ) {
        self.locals.insert(name.to_string(), (val, ty));
    }

    pub(crate) fn lookup(
        &self,
        name: &str,
    ) -> Option<(
        cranelift_codegen::ir::Value,
        cranelift_codegen::ir::types::Type,
    )> {
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

    fn push_label(&mut self, kind: NativeLabelKind, block: cranelift_codegen::ir::Block) {
        self.labels.push((kind, block));
    }

    fn pop_label(&mut self) {
        self.labels.pop();
    }

    fn find_label(&self, kind: NativeLabelKind) -> Option<cranelift_codegen::ir::Block> {
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
            => Some(types::I64),
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

// ── lower_anf_expr_cranelift ──────────────────────────────────────────────

/// Lower one `AnfExpr` into Cranelift IR instructions.
///
/// Returns a [`LowerResult`] indicating whether a value was produced and
/// whether the current basic block has been terminated.
///
/// Supported:
/// - `Literal(Int)` → `iconst(I64, n)` → [`LowerResult::Value`]
/// - `Literal(Bool)` → `iconst(I8, b)` → [`LowerResult::Value`]
/// - `Literal(Float)` → `f64const(f)` → [`LowerResult::Value`]
/// - `Literal(Unit)` → [`LowerResult::Unit`] (no instructions emitted)
/// - `Var(name)` → look up in `ctx.locals`
/// - `Let { name, value, body }` → lower value, bind name, lower body
/// - `Return(inner)` → lower inner, propagate result
/// - `Call { func: "i64.add"|"i64.sub"|"i64.mul", args: [a, b] }` → arithmetic
///
/// All other variants emit `trap(user(1))` → [`LowerResult::Terminated`].
pub(crate) fn lower_anf_expr_cranelift(
    expr: &crate::anf::AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::{TrapCode, types};

    match expr {
        AnfExpr::Literal(lit) => match lit {
            LiteralValue::Int(n) => LowerResult::Value(builder.ins().iconst(types::I64, *n)),
            LiteralValue::Bool(b) => LowerResult::Value(builder.ins().iconst(types::I8, *b as i64)),
            LiteralValue::Float(f) => LowerResult::Value(builder.ins().f64const(*f)),
            LiteralValue::Unit => LowerResult::Unit,
            LiteralValue::Text(s) => {
                let (idx, len) = ctx.data_layout.get(s.as_str());
                if idx >= ctx.data_ids.len() {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                let data_id = ctx.data_ids[idx];
                let gv = module.declare_data_in_func(data_id, builder.func);
                let ptr = builder.ins().symbol_value(types::I64, gv);
                // Pack: (len as i64) << 32 | ptr
                let len_val = builder.ins().iconst(types::I64, len as i64);
                let len_shifted = builder.ins().ishl_imm(len_val, 32);
                let packed = builder.ins().bor(len_shifted, ptr);
                LowerResult::Value(packed)
            }
        },

        AnfExpr::Var(name) => match ctx.lookup(name.as_str()) {
            Some((val, _)) => LowerResult::Value(val),
            None => {
                builder.ins().trap(TrapCode::user(1).unwrap());
                LowerResult::Terminated
            }
        },

        AnfExpr::Let { name, value, body } => {
            // Detect RecordNew to register layout before recursing into body.
            if let AnfExpr::RecordNew { fields } = value.as_ref() {
                ctx.record_layouts.insert(
                    name.clone(),
                    fields.iter().map(|(f, _)| f.clone()).collect(),
                );
            }
            match lower_anf_expr_cranelift(value, ctx, builder, module) {
                LowerResult::Value(val) => {
                    let ty = infer_cranelift_return_type(value)
                        .unwrap_or(cranelift_codegen::ir::types::I64);
                    ctx.bind(name.as_str(), val, ty);
                    lower_anf_expr_cranelift(body, ctx, builder, module)
                }
                LowerResult::Unit => lower_anf_expr_cranelift(body, ctx, builder, module),
                LowerResult::Terminated => LowerResult::Terminated,
            }
        }

        AnfExpr::Return(inner) => lower_anf_expr_cranelift(inner, ctx, builder, module),

        AnfExpr::Call { func, args } => {
            match func.as_str() {
                // ── binary I64 arithmetic ──────────────────────────────
                "i64.add" | "+" | "add" | "i64.sub" | "-" | "sub" | "i64.mul" | "*" | "mul"
                | "i64.div_s" | "/" | "div" | "i64.rem_s" | "%" | "mod" | "i64.and" | "and"
                | "i64.or" | "or"
                    if args.len() == 2 =>
                {
                    let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
                    let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
                    match (lhs, rhs) {
                        (Some(l), Some(r)) => {
                            let val = match func.as_str() {
                                "i64.add" | "+" | "add" => builder.ins().iadd(l, r),
                                "i64.sub" | "-" | "sub" => builder.ins().isub(l, r),
                                "i64.mul" | "*" | "mul" => builder.ins().imul(l, r),
                                "i64.div_s" | "/" | "div" => builder.ins().sdiv(l, r),
                                "i64.rem_s" | "%" | "mod" => builder.ins().srem(l, r),
                                "i64.and" | "and" => builder.ins().band(l, r),
                                "i64.or" | "or" => builder.ins().bor(l, r),
                                _ => unreachable!(),
                            };
                            LowerResult::Value(val)
                        }
                        _ => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                // ── binary comparisons → I8 ────────────────────────────
                "i64.eq" | "==" | "eq" | "i64.ne" | "!=" | "ne" | "i64.lt_s" | "<" | "lt"
                | "i64.le_s" | "<=" | "le" | "i64.gt_s" | ">" | "gt" | "i64.ge_s" | ">=" | "ge"
                    if args.len() == 2 =>
                {
                    let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
                    let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
                    match (lhs, rhs) {
                        (Some(l), Some(r)) => {
                            let cc = match func.as_str() {
                                "i64.eq" | "==" | "eq" => IntCC::Equal,
                                "i64.ne" | "!=" | "ne" => IntCC::NotEqual,
                                "i64.lt_s" | "<" | "lt" => IntCC::SignedLessThan,
                                "i64.le_s" | "<=" | "le" => IntCC::SignedLessThanOrEqual,
                                "i64.gt_s" | ">" | "gt" => IntCC::SignedGreaterThan,
                                "i64.ge_s" | ">=" | "ge" => IntCC::SignedGreaterThanOrEqual,
                                _ => unreachable!(),
                            };
                            LowerResult::Value(builder.ins().icmp(cc, l, r))
                        }
                        _ => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                // ── unary ops ─────────────────────────────────────────
                "i64.neg" | "neg" | "negate" if args.len() == 1 => {
                    match ctx.lookup(args[0].as_str()).map(|(v, _)| v) {
                        Some(a) => LowerResult::Value(builder.ins().ineg(a)),
                        None => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                "i64.eqz" | "not" | "!" if args.len() == 1 => {
                    match ctx.lookup(args[0].as_str()).map(|(v, _)| v) {
                        Some(a) => LowerResult::Value(builder.ins().icmp_imm(IntCC::Equal, a, 0)),
                        None => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }

        // ── Loop ─────────────────────────────────────────────────────────
        AnfExpr::Loop { body } => {
            let break_block = builder.create_block();
            let loop_block = builder.create_block();

            let result_ty = infer_cranelift_return_type(body);
            if let Some(ty) = result_ty {
                builder.append_block_param(break_block, ty);
            }

            // Jump into the loop.
            builder.ins().jump(loop_block, &[]);
            builder.switch_to_block(loop_block);
            // DO NOT seal loop_block yet — back-edges from Continue not emitted.
            ctx.push_label(NativeLabelKind::LoopBreak, break_block);
            ctx.push_label(NativeLabelKind::LoopContinue, loop_block);

            let body_result = lower_anf_expr_cranelift(body, ctx, builder, module);

            ctx.pop_label(); // LoopContinue
            ctx.pop_label(); // LoopBreak

            // Implicit fall-through (no explicit break): jump back to header.
            if !matches!(body_result, LowerResult::Terminated) {
                builder.ins().jump(loop_block, &[]);
            }
            builder.seal_block(loop_block);
            builder.switch_to_block(break_block);
            builder.seal_block(break_block);

            match result_ty {
                Some(_) => LowerResult::Value(builder.block_params(break_block)[0]),
                None => LowerResult::Unit,
            }
        }

        // ── Break ─────────────────────────────────────────────────────────
        AnfExpr::Break { value } => {
            let break_block = match ctx.find_label(NativeLabelKind::LoopBreak) {
                Some(b) => b,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            match lower_anf_expr_cranelift(value, ctx, builder, module) {
                LowerResult::Value(v) => {
                    builder.ins().jump(break_block, &[v]);
                }
                LowerResult::Unit => {
                    builder.ins().jump(break_block, &[]);
                }
                LowerResult::Terminated => {}
            }
            LowerResult::Terminated
        }

        // ── Continue ──────────────────────────────────────────────────────
        AnfExpr::Continue => {
            let loop_block = match ctx.find_label(NativeLabelKind::LoopContinue) {
                Some(b) => b,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            builder.ins().jump(loop_block, &[]);
            LowerResult::Terminated
        }

        // ── WhileLoop ─────────────────────────────────────────────────────
        AnfExpr::WhileLoop { cond, body } => {
            let break_block = builder.create_block();
            let loop_block = builder.create_block();
            let body_block = builder.create_block();

            // Jump into the loop header.
            builder.ins().jump(loop_block, &[]);
            builder.switch_to_block(loop_block);
            // DO NOT seal loop_block yet.

            let cond_val = match ctx.lookup(cond.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            // brif cond → body_block, else → break_block (exit on false)
            builder
                .ins()
                .brif(cond_val, body_block, &[], break_block, &[]);

            builder.switch_to_block(body_block);
            builder.seal_block(body_block);
            ctx.push_label(NativeLabelKind::LoopBreak, break_block);
            ctx.push_label(NativeLabelKind::LoopContinue, loop_block);
            let while_body_result = lower_anf_expr_cranelift(body, ctx, builder, module);
            ctx.pop_label(); // LoopContinue
            ctx.pop_label(); // LoopBreak
            if !matches!(while_body_result, LowerResult::Terminated) {
                builder.ins().jump(loop_block, &[]);
            }

            builder.seal_block(loop_block);
            builder.switch_to_block(break_block);
            builder.seal_block(break_block);
            LowerResult::Unit
        }

        // ── If ────────────────────────────────────────────────────────────
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_val = match ctx.lookup(cond.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();

            let result_ty = infer_cranelift_return_type(then_branch);
            if let Some(ty) = result_ty {
                builder.append_block_param(merge_block, ty);
            }

            builder
                .ins()
                .brif(cond_val, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            match lower_anf_expr_cranelift(then_branch, ctx, builder, module) {
                LowerResult::Value(v) => {
                    builder.ins().jump(merge_block, &[v]);
                }
                LowerResult::Unit => {
                    builder.ins().jump(merge_block, &[]);
                }
                LowerResult::Terminated => {}
            }

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            match lower_anf_expr_cranelift(else_branch, ctx, builder, module) {
                LowerResult::Value(v) => {
                    builder.ins().jump(merge_block, &[v]);
                }
                LowerResult::Unit => {
                    builder.ins().jump(merge_block, &[]);
                }
                LowerResult::Terminated => {}
            }

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            match result_ty {
                Some(_) => LowerResult::Value(builder.block_params(merge_block)[0]),
                None => LowerResult::Unit,
            }
        }

        // ── Match ────────────────────────────────────────────────────────
        AnfExpr::Match { scrutinee, arms } => {
            // Empty arms → trap.
            if arms.is_empty() {
                builder.ins().trap(TrapCode::user(1).unwrap());
                return LowerResult::Terminated;
            }

            let (scrutinee_val, scrutinee_ty) = match ctx.lookup(scrutinee.as_str()) {
                Some(pair) => pair,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            // Normalize scrutinee to I64 for pattern comparisons (extend I8 bools).
            let scrutinee_i64 = if scrutinee_ty == types::I64 {
                scrutinee_val
            } else {
                builder.ins().uextend(types::I64, scrutinee_val)
            };

            let result_ty = infer_cranelift_return_type(&arms[0].body);
            let merge_block = builder.create_block();
            if let Some(ty) = result_ty {
                builder.append_block_param(merge_block, ty);
            }

            // Linear scan through arms. After each non-wildcard check, if the
            // pattern doesn't match we jump to the next check block.
            // The wildcard or last arm terminates the chain.
            let n = arms.len();
            let mut has_merge_predecessor = false;
            for (i, arm) in arms.iter().enumerate() {
                let arm_block = builder.create_block();

                if arm.pattern == "_" {
                    // Wildcard: unconditional jump into arm_block.
                    builder.ins().jump(arm_block, &[]);
                    builder.switch_to_block(arm_block);
                    builder.seal_block(arm_block);
                    // Lower arm body and jump to merge.
                    match lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                        LowerResult::Value(v) => {
                            builder.ins().jump(merge_block, &[v]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Unit => {
                            builder.ins().jump(merge_block, &[]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Terminated => {}
                    }
                    // Wildcard terminates the chain; remaining arms (if any) are unreachable.
                    break;
                }

                // Non-wildcard: compute equality check.
                let pattern_val: i64 = match arm.pattern.trim() {
                    "true" => 1,
                    "false" => 0,
                    s => match s.parse::<i64>() {
                        Ok(value) => value,
                        Err(_) => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            if has_merge_predecessor {
                                break;
                            }
                            return LowerResult::Terminated;
                        }
                    },
                };
                let pat_const = builder.ins().iconst(types::I64, pattern_val);
                let eq = builder.ins().icmp(IntCC::Equal, scrutinee_i64, pat_const);

                if i + 1 < n {
                    // More arms follow: create a next_check block for the false branch.
                    let next_check = builder.create_block();
                    builder.ins().brif(eq, arm_block, &[], next_check, &[]);
                    // Emit arm body in arm_block.
                    builder.switch_to_block(arm_block);
                    builder.seal_block(arm_block);
                    match lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                        LowerResult::Value(v) => {
                            builder.ins().jump(merge_block, &[v]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Unit => {
                            builder.ins().jump(merge_block, &[]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Terminated => {}
                    }
                    // Switch to next_check for the next iteration.
                    builder.switch_to_block(next_check);
                    builder.seal_block(next_check);
                } else {
                    // Last arm and not wildcard: trap if pattern doesn't match.
                    let trap_block = builder.create_block();
                    builder.ins().brif(eq, arm_block, &[], trap_block, &[]);
                    // Trap block.
                    builder.switch_to_block(trap_block);
                    builder.seal_block(trap_block);
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    // Arm block.
                    builder.switch_to_block(arm_block);
                    builder.seal_block(arm_block);
                    match lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                        LowerResult::Value(v) => {
                            builder.ins().jump(merge_block, &[v]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Unit => {
                            builder.ins().jump(merge_block, &[]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Terminated => {}
                    }
                }
            }

            if !has_merge_predecessor {
                return LowerResult::Terminated;
            }

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            match result_ty {
                Some(_) => LowerResult::Value(builder.block_params(merge_block)[0]),
                None => LowerResult::Unit,
            }
        }

        // ── ShortCircuitAnd ───────────────────────────────────────────────
        AnfExpr::ShortCircuitAnd { left, right } => {
            let left_val = match ctx.lookup(left.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let true_block = builder.create_block();
            let false_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, cranelift_codegen::ir::types::I64);

            builder
                .ins()
                .brif(left_val, true_block, &[], false_block, &[]);

            // true branch: evaluate right
            builder.switch_to_block(true_block);
            builder.seal_block(true_block);
            let right_val = match lower_anf_expr_cranelift(right, ctx, builder, module) {
                LowerResult::Value(v) => v,
                _ => builder.ins().iconst(cranelift_codegen::ir::types::I64, 0),
            };
            builder.ins().jump(merge_block, &[right_val]);

            // false branch: short-circuit → 0
            builder.switch_to_block(false_block);
            builder.seal_block(false_block);
            let zero = builder.ins().iconst(cranelift_codegen::ir::types::I64, 0);
            builder.ins().jump(merge_block, &[zero]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            LowerResult::Value(builder.block_params(merge_block)[0])
        }

        // ── Seq ───────────────────────────────────────────────────────────
        AnfExpr::Seq(exprs) => {
            if exprs.is_empty() {
                return LowerResult::Unit;
            }
            let last_idx = exprs.len() - 1;
            for expr in &exprs[..last_idx] {
                // Lower each non-last expression; result is intentionally dropped.
                if let LowerResult::Terminated =
                    lower_anf_expr_cranelift(expr, ctx, builder, module)
                {
                    return LowerResult::Terminated;
                }
            }
            lower_anf_expr_cranelift(&exprs[last_idx], ctx, builder, module)
        }

        // ── RuntimeCheck ──────────────────────────────────────────────────
        AnfExpr::RuntimeCheck { cond, .. } => {
            let cond_val = match ctx.lookup(cond.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let trap_block = builder.create_block();
            let fallthrough = builder.create_block();

            // brif cond → trap_block (trap if cond non-zero), else fallthrough
            builder
                .ins()
                .brif(cond_val, trap_block, &[], fallthrough, &[]);

            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            builder.ins().trap(TrapCode::user(1).unwrap());

            builder.switch_to_block(fallthrough);
            builder.seal_block(fallthrough);
            LowerResult::Unit
        }

        // ── ShortCircuitOr ────────────────────────────────────────────────
        AnfExpr::ShortCircuitOr { left, right } => {
            let left_val = match ctx.lookup(left.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let true_block = builder.create_block();
            let false_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, cranelift_codegen::ir::types::I64);

            builder
                .ins()
                .brif(left_val, true_block, &[], false_block, &[]);

            // true branch: short-circuit → 1
            builder.switch_to_block(true_block);
            builder.seal_block(true_block);
            let one = builder.ins().iconst(cranelift_codegen::ir::types::I64, 1);
            builder.ins().jump(merge_block, &[one]);

            // false branch: evaluate right
            builder.switch_to_block(false_block);
            builder.seal_block(false_block);
            let right_val = match lower_anf_expr_cranelift(right, ctx, builder, module) {
                LowerResult::Value(v) => v,
                _ => builder.ins().iconst(cranelift_codegen::ir::types::I64, 0),
            };
            builder.ins().jump(merge_block, &[right_val]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            LowerResult::Value(builder.block_params(merge_block)[0])
        }

        // ── RecordNew ─────────────────────────────────────────────────────
        // Heap-allocated: pointer survives function return.
        // Layout: [field0: i64, field1: i64, ...] — no length header.
        AnfExpr::RecordNew { fields } => {
            let byte_size = (fields.len().max(1) * 8) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback to stack allocation (dangling pointer — Phase 17 legacy).
                    let size = (fields.len().max(1) * 8) as u32;
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        size,
                        3,
                    ));
                    for (idx, (_, field_expr)) in fields.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(field_expr, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(val, slot, (idx * 8) as i32);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    for (idx, (_, field_expr)) in fields.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(field_expr, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder
                            .ins()
                            .store(MemFlags::trusted(), val, ptr, (idx * 8) as i32);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── FieldGet ──────────────────────────────────────────────────────
        AnfExpr::FieldGet { record, field } => {
            let ptr = match ctx.lookup(record.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let offset = ctx.field_offset(record.as_str(), field.as_str());
            let val = builder
                .ins()
                .load(types::I64, MemFlags::trusted(), ptr, offset);
            LowerResult::Value(val)
        }

        // ── FieldUpdate ───────────────────────────────────────────────────
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            let ptr = match ctx.lookup(record.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let val = match lower_anf_expr_cranelift(value, ctx, builder, module) {
                LowerResult::Value(v) => v,
                _ => builder.ins().iconst(types::I64, 0),
            };
            let offset = ctx.field_offset(record.as_str(), field.as_str());
            builder.ins().store(MemFlags::trusted(), val, ptr, offset);
            LowerResult::Value(ptr)
        }

        // ── VariantNew ────────────────────────────────────────────────────
        // Heap-allocated: 16 bytes: [tag: i64, payload: i64].
        AnfExpr::VariantNew { tag, payload } => {
            let tag_id = ctx.assign_tag(tag.as_str()) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback stack allocation.
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        16,
                        3,
                    ));
                    let tag_val = builder.ins().iconst(types::I64, tag_id);
                    builder.ins().stack_store(tag_val, slot, 0);
                    if let Some(payload_expr) = payload {
                        let pv = match lower_anf_expr_cranelift(payload_expr, ctx, builder, module)
                        {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(pv, slot, 8);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, 16);
                    let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let tag_val = builder.ins().iconst(types::I64, tag_id);
                    builder.ins().store(MemFlags::trusted(), tag_val, ptr, 0);
                    if let Some(payload_expr) = payload {
                        let pv = match lower_anf_expr_cranelift(payload_expr, ctx, builder, module)
                        {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().store(MemFlags::trusted(), pv, ptr, 8);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── ListNew ───────────────────────────────────────────────────────
        // Heap-allocated: [len: i64, elem0: i64, ...].
        AnfExpr::ListNew(elems) => {
            let byte_size = ((1 + elems.len()) * 8) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback stack allocation.
                    let size = ((1 + elems.len()) * 8) as u32;
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        size,
                        3,
                    ));
                    let len_val = builder.ins().iconst(types::I64, elems.len() as i64);
                    builder.ins().stack_store(len_val, slot, 0);
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(val, slot, (8 + i * 8) as i32);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let len_val = builder.ins().iconst(types::I64, elems.len() as i64);
                    builder.ins().store(MemFlags::trusted(), len_val, ptr, 0);
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder
                            .ins()
                            .store(MemFlags::trusted(), val, ptr, (8 + i * 8) as i32);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── TupleNew ──────────────────────────────────────────────────────
        // Heap-allocated: [elem0: i64, elem1: i64, ...].
        AnfExpr::TupleNew(elems) => {
            let byte_size = (elems.len().max(1) * 8) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback stack allocation.
                    let size = (elems.len().max(1) * 8) as u32;
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        size,
                        3,
                    ));
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(val, slot, (i * 8) as i32);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder
                            .ins()
                            .store(MemFlags::trusted(), val, ptr, (i * 8) as i32);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── EffectCall ────────────────────────────────────────────────────
        AnfExpr::EffectCall {
            capability,
            func,
            args,
        } => {
            let host_call_id = match ctx.host_call_id {
                Some(id) => id,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            // Args buffer: store each arg as I64 in a stack slot.
            let args_size = (args.len().max(1) * 8) as u32;
            let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                args_size,
                3,
            ));
            for (idx, arg_name) in args.iter().enumerate() {
                let arg_val = match ctx.lookup(arg_name.as_str()) {
                    Some((v, _)) => v,
                    None => builder.ins().iconst(types::I64, 0),
                };
                builder
                    .ins()
                    .stack_store(arg_val, args_slot, (idx * 8) as i32);
            }
            let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);

            // Capability + func name pointers from data section.
            let (cap_idx, cap_len) = ctx.data_layout.get(capability.as_str());
            let (op_idx, op_len) = ctx.data_layout.get(func.as_str());
            let cap_gv = module.declare_data_in_func(ctx.data_ids[cap_idx], builder.func);
            let op_gv = module.declare_data_in_func(ctx.data_ids[op_idx], builder.func);
            let cap_ptr = builder.ins().symbol_value(types::I64, cap_gv);
            let op_ptr = builder.ins().symbol_value(types::I64, op_gv);

            let host_call_ref = module.declare_func_in_func(host_call_id, builder.func);
            let call_args = [
                cap_ptr,
                builder.ins().iconst(types::I64, cap_len as i64),
                op_ptr,
                builder.ins().iconst(types::I64, op_len as i64),
                args_ptr,
                builder.ins().iconst(types::I64, args.len() as i64),
            ];
            let call = builder.ins().call(host_call_ref, &call_args);
            LowerResult::Value(builder.inst_results(call)[0])
        }

        // ── Assume ───────────────────────────────────────────────────────
        // No runtime effect — a proof assumption is purely a static annotation.
        AnfExpr::Assume { .. } => LowerResult::Unit,

        // ── Abort ─────────────────────────────────────────────────────────
        // Always traps — represents an impossible branch (Never type).
        AnfExpr::Abort { .. } => {
            builder.ins().trap(TrapCode::user(2).unwrap());
            LowerResult::Terminated
        }

        // ── IndexGet ─────────────────────────────────────────────────────
        // Load element at `index` from a length-prefixed list/map/set.
        // Collection layout: [len: i64, elem0: i64, elem1: i64, ...]
        // Element offset = 8 + index * 8.
        AnfExpr::IndexGet { collection, index } => {
            let col_ptr = match ctx.lookup(collection.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let idx_val = match ctx.lookup(index.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            // offset = 8 + idx * 8
            let eight = builder.ins().iconst(types::I64, 8);
            let idx_scaled = builder.ins().imul(idx_val, eight);
            let offset = builder.ins().iadd(idx_scaled, eight);
            let addr = builder.ins().iadd(col_ptr, offset);
            let val = builder.ins().load(types::I64, MemFlags::trusted(), addr, 0);
            LowerResult::Value(val)
        }

        // ── MapNew ────────────────────────────────────────────────────────
        // Heap-allocate a map: [count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...]
        AnfExpr::MapNew { entries } => {
            let byte_size = (1 + entries.len() * 2) as i64 * 8;
            match ctx.malloc_id {
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    // Store count
                    let count = builder.ins().iconst(types::I64, entries.len() as i64);
                    builder.ins().store(MemFlags::trusted(), count, ptr, 0);
                    // Store key-value pairs
                    for (i, (k_name, v_name)) in entries.iter().enumerate() {
                        let base = (1 + i * 2) as i32 * 8;
                        let k_val = ctx
                            .lookup(k_name.as_str())
                            .map(|(v, _)| v)
                            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        let v_val = ctx
                            .lookup(v_name.as_str())
                            .map(|(v, _)| v)
                            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        builder.ins().store(MemFlags::trusted(), k_val, ptr, base);
                        builder
                            .ins()
                            .store(MemFlags::trusted(), v_val, ptr, base + 8);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── SetNew ────────────────────────────────────────────────────────
        // Heap-allocate a set: [count: i64, elem0: i64, elem1: i64, ...]
        AnfExpr::SetNew { elements } => {
            let byte_size = (1 + elements.len()) as i64 * 8;
            match ctx.malloc_id {
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let count = builder.ins().iconst(types::I64, elements.len() as i64);
                    builder.ins().store(MemFlags::trusted(), count, ptr, 0);
                    for (i, elem_name) in elements.iter().enumerate() {
                        let offset = (1 + i) as i32 * 8;
                        let val = ctx
                            .lookup(elem_name.as_str())
                            .map(|(v, _)| v)
                            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        builder.ins().store(MemFlags::trusted(), val, ptr, offset);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── ForEach ───────────────────────────────────────────────────────
        // Loop over a length-prefixed list collection.
        // Layout: [len: i64, elem0: i64, ...]
        // Generates: counter from 0 to len-1, binding each element into body.
        AnfExpr::ForEach {
            binding,
            collection,
            body,
        } => {
            let col_ptr = match ctx.lookup(collection.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            // Load length
            let len_val = builder
                .ins()
                .load(types::I64, MemFlags::trusted(), col_ptr, 0);

            // Allocate a stack slot for the loop counter.
            let counter_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                3,
            ));
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().stack_store(zero, counter_slot, 0);

            let break_block = builder.create_block();
            let loop_block = builder.create_block();
            let body_block = builder.create_block();

            builder.ins().jump(loop_block, &[]);
            builder.switch_to_block(loop_block);
            // Loop header: check counter < len
            let counter = builder.ins().stack_load(types::I64, counter_slot, 0);
            let cond = builder.ins().icmp(IntCC::SignedLessThan, counter, len_val);
            builder.ins().brif(cond, body_block, &[], break_block, &[]);

            builder.switch_to_block(body_block);
            builder.seal_block(body_block);
            // Load element: ptr + 8 + counter * 8
            let eight = builder.ins().iconst(types::I64, 8);
            let offset = builder.ins().imul(counter, eight);
            let inner_sum = builder.ins().iadd(offset, eight);
            let elem_addr = builder.ins().iadd(col_ptr, inner_sum);
            let elem_val = builder
                .ins()
                .load(types::I64, MemFlags::trusted(), elem_addr, 0);
            // Bind the element to the loop variable.
            ctx.bind(binding.as_str(), elem_val, types::I64);
            // Lower body (result discarded — ForEach is for side effects).
            let body_result = lower_anf_expr_cranelift(body, ctx, builder, module);
            if !matches!(body_result, LowerResult::Terminated) {
                // Increment counter and jump back.
                let one = builder.ins().iconst(types::I64, 1);
                let next = builder.ins().iadd(counter, one);
                builder.ins().stack_store(next, counter_slot, 0);
                builder.ins().jump(loop_block, &[]);
            }
            builder.seal_block(loop_block);

            builder.switch_to_block(break_block);
            builder.seal_block(break_block);
            LowerResult::Unit
        }

        // ── Fold ──────────────────────────────────────────────────────────
        // Left fold over a length-prefixed list using an I64 function pointer.
        // Signature expected for func: (acc: I64, elem: I64) -> I64.
        // CFG: entry → loop_hdr ←→ body_blk → exit_blk.
        AnfExpr::Fold { init, list, func } => {
            let init_val = match ctx.lookup(init.as_str()) {
                Some((v, _)) => v,
                None => builder.ins().iconst(types::I64, 0),
            };
            let list_ptr = match ctx.lookup(list.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let func_ptr = match ctx.lookup(func.as_str()) {
                Some((v, _)) => v,
                // No func pointer — return init unchanged (degenerate fold).
                None => return LowerResult::Value(init_val),
            };

            // Load list length once in the entry block.
            let len_val = builder
                .ins()
                .load(types::I64, MemFlags::trusted(), list_ptr, 0);

            // Use stack slots for mutable accumulator and counter.
            let acc_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                3,
            ));
            let ctr_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                3,
            ));
            builder.ins().stack_store(init_val, acc_slot, 0);
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().stack_store(zero, ctr_slot, 0);

            let loop_hdr = builder.create_block();
            let body_blk = builder.create_block();
            let exit_blk = builder.create_block();

            // Entry → loop_hdr.
            builder.ins().jump(loop_hdr, &[]);

            // ── loop_hdr (NOT sealed yet — back-edge from body_blk pending) ──
            builder.switch_to_block(loop_hdr);
            let ctr = builder.ins().stack_load(types::I64, ctr_slot, 0);
            let cond = builder.ins().icmp(IntCC::SignedLessThan, ctr, len_val);
            builder.ins().brif(cond, body_blk, &[], exit_blk, &[]);
            // Seal body_blk: its only predecessor is loop_hdr (brif-true edge).
            builder.seal_block(body_blk);
            // Seal exit_blk: its only predecessor is loop_hdr (brif-false edge).
            builder.seal_block(exit_blk);

            // ── body_blk ──────────────────────────────────────────────────
            builder.switch_to_block(body_blk);
            let acc = builder.ins().stack_load(types::I64, acc_slot, 0);
            let ctr2 = builder.ins().stack_load(types::I64, ctr_slot, 0);
            let eight = builder.ins().iconst(types::I64, 8);
            let elem_off = builder.ins().imul(ctr2, eight);
            let elem_base = builder.ins().iadd(elem_off, eight);
            let elem_addr = builder.ins().iadd(list_ptr, elem_base);
            let elem = builder
                .ins()
                .load(types::I64, MemFlags::trusted(), elem_addr, 0);
            // Indirect call: func_ptr(acc, elem) → I64.
            let fold_sig = {
                let mut sig = Signature::new(cranelift_codegen::isa::CallConv::SystemV);
                sig.params.push(AbiParam::new(types::I64));
                sig.params.push(AbiParam::new(types::I64));
                sig.returns.push(AbiParam::new(types::I64));
                builder.import_signature(sig)
            };
            let indirect_call = builder
                .ins()
                .call_indirect(fold_sig, func_ptr, &[acc, elem]);
            let new_acc = builder.inst_results(indirect_call)[0];
            builder.ins().stack_store(new_acc, acc_slot, 0);
            let one = builder.ins().iconst(types::I64, 1);
            let next_ctr = builder.ins().iadd(ctr2, one);
            builder.ins().stack_store(next_ctr, ctr_slot, 0);
            builder.ins().jump(loop_hdr, &[]);
            // Now seal loop_hdr: all predecessors are known (entry + body_blk).
            builder.seal_block(loop_hdr);

            // ── exit_blk ──────────────────────────────────────────────────
            builder.switch_to_block(exit_blk);
            let acc_final = builder.ins().stack_load(types::I64, acc_slot, 0);
            LowerResult::Value(acc_final)
        }

        // ── CellNew ───────────────────────────────────────────────────────
        // Heap-allocate an 8-byte mutable cell, store init value, return ptr.
        AnfExpr::CellNew { init } => match ctx.malloc_id {
            None => {
                builder.ins().trap(TrapCode::user(1).unwrap());
                LowerResult::Terminated
            }
            Some(malloc_id) => {
                let size_val = builder.ins().iconst(types::I64, 8);
                let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                let call = builder.ins().call(malloc_ref, &[size_val]);
                let ptr = builder.inst_results(call)[0];
                let init_val = ctx
                    .lookup(init.as_str())
                    .map(|(v, _)| v)
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                builder.ins().store(MemFlags::trusted(), init_val, ptr, 0);
                LowerResult::Value(ptr)
            }
        },

        // ── CellGet ───────────────────────────────────────────────────────
        // Load the I64 value stored in a heap-allocated cell.
        AnfExpr::CellGet { cell } => {
            let ptr = match ctx.lookup(cell.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let val = builder.ins().load(types::I64, MemFlags::trusted(), ptr, 0);
            LowerResult::Value(val)
        }

        // ── CellSet ───────────────────────────────────────────────────────
        // Store a new I64 value into a heap-allocated cell.
        AnfExpr::CellSet { cell, value } => {
            let ptr = match ctx.lookup(cell.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let val = ctx
                .lookup(value.as_str())
                .map(|(v, _)| v)
                .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
            builder.ins().store(MemFlags::trusted(), val, ptr, 0);
            LowerResult::Unit
        }

        // ── Lambda ────────────────────────────────────────────────────────
        // Define a nested function for the lambda body, return its address.
        // Params are bound as I64 arguments; captures are not supported yet.
        AnfExpr::Lambda { params, body } => {
            let lambda_name = format!("__ail_lambda_{}", ctx.next_lambda);
            ctx.next_lambda += 1;

            // Build signature: (param0: I64, param1: I64, ...) -> I64
            let mut lambda_sig = Signature::new(CallConv::SystemV);
            for _ in params {
                lambda_sig.params.push(AbiParam::new(types::I64));
            }
            // Return type inferred from body.
            let body_ret_ty = infer_cranelift_return_type(body);
            if let Some(ty) = body_ret_ty {
                lambda_sig.returns.push(AbiParam::new(ty));
            }

            let lambda_id = match module.declare_function(&lambda_name, Linkage::Local, &lambda_sig)
            {
                Ok(id) => id,
                Err(_) => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            // Build and define the lambda body.
            {
                let mut lam_func = Function::with_name_signature(
                    UserFuncName::user(0, lambda_id.as_u32()),
                    lambda_sig,
                );
                let mut lam_fn_ctx = FunctionBuilderContext::new();
                let mut lam_builder = FunctionBuilder::new(&mut lam_func, &mut lam_fn_ctx);
                let lam_block = lam_builder.create_block();
                lam_builder.append_block_params_for_function_params(lam_block);
                lam_builder.switch_to_block(lam_block);
                lam_builder.seal_block(lam_block);

                // Bind params to local names.
                let mut lam_ctx =
                    NativeCodegenCtx::new(ctx.data_ids, ctx.data_layout, ctx.host_call_id);
                lam_ctx.malloc_id = ctx.malloc_id;
                lam_ctx.runtime_call_id = ctx.runtime_call_id;
                for (i, param_name) in params.iter().enumerate() {
                    let param_val = lam_builder.block_params(lam_block)[i];
                    lam_ctx.bind(param_name.as_str(), param_val, types::I64);
                }

                match lower_anf_expr_cranelift(body, &mut lam_ctx, &mut lam_builder, module) {
                    LowerResult::Value(v) => {
                        lam_builder.ins().return_(&[v]);
                    }
                    LowerResult::Unit => {
                        lam_builder.ins().return_(&[]);
                    }
                    LowerResult::Terminated => {}
                }
                lam_builder.finalize();

                let mut lam_codegen = Context::for_function(lam_func);
                if module.define_function(lambda_id, &mut lam_codegen).is_err() {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            }

            // Return the function's address as I64.
            let func_ref = module.declare_func_in_func(lambda_id, builder.func);
            let ptr = builder.ins().func_addr(types::I64, func_ref);
            LowerResult::Value(ptr)
        }

        // ── TaskSpawn / TaskAwait / TaskCancel / TaskGroup ────────────────
        // ── ChannelNew / ChannelSend / ChannelReceive / Select / Timeout ──
        // ── Dispatch / ResourceAcquire / ResourceRelease ──────────────────
        // All concurrency and resource primitives are routed through the
        // imported `ail_runtime_call` function for runtime dispatch.
        AnfExpr::TaskSpawn { func, args } => {
            emit_runtime_call(ctx, builder, module, 1u64, &[func.as_str()], args)
        }
        AnfExpr::TaskAwait { task } => {
            emit_runtime_call(ctx, builder, module, 2u64, &[], std::slice::from_ref(task))
        }
        AnfExpr::TaskCancel { task } => {
            emit_runtime_call(ctx, builder, module, 3u64, &[], std::slice::from_ref(task))
        }
        AnfExpr::TaskGroup { body } => {
            // TaskGroup: lower body for side effects, then emit runtime notification.
            let _ = lower_anf_expr_cranelift(body, ctx, builder, module);
            emit_runtime_call(ctx, builder, module, 4u64, &[], &[])
        }
        AnfExpr::ChannelNew { capacity } => {
            let cap_val = capacity.unwrap_or(0) as i64;
            let cap_name = format!("__cap_{cap_val}");
            // Encode capacity as a synthetic arg: store in stack slot, pass ptr.
            let cap_iconst = builder.ins().iconst(types::I64, cap_val);
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                3,
            ));
            builder.ins().stack_store(cap_iconst, slot, 0);
            let _ = cap_name;
            emit_runtime_call(ctx, builder, module, 5u64, &[], &[])
        }
        AnfExpr::ChannelSend { channel, value } => emit_runtime_call(
            ctx,
            builder,
            module,
            6u64,
            &[],
            &[channel.clone(), value.clone()],
        ),
        AnfExpr::ChannelReceive { channel } => emit_runtime_call(
            ctx,
            builder,
            module,
            7u64,
            &[],
            std::slice::from_ref(channel),
        ),
        AnfExpr::Select { .. } => emit_runtime_call(ctx, builder, module, 8u64, &[], &[]),
        AnfExpr::Timeout { duration, body } => {
            let _ = lower_anf_expr_cranelift(body, ctx, builder, module);
            emit_runtime_call(
                ctx,
                builder,
                module,
                9u64,
                &[],
                std::slice::from_ref(duration),
            )
        }
        AnfExpr::Dispatch {
            handler,
            method,
            args,
        } => {
            let mut all_args: Vec<String> = vec![handler.clone(), method.clone()];
            all_args.extend(args.iter().cloned());
            emit_runtime_call(ctx, builder, module, 10u64, &[], &all_args)
        }
        AnfExpr::ResourceAcquire { resource, args } => {
            let mut all_args: Vec<String> = vec![resource.clone()];
            all_args.extend(args.iter().cloned());
            emit_runtime_call(ctx, builder, module, 11u64, &[], &all_args)
        }
        AnfExpr::ResourceRelease { handle } => emit_runtime_call(
            ctx,
            builder,
            module,
            12u64,
            &[],
            std::slice::from_ref(handle),
        ),

        _ => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

// ── emit_runtime_call ─────────────────────────────────────────────────────

/// Emit an `ail_runtime_call(op, args_ptr, args_len) -> I64` call.
///
/// `op` is the operation discriminant (1 = TaskSpawn, 2 = TaskAwait, ...).
/// `name_args` are string-keyed args (looked up from data section).
/// `var_args` are variable-name args (looked up from ctx.locals).
fn emit_runtime_call(
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
    op: u64,
    _name_args: &[&str],
    var_args: &[String],
) -> LowerResult {
    use cranelift_codegen::ir::{TrapCode, types};

    let runtime_id = match ctx.runtime_call_id {
        Some(id) => id,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };

    // Build args buffer on the stack.
    let args_count = var_args.len();
    let buf_size = ((args_count + 1).max(1) * 8) as u32;
    let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        buf_size,
        3,
    ));
    for (i, arg_name) in var_args.iter().enumerate() {
        let val = ctx
            .lookup(arg_name.as_str())
            .map(|(v, _)| v)
            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
        builder.ins().stack_store(val, args_slot, (i * 8) as i32);
    }
    let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
    let op_val = builder.ins().iconst(types::I64, op as i64);
    let args_len = builder.ins().iconst(types::I64, args_count as i64);

    let runtime_ref = module.declare_func_in_func(runtime_id, builder.func);
    let call = builder
        .ins()
        .call(runtime_ref, &[op_val, args_ptr, args_len]);
    LowerResult::Value(builder.inst_results(call)[0])
}
