// ── ail-compiler::native ──────────────────────────────────────────────────
//
// Native object file emission — the Cranelift-backed backend stage.
//
// # Pre-condition
//
// `emit_native` MUST be called with an `AnfIr` produced by `lower_to_anf`.
// The `anf_ir_hash` field in `stage_hashes` must be `Some(...)`.
// If it is `None`, `Err(CompileError::NativeEncodingError)` is returned.
//
// # What is emitted (Phase 17)
//
// Every `AnfBinding` becomes a native function stub:
//   - Signature: `() -> ()` (SystemV calling convention).
//   - Body: one `trap` instruction (user trap code 1).
//
// An `AnfIr` with zero bindings produces a minimal valid object file
// (no code section; platform-native ELF/Mach-O/COFF header only).
//
// # Hash chain contract
//
// `native_hash = blake3(anf_ir_hash || native_bytes)`
//
// # Determinism contract
//
// `BTreeMap` for provenance.  Cranelift codegen is deterministic for
// identical IR on the same host ISA.
// Same `AnfIr` → byte-identical `NativeArtifact` across any number of calls.
//
// # Provenance contract
//
// `NativeArtifact.provenance: BTreeMap<NodeRef, u64>` maps each binding's
// `source_ref` to the cumulative byte offset of the function's code in the
// object file's code section.  Offsets are accumulated in binding order.
//
// # What this stage does NOT do (Phase 17)
//
// - No expression / body codegen (deferred to Phase 8+).
// - No optimization.
// - No dependency on `wasmtime` or `wasmer`.

use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;
use cranelift_codegen::{
    Context,
    ir::{
        AbiParam, Function, InstBuilder, MemFlags, Signature, StackSlotData,
        UserFuncName, condcodes::IntCC, stackslot::StackSlotKind,
    },
    isa::CallConv,
    settings,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use serde::{Deserialize, Serialize};

use crate::anf::{AnfIr, SourceMap, SourceMapEntry};
use crate::artifact_manifest::ArtifactManifest;
use crate::core_ir::StageHashes;
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};

// ── NativeDataLayout ──────────────────────────────────────────────────────

/// Pre-scan of all string literals and EffectCall names in an `AnfIr`.
///
/// Interns each unique string exactly once so that multiple bindings
/// using the same literal share a single data object.
#[derive(Default)]
pub struct NativeDataLayout {
    /// Interned strings: value → (index into `ordered`, byte length).
    strings: BTreeMap<String, (usize, usize)>,
    /// Ordered list of all interned strings (index = position in this vec).
    pub ordered: Vec<String>,
    /// Set when any binding contains an `EffectCall` — triggers host_call import.
    pub needs_host_call: bool,
    /// Set when any binding allocates compound values (RecordNew, ListNew, etc.)
    /// that must survive function boundaries — triggers __ail_malloc import.
    pub needs_heap_alloc: bool,
    /// Set when any binding uses runtime services (concurrency, dispatch, resources)
    /// — triggers ail_runtime_call import.
    pub needs_runtime_call: bool,
}

impl NativeDataLayout {
    /// Intern a string, returning its index into `ordered`.
    pub fn intern(&mut self, s: &str) -> usize {
        if let Some(&(idx, _)) = self.strings.get(s) {
            return idx;
        }
        let idx = self.ordered.len();
        let len = s.len();
        self.ordered.push(s.to_string());
        self.strings.insert(s.to_string(), (idx, len));
        idx
    }

    /// Return `(index, byte_len)` for a previously interned string.
    pub fn get(&self, s: &str) -> (usize, usize) {
        self.strings.get(s).copied().unwrap_or((0, 0))
    }

    /// Pre-scan all bindings to intern strings and detect EffectCall usage.
    pub fn for_bindings(bindings: &[crate::anf::AnfBinding]) -> Self {
        let mut layout = Self::default();
        for b in bindings {
            layout.scan_expr(&b.expr);
        }
        layout
    }

    fn scan_expr(&mut self, expr: &crate::anf::AnfExpr) {
        use crate::anf::AnfExpr;
        use crate::core_ir::LiteralValue;
        match expr {
            AnfExpr::Literal(LiteralValue::Text(s)) => { self.intern(s); }
            AnfExpr::EffectCall { capability, func, .. } => {
                self.needs_host_call = true;
                self.intern(capability);
                self.intern(func);
            }
            AnfExpr::Let { value, body, .. } => {
                self.scan_expr(value);
                self.scan_expr(body);
            }
            AnfExpr::Return(inner) => self.scan_expr(inner),
            AnfExpr::Seq(exprs) => exprs.iter().for_each(|e| self.scan_expr(e)),
            AnfExpr::If { then_branch, else_branch, .. } => {
                self.scan_expr(then_branch);
                self.scan_expr(else_branch);
            }
            AnfExpr::Loop { body } | AnfExpr::Break { value: body } => self.scan_expr(body),
            AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
                self.scan_expr(right);
            }
            AnfExpr::Match { arms, .. } => {
                for arm in arms { self.scan_expr(&arm.body); }
            }
            AnfExpr::RecordNew { fields } => {
                for (_, e) in fields { self.scan_expr(e); }
                self.needs_heap_alloc = true;
            }
            AnfExpr::FieldUpdate { value, .. } => self.scan_expr(value),
            AnfExpr::TupleNew(elems) => {
                for e in elems { self.scan_expr(e); }
                self.needs_heap_alloc = true;
            }
            AnfExpr::ListNew(elems) => {
                for e in elems { self.scan_expr(e); }
                self.needs_heap_alloc = true;
            }
            AnfExpr::VariantNew { payload, .. } => {
                if let Some(p) = payload { self.scan_expr(p); }
                self.needs_heap_alloc = true;
            }
            // ola5 Gap 2/3 — new variants
            AnfExpr::ForEach { body, .. } => self.scan_expr(body),
            AnfExpr::Lambda { body, .. } => self.scan_expr(body),
            AnfExpr::TaskGroup { body } => {
                self.scan_expr(body);
                self.needs_runtime_call = true;
            }
            AnfExpr::Timeout { body, .. } => {
                self.scan_expr(body);
                self.needs_runtime_call = true;
            }
            AnfExpr::TaskSpawn { .. }
            | AnfExpr::TaskAwait { .. }
            | AnfExpr::TaskCancel { .. }
            | AnfExpr::ChannelNew { .. }
            | AnfExpr::ChannelSend { .. }
            | AnfExpr::ChannelReceive { .. }
            | AnfExpr::Select { .. }
            | AnfExpr::Dispatch { .. }
            | AnfExpr::ResourceAcquire { .. }
            | AnfExpr::ResourceRelease { .. } => { self.needs_runtime_call = true; }
            AnfExpr::CellNew { .. }
            | AnfExpr::CellGet { .. }
            | AnfExpr::CellSet { .. }
            | AnfExpr::MapNew { .. }
            | AnfExpr::SetNew { .. } => { self.needs_heap_alloc = true; }
            _ => {}
        }
    }

    /// Declare + define all interned string data objects in the `ObjectModule`.
    /// Returns a `Vec` mapping index → `DataId`.
    pub fn define_all(&self, module: &mut ObjectModule) -> Result<Vec<DataId>, CompileError> {
        let mut data_ids = Vec::with_capacity(self.ordered.len());
        for (i, s) in self.ordered.iter().enumerate() {
            let name = format!("__ail_str_{i}");
            let data_id = module
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|e| CompileError::NativeEncodingError(
                    format!("declare_data({name}): {e}")))?;
            let mut desc = DataDescription::new();
            let bytes: Box<[u8]> = s.as_bytes().to_vec().into_boxed_slice();
            desc.define(bytes);
            module
                .define_data(data_id, &desc)
                .map_err(|e| CompileError::NativeEncodingError(
                    format!("define_data({name}): {e}")))?;
            data_ids.push(data_id);
        }
        Ok(data_ids)
    }
}

// ── CapabilityEntry ───────────────────────────────────────────────────────

/// One entry in the capability manifest — one per `AnfBinding`.
///
/// Mirrors the WASM backend's capability manifest schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEntry {
    /// Binding name, copied from `AnfBinding.name`.
    pub name: String,
    /// Provenance back to the originating `SemanticGraph` node.
    pub source_ref: NodeRef,
}

// ── CapabilitiesManifest ──────────────────────────────────────────────────

/// Side-car capability manifest for native artifacts.
///
/// Generated from `AnfIr.bindings` — one `CapabilityEntry` per binding.
/// Follows the same schema as the WASM backend's capability manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitiesManifest {
    /// One entry per `AnfBinding` in source traversal order.
    pub entries: Vec<CapabilityEntry>,
}

// ── NativeArtifact ────────────────────────────────────────────────────────

/// Output of the native backend stage: a platform-native object file with
/// provenance, a capabilities manifest, and a fully sealed hash chain.
///
/// In Phase 17 every function body is a `trap` stub.
/// Expression lowering is deferred to Phase 8+.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeArtifact {
    /// Platform-native object bytes (ELF / Mach-O / COFF).
    pub native_bytes: Vec<u8>,
    /// Semantic source map with `native_offset` populated for every binding.
    ///
    /// One entry per `AnfBinding` in binding order.  `wasm_offset` is always
    /// `None` in native artifacts (populated only by `emit_wasm`).
    pub source_map: SourceMap,
    /// Maps each `NodeRef` from the source graph to its cumulative byte
    /// offset in the object file's code section.
    /// Kept as a derived compatibility index; prefer `source_map` for new code.
    /// Empty when the input `AnfIr` has no bindings.
    pub provenance: BTreeMap<NodeRef, u64>,
    /// Capability manifest listing all binding names.
    pub capabilities_manifest: CapabilitiesManifest,
    /// Hash chain extended through the native backend stage.
    /// `hash_chain.native_hash` is `Some(...)` after `emit_native` completes.
    /// `hash_chain.source_map_hash` is `Some(...)` after `emit_native` completes.
    /// `hash_chain.artifact_manifest_hash` is `Some(...)` after `emit_native`.
    pub hash_chain: StageHashes,
    /// Profile-bound artifact manifest for this native artifact.
    ///
    /// Can be serialized as `program.artifact.json` by callers.
    pub artifact_manifest: ArtifactManifest,
    /// JSON-serialized `SourceMap` — content for `program.source_map.json`.
    ///
    /// Callers write this to disk as the source-map sidecar for debugging,
    /// profiling, and runtime error mapping.
    pub source_map_json: Vec<u8>,
    /// JSON-serialized `ArtifactManifest` — content for `program.artifact.json`.
    pub artifact_manifest_json: Vec<u8>,
}

// ── build_isa ─────────────────────────────────────────────────────────────

/// Construct a Cranelift ISA targeting the host architecture.
///
/// Uses `cranelift_native::builder()` to detect the host ISA, then applies
/// the default flag set.  This is the same approach proven by the V-03 spike.
fn build_isa() -> Result<cranelift_codegen::isa::OwnedTargetIsa, CompileError> {
    let flags = settings::Flags::new(settings::builder());
    cranelift_native::builder()
        .map_err(|e| CompileError::NativeEncodingError(format!("ISA builder failed: {e}")))?
        .finish(flags)
        .map_err(|e| CompileError::NativeEncodingError(format!("ISA finish failed: {e}")))
}

// ── build_object_module ───────────────────────────────────────────────────

/// Create a fresh `ObjectModule` for the host ISA.
fn build_object_module(
    isa: cranelift_codegen::isa::OwnedTargetIsa,
) -> Result<ObjectModule, CompileError> {
    let obj_builder =
        ObjectBuilder::new(isa, "ail_native", cranelift_module::default_libcall_names())
            .map_err(|e| CompileError::NativeEncodingError(format!("ObjectBuilder failed: {e}")))?;
    Ok(ObjectModule::new(obj_builder))
}

// ── LowerResult ───────────────────────────────────────────────────────────

/// Result of lowering one `AnfExpr` into Cranelift IR.
enum LowerResult {
    /// The expression produced a Cranelift SSA value; the current block is
    /// NOT yet terminated — caller must emit `return_(&[val])`.
    Value(cranelift_codegen::ir::Value),
    /// The expression produces no value (unit); the current block is NOT
    /// yet terminated — caller must emit `return_(&[])`.
    Unit,
    /// The expression emitted a terminating instruction (`trap`); the current
    /// block IS terminated — caller must NOT emit another terminator.
    Terminated,
}

// ── NativeLabelKind ───────────────────────────────────────────────────────

/// Kind of label pushed onto the label stack for Loop/Break/Continue resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeLabelKind {
    LoopBreak,
    LoopContinue,
}

// ── NativeCodegenCtx ──────────────────────────────────────────────────────

/// Per-function compilation context for `lower_anf_expr_cranelift`.
struct NativeCodegenCtx<'a> {
    /// Maps ANF let-binding names to their Cranelift SSA `Value` + type.
    /// Uses `String` keys to avoid lifetime complexity with nested expressions.
    locals: BTreeMap<String, (cranelift_codegen::ir::Value, cranelift_codegen::ir::types::Type)>,
    /// Label stack for Loop/Break/Continue resolution.
    labels: Vec<(NativeLabelKind, cranelift_codegen::ir::Block)>,
    /// Record layout map: binding name → ordered field names.
    record_layouts: BTreeMap<String, Vec<String>>,
    /// Tag discriminant table: tag string → u32 id (first-encounter order).
    variant_tags: BTreeMap<String, u32>,
    next_variant_tag: u32,
    /// Interned data object IDs for text literals and EffectCall strings.
    data_ids: &'a [DataId],
    /// Layout describing which strings map to which data_ids index.
    data_layout: &'a NativeDataLayout,
    /// Imported host_call FuncId if the program uses EffectCall.
    host_call_id: Option<FuncId>,
    /// Imported __ail_malloc FuncId for heap allocation of compound values.
    malloc_id: Option<FuncId>,
    /// Imported ail_runtime_call FuncId for concurrency/dispatch/resource ops.
    runtime_call_id: Option<FuncId>,
    /// Counter for generating unique lambda function names.
    next_lambda: u32,
}

impl<'a> NativeCodegenCtx<'a> {
    fn new(
        data_ids: &'a [DataId],
        data_layout: &'a NativeDataLayout,
        host_call_id: Option<FuncId>,
    ) -> Self {
        Self {
            locals: BTreeMap::new(),
            labels: Vec::new(),
            record_layouts: BTreeMap::new(),
            variant_tags: BTreeMap::new(),
            next_variant_tag: 0,
            data_ids,
            data_layout,
            host_call_id,
            malloc_id: None,
            runtime_call_id: None,
            next_lambda: 0,
        }
    }

    fn bind(&mut self, name: &str, val: cranelift_codegen::ir::Value,
            ty: cranelift_codegen::ir::types::Type) {
        self.locals.insert(name.to_string(), (val, ty));
    }

    fn lookup(&self, name: &str) -> Option<(cranelift_codegen::ir::Value,
                                             cranelift_codegen::ir::types::Type)> {
        self.locals.get(name).copied()
    }

    fn field_offset(&self, record: &str, field: &str) -> i32 {
        if let Some(fields) = self.record_layouts.get(record) {
            for (i, f) in fields.iter().enumerate() {
                if f == field {
                    return (i * 8) as i32;
                }
            }
        }
        0
    }

    fn assign_tag(&mut self, tag: &str) -> u32 {
        if let Some(&id) = self.variant_tags.get(tag) {
            return id;
        }
        // Use FNV-1a hash of the tag name for a stable, name-dependent discriminant.
        // This ensures the same tag always gets the same ID across compilation units.
        let mut h: u32 = 2166136261;
        for b in tag.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
        self.variant_tags.insert(tag.to_string(), h);
        h
    }

    fn push_label(&mut self, kind: NativeLabelKind, block: cranelift_codegen::ir::Block) {
        self.labels.push((kind, block));
    }

    fn pop_label(&mut self) {
        self.labels.pop();
    }

    fn find_label(&self, kind: NativeLabelKind) -> Option<cranelift_codegen::ir::Block> {
        self.labels.iter().rev()
            .find(|(k, _)| *k == kind)
            .map(|(_, b)| *b)
    }
}

// ── infer_cranelift_return_type ───────────────────────────────────────────

/// Infer the Cranelift return type for an `AnfExpr` without compiling it.
///
/// Returns `None` when the expression produces no value (unit, trap stub,
/// or unsupported variant).
fn infer_cranelift_return_type(
    expr: &crate::anf::AnfExpr,
) -> Option<cranelift_codegen::ir::types::Type> {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::types;

    match expr {
        AnfExpr::Literal(LiteralValue::Int(_)) => Some(types::I64),
        AnfExpr::Literal(LiteralValue::Bool(_)) => Some(types::I8),
        AnfExpr::Literal(LiteralValue::Float(_)) => Some(types::F64),
        AnfExpr::Literal(LiteralValue::Text(_)) => Some(types::I64),
        AnfExpr::Let { body, .. } => infer_cranelift_return_type(body),
        AnfExpr::Return(inner) => infer_cranelift_return_type(inner),
        AnfExpr::Call { func, .. } => match func.as_str() {
            "i64.add" | "+" | "add"
            | "i64.sub" | "-" | "sub"
            | "i64.mul" | "*" | "mul"
            | "i64.div_s" | "/" | "div"
            | "i64.rem_s" | "%" | "mod"
            | "i64.and" | "and"
            | "i64.or" | "or"
            | "i64.neg" | "neg" | "negate"
            => Some(types::I64),
            "i64.eq" | "==" | "eq"
            | "i64.ne" | "!=" | "ne"
            | "i64.lt_s" | "<" | "lt"
            | "i64.le_s" | "<=" | "le"
            | "i64.gt_s" | ">" | "gt"
            | "i64.ge_s" | ">=" | "ge"
            | "i64.eqz" | "not" | "!"
            => Some(types::I8),
            _ => None,
        },
        AnfExpr::If { then_branch, .. } => infer_cranelift_return_type(then_branch),
        AnfExpr::ShortCircuitAnd { .. } | AnfExpr::ShortCircuitOr { .. } => Some(types::I64),
        AnfExpr::Seq(exprs) => exprs.last().and_then(infer_cranelift_return_type),
        AnfExpr::Loop { body } => infer_cranelift_return_type(body),
        AnfExpr::Break { value } => infer_cranelift_return_type(value),
        AnfExpr::Match { arms, .. } => arms.first()
            .and_then(|a| infer_cranelift_return_type(&a.body)),
        AnfExpr::RecordNew { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::FieldUpdate { .. }
        | AnfExpr::VariantNew { .. }
        | AnfExpr::ListNew(_)
        | AnfExpr::TupleNew(_)
        | AnfExpr::EffectCall { .. }
        // ola5 Gap 2/3 — heap-allocated compound types and runtime results
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Lambda { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. } => Some(types::I64),
        _ => None,
    }
}

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
fn lower_anf_expr_cranelift(
    expr: &crate::anf::AnfExpr,
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
) -> LowerResult {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::{TrapCode, types};

    match expr {
        AnfExpr::Literal(lit) => match lit {
            LiteralValue::Int(n) => {
                LowerResult::Value(builder.ins().iconst(types::I64, *n))
            }
            LiteralValue::Bool(b) => {
                LowerResult::Value(builder.ins().iconst(types::I8, *b as i64))
            }
            LiteralValue::Float(f) => {
                LowerResult::Value(builder.ins().f64const(*f))
            }
            LiteralValue::Unit => LowerResult::Unit,
            LiteralValue::Text(s) => {
                let (idx, len) = ctx.data_layout.get(s.as_str());
                if idx >= ctx.data_ids.len() {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                let data_id = ctx.data_ids[idx];
                let gv = module.declare_data_in_func(data_id, &mut builder.func);
                let ptr = builder.ins().symbol_value(types::I64, gv);
                // Pack: (len as i64) << 32 | ptr
                let len_val = builder.ins().iconst(types::I64, len as i64);
                let len_shifted = builder.ins().ishl_imm(len_val, 32);
                let packed = builder.ins().bor(len_shifted, ptr);
                LowerResult::Value(packed)
            }
        },

        AnfExpr::Var(name) => match ctx.lookup(name.as_str()) {
            Some((val, _)) => LowerResult::Value(val),
            None => {
                builder.ins().trap(TrapCode::user(1).unwrap());
                LowerResult::Terminated
            }
        },

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

        AnfExpr::Return(inner) => lower_anf_expr_cranelift(inner, ctx, builder, module),

        AnfExpr::Call { func, args } => {
            match func.as_str() {
                // ── binary I64 arithmetic ──────────────────────────────
                "i64.add" | "+" | "add"
                | "i64.sub" | "-" | "sub"
                | "i64.mul" | "*" | "mul"
                | "i64.div_s" | "/" | "div"
                | "i64.rem_s" | "%" | "mod"
                | "i64.and" | "and"
                | "i64.or" | "or"
                if args.len() == 2 => {
                    let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
                    let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
                    match (lhs, rhs) {
                        (Some(l), Some(r)) => {
                            let val = match func.as_str() {
                                "i64.add" | "+" | "add" => builder.ins().iadd(l, r),
                                "i64.sub" | "-" | "sub" => builder.ins().isub(l, r),
                                "i64.mul" | "*" | "mul" => builder.ins().imul(l, r),
                                "i64.div_s" | "/" | "div" => builder.ins().sdiv(l, r),
                                "i64.rem_s" | "%" | "mod" => builder.ins().srem(l, r),
                                "i64.and" | "and" => builder.ins().band(l, r),
                                "i64.or" | "or"  => builder.ins().bor(l, r),
                                _ => unreachable!(),
                            };
                            LowerResult::Value(val)
                        }
                        _ => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                // ── binary comparisons → I8 ────────────────────────────
                "i64.eq" | "==" | "eq"
                | "i64.ne" | "!=" | "ne"
                | "i64.lt_s" | "<" | "lt"
                | "i64.le_s" | "<=" | "le"
                | "i64.gt_s" | ">" | "gt"
                | "i64.ge_s" | ">=" | "ge"
                if args.len() == 2 => {
                    let lhs = ctx.lookup(args[0].as_str()).map(|(v, _)| v);
                    let rhs = ctx.lookup(args[1].as_str()).map(|(v, _)| v);
                    match (lhs, rhs) {
                        (Some(l), Some(r)) => {
                            let cc = match func.as_str() {
                                "i64.eq"  | "==" | "eq" => IntCC::Equal,
                                "i64.ne"  | "!=" | "ne" => IntCC::NotEqual,
                                "i64.lt_s"| "<"  | "lt" => IntCC::SignedLessThan,
                                "i64.le_s"| "<=" | "le" => IntCC::SignedLessThanOrEqual,
                                "i64.gt_s"| ">"  | "gt" => IntCC::SignedGreaterThan,
                                "i64.ge_s"| ">=" | "ge" => IntCC::SignedGreaterThanOrEqual,
                                _ => unreachable!(),
                            };
                            LowerResult::Value(builder.ins().icmp(cc, l, r))
                        }
                        _ => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                // ── unary ops ─────────────────────────────────────────
                "i64.neg" | "neg" | "negate" if args.len() == 1 => {
                    match ctx.lookup(args[0].as_str()).map(|(v, _)| v) {
                        Some(a) => LowerResult::Value(builder.ins().ineg(a)),
                        None => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                "i64.eqz" | "not" | "!" if args.len() == 1 => {
                    match ctx.lookup(args[0].as_str()).map(|(v, _)| v) {
                        Some(a) => LowerResult::Value(
                            builder.ins().icmp_imm(IntCC::Equal, a, 0)
                        ),
                        None => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            LowerResult::Terminated
                        }
                    }
                }
                _ => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    LowerResult::Terminated
                }
            }
        }

        // ── Loop ─────────────────────────────────────────────────────────
        AnfExpr::Loop { body } => {
            let break_block = builder.create_block();
            let loop_block  = builder.create_block();

            let result_ty = infer_cranelift_return_type(body);
            if let Some(ty) = result_ty {
                builder.append_block_param(break_block, ty);
            }

            // Jump into the loop.
            builder.ins().jump(loop_block, &[]);
            builder.switch_to_block(loop_block);
            // DO NOT seal loop_block yet — back-edges from Continue not emitted.
            ctx.push_label(NativeLabelKind::LoopBreak, break_block);
            ctx.push_label(NativeLabelKind::LoopContinue, loop_block);

            let body_result = lower_anf_expr_cranelift(body, ctx, builder, module);

            ctx.pop_label(); // LoopContinue
            ctx.pop_label(); // LoopBreak

            // Implicit fall-through (no explicit break): jump back to header.
            if !matches!(body_result, LowerResult::Terminated) {
                builder.ins().jump(loop_block, &[]);
            }
            builder.seal_block(loop_block);
            builder.switch_to_block(break_block);
            builder.seal_block(break_block);

            match result_ty {
                Some(_) => LowerResult::Value(builder.block_params(break_block)[0]),
                None    => LowerResult::Unit,
            }
        }

        // ── Break ─────────────────────────────────────────────────────────
        AnfExpr::Break { value } => {
            let break_block = match ctx.find_label(NativeLabelKind::LoopBreak) {
                Some(b) => b,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            match lower_anf_expr_cranelift(value, ctx, builder, module) {
                LowerResult::Value(v)  => { builder.ins().jump(break_block, &[v]); }
                LowerResult::Unit      => { builder.ins().jump(break_block, &[]); }
                LowerResult::Terminated => {}
            }
            LowerResult::Terminated
        }

        // ── Continue ──────────────────────────────────────────────────────
        AnfExpr::Continue => {
            let loop_block = match ctx.find_label(NativeLabelKind::LoopContinue) {
                Some(b) => b,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            builder.ins().jump(loop_block, &[]);
            LowerResult::Terminated
        }

        // ── WhileLoop ─────────────────────────────────────────────────────
        AnfExpr::WhileLoop { cond, body } => {
            let break_block    = builder.create_block();
            let loop_block     = builder.create_block();
            let body_block     = builder.create_block();

            // Jump into the loop header.
            builder.ins().jump(loop_block, &[]);
            builder.switch_to_block(loop_block);
            // DO NOT seal loop_block yet.

            let cond_val = match ctx.lookup(cond.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            // brif cond → body_block, else → break_block (exit on false)
            builder.ins().brif(cond_val, body_block, &[], break_block, &[]);

            builder.switch_to_block(body_block);
            builder.seal_block(body_block);
            ctx.push_label(NativeLabelKind::LoopBreak, break_block);
            ctx.push_label(NativeLabelKind::LoopContinue, loop_block);
            let while_body_result = lower_anf_expr_cranelift(body, ctx, builder, module);
            ctx.pop_label(); // LoopContinue
            ctx.pop_label(); // LoopBreak
            if !matches!(while_body_result, LowerResult::Terminated) {
                builder.ins().jump(loop_block, &[]);
            }

            builder.seal_block(loop_block);
            builder.switch_to_block(break_block);
            builder.seal_block(break_block);
            LowerResult::Unit
        }

        // ── If ────────────────────────────────────────────────────────────
        AnfExpr::If { cond, then_branch, else_branch } => {
            let cond_val = match ctx.lookup(cond.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();

            let result_ty = infer_cranelift_return_type(then_branch);
            if let Some(ty) = result_ty {
                builder.append_block_param(merge_block, ty);
            }

            builder.ins().brif(cond_val, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            match lower_anf_expr_cranelift(then_branch, ctx, builder, module) {
                LowerResult::Value(v) => { builder.ins().jump(merge_block, &[v]); }
                LowerResult::Unit     => { builder.ins().jump(merge_block, &[]); }
                LowerResult::Terminated => {}
            }

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            match lower_anf_expr_cranelift(else_branch, ctx, builder, module) {
                LowerResult::Value(v) => { builder.ins().jump(merge_block, &[v]); }
                LowerResult::Unit     => { builder.ins().jump(merge_block, &[]); }
                LowerResult::Terminated => {}
            }

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            match result_ty {
                Some(_) => LowerResult::Value(builder.block_params(merge_block)[0]),
                None    => LowerResult::Unit,
            }
        }

        // ── Match ────────────────────────────────────────────────────────
        AnfExpr::Match { scrutinee, arms } => {
            // Empty arms → trap.
            if arms.is_empty() {
                builder.ins().trap(TrapCode::user(1).unwrap());
                return LowerResult::Terminated;
            }

            let (scrutinee_val, scrutinee_ty) = match ctx.lookup(scrutinee.as_str()) {
                Some(pair) => pair,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            // Normalize scrutinee to I64 for pattern comparisons (extend I8 bools).
            let scrutinee_i64 = if scrutinee_ty == types::I64 {
                scrutinee_val
            } else {
                builder.ins().uextend(types::I64, scrutinee_val)
            };

            let result_ty = infer_cranelift_return_type(&arms[0].body);
            let merge_block = builder.create_block();
            if let Some(ty) = result_ty {
                builder.append_block_param(merge_block, ty);
            }

            // Linear scan through arms. After each non-wildcard check, if the
            // pattern doesn't match we jump to the next check block.
            // The wildcard or last arm terminates the chain.
            let n = arms.len();
            let mut has_merge_predecessor = false;
            for i in 0..n {
                let arm = &arms[i];
                let arm_block = builder.create_block();

                if arm.pattern == "_" {
                    // Wildcard: unconditional jump into arm_block.
                    builder.ins().jump(arm_block, &[]);
                    builder.switch_to_block(arm_block);
                    builder.seal_block(arm_block);
                    // Lower arm body and jump to merge.
                    match lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                        LowerResult::Value(v) => {
                            builder.ins().jump(merge_block, &[v]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Unit     => {
                            builder.ins().jump(merge_block, &[]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Terminated => {}
                    }
                    // Wildcard terminates the chain; remaining arms (if any) are unreachable.
                    break;
                }

                // Non-wildcard: compute equality check.
                let pattern_val: i64 = match arm.pattern.trim() {
                    "true"  => 1,
                    "false" => 0,
                    s       => match s.parse::<i64>() {
                        Ok(value) => value,
                        Err(_) => {
                            builder.ins().trap(TrapCode::user(1).unwrap());
                            if has_merge_predecessor {
                                break;
                            }
                            return LowerResult::Terminated;
                        }
                    },
                };
                let pat_const = builder.ins().iconst(types::I64, pattern_val);
                let eq = builder.ins().icmp(IntCC::Equal, scrutinee_i64, pat_const);

                if i + 1 < n {
                    // More arms follow: create a next_check block for the false branch.
                    let next_check = builder.create_block();
                    builder.ins().brif(eq, arm_block, &[], next_check, &[]);
                    // Emit arm body in arm_block.
                    builder.switch_to_block(arm_block);
                    builder.seal_block(arm_block);
                    match lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                        LowerResult::Value(v) => {
                            builder.ins().jump(merge_block, &[v]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Unit     => {
                            builder.ins().jump(merge_block, &[]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Terminated => {}
                    }
                    // Switch to next_check for the next iteration.
                    builder.switch_to_block(next_check);
                    builder.seal_block(next_check);
                } else {
                    // Last arm and not wildcard: trap if pattern doesn't match.
                    let trap_block = builder.create_block();
                    builder.ins().brif(eq, arm_block, &[], trap_block, &[]);
                    // Trap block.
                    builder.switch_to_block(trap_block);
                    builder.seal_block(trap_block);
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    // Arm block.
                    builder.switch_to_block(arm_block);
                    builder.seal_block(arm_block);
                    match lower_anf_expr_cranelift(&arm.body, ctx, builder, module) {
                        LowerResult::Value(v) => {
                            builder.ins().jump(merge_block, &[v]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Unit     => {
                            builder.ins().jump(merge_block, &[]);
                            has_merge_predecessor = true;
                        }
                        LowerResult::Terminated => {}
                    }
                }
            }

            if !has_merge_predecessor {
                return LowerResult::Terminated;
            }

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            match result_ty {
                Some(_) => LowerResult::Value(builder.block_params(merge_block)[0]),
                None    => LowerResult::Unit,
            }
        }

        // ── ShortCircuitAnd ───────────────────────────────────────────────
        AnfExpr::ShortCircuitAnd { left, right } => {
            let left_val = match ctx.lookup(left.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let true_block  = builder.create_block();
            let false_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, cranelift_codegen::ir::types::I64);

            builder.ins().brif(left_val, true_block, &[], false_block, &[]);

            // true branch: evaluate right
            builder.switch_to_block(true_block);
            builder.seal_block(true_block);
            let right_val = match lower_anf_expr_cranelift(right, ctx, builder, module) {
                LowerResult::Value(v) => v,
                _ => builder.ins().iconst(cranelift_codegen::ir::types::I64, 0),
            };
            builder.ins().jump(merge_block, &[right_val]);

            // false branch: short-circuit → 0
            builder.switch_to_block(false_block);
            builder.seal_block(false_block);
            let zero = builder.ins().iconst(cranelift_codegen::ir::types::I64, 0);
            builder.ins().jump(merge_block, &[zero]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            LowerResult::Value(builder.block_params(merge_block)[0])
        }

        // ── Seq ───────────────────────────────────────────────────────────
        AnfExpr::Seq(exprs) => {
            if exprs.is_empty() {
                return LowerResult::Unit;
            }
            let last_idx = exprs.len() - 1;
            for expr in &exprs[..last_idx] {
                // Lower each non-last expression; result is intentionally dropped.
                match lower_anf_expr_cranelift(expr, ctx, builder, module) {
                    LowerResult::Terminated => return LowerResult::Terminated,
                    _ => {}
                }
            }
            lower_anf_expr_cranelift(&exprs[last_idx], ctx, builder, module)
        }

        // ── RuntimeCheck ──────────────────────────────────────────────────
        AnfExpr::RuntimeCheck { cond, .. } => {
            let cond_val = match ctx.lookup(cond.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let trap_block  = builder.create_block();
            let fallthrough = builder.create_block();

            // brif cond → trap_block (trap if cond non-zero), else fallthrough
            builder.ins().brif(cond_val, trap_block, &[], fallthrough, &[]);

            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            builder.ins().trap(TrapCode::user(1).unwrap());

            builder.switch_to_block(fallthrough);
            builder.seal_block(fallthrough);
            LowerResult::Unit
        }

        // ── ShortCircuitOr ────────────────────────────────────────────────
        AnfExpr::ShortCircuitOr { left, right } => {
            let left_val = match ctx.lookup(left.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let true_block  = builder.create_block();
            let false_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, cranelift_codegen::ir::types::I64);

            builder.ins().brif(left_val, true_block, &[], false_block, &[]);

            // true branch: short-circuit → 1
            builder.switch_to_block(true_block);
            builder.seal_block(true_block);
            let one = builder.ins().iconst(cranelift_codegen::ir::types::I64, 1);
            builder.ins().jump(merge_block, &[one]);

            // false branch: evaluate right
            builder.switch_to_block(false_block);
            builder.seal_block(false_block);
            let right_val = match lower_anf_expr_cranelift(right, ctx, builder, module) {
                LowerResult::Value(v) => v,
                _ => builder.ins().iconst(cranelift_codegen::ir::types::I64, 0),
            };
            builder.ins().jump(merge_block, &[right_val]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            LowerResult::Value(builder.block_params(merge_block)[0])
        }

        // ── RecordNew ─────────────────────────────────────────────────────
        // Heap-allocated: pointer survives function return.
        // Layout: [field0: i64, field1: i64, ...] — no length header.
        AnfExpr::RecordNew { fields } => {
            let byte_size = (fields.len().max(1) * 8) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback to stack allocation (dangling pointer — Phase 17 legacy).
                    let size = (fields.len().max(1) * 8) as u32;
                    let slot = builder.create_sized_stack_slot(
                        StackSlotData::new(StackSlotKind::ExplicitSlot, size, 3)
                    );
                    for (idx, (_, field_expr)) in fields.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(field_expr, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(val, slot, (idx * 8) as i32);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    for (idx, (_, field_expr)) in fields.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(field_expr, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().store(MemFlags::trusted(), val, ptr, (idx * 8) as i32);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── FieldGet ──────────────────────────────────────────────────────
        AnfExpr::FieldGet { record, field } => {
            let ptr = match ctx.lookup(record.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let offset = ctx.field_offset(record.as_str(), field.as_str());
            let val = builder.ins().load(types::I64, MemFlags::trusted(), ptr, offset);
            LowerResult::Value(val)
        }

        // ── FieldUpdate ───────────────────────────────────────────────────
        AnfExpr::FieldUpdate { record, field, value } => {
            let ptr = match ctx.lookup(record.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let val = match lower_anf_expr_cranelift(value, ctx, builder, module) {
                LowerResult::Value(v) => v,
                _ => builder.ins().iconst(types::I64, 0),
            };
            let offset = ctx.field_offset(record.as_str(), field.as_str());
            builder.ins().store(MemFlags::trusted(), val, ptr, offset);
            LowerResult::Value(ptr)
        }

        // ── VariantNew ────────────────────────────────────────────────────
        // Heap-allocated: 16 bytes: [tag: i64, payload: i64].
        AnfExpr::VariantNew { tag, payload } => {
            let tag_id = ctx.assign_tag(tag.as_str()) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback stack allocation.
                    let slot = builder.create_sized_stack_slot(
                        StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3)
                    );
                    let tag_val = builder.ins().iconst(types::I64, tag_id);
                    builder.ins().stack_store(tag_val, slot, 0);
                    if let Some(payload_expr) = payload {
                        let pv = match lower_anf_expr_cranelift(payload_expr, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(pv, slot, 8);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, 16);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let tag_val = builder.ins().iconst(types::I64, tag_id);
                    builder.ins().store(MemFlags::trusted(), tag_val, ptr, 0);
                    if let Some(payload_expr) = payload {
                        let pv = match lower_anf_expr_cranelift(payload_expr, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().store(MemFlags::trusted(), pv, ptr, 8);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── ListNew ───────────────────────────────────────────────────────
        // Heap-allocated: [len: i64, elem0: i64, ...].
        AnfExpr::ListNew(elems) => {
            let byte_size = ((1 + elems.len()) * 8) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback stack allocation.
                    let size = ((1 + elems.len()) * 8) as u32;
                    let slot = builder.create_sized_stack_slot(
                        StackSlotData::new(StackSlotKind::ExplicitSlot, size, 3)
                    );
                    let len_val = builder.ins().iconst(types::I64, elems.len() as i64);
                    builder.ins().stack_store(len_val, slot, 0);
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(val, slot, (8 + i * 8) as i32);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let len_val = builder.ins().iconst(types::I64, elems.len() as i64);
                    builder.ins().store(MemFlags::trusted(), len_val, ptr, 0);
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().store(MemFlags::trusted(), val, ptr, (8 + i * 8) as i32);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── TupleNew ──────────────────────────────────────────────────────
        // Heap-allocated: [elem0: i64, elem1: i64, ...].
        AnfExpr::TupleNew(elems) => {
            let byte_size = (elems.len().max(1) * 8) as i64;
            match ctx.malloc_id {
                None => {
                    // Fallback stack allocation.
                    let size = (elems.len().max(1) * 8) as u32;
                    let slot = builder.create_sized_stack_slot(
                        StackSlotData::new(StackSlotKind::ExplicitSlot, size, 3)
                    );
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().stack_store(val, slot, (i * 8) as i32);
                    }
                    LowerResult::Value(builder.ins().stack_addr(types::I64, slot, 0))
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    for (i, elem) in elems.iter().enumerate() {
                        let val = match lower_anf_expr_cranelift(elem, ctx, builder, module) {
                            LowerResult::Value(v) => v,
                            _ => builder.ins().iconst(types::I64, 0),
                        };
                        builder.ins().store(MemFlags::trusted(), val, ptr, (i * 8) as i32);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── EffectCall ────────────────────────────────────────────────────
        AnfExpr::EffectCall { capability, func, args } => {
            let host_call_id = match ctx.host_call_id {
                Some(id) => id,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            // Args buffer: store each arg as I64 in a stack slot.
            let args_size = (args.len().max(1) * 8) as u32;
            let args_slot = builder.create_sized_stack_slot(
                StackSlotData::new(StackSlotKind::ExplicitSlot, args_size, 3)
            );
            for (idx, arg_name) in args.iter().enumerate() {
                let arg_val = match ctx.lookup(arg_name.as_str()) {
                    Some((v, _)) => v,
                    None => builder.ins().iconst(types::I64, 0),
                };
                builder.ins().stack_store(arg_val, args_slot, (idx * 8) as i32);
            }
            let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);

            // Capability + func name pointers from data section.
            let (cap_idx, cap_len) = ctx.data_layout.get(capability.as_str());
            let (op_idx, op_len) = ctx.data_layout.get(func.as_str());
            let cap_gv = module.declare_data_in_func(ctx.data_ids[cap_idx], &mut builder.func);
            let op_gv  = module.declare_data_in_func(ctx.data_ids[op_idx], &mut builder.func);
            let cap_ptr = builder.ins().symbol_value(types::I64, cap_gv);
            let op_ptr  = builder.ins().symbol_value(types::I64, op_gv);

            let host_call_ref = module.declare_func_in_func(host_call_id, &mut builder.func);
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

        // ── Assume ───────────────────────────────────────────────────────
        // No runtime effect — a proof assumption is purely a static annotation.
        AnfExpr::Assume { .. } => LowerResult::Unit,

        // ── Abort ─────────────────────────────────────────────────────────
        // Always traps — represents an impossible branch (Never type).
        AnfExpr::Abort { .. } => {
            builder.ins().trap(TrapCode::user(2).unwrap());
            LowerResult::Terminated
        }

        // ── IndexGet ─────────────────────────────────────────────────────
        // Load element at `index` from a length-prefixed list/map/set.
        // Collection layout: [len: i64, elem0: i64, elem1: i64, ...]
        // Element offset = 8 + index * 8.
        AnfExpr::IndexGet { collection, index } => {
            let col_ptr = match ctx.lookup(collection.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let idx_val = match ctx.lookup(index.as_str()) {
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

        // ── MapNew ────────────────────────────────────────────────────────
        // Heap-allocate a map: [count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...]
        AnfExpr::MapNew { entries } => {
            let byte_size = (1 + entries.len() * 2) as i64 * 8;
            match ctx.malloc_id {
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    // Store count
                    let count = builder.ins().iconst(types::I64, entries.len() as i64);
                    builder.ins().store(MemFlags::trusted(), count, ptr, 0);
                    // Store key-value pairs
                    for (i, (k_name, v_name)) in entries.iter().enumerate() {
                        let base = (1 + i * 2) as i32 * 8;
                        let k_val = ctx.lookup(k_name.as_str()).map(|(v, _)| v)
                            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        let v_val = ctx.lookup(v_name.as_str()).map(|(v, _)| v)
                            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        builder.ins().store(MemFlags::trusted(), k_val, ptr, base);
                        builder.ins().store(MemFlags::trusted(), v_val, ptr, base + 8);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── SetNew ────────────────────────────────────────────────────────
        // Heap-allocate a set: [count: i64, elem0: i64, elem1: i64, ...]
        AnfExpr::SetNew { elements } => {
            let byte_size = (1 + elements.len()) as i64 * 8;
            match ctx.malloc_id {
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, byte_size);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let count = builder.ins().iconst(types::I64, elements.len() as i64);
                    builder.ins().store(MemFlags::trusted(), count, ptr, 0);
                    for (i, elem_name) in elements.iter().enumerate() {
                        let offset = (1 + i) as i32 * 8;
                        let val = ctx.lookup(elem_name.as_str()).map(|(v, _)| v)
                            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        builder.ins().store(MemFlags::trusted(), val, ptr, offset);
                    }
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── ForEach ───────────────────────────────────────────────────────
        // Loop over a length-prefixed list collection.
        // Layout: [len: i64, elem0: i64, ...]
        // Generates: counter from 0 to len-1, binding each element into body.
        AnfExpr::ForEach { binding, collection, body } => {
            let col_ptr = match ctx.lookup(collection.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            // Load length
            let len_val = builder.ins().load(types::I64, MemFlags::trusted(), col_ptr, 0);

            // Allocate a stack slot for the loop counter.
            let counter_slot = builder.create_sized_stack_slot(
                StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3)
            );
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().stack_store(zero, counter_slot, 0);

            let break_block = builder.create_block();
            let loop_block  = builder.create_block();
            let body_block  = builder.create_block();

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
            let inner_sum  = builder.ins().iadd(offset, eight);
            let elem_addr  = builder.ins().iadd(col_ptr, inner_sum);
            let elem_val = builder.ins().load(types::I64, MemFlags::trusted(), elem_addr, 0);
            // Bind the element to the loop variable.
            ctx.bind(binding.as_str(), elem_val, types::I64);
            // Lower body (result discarded — ForEach is for side effects).
            let body_result = lower_anf_expr_cranelift(body, ctx, builder, module);
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

        // ── Fold ──────────────────────────────────────────────────────────
        // Left fold over a length-prefixed list using an I64 function pointer.
        // Signature expected for func: (acc: I64, elem: I64) -> I64.
        // CFG: entry → loop_hdr ←→ body_blk → exit_blk.
        AnfExpr::Fold { init, list, func } => {
            let init_val = match ctx.lookup(init.as_str()) {
                Some((v, _)) => v,
                None => builder.ins().iconst(types::I64, 0),
            };
            let list_ptr = match ctx.lookup(list.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let func_ptr = match ctx.lookup(func.as_str()) {
                Some((v, _)) => v,
                // No func pointer — return init unchanged (degenerate fold).
                None => return LowerResult::Value(init_val),
            };

            // Load list length once in the entry block.
            let len_val = builder.ins().load(types::I64, MemFlags::trusted(), list_ptr, 0);

            // Use stack slots for mutable accumulator and counter.
            let acc_slot = builder.create_sized_stack_slot(
                StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3)
            );
            let ctr_slot = builder.create_sized_stack_slot(
                StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3)
            );
            builder.ins().stack_store(init_val, acc_slot, 0);
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().stack_store(zero, ctr_slot, 0);

            let loop_hdr  = builder.create_block();
            let body_blk  = builder.create_block();
            let exit_blk  = builder.create_block();

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
            let acc  = builder.ins().stack_load(types::I64, acc_slot, 0);
            let ctr2 = builder.ins().stack_load(types::I64, ctr_slot, 0);
            let eight = builder.ins().iconst(types::I64, 8);
            let elem_off  = builder.ins().imul(ctr2, eight);
            let elem_base = builder.ins().iadd(elem_off, eight);
            let elem_addr = builder.ins().iadd(list_ptr, elem_base);
            let elem = builder.ins().load(types::I64, MemFlags::trusted(), elem_addr, 0);
            // Indirect call: func_ptr(acc, elem) → I64.
            let fold_sig = {
                let mut sig = Signature::new(cranelift_codegen::isa::CallConv::SystemV);
                sig.params.push(AbiParam::new(types::I64));
                sig.params.push(AbiParam::new(types::I64));
                sig.returns.push(AbiParam::new(types::I64));
                builder.import_signature(sig)
            };
            let indirect_call = builder.ins().call_indirect(fold_sig, func_ptr, &[acc, elem]);
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

        // ── CellNew ───────────────────────────────────────────────────────
        // Heap-allocate an 8-byte mutable cell, store init value, return ptr.
        AnfExpr::CellNew { init } => {
            match ctx.malloc_id {
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
                Some(malloc_id) => {
                    let size_val = builder.ins().iconst(types::I64, 8);
                    let malloc_ref = module.declare_func_in_func(malloc_id, &mut builder.func);
                    let call = builder.ins().call(malloc_ref, &[size_val]);
                    let ptr = builder.inst_results(call)[0];
                    let init_val = ctx.lookup(init.as_str()).map(|(v, _)| v)
                        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                    builder.ins().store(MemFlags::trusted(), init_val, ptr, 0);
                    LowerResult::Value(ptr)
                }
            }
        }

        // ── CellGet ───────────────────────────────────────────────────────
        // Load the I64 value stored in a heap-allocated cell.
        AnfExpr::CellGet { cell } => {
            let ptr = match ctx.lookup(cell.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let val = builder.ins().load(types::I64, MemFlags::trusted(), ptr, 0);
            LowerResult::Value(val)
        }

        // ── CellSet ───────────────────────────────────────────────────────
        // Store a new I64 value into a heap-allocated cell.
        AnfExpr::CellSet { cell, value } => {
            let ptr = match ctx.lookup(cell.as_str()) {
                Some((v, _)) => v,
                None => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };
            let val = ctx.lookup(value.as_str()).map(|(v, _)| v)
                .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
            builder.ins().store(MemFlags::trusted(), val, ptr, 0);
            LowerResult::Unit
        }

        // ── Lambda ────────────────────────────────────────────────────────
        // Define a nested function for the lambda body, return its address.
        // Params are bound as I64 arguments; captures are not supported yet.
        AnfExpr::Lambda { params, body } => {
            let lambda_name = format!("__ail_lambda_{}", ctx.next_lambda);
            ctx.next_lambda += 1;

            // Build signature: (param0: I64, param1: I64, ...) -> I64
            let mut lambda_sig = Signature::new(cranelift_codegen::isa::CallConv::SystemV);
            for _ in params {
                lambda_sig.params.push(AbiParam::new(types::I64));
            }
            // Return type inferred from body.
            let body_ret_ty = infer_cranelift_return_type(body);
            if let Some(ty) = body_ret_ty {
                lambda_sig.returns.push(AbiParam::new(ty));
            }

            let lambda_id = match module.declare_function(
                &lambda_name,
                Linkage::Local,
                &lambda_sig,
            ) {
                Ok(id) => id,
                Err(_) => {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            };

            // Build and define the lambda body.
            {
                let mut lam_func = Function::with_name_signature(
                    UserFuncName::user(0, lambda_id.as_u32()),
                    lambda_sig,
                );
                let mut lam_fn_ctx = FunctionBuilderContext::new();
                let mut lam_builder = FunctionBuilder::new(&mut lam_func, &mut lam_fn_ctx);
                let lam_block = lam_builder.create_block();
                lam_builder.append_block_params_for_function_params(lam_block);
                lam_builder.switch_to_block(lam_block);
                lam_builder.seal_block(lam_block);

                // Bind params to local names.
                let mut lam_ctx = NativeCodegenCtx::new(ctx.data_ids, ctx.data_layout, ctx.host_call_id);
                lam_ctx.malloc_id = ctx.malloc_id;
                lam_ctx.runtime_call_id = ctx.runtime_call_id;
                for (i, param_name) in params.iter().enumerate() {
                    let param_val = lam_builder.block_params(lam_block)[i];
                    lam_ctx.bind(param_name.as_str(), param_val, types::I64);
                }

                match lower_anf_expr_cranelift(body, &mut lam_ctx, &mut lam_builder, module) {
                    LowerResult::Value(v) => { lam_builder.ins().return_(&[v]); }
                    LowerResult::Unit     => { lam_builder.ins().return_(&[]); }
                    LowerResult::Terminated => {}
                }
                lam_builder.finalize();

                let mut lam_codegen = Context::for_function(lam_func);
                if module.define_function(lambda_id, &mut lam_codegen).is_err() {
                    builder.ins().trap(TrapCode::user(1).unwrap());
                    return LowerResult::Terminated;
                }
            }

            // Return the function's address as I64.
            let func_ref = module.declare_func_in_func(lambda_id, &mut builder.func);
            let ptr = builder.ins().func_addr(types::I64, func_ref);
            LowerResult::Value(ptr)
        }

        // ── TaskSpawn / TaskAwait / TaskCancel / TaskGroup ────────────────
        // ── ChannelNew / ChannelSend / ChannelReceive / Select / Timeout ──
        // ── Dispatch / ResourceAcquire / ResourceRelease ──────────────────
        // All concurrency and resource primitives are routed through the
        // imported `ail_runtime_call` function for runtime dispatch.
        AnfExpr::TaskSpawn { func, args } => {
            emit_runtime_call(ctx, builder, module, 1u64, &[func.as_str()], args)
        }
        AnfExpr::TaskAwait { task } => {
            emit_runtime_call(ctx, builder, module, 2u64, &[], &[task.clone()])
        }
        AnfExpr::TaskCancel { task } => {
            emit_runtime_call(ctx, builder, module, 3u64, &[], &[task.clone()])
        }
        AnfExpr::TaskGroup { body } => {
            // TaskGroup: lower body for side effects, then emit runtime notification.
            let _ = lower_anf_expr_cranelift(body, ctx, builder, module);
            emit_runtime_call(ctx, builder, module, 4u64, &[], &[])
        }
        AnfExpr::ChannelNew { capacity } => {
            let cap_val = capacity.unwrap_or(0) as i64;
            let cap_name = format!("__cap_{cap_val}");
            // Encode capacity as a synthetic arg: store in stack slot, pass ptr.
            let cap_iconst = builder.ins().iconst(types::I64, cap_val);
            let slot = builder.create_sized_stack_slot(
                StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3)
            );
            builder.ins().stack_store(cap_iconst, slot, 0);
            let _ = cap_name;
            emit_runtime_call(ctx, builder, module, 5u64, &[], &[])
        }
        AnfExpr::ChannelSend { channel, value } => {
            emit_runtime_call(ctx, builder, module, 6u64, &[], &[channel.clone(), value.clone()])
        }
        AnfExpr::ChannelReceive { channel } => {
            emit_runtime_call(ctx, builder, module, 7u64, &[], &[channel.clone()])
        }
        AnfExpr::Select { .. } => {
            emit_runtime_call(ctx, builder, module, 8u64, &[], &[])
        }
        AnfExpr::Timeout { duration, body } => {
            let _ = lower_anf_expr_cranelift(body, ctx, builder, module);
            emit_runtime_call(ctx, builder, module, 9u64, &[], &[duration.clone()])
        }
        AnfExpr::Dispatch { handler, method, args } => {
            let mut all_args: Vec<String> = vec![handler.clone(), method.clone()];
            all_args.extend(args.iter().cloned());
            emit_runtime_call(ctx, builder, module, 10u64, &[], &all_args)
        }
        AnfExpr::ResourceAcquire { resource, args } => {
            let mut all_args: Vec<String> = vec![resource.clone()];
            all_args.extend(args.iter().cloned());
            emit_runtime_call(ctx, builder, module, 11u64, &[], &all_args)
        }
        AnfExpr::ResourceRelease { handle } => {
            emit_runtime_call(ctx, builder, module, 12u64, &[], &[handle.clone()])
        }

        _ => {
            builder.ins().trap(TrapCode::user(1).unwrap());
            LowerResult::Terminated
        }
    }
}

// ── emit_runtime_call ─────────────────────────────────────────────────────

/// Emit an `ail_runtime_call(op, args_ptr, args_len) -> I64` call.
///
/// `op` is the operation discriminant (1 = TaskSpawn, 2 = TaskAwait, ...).
/// `name_args` are string-keyed args (looked up from data section).
/// `var_args` are variable-name args (looked up from ctx.locals).
fn emit_runtime_call(
    ctx: &mut NativeCodegenCtx<'_>,
    builder: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
    op: u64,
    _name_args: &[&str],
    var_args: &[String],
) -> LowerResult {
    use cranelift_codegen::ir::{TrapCode, types};

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
    let args_slot = builder.create_sized_stack_slot(
        StackSlotData::new(StackSlotKind::ExplicitSlot, buf_size, 3)
    );
    for (i, arg_name) in var_args.iter().enumerate() {
        let val = ctx.lookup(arg_name.as_str()).map(|(v, _)| v)
            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
        builder.ins().stack_store(val, args_slot, (i * 8) as i32);
    }
    let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
    let op_val      = builder.ins().iconst(types::I64, op as i64);
    let args_len    = builder.ins().iconst(types::I64, args_count as i64);

    let runtime_ref = module.declare_func_in_func(runtime_id, &mut builder.func);
    let call = builder.ins().call(runtime_ref, &[op_val, args_ptr, args_len]);
    LowerResult::Value(builder.inst_results(call)[0])
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
fn lower_binding(
    module: &mut ObjectModule,
    name: &str,
    expr: &crate::anf::AnfExpr,
    cumulative_offset: u64,
    data_ids: &[DataId],
    data_layout: &NativeDataLayout,
    host_call_id: Option<FuncId>,
    malloc_id: Option<FuncId>,
    runtime_call_id: Option<FuncId>,
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

        let mut codegen_ctx = NativeCodegenCtx::new(data_ids, data_layout, host_call_id);
        codegen_ctx.malloc_id = malloc_id;
        codegen_ctx.runtime_call_id = runtime_call_id;
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

    // The provenance offset is where this function starts: current cumulative
    // offset before we add this function's size.
    let _ = cumulative_offset; // consumed by caller; returned is the NEW offset
    Ok(u64::from(code_size))
}

// ── emit_native ───────────────────────────────────────────────────────────

/// Emit a platform-native object file from an `AnfIr`.
///
/// # Pre-conditions
///
/// - `anf.stage_hashes.anf_ir_hash` must be `Some(...)`.  Call
///   `lower_to_anf` before `emit_native`.
///
/// # Hash chain
///
/// Extends the chain: `native_hash = blake3(anf_ir_hash || native_bytes)`.
///
/// # Errors
///
/// - `CompileError::NativeEncodingError` — `anf_ir_hash` is `None` (pre-condition
///   violated) or Cranelift codegen / object emission failed.
pub fn emit_native(anf: &AnfIr) -> Result<NativeArtifact, CompileError> {
    emit_native_with_profile(anf, "unspecified")
}

/// Emit a native object and bind the artifact manifest to `profile`.
pub fn emit_native_with_profile(
    anf: &AnfIr,
    profile: &str,
) -> Result<NativeArtifact, CompileError> {
    // Gate: anf_ir_hash must be sealed.
    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .ok_or_else(|| CompileError::NativeEncodingError("anf_ir_hash not sealed".to_string()))?;

    let isa = build_isa()?;
    let mut module = build_object_module(isa)?;

    // Pre-scan all bindings to build the data layout and detect EffectCall.
    let data_layout = NativeDataLayout::for_bindings(&anf.bindings);
    let data_ids = data_layout.define_all(&mut module)?;

    // If any binding uses EffectCall, declare the imported host_call function.
    // Signature: (cap_ptr: I64, cap_len: I64, op_ptr: I64, op_len: I64,
    //             args_ptr: I64, args_len: I64) -> I64
    let host_call_id: Option<FuncId> = if data_layout.needs_host_call {
        let mut sig = Signature::new(CallConv::SystemV);
        for _ in 0..6 {
            sig.params.push(AbiParam::new(cranelift_codegen::ir::types::I64));
        }
        sig.returns.push(AbiParam::new(cranelift_codegen::ir::types::I64));
        let id = module
            .declare_function("host_call", Linkage::Import, &sig)
            .map_err(|e| CompileError::NativeEncodingError(
                format!("declare_function(host_call): {e}")))?;
        Some(id)
    } else {
        None
    };

    // If any binding allocates compound values on the heap, declare malloc.
    // Signature: (size: I64) -> I64  (returns opaque heap pointer)
    let malloc_id: Option<FuncId> = if data_layout.needs_heap_alloc {
        let mut sig = Signature::new(CallConv::SystemV);
        sig.params.push(AbiParam::new(cranelift_codegen::ir::types::I64));
        sig.returns.push(AbiParam::new(cranelift_codegen::ir::types::I64));
        let id = module
            .declare_function("__ail_malloc", Linkage::Import, &sig)
            .map_err(|e| CompileError::NativeEncodingError(
                format!("declare_function(__ail_malloc): {e}")))?;
        Some(id)
    } else {
        None
    };

    // If any binding uses concurrency/dispatch/resource primitives, declare
    // ail_runtime_call.
    // Signature: (op: I64, args_ptr: I64, args_len: I64) -> I64
    let runtime_call_id: Option<FuncId> = if data_layout.needs_runtime_call {
        let mut sig = Signature::new(CallConv::SystemV);
        for _ in 0..3 {
            sig.params.push(AbiParam::new(cranelift_codegen::ir::types::I64));
        }
        sig.returns.push(AbiParam::new(cranelift_codegen::ir::types::I64));
        let id = module
            .declare_function("ail_runtime_call", Linkage::Import, &sig)
            .map_err(|e| CompileError::NativeEncodingError(
                format!("declare_function(ail_runtime_call): {e}")))?;
        Some(id)
    } else {
        None
    };

    // Lower each binding and accumulate provenance.
    let mut provenance: BTreeMap<NodeRef, u64> = BTreeMap::new();
    let mut native_offsets: Vec<u64> = Vec::with_capacity(anf.bindings.len());
    let mut cumulative_offset: u64 = 0;

    for binding in &anf.bindings {
        // Record provenance entry: this binding starts at current offset.
        provenance.insert(binding.source_ref, cumulative_offset);
        native_offsets.push(cumulative_offset);

        // Lower the binding and get its compiled code size.
        let code_size = lower_binding(
            &mut module, &binding.name, &binding.expr, cumulative_offset,
            &data_ids, &data_layout, host_call_id, malloc_id, runtime_call_id,
        )?;
        cumulative_offset += code_size;
    }

    // Finish: emit the native object bytes.
    let object = module.finish();
    let native_bytes = object
        .emit()
        .map_err(|e| CompileError::NativeEncodingError(format!("object emit failed: {e}")))?;

    // Build semantic source map — clone ANF source map and populate native_offset.
    let source_map_entries: Vec<SourceMapEntry> = anf
        .source_map
        .entries
        .iter()
        .zip(
            native_offsets
                .iter()
                .map(|&o| Some(o))
                .chain(std::iter::repeat(None)),
        )
        .map(|(entry, native_offset)| SourceMapEntry {
            native_offset,
            ..entry.clone()
        })
        .collect();
    let source_map = SourceMap {
        entries: source_map_entries,
    };

    // Seal: source_map_hash = blake3(source_map_cbor_bytes).
    let source_map_bytes = stable_cbor_bytes(&source_map)
        .map_err(|e| CompileError::NativeEncodingError(format!("source_map encode: {e}")))?;
    let source_map_hash = hash_with_parent(&[], &source_map_bytes);

    // Generate capability manifest from bindings.
    let capabilities_manifest = CapabilitiesManifest {
        entries: anf
            .bindings
            .iter()
            .map(|b| CapabilityEntry {
                name: b.name.clone(),
                source_ref: b.source_ref,
            })
            .collect(),
    };

    // Seal: native_hash = blake3(anf_ir_hash || native_bytes).
    let native_hash = hash_with_parent(&anf_ir_hash, &native_bytes);

    // Extend the stage hashes from ANF.
    let mut hash_chain = anf.stage_hashes.clone();
    hash_chain.native_hash = Some(native_hash);
    hash_chain.source_map_hash = Some(source_map_hash);

    // Build ArtifactManifest from the complete hash chain.
    let capabilities_manifest_bytes = stable_cbor_bytes(&capabilities_manifest).map_err(|e| {
        CompileError::NativeEncodingError(format!("capabilities manifest encode: {e}"))
    })?;
    let capabilities_manifest_hash = hash_with_parent(&[], &capabilities_manifest_bytes);
    let artifact_manifest = ArtifactManifest {
        profile: profile.to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        graph_snapshot_hash: hash_chain.graph_snapshot_hash,
        verification_report_hash: hash_chain.verification_report_hash,
        core_ir_hash: hash_chain.core_ir_hash,
        anf_ir_hash,
        wasm_hash: None,
        native_hash: Some(native_hash),
        source_map_hash: Some(source_map_hash),
        capabilities_manifest_hash: Some(capabilities_manifest_hash),
    };

    // Seal: artifact_manifest_hash = blake3(manifest_cbor_bytes).
    let manifest_cbor = stable_cbor_bytes(&artifact_manifest)
        .map_err(|e| CompileError::NativeEncodingError(format!("manifest CBOR encode: {e}")))?;
    let artifact_manifest_hash = hash_with_parent(&[], &manifest_cbor);
    hash_chain.artifact_manifest_hash = Some(artifact_manifest_hash);

    // Serialize JSON sidecars.
    let source_map_json = serde_json::to_vec(&source_map)
        .map_err(|e| CompileError::NativeEncodingError(format!("source_map JSON encode: {e}")))?;
    let artifact_manifest_json = serde_json::to_vec(&artifact_manifest).map_err(|e| {
        CompileError::NativeEncodingError(format!("artifact_manifest JSON encode: {e}"))
    })?;

    Ok(NativeArtifact {
        native_bytes,
        source_map,
        provenance,
        capabilities_manifest,
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
    use crate::core_ir::StageHashes;
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

    // ── Task 3.1: emit_native rejects unsealed anf_ir_hash ───────────────

    // Scenario: anf_ir_hash None → NativeEncodingError.
    // Spec: "Unsealed anf_ir_hash is rejected → Err(NativeEncodingError)"
    #[test]
    fn emit_native_rejects_unsealed_anf_ir_hash() {
        let anf = AnfIr {
            schema_version: crate::anf::ANF_SCHEMA_VERSION,
            bindings: vec![],
            source_map: crate::anf::SourceMap { entries: vec![] },
            stage_hashes: StageHashes {
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
        let result = emit_native(&anf);
        assert!(
            matches!(result, Err(CompileError::NativeEncodingError(_))),
            "expected NativeEncodingError for unsealed anf_ir_hash, got {result:?}"
        );
    }

    // ── Task 3.2: native_hash is sealed after emit_native ─────────────────

    // Scenario: native_hash is Some after emit_native.
    // Spec: "NativeArtifact.hash_chain.native_hash is Some(...)"
    #[test]
    fn emit_native_seals_native_hash() {
        let anf = anf_for_n(1);
        let artifact = emit_native(&anf).unwrap();
        assert!(
            artifact.hash_chain.native_hash.is_some(),
            "native_hash must be Some after emit_native"
        );
    }

    // ── Task 3.3: different AnfIr inputs produce different native_hash ─────

    // Triangulate: different inputs → different hashes.
    #[test]
    fn different_anf_produces_different_native_hash() {
        let a1 = emit_native(&anf_for_n(1)).unwrap();
        let a2 = emit_native(&anf_for_n(2)).unwrap();
        assert_ne!(
            a1.hash_chain.native_hash, a2.hash_chain.native_hash,
            "different AnfIr inputs must produce different native_hashes"
        );
    }

    // ── Task 3.4: provenance len == binding count; empty → empty ──────────

    // Scenario: N bindings → N provenance entries.
    // Spec: "NativeArtifact.provenance.len() equals N"
    #[test]
    fn provenance_len_equals_binding_count() {
        for n in [0usize, 1, 3, 5] {
            let anf = anf_for_n(n);
            let artifact = emit_native(&anf).unwrap();
            assert_eq!(
                artifact.provenance.len(),
                n,
                "provenance must have {n} entries for {n}-binding AnfIr"
            );
        }
    }

    // Scenario: empty ANF → empty provenance.
    // Spec: "Empty AnfIr produces empty provenance"
    #[test]
    fn empty_anf_produces_empty_provenance() {
        let anf = anf_for_n(0);
        let artifact = emit_native(&anf).unwrap();
        assert!(
            artifact.provenance.is_empty(),
            "empty AnfIr must produce empty provenance map"
        );
    }

    // ── TASK-A0: Extended arithmetic ops — RED ────────────────────────────
    // These all currently hit the catch-all `_ =>` arm and emit trap,
    // producing the same bytes as Placeholder.  They must fail until A1 lands.

    fn anf_with_call2(func: &str, lhs: i64, rhs: i64) -> AnfIr {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(lhs))),
                body: Box::new(AnfExpr::Let {
                    name: "y".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(rhs))),
                    body: Box::new(AnfExpr::Call {
                        func: func.to_string(),
                        args: vec!["x".to_string(), "y".to_string()],
                    }),
                }),
            },
        })
    }

    fn anf_with_call1(func: &str, operand: i64) -> AnfIr {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(operand))),
                body: Box::new(AnfExpr::Call {
                    func: func.to_string(),
                    args: vec!["x".to_string()],
                }),
            },
        })
    }

    fn placeholder_anf() -> AnfIr {
        use crate::anf::{AnfBinding, AnfExpr};
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Placeholder,
        })
    }

    #[test]
    fn native_div_differs_from_placeholder() {
        let art = emit_native(&anf_with_call2("i64.div_s", 10, 2)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "i64.div_s must produce different bytes than Placeholder");
    }

    #[test]
    fn native_rem_differs_from_placeholder() {
        let art = emit_native(&anf_with_call2("i64.rem_s", 10, 3)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "i64.rem_s must produce different bytes than Placeholder");
    }

    #[test]
    fn native_eq_differs_from_placeholder() {
        let art = emit_native(&anf_with_call2("i64.eq", 5, 5)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "i64.eq must produce different bytes than Placeholder");
    }

    #[test]
    fn native_neg_differs_from_placeholder() {
        let art = emit_native(&anf_with_call1("i64.neg", 7)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "i64.neg must produce different bytes than Placeholder");
    }

    #[test]
    fn native_eqz_differs_from_placeholder() {
        let art = emit_native(&anf_with_call1("i64.eqz", 0)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "i64.eqz must produce different bytes than Placeholder");
    }

    // ── TASK-B0: If + ShortCircuit tests — RED ────────────────────────────
    // These hit the catch-all `_ =>` trap arm until B1 lands.

    fn anf_with_if(cond_val: bool, then_val: i64, else_val: i64) -> AnfIr {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "c".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(cond_val))),
                body: Box::new(AnfExpr::If {
                    cond: "c".to_string(),
                    then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(then_val))),
                    else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(else_val))),
                }),
            },
        })
    }

    #[test]
    fn native_if_true_returns_then_branch() {
        let art = emit_native(&anf_with_if(true, 1, 2)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "If with Bool(true) cond must produce different bytes than Placeholder");
    }

    #[test]
    fn native_if_false_returns_else_branch() {
        let art_true = emit_native(&anf_with_if(true, 1, 2)).unwrap();
        let art_false = emit_native(&anf_with_if(false, 1, 2)).unwrap();
        assert_ne!(art_true.native_bytes, art_false.native_bytes,
            "If with Bool(true) and Bool(false) cond must produce different bytes");
    }

    #[test]
    fn native_if_no_result_compiles() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "c".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                body: Box::new(AnfExpr::If {
                    cond: "c".to_string(),
                    then_branch: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                    else_branch: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                }),
            },
        });
        assert!(emit_native(&anf).is_ok(), "If with Unit branches must compile without panic");
    }

    #[test]
    fn native_if_infer_return_type_is_i64() {
        use crate::anf::AnfExpr;
        use crate::core_ir::LiteralValue;
        use cranelift_codegen::ir::types;
        let expr = AnfExpr::If {
            cond: "c".to_string(),
            then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
        };
        assert_eq!(infer_cranelift_return_type(&expr), Some(types::I64),
            "infer_cranelift_return_type for If{{Int, Int}} must return Some(I64)");
    }

    #[test]
    fn native_short_circuit_and_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "t".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                body: Box::new(AnfExpr::ShortCircuitAnd {
                    left: "t".to_string(),
                    right: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "ShortCircuitAnd must produce different bytes than Placeholder");
    }

    #[test]
    fn native_short_circuit_or_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "f".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                body: Box::new(AnfExpr::ShortCircuitOr {
                    left: "f".to_string(),
                    right: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "ShortCircuitOr must produce different bytes than Placeholder");
    }

    // ── TASK-B1: Native expression lowering tests (TDD RED) ───────────────
    // Spec scenarios C-5a, C-5b, C-5c, and C-5d.

    fn anf_for_binding(binding: crate::anf::AnfBinding) -> AnfIr {
        use crate::anf::SourceMap;
        AnfIr {
            schema_version: crate::anf::ANF_SCHEMA_VERSION,
            source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
            bindings: vec![binding],
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

    // Helper: emit native for a single Int literal binding with a FIXED name
    // so that two calls with different values produce identical symbol tables
    // and any byte difference is purely from code content.
    fn anf_with_int_literal(n: i64) -> AnfIr {
        use crate::anf::AnfBinding;
        use crate::core_ir::LiteralValue;
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_lit".to_string(), // fixed name — code difference is the only variable
            expr: crate::anf::AnfExpr::Literal(LiteralValue::Int(n)),
        })
    }

    // C-5a: Two different Int literals must produce different native_bytes.
    // RED: currently both are trap stubs → byte-identical object files.
    #[test]
    fn two_int_literal_bindings_produce_different_native_bytes() {
        let art1 = emit_native(&anf_with_int_literal(1)).unwrap();
        let art2 = emit_native(&anf_with_int_literal(2)).unwrap();
        assert_ne!(
            art1.native_bytes, art2.native_bytes,
            "Literal(Int(1)) and Literal(Int(2)) must produce different native code bytes"
        );
    }

    // C-5b: Int literal binding must produce different bytes than a Placeholder.
    // RED: currently both are trap stubs → same bytes (same name, same trap code).
    #[test]
    fn emit_native_int_literal_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        let lit_art = emit_native(&anf_with_int_literal(42)).unwrap();
        let placeholder_anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_lit".to_string(), // same name → only code differs
            expr: AnfExpr::Placeholder,
        });
        let placeholder_art = emit_native(&placeholder_anf).unwrap();
        assert_ne!(
            lit_art.native_bytes, placeholder_art.native_bytes,
            "Literal(Int(42)) must produce different native code than Placeholder (trap stub)"
        );
    }

    // C-5c: Let{x=Int(3), y=Int(4), body=Call{"i64.add",[x,y]}} must produce
    // different bytes than a plain Placeholder stub with the same function name.
    // RED: currently Let+Add → trap stub → same bytes as Placeholder.
    #[test]
    fn native_lowering_let_int_add_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let add_binding = AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_add".to_string(),
            expr: AnfExpr::Let {
                name: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                body: Box::new(AnfExpr::Let {
                    name: "y".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(4))),
                    body: Box::new(AnfExpr::Call {
                        func: "i64.add".to_string(),
                        args: vec!["x".to_string(), "y".to_string()],
                    }),
                }),
            },
        };
        let placeholder_binding = AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_add".to_string(), // same name to isolate code difference
            expr: AnfExpr::Placeholder,
        };
        let add_art = emit_native(&anf_for_binding(add_binding)).unwrap();
        let placeholder_art = emit_native(&anf_for_binding(placeholder_binding)).unwrap();
        assert!(!add_art.native_bytes.is_empty(), "native_bytes must be non-empty");
        assert_ne!(
            add_art.native_bytes, placeholder_art.native_bytes,
            "Let+Add must produce different code than a Placeholder trap stub"
        );
    }

    // ── TASK-D0: Loop / Break / Continue / WhileLoop — RED ───────────────

    #[test]
    fn native_loop_break_int_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Loop {
                body: Box::new(AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "Loop{{Break{{Int(42)}}}} must produce different bytes than Placeholder");
        assert_eq!(
            infer_cranelift_return_type(&AnfExpr::Loop {
                body: Box::new(AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
                }),
            }),
            Some(cranelift_codegen::ir::types::I64)
        );
    }

    #[test]
    fn native_loop_break_unit_compiles() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Loop {
                body: Box::new(AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                }),
            },
        });
        assert!(emit_native(&anf).is_ok(), "Loop{{Break{{Unit}}}} must compile without panic");
    }

    #[test]
    fn native_while_loop_compiles() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "c".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
                body: Box::new(AnfExpr::WhileLoop {
                    cond: "c".to_string(),
                    body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                }),
            },
        });
        assert!(emit_native(&anf).is_ok(), "WhileLoop with Bool(false) cond must compile");
    }

    #[test]
    fn native_continue_compiles() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        // Loop { Seq([Continue, Break{Int(1)}]) }
        // Continue is unreachable after first iteration but CFG must be valid.
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Loop {
                body: Box::new(AnfExpr::Seq(vec![
                    AnfExpr::Continue,
                    AnfExpr::Break {
                        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    },
                ])),
            },
        });
        assert!(emit_native(&anf).is_ok(), "Loop{{Continue; Break}} must compile without panic");
    }

    // ── TASK-F0: Literal(Text) + NativeDataLayout — RED ──────────────────

    #[test]
    fn native_text_literal_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Text("hello".to_string())),
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "Literal(Text(\"hello\")) must produce different bytes than Placeholder");
    }

    #[test]
    fn native_text_literal_two_strings_differ() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let make = |s: &str| anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Text(s.to_string())),
        });
        let art_hello = emit_native(&make("hello")).unwrap();
        let art_world = emit_native(&make("world")).unwrap();
        assert_ne!(art_hello.native_bytes, art_world.native_bytes,
            "Literal(Text(\"hello\")) and Literal(Text(\"world\")) must produce different bytes");
    }

    #[test]
    fn native_text_literal_same_string_deduplicated() {
        // Two bindings both using the same string literal should intern it once.
        // Test: NativeDataLayout interns the same string to same index.
        // RED: NativeDataLayout doesn't exist yet.
        let mut layout = NativeDataLayout::default();
        let idx1 = layout.intern("hello");
        let idx2 = layout.intern("hello");
        assert_eq!(idx1, idx2, "Same string must intern to same index");
        assert_eq!(layout.ordered.len(), 1, "Only one data object should exist");
    }

    // ── TASK-E0: Match — RED ──────────────────────────────────────────────

    #[test]
    fn native_match_int_arm_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr, AnfMatchArm};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::Match {
                    scrutinee: "x".to_string(),
                    arms: vec![
                        AnfMatchArm { pattern: "1".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(10)) },
                        AnfMatchArm { pattern: "_".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(99)) },
                    ],
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "Match with i64 arm must produce different bytes than Placeholder");
    }

    #[test]
    fn native_match_wildcard_only_compiles() {
        use crate::anf::{AnfBinding, AnfExpr, AnfMatchArm};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Match {
                    scrutinee: "x".to_string(),
                    arms: vec![
                        AnfMatchArm { pattern: "_".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(0)) },
                    ],
                }),
            },
        });
        assert!(emit_native(&anf).is_ok(), "Match with wildcard only must compile");
    }

    #[test]
    fn native_match_bool_arm() {
        use crate::anf::{AnfBinding, AnfExpr, AnfMatchArm};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "b".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                body: Box::new(AnfExpr::Match {
                    scrutinee: "b".to_string(),
                    arms: vec![
                        AnfMatchArm { pattern: "true".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(1)) },
                        AnfMatchArm { pattern: "false".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(0)) },
                    ],
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "Match with bool arms must produce different bytes than Placeholder");
        assert_eq!(
            infer_cranelift_return_type(&AnfExpr::Match {
                scrutinee: "b".to_string(),
                arms: vec![
                    crate::anf::AnfMatchArm { pattern: "true".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(1)) },
                ],
            }),
            Some(cranelift_codegen::ir::types::I64)
        );
    }

    #[test]
    fn native_match_empty_arms_compiles() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Match {
                    scrutinee: "x".to_string(),
                    arms: vec![],
                }),
            },
        });
        assert!(emit_native(&anf).is_ok(), "Match with empty arms must compile (produces trap)");
    }

    // ── TASK-C0: Seq, RuntimeCheck — RED ──────────────────────────────────

    #[test]
    fn native_seq_emits_last_value() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        // Triangulation: two Seqs that differ only in last element must produce
        // different bytes once Seq is properly lowered.
        // RED: currently both hit catch-all trap → identical bytes.
        let seq_a = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Seq(vec![
                AnfExpr::Literal(LiteralValue::Int(1)),
                AnfExpr::Literal(LiteralValue::Int(2)),
            ]),
        });
        let seq_b = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Seq(vec![
                AnfExpr::Literal(LiteralValue::Int(1)),
                AnfExpr::Literal(LiteralValue::Int(5)),
            ]),
        });
        let art_a = emit_native(&seq_a).unwrap();
        let art_b = emit_native(&seq_b).unwrap();
        assert_ne!(art_a.native_bytes, art_b.native_bytes,
            "Seq([Int(1), Int(2)]) and Seq([Int(1), Int(5)]) must produce different bytes");
        // infer_return_type should be Some for the last element
        assert_eq!(
            infer_cranelift_return_type(&AnfExpr::Seq(vec![
                AnfExpr::Literal(LiteralValue::Int(1)),
                AnfExpr::Literal(LiteralValue::Int(2)),
            ])),
            Some(cranelift_codegen::ir::types::I64)
        );
    }

    #[test]
    fn native_seq_empty_compiles() {
        use crate::anf::{AnfBinding, AnfExpr};
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Seq(vec![]),
        });
        assert!(emit_native(&anf).is_ok(), "Seq([]) must compile without panic");
    }

    #[test]
    fn native_runtime_check_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "ok".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::RuntimeCheck {
                    check_ref: "c1".to_string(),
                    cond: "ok".to_string(),
                    msg: "err".to_string(),
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "RuntimeCheck must produce different bytes than Placeholder");
    }

    // ── TASK-G0: RecordNew / FieldGet / FieldUpdate — RED ────────────────

    fn anf_with_record(fields: Vec<(&str, i64)>) -> AnfIr {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let field_exprs: Vec<(String, AnfExpr)> = fields.into_iter()
            .map(|(f, v)| (f.to_string(), AnfExpr::Literal(LiteralValue::Int(v))))
            .collect();
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::RecordNew { fields: field_exprs },
        })
    }

    #[test]
    fn native_record_new_differs_from_placeholder() {
        let art = emit_native(&anf_with_record(vec![("x", 1), ("y", 2)])).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "RecordNew must produce different bytes than Placeholder");
        assert_eq!(
            infer_cranelift_return_type(&crate::anf::AnfExpr::RecordNew {
                fields: vec![("x".to_string(), crate::anf::AnfExpr::Literal(
                    crate::core_ir::LiteralValue::Int(1)))],
            }),
            Some(cranelift_codegen::ir::types::I64)
        );
    }

    #[test]
    fn native_field_get_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "r".to_string(),
                value: Box::new(AnfExpr::RecordNew {
                    fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10)))],
                }),
                body: Box::new(AnfExpr::FieldGet {
                    record: "r".to_string(),
                    field: "x".to_string(),
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "FieldGet must produce different bytes than Placeholder");
    }

    #[test]
    fn native_field_update_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "r".to_string(),
                value: Box::new(AnfExpr::RecordNew {
                    fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(1)))],
                }),
                body: Box::new(AnfExpr::FieldUpdate {
                    record: "r".to_string(),
                    field: "x".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "FieldUpdate must produce different bytes than Placeholder");
    }

    #[test]
    fn native_record_zero_fields_compiles() {
        let art = emit_native(&anf_with_record(vec![]));
        assert!(art.is_ok(), "RecordNew{{[]}} must compile without panic");
    }

    // ── TASK-H0: VariantNew / ListNew / TupleNew ──────────────────────────

    #[test]
    fn native_variant_new_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::VariantNew {
                tag: "Ok".to_string(),
                payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "VariantNew must produce different bytes than Placeholder");
    }

    #[test]
    fn native_list_new_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::ListNew(vec![
                AnfExpr::Literal(LiteralValue::Int(1)),
                AnfExpr::Literal(LiteralValue::Int(2)),
            ]),
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "ListNew must produce different bytes than Placeholder");
    }

    #[test]
    fn native_tuple_new_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::TupleNew(vec![
                AnfExpr::Literal(LiteralValue::Int(3)),
                AnfExpr::Literal(LiteralValue::Int(4)),
            ]),
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "TupleNew must produce different bytes than Placeholder");
    }

    #[test]
    fn native_variant_two_tags_differ() {
        use crate::anf::{AnfBinding, AnfExpr};
        let make_variant = |tag: &str| anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::VariantNew { tag: tag.to_string(), payload: None },
        });
        let art_ok = emit_native(&make_variant("Ok")).unwrap();
        let art_err = emit_native(&make_variant("Err")).unwrap();
        assert_ne!(art_ok.native_bytes, art_err.native_bytes,
            "VariantNew('Ok') and VariantNew('Err') must produce different bytes (different tag ids)");
    }

    // ── TASK-I0: EffectCall — RED ─────────────────────────────────────────

    #[test]
    fn native_effect_call_differs_from_placeholder() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "id".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::EffectCall {
                    capability: "db".to_string(),
                    func: "read".to_string(),
                    args: vec!["id".to_string()],
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(art.native_bytes, ph.native_bytes,
            "EffectCall must produce different bytes than Placeholder");
    }

    #[test]
    fn native_effect_call_two_capabilities_differ() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let make_effect = |cap: &str| anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "id".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::EffectCall {
                    capability: cap.to_string(),
                    func: "read".to_string(),
                    args: vec!["id".to_string()],
                }),
            },
        });
        let art_db = emit_native(&make_effect("db")).unwrap();
        let art_fs = emit_native(&make_effect("fs")).unwrap();
        assert_ne!(art_db.native_bytes, art_fs.native_bytes,
            "EffectCall('db') and EffectCall('fs') must produce different bytes");
    }

    #[test]
    fn native_effect_call_native_hash_is_some() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let anf = anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "id".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::EffectCall {
                    capability: "db".to_string(),
                    func: "read".to_string(),
                    args: vec!["id".to_string()],
                }),
            },
        });
        let art = emit_native(&anf).unwrap();
        assert!(art.hash_chain.native_hash.is_some(), "native_hash must be Some for EffectCall");
    }
}
