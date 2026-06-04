use cranelift_codegen::ir::{BlockArg, InstBuilder, TrapCode, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;

use crate::anf::{AnfExpr, AnfMatchArm};
use crate::native_codegen::{LowerResult, NativeCodegenCtx, infer_cranelift_return_type};

// ── lower_if ─────────────────────────────────────────────────────────────

pub(crate) fn lower_if(
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
    match super::super::lower_anf_expr_cranelift(then_branch, ctx, builder, module) {
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
    match super::super::lower_anf_expr_cranelift(else_branch, ctx, builder, module) {
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

pub(crate) fn lower_match(
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
            match super::super::lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
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
            match super::super::lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
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
            match super::super::lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
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
