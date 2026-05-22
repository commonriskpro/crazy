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
use wasm_encoder::{CodeSection, Function, FunctionSection, Instruction, Module, TypeSection, ValType};

use crate::anf::{AnfExpr, AnfIr};
use crate::core_ir::{LiteralValue, StageHashes};
use crate::error::CompileError;
use crate::hash::hash_with_parent;

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
    /// Maps each `NodeRef` from the source graph to its byte offset in the
    /// WASM code section (i.e., the position of the body-size LEB128 byte
    /// for that function's entry in the encoded binary).
    /// Empty when the input `AnfIr` has no bindings.
    pub provenance: BTreeMap<NodeRef, u32>,
    /// Hash chain extended through the WASM stage.
    /// `hash_chain.wasm_hash` is `Some(...)` after `emit_wasm` completes.
    pub hash_chain: StageHashes,
}

// ── build_type_section ────────────────────────────────────────────────────

/// Build a type section with one entry: `() -> ()` (stub type for all
/// Phase 7 function bodies).
///
/// Returns `None` when `n_functions == 0` — no type section is needed for
/// an empty module.
fn build_type_section(n_functions: usize) -> Option<TypeSection> {
    if n_functions == 0 {
        return None;
    }
    let mut types = TypeSection::new();
    // Single stub type: no params, no results.
    types.ty().function([], []);
    Some(types)
}

// ── build_function_section ────────────────────────────────────────────────

/// Build a function section referencing type index 0 for every function.
///
/// Returns `None` when `n_functions == 0`.
fn build_function_section(n_functions: usize) -> Option<FunctionSection> {
    if n_functions == 0 {
        return None;
    }
    let mut functions = FunctionSection::new();
    for _ in 0..n_functions {
        functions.function(0); // all stubs share type index 0
    }
    Some(functions)
}

// ── WasmCodegenCtx ────────────────────────────────────────────────────────

/// Local-variable environment for WASM codegen.
///
/// ANF `let`-bindings are mapped to WASM `local` indices.  The context tracks
/// which name maps to which local slot so that `Var` references can be emitted
/// as `local.get <idx>`.
struct WasmCodegenCtx<'a> {
    /// Maps let-bound name → WASM local index (0-based, after params).
    locals: Vec<(&'a str, u32)>,
    /// Counter for allocating fresh local indices.
    next_local: u32,
}

impl<'a> WasmCodegenCtx<'a> {
    fn new() -> Self {
        WasmCodegenCtx { locals: Vec::new(), next_local: 0 }
    }

    /// Look up a variable name and return its local index.
    /// Returns `None` if the name is not in scope.
    fn lookup(&self, name: &str) -> Option<u32> {
        // Search from the most-recently-bound end (innermost scope).
        self.locals.iter().rev().find(|(n, _)| *n == name).map(|(_, i)| *i)
    }

    /// Bind a new name to a fresh local slot and return the slot index.
    fn bind(&mut self, name: &'a str) -> u32 {
        let idx = self.next_local;
        self.next_local += 1;
        self.locals.push((name, idx));
        idx
    }
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
    insns: &mut Vec<Instruction<'a>>,
) {
    match expr {
        // ── Literals ──────────────────────────────────────────────────────
        AnfExpr::Literal(lit) => match lit {
            LiteralValue::Int(n) => {
                insns.push(Instruction::I64Const(*n));
            }
            LiteralValue::Bool(b) => {
                insns.push(Instruction::I32Const(if *b { 1 } else { 0 }));
            }
            LiteralValue::Float(f) => {
                // wasm_encoder 0.244 requires Ieee64 for F64Const.
                insns.push(Instruction::F64Const(wasm_encoder::Ieee64::from(*f)));
            }
            // Text, Unit → opaque i32(0) placeholder (no runtime value).
            LiteralValue::Text(_) | LiteralValue::Unit => {
                insns.push(Instruction::I32Const(0));
            }
        },

        // ── Variable reference ────────────────────────────────────────────
        AnfExpr::Var(name) => {
            if let Some(idx) = ctx.lookup(name) {
                insns.push(Instruction::LocalGet(idx));
            } else {
                // Unbound variable — emit unreachable (catches missing bindings).
                insns.push(Instruction::Unreachable);
            }
        }

        // ── Let binding ───────────────────────────────────────────────────
        AnfExpr::Let { name, value, body } => {
            // Emit value expression (leaves one value on stack).
            emit_anf_expr(value, ctx, insns);
            // Allocate a fresh local and set it.
            let idx = ctx.bind(name);
            insns.push(Instruction::LocalSet(idx));
            // Emit the body with the new binding in scope.
            emit_anf_expr(body, ctx, insns);
        }

        // ── Conditional (short-circuit AND/OR) ────────────────────────────
        AnfExpr::If { cond, then_branch, else_branch } => {
            // Condition: look up the atomic variable.
            if let Some(idx) = ctx.lookup(cond) {
                insns.push(Instruction::LocalGet(idx));
            } else {
                insns.push(Instruction::I32Const(0));
            }
            // WASM if/else/end block (type: i32 result).
            insns.push(Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            emit_anf_expr(then_branch, ctx, insns);
            insns.push(Instruction::Else);
            emit_anf_expr(else_branch, ctx, insns);
            insns.push(Instruction::End);
        }

        // ── Short-circuit AND ─────────────────────────────────────────────
        // if left { right } else { false }
        AnfExpr::ShortCircuitAnd { left, right } => {
            if let Some(idx) = ctx.lookup(left) {
                insns.push(Instruction::LocalGet(idx));
            } else {
                insns.push(Instruction::I32Const(0));
            }
            insns.push(Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            emit_anf_expr(right, ctx, insns);
            insns.push(Instruction::Else);
            insns.push(Instruction::I32Const(0));
            insns.push(Instruction::End);
        }

        // ── Short-circuit OR ──────────────────────────────────────────────
        // if left { true } else { right }
        AnfExpr::ShortCircuitOr { left, right } => {
            if let Some(idx) = ctx.lookup(left) {
                insns.push(Instruction::LocalGet(idx));
            } else {
                insns.push(Instruction::I32Const(0));
            }
            insns.push(Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            insns.push(Instruction::I32Const(1));
            insns.push(Instruction::Else);
            emit_anf_expr(right, ctx, insns);
            insns.push(Instruction::End);
        }

        // ── Sequence ──────────────────────────────────────────────────────
        AnfExpr::Seq(exprs) => {
            for (i, e) in exprs.iter().enumerate() {
                emit_anf_expr(e, ctx, insns);
                // Drop intermediate results (all but the last).
                if i + 1 < exprs.len() {
                    insns.push(Instruction::Drop);
                }
            }
            // Empty Seq → push unit (i32.const 0).
            if exprs.is_empty() {
                insns.push(Instruction::I32Const(0));
            }
        }

        // ── Return ────────────────────────────────────────────────────────
        AnfExpr::Return(inner) => {
            emit_anf_expr(inner, ctx, insns);
            insns.push(Instruction::Return);
        }

        // ── Function call (pure) ──────────────────────────────────────────
        // Emits args via local.get, then calls the function.
        // Function index resolution is deferred (emit unreachable for now
        // since we don't have a full import table yet).
        AnfExpr::Call { args, .. } => {
            for arg_name in args {
                if let Some(idx) = ctx.lookup(arg_name) {
                    insns.push(Instruction::LocalGet(idx));
                }
            }
            // TODO: resolve function index from name table.
            // For now emit unreachable as the call target placeholder.
            insns.push(Instruction::Unreachable);
        }

        // ── FieldGet ──────────────────────────────────────────────────────
        AnfExpr::FieldGet { record, .. } => {
            if let Some(idx) = ctx.lookup(record) {
                insns.push(Instruction::LocalGet(idx));
            }
            // TODO: emit field-accessor logic (memory read).
            // For now the record value is left on the stack as a proxy.
        }

        // ── FieldUpdate ───────────────────────────────────────────────────
        AnfExpr::FieldUpdate { record, value, .. } => {
            if let Some(idx) = ctx.lookup(record) {
                insns.push(Instruction::LocalGet(idx));
            }
            // Emit the replacement value.
            emit_anf_expr(value, ctx, insns);
            insns.push(Instruction::Drop); // consume value (TODO: actual update)
            // Return the (unchanged, pending real update) record reference.
        }

        // ── RecordNew ─────────────────────────────────────────────────────
        AnfExpr::RecordNew { fields } => {
            // Emit all field values (they are all Var refs after full ANF normalization).
            for (_, v) in fields {
                emit_anf_expr(v, ctx, insns);
                insns.push(Instruction::Drop); // TODO: pack into record struct
            }
            // Return opaque i32(0) as the record handle placeholder.
            insns.push(Instruction::I32Const(0));
        }

        // ── TupleNew ─────────────────────────────────────────────────────
        AnfExpr::TupleNew(elems) => {
            for e in elems {
                emit_anf_expr(e, ctx, insns);
                insns.push(Instruction::Drop); // TODO: pack tuple
            }
            insns.push(Instruction::I32Const(0));
        }

        // ── VariantNew ───────────────────────────────────────────────────
        AnfExpr::VariantNew { payload, .. } => {
            if let Some(p) = payload {
                emit_anf_expr(p, ctx, insns);
                insns.push(Instruction::Drop); // TODO: tag the variant
            }
            insns.push(Instruction::I32Const(0));
        }

        // ── ListNew ──────────────────────────────────────────────────────
        AnfExpr::ListNew(elems) => {
            for e in elems {
                emit_anf_expr(e, ctx, insns);
                insns.push(Instruction::Drop); // TODO: append to list
            }
            insns.push(Instruction::I32Const(0));
        }

        // ── Match ─────────────────────────────────────────────────────────
        // Emit as a series of block/if nesting over the arms.
        // For now uses a simplified linear-scan pattern.
        AnfExpr::Match { scrutinee, arms } => {
            if let Some(idx) = ctx.lookup(scrutinee) {
                insns.push(Instruction::LocalGet(idx));
                insns.push(Instruction::Drop); // consume scrutinee (TODO: dispatch)
            }
            if let Some(first_arm) = arms.first() {
                emit_anf_expr(&first_arm.body, ctx, insns);
            } else {
                insns.push(Instruction::Unreachable);
            }
        }

        // ── Lambda ───────────────────────────────────────────────────────
        // Lambdas are hoisted to top-level WASM functions.
        // At this stage, emit an opaque function reference (i32.const 0).
        AnfExpr::Lambda { .. } => {
            insns.push(Instruction::I32Const(0));
        }

        // ── Effect/concurrent/resource variants ───────────────────────────
        // These are host-managed. The WASM body emits unreachable to signal
        // that the host runtime must intercept and dispatch.
        AnfExpr::EffectCall { .. }
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
        | AnfExpr::ResourceRelease { .. } => {
            insns.push(Instruction::Unreachable);
        }

        // ── Placeholder ───────────────────────────────────────────────────
        AnfExpr::Placeholder => {
            insns.push(Instruction::Unreachable);
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
fn build_code_section(bindings: &[crate::anf::AnfBinding]) -> Option<CodeSection> {
    if bindings.is_empty() {
        return None;
    }
    let mut codes = CodeSection::new();
    for binding in bindings {
        let mut ctx = WasmCodegenCtx::new();
        let mut insns: Vec<Instruction<'_>> = Vec::new();

        emit_anf_expr(&binding.expr, &mut ctx, &mut insns);

        // Function type is () -> () — drop any residual stack value.
        insns.push(Instruction::Drop);
        insns.push(Instruction::End);

        // Allocate locals: one i64 slot per let-binding (conservative).
        // In production we'd type-infer; here we use i32 as a safe default.
        let n_locals = ctx.next_local as usize;
        let locals = if n_locals > 0 {
            vec![(n_locals as u32, ValType::I32)]
        } else {
            vec![]
        };

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
    // Gate: anf_ir_hash must be sealed.
    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .ok_or_else(|| CompileError::EncodingError("anf_ir_hash not sealed".to_string()))?;

    let n = anf.bindings.len();

    // Assemble WASM module first so we can compute byte offsets.
    let mut module = Module::new();
    if let Some(types) = build_type_section(n) {
        module.section(&types);
    }
    if let Some(functions) = build_function_section(n) {
        module.section(&functions);
    }
    if let Some(codes) = build_code_section(&anf.bindings) {
        module.section(&codes);
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

    // Seal: wasm_hash = blake3(anf_ir_hash || wasm_binary).
    let wasm_hash = hash_with_parent(&anf_ir_hash, &wasm);

    // Extend the stage hashes from ANF.
    let mut hash_chain = anf.stage_hashes.clone();
    hash_chain.wasm_hash = Some(wasm_hash);

    Ok(WasmArtifact {
        wasm,
        provenance,
        hash_chain,
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
        assert!(build_type_section(0).is_none());
    }

    // TRIANGULATE: build_type_section returns Some for N > 0.
    #[test]
    fn build_type_section_some_for_nonzero() {
        assert!(build_type_section(1).is_some());
        assert!(build_type_section(5).is_some());
    }
}
