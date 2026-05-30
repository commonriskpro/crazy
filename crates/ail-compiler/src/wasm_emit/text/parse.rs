use super::*;

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
