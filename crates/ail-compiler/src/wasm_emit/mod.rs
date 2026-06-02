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
    EffectDataLayout, RESULT_BUFFER_MAX, binding_params, binding_result, effect_call_returns_bytes,
    export_name, infer_expr_type, record_layout_fields, well_known_variant_tag,
};

mod builder;
mod control;
mod expr;
mod int;
mod text;

pub(crate) use self::builder::build_code_section;

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
            // IDs 0 and 1 are reserved for well-known tags (None/Ok=0,
            // Some/Err=1).  User-defined tags start at 2 to prevent
            // collisions when a function mixes well-known and user tags.
            next_variant_tag: 2,
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
    /// context.  Well-known tags (`None`/`Ok`=0, `Some`/`Err`=1) are resolved
    /// directly and never consume the user-tag counter.  User-defined tags are
    /// assigned in first-encounter order starting at 2, so they can never
    /// collide with reserved IDs 0 or 1.
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

    fn bind_temp(&mut self, ty: ValType) -> u32 {
        let idx = self.next_local;
        self.next_local += 1;
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
    match expr::emit_anf_expr(expr, ctx, functions, insns) {
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
    let emitted_ty = expr::emit_anf_expr(expr, ctx, functions, insns);
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
        ("i64.and" | "and" | "int.bit_and" | "int_bit_and", 2) => insns.push(Instruction::I64And),
        ("i64.or" | "or" | "int.bit_or" | "int_bit_or", 2) => insns.push(Instruction::I64Or),
        ("i64.xor" | "xor" | "int.bit_xor" | "int_bit_xor", 2) => insns.push(Instruction::I64Xor),
        ("int.bit_not" | "int_bit_not", 1) => {
            insns.push(Instruction::I64Const(-1));
            insns.push(Instruction::I64Xor);
        }
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
