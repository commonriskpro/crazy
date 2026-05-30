use cranelift_codegen::ir::{InstBuilder, TrapCode, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;

use crate::native_codegen::{LowerResult, NativeCodegenCtx};

mod checked;

pub(super) fn try_lower_int_call(
    func: &str,
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> Option<LowerResult> {
    let result = match func {
        // ── binary I64 arithmetic ──────────────────────────────
        "i64.add" | "+" | "add" | "i64.sub" | "-" | "sub" | "i64.mul" | "*" | "mul"
        | "i64.div_s" | "/" | "div" | "i64.rem_s" | "%" | "mod" | "i64.and" | "and"
        | "int.bit_and" | "int_bit_and" | "i64.or" | "or" | "int.bit_or" | "int_bit_or"
        | "i64.xor" | "xor" | "int.bit_xor" | "int_bit_xor"
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
                        "i64.and" | "and" | "int.bit_and" | "int_bit_and" => {
                            builder.ins().band(l, r)
                        }
                        "i64.or" | "or" | "int.bit_or" | "int_bit_or" => builder.ins().bor(l, r),
                        "i64.xor" | "xor" | "int.bit_xor" | "int_bit_xor" => {
                            builder.ins().bxor(l, r)
                        }
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
        "int.wrapping_sub" | "int_wrapping_sub" if args.len() == 2 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (left, right) {
                (Some(left), Some(right)) => LowerResult::Value(builder.ins().isub(left, right)),
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.wrapping_mul" | "int_wrapping_mul" if args.len() == 2 => {
            let left = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let right = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (left, right) {
                (Some(left), Some(right)) => LowerResult::Value(builder.ins().imul(left, right)),
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.shift_left" | "int_shift_left" if args.len() == 2 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let amount = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (value, amount) {
                (Some(value), Some(amount)) => {
                    LowerResult::Value(builder.ins().ishl(value, amount))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.shift_right" | "int_shift_right" if args.len() == 2 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let amount = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (value, amount) {
                (Some(value), Some(amount)) => {
                    LowerResult::Value(builder.ins().sshr(value, amount))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.shift_right_unsigned" | "int_shift_right_unsigned" if args.len() == 2 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            let amount = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
            match (value, amount) {
                (Some(value), Some(amount)) => {
                    LowerResult::Value(builder.ins().ushr(value, amount))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.bit_not" | "int_bit_not" if args.len() == 1 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            match value {
                Some(value) => {
                    let all_ones = builder.ins().iconst(types::I64, -1);
                    LowerResult::Value(builder.ins().bxor(value, all_ones))
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }
        "int.wrapping_neg" | "int_wrapping_neg" if args.len() == 1 => {
            let value = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
            match value {
                Some(value) => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    LowerResult::Value(builder.ins().isub(zero, value))
                }
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
        _ => return None,
    };
    Some(result)
}
