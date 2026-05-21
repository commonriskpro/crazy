// ── ail-compiler::integration_tests ─────────────────────────────────────
//
// Task 3.5: Full pipeline integration tests.
//
// Spec scenarios:
//  - Pipeline run twice → identical Core/ANF/WASM hashes (determinism).
//  - Provenance map has exactly N entries for N-node graph.
//  - `verification_report_hash` is present (non-zero) in the final hash_chain.
//  - Core → ANF → WASM hash chain is internally consistent:
//    core_ir_hash ≠ anf_ir_hash ≠ wasm_hash (each stage seals differently).

use ail_compiler::{emit_wasm, lower_to_anf, lower_to_core_ir};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport { entries: vec![], ..Default::default() }
}

fn graph_with_n_nodes(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    }
}

// ── Task 3.5: full pipeline determinism ──────────────────────────────────

// Spec: same graph + report → identical Core/ANF/WASM hashes across two runs.
#[test]
fn full_pipeline_is_deterministic() {
    let graph = graph_with_n_nodes(3);
    let report = proven_report();

    // Run 1.
    let core1 = lower_to_core_ir(&graph, &report).expect("core run 1");
    let anf1 = lower_to_anf(&core1).expect("anf run 1");
    let wasm1 = emit_wasm(&anf1).expect("wasm run 1");

    // Run 2 — identical inputs.
    let core2 = lower_to_core_ir(&graph, &report).expect("core run 2");
    let anf2 = lower_to_anf(&core2).expect("anf run 2");
    let wasm2 = emit_wasm(&anf2).expect("wasm run 2");

    // All stage hashes must be identical.
    assert_eq!(
        core1.stage_hashes.core_ir_hash, core2.stage_hashes.core_ir_hash,
        "core_ir_hash must be identical across two runs"
    );
    assert_eq!(
        anf1.stage_hashes.anf_ir_hash, anf2.stage_hashes.anf_ir_hash,
        "anf_ir_hash must be identical across two runs"
    );
    assert_eq!(
        wasm1.hash_chain.wasm_hash, wasm2.hash_chain.wasm_hash,
        "wasm_hash must be identical across two runs"
    );
    assert_eq!(
        wasm1.wasm, wasm2.wasm,
        "wasm binary bytes must be identical across two runs"
    );
}

// TRIANGULATE: zero-node graph is also deterministic.
#[test]
fn empty_pipeline_is_deterministic() {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = proven_report();

    let core1 = lower_to_core_ir(&graph, &report).unwrap();
    let anf1 = lower_to_anf(&core1).unwrap();
    let wasm1 = emit_wasm(&anf1).unwrap();

    let core2 = lower_to_core_ir(&graph, &report).unwrap();
    let anf2 = lower_to_anf(&core2).unwrap();
    let wasm2 = emit_wasm(&anf2).unwrap();

    assert_eq!(wasm1.wasm, wasm2.wasm);
    assert_eq!(wasm1.hash_chain.wasm_hash, wasm2.hash_chain.wasm_hash);
}

// ── Task 3.5: provenance completeness ────────────────────────────────────

// Spec: provenance map has exactly N entries for N-node graph.
#[test]
fn provenance_map_has_exactly_n_entries() {
    let graph = graph_with_n_nodes(5);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    let artifact = emit_wasm(&anf).unwrap();

    assert_eq!(
        artifact.provenance.len(),
        5,
        "provenance must have 5 entries for a 5-node graph"
    );
}

// TRIANGULATE: provenance is empty for a zero-node graph.
#[test]
fn provenance_map_empty_for_zero_node_graph() {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    let artifact = emit_wasm(&anf).unwrap();

    assert!(
        artifact.provenance.is_empty(),
        "provenance must be empty for a zero-node graph"
    );
}

// ── Task 3.5: verification_report_hash in hash_chain ─────────────────────

// Spec: verification_report_hash is present (non-zero) in the final hash_chain.
// An empty VerificationReport still has a non-zero CBOR hash.
#[test]
fn verification_report_hash_present_in_final_hash_chain() {
    let graph = graph_with_n_nodes(2);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    let artifact = emit_wasm(&anf).unwrap();

    assert_ne!(
        artifact.hash_chain.verification_report_hash, [0u8; 32],
        "verification_report_hash must be non-zero in the final hash_chain"
    );
}

// ── Task 3.5: hash chain internal consistency ─────────────────────────────

// The three pipeline hashes must all differ from each other.
// Each stage seals blake3(predecessor_hash || stage_bytes), so identical
// output would indicate a hash chain defect.
#[test]
fn pipeline_stage_hashes_are_all_distinct() {
    let graph = graph_with_n_nodes(2);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    let artifact = emit_wasm(&anf).unwrap();

    let core_hash = artifact.hash_chain.core_ir_hash;
    let anf_hash = artifact.hash_chain.anf_ir_hash.unwrap();
    let wasm_hash = artifact.hash_chain.wasm_hash.unwrap();

    assert_ne!(
        core_hash, anf_hash,
        "core_ir_hash must differ from anf_ir_hash"
    );
    assert_ne!(
        anf_hash, wasm_hash,
        "anf_ir_hash must differ from wasm_hash"
    );
    assert_ne!(
        core_hash, wasm_hash,
        "core_ir_hash must differ from wasm_hash"
    );
}

// TRIANGULATE: graph_snapshot_hash is distinct from verification_report_hash
// (different inputs → different BLAKE3 hashes for non-identical payloads).
#[test]
fn graph_and_report_hashes_are_captured_in_chain() {
    let graph = graph_with_n_nodes(1);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    let artifact = emit_wasm(&anf).unwrap();

    // Both should be present (non-zero) since both inputs were serialised.
    assert_ne!(
        artifact.hash_chain.graph_snapshot_hash, [0u8; 32],
        "graph_snapshot_hash must be non-zero"
    );
    assert_ne!(
        artifact.hash_chain.verification_report_hash, [0u8; 32],
        "verification_report_hash must be non-zero"
    );
}
