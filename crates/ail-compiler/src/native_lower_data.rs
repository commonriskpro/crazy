// ── ail-compiler::native_lower::data ─────────────────────────────────────
//
// Data-structure ANF expression lowering helpers.
//
// Covers: RecordNew, FieldGet, FieldUpdate, VariantNew, ListNew, TupleNew,
// IndexGet, MapNew, SetNew, ForEach, Fold, CellNew, CellGet, CellSet.
//
// Memory layout contracts:
//   Record/Tuple  → [field0: i64, field1: i64, ...]  (no length header)
//   Variant       → [tag: i64, payload: i64]          (16 bytes fixed)
//   List/Set      → [count: i64, elem0: i64, ...]
//   Map           → [count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...]
//   Cell          → [value: i64]                      (8 bytes)

use cranelift_codegen::ir::{
    AbiParam, InstBuilder, MemFlags, Signature, StackSlotData, TrapCode, condcodes::IntCC,
    stackslot::StackSlotKind, types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use crate::anf::AnfExpr;
use crate::native_codegen::{LowerResult, NativeCodegenCtx};

// ── lower_record_new ──────────────────────────────────────────────────────

/// Heap-allocate a record: [field0: i64, field1: i64, ...].
///
/// Falls back to stack allocation when malloc is unavailable (Phase 17 legacy);
/// the stack address is dangling after the function returns in that case.
pub(super) fn lower_record_new(
    fields: &[(String, AnfExpr)],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let byte_size = (fields.len().max(1) * 8) as i64;
    match ctx.malloc_id {
        None => {
            // Fallback to stack allocation (dangling pointer — Phase 17 legacy).
            let size = (fields.len().max(1) * 8) as u32;
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                size,
                3,
            ));
            for (idx, (_, field_expr)) in fields.iter().enumerate() {
                let val = match super::lower_anf_expr_cranelift(field_expr, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder.ins().stack_store(val, slot, (idx * 8) as i32);
            }
            LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, byte_size);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            for (idx, (_, field_expr)) in fields.iter().enumerate() {
                let val = match super::lower_anf_expr_cranelift(field_expr, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder
                    .ins()
                    .store(MemFlags::trusted(), val, ptr, (idx * 8) as i32);
            }
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_field_get ───────────────────────────────────────────────────────

pub(super) fn lower_field_get(
    record: &str,
    field: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let ptr = match ctx.lookup(record) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let offset = ctx.field_offset(record, field);
    let val = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), ptr, offset);
    LowerResult::Value(val)
}

// ── lower_field_update ────────────────────────────────────────────────────

pub(super) fn lower_field_update(
    record: &str,
    field: &str,
    value: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let ptr = match ctx.lookup(record) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let val = match super::lower_anf_expr_cranelift(value, ctx, builder, module) {
        LowerResult::Value(v) => v,
        _ => builder.ins().iconst(types::I64, 0),
    };
    let offset = ctx.field_offset(record, field);
    builder.ins().store(MemFlags::trusted(), val, ptr, offset);
    LowerResult::Value(ptr)
}

// ── lower_variant_new ─────────────────────────────────────────────────────

/// Heap-allocate a variant: [tag: i64, payload: i64] (16 bytes fixed).
pub(super) fn lower_variant_new(
    tag: &str,
    payload: Option<&AnfExpr>,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let tag_id = ctx.assign_tag(tag) as i64;
    match ctx.malloc_id {
        None => {
            // Fallback stack allocation.
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                16,
                3,
            ));
            let tag_val = builder.ins().iconst(types::I64, tag_id);
            builder.ins().stack_store(tag_val, slot, 0);
            if let Some(payload_expr) = payload {
                let pv = match super::lower_anf_expr_cranelift(payload_expr, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder.ins().stack_store(pv, slot, 8);
            }
            LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, 16);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            let tag_val = builder.ins().iconst(types::I64, tag_id);
            builder.ins().store(MemFlags::trusted(), tag_val, ptr, 0);
            if let Some(payload_expr) = payload {
                let pv = match super::lower_anf_expr_cranelift(payload_expr, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder.ins().store(MemFlags::trusted(), pv, ptr, 8);
            }
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_list_new ────────────────────────────────────────────────────────

/// Heap-allocate a list: [len: i64, elem0: i64, ...].
pub(super) fn lower_list_new(
    elems: &[AnfExpr],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let byte_size = ((1 + elems.len()) * 8) as i64;
    match ctx.malloc_id {
        None => {
            // Fallback stack allocation.
            let size = ((1 + elems.len()) * 8) as u32;
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                size,
                3,
            ));
            let len_val = builder.ins().iconst(types::I64, elems.len() as i64);
            builder.ins().stack_store(len_val, slot, 0);
            for (i, elem) in elems.iter().enumerate() {
                let val = match super::lower_anf_expr_cranelift(elem, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder.ins().stack_store(val, slot, (8 + i * 8) as i32);
            }
            LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, byte_size);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            let len_val = builder.ins().iconst(types::I64, elems.len() as i64);
            builder.ins().store(MemFlags::trusted(), len_val, ptr, 0);
            for (i, elem) in elems.iter().enumerate() {
                let val = match super::lower_anf_expr_cranelift(elem, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder
                    .ins()
                    .store(MemFlags::trusted(), val, ptr, (8 + i * 8) as i32);
            }
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_tuple_new ───────────────────────────────────────────────────────

/// Heap-allocate a tuple: [elem0: i64, elem1: i64, ...] (no length header).
pub(super) fn lower_tuple_new(
    elems: &[AnfExpr],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let byte_size = (elems.len().max(1) * 8) as i64;
    match ctx.malloc_id {
        None => {
            // Fallback stack allocation.
            let size = (elems.len().max(1) * 8) as u32;
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                size,
                3,
            ));
            for (i, elem) in elems.iter().enumerate() {
                let val = match super::lower_anf_expr_cranelift(elem, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder.ins().stack_store(val, slot, (i * 8) as i32);
            }
            LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, byte_size);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            for (i, elem) in elems.iter().enumerate() {
                let val = match super::lower_anf_expr_cranelift(elem, ctx, builder, module) {
                    LowerResult::Value(v) => v,
                    _ => builder.ins().iconst(types::I64, 0),
                };
                builder
                    .ins()
                    .store(MemFlags::trusted(), val, ptr, (i * 8) as i32);
            }
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_index_get ───────────────────────────────────────────────────────

/// Load element at `index` from a length-prefixed list/map/set.
///
/// Collection layout: [len: i64, elem0: i64, elem1: i64, ...]
/// Element offset = 8 + index * 8.
pub(super) fn lower_index_get(
    collection: &str,
    index: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let col_ptr = match ctx.lookup(collection) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let idx_val = match ctx.lookup(index) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    // offset = 8 + idx * 8
    let eight = builder.ins().iconst(types::I64, 8);
    let idx_scaled = builder.ins().imul(idx_val, eight);
    let offset = builder.ins().iadd(idx_scaled, eight);
    let addr = builder.ins().iadd(col_ptr, offset);
    let val = builder.ins().load(types::I64, MemFlags::trusted(), addr, 0);
    LowerResult::Value(val)
}

// ── lower_map_new ─────────────────────────────────────────────────────────

/// Heap-allocate a map: [count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...].
pub(super) fn lower_map_new(
    entries: &[(String, String)],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let byte_size = (1 + entries.len() * 2) as i64 * 8;
    match ctx.malloc_id {
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, byte_size);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            // Store count
            let count = builder.ins().iconst(types::I64, entries.len() as i64);
            builder.ins().store(MemFlags::trusted(), count, ptr, 0);
            // Store key-value pairs
            for (i, (k_name, v_name)) in entries.iter().enumerate() {
                let base = (1 + i * 2) as i32 * 8;
                let k_val = ctx
                    .lookup(k_name.as_str())
                    .map(|(v, _)| v)
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                let v_val = ctx
                    .lookup(v_name.as_str())
                    .map(|(v, _)| v)
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                builder.ins().store(MemFlags::trusted(), k_val, ptr, base);
                builder
                    .ins()
                    .store(MemFlags::trusted(), v_val, ptr, base + 8);
            }
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_set_new ─────────────────────────────────────────────────────────

/// Heap-allocate a set: [count: i64, elem0: i64, elem1: i64, ...].
pub(super) fn lower_set_new(
    elements: &[String],
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let byte_size = (1 + elements.len()) as i64 * 8;
    match ctx.malloc_id {
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, byte_size);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            let count = builder.ins().iconst(types::I64, elements.len() as i64);
            builder.ins().store(MemFlags::trusted(), count, ptr, 0);
            for (i, elem_name) in elements.iter().enumerate() {
                let offset = (1 + i) as i32 * 8;
                let val = ctx
                    .lookup(elem_name.as_str())
                    .map(|(v, _)| v)
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                builder.ins().store(MemFlags::trusted(), val, ptr, offset);
            }
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_for_each ────────────────────────────────────────────────────────

/// Loop over a length-prefixed list collection.
///
/// Layout: [len: i64, elem0: i64, ...]
/// Generates a counter from 0 to len-1, binding each element into body.
pub(super) fn lower_for_each(
    binding: &str,
    collection: &str,
    body: &AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let col_ptr = match ctx.lookup(collection) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    // Load length
    let len_val = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), col_ptr, 0);

    // Allocate a stack slot for the loop counter.
    let counter_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().stack_store(zero, counter_slot, 0);

    let break_block = builder.create_block();
    let loop_block = builder.create_block();
    let body_block = builder.create_block();

    builder.ins().jump(loop_block, &[]);
    builder.switch_to_block(loop_block);
    // Loop header: check counter < len
    let counter = builder.ins().stack_load(types::I64, counter_slot, 0);
    let cond = builder.ins().icmp(IntCC::SignedLessThan, counter, len_val);
    builder.ins().brif(cond, body_block, &[], break_block, &[]);

    builder.switch_to_block(body_block);
    builder.seal_block(body_block);
    // Load element: ptr + 8 + counter * 8
    let eight = builder.ins().iconst(types::I64, 8);
    let offset = builder.ins().imul(counter, eight);
    let inner_sum = builder.ins().iadd(offset, eight);
    let elem_addr = builder.ins().iadd(col_ptr, inner_sum);
    let elem_val = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), elem_addr, 0);
    // Bind the element to the loop variable.
    ctx.bind(binding, elem_val, types::I64);
    // Lower body (result discarded — ForEach is for side effects).
    let body_result = super::lower_anf_expr_cranelift(body, ctx, builder, module);
    if !matches!(body_result, LowerResult::Terminated) {
        // Increment counter and jump back.
        let one = builder.ins().iconst(types::I64, 1);
        let next = builder.ins().iadd(counter, one);
        builder.ins().stack_store(next, counter_slot, 0);
        builder.ins().jump(loop_block, &[]);
    }
    builder.seal_block(loop_block);

    builder.switch_to_block(break_block);
    builder.seal_block(break_block);
    LowerResult::Unit
}

// ── lower_fold ────────────────────────────────────────────────────────────

/// Left fold over a length-prefixed list using an I64 function pointer.
///
/// Signature expected for func: `(acc: I64, elem: I64) -> I64`.
/// CFG: entry → loop_hdr ←→ body_blk → exit_blk.
pub(super) fn lower_fold(
    init: &str,
    list: &str,
    func: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    let _ = module; // module not used in fold — retained for signature consistency

    let init_val = match ctx.lookup(init) {
        Some((v, _)) => v,
        None => builder.ins().iconst(types::I64, 0),
    };
    let list_ptr = match ctx.lookup(list) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let func_ptr = match ctx.lookup(func) {
        Some((v, _)) => v,
        // No func pointer — return init unchanged (degenerate fold).
        None => return LowerResult::Value(init_val),
    };

    // Load list length once in the entry block.
    let len_val = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), list_ptr, 0);

    // Use stack slots for mutable accumulator and counter.
    let acc_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let ctr_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    builder.ins().stack_store(init_val, acc_slot, 0);
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().stack_store(zero, ctr_slot, 0);

    let loop_hdr = builder.create_block();
    let body_blk = builder.create_block();
    let exit_blk = builder.create_block();

    // Entry → loop_hdr.
    builder.ins().jump(loop_hdr, &[]);

    // ── loop_hdr (NOT sealed yet — back-edge from body_blk pending) ──
    builder.switch_to_block(loop_hdr);
    let ctr = builder.ins().stack_load(types::I64, ctr_slot, 0);
    let cond = builder.ins().icmp(IntCC::SignedLessThan, ctr, len_val);
    builder.ins().brif(cond, body_blk, &[], exit_blk, &[]);
    // Seal body_blk: its only predecessor is loop_hdr (brif-true edge).
    builder.seal_block(body_blk);
    // Seal exit_blk: its only predecessor is loop_hdr (brif-false edge).
    builder.seal_block(exit_blk);

    // ── body_blk ──────────────────────────────────────────────────
    builder.switch_to_block(body_blk);
    let acc = builder.ins().stack_load(types::I64, acc_slot, 0);
    let ctr2 = builder.ins().stack_load(types::I64, ctr_slot, 0);
    let eight = builder.ins().iconst(types::I64, 8);
    let elem_off = builder.ins().imul(ctr2, eight);
    let elem_base = builder.ins().iadd(elem_off, eight);
    let elem_addr = builder.ins().iadd(list_ptr, elem_base);
    let elem = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), elem_addr, 0);
    // Indirect call: func_ptr(acc, elem) → I64.
    let fold_sig = {
        let mut sig = Signature::new(CallConv::SystemV);
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        builder.import_signature(sig)
    };
    let indirect_call = builder
        .ins()
        .call_indirect(fold_sig, func_ptr, &[acc, elem]);
    let new_acc = builder.inst_results(indirect_call)[0];
    builder.ins().stack_store(new_acc, acc_slot, 0);
    let one = builder.ins().iconst(types::I64, 1);
    let next_ctr = builder.ins().iadd(ctr2, one);
    builder.ins().stack_store(next_ctr, ctr_slot, 0);
    builder.ins().jump(loop_hdr, &[]);
    // Now seal loop_hdr: all predecessors are known (entry + body_blk).
    builder.seal_block(loop_hdr);

    // ── exit_blk ──────────────────────────────────────────────────
    builder.switch_to_block(exit_blk);
    let acc_final = builder.ins().stack_load(types::I64, acc_slot, 0);
    LowerResult::Value(acc_final)
}

// ── lower_cell_new ────────────────────────────────────────────────────────

/// Heap-allocate an 8-byte mutable cell, store init value, return ptr.
pub(super) fn lower_cell_new(
    init: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    match ctx.malloc_id {
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
        Some(malloc_id) => {
            let size_val = builder.ins().iconst(types::I64, 8);
            let malloc_ref = module.declare_func_in_func(malloc_id, builder.func);
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];
            let init_val = ctx
                .lookup(init)
                .map(|(v, _)| v)
                .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
            builder.ins().store(MemFlags::trusted(), init_val, ptr, 0);
            LowerResult::Value(ptr)
        }
    }
}

// ── lower_cell_get ────────────────────────────────────────────────────────

/// Load the I64 value stored in a heap-allocated cell.
pub(super) fn lower_cell_get(
    cell: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let ptr = match ctx.lookup(cell) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let val = builder.ins().load(types::I64, MemFlags::trusted(), ptr, 0);
    LowerResult::Value(val)
}

// ── lower_cell_set ────────────────────────────────────────────────────────

/// Store a new I64 value into a heap-allocated cell.
pub(super) fn lower_cell_set(
    cell: &str,
    value: &str,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
) -> LowerResult {
    let ptr = match ctx.lookup(cell) {
        Some((v, _)) => v,
        None => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            return LowerResult::Terminated;
        }
    };
    let val = ctx
        .lookup(value)
        .map(|(v, _)| v)
        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
    builder.ins().store(MemFlags::trusted(), val, ptr, 0);
    LowerResult::Unit
}
