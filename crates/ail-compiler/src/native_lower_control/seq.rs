use cranelift_codegen::ir::{InstBuilder, TrapCode};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;

use crate::anf::AnfExpr;
use crate::native_codegen::{LowerResult, NativeCodegenCtx};

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
        if let LowerResult::Terminated =
            super::super::lower_anf_expr_cranelift(expr, ctx, builder, module)
        {
            return LowerResult::Terminated;
        }
    }
    super::super::lower_anf_expr_cranelift(&exprs[last_idx], ctx, builder, module)
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
