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
// # What is emitted (Phase 17 + Phase 8 expression lowering)
//
// Every `AnfBinding` becomes a native function with real Cranelift IR for
// the current Phase 8 subset: arithmetic, control-flow, loops, match, text
// literals, records/variants/lists/tuples, EffectCall, and Lambda.
// Lambda with no captures → bare function pointer (I64).
// Lambda with captures → heap-allocated closure env carrying the function
//   pointer and each captured value by value (layout: [fn_ptr: i64,
//   cap_count: i64, cap0: i64, ...]).  Captures are not silently dropped.
//   Closure invocation is deferred to Phase 9+.
// Concurrency and resource ops dispatch via imported `ail_runtime_call`.
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
    ir::{AbiParam, Signature},
    isa::CallConv,
    settings,
};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::anf::{AnfIr, SourceMap, SourceMapEntry, SourceMapSpan};
use crate::artifact_manifest::ArtifactManifest;
// Shared capability manifest types — defined in `capabilities.rs`.
pub use crate::capabilities::{CapabilitiesManifest, CapabilityEntry};
use crate::core_ir::StageHashes;
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
// Cranelift expression lowering helpers — see `native_codegen.rs`.
#[cfg(test)]
pub(crate) use crate::native_codegen::infer_cranelift_return_type;
// Binding-level compilation — see `native_binding.rs`.
pub use crate::native_abi::{
    NativeAbiDiagnostic, NativeAbiIssue, NativeAbiIssueCategory, NativeAbiIssueCode,
    validate_native_abi,
};
use crate::native_binding::{LowerBindingEnv, lower_binding, native_export_name};
// Shared data-layout type — see `native_types.rs`.
pub use crate::native_types::NativeDataLayout;

// ── NativeArtifact ────────────────────────────────────────────────────────

/// Output of the native backend stage: a platform-native object file with
/// provenance, a capabilities manifest, and a fully sealed hash chain.
///
/// Phase 8 expression lowering is implemented for the current subset:
/// arithmetic, control-flow, loops, match, text literals,
/// records/variants/lists/tuples, EffectCall, and Lambda.
/// Lambda with no captures returns a bare function pointer; Lambda with
/// captures returns a heap-allocated closure env (fn_ptr + captured values
/// by value).  Closure invocation is deferred to Phase 9+.
/// Concurrency and resource ops dispatch via imported `ail_runtime_call`;
/// the runtime implementation is deferred to Phase 9+.
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

fn native_generated_span(
    offsets: &[u64],
    index: usize,
    total_code_size: u64,
) -> Option<SourceMapSpan> {
    let start = *offsets.get(index)?;
    let end = offsets.get(index + 1).copied().unwrap_or(total_code_size);
    Some(SourceMapSpan::new("program.o", start, end))
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

    validate_native_abi(anf).into_result()?;

    let export_names = native_export_names(&anf.bindings)?;

    let isa = build_isa()?;
    let mut module = build_object_module(isa)?;

    // Pre-scan all bindings to build the data layout and detect EffectCall.
    let data_layout = NativeDataLayout::for_bindings(&anf.bindings);
    let data_ids = data_layout.define_all(&mut module)?;
    // Define byte-buffer data objects for LiteralValue::Bytes literals.
    let bytes_data_ids = data_layout.define_all_bytes(&mut module)?;

    // If any binding uses EffectCall, declare the imported host_call function.
    // Signature: (cap_ptr: I64, cap_len: I64, op_ptr: I64, op_len: I64,
    //             args_ptr: I64, args_len: I64) -> I64
    let host_call_id: Option<FuncId> = if data_layout.needs_host_call {
        let mut sig = Signature::new(CallConv::SystemV);
        for _ in 0..6 {
            sig.params
                .push(AbiParam::new(cranelift_codegen::ir::types::I64));
        }
        sig.returns
            .push(AbiParam::new(cranelift_codegen::ir::types::I64));
        let id = module
            .declare_function("host_call", Linkage::Import, &sig)
            .map_err(|e| {
                CompileError::NativeEncodingError(format!("declare_function(host_call): {e}"))
            })?;
        Some(id)
    } else {
        None
    };

    // If any binding allocates compound values on the heap, declare malloc.
    // Signature: (size: I64) -> I64  (returns opaque heap pointer)
    let malloc_id: Option<FuncId> = if data_layout.needs_heap_alloc {
        let mut sig = Signature::new(CallConv::SystemV);
        sig.params
            .push(AbiParam::new(cranelift_codegen::ir::types::I64));
        sig.returns
            .push(AbiParam::new(cranelift_codegen::ir::types::I64));
        let id = module
            .declare_function("__ail_malloc", Linkage::Import, &sig)
            .map_err(|e| {
                CompileError::NativeEncodingError(format!("declare_function(__ail_malloc): {e}"))
            })?;
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
            sig.params
                .push(AbiParam::new(cranelift_codegen::ir::types::I64));
        }
        sig.returns
            .push(AbiParam::new(cranelift_codegen::ir::types::I64));
        let id = module
            .declare_function("ail_runtime_call", Linkage::Import, &sig)
            .map_err(|e| {
                CompileError::NativeEncodingError(format!(
                    "declare_function(ail_runtime_call): {e}"
                ))
            })?;
        Some(id)
    } else {
        None
    };

    // Lower each binding and accumulate provenance.
    let mut provenance: BTreeMap<NodeRef, u64> = BTreeMap::new();
    let mut native_offsets: Vec<u64> = Vec::with_capacity(anf.bindings.len());
    let mut cumulative_offset: u64 = 0;

    for (binding, export_name) in anf.bindings.iter().zip(export_names.iter()) {
        // Record provenance entry: this binding starts at current offset.
        provenance.insert(binding.source_ref, cumulative_offset);
        native_offsets.push(cumulative_offset);

        // Lower the binding and get its compiled code size.
        let code_size = lower_binding(
            &mut module,
            export_name,
            &binding.expr,
            LowerBindingEnv {
                data_ids: &data_ids,
                data_layout: &data_layout,
                bytes_data_ids: &bytes_data_ids,
                host_call_id,
                malloc_id,
                runtime_call_id,
            },
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
        .enumerate()
        .map(|(index, (entry, native_offset))| SourceMapEntry {
            native_offset,
            generated_span: native_offset
                .and_then(|_| native_generated_span(&native_offsets, index, cumulative_offset)),
            ..entry.clone()
        })
        .collect();
    let source_map = SourceMap {
        entries: source_map_entries,
    };
    source_map.validate_tooling_quality()?;
    source_map.validate_required_provenance(profile, &anf.bindings)?;

    // Seal: source_map_hash = blake3(source_map_cbor_bytes).
    let source_map_bytes = stable_cbor_bytes(&source_map)
        .map_err(|e| CompileError::NativeEncodingError(format!("source_map encode: {e}")))?;
    let source_map_hash = hash_with_parent(&[], &source_map_bytes);

    // Generate capability manifest from bindings.
    let capabilities_manifest = CapabilitiesManifest::from_bindings(&anf.bindings);

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

fn native_export_names(bindings: &[crate::anf::AnfBinding]) -> Result<Vec<String>, CompileError> {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    let mut export_names = Vec::with_capacity(bindings.len());

    for binding in bindings {
        let export_name = native_export_name(&binding.name);
        if let Some(first_binding_name) = seen.insert(export_name.clone(), binding.name.as_str()) {
            return Err(CompileError::NativeEncodingError(format!(
                "duplicate native export name `{export_name}` from bindings `{first_binding_name}` and `{}`",
                binding.name
            )));
        }
        export_names.push(export_name);
    }

    Ok(export_names)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
