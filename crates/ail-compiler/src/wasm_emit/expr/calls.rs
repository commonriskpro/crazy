use super::*;

pub(super) fn emit_call_expr<'a>(
    func: &String,
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
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
    if matches!(
        func.as_str(),
        "time.duration_since" | "time_duration_since" | "std.time.duration_since"
    ) {
        return emit_time_duration_since(args, ctx, insns);
    }
    if matches!(
        func.as_str(),
        "time.add_duration" | "time_add_duration" | "std.time.add_duration"
    ) {
        return emit_time_add_duration(args, ctx, insns);
    }
    if matches!(
        func.as_str(),
        "time.instant_to_ms" | "time_instant_to_ms" | "std.time.instant_to_ms"
    ) {
        return emit_time_instant_to_ms(args, ctx, insns);
    }
    if matches!(
        func.as_str(),
        "bytes.length" | "bytes_length" | "std.bytes.length"
    ) {
        return emit_bytes_length(args, ctx, insns);
    }
    if matches!(func.as_str(), "bytes.at" | "bytes_at" | "std.bytes.at") {
        return emit_bytes_at(args, ctx, insns);
    }
    if matches!(
        func.as_str(),
        "bytes.empty" | "bytes_empty" | "std.bytes.empty"
    ) {
        return emit_bytes_empty(args, ctx, insns);
    }
    if matches!(
        func.as_str(),
        "path.from_text"
            | "path_from_text"
            | "std.path.from_text"
            | "path.to_text"
            | "path_to_text"
            | "std.path.to_text"
    ) {
        return emit_path_identity_call(args, ctx, insns);
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

fn emit_time_duration_since<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [later, earlier] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    let Some((later_idx, _)) = ctx.lookup(later) else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    let Some((earlier_idx, _)) = ctx.lookup(earlier) else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    insns.push(Instruction::LocalGet(later_idx));
    insns.push(Instruction::LocalGet(earlier_idx));
    insns.push(Instruction::I64Sub);
    Some(ValType::I64)
}

fn emit_time_add_duration<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [instant, duration] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    let Some((instant_idx, _)) = ctx.lookup(instant) else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    let Some((duration_idx, _)) = ctx.lookup(duration) else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    insns.push(Instruction::LocalGet(instant_idx));
    insns.push(Instruction::LocalGet(duration_idx));
    insns.push(Instruction::I64Add);
    Some(ValType::I64)
}

fn emit_time_instant_to_ms<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [instant] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    if let Some((idx, _)) = ctx.lookup(instant) {
        insns.push(Instruction::LocalGet(idx));
        Some(ValType::I64)
    } else {
        insns.push(Instruction::Unreachable);
        None
    }
}

fn emit_bytes_length<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    if let Some((idx, _)) = ctx.lookup(arg) {
        insns.push(Instruction::LocalGet(idx));
        insns.push(Instruction::I64Const(32));
        insns.push(Instruction::I64ShrU);
        Some(ValType::I64)
    } else {
        insns.push(Instruction::Unreachable);
        None
    }
}

fn emit_bytes_at<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [bytes_arg, index_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    let Some((bytes_idx, _)) = ctx.lookup(bytes_arg) else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    let Some((index_idx, _)) = ctx.lookup(index_arg) else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let packed = ctx.bind_temp(ValType::I64);
    let len = ctx.bind_temp(ValType::I64);
    let ptr = ctx.bind_temp(ValType::I32);
    let index = ctx.bind_temp(ValType::I64);
    let result_ptr = ctx.bind_temp(ValType::I32);

    insns.push(Instruction::LocalGet(bytes_idx));
    insns.push(Instruction::LocalSet(packed));
    insns.push(Instruction::LocalGet(packed));
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64ShrU);
    insns.push(Instruction::LocalSet(len));
    insns.push(Instruction::LocalGet(packed));
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::LocalSet(ptr));
    insns.push(Instruction::LocalGet(index_idx));
    insns.push(Instruction::LocalSet(index));

    emit_alloc(16, insns);
    insns.push(Instruction::LocalSet(result_ptr));
    insns.push(Instruction::LocalGet(result_ptr));
    insns.push(Instruction::I32Const(ctx.assign_tag("None") as i32));
    insns.push(Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    insns.push(Instruction::LocalGet(result_ptr));
    insns.push(Instruction::I64Const(0));
    store_i64_at(8, insns);

    insns.push(Instruction::LocalGet(index));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GeS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(index));
    insns.push(Instruction::LocalGet(len));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(result_ptr));
    insns.push(Instruction::I32Const(ctx.assign_tag("Some") as i32));
    insns.push(Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    insns.push(Instruction::LocalGet(result_ptr));
    insns.push(Instruction::LocalGet(ptr));
    insns.push(Instruction::LocalGet(index));
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I64ExtendI32U);
    store_i64_at(8, insns);

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result_ptr));
    Some(ValType::I32)
}

fn emit_bytes_empty<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    if let Some((idx, _)) = ctx.lookup(arg) {
        insns.push(Instruction::LocalGet(idx));
        insns.push(Instruction::I64Const(32));
        insns.push(Instruction::I64ShrU);
        insns.push(Instruction::I64Eqz);
        insns.push(Instruction::I64ExtendI32U);
        Some(ValType::I64)
    } else {
        insns.push(Instruction::Unreachable);
        None
    }
}

fn emit_path_identity_call<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };
    if let Some((idx, ty)) = ctx.lookup(arg) {
        insns.push(Instruction::LocalGet(idx));
        Some(ty)
    } else {
        insns.push(Instruction::Unreachable);
        None
    }
}

pub(super) fn emit_effect_call_expr<'a>(
    capability: &String,
    func: &String,
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
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
        if effect_call_returns_bytes(capability, func) {
            emit_host_call_write_packed_buffer(ctx, insns);
        } else {
            // Extend the i32 return to i64 to match the standard EffectCall return type.
            insns.push(Instruction::I64ExtendI32S);
        }
    } else {
        insns.push(Instruction::Call(0));
    }
    Some(ValType::I64)
}

fn emit_host_call_write_packed_buffer<'a>(
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) {
    let len_local = ctx.bind_temp(ValType::I32);
    insns.push(Instruction::LocalSet(len_local));
    insns.push(Instruction::LocalGet(len_local));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64Shl);
    insns.push(Instruction::I64Const(
        ctx.effect_data.result_buffer_offset as i64,
    ));
    insns.push(Instruction::I64Or);
}
