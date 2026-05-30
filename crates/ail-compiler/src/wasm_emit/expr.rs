use super::control::*;
use super::int::*;
use super::text::*;
use super::*;

// ── emit_anf_expr ─────────────────────────────────────────────────────────

/// Emit WASM instructions for one `AnfExpr` into `insns`.
///
/// The emitted sequence leaves exactly one value on the WASM operand stack
/// for value-producing expressions, or zero for effect-only statements.
/// The caller is responsible for consuming (or dropping) that value.
///
/// Locals in `ctx` map ANF names to WASM local indices; new `Let` bindings
/// allocate fresh slots via `ctx.bind`.
pub(super) fn emit_anf_expr<'a>(
    expr: &'a AnfExpr,
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    match expr {
        // ── Literals ──────────────────────────────────────────────────────
        AnfExpr::Literal(lit) => match lit {
            LiteralValue::Int(n) => {
                insns.push(Instruction::I64Const(*n));
                Some(ValType::I64)
            }
            LiteralValue::Bool(b) => {
                insns.push(Instruction::I64Const(if *b { 1 } else { 0 }));
                Some(ValType::I64)
            }
            LiteralValue::Float(f) => {
                // wasm_encoder 0.244 requires Ieee64 for F64Const.
                insns.push(Instruction::F64Const(wasm_encoder::Ieee64::from(*f)));
                Some(ValType::F64)
            }
            LiteralValue::Text(s) => {
                // Encode as: i64 = (len as u64) << 32 | (ptr as u64)
                let (ptr, len) = ctx.effect_data.string(s);
                let packed = ((len as i64) << 32) | (ptr as i64);
                insns.push(Instruction::I64Const(packed));
                Some(ValType::I64)
            }
            LiteralValue::Bytes(data) => {
                // Same packed encoding as Text: upper 32 = len, lower 32 = ptr.
                // The runtime decodes this via ValueLayout::Bytes →
                // StructuredValue::Bytes { ptr, len } with no UTF-8 assumption.
                let (ptr, len) = ctx.effect_data.bytes(data);
                let packed = ((len as i64) << 32) | (ptr as i64);
                insns.push(Instruction::I64Const(packed));
                Some(ValType::I64)
            }
            LiteralValue::Unit => {
                insns.push(Instruction::I32Const(0));
                Some(ValType::I32)
            }
        },

        // ── Variable reference ────────────────────────────────────────────
        AnfExpr::Var(name) => {
            if let Some((idx, ty)) = ctx.lookup(name) {
                insns.push(Instruction::LocalGet(idx));
                Some(ty)
            } else {
                // Unbound variable — emit unreachable (catches missing bindings).
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── Let binding ───────────────────────────────────────────────────
        AnfExpr::Let { name, value, body } => {
            // Emit value expression (leaves one value on stack).
            let value_ty = emit_anf_expr(value, ctx, functions, insns).unwrap_or(ValType::I32);
            // Allocate a fresh local and set it.
            let idx = ctx.bind(name, value_ty);
            insns.push(Instruction::LocalSet(idx));
            if let Some(fields) = record_layout_fields(value) {
                ctx.bind_record_layout(name, fields);
            }
            // Emit the body with the new binding in scope.
            emit_anf_expr(body, ctx, functions, insns)
        }

        // ── Conditional (short-circuit AND/OR) ────────────────────────────
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            // Condition: look up the atomic variable.
            emit_condition_get(ctx, cond, insns);
            let result_ty = ctx
                .expr_type(then_branch)
                .filter(|ty| Some(*ty) == ctx.expr_type(else_branch));
            insns.push(Instruction::If(block_type(result_ty)));
            ctx.labels.push(LabelKind::Other);
            emit_branch_expr(then_branch, result_ty, ctx, functions, insns);
            insns.push(Instruction::Else);
            emit_branch_expr(else_branch, result_ty, ctx, functions, insns);
            ctx.labels.pop();
            insns.push(Instruction::End);
            result_ty
        }

        // ── Short-circuit AND ─────────────────────────────────────────────
        // if left { right } else { false }
        AnfExpr::ShortCircuitAnd { left, right } => {
            emit_condition_get(ctx, left, insns);
            insns.push(Instruction::If(BlockType::Result(ValType::I64)));
            ctx.labels.push(LabelKind::Other);
            emit_anf_expr(right, ctx, functions, insns);
            insns.push(Instruction::Else);
            insns.push(Instruction::I64Const(0));
            ctx.labels.pop();
            insns.push(Instruction::End);
            Some(ValType::I64)
        }

        // ── Short-circuit OR ──────────────────────────────────────────────
        // if left { true } else { right }
        AnfExpr::ShortCircuitOr { left, right } => {
            emit_condition_get(ctx, left, insns);
            insns.push(Instruction::If(BlockType::Result(ValType::I64)));
            ctx.labels.push(LabelKind::Other);
            insns.push(Instruction::I64Const(1));
            insns.push(Instruction::Else);
            emit_anf_expr(right, ctx, functions, insns);
            ctx.labels.pop();
            insns.push(Instruction::End);
            Some(ValType::I64)
        }

        AnfExpr::Loop { body } => {
            let result_ty = ctx.expr_type(body);
            insns.push(Instruction::Block(block_type(result_ty)));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);
            let emitted_ty = emit_anf_expr(body, ctx, functions, insns);
            if result_ty.is_none() && emitted_ty.is_some() {
                insns.push(Instruction::Drop);
            }
            insns.push(Instruction::Br(1));
            ctx.labels.pop();
            insns.push(Instruction::End);
            if result_ty.is_some() {
                insns.push(Instruction::Unreachable);
            }
            ctx.labels.pop();
            insns.push(Instruction::End);
            result_ty
        }

        AnfExpr::Break { value } => {
            emit_anf_expr(value, ctx, functions, insns);
            if let Some(depth) = ctx.branch_depth(LabelKind::LoopBreak) {
                insns.push(Instruction::Br(depth));
            } else {
                insns.push(Instruction::Unreachable);
            }
            None
        }

        AnfExpr::Continue => {
            if let Some(depth) = ctx.branch_depth(LabelKind::LoopContinue) {
                insns.push(Instruction::Br(depth));
            } else {
                insns.push(Instruction::Unreachable);
            }
            None
        }

        AnfExpr::WhileLoop { cond, body } => {
            // Direct ANF while-loop.  `cond` must be an immutable ANF-local
            // binding (a `String` name).  `emit_condition_get` reads it via a
            // single `local.get` on every iteration — the local may be an I32
            // Bool (used directly) or an I64 truthy value (reduced via
            // `i64.ne 0`).  It does NOT re-evaluate a computed expression.
            // Callers that need per-iteration re-evaluation of a computed
            // condition should use
            // `CoreExpr::WhileLoop`, which the lowering pipeline desugars into
            // `Loop + If + Break/Continue` with the condition lowered inside
            // the loop body.  See `AnfExpr::WhileLoop` doc and the
            // `CoreExpr::WhileLoop` arm in `lower/lower_expr.rs` for details.
            insns.push(Instruction::Block(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);
            emit_condition_get(ctx, cond, insns);
            insns.push(Instruction::I32Eqz);
            insns.push(Instruction::BrIf(1));
            let emitted_ty = emit_anf_expr(body, ctx, functions, insns);
            if emitted_ty.is_some() {
                insns.push(Instruction::Drop);
            }
            insns.push(Instruction::Br(0));
            ctx.labels.pop();
            insns.push(Instruction::End);
            ctx.labels.pop();
            insns.push(Instruction::End);
            // WhileLoop is side-effect only in terms of semantics, but it must
            // produce a unit value on the WASM stack so that it can appear as
            // the `value` in an `AnfExpr::Let` binding or as an intermediate
            // element in a `Seq` without causing a stack-underflow validation
            // error.  Push I32 0 (unit) here — mirrors the ForEach fix (Wave 18B).
            insns.push(Instruction::I32Const(0));
            Some(ValType::I32)
        }

        // ── Sequence ──────────────────────────────────────────────────────
        AnfExpr::Seq(exprs) => {
            let mut last_ty = Some(ValType::I32);
            for (i, e) in exprs.iter().enumerate() {
                last_ty = emit_anf_expr(e, ctx, functions, insns);
                // Drop intermediate results (all but the last).
                // Only emit Drop when the element actually produced a value —
                // expressions that return None (e.g. Loop with no result, Break,
                // Continue) leave nothing on the stack and must not be dropped.
                if i + 1 < exprs.len() && last_ty.is_some() {
                    insns.push(Instruction::Drop);
                }
            }
            // Empty Seq → push unit (i32.const 0).
            if exprs.is_empty() {
                insns.push(Instruction::I32Const(0));
            }
            last_ty
        }

        // ── Return ────────────────────────────────────────────────────────
        AnfExpr::Return(inner) => {
            emit_anf_expr(inner, ctx, functions, insns);
            insns.push(Instruction::Return);
            None
        }

        // ── Function call (pure) ──────────────────────────────────────────
        // Emits args via local.get, then calls the function.
        AnfExpr::Call { func, args } => {
            if matches!(func.as_str(), "len" | "text.len") && args.len() == 1 {
                if emit_list_len_from_local(ctx, &args[0], insns) {
                    return Some(ValType::I64);
                }
                emit_text_len_from_local(ctx, &args[0], insns);
                insns.push(Instruction::I64ExtendI32U);
                return Some(ValType::I64);
            }
            if matches!(func.as_str(), "int.min" | "int_min") {
                return emit_int_min(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.max" | "int_max") {
                return emit_int_max(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.clamp" | "int_clamp") {
                return emit_int_clamp(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.abs_or" | "int_abs_or") {
                return emit_int_abs_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.neg_or" | "int_neg_or") {
                return emit_int_neg_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.saturating_neg" | "int_saturating_neg") {
                return emit_int_saturating_neg(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.wrapping_add" | "int_wrapping_add") {
                return emit_int_wrapping_add(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.wrapping_sub" | "int_wrapping_sub") {
                return emit_int_wrapping_sub(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.wrapping_mul" | "int_wrapping_mul") {
                return emit_int_wrapping_mul(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.wrapping_neg" | "int_wrapping_neg") {
                return emit_int_wrapping_neg(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.shift_left" | "int_shift_left") {
                return emit_int_shift_left(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.shift_right" | "int_shift_right") {
                return emit_int_shift_right(args, ctx, insns);
            }
            if matches!(
                func.as_str(),
                "int.shift_right_unsigned" | "int_shift_right_unsigned"
            ) {
                return emit_int_shift_right_unsigned(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.add_or" | "int_add_or") {
                return emit_int_add_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.sub_or" | "int_sub_or") {
                return emit_int_sub_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.mul_or" | "int_mul_or") {
                return emit_int_mul_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.saturating_add" | "int_saturating_add") {
                return emit_int_saturating_add(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.saturating_sub" | "int_saturating_sub") {
                return emit_int_saturating_sub(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.saturating_mul" | "int_saturating_mul") {
                return emit_int_saturating_mul(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.div_or" | "int_div_or") {
                return emit_int_div_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "int.rem_or" | "int_rem_or") {
                return emit_int_rem_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "concat" | "text.concat") {
                return emit_text_concat(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.trim" | "text_trim") {
                return emit_text_trim(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.byte_at_or" | "text_byte_at_or") {
                return emit_text_byte_at_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.parse_int_or" | "text_parse_int_or") {
                return emit_text_parse_int_or(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.slice" | "text_slice") {
                return emit_text_slice(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.replace_first" | "text_replace_first") {
                return emit_text_replace_first(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.eq" | "text_eq") {
                return emit_text_eq(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.contains" | "text_contains") {
                return emit_text_contains(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.index_of" | "text_index_of") {
                return emit_text_index_of(args, ctx, insns);
            }
            if matches!(func.as_str(), "text.starts_with" | "text_starts_with") {
                return emit_text_boundary_match(args, ctx, insns, false);
            }
            if matches!(func.as_str(), "text.ends_with" | "text_ends_with") {
                return emit_text_boundary_match(args, ctx, insns, true);
            }

            for arg_name in args {
                if let Some((idx, _)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(idx));
                } else {
                    insns.push(Instruction::Unreachable);
                    return None;
                }
            }
            if let Some(ty) = emit_i64_primitive_call(func, args.len(), insns) {
                Some(ty)
            } else if let Some(idx) = functions.get(func) {
                insns.push(Instruction::Call(*idx));
                Some(ValType::I64)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        AnfExpr::EffectCall {
            capability,
            func,
            args,
        } => {
            for (idx, arg_name) in args.iter().enumerate() {
                insns.push(Instruction::I32Const(
                    ctx.effect_data.args_offset + (idx as i32 * 8),
                ));
                if let Some((local_idx, arg_ty)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(local_idx));
                    // Zero-extend I32 args to I64 before storing in the args buffer.
                    // I64 args are already the right width and need no extension.
                    if arg_ty == ValType::I32 {
                        insns.push(Instruction::I64ExtendI32U);
                    }
                    insns.push(Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                } else {
                    insns.push(Instruction::Unreachable);
                    return None;
                }
            }

            let (cap_ptr, cap_len) = ctx.effect_data.string(capability);
            let (op_ptr, op_len) = ctx.effect_data.string(func);
            insns.push(Instruction::I32Const(cap_ptr));
            insns.push(Instruction::I32Const(cap_len));
            insns.push(Instruction::I32Const(op_ptr));
            insns.push(Instruction::I32Const(op_len));
            insns.push(Instruction::I32Const(ctx.effect_data.args_offset));
            insns.push(Instruction::I32Const(args.len() as i32));

            if ctx.effect_data.needs_host_call_write {
                // host_call_write: (cap, op, args, out_ptr, out_max) → i32
                // Function index 1 (after host_call at 0).
                insns.push(Instruction::I32Const(ctx.effect_data.result_buffer_offset));
                insns.push(Instruction::I32Const(RESULT_BUFFER_MAX));
                insns.push(Instruction::Call(1));
                // Extend the i32 return to i64 to match the standard EffectCall return type.
                insns.push(Instruction::I64ExtendI32S);
            } else {
                insns.push(Instruction::Call(0));
            }
            Some(ValType::I64)
        }

        // ── FieldGet ──────────────────────────────────────────────────────
        AnfExpr::FieldGet { record, field } => {
            if let Some((idx, _)) = ctx.lookup(record) {
                insns.push(Instruction::LocalGet(idx));
                load_i64_at(ctx.field_offset(record, field), insns);
                Some(ValType::I64)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── FieldUpdate ───────────────────────────────────────────────────
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            let Some((idx, _)) = ctx.lookup(record) else {
                insns.push(Instruction::Unreachable);
                return None;
            };
            insns.push(Instruction::LocalGet(idx));
            emit_i64_value(value, ctx, functions, insns);
            store_i64_at(ctx.field_offset(record, field), insns);
            if let Some((idx, ty)) = ctx.lookup(record) {
                insns.push(Instruction::LocalGet(idx));
                Some(ty)
            } else {
                None
            }
        }

        // ── RecordNew ─────────────────────────────────────────────────────
        AnfExpr::RecordNew { fields } => {
            emit_alloc((fields.len() * 8).max(1) as i32, insns);
            let ptr = ctx.bind("__record_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            for (idx, (_, v)) in fields.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(v, ctx, functions, insns);
                store_i64_at((idx * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── TupleNew ─────────────────────────────────────────────────────
        AnfExpr::TupleNew(elems) => {
            emit_alloc((elems.len() * 8).max(1) as i32, insns);
            let ptr = ctx.bind("__tuple_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            for (idx, e) in elems.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(e, ctx, functions, insns);
                store_i64_at((idx * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── VariantNew ───────────────────────────────────────────────────
        AnfExpr::VariantNew { tag, payload } => {
            emit_alloc(16, insns);
            let ptr = ctx.bind("__variant_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            insns.push(Instruction::LocalGet(ptr));
            let tag_id = ctx.assign_tag(tag) as i32;
            insns.push(Instruction::I32Const(tag_id));
            insns.push(Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
            if let Some(p) = payload {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(p, ctx, functions, insns);
                store_i64_at(8, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── ListNew ──────────────────────────────────────────────────────
        AnfExpr::ListNew(elems) => {
            emit_alloc((8 + elems.len() * 8) as i32, insns);
            let ptr = ctx.bind("__list_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            insns.push(Instruction::LocalGet(ptr));
            insns.push(Instruction::I64Const(elems.len() as i64));
            store_i64_at(0, insns);
            for (idx, e) in elems.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(e, ctx, functions, insns);
                store_i64_at(8 + (idx * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── Match ─────────────────────────────────────────────────────────
        // Emit as a series of block/if nesting over the arms.
        // For now uses a simplified linear-scan pattern.
        AnfExpr::Match { scrutinee, arms } => {
            if let Some((_, scrutinee_ty)) = ctx.lookup(scrutinee) {
                let result_ty = ctx.expr_type(expr);
                emit_match_arms(
                    scrutinee,
                    scrutinee_ty,
                    arms,
                    result_ty,
                    ctx,
                    functions,
                    insns,
                )
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── Lambda (nested sub-expression) ───────────────────────────────
        // When a Lambda appears as a sub-expression (not the top-level body
        // of a binding — that case is handled in build_code_section), emit
        // one of three shapes depending on the Lambda's arity and captures.
        //
        // ## Hoistable fold reducer (params.len() == 2, captures.is_empty())
        //
        // A Lambda with exactly 2 parameters and no captures matches the
        // fold-reducer shape `(i64, i64) → i64`.  Its body is hoisted into a
        // separate WASM function by `build_code_section`; this arm emits only
        // the table index as an `i64.const` so Fold can dispatch it via the
        // existing I64 path (`i32.wrap_i64` + `call_indirect`).
        //
        // ## Closure-hoistable reducer (params.len() == 2, !captures.is_empty())
        // (Wave 16A PR3)
        //
        // A Lambda with exactly 2 parameters and at least one capture.  Its
        // body is hoisted into a 3-param WASM function
        // `(env_ptr: i64, acc: i64, elem: i64) → i64` by `build_code_section`.
        // This arm emits a closure env struct in linear memory and writes the
        // REAL table index into the fn_idx slot (offset 0), so Fold can
        // dispatch it via the I32 path using call_indirect with the
        // closure-reducer type.
        //
        // ## Non-hoistable Lambda (with captures, params.len() != 2)
        //
        // Emit a closure env struct in linear memory with fn_idx = 0
        // (placeholder — these Lambdas cannot be Fold reducers).
        //
        // Closure env layout (all cases):
        //   [fn_idx: i64, cap_count: i64, cap0: i64, ..., capN-1: i64]
        AnfExpr::Lambda {
            params,
            captures,
            body: _,
        } => {
            if params.len() == 2 && captures.is_empty() && ctx.fold_reducer_type_idx.is_some() {
                // Hoistable fold reducer: emit table index directly as I64.
                // Only reached when a function table exists (fold_reducer_type_idx.is_some()),
                // guaranteeing the table and hoisted body are present.
                // `build_code_section` emits the body as an extra function at
                // the same index, in the same DFS encounter order.
                let table_idx = ctx.next_hoisted_table_idx;
                ctx.next_hoisted_table_idx += 1;
                insns.push(Instruction::I64Const(i64::from(table_idx)));
                Some(ValType::I64)
            } else if params.len() == 2
                && !captures.is_empty()
                && ctx.closure_reducer_type_idx.is_some()
            {
                // Closure-hoistable reducer (Wave 16A PR3): emit closure env
                // with the REAL table index in the fn_idx slot.
                let table_idx = ctx.next_closure_hoisted_table_idx;
                ctx.next_closure_hoisted_table_idx += 1;

                let cap_count = captures.len();
                // Allocate: fn_idx (8 B) + cap_count (8 B) + N × 8 B.
                let byte_size = ((2 + cap_count) * 8) as i32;
                emit_alloc(byte_size, insns);
                let ptr_local = ctx.bind("__closure_env", ValType::I32);
                insns.push(Instruction::LocalSet(ptr_local));

                // fn_idx at offset 0 — REAL table index (not placeholder).
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(i64::from(table_idx)));
                store_i64_at(0, insns);

                // cap_count at offset 8.
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(cap_count as i64));
                store_i64_at(8, insns);

                // Each captured value at offset 16, 24, …
                for (i, cap_name) in captures.iter().enumerate() {
                    let offset = (16 + i * 8) as u64;
                    insns.push(Instruction::LocalGet(ptr_local));
                    if let Some((idx, ty)) = ctx.lookup(cap_name) {
                        insns.push(Instruction::LocalGet(idx));
                        // Zero-extend I32 captures to I64 for uniform storage.
                        if ty == ValType::I32 {
                            insns.push(Instruction::I64ExtendI32U);
                        }
                    } else {
                        insns.push(Instruction::I64Const(0));
                    }
                    store_i64_at(offset, insns);
                }

                insns.push(Instruction::LocalGet(ptr_local));
                Some(ValType::I32)
            } else {
                // Non-hoistable Lambda: emit closure env with fn_idx = 0
                // (placeholder — cannot be a Fold reducer).
                let cap_count = captures.len();
                let byte_size = ((2 + cap_count) * 8) as i32;
                emit_alloc(byte_size, insns);
                let ptr_local = ctx.bind("__closure_env", ValType::I32);
                insns.push(Instruction::LocalSet(ptr_local));

                // fn_idx at offset 0 (placeholder = 0).
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(0));
                store_i64_at(0, insns);

                // cap_count at offset 8.
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(cap_count as i64));
                store_i64_at(8, insns);

                // Each captured value at offset 16, 24, …
                for (i, cap_name) in captures.iter().enumerate() {
                    let offset = (16 + i * 8) as u64;
                    insns.push(Instruction::LocalGet(ptr_local));
                    if let Some((idx, ty)) = ctx.lookup(cap_name) {
                        insns.push(Instruction::LocalGet(idx));
                        if ty == ValType::I32 {
                            insns.push(Instruction::I64ExtendI32U);
                        }
                    } else {
                        insns.push(Instruction::I64Const(0));
                    }
                    store_i64_at(offset, insns);
                }

                insns.push(Instruction::LocalGet(ptr_local));
                Some(ValType::I32)
            }
        }

        // ── CellNew — allocate an 8-byte mutable cell initialised to `init` ─
        //
        // Layout: [value: i64] at offset 0.
        // Returns: I32 pointer to the cell.
        AnfExpr::CellNew { init } => {
            emit_alloc(8, insns);
            let ptr = ctx.bind("__cell_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            insns.push(Instruction::LocalGet(ptr));
            emit_local_as_i64(ctx, init, insns);
            store_i64_at(0, insns);
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── CellGet — read the I64 value stored in a cell ─────────────────
        //
        // `cell` is an I32 pointer (produced by CellNew).
        // Returns: I64 value at offset 0 of the cell.
        AnfExpr::CellGet { cell } => {
            if let Some((idx, _)) = ctx.lookup(cell) {
                insns.push(Instruction::LocalGet(idx));
                load_i64_at(0, insns);
                Some(ValType::I64)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── CellSet — write a new value into a cell ────────────────────────
        //
        // `cell` is an I32 pointer; `value` is the new I64 value.
        // Returns: unit (I32 0).
        AnfExpr::CellSet { cell, value } => {
            if let Some((cell_idx, _)) = ctx.lookup(cell) {
                insns.push(Instruction::LocalGet(cell_idx));
                emit_local_as_i64(ctx, value, insns);
                store_i64_at(0, insns);
                insns.push(Instruction::I32Const(0));
                Some(ValType::I32)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── Concurrency / dispatch stubs (defence-in-depth) ──────────────
        //
        // `emit_wasm_with_profile` detects these before code generation and
        // returns `CompileError::UnsupportedWasmConstruct` so callers never
        // reach these arms via the top-level entry point.
        //
        // The `unreachable` here is a defence-in-depth fallback: unit tests or
        // other callers that invoke `emit_anf_expr` directly (bypassing
        // `emit_wasm_with_profile`) will still get a runtime trap rather than
        // undefined behaviour or silent corruption.
        AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Select { .. }
        | AnfExpr::Timeout { .. } => {
            insns.push(Instruction::Unreachable);
            None
        }

        // ── ResourceAcquire ───────────────────────────────────────────────
        //
        // ABI: `ail/resource_acquire(res_ptr: i32, res_len: i32,
        //                             args_ptr: i32, args_count: i32) → i64`
        //
        // The resource name is stored in the data section (interned by
        // `EffectDataLayout::collect_expr`).  Each arg is written as an i64
        // into the shared args buffer at `args_offset + i * 8`, then
        // `resource_acquire` is called with the buffer start and count.
        // Returns an opaque handle packed as i64.
        AnfExpr::ResourceAcquire { resource, args } => {
            // Write args into the shared args buffer.
            for (idx, arg_name) in args.iter().enumerate() {
                insns.push(Instruction::I32Const(
                    ctx.effect_data.args_offset + (idx as i32 * 8),
                ));
                if let Some((local_idx, arg_ty)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(local_idx));
                    if arg_ty == ValType::I32 {
                        insns.push(Instruction::I64ExtendI32U);
                    }
                    insns.push(Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                } else {
                    insns.push(Instruction::Unreachable);
                    return None;
                }
            }
            // Push resource name (ptr, len) from the interned data section.
            let (res_ptr, res_len) = ctx.effect_data.string(resource);
            insns.push(Instruction::I32Const(res_ptr));
            insns.push(Instruction::I32Const(res_len));
            // Push args buffer start and count.
            insns.push(Instruction::I32Const(ctx.effect_data.args_offset));
            insns.push(Instruction::I32Const(args.len() as i32));
            // Call ail/resource_acquire.
            insns.push(Instruction::Call(
                ctx.effect_data.resource_acquire_func_index(),
            ));
            Some(ValType::I64)
        }

        // ── ResourceRelease ───────────────────────────────────────────────
        //
        // ABI: `ail/resource_release(handle: i64) → (void)`
        //
        // The handle local is pushed as i64 and passed directly to
        // `resource_release`.  No return value.
        AnfExpr::ResourceRelease { handle } => {
            if let Some((local_idx, handle_ty)) = ctx.lookup(handle) {
                insns.push(Instruction::LocalGet(local_idx));
                // Handles are i64; extend if the local was stored as i32.
                if handle_ty == ValType::I32 {
                    insns.push(Instruction::I64ExtendI32U);
                }
            } else {
                insns.push(Instruction::Unreachable);
                return None;
            }
            insns.push(Instruction::Call(
                ctx.effect_data.resource_release_func_index(),
            ));
            None
        }

        // ── RuntimeCheck ─────────────────────────────────────────────────
        // Emit a conditional trap: if `cond` is non-zero (violation detected)
        // → Unreachable.  If `cond` is zero → continue silently.
        AnfExpr::RuntimeCheck { cond, .. } => {
            emit_condition_get(ctx, cond, insns);
            insns.push(Instruction::If(wasm_encoder::BlockType::Empty));
            insns.push(Instruction::Unreachable);
            insns.push(Instruction::End);
            None
        }

        // ── ola5 Gap 2 — new primitives (WASM stubs) ─────────────────────
        // Assume: no runtime effect.
        AnfExpr::Assume { .. } => None,
        // Abort: always unreachable.
        AnfExpr::Abort { .. } => {
            insns.push(Instruction::Unreachable);
            None
        }
        // ── MapNew — construct a key-value map in linear memory ───────────
        //
        // Layout: [count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...]
        // Returns: I32 pointer to the map header.
        AnfExpr::MapNew { entries } => {
            let byte_size = ((1 + entries.len() * 2) * 8).max(8) as i32;
            emit_alloc(byte_size, insns);
            let ptr = ctx.bind("__map_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            // Store count at offset 0.
            insns.push(Instruction::LocalGet(ptr));
            insns.push(Instruction::I64Const(entries.len() as i64));
            store_i64_at(0, insns);
            // Store interleaved key-value pairs: k at 8+i*16, v at 16+i*16.
            for (i, (k, v)) in entries.iter().enumerate() {
                let key_offset = (8 + i * 16) as u64;
                let val_offset = (16 + i * 16) as u64;
                insns.push(Instruction::LocalGet(ptr));
                emit_local_as_i64(ctx, k, insns);
                store_i64_at(key_offset, insns);
                insns.push(Instruction::LocalGet(ptr));
                emit_local_as_i64(ctx, v, insns);
                store_i64_at(val_offset, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── SetNew — construct a set in linear memory ──────────────────────
        //
        // Layout: [count: i64, elem0: i64, elem1: i64, ...]
        // Returns: I32 pointer to the set header.
        AnfExpr::SetNew { elements } => {
            let byte_size = ((1 + elements.len()) * 8).max(8) as i32;
            emit_alloc(byte_size, insns);
            let ptr = ctx.bind("__set_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            // Store count at offset 0.
            insns.push(Instruction::LocalGet(ptr));
            insns.push(Instruction::I64Const(elements.len() as i64));
            store_i64_at(0, insns);
            // Store elements at offsets 8, 16, ...
            for (i, elem) in elements.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_local_as_i64(ctx, elem, insns);
                store_i64_at((8 + i * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── IndexGet — dynamic indexed element access from a list ──────────
        //
        // List layout: [len: i64, elem0: i64, elem1: i64, ...]
        // Element at index i: ptr + 8 + i * 8
        // Returns: I64 element value.
        //
        // Emission sequence:
        //   local.get index        ; [I64]
        //   local.get collection   ; [I64, I32] list pointer
        //   i64.load { offset: 0 } ; [I64, I64] len
        //   i64.ge_u               ; [I32]       index >= len
        //   if [] unreachable end  ; trap on out-of-bounds
        //   local.get collection   ; [I32] list pointer
        //   local.get index        ; [I32, I64]
        //   i64.const 8
        //   i64.mul                ; [I32, I64]  index * 8
        //   i64.const 8
        //   i64.add                ; [I32, I64]  8 + index * 8
        //   i32.wrap_i64           ; [I32, I32]  byte offset
        //   i32.add                ; [I32]        ptr + 8 + index * 8
        //   i64.load { offset: 0 } ; [I64]        element
        AnfExpr::IndexGet { collection, index } => {
            let Some((coll_idx, _)) = ctx.lookup(collection) else {
                insns.push(Instruction::Unreachable);
                return None;
            };
            let Some((idx_idx, idx_ty)) = ctx.lookup(index) else {
                insns.push(Instruction::Unreachable);
                return None;
            };

            // Bounds guard: trap before computing the element address when
            // index >= len. The unsigned compare also rejects negative i64
            // indices, which appear as very large unsigned values.
            insns.push(Instruction::LocalGet(idx_idx));
            if idx_ty == ValType::I32 {
                insns.push(Instruction::I64ExtendI32U);
            }
            insns.push(Instruction::LocalGet(coll_idx));
            load_i64_at(0, insns);
            insns.push(Instruction::I64GeU);
            insns.push(Instruction::If(BlockType::Empty));
            insns.push(Instruction::Unreachable);
            insns.push(Instruction::End);

            insns.push(Instruction::LocalGet(coll_idx));
            insns.push(Instruction::LocalGet(idx_idx));
            // Normalise index to I64 for arithmetic.
            if idx_ty == ValType::I32 {
                insns.push(Instruction::I64ExtendI32U);
            }
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Mul);
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::I32WrapI64);
            insns.push(Instruction::I32Add);
            load_i64_at(0, insns);
            Some(ValType::I64)
        }

        // ── ForEach — inline loop over a length-prefixed list ────────────
        //
        // List layout: [count: i64, elem0: i64, elem1: i64, ...]
        //
        // Emission:
        //   1. Load count from list header (offset 0).
        //   2. Initialise loop counter to 0.
        //   3. block (empty) — break target.
        //   4.   loop (empty) — continue target.
        //   5.     i >= count  → br_if 1 (exit block).
        //   6.     Load element at coll_ptr + 8 + i * 8.
        //   7.     Store element to `binding` local.
        //   8.     Emit body; drop result (ForEach is side-effect only).
        //   9.     i += 1; br 0 (restart loop).
        //  10. end loop / end block.
        //
        // No call_indirect is required: the body is already an inlined
        // AnfExpr, so the loop executes it directly without a function
        // pointer dispatch.
        AnfExpr::ForEach {
            binding,
            collection,
            body,
        } => {
            let Some((coll_idx, _)) = ctx.lookup(collection) else {
                insns.push(Instruction::Unreachable);
                return None;
            };

            // Allocate locals: count (I64), loop counter (I64), loop var (I64).
            let count_idx = ctx.bind("__foreach_count", ValType::I64);
            let i_idx = ctx.bind("__foreach_i", ValType::I64);
            let elem_idx = ctx.bind(binding.as_str(), ValType::I64);

            // Load element count from list header at offset 0.
            insns.push(Instruction::LocalGet(coll_idx));
            load_i64_at(0, insns);
            insns.push(Instruction::LocalSet(count_idx));

            // Initialise counter to 0.
            insns.push(Instruction::I64Const(0));
            insns.push(Instruction::LocalSet(i_idx));

            // block (break target) + loop (continue target).
            insns.push(Instruction::Block(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);

            // Exit condition: i >= count → break.
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::LocalGet(count_idx));
            insns.push(Instruction::I64GeU);
            let break_depth = ctx.branch_depth(LabelKind::LoopBreak).unwrap_or(1);
            insns.push(Instruction::BrIf(break_depth));

            // Load element at coll_ptr + 8 + i * 8.
            insns.push(Instruction::LocalGet(coll_idx));
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Mul);
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::I32WrapI64);
            insns.push(Instruction::I32Add);
            load_i64_at(0, insns);
            insns.push(Instruction::LocalSet(elem_idx));

            // Emit loop body; discard any produced value.
            let body_ty = emit_anf_expr(body, ctx, functions, insns);
            if body_ty.is_some() {
                insns.push(Instruction::Drop);
            }

            // Increment counter: i += 1.
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::I64Const(1));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::LocalSet(i_idx));

            // Jump back to top of loop.
            insns.push(Instruction::Br(0));

            ctx.labels.pop();
            insns.push(Instruction::End); // end loop
            ctx.labels.pop();
            insns.push(Instruction::End); // end block

            // ForEach is side-effect only in terms of semantics, but it
            // must produce a unit value on the WASM stack so that it can
            // appear as the `value` in an `AnfExpr::Let` binding or as an
            // intermediate element in a `Seq` without causing a stack-
            // underflow validation error.  Push I32 0 (unit) here so that
            // the enclosing `LocalSet` or `Drop` has something to consume.
            insns.push(Instruction::I32Const(0));
            Some(ValType::I32)
        }

        // ── Fold — call_indirect over function table ──────────────────────
        //
        // Fold { init, list, func } accumulates over a length-prefixed list
        // by calling `func(acc, elem) → i64` for each element.
        //
        // WASM emission:
        //   1. Load list element count from header (offset 0).
        //   2. Initialise accumulator from `init` and counter to 0.
        //   3. Loop:
        //        a. If i >= count → break with current acc (result of block).
        //        b. Load element: list_ptr + 8 + i * 8.
        //        c. call_indirect(fold_reducer_type, table 0) with acc and elem.
        //        d. Update acc; increment i; continue.
        //
        // `func` is resolved as one of:
        //   • A top-level function name (in the `functions` map) — table index
        //     is `func_idx - function_offset`, pushed as `i32.const`.
        //   • A local I32 variable (closure env) — loads `fn_idx` (i64) from
        //     offset 0 of the env pointer, wraps to i32.
        //   • A local I64 variable — wraps directly to i32.
        //
        // Note: capture-free 2-param Lambdas are hoisted (Wave 12A) and
        // dispatch via the I64 path above.  Lambdas with captures still emit a
        // closure env (I32 pointer) whose fn_idx is a placeholder; the I32
        // path below traps at runtime.  General closure hoisting is deferred.
        AnfExpr::Fold { init, list, func } => {
            let Some(fold_type_idx) = ctx.fold_reducer_type_idx else {
                // Pre-flight gate should have inserted the type; trap defensively.
                insns.push(Instruction::Unreachable);
                return None;
            };

            let Some((list_local, _)) = ctx.lookup(list) else {
                insns.push(Instruction::Unreachable);
                return None;
            };

            // Allocate locals: count, loop index, accumulator, element.
            let count_idx = ctx.bind("__fold_count", ValType::I64);
            let i_idx = ctx.bind("__fold_i", ValType::I64);
            let acc_idx = ctx.bind("__fold_acc", ValType::I64);
            let elem_idx = ctx.bind("__fold_elem", ValType::I64);

            // Load element count from list header (offset 0).
            insns.push(Instruction::LocalGet(list_local));
            load_i64_at(0, insns);
            insns.push(Instruction::LocalSet(count_idx));

            // Initialise accumulator from `init`.
            emit_local_as_i64(ctx, init, insns);
            insns.push(Instruction::LocalSet(acc_idx));

            // Initialise loop counter to 0.
            insns.push(Instruction::I64Const(0));
            insns.push(Instruction::LocalSet(i_idx));

            // block (result I64) — break target that yields the final accumulator.
            insns.push(Instruction::Block(BlockType::Result(ValType::I64)));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);

            // Exit check: if i >= count, break with the current accumulator.
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::LocalGet(count_idx));
            insns.push(Instruction::I64GeU);
            insns.push(Instruction::If(BlockType::Empty));
            ctx.labels.push(LabelKind::Other);
            insns.push(Instruction::LocalGet(acc_idx));
            // Break to the enclosing block (carries acc as the block result).
            // Depth from inside the If: 0 = If, 1 = Loop, 2 = Block.
            let break_depth = ctx.branch_depth(LabelKind::LoopBreak).unwrap_or(2);
            insns.push(Instruction::Br(break_depth));
            ctx.labels.pop(); // Other (If body)
            insns.push(Instruction::End); // end if

            // Load element: list_ptr + 8 + i * 8.
            insns.push(Instruction::LocalGet(list_local));
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Mul);
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::I32WrapI64);
            insns.push(Instruction::I32Add);
            load_i64_at(0, insns);
            insns.push(Instruction::LocalSet(elem_idx));

            // Push reducer arguments: acc (i64), elem (i64).
            insns.push(Instruction::LocalGet(acc_idx));
            insns.push(Instruction::LocalGet(elem_idx));

            // Push callee table index (i32).
            if let Some(&func_idx) = functions.get(func.as_str()) {
                // Top-level function: table index = absolute func idx − offset.
                let table_idx = func_idx.saturating_sub(ctx.function_offset);
                insns.push(Instruction::I32Const(table_idx as i32));
            } else if let Some((local_idx, local_ty)) = ctx.lookup(func) {
                match local_ty {
                    ValType::I32 => {
                        // Closure env pointer (Wave 16A PR3): load fn_idx from
                        // offset 0 of the env, wrap to i32 for call_indirect.
                        // Also push env_ptr (as i64) as the first argument to the
                        // closure-reducer `(env_ptr: i64, acc: i64, elem: i64)`.
                        //
                        // The argument order before call_indirect must be:
                        //   [acc: i64, elem: i64] already on stack
                        // But we need [env_ptr: i64, acc: i64, elem: i64, callee: i32].
                        // Since acc and elem are already pushed above, and call_indirect
                        // is a stack-based dispatch, we need to reorganise:
                        //
                        // Strategy: DON'T push acc/elem above; push them after env_ptr.
                        // But acc and elem were already pushed above — we need to move
                        // env_ptr to before them.
                        //
                        // We use the closure_reducer_type path differently: the acc and
                        // elem are on the stack already (pushed in the block before
                        // this else-if).  We insert env_ptr before them using a local.
                        //
                        // Actually, the `call_indirect` with closure-reducer type
                        // expects [env_ptr: i64, acc: i64, elem: i64] in that order.
                        // Since acc and elem are already on the stack (pushed above),
                        // and we can't easily insert before them, we DON'T use the
                        // standard call_indirect tail here.  Instead we take over the
                        // full dispatch below and break out of the normal post-branch.
                        //
                        // NOTE: the acc/elem pushes above are WASTED when the I32 path
                        // is taken — they're dropped here so we can re-push in the
                        // right order for the closure-reducer ABI.
                        //
                        // This is safe because Fold only cares about the final result.
                        if let Some(closure_type_idx) = ctx.closure_reducer_type_idx {
                            // Drop acc and elem (already on stack from the push above).
                            insns.push(Instruction::Drop); // elem
                            insns.push(Instruction::Drop); // acc

                            // Push env_ptr (as i64) — first argument.
                            insns.push(Instruction::LocalGet(local_idx));
                            insns.push(Instruction::I64ExtendI32U);

                            // Re-push acc and elem.
                            insns.push(Instruction::LocalGet(acc_idx));
                            insns.push(Instruction::LocalGet(elem_idx));

                            // Load fn_idx (i64) from env[0], wrap to i32 for table.
                            insns.push(Instruction::LocalGet(local_idx));
                            load_i64_at(0, insns);
                            insns.push(Instruction::I32WrapI64);

                            // call_indirect with closure-reducer type.
                            insns.push(Instruction::CallIndirect {
                                type_index: closure_type_idx,
                                table_index: 0,
                            });
                            insns.push(Instruction::LocalSet(acc_idx));

                            // Increment loop counter.
                            insns.push(Instruction::LocalGet(i_idx));
                            insns.push(Instruction::I64Const(1));
                            insns.push(Instruction::I64Add);
                            insns.push(Instruction::LocalSet(i_idx));

                            // Branch back to loop header.
                            insns.push(Instruction::Br(0));

                            ctx.labels.pop(); // LoopContinue
                            insns.push(Instruction::End); // end loop
                            insns.push(Instruction::Unreachable);
                            ctx.labels.pop(); // LoopBreak
                            insns.push(Instruction::End); // end block

                            return Some(ValType::I64);
                        }
                        // No closure-reducer type available — fall through to
                        // Unreachable (shouldn't happen with needs_fold, but safe).
                        insns.push(Instruction::Drop); // elem
                        insns.push(Instruction::Drop); // acc
                        insns.push(Instruction::Unreachable);
                    }
                    ValType::I64 => {
                        // Direct table index packed as i64: push local, wrap to i32.
                        insns.push(Instruction::LocalGet(local_idx));
                        insns.push(Instruction::I32WrapI64);
                    }
                    _ => {
                        // Unexpected local type (e.g. F64) — drop acc and elem
                        // from the stack, then trap via Unreachable.  Dead code
                        // after Unreachable is accepted by the WASM validator.
                        insns.push(Instruction::Drop); // elem
                        insns.push(Instruction::Drop); // acc
                        insns.push(Instruction::Unreachable);
                    }
                }
            } else {
                // Unresolved function reference — trap at runtime.
                insns.push(Instruction::Unreachable);
            }

            // call_indirect: pops [acc: i64, elem: i64, callee: i32] → i64.
            // (Only reached for the I64 and top-level-function paths above;
            // the I32/closure path returns early after its own call_indirect.)
            insns.push(Instruction::CallIndirect {
                type_index: fold_type_idx,
                table_index: 0,
            });
            insns.push(Instruction::LocalSet(acc_idx));

            // Increment loop counter.
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::I64Const(1));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::LocalSet(i_idx));

            // Branch back to loop header.
            insns.push(Instruction::Br(0));

            ctx.labels.pop(); // LoopContinue
            insns.push(Instruction::End); // end loop
            // Unreachable: the loop always exits via Br(break_depth) above.
            insns.push(Instruction::Unreachable);
            ctx.labels.pop(); // LoopBreak
            insns.push(Instruction::End); // end block — I64 result from Br

            Some(ValType::I64)
        }

        // ── Placeholder ───────────────────────────────────────────────────
        AnfExpr::Placeholder => {
            insns.push(Instruction::Unreachable);
            None
        }
    }
}
