use super::expr::emit_anf_expr;
use super::*;

// ── function_index ────────────────────────────────────────────────────────

/// Build a name→function-index map from the binding list.
///
/// Both the raw name and the derived export name are mapped so that call
/// resolution works regardless of which form the caller uses.
fn function_index(bindings: &[AnfBinding], function_offset: u32) -> BTreeMap<String, u32> {
    let mut functions = BTreeMap::new();
    for (idx, binding) in bindings.iter().enumerate() {
        functions.insert(binding.name.clone(), function_offset + idx as u32);
        if let Some(source_name) = binding.name.strip_prefix("fn.") {
            functions.insert(source_name.to_string(), function_offset + idx as u32);
        }
        functions.insert(export_name(&binding.name), function_offset + idx as u32);
    }
    functions
}

// ── build_code_section ────────────────────────────────────────────────────

/// Build a code section from ANF bindings, emitting real WASM code.
///
/// Each binding produces one WASM function.  `WasmCodegenCtx` tracks local
/// variable slots for ANF let-bindings.  The final value on the stack is
/// dropped before `end` so the function type remains `() -> ()`.
///
/// `fold_reducer_type_idx` is the type-section index of the `(i64, i64) → i64`
/// fold-reducer signature, or `None` if the module contains no Fold.
///
/// `closure_reducer_type_idx` is the type-section index of the
/// `(i64, i64, i64) → i64` closure-reducer signature, or `None` when Fold
/// is absent.  Used by `emit_anf_expr` for the Fold I32 (captured-Lambda)
/// dispatch path (Wave 16A PR3).
///
/// `hoisted_lambdas` contains the `(params, body)` pairs for nested Lambda
/// bodies that were hoisted out of binding expressions (Wave 12A).  Each entry
/// produces one additional WASM function immediately after the binding
/// functions.  Their type is `(i64, i64) → i64` (fold-reducer shape) and
/// they do not appear in the export section.
///
/// `closure_hoistable_lambdas` contains `(params, captures, body)` triples for
/// Lambdas with exactly 2 params and captures (Wave 16A PR3).  Each entry
/// produces one additional WASM function with type `(i64, i64, i64) → i64`
/// (env_ptr, acc, elem → result) emitted after all hoisted Lambda functions.
/// The function body starts with preamble instructions that load each capture
/// from the env pointer before emitting the Lambda's body expression.
///
/// The counter `next_hoisted_table_idx` starts at `n_bindings` and
/// increments once per hoistable Lambda encountered during DFS traversal.
/// Similarly, `next_closure_hoisted_table_idx` starts at
/// `n_bindings + n_hoisted` and increments once per closure-hoistable Lambda.
/// The same DFS order is used in both collection passes and in `emit_anf_expr`,
/// so the table indices assigned by Lambda emission and the body indices emitted
/// here are always consistent.
///
/// Returns `Ok(None)` when `bindings` is empty AND both hoisted lists are empty.
/// Returns `Err(CompileError)` if any binding contains an unsupported pattern.
pub(crate) fn build_code_section(
    bindings: &[AnfBinding],
    effect_data: &EffectDataLayout,
    function_offset: u32,
    fold_reducer_type_idx: Option<u32>,
    closure_reducer_type_idx: Option<u32>,
    hoisted_lambdas: &[(Vec<String>, AnfExpr)],
    closure_hoistable_lambdas: &[(Vec<String>, Vec<String>, AnfExpr)],
) -> Result<Option<CodeSection>, CompileError> {
    if bindings.is_empty() && hoisted_lambdas.is_empty() && closure_hoistable_lambdas.is_empty() {
        return Ok(None);
    }
    let mut codes = CodeSection::new();
    let functions = function_index(bindings, function_offset);

    // First hoisted table index: element table index i maps to function index
    // `function_offset + i`, so table index for the first hoisted Lambda is
    // simply `bindings.len()` (not `function_offset + bindings.len()`).
    let first_hoisted_table_idx = bindings.len() as u32;
    // First closure-hoisted table index: after all regular-hoisted Lambdas.
    let first_closure_hoisted_table_idx = first_hoisted_table_idx + hoisted_lambdas.len() as u32;

    // Running counters shared (by sequential extraction) across all binding ctx.
    let mut next_hoisted_table_idx = first_hoisted_table_idx;
    let mut next_closure_hoisted_table_idx = first_closure_hoisted_table_idx;

    for binding in bindings {
        // For a top-level Lambda binding, emit the Lambda body directly so
        // that both captures (WASM function params via binding_params) and
        // Lambda-own params are in scope.  For non-Lambda bindings, emit the
        // expression as before.
        //
        // This avoids hitting the nested-Lambda arm in emit_anf_expr (which
        // emits a closure env pointer or I64 table index instead of the body).
        let (body_to_emit, lambda_own_params): (&AnfExpr, &[String]) = match &binding.expr {
            AnfExpr::Lambda { params, body, .. } => (body.as_ref(), params.as_slice()),
            other => (other, &[]),
        };

        let mut all_params = binding_params(binding);
        all_params.extend(lambda_own_params.iter().map(String::as_str));

        let mut ctx = WasmCodegenCtx::new(
            all_params,
            effect_data,
            fold_reducer_type_idx,
            closure_reducer_type_idx,
            function_offset,
            next_hoisted_table_idx,
            next_closure_hoisted_table_idx,
        );
        let mut insns: Vec<Instruction<'_>> = Vec::new();

        let emitted_ty = emit_anf_expr(body_to_emit, &mut ctx, &functions, &mut insns);

        // Propagate any compile-time error detected during emission
        // (e.g. unsupported pattern syntax in a Match arm).
        if let Some(e) = ctx.error.take() {
            return Err(e);
        }

        // Advance the shared counters: the binding may have encountered N
        // hoistable or closure-hoistable Lambdas, each consuming one slot.
        next_hoisted_table_idx = ctx.next_hoisted_table_idx;
        next_closure_hoisted_table_idx = ctx.next_closure_hoisted_table_idx;

        if binding_result(binding).is_none() && emitted_ty.is_some() {
            insns.push(Instruction::Drop);
        }
        insns.push(Instruction::End);

        // Allocate locals: one slot per let-binding (type-inferred via ctx).
        let locals = ctx
            .local_types
            .into_iter()
            .map(|ty| (1, ty))
            .collect::<Vec<_>>();

        let mut f = Function::new(locals);
        for insn in &insns {
            f.instruction(insn);
        }
        codes.function(&f);
    }

    // Emit hoisted Lambda bodies as additional WASM functions.
    //
    // Each hoisted Lambda has the fold-reducer shape `(i64, i64) → i64`:
    //   - params.len() == 2, captures.is_empty()
    //   - WASM params are the Lambda's own param names, both I64.
    //   - The body is emitted directly (no closure env wrapper).
    for (params, body) in hoisted_lambdas {
        let param_strs: Vec<&str> = params.iter().map(String::as_str).collect();
        // Hoisted Lambda ctx: uses the same functions map so the body can
        // call top-level functions by name.
        let mut ctx = WasmCodegenCtx::new(
            param_strs,
            effect_data,
            fold_reducer_type_idx,
            closure_reducer_type_idx,
            function_offset,
            next_hoisted_table_idx,
            next_closure_hoisted_table_idx,
        );
        let mut insns: Vec<Instruction<'_>> = Vec::new();

        let emitted_ty = emit_anf_expr(body, &mut ctx, &functions, &mut insns);

        // Propagate any compile-time error from the hoisted Lambda body.
        if let Some(e) = ctx.error.take() {
            return Err(e);
        }

        // Hoisted Lambda must return I64 (fold reducer: (i64, i64) → i64).
        // If the body produced I32 or nothing, extend/fill to I64.
        match emitted_ty {
            Some(ValType::I64) => {}
            Some(ValType::I32) => insns.push(Instruction::I64ExtendI32U),
            Some(_) => {
                insns.push(Instruction::Drop);
                insns.push(Instruction::I64Const(0));
            }
            None => insns.push(Instruction::I64Const(0)),
        }
        insns.push(Instruction::End);

        let locals = ctx
            .local_types
            .into_iter()
            .map(|ty| (1, ty))
            .collect::<Vec<_>>();

        let mut f = Function::new(locals);
        for insn in &insns {
            f.instruction(insn);
        }
        codes.function(&f);
    }

    // Emit closure-hoisted Lambda bodies as additional WASM functions (Wave 16A PR3).
    //
    // Each closure-hoisted Lambda has the closure-reducer shape
    // `(env_ptr: i64, acc: i64, elem: i64) → i64`:
    //   - params.len() == 2 (the user-facing acc/elem params)
    //   - captures.len() >= 1
    //   - WASM params are: __env_ptr (i64), then the Lambda's param names (i64 each).
    //   - A preamble loads each capture from the env pointer before the body.
    //
    // Capture load preamble per capture[i]:
    //   local.get __env_ptr   ; i64
    //   i32.wrap_i64          ; i32 (memory address)
    //   i64.load { offset: 16 + i*8 }  ; i64 capture value
    //   local.set capture_local
    for (params, captures, body) in closure_hoistable_lambdas {
        // WASM params: __env_ptr (i64), then user params (i64 each).
        let mut param_strs: Vec<&str> = vec!["__env_ptr"];
        param_strs.extend(params.iter().map(String::as_str));

        let mut ctx = WasmCodegenCtx::new(
            param_strs,
            effect_data,
            fold_reducer_type_idx,
            closure_reducer_type_idx,
            function_offset,
            next_hoisted_table_idx,
            next_closure_hoisted_table_idx,
        );
        let mut insns: Vec<Instruction<'_>> = Vec::new();

        // Preamble: load each capture from the env pointer.
        // env_ptr is at WASM local index 0 (first param).
        let env_ptr_local = 0u32; // always slot 0 (__env_ptr is the first param)
        for (i, cap_name) in captures.iter().enumerate() {
            let offset = (16 + i * 8) as u64;
            let cap_local = ctx.bind(cap_name.as_str(), ValType::I64);
            // Load capture from env: i32.wrap_i64(env_ptr) + offset → i64
            insns.push(Instruction::LocalGet(env_ptr_local));
            insns.push(Instruction::I32WrapI64);
            insns.push(Instruction::I64Load(wasm_encoder::MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            insns.push(Instruction::LocalSet(cap_local));
        }

        // Emit Lambda body with captures and user params in scope.
        let emitted_ty = emit_anf_expr(body, &mut ctx, &functions, &mut insns);

        // Propagate any compile-time error from the closure-hoisted Lambda body.
        if let Some(e) = ctx.error.take() {
            return Err(e);
        }

        // Closure-hoisted Lambda must return I64 (closure-reducer: (i64,i64,i64)→i64).
        match emitted_ty {
            Some(ValType::I64) => {}
            Some(ValType::I32) => insns.push(Instruction::I64ExtendI32U),
            Some(_) => {
                insns.push(Instruction::Drop);
                insns.push(Instruction::I64Const(0));
            }
            None => insns.push(Instruction::I64Const(0)),
        }
        insns.push(Instruction::End);

        let locals = ctx
            .local_types
            .into_iter()
            .map(|ty| (1, ty))
            .collect::<Vec<_>>();

        let mut f = Function::new(locals);
        for insn in &insns {
            f.instruction(insn);
        }
        codes.function(&f);
    }

    Ok(Some(codes))
}

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::NodeRef;

    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;

    use super::function_index;

    #[test]
    fn function_index_resolves_source_module_qualified_calls() {
        let bindings = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.math.add_pair".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(0)),
        }];

        let functions = function_index(&bindings, 3);

        assert_eq!(functions.get("fn.math.add_pair"), Some(&3));
        assert_eq!(functions.get("math.add_pair"), Some(&3));
        assert_eq!(functions.get("math_add_pair"), Some(&3));
        assert_eq!(functions.get("add_pair"), None);
    }
}
