use cranelift_codegen::ir::{InstBuilder, TrapCode, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;

use crate::native_codegen::{LowerResult, NativeCodegenCtx};

pub(super) fn try_lower_checked_int_call(
    func: &str,
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> Option<LowerResult> {
    let result = match func {
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

        _ => return None,
    };
    Some(result)
}
