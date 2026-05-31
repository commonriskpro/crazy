use super::*;

pub(super) fn emit_text_byte_at_or<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, index_arg, fallback_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let value_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_len));

    let value_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_ptr));

    let index_i64 = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, index_arg, insns);
    insns.push(Instruction::LocalSet(index_i64));

    let fallback = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, fallback_arg, insns);
    insns.push(Instruction::LocalSet(fallback));

    let index_i32 = ctx.bind_temp(ValType::I32);
    let result = ctx.bind_temp(ValType::I64);

    insns.push(Instruction::LocalGet(fallback));
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::LocalGet(index_i64));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GeS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(index_i64));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(index_i64));
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::LocalSet(index_i32));

    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(index_i32));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result));
    Some(ValType::I64)
}

pub(super) fn emit_text_slice<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, start_arg, length_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let value_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_len));

    let value_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_ptr));

    let start_i64 = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, start_arg, insns);
    insns.push(Instruction::LocalSet(start_i64));

    let requested_i64 = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, length_arg, insns);
    insns.push(Instruction::LocalSet(requested_i64));

    let start_i32 = ctx.bind_temp(ValType::I32);
    let remaining_i32 = ctx.bind_temp(ValType::I32);
    let remaining_i64 = ctx.bind_temp(ValType::I64);
    let copy_len = ctx.bind_temp(ValType::I32);
    let end_i32 = ctx.bind_temp(ValType::I32);
    let out_ptr = ctx.bind_temp(ValType::I32);

    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(copy_len));
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalSet(out_ptr));

    insns.push(Instruction::LocalGet(start_i64));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GeS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(requested_i64));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(start_i64));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(start_i64));
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::LocalSet(start_i32));

    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalGet(start_i32));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::LocalSet(remaining_i32));

    insns.push(Instruction::LocalGet(remaining_i32));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::LocalSet(remaining_i64));

    insns.push(Instruction::LocalGet(requested_i64));
    insns.push(Instruction::LocalGet(remaining_i64));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::LocalGet(requested_i64));
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::LocalSet(copy_len));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(remaining_i32));
    insns.push(Instruction::LocalSet(copy_len));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(start_i32));
    insns.push(Instruction::LocalGet(copy_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(end_i32));

    emit_text_utf8_boundary_test_from_locals(value_ptr, value_len, start_i32, insns);
    insns.push(Instruction::If(BlockType::Empty));

    emit_text_utf8_boundary_test_from_locals(value_ptr, value_len, end_i32, insns);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalSet(out_ptr));
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalGet(copy_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::GlobalSet(0));

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(start_i32));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(copy_len));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(copy_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64Shl);
    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Or);

    Some(ValType::I64)
}
