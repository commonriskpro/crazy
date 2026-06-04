use cranelift_codegen::ir::{BlockArg, InstBuilder, TrapCode};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;

use crate::anf::AnfExpr;
use crate::native_codegen::{
    LowerResult, NativeCodegenCtx, NativeLabelKind, infer_cranelift_return_type,
};

// ── lower_loop ────────────────────────────────────────────────────────────

pub(crate) fn lower_loop(
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

    let body_result = super::super::lower_anf_expr_cranelift(body, ctx, builder, module);

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

pub(crate) fn lower_break(
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
    match super::super::lower_anf_expr_cranelift(value, ctx, builder, module) {
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

pub(crate) fn lower_continue(
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

pub(crate) fn lower_while_loop(
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
    let while_body_result = super::super::lower_anf_expr_cranelift(body, ctx, builder, module);
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
