use super::*;

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
