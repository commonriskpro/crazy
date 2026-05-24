// ── ail-compiler::core_ir::nodes ─────────────────────────────────────────
//
// Aggregate pipeline types: `CoreNode`, `StageHashes`, and `CoreIr`.
// These are the primary output types of the first compiler stage.

use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

use super::expr::CoreExpr;
use super::primitives::CoreNodeKind;
use super::types::CoreType;

// ── CoreNode ──────────────────────────────────────────────────────────────

/// One node in the Core IR, with full provenance back to the source graph.
///
/// In Phase 7 there is a 1-to-1 mapping from `SemanticGraph` nodes to
/// `CoreNode`s; the `source_ref` field preserves that mapping.
///
/// The `ty` and `expr` fields were added in G2 (core-ir-full).  Both are
/// serialized only when `Some` to preserve CBOR wire-format compatibility
/// with pre-G2 artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreNode {
    /// The `NodeRef` this `CoreNode` was lowered from.
    pub source_ref: NodeRef,
    /// Compiler IR node kind (mirrors the source `NodeKind`).
    pub kind: CoreNodeKind,
    /// Node name, copied verbatim from the source `GraphNode`.
    pub name: String,
    /// Resolved Core IR type, when available.
    ///
    /// Populated by `lower_to_core_ir` from `GraphNode.type_facts` when
    /// present; `None` for nodes without type information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<CoreType>,
    /// Core IR expression body, when available.
    ///
    /// `None` for nodes that carry only structural information (modules,
    /// types, capabilities, etc.) at this stage.  Expression bodies will
    /// be populated in a future expression-lowering phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<CoreExpr>,
}

// ── StageHashes ───────────────────────────────────────────────────────────

/// Accumulates BLAKE3 hashes as the pipeline advances through its stages.
///
/// `graph_snapshot_hash` and `verification_report_hash` are computed from
/// the pipeline inputs.  `core_ir_hash`, `anf_ir_hash`, and `wasm_hash` are
/// filled in by successive stages.  Optional fields are `None` until the
/// corresponding stage completes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageHashes {
    /// BLAKE3 hash of the serialised `SemanticGraph` (pipeline input).
    pub graph_snapshot_hash: [u8; 32],
    /// BLAKE3 hash of the serialised `VerificationReport` (pipeline input).
    pub verification_report_hash: [u8; 32],
    /// `blake3(graph_snapshot_hash || core_ir_bytes)` — set by `lower_to_core_ir`.
    pub core_ir_hash: [u8; 32],
    /// `blake3(core_ir_hash || anf_ir_bytes)` — set by `lower_to_anf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anf_ir_hash: Option<[u8; 32]>,
    /// `blake3(anf_ir_hash || wasm_binary)` — set by `emit_wasm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_hash: Option<[u8; 32]>,
    /// `blake3(anf_ir_hash || native_binary)` — set by `emit_native`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_hash: Option<[u8; 32]>,
    /// `blake3(source_map_cbor_bytes)` — set by backend stages after populating offsets.
    ///
    /// Any change to the semantic source map (offsets, provenance fields)
    /// causes this hash to change, invalidating downstream manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map_hash: Option<[u8; 32]>,
    /// `blake3(artifact_manifest_cbor_bytes)` — set by artifact manifest emission.
    ///
    /// Covers profile, compiler version, and all upstream artifact hashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_manifest_hash: Option<[u8; 32]>,
}

// ── CoreIr ────────────────────────────────────────────────────────────────

/// Output of the first pipeline stage: a flat list of typed Core IR nodes
/// with full provenance and a sealed hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreIr {
    /// Lowered nodes in source graph traversal order.
    pub nodes: Vec<CoreNode>,
    /// Hash chain sealed through the Core IR stage.
    pub stage_hashes: StageHashes,
}
