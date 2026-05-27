use super::helpers::*;

// ── Task 1.5 — RED: tests written before types existed. ───────────────

// Scenario: CoreIr is constructible with one CoreNode.
// Base case — proves the struct and its fields accept the right types.
#[test]
fn core_ir_is_constructible_with_one_node() {
    let node = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "core_mod".to_string(),
        ty: None,
        expr: None,
    };
    let ir = CoreIr {
        nodes: vec![node],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
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
        ty: None,
        expr: None,
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
            ty: None,
            expr: None,
        },
        CoreNode {
            source_ref: NodeRef(1),
            kind: CoreNodeKind::Module,
            name: "mod_b".to_string(),
            ty: None,
            expr: None,
        },
        CoreNode {
            source_ref: NodeRef(2),
            kind: CoreNodeKind::Effect,
            name: "eff_c".to_string(),
            ty: None,
            expr: None,
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
        ty: None,
        expr: None,
    }];
    let list_b = vec![CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "b".to_string(),
        ty: None,
        expr: None,
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
        native_hash: None,
        source_map_hash: None,
        artifact_manifest_hash: None,
    };
    assert!(h.anf_ir_hash.is_none());
    assert!(h.wasm_hash.is_none());
    assert!(h.native_hash.is_none());
    assert_eq!(h.core_ir_hash, [42u8; 32]);
}
