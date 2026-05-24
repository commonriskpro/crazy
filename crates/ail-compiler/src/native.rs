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
// literals, records/variants/lists/tuples, EffectCall, and Lambda (params
// bound, body lowered, address returned; no closure capture). Concurrency
// and resource ops dispatch via imported `ail_runtime_call`.
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
    ir::{AbiParam, Function, InstBuilder, Signature, UserFuncName},
    isa::CallConv,
    settings,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use serde::{Deserialize, Serialize};

use crate::anf::{AnfIr, SourceMap, SourceMapEntry};
use crate::artifact_manifest::ArtifactManifest;
use crate::core_ir::StageHashes;
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
// Cranelift expression lowering helpers — see `native_codegen.rs`.
pub(crate) use crate::native_codegen::infer_cranelift_return_type;
use crate::native_codegen::{LowerResult, NativeCodegenCtx, lower_anf_expr_cranelift};
// Shared data-layout type — see `native_types.rs`.
pub use crate::native_types::NativeDataLayout;

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
/// Phase 8 expression lowering is implemented for the current subset:
/// arithmetic, control-flow, loops, match, text literals,
/// records/variants/lists/tuples, EffectCall, and Lambda (params bound, body
/// lowered; no closure capture). Concurrency and resource ops dispatch via
/// imported `ail_runtime_call`; the runtime implementation is deferred to
/// Phase 9+.
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

// ── lower_binding ─────────────────────────────────────────────────────────

struct LowerBindingEnv<'a> {
    data_ids: &'a [DataId],
    data_layout: &'a NativeDataLayout,
    host_call_id: Option<FuncId>,
    malloc_id: Option<FuncId>,
    runtime_call_id: Option<FuncId>,
}

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
    env: LowerBindingEnv<'_>,
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

        let mut codegen_ctx =
            NativeCodegenCtx::new(env.data_ids, env.data_layout, env.host_call_id);
        codegen_ctx.malloc_id = env.malloc_id;
        codegen_ctx.runtime_call_id = env.runtime_call_id;
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

    for binding in &anf.bindings {
        // Record provenance entry: this binding starts at current offset.
        provenance.insert(binding.source_ref, cumulative_offset);
        native_offsets.push(cumulative_offset);

        // Lower the binding and get its compiled code size.
        let code_size = lower_binding(
            &mut module,
            &binding.name,
            &binding.expr,
            LowerBindingEnv {
                data_ids: &data_ids,
                data_layout: &data_layout,
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
        .map(|(entry, native_offset)| SourceMapEntry {
            native_offset,
            ..entry.clone()
        })
        .collect();
    let source_map = SourceMap {
        entries: source_map_entries,
    };
    source_map.validate_required_provenance(profile, &anf.bindings)?;

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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "i64.div_s must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_rem_differs_from_placeholder() {
        let art = emit_native(&anf_with_call2("i64.rem_s", 10, 3)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "i64.rem_s must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_eq_differs_from_placeholder() {
        let art = emit_native(&anf_with_call2("i64.eq", 5, 5)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "i64.eq must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_neg_differs_from_placeholder() {
        let art = emit_native(&anf_with_call1("i64.neg", 7)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "i64.neg must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_eqz_differs_from_placeholder() {
        let art = emit_native(&anf_with_call1("i64.eqz", 0)).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "i64.eqz must produce different bytes than Placeholder"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "If with Bool(true) cond must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_if_false_returns_else_branch() {
        let art_true = emit_native(&anf_with_if(true, 1, 2)).unwrap();
        let art_false = emit_native(&anf_with_if(false, 1, 2)).unwrap();
        assert_ne!(
            art_true.native_bytes, art_false.native_bytes,
            "If with Bool(true) and Bool(false) cond must produce different bytes"
        );
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
        assert!(
            emit_native(&anf).is_ok(),
            "If with Unit branches must compile without panic"
        );
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
        assert_eq!(
            infer_cranelift_return_type(&expr),
            Some(types::I64),
            "infer_cranelift_return_type for If{{Int, Int}} must return Some(I64)"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "ShortCircuitAnd must produce different bytes than Placeholder"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "ShortCircuitOr must produce different bytes than Placeholder"
        );
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
        assert!(
            !add_art.native_bytes.is_empty(),
            "native_bytes must be non-empty"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "Loop{{Break{{Int(42)}}}} must produce different bytes than Placeholder"
        );
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
        assert!(
            emit_native(&anf).is_ok(),
            "Loop{{Break{{Unit}}}} must compile without panic"
        );
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
        assert!(
            emit_native(&anf).is_ok(),
            "WhileLoop with Bool(false) cond must compile"
        );
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
        assert!(
            emit_native(&anf).is_ok(),
            "Loop{{Continue; Break}} must compile without panic"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "Literal(Text(\"hello\")) must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_text_literal_two_strings_differ() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let make = |s: &str| {
            anf_for_binding(AnfBinding {
                source_ref: NodeRef(0),
                name: "fn_op".to_string(),
                expr: AnfExpr::Literal(LiteralValue::Text(s.to_string())),
            })
        };
        let art_hello = emit_native(&make("hello")).unwrap();
        let art_world = emit_native(&make("world")).unwrap();
        assert_ne!(
            art_hello.native_bytes, art_world.native_bytes,
            "Literal(Text(\"hello\")) and Literal(Text(\"world\")) must produce different bytes"
        );
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
                        AnfMatchArm {
                            pattern: "1".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(10)),
                        },
                        AnfMatchArm {
                            pattern: "_".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(99)),
                        },
                    ],
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "Match with i64 arm must produce different bytes than Placeholder"
        );
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
                    arms: vec![AnfMatchArm {
                        pattern: "_".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(0)),
                    }],
                }),
            },
        });
        assert!(
            emit_native(&anf).is_ok(),
            "Match with wildcard only must compile"
        );
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
                        AnfMatchArm {
                            pattern: "true".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(1)),
                        },
                        AnfMatchArm {
                            pattern: "false".to_string(),
                            body: AnfExpr::Literal(LiteralValue::Int(0)),
                        },
                    ],
                }),
            },
        });
        let ph = emit_native(&placeholder_anf()).unwrap();
        let art = emit_native(&anf).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "Match with bool arms must produce different bytes than Placeholder"
        );
        assert_eq!(
            infer_cranelift_return_type(&AnfExpr::Match {
                scrutinee: "b".to_string(),
                arms: vec![crate::anf::AnfMatchArm {
                    pattern: "true".to_string(),
                    body: AnfExpr::Literal(LiteralValue::Int(1))
                },],
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
        assert!(
            emit_native(&anf).is_ok(),
            "Match with empty arms must compile (produces trap)"
        );
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
        assert_ne!(
            art_a.native_bytes, art_b.native_bytes,
            "Seq([Int(1), Int(2)]) and Seq([Int(1), Int(5)]) must produce different bytes"
        );
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
        assert!(
            emit_native(&anf).is_ok(),
            "Seq([]) must compile without panic"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "RuntimeCheck must produce different bytes than Placeholder"
        );
    }

    // ── TASK-G0: RecordNew / FieldGet / FieldUpdate — RED ────────────────

    fn anf_with_record(fields: Vec<(&str, i64)>) -> AnfIr {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let field_exprs: Vec<(String, AnfExpr)> = fields
            .into_iter()
            .map(|(f, v)| (f.to_string(), AnfExpr::Literal(LiteralValue::Int(v))))
            .collect();
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::RecordNew {
                fields: field_exprs,
            },
        })
    }

    #[test]
    fn native_record_new_differs_from_placeholder() {
        let art = emit_native(&anf_with_record(vec![("x", 1), ("y", 2)])).unwrap();
        let ph = emit_native(&placeholder_anf()).unwrap();
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "RecordNew must produce different bytes than Placeholder"
        );
        assert_eq!(
            infer_cranelift_return_type(&crate::anf::AnfExpr::RecordNew {
                fields: vec![(
                    "x".to_string(),
                    crate::anf::AnfExpr::Literal(crate::core_ir::LiteralValue::Int(1))
                )],
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "FieldGet must produce different bytes than Placeholder"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "FieldUpdate must produce different bytes than Placeholder"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "VariantNew must produce different bytes than Placeholder"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "ListNew must produce different bytes than Placeholder"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "TupleNew must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_variant_two_tags_differ() {
        use crate::anf::{AnfBinding, AnfExpr};
        let make_variant = |tag: &str| {
            anf_for_binding(AnfBinding {
                source_ref: NodeRef(0),
                name: "fn_op".to_string(),
                expr: AnfExpr::VariantNew {
                    tag: tag.to_string(),
                    payload: None,
                },
            })
        };
        let art_ok = emit_native(&make_variant("Ok")).unwrap();
        let art_err = emit_native(&make_variant("Err")).unwrap();
        assert_ne!(
            art_ok.native_bytes, art_err.native_bytes,
            "VariantNew('Ok') and VariantNew('Err') must produce different bytes (different tag ids)"
        );
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
        assert_ne!(
            art.native_bytes, ph.native_bytes,
            "EffectCall must produce different bytes than Placeholder"
        );
    }

    #[test]
    fn native_effect_call_two_capabilities_differ() {
        use crate::anf::{AnfBinding, AnfExpr};
        use crate::core_ir::LiteralValue;
        let make_effect = |cap: &str| {
            anf_for_binding(AnfBinding {
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
            })
        };
        let art_db = emit_native(&make_effect("db")).unwrap();
        let art_fs = emit_native(&make_effect("fs")).unwrap();
        assert_ne!(
            art_db.native_bytes, art_fs.native_bytes,
            "EffectCall('db') and EffectCall('fs') must produce different bytes"
        );
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
        assert!(
            art.hash_chain.native_hash.is_some(),
            "native_hash must be Some for EffectCall"
        );
    }
}
