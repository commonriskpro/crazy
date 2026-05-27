use super::helpers::*;

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

// ── G20 R2: function bodies are real ANF-derived WASM code ───────────────

// Spec (G20 R2): function bodies are generated from ANF expressions — no longer
// stub-only.  For Placeholder/default nodes, the body contains unreachable.
// For real AnfExpr nodes, the body contains real instructions ending with End.
//
// We assert:
//  1. The correct number of code sections are emitted.
//  2. Every body ends with End (valid WASM).
//  3. Bodies are structurally valid (wasmparser validates the whole module).

#[test]
fn provenance_values_are_byte_offsets_not_function_indexes() {
    let n = 3usize;
    let anf = anf_for_graph(&graph_with_n_nodes(n));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");

    assert_eq!(
        artifact.provenance.len(),
        n,
        "provenance must have {n} entries"
    );

    for i in 0..n {
        let nr = NodeRef(i as u32);
        let offset = *artifact
            .provenance
            .get(&nr)
            .unwrap_or_else(|| panic!("NodeRef({i}) missing from provenance"));

        // The byte at the stored offset is the LEB128-encoded body size.
        // For G20 R2, bodies contain real ANF-derived code — size > 0.
        // We assert the offset is a valid index into the WASM binary.
        assert!(
            (offset as usize) < artifact.wasm.len(),
            "provenance offset {offset} must be within wasm binary of len {}",
            artifact.wasm.len()
        );
        // The byte at the offset must be non-zero (non-empty body).
        assert_ne!(
            artifact.wasm[offset as usize], 0x00,
            "wasm[provenance[NodeRef({i})]] = wasm[{offset}] must be non-zero (body size), \
             got 0x{:02x} — body cannot be empty",
            artifact.wasm[offset as usize]
        );
        // Crucially: the value must NOT equal the function index (0, 1, 2).
        // If it were a function index, wasm[0] = 0x00 (magic byte) and
        // provenance[NodeRef(0)] would equal 0 — but 0x00 is the magic byte,
        // not a valid code-entry header.
        // We prove it's an offset by checking offset > 8 (past the WASM header).
        assert!(
            offset > 8,
            "provenance offset {offset} must be past the 8-byte WASM header \
             (function index 0 would point to magic byte region)"
        );
    }
}

// ── verify-fix: explicit WASM hash-chain recomputation ────────────────────

// Spec: wasm_hash = blake3(anf_ir_hash || wasm_binary)
// Verify by recomputing the hash explicitly and comparing.
#[test]
fn wasm_hash_chain_matches_explicit_recomputation() {
    use ail_compiler::hash::hash_with_parent;

    let anf = anf_for_graph(&graph_with_n_nodes(2));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");

    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .expect("anf_ir_hash must be sealed before emit_wasm");

    // Recompute: wasm_hash = blake3(anf_ir_hash || wasm_bytes)
    let expected_hash = hash_with_parent(&anf_ir_hash, &artifact.wasm);

    assert_eq!(
        artifact.hash_chain.wasm_hash,
        Some(expected_hash),
        "wasm_hash must equal blake3(anf_ir_hash || wasm_binary)"
    );
}
