use super::control::emit_local_as_i64;
use super::*;

pub(super) fn emit_text_len_from_local<'a>(
    ctx: &WasmCodegenCtx<'a>,
    name: &str,
    insns: &mut Vec<Instruction<'a>>,
) {
    emit_local_as_i64(ctx, name, insns);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64ShrU);
    insns.push(Instruction::I32WrapI64);
}

pub(super) fn emit_list_len_from_local<'a>(
    ctx: &WasmCodegenCtx<'a>,
    name: &str,
    insns: &mut Vec<Instruction<'a>>,
) -> bool {
    let Some((idx, ValType::I32)) = ctx.lookup(name) else {
        return false;
    };
    insns.push(Instruction::LocalGet(idx));
    load_i64_at(0, insns);
    true
}

pub(super) fn emit_text_ptr_from_local<'a>(
    ctx: &WasmCodegenCtx<'a>,
    name: &str,
    insns: &mut Vec<Instruction<'a>>,
) {
    emit_local_as_i64(ctx, name, insns);
    insns.push(Instruction::I32WrapI64);
}

pub(super) fn emit_text_concat<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, left, insns);
    insns.push(Instruction::LocalSet(left_len));

    let right_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, right, insns);
    insns.push(Instruction::LocalSet(right_len));

    let left_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, left, insns);
    insns.push(Instruction::LocalSet(left_ptr));

    let right_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, right, insns);
    insns.push(Instruction::LocalSet(right_ptr));

    let total_len = ctx.bind_temp(ValType::I32);
    insns.push(Instruction::LocalGet(left_len));
    insns.push(Instruction::LocalGet(right_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(total_len));

    let out_ptr = ctx.bind_temp(ValType::I32);
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalGet(total_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::GlobalSet(0));
    insns.push(Instruction::LocalSet(out_ptr));

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(left_ptr));
    insns.push(Instruction::LocalGet(left_len));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(left_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(right_ptr));
    insns.push(Instruction::LocalGet(right_len));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::LocalGet(total_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64Shl);
    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Or);

    Some(ValType::I64)
}

pub(super) fn emit_ascii_whitespace_test_from_local<'a>(
    byte: u32,
    insns: &mut Vec<Instruction<'a>>,
) {
    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(32));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(9));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::I32Or);
    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(10));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::I32Or);
    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(13));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::I32Or);
}

pub(super) fn emit_text_trim<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let value_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_len));

    let value_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_ptr));

    let start = ctx.bind_temp(ValType::I32);
    let end = ctx.bind_temp(ValType::I32);
    let byte = ctx.bind_temp(ValType::I32);
    let out_len = ctx.bind_temp(ValType::I32);
    let out_ptr = ctx.bind_temp(ValType::I32);

    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(start));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalSet(end));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::LocalGet(end));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalSet(byte));
    emit_ascii_whitespace_test_from_local(byte, insns);
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(start));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(end));
    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32GtU);
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(end));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalSet(byte));
    emit_ascii_whitespace_test_from_local(byte, insns);
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(end));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::LocalSet(end));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(end));
    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::LocalSet(out_len));

    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalSet(out_ptr));
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalGet(out_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::GlobalSet(0));

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(out_len));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::LocalGet(out_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64Shl);
    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Or);

    Some(ValType::I64)
}

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

pub(super) fn emit_text_parse_int_or<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, fallback_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let value_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_len));

    let value_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_ptr));

    let fallback = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, fallback_arg, insns);
    insns.push(Instruction::LocalSet(fallback));

    let idx = ctx.bind_temp(ValType::I32);
    let byte = ctx.bind_temp(ValType::I32);
    let digit = ctx.bind_temp(ValType::I64);
    let parsed = ctx.bind_temp(ValType::I64);
    let sign = ctx.bind_temp(ValType::I64);
    let max_last_digit = ctx.bind_temp(ValType::I64);
    let overflow = ctx.bind_temp(ValType::I64);
    let valid = ctx.bind_temp(ValType::I64);
    let saw_digit = ctx.bind_temp(ValType::I64);
    let result = ctx.bind_temp(ValType::I64);

    insns.push(Instruction::LocalGet(fallback));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(idx));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(parsed));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(sign));
    insns.push(Instruction::I64Const(7));
    insns.push(Instruction::LocalSet(max_last_digit));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(valid));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(saw_digit));

    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::I32GtU);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(value_ptr));
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalSet(byte));

    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(45));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::LocalSet(sign));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::LocalSet(idx));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(43));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::LocalSet(idx));
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(sign));
    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(8));
    insns.push(Instruction::LocalSet(max_last_digit));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I32LtU);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalSet(byte));

    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(48));
    insns.push(Instruction::I32LtU);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(valid));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(57));
    insns.push(Instruction::I32GtU);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(valid));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(byte));
    insns.push(Instruction::I32Const(48));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::LocalSet(digit));

    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(overflow));

    insns.push(Instruction::LocalGet(parsed));
    insns.push(Instruction::I64Const(922337203685477580));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(overflow));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(parsed));
    insns.push(Instruction::I64Const(922337203685477580));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::LocalGet(digit));
    insns.push(Instruction::LocalGet(max_last_digit));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(overflow));
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(overflow));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(valid));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(parsed));
    insns.push(Instruction::I64Const(10));
    insns.push(Instruction::I64Mul);
    insns.push(Instruction::LocalGet(digit));
    insns.push(Instruction::I64Add);
    insns.push(Instruction::LocalSet(parsed));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(saw_digit));
    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(idx));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(valid));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::LocalGet(saw_digit));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::LocalGet(parsed));
    insns.push(Instruction::LocalGet(sign));
    insns.push(Instruction::I64Mul);
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::End);

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

    insns.push(Instruction::LocalGet(copy_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64Shl);
    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Or);

    Some(ValType::I64)
}

pub(super) fn emit_text_replace_first<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, needle, replacement] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let value_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_len));

    let needle_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_len));

    let replacement_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, replacement, insns);
    insns.push(Instruction::LocalSet(replacement_len));

    let value_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, value, insns);
    insns.push(Instruction::LocalSet(value_ptr));

    let needle_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_ptr));

    let replacement_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, replacement, insns);
    insns.push(Instruction::LocalSet(replacement_ptr));

    let pos = ctx.bind_temp(ValType::I32);
    let found_pos = ctx.bind_temp(ValType::I32);
    let offset = ctx.bind_temp(ValType::I32);
    let limit_exclusive = ctx.bind_temp(ValType::I32);
    let matched = ctx.bind_temp(ValType::I64);
    let found = ctx.bind_temp(ValType::I64);
    let out_len = ctx.bind_temp(ValType::I32);
    let out_ptr = ctx.bind_temp(ValType::I32);
    let suffix_start = ctx.bind_temp(ValType::I32);
    let suffix_len = ctx.bind_temp(ValType::I32);

    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(found));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(found_pos));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalSet(out_len));
    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalSet(out_ptr));

    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::I32GtU);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(limit_exclusive));

    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(pos));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::LocalGet(limit_exclusive));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(matched));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(offset));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalGet(needle_ptr));
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(matched));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(offset));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(matched));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(found));
    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::LocalSet(found_pos));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(pos));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(found));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::LocalGet(replacement_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(out_len));

    insns.push(Instruction::LocalGet(found_pos));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(suffix_start));

    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::LocalGet(suffix_start));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::LocalSet(suffix_len));

    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalSet(out_ptr));
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::LocalGet(out_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::GlobalSet(0));

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(found_pos));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(found_pos));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(replacement_ptr));
    insns.push(Instruction::LocalGet(replacement_len));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::LocalGet(found_pos));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(replacement_len));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(suffix_start));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(suffix_len));
    insns.push(Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });

    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(out_len));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Const(32));
    insns.push(Instruction::I64Shl);
    insns.push(Instruction::LocalGet(out_ptr));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::I64Or);

    Some(ValType::I64)
}

pub(super) fn load_i32_u8_at<'a>(offset: u64, insns: &mut Vec<Instruction<'a>>) {
    insns.push(Instruction::I32Load8U(wasm_encoder::MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }));
}

pub(super) fn emit_text_eq<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, left, insns);
    insns.push(Instruction::LocalSet(left_len));

    let right_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, right, insns);
    insns.push(Instruction::LocalSet(right_len));

    let left_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, left, insns);
    insns.push(Instruction::LocalSet(left_ptr));

    let right_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, right, insns);
    insns.push(Instruction::LocalSet(right_ptr));

    let idx = ctx.bind_temp(ValType::I32);
    let result = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::LocalGet(left_len));
    insns.push(Instruction::LocalGet(right_len));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(idx));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::LocalGet(left_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(left_ptr));
    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalGet(right_ptr));
    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(idx));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(idx));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result));
    Some(ValType::I64)
}

pub(super) fn emit_text_contains<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [haystack, needle] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let haystack_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_len));

    let needle_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_len));

    let haystack_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_ptr));

    let needle_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_ptr));

    let pos = ctx.bind_temp(ValType::I32);
    let offset = ctx.bind_temp(ValType::I32);
    let limit_exclusive = ctx.bind_temp(ValType::I32);
    let matched = ctx.bind_temp(ValType::I64);
    let result = ctx.bind_temp(ValType::I64);

    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(haystack_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(haystack_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(limit_exclusive));

    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(pos));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::LocalGet(limit_exclusive));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(matched));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(offset));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(haystack_ptr));
    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalGet(needle_ptr));
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(matched));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(offset));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(matched));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(pos));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result));
    Some(ValType::I64)
}

pub(super) fn emit_text_index_of<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [haystack, needle] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let haystack_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_len));

    let needle_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_len));

    let haystack_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_ptr));

    let needle_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_ptr));

    let pos = ctx.bind_temp(ValType::I32);
    let offset = ctx.bind_temp(ValType::I32);
    let limit_exclusive = ctx.bind_temp(ValType::I32);
    let matched = ctx.bind_temp(ValType::I64);
    let result = ctx.bind_temp(ValType::I64);

    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(haystack_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::If(BlockType::Empty));

    insns.push(Instruction::LocalGet(haystack_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Sub);
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(limit_exclusive));

    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(pos));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::LocalGet(limit_exclusive));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(matched));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(offset));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(haystack_ptr));
    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalGet(needle_ptr));
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(matched));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(offset));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(matched));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I64ExtendI32U);
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(pos));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(pos));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result));
    Some(ValType::I64)
}

pub(super) fn emit_text_boundary_match<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
    at_end: bool,
) -> Option<ValType> {
    let [haystack, needle] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let haystack_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_len));

    let needle_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_len));

    let haystack_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_ptr));

    let needle_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_ptr));

    let start = ctx.bind_temp(ValType::I32);
    let offset = ctx.bind_temp(ValType::I32);
    let result = ctx.bind_temp(ValType::I64);

    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(haystack_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::If(BlockType::Empty));

    if at_end {
        insns.push(Instruction::LocalGet(haystack_len));
        insns.push(Instruction::LocalGet(needle_len));
        insns.push(Instruction::I32Sub);
    } else {
        insns.push(Instruction::I32Const(0));
    }
    insns.push(Instruction::LocalSet(start));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(offset));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(haystack_ptr));
    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalGet(needle_ptr));
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(offset));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result));
    Some(ValType::I64)
}
