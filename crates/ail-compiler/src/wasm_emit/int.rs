use super::control::emit_local_as_i64;
use super::*;

pub(super) fn emit_int_min<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, left, insns);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::I64LeS);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, left, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_max<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, left, insns);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::I64GeS);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, left, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_clamp<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, low, high] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, low, insns);
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, low, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, high, insns);
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, high, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_abs_or<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, fallback] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, fallback, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(0));
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Sub);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_neg_or<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, fallback] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, fallback, insns);
    insns.push(Instruction::Else);
    insns.push(Instruction::I64Const(0));
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Sub);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_add_or<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left_arg, right_arg, fallback_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, left_arg, insns);
    insns.push(Instruction::LocalSet(left));

    let right = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, right_arg, insns);
    insns.push(Instruction::LocalSet(right));

    let fallback = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, fallback_arg, insns);
    insns.push(Instruction::LocalSet(fallback));

    let sum = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Add);
    insns.push(Instruction::LocalSet(sum));

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::LocalGet(sum));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::I32And);

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::LocalGet(sum));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::I32And);

    insns.push(Instruction::I32Or);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(fallback));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(sum));
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_sub_or<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left_arg, right_arg, fallback_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, left_arg, insns);
    insns.push(Instruction::LocalSet(left));

    let right = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, right_arg, insns);
    insns.push(Instruction::LocalSet(right));

    let fallback = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, fallback_arg, insns);
    insns.push(Instruction::LocalSet(fallback));

    let diff = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Sub);
    insns.push(Instruction::LocalSet(diff));

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::LocalGet(diff));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::I32And);

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::LocalGet(diff));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::I32And);

    insns.push(Instruction::I32Or);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(fallback));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(diff));
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_mul_or<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left_arg, right_arg, fallback_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, left_arg, insns);
    insns.push(Instruction::LocalSet(left));

    let right = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, right_arg, insns);
    insns.push(Instruction::LocalSet(right));

    let fallback = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, fallback_arg, insns);
    insns.push(Instruction::LocalSet(fallback));

    let product = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Mul);
    insns.push(Instruction::LocalSet(product));

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(fallback));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64DivS);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(fallback));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::End);

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_shift_left<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, amount] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, amount, insns);
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::I64Shl);

    Some(ValType::I64)
}

pub(super) fn emit_int_shift_right<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, amount] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, amount, insns);
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::I64ShrS);

    Some(ValType::I64)
}

pub(super) fn emit_int_shift_right_unsigned<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, amount] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, amount, insns);
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::I64ShrU);

    Some(ValType::I64)
}

pub(super) fn emit_int_wrapping_add<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, left, insns);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::I64Add);

    Some(ValType::I64)
}

pub(super) fn emit_int_wrapping_sub<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, left, insns);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::I64Sub);

    Some(ValType::I64)
}

pub(super) fn emit_int_wrapping_mul<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left, right] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, left, insns);
    emit_local_as_i64(ctx, right, insns);
    insns.push(Instruction::I64Mul);

    Some(ValType::I64)
}

pub(super) fn emit_int_wrapping_neg<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    insns.push(Instruction::I64Const(0));
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Sub);

    Some(ValType::I64)
}

pub(super) fn emit_int_saturating_neg<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(i64::MAX));
    insns.push(Instruction::Else);
    insns.push(Instruction::I64Const(0));
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Sub);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_saturating_add<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left_arg, right_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, left_arg, insns);
    insns.push(Instruction::LocalSet(left));

    let right = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, right_arg, insns);
    insns.push(Instruction::LocalSet(right));

    let sum = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Add);
    insns.push(Instruction::LocalSet(sum));

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::LocalGet(sum));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(i64::MAX));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::LocalGet(sum));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(sum));
    insns.push(Instruction::End);

    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_saturating_sub<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left_arg, right_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, left_arg, insns);
    insns.push(Instruction::LocalSet(left));

    let right = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, right_arg, insns);
    insns.push(Instruction::LocalSet(right));

    let diff = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Sub);
    insns.push(Instruction::LocalSet(diff));

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::LocalGet(diff));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64GtS);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::LocalGet(diff));
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(i64::MAX));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(diff));
    insns.push(Instruction::End);

    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_saturating_mul<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [left_arg, right_arg] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let left = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, left_arg, insns);
    insns.push(Instruction::LocalSet(left));

    let right = ctx.bind_temp(ValType::I64);
    emit_local_as_i64(ctx, right_arg, insns);
    insns.push(Instruction::LocalSet(right));

    let product = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Mul);
    insns.push(Instruction::LocalSet(product));

    let clamp = ctx.bind_temp(ValType::I64);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64LtS);
    insns.push(Instruction::I32Xor);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::Else);
    insns.push(Instruction::I64Const(i64::MAX));
    insns.push(Instruction::End);
    insns.push(Instruction::LocalSet(clamp));

    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(clamp));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::LocalGet(right));
    insns.push(Instruction::I64DivS);
    insns.push(Instruction::LocalGet(left));
    insns.push(Instruction::I64Ne);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    insns.push(Instruction::LocalGet(clamp));
    insns.push(Instruction::Else);
    insns.push(Instruction::LocalGet(product));
    insns.push(Instruction::End);

    insns.push(Instruction::End);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_div_or<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, divisor, fallback] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, divisor, insns);
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, fallback, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    emit_local_as_i64(ctx, divisor, insns);
    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, fallback, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, divisor, insns);
    insns.push(Instruction::I64DivS);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    Some(ValType::I64)
}

pub(super) fn emit_int_rem_or<'a>(
    args: &[String],
    ctx: &WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let [value, divisor, fallback] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    emit_local_as_i64(ctx, divisor, insns);
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, fallback, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    insns.push(Instruction::I64Const(i64::MIN));
    insns.push(Instruction::I64Eq);
    emit_local_as_i64(ctx, divisor, insns);
    insns.push(Instruction::I64Const(-1));
    insns.push(Instruction::I64Eq);
    insns.push(Instruction::I32And);
    insns.push(Instruction::If(BlockType::Result(ValType::I64)));
    emit_local_as_i64(ctx, fallback, insns);
    insns.push(Instruction::Else);
    emit_local_as_i64(ctx, value, insns);
    emit_local_as_i64(ctx, divisor, insns);
    insns.push(Instruction::I64RemS);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    Some(ValType::I64)
}
