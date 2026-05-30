use super::*;

pub(super) fn emit_fold_expr<'a>(
    init: &str,
    list: &str,
    func: &String,
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let Some(fold_type_idx) = ctx.fold_reducer_type_idx else {
        // Pre-flight gate should have inserted the type; trap defensively.
        insns.push(Instruction::Unreachable);
        return None;
    };

    let Some((list_local, _)) = ctx.lookup(list) else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    // Allocate locals: count, loop index, accumulator, element.
    let count_idx = ctx.bind("__fold_count", ValType::I64);
    let i_idx = ctx.bind("__fold_i", ValType::I64);
    let acc_idx = ctx.bind("__fold_acc", ValType::I64);
    let elem_idx = ctx.bind("__fold_elem", ValType::I64);

    // Load element count from list header (offset 0).
    insns.push(Instruction::LocalGet(list_local));
    load_i64_at(0, insns);
    insns.push(Instruction::LocalSet(count_idx));

    // Initialise accumulator from `init`.
    emit_local_as_i64(ctx, init, insns);
    insns.push(Instruction::LocalSet(acc_idx));

    // Initialise loop counter to 0.
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(i_idx));

    // block (result I64) — break target that yields the final accumulator.
    insns.push(Instruction::Block(BlockType::Result(ValType::I64)));
    ctx.labels.push(LabelKind::LoopBreak);
    insns.push(Instruction::Loop(BlockType::Empty));
    ctx.labels.push(LabelKind::LoopContinue);

    // Exit check: if i >= count, break with the current accumulator.
    insns.push(Instruction::LocalGet(i_idx));
    insns.push(Instruction::LocalGet(count_idx));
    insns.push(Instruction::I64GeU);
    insns.push(Instruction::If(BlockType::Empty));
    ctx.labels.push(LabelKind::Other);
    insns.push(Instruction::LocalGet(acc_idx));
    // Break to the enclosing block (carries acc as the block result).
    // Depth from inside the If: 0 = If, 1 = Loop, 2 = Block.
    let break_depth = ctx.branch_depth(LabelKind::LoopBreak).unwrap_or(2);
    insns.push(Instruction::Br(break_depth));
    ctx.labels.pop(); // Other (If body)
    insns.push(Instruction::End); // end if

    // Load element: list_ptr + 8 + i * 8.
    insns.push(Instruction::LocalGet(list_local));
    insns.push(Instruction::LocalGet(i_idx));
    insns.push(Instruction::I64Const(8));
    insns.push(Instruction::I64Mul);
    insns.push(Instruction::I64Const(8));
    insns.push(Instruction::I64Add);
    insns.push(Instruction::I32WrapI64);
    insns.push(Instruction::I32Add);
    load_i64_at(0, insns);
    insns.push(Instruction::LocalSet(elem_idx));

    // Push reducer arguments: acc (i64), elem (i64).
    insns.push(Instruction::LocalGet(acc_idx));
    insns.push(Instruction::LocalGet(elem_idx));

    // Push callee table index (i32).
    if let Some(&func_idx) = functions.get(func.as_str()) {
        // Top-level function: table index = absolute func idx − offset.
        let table_idx = func_idx.saturating_sub(ctx.function_offset);
        insns.push(Instruction::I32Const(table_idx as i32));
    } else if let Some((local_idx, local_ty)) = ctx.lookup(func) {
        match local_ty {
            ValType::I32 => {
                // Closure env pointer (Wave 16A PR3): load fn_idx from
                // offset 0 of the env, wrap to i32 for call_indirect.
                // Also push env_ptr (as i64) as the first argument to the
                // closure-reducer `(env_ptr: i64, acc: i64, elem: i64)`.
                //
                // The argument order before call_indirect must be:
                //   [acc: i64, elem: i64] already on stack
                // But we need [env_ptr: i64, acc: i64, elem: i64, callee: i32].
                // Since acc and elem are already pushed above, and call_indirect
                // is a stack-based dispatch, we need to reorganise:
                //
                // Strategy: DON'T push acc/elem above; push them after env_ptr.
                // But acc and elem were already pushed above — we need to move
                // env_ptr to before them.
                //
                // We use the closure_reducer_type path differently: the acc and
                // elem are on the stack already (pushed in the block before
                // this else-if).  We insert env_ptr before them using a local.
                //
                // Actually, the `call_indirect` with closure-reducer type
                // expects [env_ptr: i64, acc: i64, elem: i64] in that order.
                // Since acc and elem are already on the stack (pushed above),
                // and we can't easily insert before them, we DON'T use the
                // standard call_indirect tail here.  Instead we take over the
                // full dispatch below and break out of the normal post-branch.
                //
                // NOTE: the acc/elem pushes above are WASTED when the I32 path
                // is taken — they're dropped here so we can re-push in the
                // right order for the closure-reducer ABI.
                //
                // This is safe because Fold only cares about the final result.
                if let Some(closure_type_idx) = ctx.closure_reducer_type_idx {
                    // Drop acc and elem (already on stack from the push above).
                    insns.push(Instruction::Drop); // elem
                    insns.push(Instruction::Drop); // acc

                    // Push env_ptr (as i64) — first argument.
                    insns.push(Instruction::LocalGet(local_idx));
                    insns.push(Instruction::I64ExtendI32U);

                    // Re-push acc and elem.
                    insns.push(Instruction::LocalGet(acc_idx));
                    insns.push(Instruction::LocalGet(elem_idx));

                    // Load fn_idx (i64) from env[0], wrap to i32 for table.
                    insns.push(Instruction::LocalGet(local_idx));
                    load_i64_at(0, insns);
                    insns.push(Instruction::I32WrapI64);

                    // call_indirect with closure-reducer type.
                    insns.push(Instruction::CallIndirect {
                        type_index: closure_type_idx,
                        table_index: 0,
                    });
                    insns.push(Instruction::LocalSet(acc_idx));

                    // Increment loop counter.
                    insns.push(Instruction::LocalGet(i_idx));
                    insns.push(Instruction::I64Const(1));
                    insns.push(Instruction::I64Add);
                    insns.push(Instruction::LocalSet(i_idx));

                    // Branch back to loop header.
                    insns.push(Instruction::Br(0));

                    ctx.labels.pop(); // LoopContinue
                    insns.push(Instruction::End); // end loop
                    insns.push(Instruction::Unreachable);
                    ctx.labels.pop(); // LoopBreak
                    insns.push(Instruction::End); // end block

                    return Some(ValType::I64);
                }
                // No closure-reducer type available — fall through to
                // Unreachable (shouldn't happen with needs_fold, but safe).
                insns.push(Instruction::Drop); // elem
                insns.push(Instruction::Drop); // acc
                insns.push(Instruction::Unreachable);
            }
            ValType::I64 => {
                // Direct table index packed as i64: push local, wrap to i32.
                insns.push(Instruction::LocalGet(local_idx));
                insns.push(Instruction::I32WrapI64);
            }
            _ => {
                // Unexpected local type (e.g. F64) — drop acc and elem
                // from the stack, then trap via Unreachable.  Dead code
                // after Unreachable is accepted by the WASM validator.
                insns.push(Instruction::Drop); // elem
                insns.push(Instruction::Drop); // acc
                insns.push(Instruction::Unreachable);
            }
        }
    } else {
        // Unresolved function reference — trap at runtime.
        insns.push(Instruction::Unreachable);
    }

    // call_indirect: pops [acc: i64, elem: i64, callee: i32] → i64.
    // (Only reached for the I64 and top-level-function paths above;
    // the I32/closure path returns early after its own call_indirect.)
    insns.push(Instruction::CallIndirect {
        type_index: fold_type_idx,
        table_index: 0,
    });
    insns.push(Instruction::LocalSet(acc_idx));

    // Increment loop counter.
    insns.push(Instruction::LocalGet(i_idx));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::I64Add);
    insns.push(Instruction::LocalSet(i_idx));

    // Branch back to loop header.
    insns.push(Instruction::Br(0));

    ctx.labels.pop(); // LoopContinue
    insns.push(Instruction::End); // end loop
    // Unreachable: the loop always exits via Br(break_depth) above.
    insns.push(Instruction::Unreachable);
    ctx.labels.pop(); // LoopBreak
    insns.push(Instruction::End); // end block — I64 result from Br

    Some(ValType::I64)
}
