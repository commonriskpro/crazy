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

pub(super) fn load_i32_u8_at<'a>(offset: u64, insns: &mut Vec<Instruction<'a>>) {
    insns.push(Instruction::I32Load8U(wasm_encoder::MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }));
}

pub(super) fn emit_text_utf8_boundary_test_from_locals<'a>(
    value_ptr: u32,
    value_len: u32,
    offset: u32,
    insns: &mut Vec<Instruction<'a>>,
) {
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I32GtU);
    insns.push(Instruction::If(BlockType::Result(ValType::I32)));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::If(BlockType::Result(ValType::I32)));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(value_len));
    insns.push(Instruction::I32Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I32)));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(value_ptr));
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Const(0xC0));
    insns.push(Instruction::I32And);
    insns.push(Instruction::I32Const(0x80));
    insns.push(Instruction::I32Ne);

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);
}
