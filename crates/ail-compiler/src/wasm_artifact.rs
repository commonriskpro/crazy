// ── ail-compiler::wasm_artifact ───────────────────────────────────────────
//
// WASM output artifact and binary-scanning helpers.
//
// `WasmArtifact` is the sealed output of the WASM emission stage.  It bundles
// the binary, the semantic source map, the provenance index, the hash chain,
// the artifact manifest, and JSON-serialized sidecars into a single value.
//
// `leb128_u32` and `code_entry_offsets` are pure byte-level helpers for
// scanning an already-assembled WASM binary to recover per-function byte
// offsets from the code section.

use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;

use crate::anf::SourceMap;
use crate::artifact_manifest::ArtifactManifest;
use crate::core_ir::StageHashes;
use crate::wasm_abi::WasmTypeDescriptor;

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
    /// Maps each exported function name to its `WasmTypeDescriptor`.
    ///
    /// Populated by `emit_wasm` from the expression trees of exported bindings.
    /// Used by the runtime's `invoke_typed` to decode structured return values.
    pub export_types: BTreeMap<String, WasmTypeDescriptor>,
    /// Offset in WASM linear memory where `host_call_write` writes structured
    /// effect call results.
    ///
    /// `Some(offset)` when the module imports `ail/host_call_write` (i.e. at
    /// least one exported binding uses a structured EffectCall).
    /// `None` for modules that do not use `host_call_write`.
    pub result_buffer_offset: Option<i32>,
}

// ── leb128_u32 ────────────────────────────────────────────────────────────

/// Decode one LEB128-encoded unsigned 32-bit integer from `bytes`.
///
/// Returns `(value, bytes_consumed)`.  Panics if `bytes` is empty or the
/// encoding exceeds 5 bytes (which cannot happen for a valid WASM binary).
pub(crate) fn leb128_u32(bytes: &[u8]) -> (u32, usize) {
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
pub(crate) fn code_entry_offsets(wasm: &[u8]) -> Vec<u32> {
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
