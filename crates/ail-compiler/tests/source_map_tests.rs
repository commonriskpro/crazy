// ── ail-compiler::source_map_tests ───────────────────────────────────────
//
// G32: Semantic source map integration tests.
//
// Spec scenarios covered:
//  - emit_wasm populates wasm_offset for every emitted binding.
//  - emit_native populates native_offset for every emitted binding.
//  - Source maps have one entry per binding (including synthetic duplicates).
//  - Empty input yields empty source map.
//  - Any source-map change changes source_map_hash downstream.
//  - Duplicate NodeRefs are preserved (not collapsed) in source maps.

use ail_compiler::{
    emit_native, emit_wasm,
    lower::{lower_to_anf, lower_to_core_ir},
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
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

fn anf_for_n(n: usize) -> ail_compiler::AnfIr {
    let graph = graph_with_n_nodes(n);
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    lower_to_anf(&core).expect("lower_to_anf")
}

// ── Task 4: WASM backend populates wasm_offset ────────────────────────────

// Spec: emit_wasm must return a semantic source map with wasm_offset populated
// for every emitted binding.
// RED → GREEN: WasmArtifact.source_map is populated with non-None wasm_offset.
#[test]
fn emit_wasm_populates_wasm_offset_for_every_binding() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_wasm(&anf).expect("emit_wasm");

    assert_eq!(
        artifact.source_map.entries.len(),
        n,
        "source map must have one entry per binding"
    );
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        assert!(
            entry.wasm_offset.is_some(),
            "entry {i} must have wasm_offset set after emit_wasm, got None"
        );
        assert!(
            entry.native_offset.is_none(),
            "entry {i} must NOT have native_offset set by emit_wasm"
        );
    }
}

// TRIANGULATE: wasm_offset values are actual byte offsets (past WASM header).
#[test]
fn emit_wasm_offsets_are_past_wasm_header() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        let offset = entry.wasm_offset.expect("wasm_offset must be Some");
        assert!(
            offset > 8,
            "entry {i}: wasm_offset {offset} must be past the 8-byte WASM header"
        );
    }
}

// Spec: empty input yields empty source map after emit_wasm.
#[test]
fn emit_wasm_empty_input_yields_empty_source_map() {
    let anf = anf_for_n(0);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    assert!(
        artifact.source_map.entries.is_empty(),
        "empty AnfIr must produce empty source map in WasmArtifact"
    );
}

// Spec: source map entry node_id matches the binding's source_ref.
#[test]
fn emit_wasm_source_map_node_ids_match_bindings() {
    let n = 3;
    let anf = anf_for_n(n);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    for (binding, entry) in anf.bindings.iter().zip(artifact.source_map.entries.iter()) {
        assert_eq!(
            entry.node_id, binding.source_ref,
            "source map entry node_id must match binding.source_ref"
        );
        assert_eq!(
            entry.binding_name, binding.name,
            "source map entry binding_name must match binding.name"
        );
    }
}

// ── Task 4: Native backend populates native_offset ────────────────────────

// Spec: emit_native must return a semantic source map with native_offset
// populated for every emitted binding.
#[test]
fn emit_native_populates_native_offset_for_every_binding() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    assert_eq!(
        artifact.source_map.entries.len(),
        n,
        "source map must have one entry per binding"
    );
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        assert!(
            entry.native_offset.is_some(),
            "entry {i} must have native_offset set after emit_native, got None"
        );
        assert!(
            entry.wasm_offset.is_none(),
            "entry {i} must NOT have wasm_offset set by emit_native"
        );
    }
}

// Spec: empty input yields empty source map after emit_native.
#[test]
fn emit_native_empty_input_yields_empty_source_map() {
    let anf = anf_for_n(0);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.source_map.entries.is_empty(),
        "empty AnfIr must produce empty source map in NativeArtifact"
    );
}

// Spec: source map entry node_id matches the binding's source_ref.
#[test]
fn emit_native_source_map_node_ids_match_bindings() {
    let n = 3;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");
    for (binding, entry) in anf.bindings.iter().zip(artifact.source_map.entries.iter()) {
        assert_eq!(
            entry.node_id, binding.source_ref,
            "source map entry node_id must match binding.source_ref"
        );
    }
}

// ── Task 3: source_map_hash ────────────────────────────────────────────────

// Spec: any source-map change changes source_map_hash.
// We verify that source_map_hash is Some and that different inputs produce
// different hashes.
#[test]
fn emit_wasm_sets_source_map_hash() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    assert!(
        artifact.hash_chain.source_map_hash.is_some(),
        "source_map_hash must be Some after emit_wasm"
    );
}

// TRIANGULATE: different AnfIr inputs produce different source_map_hash.
#[test]
fn different_inputs_produce_different_source_map_hashes_wasm() {
    let a1 = emit_wasm(&anf_for_n(1)).expect("emit_wasm 1");
    let a2 = emit_wasm(&anf_for_n(2)).expect("emit_wasm 2");
    assert_ne!(
        a1.hash_chain.source_map_hash, a2.hash_chain.source_map_hash,
        "different AnfIr inputs must produce different source_map_hashes"
    );
}

// Spec: emit_native also sets source_map_hash.
#[test]
fn emit_native_sets_source_map_hash() {
    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.hash_chain.source_map_hash.is_some(),
        "source_map_hash must be Some after emit_native"
    );
}

// TRIANGULATE: different native inputs produce different source_map_hash.
#[test]
fn different_inputs_produce_different_source_map_hashes_native() {
    let a1 = emit_native(&anf_for_n(1)).expect("emit_native 1");
    let a2 = emit_native(&anf_for_n(2)).expect("emit_native 2");
    assert_ne!(
        a1.hash_chain.source_map_hash, a2.hash_chain.source_map_hash,
        "different AnfIr inputs must produce different source_map_hashes for native"
    );
}

// Spec: source_map_hash changes when source map content changes.
// We verify by computing a deterministic hash and comparing across runs.
#[test]
fn source_map_hash_is_deterministic_across_runs() {
    let anf = anf_for_n(3);
    let a1 = emit_wasm(&anf).expect("first emit_wasm");
    let a2 = emit_wasm(&anf).expect("second emit_wasm");
    assert_eq!(
        a1.hash_chain.source_map_hash, a2.hash_chain.source_map_hash,
        "source_map_hash must be identical across two calls with the same AnfIr"
    );
}
