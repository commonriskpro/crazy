// ── ail-compiler::native_lower ───────────────────────────────────────────
//
// Cranelift ANF expression lowering for the native backend.
//
// Extracted from `native_codegen.rs` to isolate expression lowering from
// per-function compilation context, return-type inference, and data-layout
// concerns.
//
// # Responsibilities
//
// - ANF expression → Cranelift IR lowering (`lower_anf_expr_cranelift`)
// - Runtime call emission (`emit_runtime_call`, via `runtime` sub-module)
//
// # Non-responsibilities
//
// - Per-function compilation context       → `native_codegen::NativeCodegenCtx`
// - Return-type inference                  → `native_codegen::infer_cranelift_return_type`
// - Object module creation / data layout   → `native::build_object_module`
// - Binding-level function compilation     → `native_binding::lower_binding`
// - Artifact sealing, hash chain           → `native::emit_native_with_profile`
//
// # Sub-modules
//
// - `control` (`native_lower_control.rs`) — arithmetic, comparisons, loops,
//   if/match, short-circuit logic, seq, runtime-check
// - `data` (`native_lower_data.rs`) — record, variant, list, tuple, map, set,
//   cell, index-get, for-each, fold
// - `runtime` (`native_lower_runtime.rs`) — effect-call, lambda/closure env,
//   channel-new, emit_runtime_call dispatcher

use cranelift_codegen::ir::{InstBuilder, TrapCode, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use crate::native_codegen::{LowerResult, NativeCodegenCtx, infer_cranelift_return_type};

#[path = "native_lower_control/mod.rs"]
mod control;
#[path = "native_lower_data.rs"]
mod data;
#[path = "native_lower_runtime.rs"]
mod runtime;

// ── lower_anf_expr_cranelift ──────────────────────────────────────────────

/// Lower one `AnfExpr` into Cranelift IR instructions.
///
/// Returns a [`LowerResult`] indicating whether a value was produced and
/// whether the current basic block has been terminated.
///
/// Supported:
/// - `Literal(Int)` → `iconst(I64, n)` → [`LowerResult::Value`]
/// - `Literal(Bool)` → `iconst(I8, b)` → [`LowerResult::Value`]
/// - `Literal(Float)` → `f64const(f)` → [`LowerResult::Value`]
/// - `Literal(Unit)` → [`LowerResult::Unit`] (no instructions emitted)
/// - `Var(name)` → look up in `ctx.locals`
/// - `Let { name, value, body }` → lower value, bind name, lower body
/// - `Return(inner)` → lower inner, propagate result
/// - `Call { func: "i64.add"|"i64.sub"|"i64.mul", args: [a, b] }` → arithmetic
///
/// All other variants emit `trap(user(1))` → [`LowerResult::Terminated`].
pub(crate) fn lower_anf_expr_cranelift(
    expr: &crate::anf::AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;

    match expr {
        // ── Literals ─────────────────────────────────────────────────────
        AnfExpr::Literal(lit) => match lit {
            LiteralValue::Int(n) => LowerResult::Value(builder.ins().iconst(types::I64, *n)),
            LiteralValue::Bool(b) => LowerResult::Value(builder.ins().iconst(types::I8, *b as i64)),
            LiteralValue::Float(f) => LowerResult::Value(builder.ins().f64const(*f)),
            LiteralValue::Unit => LowerResult::Unit,
            LiteralValue::Text(s) => {
                let (idx, len) = ctx.data_layout.get(s.as_str());
                if idx >= ctx.data_ids.len() {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                let data_id = ctx.data_ids[idx];
                let gv = module.declare_data_in_func(data_id, builder.func);
                let ptr = builder.ins().symbol_value(types::I64, gv);
                // Pack: (len as i64) << 32 | ptr
                let len_val = builder.ins().iconst(types::I64, len as i64);
                let len_shifted = builder.ins().ishl_imm(len_val, 32);
                let packed = builder.ins().bor(len_shifted, ptr);
                LowerResult::Value(packed)
            }
            LiteralValue::Bytes(data) => {
                // Emit a packed (len << 32) | ptr i64 — the same encoding as
                // Text.  The byte buffer is in a separate __ail_bytes_N data
                // object interned by NativeDataLayout::bytes_table.
                let (idx, len) = ctx.data_layout.get_bytes(data.as_slice());
                if idx >= ctx.bytes_data_ids.len() {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                let data_id = ctx.bytes_data_ids[idx];
                let gv = module.declare_data_in_func(data_id, builder.func);
                let ptr = builder.ins().symbol_value(types::I64, gv);
                // Pack: (len as i64) << 32 | ptr
                let len_val = builder.ins().iconst(types::I64, len as i64);
                let len_shifted = builder.ins().ishl_imm(len_val, 32);
                let packed = builder.ins().bor(len_shifted, ptr);
                LowerResult::Value(packed)
            }
        },

        // ── Variable reference ────────────────────────────────────────────
        AnfExpr::Var(name) => match ctx.lookup(name.as_str()) {
            Some((val, _)) => LowerResult::Value(val),
            None => {
                builder.ins().trap(TrapCode::user(1).unwrap());
                LowerResult::Terminated
            }
        },

        // ── Let binding ───────────────────────────────────────────────────
        AnfExpr::Let { name, value, body } => {
            // Detect RecordNew to register layout before recursing into body.
            if let AnfExpr::RecordNew { fields } = value.as_ref() {
                ctx.record_layouts.insert(
                    name.clone(),
                    fields.iter().map(|(f, _)| f.clone()).collect(),
                );
            }
            match lower_anf_expr_cranelift(value, ctx, builder, module) {
                LowerResult::Value(val) => {
                    let ty = infer_cranelift_return_type(value)
                        .unwrap_or(cranelift_codegen::ir::types::I64);
                    ctx.bind(name.as_str(), val, ty);
                    lower_anf_expr_cranelift(body, ctx, builder, module)
                }
                LowerResult::Unit => lower_anf_expr_cranelift(body, ctx, builder, module),
                LowerResult::Terminated => LowerResult::Terminated,
            }
        }

        // ── Return ────────────────────────────────────────────────────────
        AnfExpr::Return(inner) => lower_anf_expr_cranelift(inner, ctx, builder, module),

        // ── Call (arithmetic / comparison / unary) ────────────────────────
        AnfExpr::Call { func, args } => control::lower_call(func.as_str(), args, ctx, builder),

        // ── Loop / Break / Continue / WhileLoop ───────────────────────────
        AnfExpr::Loop { body } => control::lower_loop(body, ctx, builder, module),
        AnfExpr::Break { value } => control::lower_break(value, ctx, builder, module),
        AnfExpr::Continue => control::lower_continue(ctx, builder),
        AnfExpr::WhileLoop { cond, body } => {
            control::lower_while_loop(cond.as_str(), body, ctx, builder, module)
        }

        // ── If ────────────────────────────────────────────────────────────
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => control::lower_if(
            cond.as_str(),
            then_branch,
            else_branch,
            ctx,
            builder,
            module,
        ),

        // ── Match ─────────────────────────────────────────────────────────
        AnfExpr::Match { scrutinee, arms } => {
            control::lower_match(scrutinee.as_str(), arms, ctx, builder, module)
        }

        // ── Short-circuit logic ───────────────────────────────────────────
        AnfExpr::ShortCircuitAnd { left, right } => {
            control::lower_short_circuit_and(left.as_str(), right, ctx, builder, module)
        }
        AnfExpr::ShortCircuitOr { left, right } => {
            control::lower_short_circuit_or(left.as_str(), right, ctx, builder, module)
        }

        // ── Seq / RuntimeCheck ────────────────────────────────────────────
        AnfExpr::Seq(exprs) => control::lower_seq(exprs, ctx, builder, module),
        AnfExpr::RuntimeCheck { cond, .. } => {
            control::lower_runtime_check(cond.as_str(), ctx, builder)
        }

        // ── Record / Field ────────────────────────────────────────────────
        AnfExpr::RecordNew { fields } => data::lower_record_new(fields, ctx, builder, module),
        AnfExpr::FieldGet { record, field } => {
            data::lower_field_get(record.as_str(), field.as_str(), ctx, builder)
        }
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => data::lower_field_update(record.as_str(), field.as_str(), value, ctx, builder, module),

        // ── Variant / List / Tuple ────────────────────────────────────────
        AnfExpr::VariantNew { tag, payload } => {
            data::lower_variant_new(tag.as_str(), payload.as_deref(), ctx, builder, module)
        }
        AnfExpr::ListNew(elems) => data::lower_list_new(elems, ctx, builder, module),
        AnfExpr::TupleNew(elems) => data::lower_tuple_new(elems, ctx, builder, module),

        // ── Collections ───────────────────────────────────────────────────
        AnfExpr::IndexGet { collection, index } => {
            data::lower_index_get(collection.as_str(), index.as_str(), ctx, builder)
        }
        AnfExpr::MapNew { entries } => data::lower_map_new(entries, ctx, builder, module),
        AnfExpr::SetNew { elements } => data::lower_set_new(elements, ctx, builder, module),
        AnfExpr::ForEach {
            binding,
            collection,
            body,
        } => data::lower_for_each(
            binding.as_str(),
            collection.as_str(),
            body,
            ctx,
            builder,
            module,
        ),
        AnfExpr::Fold { init, list, func } => data::lower_fold(
            init.as_str(),
            list.as_str(),
            func.as_str(),
            ctx,
            builder,
            module,
        ),

        // ── Cells ─────────────────────────────────────────────────────────
        AnfExpr::CellNew { init } => data::lower_cell_new(init.as_str(), ctx, builder, module),
        AnfExpr::CellGet { cell } => data::lower_cell_get(cell.as_str(), ctx, builder),
        AnfExpr::CellSet { cell, value } => {
            data::lower_cell_set(cell.as_str(), value.as_str(), ctx, builder)
        }

        // ── Effect call ───────────────────────────────────────────────────
        AnfExpr::EffectCall {
            capability,
            func,
            args,
        } => runtime::lower_effect_call(
            capability.as_str(),
            func.as_str(),
            args,
            ctx,
            builder,
            module,
        ),

        // ── Static annotations ────────────────────────────────────────────
        // Assume: no runtime effect — a proof assumption is purely static.
        AnfExpr::Assume { .. } => LowerResult::Unit,
        // Abort: always traps — represents an impossible branch (Never type).
        AnfExpr::Abort { .. } => {
            builder.ins().trap(TrapCode::user(2).unwrap());
            LowerResult::Terminated
        }

        // ── Lambda ────────────────────────────────────────────────────────
        AnfExpr::Lambda {
            params,
            captures,
            body,
        } => runtime::lower_lambda(params, captures, body, ctx, builder, module),

        // ── Concurrency and resource primitives ───────────────────────────
        // All are routed through the imported `ail_runtime_call` function.
        AnfExpr::TaskSpawn { func, args } => {
            runtime::emit_runtime_call(ctx, builder, module, 1u64, &[func.as_str()], args)
        }
        AnfExpr::TaskAwait { task } => {
            runtime::emit_runtime_call(ctx, builder, module, 2u64, &[], std::slice::from_ref(task))
        }
        AnfExpr::TaskCancel { task } => {
            runtime::emit_runtime_call(ctx, builder, module, 3u64, &[], std::slice::from_ref(task))
        }
        AnfExpr::TaskGroup { body } => {
            // TaskGroup: lower body for side effects, then emit runtime notification.
            let _ = lower_anf_expr_cranelift(body, ctx, builder, module);
            runtime::emit_runtime_call(ctx, builder, module, 4u64, &[], &[])
        }
        AnfExpr::ChannelNew { capacity } => {
            runtime::lower_channel_new(*capacity, ctx, builder, module)
        }
        AnfExpr::ChannelSend { channel, value } => runtime::emit_runtime_call(
            ctx,
            builder,
            module,
            6u64,
            &[],
            &[channel.clone(), value.clone()],
        ),
        AnfExpr::ChannelReceive { channel } => runtime::emit_runtime_call(
            ctx,
            builder,
            module,
            7u64,
            &[],
            std::slice::from_ref(channel),
        ),
        AnfExpr::Select { .. } => runtime::emit_runtime_call(ctx, builder, module, 8u64, &[], &[]),
        AnfExpr::Timeout { duration, body } => {
            let _ = lower_anf_expr_cranelift(body, ctx, builder, module);
            runtime::emit_runtime_call(
                ctx,
                builder,
                module,
                9u64,
                &[],
                std::slice::from_ref(duration),
            )
        }
        AnfExpr::Dispatch {
            handler,
            method,
            args,
        } => {
            let mut all_args: Vec<String> = vec![handler.clone(), method.clone()];
            all_args.extend(args.iter().cloned());
            runtime::emit_runtime_call(ctx, builder, module, 10u64, &[], &all_args)
        }
        AnfExpr::ResourceAcquire { resource, args } => {
            let mut all_args: Vec<String> = vec![resource.clone()];
            all_args.extend(args.iter().cloned());
            runtime::emit_runtime_call(ctx, builder, module, 11u64, &[], &all_args)
        }
        AnfExpr::ResourceRelease { handle } => runtime::emit_runtime_call(
            ctx,
            builder,
            module,
            12u64,
            &[],
            std::slice::from_ref(handle),
        ),

        _ => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}
