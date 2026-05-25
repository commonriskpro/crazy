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
//   - `AnfExpr::Literal(Bool(b))` → `i64.const 0|1` + `local.set`
//   - `AnfExpr::Literal(Float(f))` → `f64.const f`
//   - `AnfExpr::Literal(Text)` → packed `(ptr, len)` i64 into linear memory
//   - `AnfExpr::Literal(Unit)` → `i32.const 0`
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
//
// # Module layout
//
// The implementation is split across focused sibling modules:
//   - `wasm_sections.rs` — pure section builders (type/func/export/import/…)
//   - `wasm_emit.rs`     — ANF→instruction emitter and code-section builder
//
// Tests live in `wasm_tests.rs`.

use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;
use wasm_encoder::Module;

use crate::anf::{AnfIr, SourceMap, SourceMapEntry};
use crate::artifact_manifest::ArtifactManifest;
use crate::capabilities::CapabilitiesManifest;
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
// Public re-exports: maintain the pre-existing surface of `ail_compiler::wasm`.
pub use crate::wasm_abi::{
    ABI_VERSION, AbiDescriptor, WasmScalarType, WasmTypeDescriptor, derive_wasm_type,
};
pub use crate::wasm_artifact::WasmArtifact;

use crate::wasm_abi::{EffectDataLayout, binding_result, binding_signatures, export_name};
use crate::wasm_artifact::code_entry_offsets;
use crate::wasm_emit::build_code_section;
use crate::wasm_sections::{
    align_to_i64, build_data_section, build_export_section_with_memory, build_function_section,
    build_global_section, build_import_section, build_memory_section, build_type_section,
    build_type_section_with_host_call,
};

#[cfg(test)]
#[path = "wasm_tests.rs"]
mod tests;

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
    let needs_host_call_write = effect_data.needs_host_call_write;
    let needs_resource_call = effect_data.needs_resource_call;
    // ResourceAcquire sets `needs_memory = true` directly in `collect_expr`
    // (it interns the resource name string and writes the args buffer in linear
    // memory).  ResourceRelease only passes an i64 handle — no memory access —
    // so `needs_resource_call` is NOT folded in here; doing so would
    // over-provision a memory section for ResourceRelease-only modules.
    let needs_memory = effect_data.needs_host_call || effect_data.needs_memory;
    // type_offset / function_offset: bindings start after all imported function entries.
    // Import order:
    //   [0]  ail/host_call          (if needs_host_call)
    //   [1]  ail/host_call_write    (if needs_host_call_write)
    //   [N]  ail/resource_acquire   (if needs_resource_call)
    //   [N+1] ail/resource_release  (if needs_resource_call)
    let type_offset =
        needs_host_call as u32 + needs_host_call_write as u32 + needs_resource_call as u32 * 2;
    let function_offset = type_offset;

    // Assemble WASM module first so we can compute byte offsets.
    let mut module = Module::new();
    if needs_host_call || needs_resource_call {
        module.section(&build_type_section_with_host_call(
            &signatures,
            needs_host_call,
            needs_host_call_write,
            needs_resource_call,
        ));
    } else if let Some(types) = build_type_section(&signatures) {
        module.section(&types);
    }
    if let Some(imports) =
        build_import_section(needs_host_call, needs_host_call_write, needs_resource_call)
    {
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
    source_map.validate_required_provenance(profile, &anf.bindings)?;

    // Seal: source_map_hash = blake3(source_map_cbor_bytes).
    let source_map_bytes = stable_cbor_bytes(&source_map)?;
    let source_map_hash = hash_with_parent(&[], &source_map_bytes);

    // Seal: wasm_hash = blake3(anf_ir_hash || wasm_binary).
    let wasm_hash = hash_with_parent(&anf_ir_hash, &wasm);

    // Extend the stage hashes from ANF.
    let mut hash_chain = anf.stage_hashes.clone();
    hash_chain.wasm_hash = Some(wasm_hash);
    hash_chain.source_map_hash = Some(source_map_hash);

    // Build capability manifest from bindings — one entry per AnfBinding.
    let capabilities_manifest = CapabilitiesManifest::from_bindings(&anf.bindings);

    // Seal: capabilities_manifest_hash = blake3(cbor(capabilities_manifest)).
    // Uses the same manifest bytes that are stored in WasmArtifact so the hash
    // is consistent with the real manifest (not a proxy over raw bindings).
    let capabilities_manifest_bytes = stable_cbor_bytes(&capabilities_manifest)?;
    let capabilities_manifest_hash = hash_with_parent(&[], &capabilities_manifest_bytes);

    // Build ArtifactManifest from the complete hash chain.
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

    // Populate export_types: for each exported binding, derive its descriptor.
    let mut export_types: BTreeMap<String, WasmTypeDescriptor> = BTreeMap::new();
    for binding in &anf.bindings {
        if binding_result(binding).is_some() {
            let name = export_name(&binding.name);
            let descriptor = derive_wasm_type(&binding.expr);
            export_types.insert(name, descriptor);
        }
    }

    let result_buffer_offset = if effect_data.needs_host_call_write {
        Some(effect_data.result_buffer_offset)
    } else {
        None
    };

    let abi_descriptor = AbiDescriptor::new(export_types.clone());

    Ok(WasmArtifact {
        wasm,
        source_map,
        provenance,
        capabilities_manifest,
        hash_chain,
        artifact_manifest,
        source_map_json,
        artifact_manifest_json,
        export_types,
        abi_descriptor,
        result_buffer_offset,
    })
}
