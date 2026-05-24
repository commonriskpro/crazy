// ── ail-compiler::native_binding ─────────────────────────────────────────
//
// Per-binding compilation environment for the Cranelift native backend.
//
// Extracted from `native.rs` to separate binding-level function compilation
// from module-building, data-layout, and artifact-sealing concerns.
//
// # Responsibilities
//
// - `LowerBindingEnv`: borrowed references needed to compile one binding
// - `lower_binding`: compile one `AnfBinding` into a Cranelift function

use cranelift_codegen::{
    Context,
    ir::{AbiParam, Function, InstBuilder, Signature, UserFuncName},
    isa::CallConv,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;

use crate::error::CompileError;
use crate::native_codegen::{LowerResult, NativeCodegenCtx, infer_cranelift_return_type};
use crate::native_lower::lower_anf_expr_cranelift;
use crate::native_types::NativeDataLayout;

// ── LowerBindingEnv ───────────────────────────────────────────────────────

pub(crate) struct LowerBindingEnv<'a> {
    pub(crate) data_ids: &'a [DataId],
    pub(crate) data_layout: &'a NativeDataLayout,
    pub(crate) host_call_id: Option<FuncId>,
    pub(crate) malloc_id: Option<FuncId>,
    pub(crate) runtime_call_id: Option<FuncId>,
}

// ── lower_binding ─────────────────────────────────────────────────────────

/// Lower one `AnfBinding` into a compiled Cranelift function inside `module`.
///
/// Returns the compiled code size in bytes so the caller can advance the
/// cumulative offset accumulator.
///
/// The function signature and body are inferred from `expr`:
/// - `Literal(Int)` / arithmetic → `() -> i64` with computed return value.
/// - `Literal(Unit)` → `() -> ()` with empty return.
/// - `Placeholder` / unsupported → `() -> ()` with `trap` body.
pub(crate) fn lower_binding(
    module: &mut ObjectModule,
    name: &str,
    expr: &crate::anf::AnfExpr,
    env: LowerBindingEnv<'_>,
) -> Result<u64, CompileError> {
    // Infer return type from the expression before building the function.
    let ret_ty = infer_cranelift_return_type(expr);

    let mut sig = Signature::new(CallConv::SystemV);
    if let Some(ty) = ret_ty {
        sig.returns.push(AbiParam::new(ty));
    }

    let func_id = module
        .declare_function(name, Linkage::Export, &sig)
        .map_err(|e| CompileError::NativeEncodingError(format!("declare_function({name}): {e}")))?;

    let mut func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);

    {
        let mut fn_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut fn_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut codegen_ctx =
            NativeCodegenCtx::new(env.data_ids, env.data_layout, env.host_call_id);
        codegen_ctx.malloc_id = env.malloc_id;
        codegen_ctx.runtime_call_id = env.runtime_call_id;
        match lower_anf_expr_cranelift(expr, &mut codegen_ctx, &mut builder, module) {
            LowerResult::Value(val) => {
                builder.ins().return_(&[val]);
            }
            LowerResult::Unit => {
                builder.ins().return_(&[]);
            }
            LowerResult::Terminated => {
                // Block already has a terminator — finalize only.
            }
        }

        builder.finalize();
    }

    let mut ctx = Context::for_function(func);
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| CompileError::NativeEncodingError(format!("define_function({name}): {e}")))?;

    let code_size = ctx
        .compiled_code()
        .ok_or_else(|| {
            CompileError::NativeEncodingError(format!("compiled_code missing for {name}"))
        })?
        .code_info()
        .total_size;

    Ok(u64::from(code_size))
}
