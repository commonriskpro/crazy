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
    ir::{Function, InstBuilder, Signature, UserFuncName},
    isa::CallConv,
    settings,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use serde::{Deserialize, Serialize};

use crate::anf::{AnfIr, SourceMap, SourceMapEntry};
use crate::artifact_manifest::ArtifactManifest;
use crate::core_ir::StageHashes;
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};

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

// ── lower_binding ─────────────────────────────────────────────────────────

/// Lower one `AnfBinding` into a compiled Cranelift function inside `module`.
///
/// Returns `(cumulative_offset_before, code_size_in_bytes)` so the caller
/// can record the provenance entry and advance the offset accumulator.
///
/// Body is a single `trap` instruction (user trap code 1), matching the WASM
/// phase's `unreachable` stub strategy.
fn lower_binding(
    module: &mut ObjectModule,
    name: &str,
    cumulative_offset: u64,
) -> Result<u64, CompileError> {
    let sig = Signature::new(CallConv::SystemV);

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
        // IMPORTANT: TrapCode::user(0) returns None — use user(1).unwrap()
        builder
            .ins()
            .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
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

    // Lower each binding and accumulate provenance.
    let mut provenance: BTreeMap<NodeRef, u64> = BTreeMap::new();
    let mut native_offsets: Vec<u64> = Vec::with_capacity(anf.bindings.len());
    let mut cumulative_offset: u64 = 0;

    for binding in &anf.bindings {
        // Record provenance entry: this binding starts at current offset.
        provenance.insert(binding.source_ref, cumulative_offset);
        native_offsets.push(cumulative_offset);

        // Lower the binding and get its compiled code size.
        let code_size = lower_binding(&mut module, &binding.name, cumulative_offset)?;
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
}
