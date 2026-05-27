use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;

use wasm_encoder::Module;

use crate::anf::{AnfExpr, AnfIr, SourceMap, SourceMapEntry};

use crate::artifact_manifest::ArtifactManifest;

use crate::capabilities::CapabilitiesManifest;

use crate::error::CompileError;

use crate::hash::{hash_with_parent, stable_cbor_bytes};

use crate::wasm_abi::{binding_result, binding_signatures, export_name, EffectDataLayout, AbiDescriptor, WasmTypeDescriptor, derive_wasm_type};

use crate::wasm_artifact::{code_entry_offsets, WasmArtifact};

use crate::wasm_emit::build_code_section;

use crate::wasm_sections::{align_to_i64, build_data_section, build_element_section, build_export_section_with_memory, build_function_section, build_global_section, build_import_section, build_memory_section, build_table_section, build_type_section, build_type_section_with_host_call};

use super::lambdas::{anf_has_fold, collect_closure_hoistable_lambdas, collect_hoistable_lambdas, expr_contains_2param_lambda, first_unsupported_wasm_construct, has_fold_with_captured_reducer, has_fold_with_uncaptured_wrong_arity_reducer};

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

    // Gate: unsupported WASM constructs.
    // Walk every binding's expression before code generation so callers receive
    // a structured CompileError::UnsupportedWasmConstruct instead of a silent
    // unreachable trap at runtime.
    //
    // Unsupported: Dispatch, TaskSpawn/Await/Cancel/Group (async runtime),
    // ChannelNew/Send/Receive/Select/Timeout (channel runtime).
    // Note: Fold is now implemented via call_indirect + function table.
    for binding in &anf.bindings {
        if let Some(name) = first_unsupported_wasm_construct(&binding.expr) {
            return Err(CompileError::UnsupportedWasmConstruct(name.to_string()));
        }
    }

    // Gate: Fold reducer that is a captured Lambda with ≠ 2 params (Wave 13B,
    // updated in Wave 16A PR3).
    //
    // 2-param captured Lambdas are now supported via closure hoisting (PR3):
    // they are emitted as `(env_ptr: i64, acc: i64, elem: i64) → i64` WASM
    // functions with the real table index stored in the closure env's fn_idx.
    //
    // Non-2-param captured Lambdas (0, 1, 3+ params) cannot be Fold reducers
    // and still produce a runtime trap.  The gate preserves the compile-time
    // diagnostic for those shapes.
    if has_fold_with_captured_reducer(&anf.bindings) {
        return Err(CompileError::UnsupportedWasmConstruct(
            "FoldWithCapturedReducer".to_string(),
        ));
    }

    // Gate: Fold reducer that is a capture-free Lambda with ≠ 2 params
    // (Wave 26C audit).
    //
    // A capture-free Lambda with wrong arity (0, 1, 3+ params) falls into the
    // non-hoistable `else` branch in `emit_anf_expr` and emits a closure env
    // with `fn_idx = 0` (placeholder).  When a Fold reads this I32 pointer,
    // the I32 dispatch path loads `fn_idx = 0` and issues
    // `call_indirect(closure-reducer type, table 0)` — silently calling
    // `table[0]` with the wrong function and arity rather than trapping
    // deterministically.
    //
    // This gate returns a compile-time diagnostic for that shape so callers
    // receive a structured `UnsupportedWasmConstruct` instead.
    if has_fold_with_uncaptured_wrong_arity_reducer(&anf.bindings) {
        return Err(CompileError::UnsupportedWasmConstruct(
            "FoldWithUncapturedWrongArityReducer".to_string(),
        ));
    }

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

    // Detect whether any binding contains a Fold expression.
    // When true: add a function table, element section, and fold-reducer type.
    let needs_fold = anf.bindings.iter().any(|b| anf_has_fold(&b.expr));

    // Collect nested Lambdas that qualify for hoisting (params == 2, no captures).
    // Only meaningful when needs_fold is true; empty otherwise.
    let hoisted_lambdas: Vec<(Vec<String>, AnfExpr)> = if needs_fold {
        collect_hoistable_lambdas(&anf.bindings)
    } else {
        vec![]
    };
    let n_hoisted = hoisted_lambdas.len() as u32;

    // Collect closure-hoistable Lambdas (params == 2, captures non-empty).
    // Only meaningful when needs_fold is true; empty otherwise.
    // (Wave 16A PR3: general closure dispatch for captured fold reducers.)
    let closure_hoistable_lambdas: Vec<(Vec<String>, Vec<String>, AnfExpr)> = if needs_fold {
        collect_closure_hoistable_lambdas(&anf.bindings)
    } else {
        vec![]
    };
    let n_closure_hoisted = closure_hoistable_lambdas.len() as u32;

    // Gate: W1 — nested 2-param Lambdas inside hoisted Lambda bodies.
    //
    // `collect_hoistable_lambdas` and `collect_closure_hoistable_lambdas` do
    // NOT recurse into Lambda bodies (those become separate WASM functions).
    // A 2-param Lambda nested inside a hoisted or closure-hoisted Lambda body
    // would consume a table index that was never allocated, silently writing an
    // out-of-range index into linear memory — not caught by wasmparser::validate.
    //
    // Reject at compile time until recursive collection/indexing is implemented.
    for (_, body) in &hoisted_lambdas {
        if expr_contains_2param_lambda(body) {
            return Err(CompileError::UnsupportedWasmConstruct(
                "NestedHoistableLambda".to_string(),
            ));
        }
    }
    for (_, _, body) in &closure_hoistable_lambdas {
        if expr_contains_2param_lambda(body) {
            return Err(CompileError::UnsupportedWasmConstruct(
                "NestedClosureHoistableLambda".to_string(),
            ));
        }
    }

    // Total functions in the module = bindings + hoisted + closure-hoisted.
    let n_bindings = anf.bindings.len() as u32;
    let n_functions = n_bindings + n_hoisted + n_closure_hoisted;

    // fold_reducer_type_idx: the type-section index of (i64, i64) → i64.
    // It is appended after all host-import types and binding signatures:
    //   index = type_offset + signatures.len()
    let fold_reducer_type_idx: Option<u32> = if needs_fold {
        Some(type_offset + signatures.len() as u32)
    } else {
        None
    };

    // closure_reducer_type_idx: the type-section index of (i64, i64, i64) → i64.
    // Appended immediately after fold_reducer_type when needs_fold is true.
    // Used by the Fold I32 dispatch path for captured Lambda reducers (PR3).
    //   index = type_offset + signatures.len() + 1   (when needs_fold)
    let closure_reducer_type_idx: Option<u32> = if needs_fold {
        Some(type_offset + signatures.len() as u32 + 1)
    } else {
        None
    };

    // Both needs_fold → needs_closure_reducer: add closure-reducer type whenever
    // there is a Fold, even if no captured Lambdas exist in this module.  The
    // type is unused in that case but its presence is harmless and avoids
    // conditional type-section layout changes that would complicate the index
    // arithmetic.
    let needs_closure_reducer = needs_fold;

    // Assemble WASM module first so we can compute byte offsets.
    // Section order follows the WASM binary format spec:
    //   Type(1) Import(2) Function(3) Table(4) Memory(5) Global(6)
    //   Export(7) Element(9) Code(10) Data(11)
    let mut module = Module::new();
    if needs_host_call || needs_resource_call {
        module.section(&build_type_section_with_host_call(
            &signatures,
            needs_host_call,
            needs_host_call_write,
            needs_resource_call,
            needs_fold,
            needs_closure_reducer,
        ));
    } else if let Some(types) = build_type_section(&signatures, needs_fold, needs_closure_reducer) {
        module.section(&types);
    }
    if let Some(imports) =
        build_import_section(needs_host_call, needs_host_call_write, needs_resource_call)
    {
        module.section(&imports);
    }
    if let Some(functions) = build_function_section(
        &signatures,
        type_offset,
        n_hoisted,
        fold_reducer_type_idx,
        n_closure_hoisted,
        closure_reducer_type_idx,
    ) {
        module.section(&functions);
    }
    // Table section (4): required for call_indirect when Fold is present.
    if needs_fold && let Some(table) = build_table_section(n_functions) {
        module.section(&table);
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
    // Element section (9): populates the function table for call_indirect.
    // Must appear after Export and before Code per WASM binary format.
    if needs_fold && let Some(elems) = build_element_section(function_offset, n_functions) {
        module.section(&elems);
    }
    match build_code_section(
        &anf.bindings,
        &effect_data,
        function_offset,
        fold_reducer_type_idx,
        closure_reducer_type_idx,
        &hoisted_lambdas,
        &closure_hoistable_lambdas,
    ) {
        Ok(Some(codes)) => {
            module.section(&codes);
        }
        Ok(None) => {}
        Err(e) => return Err(e),
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
