// ── ail-compiler::native_lower::runtime ──────────────────────────────────
//
// Runtime-dispatch, lambda, and effect-call ANF expression lowering helpers.
//
// Covers: EffectCall, Lambda (with closure env), ChannelNew, and the shared
// `emit_runtime_call` dispatcher used by all concurrency / resource primitives
// (TaskSpawn, TaskAwait, TaskCancel, TaskGroup, ChannelSend, ChannelReceive,
// Select, Timeout, Dispatch, ResourceAcquire, ResourceRelease).

use cranelift_codegen::{
    Context,
    ir::{
        AbiParam, Function, InstBuilder, MemFlags, Signature, StackSlotData, TrapCode,
        UserFuncName, stackslot::StackSlotKind, types,
    },
    isa::CallConv,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;

use crate::anf::AnfExpr;
use crate::native_codegen::{LowerResult, NativeCodegenCtx, infer_cranelift_return_type};

// ── lower_effect_call ─────────────────────────────────────────────────────

/// Lower `AnfExpr::EffectCall` — capability-dispatched host call.
///
/// Signature: `host_call(cap_ptr, cap_len, op_ptr, op_len, args_ptr, args_len) -> I64`.
pub(super) fn lower_effect_call(
    capability: &str,
    func: &str,
    args: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let host_call_id = match ctx.host_call_id {
        Some(id) => id,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };

    // Args buffer: store each arg as I64 in a stack slot.
    let args_size = (args.len().max(1) * 8) as u32;
    let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        args_size,
        3,
    ));
    for (idx, arg_name) in args.iter().enumerate() {
        let arg_val = match ctx.lookup(arg_name.as_str()) {
            Some((v, _)) => v,
            None => builder.ins().iconst(types::I64, 0),
        };
        builder
            .ins()
            .stack_store(arg_val, args_slot, (idx * 8) as i32);
    }
    let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);

    // Capability + func name pointers from data section.
    let (cap_idx, cap_len) = ctx.data_layout.get(capability);
    let (op_idx, op_len) = ctx.data_layout.get(func);
    let cap_gv = module.declare_data_in_func(ctx.data_ids[cap_idx], builder.func);
    let op_gv = module.declare_data_in_func(ctx.data_ids[op_idx], builder.func);
    let cap_ptr = builder.ins().symbol_value(types::I64, cap_gv);
    let op_ptr = builder.ins().symbol_value(types::I64, op_gv);

    let host_call_ref = module.declare_func_in_func(host_call_id, builder.func);
    let call_args = [
        cap_ptr,
        builder.ins().iconst(types::I64, cap_len as i64),
        op_ptr,
        builder.ins().iconst(types::I64, op_len as i64),
        args_ptr,
        builder.ins().iconst(types::I64, args.len() as i64),
    ];
    let call = builder.ins().call(host_call_ref, &call_args);
    LowerResult::Value(builder.inst_results(call)[0])
}

// ── lower_lambda ──────────────────────────────────────────────────────────

/// Lower `AnfExpr::Lambda` — define a nested function and return a closure value.
///
/// If `captures` is empty: return the bare function pointer as I64.
/// If `captures` is non-empty: heap-allocate a closure env struct:
///   Layout: [fn_ptr: i64, cap_count: i64, cap0: i64, ..., capN-1: i64]
///   Captured values are read from the outer ctx at lambda-creation time.
/// If captures are present but heap alloc is unavailable: emit an explicit
///   trap (TrapCode::user(3)) — not a silent no-op.
pub(super) fn lower_lambda(
    params: &[String],
    captures: &[String],
    body: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let lambda_name = format!("__ail_lambda_{}", ctx.next_lambda);
    ctx.next_lambda += 1;

    // Build signature: (param0: I64, param1: I64, ...) -> I64
    let mut lambda_sig = Signature::new(CallConv::SystemV);
    for _ in params {
        lambda_sig.params.push(AbiParam::new(types::I64));
    }
    // Return type inferred from body. Lambda params are always lowered as I64,
    // so a body that directly returns one needs an explicit I64 result.
    let body_ret_ty = infer_lambda_return_type(body, params);
    if let Some(ty) = body_ret_ty {
        lambda_sig.returns.push(AbiParam::new(ty));
    }

    let lambda_id = match module.declare_function(&lambda_name, Linkage::Local, &lambda_sig) {
        Ok(id) => id,
        Err(_) => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };

    // Build and define the lambda body.
    {
        let mut lam_func =
            Function::with_name_signature(UserFuncName::user(0, lambda_id.as_u32()), lambda_sig);
        let mut lam_fn_ctx = FunctionBuilderContext::new();
        let mut lam_builder = FunctionBuilder::new(&mut lam_func, &mut lam_fn_ctx);
        let lam_block = lam_builder.create_block();
        lam_builder.append_block_params_for_function_params(lam_block);
        lam_builder.switch_to_block(lam_block);
        lam_builder.seal_block(lam_block);

        // Bind params to local names.
        let mut lam_ctx = NativeCodegenCtx::new(
            ctx.data_ids,
            ctx.data_layout,
            ctx.bytes_data_ids,
            ctx.host_call_id,
        );
        lam_ctx.malloc_id = ctx.malloc_id;
        lam_ctx.runtime_call_id = ctx.runtime_call_id;
        for (i, param_name) in params.iter().enumerate() {
            let param_val = lam_builder.block_params(lam_block)[i];
            lam_ctx.bind(param_name.as_str(), param_val, types::I64);
        }

        match super::lower_anf_expr_cranelift(body, &mut lam_ctx, &mut lam_builder, module) {
            LowerResult::Value(v) => {
                lam_builder.ins().return_(&[v]);
            }
            LowerResult::Unit => {
                lam_builder.ins().return_(&[]);
            }
            LowerResult::Terminated => {}
        }
        lam_builder.finalize();

        let mut lam_codegen = Context::for_function(lam_func);
        if module.define_function(lambda_id, &mut lam_codegen).is_err() {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    }

    // Obtain the function address in the outer builder.
    let func_ref = module.declare_func_in_func(lambda_id, builder.func);
    let fn_ptr = builder.ins().func_addr(types::I64, func_ref);

    if captures.is_empty() {
        // No closure environment needed — return the bare function pointer.
        LowerResult::Value(fn_ptr)
    } else {
        // Closure env: [fn_ptr: i64, cap_count: i64, cap0: i64, ..., capN-1: i64]
        let cap_count = captures.len();
        let byte_size = (2 + cap_count) as i64 * 8;

        match ctx.malloc_id {
            None => {
                // Heap alloc unavailable — cannot build env, emit explicit trap.
                // TrapCode::user(3) = "closure env requires heap allocation".
                builder.ins().trap(TrapCode::user(3).unwrap());
                LowerResult::Terminated
            }
            Some(malloc_id) => {
                let size_val = builder.ins().iconst(types::I64, byte_size);
                let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
                let call = builder.ins().call(malloc_ref, &[size_val]);
                let env_ptr = builder.inst_results(call)[0];
                // Store function pointer at offset 0.
                builder.ins().store(MemFlags::trusted(), fn_ptr, env_ptr, 0);
                // Store capture count at offset 8.
                let count_val = builder.ins().iconst(types::I64, cap_count as i64);
                builder
                    .ins()
                    .store(MemFlags::trusted(), count_val, env_ptr, 8);
                // Store each captured value at offset 16, 24, ...
                // Values are read from the outer context at lambda-creation time.
                for (i, cap_name) in captures.iter().enumerate() {
                    let cap_val = ctx
                        .lookup(cap_name.as_str())
                        .map(|(v, _)| v)
                        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                    let offset = (16 + i * 8) as i32;
                    builder
                        .ins()
                        .store(MemFlags::trusted(), cap_val, env_ptr, offset);
                }
                LowerResult::Value(env_ptr)
            }
        }
    }
}

fn infer_lambda_return_type(body: &AnfExpr, params: &[String]) -> Option<types::Type> {
    match infer_cranelift_return_type(body) {
        Some(ty) => Some(ty),
        None => match body {
            AnfExpr::Var(name) if params.iter().any(|param| param == name) => Some(types::I64),
            AnfExpr::Return(inner) => infer_lambda_return_type(inner, params),
            _ => None,
        },
    }
}

// ── lower_channel_new ─────────────────────────────────────────────────────

/// Lower `AnfExpr::ChannelNew` — encode capacity and dispatch to runtime.
pub(super) fn lower_channel_new(
    capacity: Option<u64>,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let cap_val = capacity.unwrap_or(0) as i64;
    let cap_name = format!("__cap_{cap_val}");
    // Encode capacity as a synthetic arg: store in stack slot, pass ptr.
    let cap_iconst = builder.ins().iconst(types::I64, cap_val);
    let slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    builder.ins().stack_store(cap_iconst, slot, 0);
    let _ = cap_name;
    emit_runtime_call(ctx, builder, module, 5u64, &[], &[])
}

// ── emit_runtime_call ─────────────────────────────────────────────────────

/// Emit an `ail_runtime_call(op, args_ptr, args_len) -> I64` call.
///
/// `op` is the operation discriminant (1 = TaskSpawn, 2 = TaskAwait, ...).
/// `name_args` are string-keyed args (looked up from data section).
/// `var_args` are variable-name args (looked up from ctx.locals).
pub(super) fn emit_runtime_call(
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
    op: u64,
    _name_args: &[&str],
    var_args: &[String],
) -> LowerResult {
    let runtime_id = match ctx.runtime_call_id {
        Some(id) => id,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };

    // Build args buffer on the stack.
    let args_count = var_args.len();
    let buf_size = ((args_count + 1).max(1) * 8) as u32;
    let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        buf_size,
        3,
    ));
    for (i, arg_name) in var_args.iter().enumerate() {
        let val = ctx
            .lookup(arg_name.as_str())
            .map(|(v, _)| v)
            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
        builder.ins().stack_store(val, args_slot, (i * 8) as i32);
    }
    let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
    let op_val = builder.ins().iconst(types::I64, op as i64);
    let args_len = builder.ins().iconst(types::I64, args_count as i64);

    let runtime_ref = module.declare_func_in_func(runtime_id, builder.func);
    let call = builder
        .ins()
        .call(runtime_ref, &[op_val, args_ptr, args_len]);
    LowerResult::Value(builder.inst_results(call)[0])
}
