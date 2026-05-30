use super::*;

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
