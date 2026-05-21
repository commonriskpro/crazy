// ── ail-compiler::wasm_tests ─────────────────────────────────────────────
//
// Task 3.1 (RED): Tests written BEFORE emit_wasm / WasmArtifact exist.
// These drive the implementation of src/wasm.rs.
//
// Spec scenarios covered:
//  - wasmparser::validate accepts every emitted module.
//  - Zero-binding graph → minimal valid WASM module (magic + version).
//  - N-binding graph → N functions each with [unreachable, end] body.
//  - provenance map has exactly N entries for an N-binding AnfIr.
//  - wasm_hash is sealed in hash_chain after emit_wasm.
//  - Determinism: same AnfIr → identical wasm bytes across two calls.

use ail_compiler::{
    emit_wasm,
    lower::{lower_to_anf, lower_to_core_ir},
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

fn graph_with_n_nodes(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    }
}

fn proven_report() -> VerificationReport {
    VerificationReport { entries: vec![] }
}

fn anf_for_graph(graph: &SemanticGraph) -> ail_compiler::AnfIr {
    let core = lower_to_core_ir(graph, &proven_report()).expect("lower_to_core_ir failed");
    lower_to_anf(&core).expect("lower_to_anf failed")
}

// ── Task 3.1: wasmparser validates emitted modules ────────────────────────

// Scenario: zero-binding graph → minimal valid WASM module.
// The emitted bytes must pass wasmparser::validate (structural validity only).
#[test]
fn empty_anf_emits_valid_wasm_module() {
    let anf = anf_for_graph(&empty_graph());
    let artifact = emit_wasm(&anf).expect("emit_wasm failed on empty anf");
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected empty wasm module");
}

// Scenario: N-binding graph → valid WASM module with N function stubs.
#[test]
fn three_node_graph_emits_valid_wasm_module() {
    let anf = anf_for_graph(&graph_with_n_nodes(3));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed on 3-node graph");
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected 3-function wasm module");
}

// TRIANGULATE: single-node graph also produces a structurally valid module.
#[test]
fn one_node_graph_emits_valid_wasm_module() {
    let anf = anf_for_graph(&graph_with_n_nodes(1));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed on 1-node graph");
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected 1-function wasm module");
}

// ── Task 3.1: provenance map completeness ────────────────────────────────

// Spec: provenance map has exactly N entries for N-binding AnfIr.
#[test]
fn provenance_map_has_n_entries_for_n_nodes() {
    let anf = anf_for_graph(&graph_with_n_nodes(4));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert_eq!(
        artifact.provenance.len(),
        4,
        "provenance map must have exactly 4 entries for a 4-node graph"
    );
}

// TRIANGULATE: zero-node graph has empty provenance map.
#[test]
fn provenance_map_is_empty_for_zero_node_graph() {
    let anf = anf_for_graph(&empty_graph());
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert!(
        artifact.provenance.is_empty(),
        "provenance map must be empty for a zero-binding AnfIr"
    );
}

// Spec: provenance NodeRefs match the source bindings.
#[test]
fn provenance_map_contains_correct_node_refs() {
    let graph = graph_with_n_nodes(3);
    let anf = anf_for_graph(&graph);
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    // Each NodeRef(0), NodeRef(1), NodeRef(2) must be in the provenance map.
    for i in 0..3u32 {
        assert!(
            artifact.provenance.contains_key(&NodeRef(i)),
            "provenance must contain NodeRef({i})"
        );
    }
}

// ── Task 3.1: hash chain sealing ─────────────────────────────────────────

// Spec: wasm_hash is Some after emit_wasm.
#[test]
fn wasm_hash_is_sealed_after_emit() {
    let anf = anf_for_graph(&graph_with_n_nodes(2));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be Some after emit_wasm"
    );
}

// Spec: hash_chain carries verification_report_hash through to WASM stage.
#[test]
fn hash_chain_preserves_verification_report_hash() {
    let anf = anf_for_graph(&graph_with_n_nodes(1));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    // verification_report_hash is always computed (even for empty report).
    assert_ne!(
        artifact.hash_chain.verification_report_hash, [0u8; 32],
        "verification_report_hash must be non-zero after pipeline"
    );
}

// ── Task 3.1: determinism ────────────────────────────────────────────────

// Spec: same AnfIr produces identical wasm bytes in two calls.
#[test]
fn emit_wasm_is_deterministic() {
    let anf = anf_for_graph(&graph_with_n_nodes(3));
    let a1 = emit_wasm(&anf).expect("emit_wasm first call");
    let a2 = emit_wasm(&anf).expect("emit_wasm second call");
    assert_eq!(
        a1.wasm, a2.wasm,
        "emit_wasm must produce identical bytes for the same AnfIr"
    );
    assert_eq!(
        a1.hash_chain.wasm_hash, a2.hash_chain.wasm_hash,
        "wasm_hash must be identical across two calls with the same AnfIr"
    );
}

// TRIANGULATE: different AnfIrs produce different wasm hashes.
#[test]
fn different_anf_inputs_produce_different_wasm_hashes() {
    let anf_2 = anf_for_graph(&graph_with_n_nodes(2));
    let anf_3 = anf_for_graph(&graph_with_n_nodes(3));
    let a2 = emit_wasm(&anf_2).expect("emit_wasm 2-node");
    let a3 = emit_wasm(&anf_3).expect("emit_wasm 3-node");
    assert_ne!(
        a2.hash_chain.wasm_hash, a3.hash_chain.wasm_hash,
        "different AnfIr inputs must produce different wasm_hashes"
    );
}

// ── Task 3.1: function body stubs ────────────────────────────────────────

// Spec: each function body is [unreachable, end].
// We verify by parsing the emitted module with wasmparser and counting
// CodeSectionEntry payloads, then checking first operator is Unreachable.
#[test]
fn function_bodies_contain_unreachable_instruction() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = anf_for_graph(&graph_with_n_nodes(2));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");

    let mut function_bodies_found = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            function_bodies_found += 1;
            // Verify first instruction is Unreachable.
            let mut reader = body
                .get_operators_reader()
                .expect("get_operators_reader failed");
            let first_op = reader.read().expect("read first operator failed");
            assert!(
                matches!(first_op, Operator::Unreachable),
                "first instruction must be Unreachable, got {first_op:?}"
            );
        }
    }
    // We expect exactly 2 function bodies for a 2-binding AnfIr.
    assert_eq!(
        function_bodies_found, 2,
        "expected 2 code section entries for 2-binding AnfIr"
    );
}
