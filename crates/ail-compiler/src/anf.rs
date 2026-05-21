// ── ail-compiler::anf ─────────────────────────────────────────────────────
//
// ANF (Administrative Normal Form) IR value types — the second lowering
// stage output.
//
// # Design constraints
//
// - `Vec` only (no `HashMap`) — workspace determinism contract.
// - All types `#[derive(Serialize)]` for CBOR hash sealing.
// - Every `AnfBinding` carries a `source_ref: NodeRef` that traces back to
//   the original `SemanticGraph` node; this provenance must survive lowering.
//
// # Phase 7 scope
//
// `AnfBinding` is a 1-to-1 normalisation of a `CoreNode`.  In Phase 7,
// there are no sub-expressions or let-bindings yet — that structural
// normalisation is deferred to Phase 8 when function bodies are emitted.

use ail_core::semantic_graph::NodeRef;
use serde::Serialize;

use crate::core_ir::StageHashes;

// ── AnfBinding ────────────────────────────────────────────────────────────

/// One binding in the ANF IR — normalised from a `CoreNode`.
///
/// `source_ref` is the provenance chain back to the originating
/// `SemanticGraph` node.  It MUST equal the `CoreNode::source_ref` that
/// this binding was produced from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnfBinding {
    /// Original `NodeRef` from the `SemanticGraph` — preserved through
    /// Core IR and into ANF for full end-to-end provenance.
    pub source_ref: NodeRef,
    /// Binding name, copied from the `CoreNode`.
    pub name: String,
}

// ── AnfIr ─────────────────────────────────────────────────────────────────

/// Output of the second pipeline stage: a flat list of ANF bindings with
/// full provenance and an extended hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnfIr {
    /// ANF bindings in source traversal order.
    pub bindings: Vec<AnfBinding>,
    /// Hash chain extended through the ANF stage.
    /// `stage_hashes.anf_ir_hash` is `Some(...)` after this stage completes.
    pub stage_hashes: StageHashes,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_ir::StageHashes;
    use crate::hash::stable_cbor_bytes;

    // ── Task 1.6 — RED: tests written before types existed. ───────────────

    // Scenario: AnfBinding preserves its source_ref provenance.
    // Spec: "every AnfBinding.source_ref matches origin NodeRef"
    #[test]
    fn anf_binding_preserves_source_ref() {
        let binding = AnfBinding {
            source_ref: NodeRef(7),
            name: "fn_x".to_string(),
        };
        assert_eq!(
            binding.source_ref,
            NodeRef(7),
            "source_ref must be preserved verbatim"
        );
    }

    // Scenario: AnfIr is constructible with bindings and stage hashes.
    #[test]
    fn anf_ir_is_constructible() {
        let ir = AnfIr {
            bindings: vec![
                AnfBinding {
                    source_ref: NodeRef(0),
                    name: "mod_root".to_string(),
                },
                AnfBinding {
                    source_ref: NodeRef(1),
                    name: "fn_main".to_string(),
                },
            ],
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: Some([2u8; 32]),
                wasm_hash: None,
                native_hash: None,
            },
        };
        assert_eq!(ir.bindings.len(), 2);
        assert!(ir.stage_hashes.anf_ir_hash.is_some());
    }

    // TRIANGULATE: stable_cbor_bytes on Vec<AnfBinding> is deterministic.
    // This is the content that lower_to_anf (PR 2) will use for hash sealing.
    #[test]
    fn anf_binding_list_cbor_is_deterministic() {
        let bindings = vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "a".to_string(),
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "b".to_string(),
            },
            AnfBinding {
                source_ref: NodeRef(2),
                name: "c".to_string(),
            },
        ];
        let b1 = stable_cbor_bytes(&bindings).expect("first encode");
        let b2 = stable_cbor_bytes(&bindings).expect("second encode");
        assert_eq!(b1, b2, "Vec<AnfBinding> must produce identical CBOR bytes");
    }

    // TRIANGULATE: different binding lists produce different CBOR bytes.
    #[test]
    fn different_anf_binding_lists_produce_different_cbor() {
        let list_a = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "x".to_string(),
        }];
        let list_b = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "y".to_string(),
        }];
        let b_a = stable_cbor_bytes(&list_a).expect("encode a");
        let b_b = stable_cbor_bytes(&list_b).expect("encode b");
        assert_ne!(
            b_a, b_b,
            "different AnfBinding lists must produce different CBOR"
        );
    }

    // Scenario: source_ref is not dropped when name is the same.
    // Proves NodeRef(3) ≠ NodeRef(4) even with identical names.
    #[test]
    fn anf_binding_distinct_refs_are_not_equal() {
        let b1 = AnfBinding {
            source_ref: NodeRef(3),
            name: "shared_name".to_string(),
        };
        let b2 = AnfBinding {
            source_ref: NodeRef(4),
            name: "shared_name".to_string(),
        };
        assert_ne!(b1, b2, "bindings with different NodeRefs must not be equal");
    }
}
