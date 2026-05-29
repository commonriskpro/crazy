// ── ail-compiler::native_lower::control ──────────────────────────────────
//
// Control-flow ANF expression lowering helpers.
//
// Covers: Call (arithmetic / comparison / unary intrinsics), Loop, Break,
// Continue, WhileLoop, If, Match, ShortCircuitAnd, ShortCircuitOr, Seq,
// RuntimeCheck.

use cranelift_codegen::ir::{BlockArg, InstBuilder, TrapCode, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;

use crate::anf::{AnfExpr, AnfMatchArm};
use crate::native_codegen::{
    LowerResult, NativeCodegenCtx, NativeLabelKind, infer_cranelift_return_type,
};

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
    match func {
        // ── binary I64 arithmetic ──────────────────────────────
        "i64.add" | "+" | "add" | "i64.sub" | "-" | "sub" | "i64.mul" | "*" | "mul"
        | "i64.div_s" | "/" | "div" | "i64.rem_s" | "%" | "mod" | "i64.and" | "and" | "i64.or"
        | "or"
            if args.len() == 2 =>
        {
            let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (lhs, rhs) {
                (Some(l), Some(r)) => {
                    let val = match func {
                        "i64.add" | "+" | "add" => builder.ins().iadd(l, r),
                        "i64.sub" | "-" | "sub" => builder.ins().isub(l, r),
                        "i64.mul" | "*" | "mul" => builder.ins().imul(l, r),
                        "i64.div_s" | "/" | "div" => builder.ins().sdiv(l, r),
                        "i64.rem_s" | "%" | "mod" => builder.ins().srem(l, r),
                        "i64.and" | "and" => builder.ins().band(l, r),
                        "i64.or" | "or" => builder.ins().bor(l, r),
                        _ => unreachable!(),
                    };
                    LowerResult::Value(val)
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        // ── Int bounds helpers → I64 ────────────────────────────
        "int.min" | "int_min" | "int.max" | "int_max" if args.len() == 2 => {
            let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (lhs, rhs) {
                (Some(l), Some(r)) => {
                    let cc = if matches!(func, "int.min" | "int_min") {
                        IntCC::SignedLessThanOrEqual
                    } else {
                        IntCC::SignedGreaterThanOrEqual
                    };
                    let keep_left = builder.ins().icmp(cc, l, r);
                    LowerResult::Value(builder.ins().select(keep_left, l, r))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.clamp" | "int_clamp" if args.len() == 3 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let low = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            let high = ctx.lookup(args[2].as_str()).map(|(v, _)| v);
            match (value, low, high) {
                (Some(value), Some(low), Some(high)) => {
                    let below_low = builder.ins().icmp(IntCC::SignedLessThan, value, low);
                    let low_or_value = builder.ins().select(below_low, low, value);
                    let above_high =
                        builder
                            .ins()
                            .icmp(IntCC::SignedGreaterThan, low_or_value, high);
                    LowerResult::Value(builder.ins().select(above_high, high, low_or_value))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.abs_or" | "int_abs_or" if args.len() == 2 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (value, fallback) {
                (Some(value), Some(fallback)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let min_value = builder.ins().iconst(types::I64, i64::MIN);
                    let is_min = builder.ins().icmp(IntCC::Equal, value, min_value);
                    let is_negative = builder.ins().icmp(IntCC::SignedLessThan, value, zero);
                    let negated = builder.ins().isub(zero, value);
                    let abs_or_value = builder.ins().select(is_negative, negated, value);
                    LowerResult::Value(builder.ins().select(is_min, fallback, abs_or_value))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }

        "int.neg_or" | "int_neg_or" if args.len() == 2 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (value, fallback) {
                (Some(value), Some(fallback)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let min_value = builder.ins().iconst(types::I64, i64::MIN);
                    let is_min = builder.ins().icmp(IntCC::Equal, value, min_value);
                    let negated = builder.ins().isub(zero, value);
                    LowerResult::Value(builder.ins().select(is_min, fallback, negated))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }

        "int.wrapping_add" | "int_wrapping_add" if args.len() == 2 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (left, right) {
                (Some(left), Some(right)) => LowerResult::Value(builder.ins().iadd(left, right)),
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.saturating_neg" | "int_saturating_neg" if args.len() == 1 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            match value {
                Some(value) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let max = builder.ins().iconst(types::I64, i64::MAX);
                    let min = builder.ins().iconst(types::I64, i64::MIN);
                    let is_min = builder.ins().icmp(IntCC::Equal, value, min);
                    let negated = builder.ins().isub(zero, value);
                    LowerResult::Value(builder.ins().select(is_min, max, negated))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.add_or" | "int_add_or" if args.len() == 3 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[2].as_str()).map(|(v, _)| v);
            match (left, right, fallback) {
                (Some(left), Some(right), Some(fallback)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let sum = builder.ins().iadd(left, right);
                    let right_positive = builder.ins().icmp(IntCC::SignedGreaterThan, right, zero);
                    let sum_lt_left = builder.ins().icmp(IntCC::SignedLessThan, sum, left);
                    let pos_overflow = builder.ins().band(right_positive, sum_lt_left);
                    let right_negative = builder.ins().icmp(IntCC::SignedLessThan, right, zero);
                    let sum_gt_left = builder.ins().icmp(IntCC::SignedGreaterThan, sum, left);
                    let neg_overflow = builder.ins().band(right_negative, sum_gt_left);
                    let overflow = builder.ins().bor(pos_overflow, neg_overflow);
                    LowerResult::Value(builder.ins().select(overflow, fallback, sum))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.sub_or" | "int_sub_or" if args.len() == 3 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[2].as_str()).map(|(v, _)| v);
            match (left, right, fallback) {
                (Some(left), Some(right), Some(fallback)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let diff = builder.ins().isub(left, right);
                    let right_positive = builder.ins().icmp(IntCC::SignedGreaterThan, right, zero);
                    let diff_gt_left = builder.ins().icmp(IntCC::SignedGreaterThan, diff, left);
                    let pos_overflow = builder.ins().band(right_positive, diff_gt_left);
                    let right_negative = builder.ins().icmp(IntCC::SignedLessThan, right, zero);
                    let diff_lt_left = builder.ins().icmp(IntCC::SignedLessThan, diff, left);
                    let neg_overflow = builder.ins().band(right_negative, diff_lt_left);
                    let overflow = builder.ins().bor(pos_overflow, neg_overflow);
                    LowerResult::Value(builder.ins().select(overflow, fallback, diff))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.mul_or" | "int_mul_or" if args.len() == 3 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[2].as_str()).map(|(v, _)| v);
            match (left, right, fallback) {
                (Some(left), Some(right), Some(fallback)) => {
                    let (product, overflow) = builder.ins().smul_overflow(left, right);
                    LowerResult::Value(builder.ins().select(overflow, fallback, product))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.saturating_add" | "int_saturating_add" if args.len() == 2 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (left, right) {
                (Some(left), Some(right)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let max = builder.ins().iconst(types::I64, i64::MAX);
                    let min = builder.ins().iconst(types::I64, i64::MIN);
                    let sum = builder.ins().iadd(left, right);
                    let right_positive = builder.ins().icmp(IntCC::SignedGreaterThan, right, zero);
                    let sum_lt_left = builder.ins().icmp(IntCC::SignedLessThan, sum, left);
                    let pos_overflow = builder.ins().band(right_positive, sum_lt_left);
                    let right_negative = builder.ins().icmp(IntCC::SignedLessThan, right, zero);
                    let sum_gt_left = builder.ins().icmp(IntCC::SignedGreaterThan, sum, left);
                    let neg_overflow = builder.ins().band(right_negative, sum_gt_left);
                    let high_or_sum = builder.ins().select(pos_overflow, max, sum);
                    LowerResult::Value(builder.ins().select(neg_overflow, min, high_or_sum))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.saturating_sub" | "int_saturating_sub" if args.len() == 2 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (left, right) {
                (Some(left), Some(right)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let max = builder.ins().iconst(types::I64, i64::MAX);
                    let min = builder.ins().iconst(types::I64, i64::MIN);
                    let diff = builder.ins().isub(left, right);
                    let right_positive = builder.ins().icmp(IntCC::SignedGreaterThan, right, zero);
                    let diff_gt_left = builder.ins().icmp(IntCC::SignedGreaterThan, diff, left);
                    let pos_underflow = builder.ins().band(right_positive, diff_gt_left);
                    let right_negative = builder.ins().icmp(IntCC::SignedLessThan, right, zero);
                    let diff_lt_left = builder.ins().icmp(IntCC::SignedLessThan, diff, left);
                    let neg_overflow = builder.ins().band(right_negative, diff_lt_left);
                    let low_or_diff = builder.ins().select(pos_underflow, min, diff);
                    LowerResult::Value(builder.ins().select(neg_overflow, max, low_or_diff))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.saturating_mul" | "int_saturating_mul" if args.len() == 2 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (left, right) {
                (Some(left), Some(right)) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    let max = builder.ins().iconst(types::I64, i64::MAX);
                    let min = builder.ins().iconst(types::I64, i64::MIN);
                    let (product, overflow) = builder.ins().smul_overflow(left, right);
                    let left_negative = builder.ins().icmp(IntCC::SignedLessThan, left, zero);
                    let right_negative = builder.ins().icmp(IntCC::SignedLessThan, right, zero);
                    let sign_diff = builder.ins().bxor(left_negative, right_negative);
                    let clamp = builder.ins().select(sign_diff, min, max);
                    LowerResult::Value(builder.ins().select(overflow, clamp, product))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.div_or" | "int_div_or" if args.len() == 3 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let divisor = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[2].as_str()).map(|(v, _)| v);
            match (value, divisor, fallback) {
                (Some(value), Some(divisor), Some(fallback)) => {
                    let fallback_block = builder.create_block();
                    let overflow_check_block = builder.create_block();
                    let divisor_minus_one_check_block = builder.create_block();
                    let div_block = builder.create_block();
                    let merge_block = builder.create_block();
                    builder.append_block_param(merge_block, types::I64);

                    let zero = builder.ins().iconst(types::I64, 0);
                    let divisor_is_zero = builder.ins().icmp(IntCC::Equal, divisor, zero);
                    builder.ins().brif(
                        divisor_is_zero,
                        fallback_block,
                        &[],
                        overflow_check_block,
                        &[],
                    );

                    builder.switch_to_block(overflow_check_block);
                    builder.seal_block(overflow_check_block);
                    let min_value = builder.ins().iconst(types::I64, i64::MIN);
                    let value_is_min = builder.ins().icmp(IntCC::Equal, value, min_value);
                    builder.ins().brif(
                        value_is_min,
                        divisor_minus_one_check_block,
                        &[],
                        div_block,
                        &[],
                    );

                    builder.switch_to_block(divisor_minus_one_check_block);
                    builder.seal_block(divisor_minus_one_check_block);
                    let minus_one = builder.ins().iconst(types::I64, -1);
                    let divisor_is_minus_one = builder.ins().icmp(IntCC::Equal, divisor, minus_one);
                    builder
                        .ins()
                        .brif(divisor_is_minus_one, fallback_block, &[], div_block, &[]);

                    builder.switch_to_block(div_block);
                    builder.seal_block(div_block);
                    let quotient = builder.ins().sdiv(value, divisor);
                    builder
                        .ins()
                        .jump(merge_block, &[BlockArg::Value(quotient)]);

                    builder.switch_to_block(fallback_block);
                    builder.seal_block(fallback_block);
                    builder
                        .ins()
                        .jump(merge_block, &[BlockArg::Value(fallback)]);

                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                    LowerResult::Value(builder.block_params(merge_block)[0])
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }

        "int.rem_or" | "int_rem_or" if args.len() == 3 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let divisor = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            let fallback = ctx.lookup(args[2].as_str()).map(|(v, _)| v);
            match (value, divisor, fallback) {
                (Some(value), Some(divisor), Some(fallback)) => {
                    let fallback_block = builder.create_block();
                    let overflow_check_block = builder.create_block();
                    let divisor_minus_one_check_block = builder.create_block();
                    let rem_block = builder.create_block();
                    let merge_block = builder.create_block();
                    builder.append_block_param(merge_block, types::I64);

                    let zero = builder.ins().iconst(types::I64, 0);
                    let divisor_is_zero = builder.ins().icmp(IntCC::Equal, divisor, zero);
                    builder.ins().brif(
                        divisor_is_zero,
                        fallback_block,
                        &[],
                        overflow_check_block,
                        &[],
                    );

                    builder.switch_to_block(overflow_check_block);
                    builder.seal_block(overflow_check_block);
                    let min_value = builder.ins().iconst(types::I64, i64::MIN);
                    let value_is_min = builder.ins().icmp(IntCC::Equal, value, min_value);
                    builder.ins().brif(
                        value_is_min,
                        divisor_minus_one_check_block,
                        &[],
                        rem_block,
                        &[],
                    );

                    builder.switch_to_block(divisor_minus_one_check_block);
                    builder.seal_block(divisor_minus_one_check_block);
                    let minus_one = builder.ins().iconst(types::I64, -1);
                    let divisor_is_minus_one = builder.ins().icmp(IntCC::Equal, divisor, minus_one);
                    builder
                        .ins()
                        .brif(divisor_is_minus_one, fallback_block, &[], rem_block, &[]);

                    builder.switch_to_block(rem_block);
                    builder.seal_block(rem_block);
                    let remainder = builder.ins().srem(value, divisor);
                    builder
                        .ins()
                        .jump(merge_block, &[BlockArg::Value(remainder)]);

                    builder.switch_to_block(fallback_block);
                    builder.seal_block(fallback_block);
                    builder
                        .ins()
                        .jump(merge_block, &[BlockArg::Value(fallback)]);

                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                    LowerResult::Value(builder.block_params(merge_block)[0])
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }

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

// ── lower_loop ────────────────────────────────────────────────────────────

pub(super) fn lower_loop(
    body: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let break_block = builder.create_block();
    let loop_block = builder.create_block();

    let result_ty = infer_cranelift_return_type(body);
    if let Some(ty) = result_ty {
        builder.append_block_param(break_block, ty);
    }

    // Jump into the loop.
    builder.ins().jump(loop_block, &[]);
    builder.switch_to_block(loop_block);
    // DO NOT seal loop_block yet — back-edges from Continue not emitted.
    ctx.push_label(NativeLabelKind::LoopBreak, break_block);
    ctx.push_label(NativeLabelKind::LoopContinue, loop_block);

    let body_result = super::lower_anf_expr_cranelift(body, ctx, builder, module);

    ctx.pop_label(); // LoopContinue
    ctx.pop_label(); // LoopBreak

    // Implicit fall-through (no explicit break): jump back to header.
    if !matches!(body_result, LowerResult::Terminated) {
        builder.ins().jump(loop_block, &[]);
    }
    builder.seal_block(loop_block);
    builder.switch_to_block(break_block);
    builder.seal_block(break_block);

    match result_ty {
        Some(_) => LowerResult::Value(builder.block_params(break_block)[0]),
        None => LowerResult::Unit,
    }
}

// ── lower_break ───────────────────────────────────────────────────────────

pub(super) fn lower_break(
    value: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let break_block = match ctx.find_label(NativeLabelKind::LoopBreak) {
        Some(b) => b,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    match super::lower_anf_expr_cranelift(value, ctx, builder, module) {
        LowerResult::Value(v) => {
            builder.ins().jump(break_block, &[BlockArg::Value(v)]);
        }
        LowerResult::Unit => {
            builder.ins().jump(break_block, &[]);
        }
        LowerResult::Terminated => {}
    }
    LowerResult::Terminated
}

// ── lower_continue ────────────────────────────────────────────────────────

pub(super) fn lower_continue(
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let loop_block = match ctx.find_label(NativeLabelKind::LoopContinue) {
        Some(b) => b,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    builder.ins().jump(loop_block, &[]);
    LowerResult::Terminated
}

// ── lower_while_loop ──────────────────────────────────────────────────────

pub(super) fn lower_while_loop(
    cond: &str,
    body: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let break_block = builder.create_block();
    let loop_block = builder.create_block();
    let body_block = builder.create_block();

    // Jump into the loop header.
    builder.ins().jump(loop_block, &[]);
    builder.switch_to_block(loop_block);
    // DO NOT seal loop_block yet.

    let cond_val = match ctx.lookup(cond) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    // brif cond → body_block, else → break_block (exit on false)
    builder
        .ins()
        .brif(cond_val, body_block, &[], break_block, &[]);

    builder.switch_to_block(body_block);
    builder.seal_block(body_block);
    ctx.push_label(NativeLabelKind::LoopBreak, break_block);
    ctx.push_label(NativeLabelKind::LoopContinue, loop_block);
    let while_body_result = super::lower_anf_expr_cranelift(body, ctx, builder, module);
    ctx.pop_label(); // LoopContinue
    ctx.pop_label(); // LoopBreak
    if !matches!(while_body_result, LowerResult::Terminated) {
        builder.ins().jump(loop_block, &[]);
    }

    builder.seal_block(loop_block);
    builder.switch_to_block(break_block);
    builder.seal_block(break_block);
    LowerResult::Unit
}

// ── lower_if ─────────────────────────────────────────────────────────────

pub(super) fn lower_if(
    cond: &str,
    then_branch: &AnfExpr,
    else_branch: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let cond_val = match ctx.lookup(cond) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };

    let then_block = builder.create_block();
    let else_block = builder.create_block();
    let merge_block = builder.create_block();

    let result_ty = infer_cranelift_return_type(then_branch);
    if let Some(ty) = result_ty {
        builder.append_block_param(merge_block, ty);
    }

    builder
        .ins()
        .brif(cond_val, then_block, &[], else_block, &[]);

    builder.switch_to_block(then_block);
    builder.seal_block(then_block);
    match super::lower_anf_expr_cranelift(then_branch, ctx, builder, module) {
        LowerResult::Value(v) => {
            builder.ins().jump(merge_block, &[BlockArg::Value(v)]);
        }
        LowerResult::Unit => {
            builder.ins().jump(merge_block, &[]);
        }
        LowerResult::Terminated => {}
    }

    builder.switch_to_block(else_block);
    builder.seal_block(else_block);
    match super::lower_anf_expr_cranelift(else_branch, ctx, builder, module) {
        LowerResult::Value(v) => {
            builder.ins().jump(merge_block, &[BlockArg::Value(v)]);
        }
        LowerResult::Unit => {
            builder.ins().jump(merge_block, &[]);
        }
        LowerResult::Terminated => {}
    }

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    match result_ty {
        Some(_) => LowerResult::Value(builder.block_params(merge_block)[0]),
        None => LowerResult::Unit,
    }
}

// ── lower_match ───────────────────────────────────────────────────────────

pub(super) fn lower_match(
    scrutinee: &str,
    arms: &[AnfMatchArm],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    // Empty arms → trap.
    if arms.is_empty() {
        builder.ins().trap(TrapCode::user(1).unwrap());
        return LowerResult::Terminated;
    }

    let (scrutinee_val, scrutinee_ty) = match ctx.lookup(scrutinee) {
        Some(pair) => pair,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };

    // Normalize scrutinee to I64 for pattern comparisons (extend I8 bools).
    let scrutinee_i64 = if scrutinee_ty == types::I64 {
        scrutinee_val
    } else {
        builder.ins().uextend(types::I64, scrutinee_val)
    };

    let result_ty = infer_cranelift_return_type(&arms[0].body);
    let merge_block = builder.create_block();
    if let Some(ty) = result_ty {
        builder.append_block_param(merge_block, ty);
    }

    // Linear scan through arms. After each non-wildcard check, if the
    // pattern doesn't match we jump to the next check block.
    // The wildcard or last arm terminates the chain.
    let n = arms.len();
    let mut has_merge_predecessor = false;
    for (i, arm) in arms.iter().enumerate() {
        let arm_block = builder.create_block();

        if arm.pattern == "_" {
            // Wildcard: unconditional jump into arm_block.
            builder.ins().jump(arm_block, &[]);
            builder.switch_to_block(arm_block);
            builder.seal_block(arm_block);
            // Lower arm body and jump to merge.
            match super::lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                LowerResult::Value(v) => {
                    builder.ins().jump(merge_block, &[BlockArg::Value(v)]);
                    has_merge_predecessor = true;
                }
                LowerResult::Unit => {
                    builder.ins().jump(merge_block, &[]);
                    has_merge_predecessor = true;
                }
                LowerResult::Terminated => {}
            }
            // Wildcard terminates the chain; remaining arms (if any) are unreachable.
            break;
        }

        // Non-wildcard: compute equality check.
        let pattern_val: i64 = match arm.pattern.trim() {
            "true" => 1,
            "false" => 0,
            s => match s.parse::<i64>() {
                Ok(value) => value,
                Err(_) => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    if has_merge_predecessor {
                        break;
                    }
                    return LowerResult::Terminated;
                }
            },
        };
        let pat_const = builder.ins().iconst(types::I64, pattern_val);
        let eq = builder.ins().icmp(IntCC::Equal, scrutinee_i64, pat_const);

        if i + 1 < n {
            // More arms follow: create a next_check block for the false branch.
            let next_check = builder.create_block();
            builder.ins().brif(eq, arm_block, &[], next_check, &[]);
            // Emit arm body in arm_block.
            builder.switch_to_block(arm_block);
            builder.seal_block(arm_block);
            match super::lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                LowerResult::Value(v) => {
                    builder.ins().jump(merge_block, &[BlockArg::Value(v)]);
                    has_merge_predecessor = true;
                }
                LowerResult::Unit => {
                    builder.ins().jump(merge_block, &[]);
                    has_merge_predecessor = true;
                }
                LowerResult::Terminated => {}
            }
            // Switch to next_check for the next iteration.
            builder.switch_to_block(next_check);
            builder.seal_block(next_check);
        } else {
            // Last arm and not wildcard: trap if pattern doesn't match.
            let trap_block = builder.create_block();
            builder.ins().brif(eq, arm_block, &[], trap_block, &[]);
            // Trap block.
            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            builder.ins().trap(TrapCode::user(1).unwrap());
            // Arm block.
            builder.switch_to_block(arm_block);
            builder.seal_block(arm_block);
            match super::lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                LowerResult::Value(v) => {
                    builder.ins().jump(merge_block, &[BlockArg::Value(v)]);
                    has_merge_predecessor = true;
                }
                LowerResult::Unit => {
                    builder.ins().jump(merge_block, &[]);
                    has_merge_predecessor = true;
                }
                LowerResult::Terminated => {}
            }
        }
    }

    if !has_merge_predecessor {
        return LowerResult::Terminated;
    }

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    match result_ty {
        Some(_) => LowerResult::Value(builder.block_params(merge_block)[0]),
        None => LowerResult::Unit,
    }
}

// ── lower_short_circuit_and ───────────────────────────────────────────────

pub(super) fn lower_short_circuit_and(
    left: &str,
    right: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let left_val = match ctx.lookup(left) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let true_block = builder.create_block();
    let false_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    builder
        .ins()
        .brif(left_val, true_block, &[], false_block, &[]);

    // true branch: evaluate right
    builder.switch_to_block(true_block);
    builder.seal_block(true_block);
    let right_val = match super::lower_anf_expr_cranelift(right, ctx, builder, module) {
        LowerResult::Value(v) => v,
        _ => builder.ins().iconst(types::I64, 0),
    };
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(right_val)]);

    // false branch: short-circuit → 0
    builder.switch_to_block(false_block);
    builder.seal_block(false_block);
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(zero)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    LowerResult::Value(builder.block_params(merge_block)[0])
}

// ── lower_short_circuit_or ────────────────────────────────────────────────

pub(super) fn lower_short_circuit_or(
    left: &str,
    right: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let left_val = match ctx.lookup(left) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let true_block = builder.create_block();
    let false_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    builder
        .ins()
        .brif(left_val, true_block, &[], false_block, &[]);

    // true branch: short-circuit → 1
    builder.switch_to_block(true_block);
    builder.seal_block(true_block);
    let one = builder.ins().iconst(types::I64, 1);
    builder.ins().jump(merge_block, &[BlockArg::Value(one)]);

    // false branch: evaluate right
    builder.switch_to_block(false_block);
    builder.seal_block(false_block);
    let right_val = match super::lower_anf_expr_cranelift(right, ctx, builder, module) {
        LowerResult::Value(v) => v,
        _ => builder.ins().iconst(types::I64, 0),
    };
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(right_val)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    LowerResult::Value(builder.block_params(merge_block)[0])
}

// ── lower_seq ─────────────────────────────────────────────────────────────

pub(super) fn lower_seq(
    exprs: &[AnfExpr],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    if exprs.is_empty() {
        return LowerResult::Unit;
    }
    let last_idx = exprs.len() - 1;
    for expr in &exprs[..last_idx] {
        // Lower each non-last expression; result is intentionally dropped.
        if let LowerResult::Terminated = super::lower_anf_expr_cranelift(expr, ctx, builder, module)
        {
            return LowerResult::Terminated;
        }
    }
    super::lower_anf_expr_cranelift(&exprs[last_idx], ctx, builder, module)
}

// ── lower_runtime_check ───────────────────────────────────────────────────

pub(super) fn lower_runtime_check(
    cond: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let cond_val = match ctx.lookup(cond) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let trap_block = builder.create_block();
    let fallthrough = builder.create_block();

    // brif cond → trap_block (trap if cond non-zero), else fallthrough
    builder
        .ins()
        .brif(cond_val, trap_block, &[], fallthrough, &[]);

    builder.switch_to_block(trap_block);
    builder.seal_block(trap_block);
    builder.ins().trap(TrapCode::user(1).unwrap());

    builder.switch_to_block(fallthrough);
    builder.seal_block(fallthrough);
    LowerResult::Unit
}
