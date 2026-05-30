use super::*;

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
