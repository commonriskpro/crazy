use super::*;

/// Load a local variable as an I64, zero-extending I32 values.
/// Emits `Unreachable` if the name is not in scope, matching `emit_local_get`.
pub(super) fn emit_local_as_i64<'a>(
    ctx: &WasmCodegenCtx<'a>,
    name: &str,
    insns: &mut Vec<Instruction<'a>>,
) {
    if let Some((idx, ty)) = ctx.lookup(name) {
        insns.push(Instruction::LocalGet(idx));
        if ty == ValType::I32 {
            insns.push(Instruction::I64ExtendI32U);
        }
    } else {
        insns.push(Instruction::Unreachable);
    }
}

pub(super) fn emit_condition_get<'a>(
    ctx: &WasmCodegenCtx<'a>,
    name: &str,
    insns: &mut Vec<Instruction<'a>>,
) {
    if let Some((idx, ty)) = ctx.lookup(name) {
        insns.push(Instruction::LocalGet(idx));
        if ty == ValType::I64 {
            insns.push(Instruction::I64Const(0));
            insns.push(Instruction::I64Ne);
        }
    } else {
        insns.push(Instruction::I32Const(0));
    }
}

pub(super) fn parse_i64_pattern(pattern: &str) -> Option<i64> {
    pattern.trim().parse::<i64>().ok()
}

pub(super) fn parse_bool_pattern(pattern: &str) -> Option<bool> {
    match pattern.trim() {
        "true" | "True" => Some(true),
        "false" | "False" => Some(false),
        _ => None,
    }
}

pub(super) fn emit_match_arms<'a>(
    scrutinee: &str,
    scrutinee_ty: ValType,
    arms: &'a [crate::anf::AnfMatchArm],
    result_ty: Option<ValType>,
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let Some((first, rest)) = arms.split_first() else {
        insns.push(Instruction::Unreachable);
        return result_ty;
    };

    if first.pattern.trim() == "_" {
        return emit_branch_expr(&first.body, result_ty, ctx, functions, insns);
    }

    // ── Variant constructor patterns (I32 scrutinee = pointer) ───────────
    // Must be checked before the bool/int fallback so that tag-only patterns
    // like `"None"` are not misidentified as unhandled patterns.
    if scrutinee_ty == ValType::I32
        && let Some((tag, binding)) = parse_constructor_pattern(&first.pattern)
    {
        // Emit: load tag field (i32 at offset 0) and compare.
        emit_local_get(ctx, scrutinee, insns);
        insns.push(Instruction::I32Load(wasm_encoder::MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        let tag_id = ctx.assign_tag(tag) as i32;
        insns.push(Instruction::I32Const(tag_id));
        insns.push(Instruction::I32Eq);

        insns.push(Instruction::If(block_type(result_ty)));
        ctx.labels.push(LabelKind::Other);

        // Bind payload (i64 at offset 8) if the pattern names it (and is not wildcard).
        if let Some(bind_name) = binding
            && bind_name != "_"
        {
            let payload_local = ctx.bind(bind_name, ValType::I64);
            emit_local_get(ctx, scrutinee, insns);
            insns.push(Instruction::I64Load(wasm_encoder::MemArg {
                offset: 8,
                align: 3,
                memory_index: 0,
            }));
            insns.push(Instruction::LocalSet(payload_local));
        }

        emit_branch_expr(&first.body, result_ty, ctx, functions, insns);
        insns.push(Instruction::Else);
        emit_match_arms(
            scrutinee,
            scrutinee_ty,
            rest,
            result_ty,
            ctx,
            functions,
            insns,
        );
        ctx.labels.pop();
        insns.push(Instruction::End);
        return result_ty;
    }

    let can_match = match scrutinee_ty {
        ValType::I64 => parse_i64_pattern(&first.pattern)
            .map(|value| {
                emit_local_get(ctx, scrutinee, insns);
                insns.push(Instruction::I64Const(value));
                insns.push(Instruction::I64Eq);
            })
            .or_else(|| {
                parse_bool_pattern(&first.pattern).map(|value| {
                    emit_local_get(ctx, scrutinee, insns);
                    insns.push(Instruction::I64Const(if value { 1 } else { 0 }));
                    insns.push(Instruction::I64Eq);
                })
            }),
        ValType::I32 => parse_bool_pattern(&first.pattern).map(|value| {
            emit_local_get(ctx, scrutinee, insns);
            insns.push(Instruction::I32Const(if value { 1 } else { 0 }));
            insns.push(Instruction::I32Eq);
        }),
        _ => None,
    };

    if can_match.is_none() {
        // Detect compile-time unsupported pattern shapes (nested constructors,
        // multi-binding, record-field syntax) and record a structured error
        // before emitting Unreachable as a defence-in-depth instruction stream.
        if is_unsupported_pattern_shape(first.pattern.trim()) {
            ctx.set_error(CompileError::UnsupportedPatternSyntax(
                first.pattern.trim().to_string(),
            ));
        }
        // Pattern is not integer, boolean, wildcard, or a recognised constructor.
        // Unreachable is emitted so the instruction stream remains structurally
        // valid for the WASM validator; the error above is the caller-visible signal.
        insns.push(Instruction::Unreachable);
        return result_ty;
    }

    insns.push(Instruction::If(block_type(result_ty)));
    ctx.labels.push(LabelKind::Other);
    emit_branch_expr(&first.body, result_ty, ctx, functions, insns);
    insns.push(Instruction::Else);
    emit_match_arms(
        scrutinee,
        scrutinee_ty,
        rest,
        result_ty,
        ctx,
        functions,
        insns,
    );
    ctx.labels.pop();
    insns.push(Instruction::End);
    result_ty
}
