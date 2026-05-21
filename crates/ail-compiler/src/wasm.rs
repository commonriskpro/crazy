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
// # What is emitted (Phase 7)
//
// Every `AnfBinding` becomes a WASM function stub:
//   - Type: `() -> ()` (no parameters, no results).
//   - Body: `[unreachable, end]`.
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
// # What this stage does NOT do (Phase 7)
//
// - No expression / body codegen (deferred to Phase 8).
// - No optimization.
// - No runtime / Wasmtime dependency.

use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;
use wasm_encoder::{CodeSection, Function, FunctionSection, Module, TypeSection};

use crate::anf::AnfIr;
use crate::core_ir::StageHashes;
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

// ── build_code_section ────────────────────────────────────────────────────

/// Build a code section where every function body is `[unreachable, end]`.
///
/// Returns `None` when `n_functions == 0`.
fn build_code_section(n_functions: usize) -> Option<CodeSection> {
    if n_functions == 0 {
        return None;
    }
    let mut codes = CodeSection::new();
    for _ in 0..n_functions {
        let mut f = Function::new(vec![]); // no locals
        f.instructions().unreachable().end();
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
    if let Some(codes) = build_code_section(n) {
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
        VerificationReport::new(vec![])
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
            bindings: vec![],
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
