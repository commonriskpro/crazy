use cranelift_codegen::ir::{BlockArg, InstBuilder, TrapCode, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;

use crate::anf::AnfExpr;
use crate::native_codegen::{LowerResult, NativeCodegenCtx};

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
    let right_val = match super::super::lower_anf_expr_cranelift(right, ctx, builder, module) {
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
    let right_val = match super::super::lower_anf_expr_cranelift(right, ctx, builder, module) {
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
