// ── ail-compiler::wasm ────────────────────────────────────────────────────
//
// WASM emission — the third and final pipeline stage.
//
// # Pre-condition
//
// `emit_wasm` MUST be called with an `AnfIr` produced by `lower_to_anf`.
// The `anf_ir_hash` field in `stage_hashes` must be `Some(...)`.
// If it is `None`, `Err(CompileError::EncodingError)` is returned.
//
// # What is emitted (G20 R2)
//
// Every `AnfBinding` becomes a WASM function with a real body emitted from
// the ANF IR.  The codegen uses WASM locals to represent ANF let-bindings:
//   - `AnfExpr::Literal(Int(n))` → `i64.const n` + `local.set`
//   - `AnfExpr::Literal(Bool(b))` → `i32.const 0|1` + `local.set`
//   - `AnfExpr::Literal(Float(f))` → `f64.const f`
//   - `AnfExpr::Literal(Text/Bytes/Unit)` → `i32.const 0` (opaque ref)
//   - `AnfExpr::Var(n)` → `local.get <index>`
//   - `AnfExpr::Call { func, args }` → `call <func_ref>` (host import)
//   - `AnfExpr::If` → `block/if/else/end`
//   - `AnfExpr::Let` → let-bind value, then emit body
//   - Effect/concurrency/runtime-check/resource variants → `unreachable`
//     (host-managed; emitting a trap stub signals "needs host dispatch")
//   - `AnfExpr::Placeholder` → `unreachable`
//
// An `AnfIr` with zero bindings produces a minimal valid WASM module
// (magic + version only — no sections).
//
// # Hash chain contract
//
// `wasm_hash = blake3(anf_ir_hash || wasm_binary)`
//
// # Determinism contract
//
// `BTreeMap` for provenance.  `stable_cbor_bytes` + BLAKE3 for hashing.
// Same `AnfIr` → byte-identical `WasmArtifact` across any number of calls.
//
// # What this stage does NOT do
//
// - No optimization.
// - No runtime / Wasmtime dependency.
// - Host capability calls are emitted as `unreachable` (host dispatch stubs).

use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, GlobalSection, GlobalType, ImportSection, Instruction,
    MemorySection, MemoryType, Module, TypeSection, ValType,
};

use crate::anf::{AnfExpr, AnfIr, SourceMap, SourceMapEntry};
use crate::artifact_manifest::ArtifactManifest;
use crate::core_ir::{LiteralValue, StageHashes};
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};

// ── WasmArtifact ─────────────────────────────────────────────────────────

/// Output of the third pipeline stage: a valid WASM binary with provenance
/// and a fully sealed hash chain.
///
/// In Phase 7, every function body is a `[unreachable, end]` stub.
/// Expression lowering is deferred to Phase 8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmArtifact {
    /// Encoded WASM binary; passes `wasmparser::validate` structural checks.
    pub wasm: Vec<u8>,
    /// Semantic source map with `wasm_offset` populated for every binding.
    ///
    /// One entry per `AnfBinding` in binding order.  `native_offset` is always
    /// `None` in WASM artifacts (populated only by `emit_native`).
    pub source_map: SourceMap,
    /// Maps each `NodeRef` from the source graph to its byte offset in the
    /// WASM code section (i.e., the position of the body-size LEB128 byte
    /// for that function's entry in the encoded binary).
    /// Kept as a derived compatibility index; prefer `source_map` for new code.
    /// Empty when the input `AnfIr` has no bindings.
    pub provenance: BTreeMap<NodeRef, u32>,
    /// Hash chain extended through the WASM stage.
    /// `hash_chain.wasm_hash` is `Some(...)` after `emit_wasm` completes.
    /// `hash_chain.source_map_hash` is `Some(...)` after `emit_wasm` completes.
    /// `hash_chain.artifact_manifest_hash` is `Some(...)` after `emit_wasm`.
    pub hash_chain: StageHashes,
    /// Profile-bound artifact manifest for this WASM artifact.
    ///
    /// Can be serialized as `program.artifact.json` by callers.
    /// Includes the full hash chain and compiler version.
    pub artifact_manifest: ArtifactManifest,
    /// JSON-serialized `SourceMap` — content for `program.source_map.json`.
    ///
    /// Callers write this to disk as the source-map sidecar for debugging,
    /// profiling, and runtime error mapping.
    pub source_map_json: Vec<u8>,
    /// JSON-serialized `ArtifactManifest` — content for `program.artifact.json`.
    ///
    /// Callers write this to disk as the artifact metadata sidecar.
    pub artifact_manifest_json: Vec<u8>,
}

// ── build_type_section ────────────────────────────────────────────────────

/// Build a type section with one entry per function signature.
///
/// Returns `None` when `n_functions == 0` — no type section is needed for
/// an empty module.
fn build_type_section(signatures: &[WasmSignature]) -> Option<TypeSection> {
    if signatures.is_empty() {
        return None;
    }
    let mut types = TypeSection::new();
    for signature in signatures {
        let params = vec![ValType::I64; signature.param_count];
        if signature.result.is_some() {
            types.ty().function(params, [ValType::I64]);
        } else {
            types.ty().function(params, []);
        }
    }
    Some(types)
}

fn build_type_section_with_host_call(signatures: &[WasmSignature]) -> TypeSection {
    let mut types = TypeSection::new();
    types.ty().function(
        [
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        [ValType::I64],
    );
    for signature in signatures {
        let params = vec![ValType::I64; signature.param_count];
        if signature.result.is_some() {
            types.ty().function(params, [ValType::I64]);
        } else {
            types.ty().function(params, []);
        }
    }
    types
}

fn literal_type(lit: &LiteralValue) -> ValType {
    match lit {
        LiteralValue::Int(_) | LiteralValue::Bool(_) => ValType::I64,
        LiteralValue::Text(_) | LiteralValue::Unit => ValType::I32,
        LiteralValue::Float(_) => ValType::F64,
    }
}

fn infer_expr_type(expr: &AnfExpr, locals: &mut Vec<(String, ValType)>) -> Option<ValType> {
    match expr {
        AnfExpr::Literal(lit) => Some(literal_type(lit)),
        AnfExpr::Var(name) => locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, ty)| *ty),
        AnfExpr::Let { name, value, body } => {
            let value_ty = infer_expr_type(value, locals).unwrap_or(ValType::I32);
            locals.push((name.clone(), value_ty));
            let body_ty = infer_expr_type(body, locals);
            locals.pop();
            body_ty
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            let then_ty = infer_expr_type(then_branch, locals);
            let else_ty = infer_expr_type(else_branch, locals);
            if then_ty == else_ty { then_ty } else { None }
        }
        AnfExpr::Match { arms, .. } => {
            let first_ty = arms
                .first()
                .and_then(|arm| infer_expr_type(&arm.body, locals));
            if arms
                .iter()
                .all(|arm| infer_expr_type(&arm.body, locals) == first_ty)
            {
                first_ty
            } else {
                None
            }
        }
        AnfExpr::Return(inner) => infer_expr_type(inner, locals),
        AnfExpr::ShortCircuitAnd { .. } | AnfExpr::ShortCircuitOr { .. } => Some(ValType::I64),
        AnfExpr::Loop { body } => infer_expr_type(body, locals),
        AnfExpr::Break { value } => infer_expr_type(value, locals),
        AnfExpr::Continue | AnfExpr::WhileLoop { .. } => None,
        AnfExpr::RecordNew { .. }
        | AnfExpr::TupleNew(_)
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::Lambda { .. }
        | AnfExpr::Seq(_) => Some(ValType::I32),
        AnfExpr::FieldGet { .. } | AnfExpr::Call { .. } => Some(ValType::I64),
        AnfExpr::EffectCall { .. } => Some(ValType::I64),
        AnfExpr::Placeholder
        | AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Select { .. }
        | AnfExpr::Timeout { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. } => None,
        AnfExpr::FieldUpdate { value, .. } => infer_expr_type(value, locals).or(Some(ValType::I32)),
    }
}

// ── build_function_section ────────────────────────────────────────────────

/// Build a function section referencing type index 0 for every function.
///
/// Returns `None` when `n_functions == 0`.
fn binding_result(binding: &crate::anf::AnfBinding) -> Option<ValType> {
    let mut locals = binding_params(binding)
        .into_iter()
        .map(|name| (name.to_string(), ValType::I64))
        .collect();
    infer_expr_type(&binding.expr, &mut locals).filter(|ty| *ty == ValType::I64)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WasmSignature {
    param_count: usize,
    result: Option<ValType>,
}

fn build_function_section(
    signatures: &[WasmSignature],
    type_offset: u32,
) -> Option<FunctionSection> {
    if signatures.is_empty() {
        return None;
    }
    let mut functions = FunctionSection::new();
    for (type_idx, _) in signatures.iter().enumerate() {
        functions.function(type_offset + type_idx as u32);
    }
    Some(functions)
}

fn export_name(binding_name: &str) -> String {
    let local = binding_name.rsplit('.').next().unwrap_or(binding_name);
    local
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn build_export_section(
    bindings: &[crate::anf::AnfBinding],
    function_offset: u32,
) -> Option<ExportSection> {
    let mut exports = ExportSection::new();
    let mut count = 0usize;
    for (idx, binding) in bindings.iter().enumerate() {
        if binding_result(binding).is_some() {
            exports.export(
                &export_name(&binding.name),
                ExportKind::Func,
                function_offset + idx as u32,
            );
            count += 1;
        }
    }
    (count > 0).then_some(exports)
}

fn build_export_section_with_memory(
    bindings: &[crate::anf::AnfBinding],
    function_offset: u32,
    export_memory: bool,
) -> Option<ExportSection> {
    let mut exports =
        build_export_section(bindings, function_offset).unwrap_or_else(ExportSection::new);
    let mut count = usize::from(export_memory);
    if export_memory {
        exports.export("memory", ExportKind::Memory, 0);
    }
    count += bindings
        .iter()
        .filter(|binding| binding_result(binding).is_some())
        .count();
    (count > 0).then_some(exports)
}

fn function_index(
    bindings: &[crate::anf::AnfBinding],
    function_offset: u32,
) -> BTreeMap<String, u32> {
    let mut functions = BTreeMap::new();
    for (idx, binding) in bindings.iter().enumerate() {
        functions.insert(binding.name.clone(), function_offset + idx as u32);
        functions.insert(export_name(&binding.name), function_offset + idx as u32);
    }
    functions
}

fn collect_free_vars<'a>(expr: &'a AnfExpr, bound: &mut Vec<&'a str>, out: &mut Vec<&'a str>) {
    match expr {
        AnfExpr::Var(name) => {
            if !bound.iter().rev().any(|bound_name| *bound_name == name)
                && !out.iter().any(|existing| *existing == name)
            {
                out.push(name);
            }
        }
        AnfExpr::Let { name, value, body } => {
            collect_free_vars(value, bound, out);
            bound.push(name);
            collect_free_vars(body, bound, out);
            bound.pop();
        }
        AnfExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if !bound.iter().rev().any(|bound_name| *bound_name == cond)
                && !out.iter().any(|existing| *existing == cond)
            {
                out.push(cond);
            }
            collect_free_vars(then_branch, bound, out);
            collect_free_vars(else_branch, bound, out);
        }
        AnfExpr::Call { args, .. } => {
            for arg in args {
                if !bound.iter().rev().any(|bound_name| *bound_name == arg)
                    && !out.iter().any(|existing| *existing == arg)
                {
                    out.push(arg);
                }
            }
        }
        AnfExpr::Return(inner)
        | AnfExpr::ShortCircuitAnd { right: inner, .. }
        | AnfExpr::ShortCircuitOr { right: inner, .. }
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::FieldUpdate { value: inner, .. } => collect_free_vars(inner, bound, out),
        AnfExpr::WhileLoop { cond, body } => {
            if !bound.iter().rev().any(|bound_name| *bound_name == cond)
                && !out.iter().any(|existing| *existing == cond)
            {
                out.push(cond);
            }
            collect_free_vars(body, bound, out);
        }
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            for expr in exprs {
                collect_free_vars(expr, bound, out);
            }
        }
        AnfExpr::Match { arms, .. } => {
            for arm in arms {
                collect_free_vars(&arm.body, bound, out);
            }
        }
        AnfExpr::Lambda { params, body } => {
            let original_len = bound.len();
            bound.extend(params.iter().map(String::as_str));
            collect_free_vars(body, bound, out);
            bound.truncate(original_len);
        }
        AnfExpr::RecordNew { fields } => {
            for (_, expr) in fields {
                collect_free_vars(expr, bound, out);
            }
        }
        AnfExpr::VariantNew { payload, .. } => {
            if let Some(payload) = payload {
                collect_free_vars(payload, bound, out);
            }
        }
        _ => {}
    }
}

fn binding_params(binding: &crate::anf::AnfBinding) -> Vec<&str> {
    let mut params = Vec::new();
    collect_free_vars(&binding.expr, &mut Vec::new(), &mut params);
    params
}

fn binding_signatures(bindings: &[crate::anf::AnfBinding]) -> Vec<WasmSignature> {
    bindings
        .iter()
        .map(|binding| WasmSignature {
            param_count: binding_params(binding).len(),
            result: binding_result(binding),
        })
        .collect()
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelKind {
    Other,
    LoopBreak,
    LoopContinue,
}

impl<'a> WasmCodegenCtx<'a> {
    fn new(params: Vec<&'a str>, effect_data: &'a EffectDataLayout) -> Self {
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
}

#[derive(Clone, Debug, Default)]
struct EffectDataLayout {
    strings: BTreeMap<String, (i32, i32)>,
    next_offset: i32,
    args_offset: i32,
    needs_host_call: bool,
    needs_memory: bool,
}

impl EffectDataLayout {
    fn for_bindings(bindings: &[crate::anf::AnfBinding]) -> Self {
        let mut layout = Self::default();
        for binding in bindings {
            layout.collect_expr(&binding.expr);
        }
        if layout.needs_host_call {
            layout.args_offset = layout.next_offset.max(1);
        }
        layout
    }

    fn collect_expr(&mut self, expr: &AnfExpr) {
        match expr {
            AnfExpr::EffectCall {
                capability, func, ..
            } => {
                self.needs_host_call = true;
                self.intern(capability);
                self.intern(func);
            }
            AnfExpr::Let { value, body, .. } => {
                self.collect_expr(value);
                self.collect_expr(body);
            }
            AnfExpr::FieldGet { .. } => {
                self.needs_memory = true;
            }
            AnfExpr::FieldUpdate { value, .. } => {
                self.needs_memory = true;
                self.collect_expr(value);
            }
            AnfExpr::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_expr(then_branch);
                self.collect_expr(else_branch);
            }
            AnfExpr::Return(inner)
            | AnfExpr::ShortCircuitAnd { right: inner, .. }
            | AnfExpr::ShortCircuitOr { right: inner, .. }
            | AnfExpr::Loop { body: inner }
            | AnfExpr::Break { value: inner } => self.collect_expr(inner),
            AnfExpr::WhileLoop { body, .. } => self.collect_expr(body),
            AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
                if !matches!(expr, AnfExpr::Seq(_)) {
                    self.needs_memory = true;
                }
                for expr in exprs {
                    self.collect_expr(expr);
                }
            }
            AnfExpr::Match { arms, .. } => {
                for arm in arms {
                    self.collect_expr(&arm.body);
                }
            }
            AnfExpr::Lambda { body, .. } => self.collect_expr(body),
            AnfExpr::RecordNew { fields } => {
                self.needs_memory = true;
                for (_, expr) in fields {
                    self.collect_expr(expr);
                }
            }
            AnfExpr::VariantNew { payload, .. } => {
                self.needs_memory = true;
                if let Some(payload) = payload {
                    self.collect_expr(payload);
                }
            }
            _ => {}
        }
    }

    fn intern(&mut self, value: &str) {
        if self.strings.contains_key(value) {
            return;
        }
        let ptr = self.next_offset;
        let len = value.len() as i32;
        self.strings.insert(value.to_string(), (ptr, len));
        self.next_offset += len.max(1);
    }

    fn string(&self, value: &str) -> (i32, i32) {
        self.strings[value]
    }
}

fn build_import_section(needs_host_call: bool) -> Option<ImportSection> {
    if !needs_host_call {
        return None;
    }
    let mut imports = ImportSection::new();
    imports.import("ail", "host_call", EntityType::Function(0));
    Some(imports)
}

fn build_memory_section(needs_memory: bool) -> Option<MemorySection> {
    if !needs_memory {
        return None;
    }
    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    Some(memories)
}

fn build_global_section(needs_memory: bool, heap_start: i32) -> Option<GlobalSection> {
    if !needs_memory {
        return None;
    }
    let mut globals = GlobalSection::new();
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(heap_start),
    );
    Some(globals)
}

fn align_to_i64(offset: i32) -> i32 {
    let offset = offset.max(8);
    ((offset + 7) / 8) * 8
}

fn build_data_section(layout: &EffectDataLayout) -> Option<DataSection> {
    if !layout.needs_host_call {
        return None;
    }
    let mut data = DataSection::new();
    for (value, (ptr, _)) in &layout.strings {
        data.active(
            0,
            &ConstExpr::i32_const(*ptr),
            value.as_bytes().iter().copied(),
        );
    }
    Some(data)
}

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
        if rest.is_empty() {
            return emit_branch_expr(&first.body, result_ty, ctx, functions, insns);
        }
        return emit_match_arms(
            scrutinee,
            scrutinee_ty,
            rest,
            result_ty,
            ctx,
            functions,
            insns,
        );
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
            // Text, Unit → opaque i32(0) placeholder (no runtime value).
            LiteralValue::Text(_) | LiteralValue::Unit => {
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
            if let AnfExpr::RecordNew { fields } = value.as_ref() {
                ctx.bind_record_layout(
                    name,
                    fields.iter().map(|(field, _)| field.clone()).collect(),
                );
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
            None
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
                if let Some((local_idx, _)) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(local_idx));
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
            insns.push(Instruction::Call(0));
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
            let tag_byte = tag
                .as_bytes()
                .iter()
                .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
            insns.push(Instruction::I32Const(i32::from(tag_byte)));
            insns.push(Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
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

        // ── Lambda ───────────────────────────────────────────────────────
        // Lambdas are hoisted to top-level WASM functions.
        // At this stage, emit an opaque function reference (i32.const 0).
        AnfExpr::Lambda { .. } => {
            insns.push(Instruction::I32Const(0));
            Some(ValType::I32)
        }

        // ── Effect/concurrent/resource variants ───────────────────────────
        // These are host-managed. The WASM body emits unreachable to signal
        // that the host runtime must intercept and dispatch.
        AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::TaskGroup { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Select { .. }
        | AnfExpr::Timeout { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. } => {
            insns.push(Instruction::Unreachable);
            None
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
/// Returns `None` when `bindings` is empty.
fn build_code_section(
    bindings: &[crate::anf::AnfBinding],
    effect_data: &EffectDataLayout,
    function_offset: u32,
) -> Option<CodeSection> {
    if bindings.is_empty() {
        return None;
    }
    let mut codes = CodeSection::new();
    let functions = function_index(bindings, function_offset);
    for binding in bindings {
        let params = binding_params(binding);
        let mut ctx = WasmCodegenCtx::new(params, effect_data);
        let mut insns: Vec<Instruction<'_>> = Vec::new();

        let emitted_ty = emit_anf_expr(&binding.expr, &mut ctx, &functions, &mut insns);

        if binding_result(binding).is_none() && emitted_ty.is_some() {
            insns.push(Instruction::Drop);
        }
        insns.push(Instruction::End);

        // Allocate locals: one i64 slot per let-binding (conservative).
        // In production we'd type-infer; here we use i32 as a safe default.
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
    Some(codes)
}

// ── leb128_u32 ────────────────────────────────────────────────────────────

/// Decode one LEB128-encoded unsigned 32-bit integer from `bytes`.
///
/// Returns `(value, bytes_consumed)`.  Panics if `bytes` is empty or the
/// encoding exceeds 5 bytes (which cannot happen for a valid WASM binary).
fn leb128_u32(bytes: &[u8]) -> (u32, usize) {
    let mut result = 0u32;
    let mut shift = 0u32;
    let mut n = 0usize;
    for &b in bytes {
        result |= u32::from(b & 0x7f) << shift;
        shift += 7;
        n += 1;
        if b & 0x80 == 0 {
            break;
        }
    }
    (result, n)
}

// ── code_entry_offsets ────────────────────────────────────────────────────

/// Scan `wasm` for the code section (section id 10) and return the absolute
/// byte offset of each code-entry header (the LEB128-encoded body-size
/// prefix) in function order.
///
/// Returns an empty `Vec` when the module contains no code section.
fn code_entry_offsets(wasm: &[u8]) -> Vec<u32> {
    const HEADER_LEN: usize = 8; // 4-byte magic + 4-byte version
    let mut pos = HEADER_LEN;

    while pos < wasm.len() {
        let section_id = wasm[pos];
        pos += 1;

        let (section_size, leb_len) = leb128_u32(&wasm[pos..]);
        let content_start = pos + leb_len;
        pos = content_start + section_size as usize;

        if section_id == 10 {
            // Code section: content = LEB128(count) + entries
            let (count, count_len) = leb128_u32(&wasm[content_start..]);
            let mut entry_pos = content_start + count_len;
            let mut offsets = Vec::with_capacity(count as usize);

            for _ in 0..count {
                offsets.push(entry_pos as u32);
                let (entry_size, entry_size_len) = leb128_u32(&wasm[entry_pos..]);
                entry_pos += entry_size_len + entry_size as usize;
            }

            return offsets;
        }
    }

    Vec::new()
}

// ── emit_wasm ─────────────────────────────────────────────────────────────

/// Emit a structurally valid WASM module from an `AnfIr`.
///
/// # Pre-conditions
///
/// - `anf.stage_hashes.anf_ir_hash` must be `Some(...)`.  Call
///   `lower_to_anf` before `emit_wasm`.
///
/// # Hash chain
///
/// Extends the chain: `wasm_hash = blake3(anf_ir_hash || wasm_binary)`.
///
/// # Errors
///
/// - `CompileError::EncodingError` — `anf_ir_hash` is `None` (pre-condition
///   violated) or WASM binary assembly failed.
pub fn emit_wasm(anf: &AnfIr) -> Result<WasmArtifact, CompileError> {
    emit_wasm_with_profile(anf, "unspecified")
}

/// Emit a WASM module and bind the artifact manifest to `profile`.
pub fn emit_wasm_with_profile(anf: &AnfIr, profile: &str) -> Result<WasmArtifact, CompileError> {
    // Gate: anf_ir_hash must be sealed.
    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .ok_or_else(|| CompileError::EncodingError("anf_ir_hash not sealed".to_string()))?;

    let signatures = binding_signatures(&anf.bindings);
    let effect_data = EffectDataLayout::for_bindings(&anf.bindings);
    let needs_host_call = effect_data.needs_host_call;
    let needs_memory = effect_data.needs_host_call || effect_data.needs_memory;
    let type_offset = u32::from(needs_host_call);
    let function_offset = u32::from(needs_host_call);

    // Assemble WASM module first so we can compute byte offsets.
    let mut module = Module::new();
    if needs_host_call {
        module.section(&build_type_section_with_host_call(&signatures));
    } else if let Some(types) = build_type_section(&signatures) {
        module.section(&types);
    }
    if let Some(imports) = build_import_section(needs_host_call) {
        module.section(&imports);
    }
    if let Some(functions) = build_function_section(&signatures, type_offset) {
        module.section(&functions);
    }
    if let Some(memory) = build_memory_section(needs_memory) {
        module.section(&memory);
    }
    if let Some(globals) = build_global_section(needs_memory, align_to_i64(effect_data.next_offset))
    {
        module.section(&globals);
    }
    if let Some(exports) =
        build_export_section_with_memory(&anf.bindings, function_offset, needs_memory)
    {
        module.section(&exports);
    }
    if let Some(codes) = build_code_section(&anf.bindings, &effect_data, function_offset) {
        module.section(&codes);
    }
    if let Some(data) = build_data_section(&effect_data) {
        module.section(&data);
    }
    let wasm = module.finish();

    // Build provenance map: NodeRef → WASM byte offset of the code entry.
    // `code_entry_offsets` scans the binary and returns the position of each
    // function's LEB128-encoded body-size prefix in the code section.
    let entry_offsets = code_entry_offsets(&wasm);
    let provenance: BTreeMap<NodeRef, u32> = anf
        .bindings
        .iter()
        .zip(entry_offsets.iter())
        .map(|(b, &offset)| (b.source_ref, offset))
        .collect();

    // Build semantic source map — clone ANF source map and populate wasm_offset.
    // Each binding entry gets wasm_offset = the byte offset from code_entry_offsets.
    let source_map_entries: Vec<SourceMapEntry> = anf
        .source_map
        .entries
        .iter()
        .zip(
            // Zip with entry_offsets; pad with None if offsets are shorter.
            entry_offsets
                .iter()
                .map(|&o| Some(o))
                .chain(std::iter::repeat(None)),
        )
        .map(|(entry, wasm_offset)| SourceMapEntry {
            wasm_offset,
            ..entry.clone()
        })
        .collect();
    let source_map = SourceMap {
        entries: source_map_entries,
    };

    // Seal: source_map_hash = blake3(source_map_cbor_bytes).
    let source_map_bytes = stable_cbor_bytes(&source_map)?;
    let source_map_hash = hash_with_parent(&[], &source_map_bytes);

    // Seal: wasm_hash = blake3(anf_ir_hash || wasm_binary).
    let wasm_hash = hash_with_parent(&anf_ir_hash, &wasm);

    // Extend the stage hashes from ANF.
    let mut hash_chain = anf.stage_hashes.clone();
    hash_chain.wasm_hash = Some(wasm_hash);
    hash_chain.source_map_hash = Some(source_map_hash);

    // Build ArtifactManifest from the complete hash chain.
    let capabilities_manifest_bytes = stable_cbor_bytes(&anf.bindings)?;
    let capabilities_manifest_hash = hash_with_parent(&[], &capabilities_manifest_bytes);
    let artifact_manifest = ArtifactManifest {
        profile: profile.to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        graph_snapshot_hash: hash_chain.graph_snapshot_hash,
        verification_report_hash: hash_chain.verification_report_hash,
        core_ir_hash: hash_chain.core_ir_hash,
        anf_ir_hash,
        wasm_hash: Some(wasm_hash),
        native_hash: None,
        source_map_hash: Some(source_map_hash),
        capabilities_manifest_hash: Some(capabilities_manifest_hash),
    };

    // Seal: artifact_manifest_hash = blake3(manifest_cbor_bytes).
    let manifest_cbor = stable_cbor_bytes(&artifact_manifest)?;
    let artifact_manifest_hash = hash_with_parent(&[], &manifest_cbor);
    hash_chain.artifact_manifest_hash = Some(artifact_manifest_hash);

    // Serialize JSON sidecars.
    // `program.source_map.json` — semantic source map for debugging/profiling.
    let source_map_json = serde_json::to_vec(&source_map)
        .map_err(|e| CompileError::EncodingError(format!("source_map JSON encode: {e}")))?;
    // `program.artifact.json` — profile-bound artifact manifest.
    let artifact_manifest_json = serde_json::to_vec(&artifact_manifest)
        .map_err(|e| CompileError::EncodingError(format!("artifact_manifest JSON encode: {e}")))?;

    Ok(WasmArtifact {
        wasm,
        source_map,
        provenance,
        hash_chain,
        artifact_manifest,
        source_map_json,
        artifact_manifest_json,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_verify::report::VerificationReport;

    use super::*;
    use crate::lower::{lower_to_anf, lower_to_core_ir};

    fn proven_report() -> VerificationReport {
        VerificationReport {
            entries: vec![],
            ..Default::default()
        }
    }

    fn anf_for_n(n: usize) -> AnfIr {
        let graph = SemanticGraph {
            nodes: (0..n)
                .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
                .collect(),
            edges: vec![],
        };
        let core = lower_to_core_ir(&graph, &proven_report()).unwrap();
        lower_to_anf(&core).unwrap()
    }

    // Task 3.3 inline unit tests ──────────────────────────────────────────

    // Scenario: anf_ir_hash None → EncodingError.
    // Proves the pre-condition gate fires correctly.
    #[test]
    fn emit_wasm_rejects_unsealed_anf_ir_hash() {
        let anf = AnfIr {
            schema_version: crate::anf::ANF_SCHEMA_VERSION,
            bindings: vec![],
            source_map: crate::anf::SourceMap { entries: vec![] },
            stage_hashes: crate::core_ir::StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: None, // unsealed
                wasm_hash: None,
                native_hash: None,
                source_map_hash: None,
                artifact_manifest_hash: None,
            },
        };
        let result = emit_wasm(&anf);
        assert!(
            matches!(result, Err(CompileError::EncodingError(_))),
            "expected EncodingError for unsealed anf_ir_hash, got {result:?}"
        );
    }

    // Scenario: wasm_hash is sealed after emit_wasm.
    #[test]
    fn emit_wasm_seals_wasm_hash() {
        let anf = anf_for_n(1);
        let artifact = emit_wasm(&anf).unwrap();
        assert!(
            artifact.hash_chain.wasm_hash.is_some(),
            "wasm_hash must be Some after emit_wasm"
        );
    }

    // TRIANGULATE: different inputs produce different wasm hashes.
    #[test]
    fn different_anf_produces_different_wasm_hash() {
        let a1 = emit_wasm(&anf_for_n(1)).unwrap();
        let a2 = emit_wasm(&anf_for_n(2)).unwrap();
        assert_ne!(
            a1.hash_chain.wasm_hash, a2.hash_chain.wasm_hash,
            "different AnfIr inputs must produce different wasm_hashes"
        );
    }

    // Scenario: build_type_section returns None for 0 functions.
    #[test]
    fn build_type_section_none_for_zero() {
        assert!(build_type_section(&[]).is_none());
    }

    // TRIANGULATE: build_type_section returns Some for N > 0.
    #[test]
    fn build_type_section_some_for_nonzero() {
        let signature = WasmSignature {
            param_count: 0,
            result: None,
        };
        assert!(build_type_section(std::slice::from_ref(&signature)).is_some());
        assert!(build_type_section(&vec![signature; 5]).is_some());
    }

    fn sealed_anf(bindings: Vec<crate::anf::AnfBinding>) -> AnfIr {
        AnfIr {
            schema_version: crate::anf::ANF_SCHEMA_VERSION,
            source_map: SourceMap::from_bindings(&bindings),
            bindings,
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: Some([2u8; 32]),
                wasm_hash: None,
                native_hash: None,
                source_map_hash: None,
                artifact_manifest_hash: None,
            },
        }
    }

    #[test]
    fn emit_wasm_call_uses_resolved_function_index() {
        use crate::anf::AnfBinding;
        use wasmparser::{Operator, Parser, Payload};

        let anf = sealed_anf(vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "fn.answer".to_string(),
                expr: AnfExpr::Literal(LiteralValue::Int(42)),
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "fn.main".to_string(),
                expr: AnfExpr::Call {
                    func: "answer".to_string(),
                    args: vec![],
                },
            },
        ]);

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("wasm must validate");

        let mut saw_call_answer = false;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if matches!(reader.read().unwrap(), Operator::Call { function_index: 0 }) {
                        saw_call_answer = true;
                    }
                }
            }
        }

        assert!(saw_call_answer, "expected fn.main to call function index 0");
    }

    #[test]
    fn emit_wasm_single_arg_call_emits_i64_add_and_call() {
        use crate::anf::AnfBinding;
        use wasmparser::{Operator, Parser, Payload};

        let anf = sealed_anf(vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "fn.double".to_string(),
                expr: AnfExpr::Call {
                    func: "i64.add".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "fn.main".to_string(),
                expr: AnfExpr::Let {
                    name: "n".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(21))),
                    body: Box::new(AnfExpr::Call {
                        func: "double".to_string(),
                        args: vec!["n".to_string()],
                    }),
                },
            },
        ]);

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("wasm must validate");

        let mut saw_i64_add = false;
        let mut saw_call_double = false;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    match reader.read().unwrap() {
                        Operator::I64Add => saw_i64_add = true,
                        Operator::Call { function_index: 0 } => saw_call_double = true,
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_i64_add, "expected double to use i64.add");
        assert!(saw_call_double, "expected main to call double");
    }

    #[test]
    fn emit_wasm_multi_arg_call_emits_call() {
        use crate::anf::AnfBinding;
        use wasmparser::{Operator, Parser, Payload};

        let anf = sealed_anf(vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "fn.sum".to_string(),
                expr: AnfExpr::Call {
                    func: "i64.add".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                },
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "fn.main".to_string(),
                expr: AnfExpr::Let {
                    name: "a".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
                    body: Box::new(AnfExpr::Let {
                        name: "b".to_string(),
                        value: Box::new(AnfExpr::Literal(LiteralValue::Int(22))),
                        body: Box::new(AnfExpr::Call {
                            func: "sum".to_string(),
                            args: vec!["a".to_string(), "b".to_string()],
                        }),
                    }),
                },
            },
        ]);

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("wasm must validate");

        let mut saw_call_sum = false;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if matches!(reader.read().unwrap(), Operator::Call { function_index: 0 }) {
                        saw_call_sum = true;
                    }
                }
            }
        }

        assert!(saw_call_sum, "expected main to call sum");
    }

    #[test]
    fn emit_wasm_recursive_call_validates() {
        use crate::anf::AnfBinding;
        use wasmparser::{Operator, Parser, Payload};

        let anf = sealed_anf(vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.recur".to_string(),
            expr: AnfExpr::Call {
                func: "recur".to_string(),
                args: vec!["n".to_string()],
            },
        }]);

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("recursive call module must validate");

        let mut saw_self_call = false;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if matches!(reader.read().unwrap(), Operator::Call { function_index: 0 }) {
                        saw_self_call = true;
                    }
                }
            }
        }

        assert!(
            saw_self_call,
            "recursive call should target its own function index"
        );
    }

    #[test]
    fn emit_wasm_exports_literal_function_name() {
        use crate::anf::AnfBinding;
        use wasmparser::{ExternalKind, Parser, Payload};

        let binding = AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.answer".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(42)),
        };
        let anf = AnfIr {
            schema_version: crate::anf::ANF_SCHEMA_VERSION,
            bindings: vec![binding.clone()],
            source_map: SourceMap::from_bindings(&[binding]),
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: Some([2u8; 32]),
                wasm_hash: None,
                native_hash: None,
                source_map_hash: None,
                artifact_manifest_hash: None,
            },
        };

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("wasm must validate");

        let mut found = false;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            if let Payload::ExportSection(exports) = payload.unwrap() {
                for export in exports {
                    let export = export.unwrap();
                    if export.name == "answer" && export.kind == ExternalKind::Func {
                        found = true;
                    }
                }
            }
        }

        assert!(found, "expected function export named answer");
    }

    #[test]
    fn emit_wasm_record_new_and_field_get_use_linear_memory() {
        use crate::anf::AnfBinding;
        use wasmparser::{Operator, Parser, Payload};

        let anf = sealed_anf(vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.main".to_string(),
            expr: AnfExpr::Let {
                name: "rec".to_string(),
                value: Box::new(AnfExpr::RecordNew {
                    fields: vec![
                        ("a".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                        ("b".to_string(), AnfExpr::Literal(LiteralValue::Int(32))),
                    ],
                }),
                body: Box::new(AnfExpr::FieldGet {
                    record: "rec".to_string(),
                    field: "b".to_string(),
                }),
            },
        }]);

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("wasm must validate");

        let mut saw_memory = false;
        let mut saw_store_b = false;
        let mut saw_load_b = false;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            match payload.unwrap() {
                Payload::MemorySection(_) => saw_memory = true,
                Payload::CodeSectionEntry(body) => {
                    let mut reader = body.get_operators_reader().unwrap();
                    while !reader.eof() {
                        match reader.read().unwrap() {
                            Operator::I64Store { memarg } if memarg.offset == 8 => {
                                saw_store_b = true
                            }
                            Operator::I64Load { memarg } if memarg.offset == 8 => saw_load_b = true,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        assert!(saw_memory, "record codegen must declare linear memory");
        assert!(
            saw_store_b,
            "record construction must store field b at offset 8"
        );
        assert!(saw_load_b, "field get must load field b from offset 8");
    }

    #[test]
    fn emit_wasm_tuple_list_variant_constructors_store_payloads() {
        use crate::anf::AnfBinding;
        use wasmparser::{Operator, Parser, Payload};

        let anf = sealed_anf(vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "fn.tuple".to_string(),
                expr: AnfExpr::TupleNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(1)),
                    AnfExpr::Literal(LiteralValue::Int(2)),
                ]),
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "fn.list".to_string(),
                expr: AnfExpr::ListNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(3)),
                    AnfExpr::Literal(LiteralValue::Int(4)),
                ]),
            },
            AnfBinding {
                source_ref: NodeRef(2),
                name: "fn.variant".to_string(),
                expr: AnfExpr::VariantNew {
                    tag: "Some".to_string(),
                    payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(5)))),
                },
            },
        ]);

        let artifact = emit_wasm(&anf).unwrap();
        wasmparser::validate(&artifact.wasm).expect("wasm must validate");

        let mut saw_tag_store = false;
        let mut i64_store_count = 0usize;
        for payload in Parser::new(0).parse_all(&artifact.wasm) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    match reader.read().unwrap() {
                        Operator::I32Store8 { .. } => saw_tag_store = true,
                        Operator::I64Store { .. } => i64_store_count += 1,
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_tag_store, "variant construction must store a tag byte");
        assert!(
            i64_store_count >= 6,
            "tuple/list/variant constructors must store i64 payloads"
        );
    }
}
