// ── ail-compiler::wasm_emit ───────────────────────────────────────────────
//
// WASM instruction emission and code-section builder.
//
// Contains the ANF→WASM expression emitter (`emit_anf_expr`), the codegen
// context (`WasmCodegenCtx`), emission helper routines, and
// `build_code_section` which orchestrates them.
//
// All items except `build_code_section` are private to this module.

use std::collections::BTreeMap;

use wasm_encoder::{BlockType, CodeSection, Function, Instruction, ValType};

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;
use crate::error::CompileError;
use crate::pattern_string::{is_unsupported_pattern_shape, parse_constructor_pattern};
use crate::wasm_abi::{
    EffectDataLayout, RESULT_BUFFER_MAX, binding_params, binding_result, export_name,
    infer_expr_type, record_layout_fields, well_known_variant_tag,
};

// ── function_index ────────────────────────────────────────────────────────

/// Build a name→function-index map from the binding list.
///
/// Both the raw name and the derived export name are mapped so that call
/// resolution works regardless of which form the caller uses.
fn function_index(bindings: &[AnfBinding], function_offset: u32) -> BTreeMap<String, u32> {
    let mut functions = BTreeMap::new();
    for (idx, binding) in bindings.iter().enumerate() {
        functions.insert(binding.name.clone(), function_offset + idx as u32);
        functions.insert(export_name(&binding.name), function_offset + idx as u32);
    }
    functions
}

// ── WasmCodegenCtx ────────────────────────────────────────────────────────

/// Local-variable environment for WASM codegen.
///
/// ANF `let`-bindings are mapped to WASM `local` indices.  The context tracks
/// which name maps to which local slot so that `Var` references can be emitted
/// as `local.get <idx>`.
struct WasmCodegenCtx<'a> {
    /// Maps let-bound name → WASM local index (0-based, after params).
    locals: Vec<(&'a str, u32, ValType)>,
    /// Counter for allocating fresh local indices.
    next_local: u32,
    local_types: Vec<ValType>,
    effect_data: &'a EffectDataLayout,
    labels: Vec<LabelKind>,
    record_layouts: BTreeMap<String, Vec<String>>,
    /// Stable discriminant table: tag name → assigned u32 id.
    variant_tags: BTreeMap<String, u32>,
    /// Counter for the next unassigned variant discriminant.
    next_variant_tag: u32,
    /// Type index of the fold-reducer signature `(i64, i64) → i64` in the
    /// module type section.  `None` when no Fold is present in this module.
    fold_reducer_type_idx: Option<u32>,
    /// Type index of the closure-reducer signature `(i64, i64, i64) → i64`
    /// in the module type section.  `None` when no Fold is present.
    /// Used by the Fold I32 dispatch path (captured Lambda reducers, PR3).
    closure_reducer_type_idx: Option<u32>,
    /// Number of imported functions (host calls, resource acquire/release)
    /// that precede the defined functions in the function index space.
    /// Used to compute table indices: `table_idx = func_idx - function_offset`.
    function_offset: u32,
    /// Absolute table index to assign to the next hoistable nested Lambda
    /// encountered during expression emission.
    ///
    /// A "hoistable" Lambda is one with exactly 2 params and no captures
    /// (fold-reducer shape).  Its body is emitted as a separate WASM function;
    /// the Lambda node itself emits `i64.const <table_idx>` so the Fold can
    /// dispatch it via the I64 path (`i32.wrap_i64` + `call_indirect`).
    next_hoisted_table_idx: u32,
    /// Absolute table index to assign to the next closure-hoistable nested
    /// Lambda encountered during expression emission.
    ///
    /// A "closure-hoistable" Lambda has exactly 2 params and at least one
    /// capture.  Its body is emitted as a 3-param WASM function
    /// `(env_ptr: i64, acc: i64, elem: i64) → i64`; the Lambda node writes
    /// the real table index into the closure env's `fn_idx` slot so the Fold
    /// can dispatch it via the I32 path (`call_indirect` with closure-reducer
    /// type).
    ///
    /// Closure-hoisted table indices start at `n_bindings + n_hoisted` so
    /// they are laid out after the regular-hoisted Lambda entries.
    next_closure_hoisted_table_idx: u32,
    /// First compile-time error recorded during emission, if any.
    ///
    /// Set via `set_error`; checked by `build_code_section` after each
    /// `emit_anf_expr` call.  Only the first error is kept — subsequent
    /// calls to `set_error` are no-ops once an error is present.
    error: Option<CompileError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelKind {
    Other,
    LoopBreak,
    LoopContinue,
}

impl<'a> WasmCodegenCtx<'a> {
    fn new(
        params: Vec<&'a str>,
        effect_data: &'a EffectDataLayout,
        fold_reducer_type_idx: Option<u32>,
        closure_reducer_type_idx: Option<u32>,
        function_offset: u32,
        first_hoisted_table_idx: u32,
        first_closure_hoisted_table_idx: u32,
    ) -> Self {
        let param_count = params.len() as u32;
        WasmCodegenCtx {
            locals: params
                .into_iter()
                .enumerate()
                .map(|(idx, name)| (name, idx as u32, ValType::I64))
                .collect(),
            next_local: param_count,
            local_types: Vec::new(),
            effect_data,
            labels: Vec::new(),
            record_layouts: BTreeMap::new(),
            variant_tags: BTreeMap::new(),
            next_variant_tag: 0,
            fold_reducer_type_idx,
            closure_reducer_type_idx,
            function_offset,
            next_hoisted_table_idx: first_hoisted_table_idx,
            next_closure_hoisted_table_idx: first_closure_hoisted_table_idx,
            error: None,
        }
    }

    /// Assign a stable discriminant to `tag` within this function context.
    ///
    /// The same tag name always returns the same u32 within one codegen
    /// context.  New tags are assigned in first-encounter order (0, 1, 2, …).
    fn assign_tag(&mut self, tag: &str) -> u32 {
        if let Some(&existing) = self.variant_tags.get(tag) {
            existing
        } else if let Some(id) = well_known_variant_tag(tag) {
            self.next_variant_tag = self.next_variant_tag.max(id + 1);
            self.variant_tags.insert(tag.to_string(), id);
            id
        } else {
            let id = self.next_variant_tag;
            self.next_variant_tag += 1;
            self.variant_tags.insert(tag.to_string(), id);
            id
        }
    }

    /// Look up a variable name and return its local index.
    /// Returns `None` if the name is not in scope.
    fn lookup(&self, name: &str) -> Option<(u32, ValType)> {
        // Search from the most-recently-bound end (innermost scope).
        self.locals
            .iter()
            .rev()
            .find(|(n, _, _)| *n == name)
            .map(|(_, i, ty)| (*i, *ty))
    }

    /// Bind a new name to a fresh local slot and return the slot index.
    fn bind(&mut self, name: &'a str, ty: ValType) -> u32 {
        let idx = self.next_local;
        self.next_local += 1;
        self.locals.push((name, idx, ty));
        self.local_types.push(ty);
        idx
    }

    fn bind_record_layout(&mut self, name: &str, fields: Vec<String>) {
        self.record_layouts.insert(name.to_string(), fields);
    }

    fn field_offset(&self, record: &str, field: &str) -> u64 {
        self.record_layouts
            .get(record)
            .and_then(|fields| fields.iter().position(|candidate| candidate == field))
            .unwrap_or_else(|| field.parse::<usize>().unwrap_or(0)) as u64
            * 8
    }

    fn expr_type(&self, expr: &AnfExpr) -> Option<ValType> {
        let mut locals = self
            .locals
            .iter()
            .map(|(name, _, ty)| ((*name).to_string(), *ty))
            .collect();
        infer_expr_type(expr, &mut locals)
    }

    fn branch_depth(&self, target: LabelKind) -> Option<u32> {
        self.labels
            .iter()
            .rposition(|label| *label == target)
            .map(|idx| (self.labels.len() - 1 - idx) as u32)
    }

    /// Record the first compile-time error encountered during emission.
    ///
    /// Subsequent calls are no-ops — only the first error is kept so that
    /// callers see the root cause rather than a cascade of follow-on errors.
    fn set_error(&mut self, e: CompileError) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }
}

// ── Emission helpers ──────────────────────────────────────────────────────

fn emit_alloc<'a>(size: i32, insns: &mut Vec<Instruction<'a>>) {
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::GlobalGet(0));
    insns.push(Instruction::I32Const(size));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::GlobalSet(0));
}

fn emit_i64_value<'a>(
    expr: &'a AnfExpr,
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) {
    match emit_anf_expr(expr, ctx, functions, insns) {
        Some(ValType::I64) => {}
        Some(ValType::I32) => insns.push(Instruction::I64ExtendI32U),
        Some(_) => {
            insns.push(Instruction::Drop);
            insns.push(Instruction::I64Const(0));
        }
        None => insns.push(Instruction::I64Const(0)),
    }
}

fn store_i64_at<'a>(offset: u64, insns: &mut Vec<Instruction<'a>>) {
    insns.push(Instruction::I64Store(wasm_encoder::MemArg {
        offset,
        align: 3,
        memory_index: 0,
    }));
}

fn load_i64_at<'a>(offset: u64, insns: &mut Vec<Instruction<'a>>) {
    insns.push(Instruction::I64Load(wasm_encoder::MemArg {
        offset,
        align: 3,
        memory_index: 0,
    }));
}

fn block_type(result_ty: Option<ValType>) -> BlockType {
    result_ty.map(BlockType::Result).unwrap_or(BlockType::Empty)
}

fn emit_branch_expr<'a>(
    expr: &'a AnfExpr,
    result_ty: Option<ValType>,
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    let emitted_ty = emit_anf_expr(expr, ctx, functions, insns);
    if result_ty.is_none() && emitted_ty.is_some() {
        insns.push(Instruction::Drop);
        // The value was dropped: no value remains on the stack.
        // Return None so callers do not try to consume or drop again.
        return None;
    }
    emitted_ty
}

fn emit_local_get<'a>(ctx: &WasmCodegenCtx<'a>, name: &str, insns: &mut Vec<Instruction<'a>>) {
    if let Some((idx, _)) = ctx.lookup(name) {
        insns.push(Instruction::LocalGet(idx));
    } else {
        insns.push(Instruction::Unreachable);
    }
}

fn emit_i64_primitive_call<'a>(
    func: &str,
    arg_count: usize,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    match (func, arg_count) {
        ("i64.add" | "+" | "add", 2) => insns.push(Instruction::I64Add),
        ("i64.sub" | "-" | "sub", 2) => insns.push(Instruction::I64Sub),
        ("i64.mul" | "*" | "mul", 2) => insns.push(Instruction::I64Mul),
        ("i64.div_s" | "/" | "div", 2) => insns.push(Instruction::I64DivS),
        ("i64.rem_s" | "%" | "mod", 2) => insns.push(Instruction::I64RemS),
        ("i64.and" | "and", 2) => insns.push(Instruction::I64And),
        ("i64.or" | "or", 2) => insns.push(Instruction::I64Or),
        ("i64.neg" | "neg" | "negate", 1) => {
            insns.push(Instruction::I64Const(-1));
            insns.push(Instruction::I64Mul);
        }
        ("i64.eq" | "==" | "eq", 2) => {
            insns.push(Instruction::I64Eq);
            insns.push(Instruction::I64ExtendI32U);
        }
        ("i64.ne" | "!=" | "ne", 2) => {
            insns.push(Instruction::I64Ne);
            insns.push(Instruction::I64ExtendI32U);
        }
        ("i64.lt_s" | "<" | "lt", 2) => {
            insns.push(Instruction::I64LtS);
            insns.push(Instruction::I64ExtendI32U);
        }
        ("i64.le_s" | "<=" | "le", 2) => {
            insns.push(Instruction::I64LeS);
            insns.push(Instruction::I64ExtendI32U);
        }
        ("i64.gt_s" | ">" | "gt", 2) => {
            insns.push(Instruction::I64GtS);
            insns.push(Instruction::I64ExtendI32U);
        }
        ("i64.ge_s" | ">=" | "ge", 2) => {
            insns.push(Instruction::I64GeS);
            insns.push(Instruction::I64ExtendI32U);
        }
        ("i64.eqz" | "not" | "!", 1) => {
            insns.push(Instruction::I64Eqz);
            insns.push(Instruction::I64ExtendI32U);
        }
        _ => return None,
    }
    Some(ValType::I64)
}

/// Load a local variable as an I64, zero-extending I32 values.
/// Emits `Unreachable` if the name is not in scope, matching `emit_local_get`.
fn emit_local_as_i64<'a>(ctx: &WasmCodegenCtx<'a>, name: &str, insns: &mut Vec<Instruction<'a>>) {
    if let Some((idx, ty)) = ctx.lookup(name) {
        insns.push(Instruction::LocalGet(idx));
        if ty == ValType::I32 {
            insns.push(Instruction::I64ExtendI32U);
        }
    } else {
        insns.push(Instruction::Unreachable);
    }
}

fn emit_condition_get<'a>(ctx: &WasmCodegenCtx<'a>, name: &str, insns: &mut Vec<Instruction<'a>>) {
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

fn parse_i64_pattern(pattern: &str) -> Option<i64> {
    pattern.trim().parse::<i64>().ok()
}

fn parse_bool_pattern(pattern: &str) -> Option<bool> {
    match pattern.trim() {
        "true" | "True" => Some(true),
        "false" | "False" => Some(false),
        _ => None,
    }
}

fn emit_match_arms<'a>(
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

// ── emit_anf_expr ─────────────────────────────────────────────────────────

/// Emit WASM instructions for one `AnfExpr` into `insns`.
///
/// The emitted sequence leaves exactly one value on the WASM operand stack
/// for value-producing expressions, or zero for effect-only statements.
/// The caller is responsible for consuming (or dropping) that value.
///
/// Locals in `ctx` map ANF names to WASM local indices; new `Let` bindings
/// allocate fresh slots via `ctx.bind`.
fn emit_anf_expr<'a>(
    expr: &'a AnfExpr,
    ctx: &mut WasmCodegenCtx<'a>,
    functions: &BTreeMap<String, u32>,
    insns: &mut Vec<Instruction<'a>>,
) -> Option<ValType> {
    match expr {
        // ── Literals ──────────────────────────────────────────────────────
        AnfExpr::Literal(lit) => match lit {
            LiteralValue::Int(n) => {
                insns.push(Instruction::I64Const(*n));
                Some(ValType::I64)
            }
            LiteralValue::Bool(b) => {
                insns.push(Instruction::I64Const(if *b { 1 } else { 0 }));
                Some(ValType::I64)
            }
            LiteralValue::Float(f) => {
                // wasm_encoder 0.244 requires Ieee64 for F64Const.
                insns.push(Instruction::F64Const(wasm_encoder::Ieee64::from(*f)));
                Some(ValType::F64)
            }
            LiteralValue::Text(s) => {
                // Encode as: i64 = (len as u64) << 32 | (ptr as u64)
                let (ptr, len) = ctx.effect_data.string(s);
                let packed = ((len as i64) << 32) | (ptr as i64);
                insns.push(Instruction::I64Const(packed));
                Some(ValType::I64)
            }
            LiteralValue::Bytes(data) => {
                // Same packed encoding as Text: upper 32 = len, lower 32 = ptr.
                // The runtime decodes this via ValueLayout::Bytes →
                // StructuredValue::Bytes { ptr, len } with no UTF-8 assumption.
                let (ptr, len) = ctx.effect_data.bytes(data);
                let packed = ((len as i64) << 32) | (ptr as i64);
                insns.push(Instruction::I64Const(packed));
                Some(ValType::I64)
            }
            LiteralValue::Unit => {
                insns.push(Instruction::I32Const(0));
                Some(ValType::I32)
            }
        },

        // ── Variable reference ────────────────────────────────────────────
        AnfExpr::Var(name) => {
            if let Some((idx, ty)) = ctx.lookup(name) {
                insns.push(Instruction::LocalGet(idx));
                Some(ty)
            } else {
                // Unbound variable — emit unreachable (catches missing bindings).
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── Let binding ───────────────────────────────────────────────────
        AnfExpr::Let { name, value, body } => {
            // Emit value expression (leaves one value on stack).
            let value_ty = emit_anf_expr(value, ctx, functions, insns).unwrap_or(ValType::I32);
            // Allocate a fresh local and set it.
            let idx = ctx.bind(name, value_ty);
            insns.push(Instruction::LocalSet(idx));
            if let Some(fields) = record_layout_fields(value) {
                ctx.bind_record_layout(name, fields);
            }
            // Emit the body with the new binding in scope.
            emit_anf_expr(body, ctx, functions, insns)
        }

        // ── Conditional (short-circuit AND/OR) ────────────────────────────
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            // Condition: look up the atomic variable.
            emit_condition_get(ctx, cond, insns);
            let result_ty = ctx
                .expr_type(then_branch)
                .filter(|ty| Some(*ty) == ctx.expr_type(else_branch));
            insns.push(Instruction::If(block_type(result_ty)));
            ctx.labels.push(LabelKind::Other);
            emit_branch_expr(then_branch, result_ty, ctx, functions, insns);
            insns.push(Instruction::Else);
            emit_branch_expr(else_branch, result_ty, ctx, functions, insns);
            ctx.labels.pop();
            insns.push(Instruction::End);
            result_ty
        }

        // ── Short-circuit AND ─────────────────────────────────────────────
        // if left { right } else { false }
        AnfExpr::ShortCircuitAnd { left, right } => {
            emit_condition_get(ctx, left, insns);
            insns.push(Instruction::If(BlockType::Result(ValType::I64)));
            ctx.labels.push(LabelKind::Other);
            emit_anf_expr(right, ctx, functions, insns);
            insns.push(Instruction::Else);
            insns.push(Instruction::I64Const(0));
            ctx.labels.pop();
            insns.push(Instruction::End);
            Some(ValType::I64)
        }

        // ── Short-circuit OR ──────────────────────────────────────────────
        // if left { true } else { right }
        AnfExpr::ShortCircuitOr { left, right } => {
            emit_condition_get(ctx, left, insns);
            insns.push(Instruction::If(BlockType::Result(ValType::I64)));
            ctx.labels.push(LabelKind::Other);
            insns.push(Instruction::I64Const(1));
            insns.push(Instruction::Else);
            emit_anf_expr(right, ctx, functions, insns);
            ctx.labels.pop();
            insns.push(Instruction::End);
            Some(ValType::I64)
        }

        AnfExpr::Loop { body } => {
            let result_ty = ctx.expr_type(body);
            insns.push(Instruction::Block(block_type(result_ty)));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);
            let emitted_ty = emit_anf_expr(body, ctx, functions, insns);
            if result_ty.is_none() && emitted_ty.is_some() {
                insns.push(Instruction::Drop);
            }
            insns.push(Instruction::Br(1));
            ctx.labels.pop();
            insns.push(Instruction::End);
            if result_ty.is_some() {
                insns.push(Instruction::Unreachable);
            }
            ctx.labels.pop();
            insns.push(Instruction::End);
            result_ty
        }

        AnfExpr::Break { value } => {
            emit_anf_expr(value, ctx, functions, insns);
            if let Some(depth) = ctx.branch_depth(LabelKind::LoopBreak) {
                insns.push(Instruction::Br(depth));
            } else {
                insns.push(Instruction::Unreachable);
            }
            None
        }

        AnfExpr::Continue => {
            if let Some(depth) = ctx.branch_depth(LabelKind::LoopContinue) {
                insns.push(Instruction::Br(depth));
            } else {
                insns.push(Instruction::Unreachable);
            }
            None
        }

        AnfExpr::WhileLoop { cond, body } => {
            insns.push(Instruction::Block(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);
            emit_condition_get(ctx, cond, insns);
            insns.push(Instruction::I32Eqz);
            insns.push(Instruction::BrIf(1));
            let emitted_ty = emit_anf_expr(body, ctx, functions, insns);
            if emitted_ty.is_some() {
                insns.push(Instruction::Drop);
            }
            insns.push(Instruction::Br(0));
            ctx.labels.pop();
            insns.push(Instruction::End);
            ctx.labels.pop();
            insns.push(Instruction::End);
            // WhileLoop is side-effect only in terms of semantics, but it must
            // produce a unit value on the WASM stack so that it can appear as
            // the `value` in an `AnfExpr::Let` binding or as an intermediate
            // element in a `Seq` without causing a stack-underflow validation
            // error.  Push I32 0 (unit) here — mirrors the ForEach fix (Wave 18B).
            insns.push(Instruction::I32Const(0));
            Some(ValType::I32)
        }

        // ── Sequence ──────────────────────────────────────────────────────
        AnfExpr::Seq(exprs) => {
            let mut last_ty = Some(ValType::I32);
            for (i, e) in exprs.iter().enumerate() {
                last_ty = emit_anf_expr(e, ctx, functions, insns);
                // Drop intermediate results (all but the last).
                if i + 1 < exprs.len() {
                    insns.push(Instruction::Drop);
                }
            }
            // Empty Seq → push unit (i32.const 0).
            if exprs.is_empty() {
                insns.push(Instruction::I32Const(0));
            }
            last_ty
        }

        // ── Return ────────────────────────────────────────────────────────
        AnfExpr::Return(inner) => {
            emit_anf_expr(inner, ctx, functions, insns);
            insns.push(Instruction::Return);
            None
        }

        // ── Function call (pure) ──────────────────────────────────────────
        // Emits args via local.get, then calls the function.
        AnfExpr::Call { func, args } => {
            for arg_name in args {
                if let Some((idx, _)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(idx));
                } else {
                    insns.push(Instruction::Unreachable);
                    return None;
                }
            }
            if let Some(ty) = emit_i64_primitive_call(func, args.len(), insns) {
                Some(ty)
            } else if let Some(idx) = functions.get(func) {
                insns.push(Instruction::Call(*idx));
                Some(ValType::I64)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        AnfExpr::EffectCall {
            capability,
            func,
            args,
        } => {
            for (idx, arg_name) in args.iter().enumerate() {
                insns.push(Instruction::I32Const(
                    ctx.effect_data.args_offset + (idx as i32 * 8),
                ));
                if let Some((local_idx, arg_ty)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(local_idx));
                    // Zero-extend I32 args to I64 before storing in the args buffer.
                    // I64 args are already the right width and need no extension.
                    if arg_ty == ValType::I32 {
                        insns.push(Instruction::I64ExtendI32U);
                    }
                    insns.push(Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                } else {
                    insns.push(Instruction::Unreachable);
                    return None;
                }
            }

            let (cap_ptr, cap_len) = ctx.effect_data.string(capability);
            let (op_ptr, op_len) = ctx.effect_data.string(func);
            insns.push(Instruction::I32Const(cap_ptr));
            insns.push(Instruction::I32Const(cap_len));
            insns.push(Instruction::I32Const(op_ptr));
            insns.push(Instruction::I32Const(op_len));
            insns.push(Instruction::I32Const(ctx.effect_data.args_offset));
            insns.push(Instruction::I32Const(args.len() as i32));

            if ctx.effect_data.needs_host_call_write {
                // host_call_write: (cap, op, args, out_ptr, out_max) → i32
                // Function index 1 (after host_call at 0).
                insns.push(Instruction::I32Const(ctx.effect_data.result_buffer_offset));
                insns.push(Instruction::I32Const(RESULT_BUFFER_MAX));
                insns.push(Instruction::Call(1));
                // Extend the i32 return to i64 to match the standard EffectCall return type.
                insns.push(Instruction::I64ExtendI32S);
            } else {
                insns.push(Instruction::Call(0));
            }
            Some(ValType::I64)
        }

        // ── FieldGet ──────────────────────────────────────────────────────
        AnfExpr::FieldGet { record, field } => {
            if let Some((idx, _)) = ctx.lookup(record) {
                insns.push(Instruction::LocalGet(idx));
                load_i64_at(ctx.field_offset(record, field), insns);
                Some(ValType::I64)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── FieldUpdate ───────────────────────────────────────────────────
        AnfExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            let Some((idx, _)) = ctx.lookup(record) else {
                insns.push(Instruction::Unreachable);
                return None;
            };
            insns.push(Instruction::LocalGet(idx));
            emit_i64_value(value, ctx, functions, insns);
            store_i64_at(ctx.field_offset(record, field), insns);
            if let Some((idx, ty)) = ctx.lookup(record) {
                insns.push(Instruction::LocalGet(idx));
                Some(ty)
            } else {
                None
            }
        }

        // ── RecordNew ─────────────────────────────────────────────────────
        AnfExpr::RecordNew { fields } => {
            emit_alloc((fields.len() * 8).max(1) as i32, insns);
            let ptr = ctx.bind("__record_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            for (idx, (_, v)) in fields.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(v, ctx, functions, insns);
                store_i64_at((idx * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── TupleNew ─────────────────────────────────────────────────────
        AnfExpr::TupleNew(elems) => {
            emit_alloc((elems.len() * 8).max(1) as i32, insns);
            let ptr = ctx.bind("__tuple_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            for (idx, e) in elems.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(e, ctx, functions, insns);
                store_i64_at((idx * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── VariantNew ───────────────────────────────────────────────────
        AnfExpr::VariantNew { tag, payload } => {
            emit_alloc(16, insns);
            let ptr = ctx.bind("__variant_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            insns.push(Instruction::LocalGet(ptr));
            let tag_id = ctx.assign_tag(tag) as i32;
            insns.push(Instruction::I32Const(tag_id));
            insns.push(Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
            if let Some(p) = payload {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(p, ctx, functions, insns);
                store_i64_at(8, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── ListNew ──────────────────────────────────────────────────────
        AnfExpr::ListNew(elems) => {
            emit_alloc((8 + elems.len() * 8) as i32, insns);
            let ptr = ctx.bind("__list_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            insns.push(Instruction::LocalGet(ptr));
            insns.push(Instruction::I64Const(elems.len() as i64));
            store_i64_at(0, insns);
            for (idx, e) in elems.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_i64_value(e, ctx, functions, insns);
                store_i64_at(8 + (idx * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── Match ─────────────────────────────────────────────────────────
        // Emit as a series of block/if nesting over the arms.
        // For now uses a simplified linear-scan pattern.
        AnfExpr::Match { scrutinee, arms } => {
            if let Some((_, scrutinee_ty)) = ctx.lookup(scrutinee) {
                let result_ty = ctx.expr_type(expr);
                emit_match_arms(
                    scrutinee,
                    scrutinee_ty,
                    arms,
                    result_ty,
                    ctx,
                    functions,
                    insns,
                )
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── Lambda (nested sub-expression) ───────────────────────────────
        // When a Lambda appears as a sub-expression (not the top-level body
        // of a binding — that case is handled in build_code_section), emit
        // one of three shapes depending on the Lambda's arity and captures.
        //
        // ## Hoistable fold reducer (params.len() == 2, captures.is_empty())
        //
        // A Lambda with exactly 2 parameters and no captures matches the
        // fold-reducer shape `(i64, i64) → i64`.  Its body is hoisted into a
        // separate WASM function by `build_code_section`; this arm emits only
        // the table index as an `i64.const` so Fold can dispatch it via the
        // existing I64 path (`i32.wrap_i64` + `call_indirect`).
        //
        // ## Closure-hoistable reducer (params.len() == 2, !captures.is_empty())
        // (Wave 16A PR3)
        //
        // A Lambda with exactly 2 parameters and at least one capture.  Its
        // body is hoisted into a 3-param WASM function
        // `(env_ptr: i64, acc: i64, elem: i64) → i64` by `build_code_section`.
        // This arm emits a closure env struct in linear memory and writes the
        // REAL table index into the fn_idx slot (offset 0), so Fold can
        // dispatch it via the I32 path using call_indirect with the
        // closure-reducer type.
        //
        // ## Non-hoistable Lambda (with captures, params.len() != 2)
        //
        // Emit a closure env struct in linear memory with fn_idx = 0
        // (placeholder — these Lambdas cannot be Fold reducers).
        //
        // Closure env layout (all cases):
        //   [fn_idx: i64, cap_count: i64, cap0: i64, ..., capN-1: i64]
        AnfExpr::Lambda {
            params,
            captures,
            body: _,
        } => {
            if params.len() == 2 && captures.is_empty() && ctx.fold_reducer_type_idx.is_some() {
                // Hoistable fold reducer: emit table index directly as I64.
                // Only reached when a function table exists (fold_reducer_type_idx.is_some()),
                // guaranteeing the table and hoisted body are present.
                // `build_code_section` emits the body as an extra function at
                // the same index, in the same DFS encounter order.
                let table_idx = ctx.next_hoisted_table_idx;
                ctx.next_hoisted_table_idx += 1;
                insns.push(Instruction::I64Const(i64::from(table_idx)));
                Some(ValType::I64)
            } else if params.len() == 2
                && !captures.is_empty()
                && ctx.closure_reducer_type_idx.is_some()
            {
                // Closure-hoistable reducer (Wave 16A PR3): emit closure env
                // with the REAL table index in the fn_idx slot.
                let table_idx = ctx.next_closure_hoisted_table_idx;
                ctx.next_closure_hoisted_table_idx += 1;

                let cap_count = captures.len();
                // Allocate: fn_idx (8 B) + cap_count (8 B) + N × 8 B.
                let byte_size = ((2 + cap_count) * 8) as i32;
                emit_alloc(byte_size, insns);
                let ptr_local = ctx.bind("__closure_env", ValType::I32);
                insns.push(Instruction::LocalSet(ptr_local));

                // fn_idx at offset 0 — REAL table index (not placeholder).
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(i64::from(table_idx)));
                store_i64_at(0, insns);

                // cap_count at offset 8.
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(cap_count as i64));
                store_i64_at(8, insns);

                // Each captured value at offset 16, 24, …
                for (i, cap_name) in captures.iter().enumerate() {
                    let offset = (16 + i * 8) as u64;
                    insns.push(Instruction::LocalGet(ptr_local));
                    if let Some((idx, ty)) = ctx.lookup(cap_name) {
                        insns.push(Instruction::LocalGet(idx));
                        // Zero-extend I32 captures to I64 for uniform storage.
                        if ty == ValType::I32 {
                            insns.push(Instruction::I64ExtendI32U);
                        }
                    } else {
                        insns.push(Instruction::I64Const(0));
                    }
                    store_i64_at(offset, insns);
                }

                insns.push(Instruction::LocalGet(ptr_local));
                Some(ValType::I32)
            } else {
                // Non-hoistable Lambda: emit closure env with fn_idx = 0
                // (placeholder — cannot be a Fold reducer).
                let cap_count = captures.len();
                let byte_size = ((2 + cap_count) * 8) as i32;
                emit_alloc(byte_size, insns);
                let ptr_local = ctx.bind("__closure_env", ValType::I32);
                insns.push(Instruction::LocalSet(ptr_local));

                // fn_idx at offset 0 (placeholder = 0).
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(0));
                store_i64_at(0, insns);

                // cap_count at offset 8.
                insns.push(Instruction::LocalGet(ptr_local));
                insns.push(Instruction::I64Const(cap_count as i64));
                store_i64_at(8, insns);

                // Each captured value at offset 16, 24, …
                for (i, cap_name) in captures.iter().enumerate() {
                    let offset = (16 + i * 8) as u64;
                    insns.push(Instruction::LocalGet(ptr_local));
                    if let Some((idx, ty)) = ctx.lookup(cap_name) {
                        insns.push(Instruction::LocalGet(idx));
                        if ty == ValType::I32 {
                            insns.push(Instruction::I64ExtendI32U);
                        }
                    } else {
                        insns.push(Instruction::I64Const(0));
                    }
                    store_i64_at(offset, insns);
                }

                insns.push(Instruction::LocalGet(ptr_local));
                Some(ValType::I32)
            }
        }

        // ── CellNew — allocate an 8-byte mutable cell initialised to `init` ─
        //
        // Layout: [value: i64] at offset 0.
        // Returns: I32 pointer to the cell.
        AnfExpr::CellNew { init } => {
            emit_alloc(8, insns);
            let ptr = ctx.bind("__cell_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            insns.push(Instruction::LocalGet(ptr));
            emit_local_as_i64(ctx, init, insns);
            store_i64_at(0, insns);
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── CellGet — read the I64 value stored in a cell ─────────────────
        //
        // `cell` is an I32 pointer (produced by CellNew).
        // Returns: I64 value at offset 0 of the cell.
        AnfExpr::CellGet { cell } => {
            if let Some((idx, _)) = ctx.lookup(cell) {
                insns.push(Instruction::LocalGet(idx));
                load_i64_at(0, insns);
                Some(ValType::I64)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── CellSet — write a new value into a cell ────────────────────────
        //
        // `cell` is an I32 pointer; `value` is the new I64 value.
        // Returns: unit (I32 0).
        AnfExpr::CellSet { cell, value } => {
            if let Some((cell_idx, _)) = ctx.lookup(cell) {
                insns.push(Instruction::LocalGet(cell_idx));
                emit_local_as_i64(ctx, value, insns);
                store_i64_at(0, insns);
                insns.push(Instruction::I32Const(0));
                Some(ValType::I32)
            } else {
                insns.push(Instruction::Unreachable);
                None
            }
        }

        // ── Concurrency / dispatch stubs (defence-in-depth) ──────────────
        //
        // `emit_wasm_with_profile` detects these before code generation and
        // returns `CompileError::UnsupportedWasmConstruct` so callers never
        // reach these arms via the top-level entry point.
        //
        // The `unreachable` here is a defence-in-depth fallback: unit tests or
        // other callers that invoke `emit_anf_expr` directly (bypassing
        // `emit_wasm_with_profile`) will still get a runtime trap rather than
        // undefined behaviour or silent corruption.
        AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Select { .. }
        | AnfExpr::Timeout { .. } => {
            insns.push(Instruction::Unreachable);
            None
        }

        // ── ResourceAcquire ───────────────────────────────────────────────
        //
        // ABI: `ail/resource_acquire(res_ptr: i32, res_len: i32,
        //                             args_ptr: i32, args_count: i32) → i64`
        //
        // The resource name is stored in the data section (interned by
        // `EffectDataLayout::collect_expr`).  Each arg is written as an i64
        // into the shared args buffer at `args_offset + i * 8`, then
        // `resource_acquire` is called with the buffer start and count.
        // Returns an opaque handle packed as i64.
        AnfExpr::ResourceAcquire { resource, args } => {
            // Write args into the shared args buffer.
            for (idx, arg_name) in args.iter().enumerate() {
                insns.push(Instruction::I32Const(
                    ctx.effect_data.args_offset + (idx as i32 * 8),
                ));
                if let Some((local_idx, arg_ty)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(local_idx));
                    if arg_ty == ValType::I32 {
                        insns.push(Instruction::I64ExtendI32U);
                    }
                    insns.push(Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                } else {
                    insns.push(Instruction::Unreachable);
                    return None;
                }
            }
            // Push resource name (ptr, len) from the interned data section.
            let (res_ptr, res_len) = ctx.effect_data.string(resource);
            insns.push(Instruction::I32Const(res_ptr));
            insns.push(Instruction::I32Const(res_len));
            // Push args buffer start and count.
            insns.push(Instruction::I32Const(ctx.effect_data.args_offset));
            insns.push(Instruction::I32Const(args.len() as i32));
            // Call ail/resource_acquire.
            insns.push(Instruction::Call(
                ctx.effect_data.resource_acquire_func_index(),
            ));
            Some(ValType::I64)
        }

        // ── ResourceRelease ───────────────────────────────────────────────
        //
        // ABI: `ail/resource_release(handle: i64) → (void)`
        //
        // The handle local is pushed as i64 and passed directly to
        // `resource_release`.  No return value.
        AnfExpr::ResourceRelease { handle } => {
            if let Some((local_idx, handle_ty)) = ctx.lookup(handle) {
                insns.push(Instruction::LocalGet(local_idx));
                // Handles are i64; extend if the local was stored as i32.
                if handle_ty == ValType::I32 {
                    insns.push(Instruction::I64ExtendI32U);
                }
            } else {
                insns.push(Instruction::Unreachable);
                return None;
            }
            insns.push(Instruction::Call(
                ctx.effect_data.resource_release_func_index(),
            ));
            None
        }

        // ── RuntimeCheck ─────────────────────────────────────────────────
        // Emit a conditional trap: if `cond` is non-zero (violation detected)
        // → Unreachable.  If `cond` is zero → continue silently.
        AnfExpr::RuntimeCheck { cond, .. } => {
            emit_condition_get(ctx, cond, insns);
            insns.push(Instruction::If(wasm_encoder::BlockType::Empty));
            insns.push(Instruction::Unreachable);
            insns.push(Instruction::End);
            None
        }

        // ── ola5 Gap 2 — new primitives (WASM stubs) ─────────────────────
        // Assume: no runtime effect.
        AnfExpr::Assume { .. } => None,
        // Abort: always unreachable.
        AnfExpr::Abort { .. } => {
            insns.push(Instruction::Unreachable);
            None
        }
        // ── MapNew — construct a key-value map in linear memory ───────────
        //
        // Layout: [count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...]
        // Returns: I32 pointer to the map header.
        AnfExpr::MapNew { entries } => {
            let byte_size = ((1 + entries.len() * 2) * 8).max(8) as i32;
            emit_alloc(byte_size, insns);
            let ptr = ctx.bind("__map_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            // Store count at offset 0.
            insns.push(Instruction::LocalGet(ptr));
            insns.push(Instruction::I64Const(entries.len() as i64));
            store_i64_at(0, insns);
            // Store interleaved key-value pairs: k at 8+i*16, v at 16+i*16.
            for (i, (k, v)) in entries.iter().enumerate() {
                let key_offset = (8 + i * 16) as u64;
                let val_offset = (16 + i * 16) as u64;
                insns.push(Instruction::LocalGet(ptr));
                emit_local_as_i64(ctx, k, insns);
                store_i64_at(key_offset, insns);
                insns.push(Instruction::LocalGet(ptr));
                emit_local_as_i64(ctx, v, insns);
                store_i64_at(val_offset, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── SetNew — construct a set in linear memory ──────────────────────
        //
        // Layout: [count: i64, elem0: i64, elem1: i64, ...]
        // Returns: I32 pointer to the set header.
        AnfExpr::SetNew { elements } => {
            let byte_size = ((1 + elements.len()) * 8).max(8) as i32;
            emit_alloc(byte_size, insns);
            let ptr = ctx.bind("__set_ptr", ValType::I32);
            insns.push(Instruction::LocalSet(ptr));
            // Store count at offset 0.
            insns.push(Instruction::LocalGet(ptr));
            insns.push(Instruction::I64Const(elements.len() as i64));
            store_i64_at(0, insns);
            // Store elements at offsets 8, 16, ...
            for (i, elem) in elements.iter().enumerate() {
                insns.push(Instruction::LocalGet(ptr));
                emit_local_as_i64(ctx, elem, insns);
                store_i64_at((8 + i * 8) as u64, insns);
            }
            insns.push(Instruction::LocalGet(ptr));
            Some(ValType::I32)
        }

        // ── IndexGet — dynamic indexed element access from a list ──────────
        //
        // List layout: [len: i64, elem0: i64, elem1: i64, ...]
        // Element at index i: ptr + 8 + i * 8
        // Returns: I64 element value.
        //
        // Emission sequence:
        //   local.get collection   ; [I32] list pointer
        //   local.get index        ; [I32, I64]
        //   i64.const 8
        //   i64.mul                ; [I32, I64]  index * 8
        //   i64.const 8
        //   i64.add                ; [I32, I64]  8 + index * 8
        //   i32.wrap_i64           ; [I32, I32]  byte offset
        //   i32.add                ; [I32]        ptr + 8 + index * 8
        //   i64.load { offset: 0 } ; [I64]        element
        AnfExpr::IndexGet { collection, index } => {
            let Some((coll_idx, _)) = ctx.lookup(collection) else {
                insns.push(Instruction::Unreachable);
                return None;
            };
            let Some((idx_idx, idx_ty)) = ctx.lookup(index) else {
                insns.push(Instruction::Unreachable);
                return None;
            };
            insns.push(Instruction::LocalGet(coll_idx));
            insns.push(Instruction::LocalGet(idx_idx));
            // Normalise index to I64 for arithmetic.
            if idx_ty == ValType::I32 {
                insns.push(Instruction::I64ExtendI32U);
            }
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Mul);
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::I32WrapI64);
            insns.push(Instruction::I32Add);
            load_i64_at(0, insns);
            Some(ValType::I64)
        }

        // ── ForEach — inline loop over a length-prefixed list ────────────
        //
        // List layout: [count: i64, elem0: i64, elem1: i64, ...]
        //
        // Emission:
        //   1. Load count from list header (offset 0).
        //   2. Initialise loop counter to 0.
        //   3. block (empty) — break target.
        //   4.   loop (empty) — continue target.
        //   5.     i >= count  → br_if 1 (exit block).
        //   6.     Load element at coll_ptr + 8 + i * 8.
        //   7.     Store element to `binding` local.
        //   8.     Emit body; drop result (ForEach is side-effect only).
        //   9.     i += 1; br 0 (restart loop).
        //  10. end loop / end block.
        //
        // No call_indirect is required: the body is already an inlined
        // AnfExpr, so the loop executes it directly without a function
        // pointer dispatch.
        AnfExpr::ForEach {
            binding,
            collection,
            body,
        } => {
            let Some((coll_idx, _)) = ctx.lookup(collection) else {
                insns.push(Instruction::Unreachable);
                return None;
            };

            // Allocate locals: count (I64), loop counter (I64), loop var (I64).
            let count_idx = ctx.bind("__foreach_count", ValType::I64);
            let i_idx = ctx.bind("__foreach_i", ValType::I64);
            let elem_idx = ctx.bind(binding.as_str(), ValType::I64);

            // Load element count from list header at offset 0.
            insns.push(Instruction::LocalGet(coll_idx));
            load_i64_at(0, insns);
            insns.push(Instruction::LocalSet(count_idx));

            // Initialise counter to 0.
            insns.push(Instruction::I64Const(0));
            insns.push(Instruction::LocalSet(i_idx));

            // block (break target) + loop (continue target).
            insns.push(Instruction::Block(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopBreak);
            insns.push(Instruction::Loop(BlockType::Empty));
            ctx.labels.push(LabelKind::LoopContinue);

            // Exit condition: i >= count → break.
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::LocalGet(count_idx));
            insns.push(Instruction::I64GeU);
            let break_depth = ctx.branch_depth(LabelKind::LoopBreak).unwrap_or(1);
            insns.push(Instruction::BrIf(break_depth));

            // Load element at coll_ptr + 8 + i * 8.
            insns.push(Instruction::LocalGet(coll_idx));
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Mul);
            insns.push(Instruction::I64Const(8));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::I32WrapI64);
            insns.push(Instruction::I32Add);
            load_i64_at(0, insns);
            insns.push(Instruction::LocalSet(elem_idx));

            // Emit loop body; discard any produced value.
            let body_ty = emit_anf_expr(body, ctx, functions, insns);
            if body_ty.is_some() {
                insns.push(Instruction::Drop);
            }

            // Increment counter: i += 1.
            insns.push(Instruction::LocalGet(i_idx));
            insns.push(Instruction::I64Const(1));
            insns.push(Instruction::I64Add);
            insns.push(Instruction::LocalSet(i_idx));

            // Jump back to top of loop.
            insns.push(Instruction::Br(0));

            ctx.labels.pop();
            insns.push(Instruction::End); // end loop
            ctx.labels.pop();
            insns.push(Instruction::End); // end block

            // ForEach is side-effect only in terms of semantics, but it
            // must produce a unit value on the WASM stack so that it can
            // appear as the `value` in an `AnfExpr::Let` binding or as an
            // intermediate element in a `Seq` without causing a stack-
            // underflow validation error.  Push I32 0 (unit) here so that
            // the enclosing `LocalSet` or `Drop` has something to consume.
            insns.push(Instruction::I32Const(0));
            Some(ValType::I32)
        }

        // ── Fold — call_indirect over function table ──────────────────────
        //
        // Fold { init, list, func } accumulates over a length-prefixed list
        // by calling `func(acc, elem) → i64` for each element.
        //
        // WASM emission:
        //   1. Load list element count from header (offset 0).
        //   2. Initialise accumulator from `init` and counter to 0.
        //   3. Loop:
        //        a. If i >= count → break with current acc (result of block).
        //        b. Load element: list_ptr + 8 + i * 8.
        //        c. call_indirect(fold_reducer_type, table 0) with acc and elem.
        //        d. Update acc; increment i; continue.
        //
        // `func` is resolved as one of:
        //   • A top-level function name (in the `functions` map) — table index
        //     is `func_idx - function_offset`, pushed as `i32.const`.
        //   • A local I32 variable (closure env) — loads `fn_idx` (i64) from
        //     offset 0 of the env pointer, wraps to i32.
        //   • A local I64 variable — wraps directly to i32.
        //
        // Note: capture-free 2-param Lambdas are hoisted (Wave 12A) and
        // dispatch via the I64 path above.  Lambdas with captures still emit a
        // closure env (I32 pointer) whose fn_idx is a placeholder; the I32
        // path below traps at runtime.  General closure hoisting is deferred.
        AnfExpr::Fold { init, list, func } => {
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

        // ── Placeholder ───────────────────────────────────────────────────
        AnfExpr::Placeholder => {
            insns.push(Instruction::Unreachable);
            None
        }
    }
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
