// ── ail-compiler::core_ir ─────────────────────────────────────────────────
//
// Core IR value types — the first lowering stage output.
//
// # Design constraints
//
// - `Vec` and `BTreeMap` only (no `HashMap`) — workspace determinism contract.
// - All types `#[derive(Serialize)]` for CBOR hash sealing.
// - `CoreIr` owns the `StageHashes` accumulator so later stages can read
//   predecessor hashes without re-computing them.
//
// # Phase 7 scope
//
// `CoreNode` carries a `source_ref: NodeRef` (provenance) and a `CoreNodeKind`
// that mirrors `NodeKind` for Phase 7.  Bodies are absent at this stage;
// they are deferred to Phase 8 expression lowering.

use ail_core::semantic_graph::NodeRef;
use serde::Serialize;

// ── CoreNodeKind ──────────────────────────────────────────────────────────

/// Compiler IR node kind — mirrors `ail_core::semantic_graph::NodeKind` for
/// Phase 7.  Defined as a separate enum to allow the compiler IR to diverge
/// from the source graph model in future phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum CoreNodeKind {
    Module,
    Function,
    Type,
    Effect,
    Capability,
    Contract,
    Invariant,
    Test,
    Boundary,
}

// ── CoreNode ──────────────────────────────────────────────────────────────

/// One node in the Core IR, with full provenance back to the source graph.
///
/// In Phase 7 there is a 1-to-1 mapping from `SemanticGraph` nodes to
/// `CoreNode`s; the `source_ref` field preserves that mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoreNode {
    /// The `NodeRef` this `CoreNode` was lowered from.
    pub source_ref: NodeRef,
    /// Compiler IR node kind (mirrors the source `NodeKind`).
    pub kind: CoreNodeKind,
    /// Node name, copied verbatim from the source `GraphNode`.
    pub name: String,
}

// ── StageHashes ───────────────────────────────────────────────────────────

/// Accumulates BLAKE3 hashes as the pipeline advances through its stages.
///
/// `graph_snapshot_hash` and `verification_report_hash` are computed from
/// the pipeline inputs.  `core_ir_hash`, `anf_ir_hash`, and `wasm_hash` are
/// filled in by successive stages.  Optional fields are `None` until the
/// corresponding stage completes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
}

// ── CoreIr ────────────────────────────────────────────────────────────────

/// Output of the first pipeline stage: a flat list of typed Core IR nodes
/// with full provenance and a sealed hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoreIr {
    /// Lowered nodes in source graph traversal order.
    pub nodes: Vec<CoreNode>,
    /// Hash chain sealed through the Core IR stage.
    pub stage_hashes: StageHashes,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::stable_cbor_bytes;

    // ── Task 1.5 — RED: tests written before types existed. ───────────────

    // Scenario: CoreIr is constructible with one CoreNode.
    // Base case — proves the struct and its fields accept the right types.
    #[test]
    fn core_ir_is_constructible_with_one_node() {
        let node = CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "core_mod".to_string(),
        };
        let ir = CoreIr {
            nodes: vec![node],
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: None,
                wasm_hash: None,
            },
        };
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].source_ref, NodeRef(0));
        assert_eq!(ir.nodes[0].kind, CoreNodeKind::Module);
    }

    // Scenario: CoreNode preserves its source_ref provenance.
    // Proves the provenance contract: source_ref is not dropped or mutated.
    #[test]
    fn core_node_preserves_source_ref() {
        let node = CoreNode {
            source_ref: NodeRef(99),
            kind: CoreNodeKind::Function,
            name: "fn_with_high_ref".to_string(),
        };
        assert_eq!(node.source_ref, NodeRef(99));
    }

    // TRIANGULATE: stable_cbor_bytes on Vec<CoreNode> is deterministic.
    // Proves that the Serialize impl produces stable bytes for the node list
    // — the actual content used for hash sealing in lower_to_core_ir (PR 2).
    #[test]
    fn core_node_list_cbor_is_deterministic() {
        let nodes = vec![
            CoreNode {
                source_ref: NodeRef(0),
                kind: CoreNodeKind::Function,
                name: "fn_a".to_string(),
            },
            CoreNode {
                source_ref: NodeRef(1),
                kind: CoreNodeKind::Module,
                name: "mod_b".to_string(),
            },
            CoreNode {
                source_ref: NodeRef(2),
                kind: CoreNodeKind::Effect,
                name: "eff_c".to_string(),
            },
        ];
        let b1 = stable_cbor_bytes(&nodes).expect("first encode");
        let b2 = stable_cbor_bytes(&nodes).expect("second encode");
        assert_eq!(
            b1, b2,
            "Vec<CoreNode> must produce identical CBOR bytes across calls"
        );
    }

    // TRIANGULATE: different CoreNode lists produce different CBOR bytes.
    // Proves the encoding is not constant (real content affects output).
    #[test]
    fn different_core_node_lists_produce_different_cbor() {
        let list_a = vec![CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "a".to_string(),
        }];
        let list_b = vec![CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "b".to_string(),
        }];
        let b_a = stable_cbor_bytes(&list_a).expect("encode a");
        let b_b = stable_cbor_bytes(&list_b).expect("encode b");
        assert_ne!(
            b_a, b_b,
            "different CoreNode lists must produce different CBOR"
        );
    }

    // Scenario: StageHashes optional fields are None by default.
    #[test]
    fn stage_hashes_optional_fields_default_none() {
        let h = StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [42u8; 32],
            anf_ir_hash: None,
            wasm_hash: None,
        };
        assert!(h.anf_ir_hash.is_none());
        assert!(h.wasm_hash.is_none());
        assert_eq!(h.core_ir_hash, [42u8; 32]);
    }

    // TRIANGULATE: all CoreNodeKind variants are constructible.
    // Ensures no variant is accidentally omitted from the enum.
    #[test]
    fn all_core_node_kinds_are_constructible() {
        let kinds = [
            CoreNodeKind::Module,
            CoreNodeKind::Function,
            CoreNodeKind::Type,
            CoreNodeKind::Effect,
            CoreNodeKind::Capability,
            CoreNodeKind::Contract,
            CoreNodeKind::Invariant,
            CoreNodeKind::Test,
            CoreNodeKind::Boundary,
        ];
        assert_eq!(
            kinds.len(),
            9,
            "all 9 CoreNodeKind variants must be reachable"
        );
    }
}
