use cranelift_codegen::ir::{
    InstBuilder, MemFlags, StackSlotData, TrapCode, condcodes::IntCC, stackslot::StackSlotKind,
    types,
};
use cranelift_frontend::FunctionBuilder;

use crate::native_codegen::{LowerResult, NativeCodegenCtx};

mod int_ops;

// ── lower_call ────────────────────────────────────────────────────────────

/// Lower `AnfExpr::Call` — arithmetic, comparison, and unary intrinsics.
///
/// Only pure register-level operations; no module references required.
pub(super) fn lower_call(
    func: &str,
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    if let Some(result) = int_ops::try_lower_int_call(func, args, ctx, builder) {
        return result;
    }
    if matches!(
        func,
        "time.duration_since" | "time_duration_since" | "std.time.duration_since"
    ) {
        return lower_time_duration_since_call(args, ctx, builder);
    }
    if matches!(
        func,
        "time.add_duration" | "time_add_duration" | "std.time.add_duration"
    ) {
        return lower_time_add_duration_call(args, ctx, builder);
    }
    if matches!(
        func,
        "time.instant_to_ms" | "time_instant_to_ms" | "std.time.instant_to_ms"
    ) {
        return lower_time_instant_to_ms_call(args, ctx, builder);
    }
    if matches!(
        func,
        "path.from_text"
            | "path_from_text"
            | "std.path.from_text"
            | "path.to_text"
            | "path_to_text"
            | "std.path.to_text"
    ) {
        return lower_path_identity_call(args, ctx, builder);
    }
    if matches!(func, "bytes.length" | "bytes_length" | "std.bytes.length") {
        return lower_bytes_length_call(args, ctx, builder);
    }
    if matches!(func, "bytes.at" | "bytes_at" | "std.bytes.at") {
        return lower_bytes_at_call(args, ctx, builder);
    }
    if matches!(func, "bytes.empty" | "bytes_empty" | "std.bytes.empty") {
        return lower_bytes_empty_call(args, ctx, builder);
    }

    match func {
        // ── binary comparisons → I8 ────────────────────────────
        "i64.eq" | "==" | "eq" | "i64.ne" | "!=" | "ne" | "i64.lt_s" | "<" | "lt" | "i64.le_s"
        | "<=" | "le" | "i64.gt_s" | ">" | "gt" | "i64.ge_s" | ">=" | "ge"
            if args.len() == 2 =>
        {
            let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (lhs, rhs) {
                (Some(l), Some(r)) => {
                    let cc = match func {
                        "i64.eq" | "==" | "eq" => IntCC::Equal,
                        "i64.ne" | "!=" | "ne" => IntCC::NotEqual,
                        "i64.lt_s" | "<" | "lt" => IntCC::SignedLessThan,
                        "i64.le_s" | "<=" | "le" => IntCC::SignedLessThanOrEqual,
                        "i64.gt_s" | ">" | "gt" => IntCC::SignedGreaterThan,
                        "i64.ge_s" | ">=" | "ge" => IntCC::SignedGreaterThanOrEqual,
                        _ => unreachable!(),
                    };
                    LowerResult::Value(builder.ins().icmp(cc, l, r))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        // ── unary ops ─────────────────────────────────────────
        "i64.neg" | "neg" | "negate" if args.len() == 1 => {
            match ctx.lookup(args[0].as_str()).map(|(v, _)| v) {
                Some(a) => LowerResult::Value(builder.ins().ineg(a)),
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "i64.eqz" | "not" | "!" if args.len() == 1 => {
            match ctx.lookup(args[0].as_str()).map(|(v, _)| v) {
                Some(a) => LowerResult::Value(builder.ins().icmp_imm(IntCC::Equal, a, 0)),
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        _ => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

fn lower_time_duration_since_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [later, earlier] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    let later = ctx.lookup(later.as_str()).map(|(v, _)| v);
    let earlier = ctx.lookup(earlier.as_str()).map(|(v, _)| v);
    match (later, earlier) {
        (Some(later), Some(earlier)) => LowerResult::Value(builder.ins().isub(later, earlier)),
        _ => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

fn lower_time_add_duration_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [instant, duration] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    let instant = ctx.lookup(instant.as_str()).map(|(v, _)| v);
    let duration = ctx.lookup(duration.as_str()).map(|(v, _)| v);
    match (instant, duration) {
        (Some(instant), Some(duration)) => {
            LowerResult::Value(builder.ins().iadd(instant, duration))
        }
        _ => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

fn lower_time_instant_to_ms_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [instant] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    match ctx.lookup(instant.as_str()).map(|(v, _)| v) {
        Some(value) => LowerResult::Value(value),
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

fn lower_bytes_length_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [arg] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    match ctx.lookup(arg.as_str()).map(|(v, _)| v) {
        Some(value) => LowerResult::Value(builder.ins().ushr_imm(value, 32)),
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

fn lower_bytes_at_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [bytes_arg, index_arg] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    let bytes = ctx.lookup(bytes_arg.as_str()).map(|(v, _)| v);
    let index = ctx.lookup(index_arg.as_str()).map(|(v, _)| v);
    let (Some(bytes), Some(index)) = (bytes, index) else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };

    let slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
    let none_tag = builder.ins().iconst(types::I64, 0);
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().stack_store(none_tag, slot, 0);
    builder.ins().stack_store(zero, slot, 8);

    let len = builder.ins().ushr_imm(bytes, 32);
    let index_non_negative = builder
        .ins()
        .icmp_imm(IntCC::SignedGreaterThanOrEqual, index, 0);
    let index_in_range = builder.ins().icmp(IntCC::SignedLessThan, index, len);
    let in_bounds = builder.ins().band(index_non_negative, index_in_range);

    let hit_block = builder.create_block();
    let done_block = builder.create_block();
    builder
        .ins()
        .brif(in_bounds, hit_block, &[], done_block, &[]);

    builder.switch_to_block(hit_block);
    builder.seal_block(hit_block);
    let some_tag = builder.ins().iconst(types::I64, 1);
    builder.ins().stack_store(some_tag, slot, 0);
    let ptr = builder.ins().band_imm(bytes, 0xffff_ffff);
    let addr = builder.ins().iadd(ptr, index);
    let byte = builder.ins().load(types::I8, MemFlags::trusted(), addr, 0);
    let byte_i64 = builder.ins().uextend(types::I64, byte);
    builder.ins().stack_store(byte_i64, slot, 8);
    builder.ins().jump(done_block, &[]);

    builder.switch_to_block(done_block);
    builder.seal_block(done_block);
    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
}

fn lower_bytes_empty_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [arg] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    match ctx.lookup(arg.as_str()).map(|(v, _)| v) {
        Some(value) => {
            let len = builder.ins().ushr_imm(value, 32);
            LowerResult::Value(builder.ins().icmp_imm(IntCC::Equal, len, 0))
        }
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

fn lower_path_identity_call(
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let [arg] = args else {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    };
    match ctx.lookup(arg.as_str()).map(|(v, _)| v) {
        Some(value) => LowerResult::Value(value),
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}
