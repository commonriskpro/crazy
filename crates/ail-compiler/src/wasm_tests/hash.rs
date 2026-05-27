use super::helpers::*;

// Task 3.3 inline unit tests ──────────────────────────────────────────

// Scenario: anf_ir_hash None → EncodingError.
// Proves the pre-condition gate fires correctly.
#[test]
fn emit_wasm_rejects_unsealed_anf_ir_hash() {
    let anf = AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings: vec![],
        source_map: SourceMap { entries: vec![] },
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
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::EncodingError(_))),
        "expected EncodingError for unsealed anf_ir_hash, got {result:?}"
    );
}

// Scenario: wasm_hash is sealed after emit_wasm.
#[test]
fn emit_wasm_seals_wasm_hash() {
    let anf = anf_for_n(1);
    let artifact = emit_wasm(&anf).unwrap();
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be Some after emit_wasm"
    );
}

// TRIANGULATE: different inputs produce different wasm hashes.
#[test]
fn different_anf_produces_different_wasm_hash() {
    let a1 = emit_wasm(&anf_for_n(1)).unwrap();
    let a2 = emit_wasm(&anf_for_n(2)).unwrap();
    assert_ne!(
        a1.hash_chain.wasm_hash, a2.hash_chain.wasm_hash,
        "different AnfIr inputs must produce different wasm_hashes"
    );
}

// Scenario: build_type_section returns None for 0 functions and no fold.

#[test]
fn emit_wasm_capabilities_manifest_len_equals_binding_count() {
    for n in [0usize, 1, 3, 5] {
        let anf = anf_for_n(n);
        let artifact = emit_wasm(&anf).unwrap();
        assert_eq!(
            artifact.capabilities_manifest.entries.len(),
            n,
            "capabilities_manifest must have {n} entries for {n}-binding AnfIr"
        );
    }
}

// Scenario: empty AnfIr produces empty capabilities_manifest.
// Triangulate: zero bindings → zero entries.
#[test]
fn emit_wasm_empty_anf_produces_empty_capabilities_manifest() {
    let anf = anf_for_n(0);
    let artifact = emit_wasm(&anf).unwrap();
    assert!(
        artifact.capabilities_manifest.entries.is_empty(),
        "empty AnfIr must produce empty capabilities_manifest"
    );
}

// Scenario: capabilities_manifest entries carry correct names and source_refs.
// Spec: "entry.name == binding.name; entry.source_ref == binding.source_ref"
#[test]
fn emit_wasm_capabilities_manifest_entries_match_bindings() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_wasm(&anf).unwrap();
    for (i, entry) in artifact.capabilities_manifest.entries.iter().enumerate() {
        assert_eq!(
            entry.name,
            format!("fn_{i}"),
            "entry {i} name must match binding name"
        );
        assert_eq!(
            entry.source_ref,
            ail_core::semantic_graph::NodeRef(i as u32),
            "entry {i} source_ref must match binding source_ref"
        );
    }
}

// Scenario: capabilities_manifest_hash in artifact_manifest is derived from
// the real manifest bytes, not a proxy over raw bindings.
// Triangulate: two different N-binding AnfIrs produce different manifest hashes.
#[test]
fn emit_wasm_different_bindings_produce_different_capabilities_manifest_hash() {
    let a1 = emit_wasm(&anf_for_n(1)).unwrap();
    let a2 = emit_wasm(&anf_for_n(2)).unwrap();
    assert_ne!(
        a1.artifact_manifest.capabilities_manifest_hash,
        a2.artifact_manifest.capabilities_manifest_hash,
        "different binding counts must produce different capabilities_manifest_hash"
    );
}

// Scenario: capabilities_manifest serialises to JSON with entries array.
// Spec: JSON consumers must be able to parse capabilities_manifest.entries uniformly.
#[test]
fn emit_wasm_capabilities_manifest_serialises_to_json_with_entries_array() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).unwrap();
    let json_val = serde_json::to_value(&artifact.capabilities_manifest)
        .expect("capabilities_manifest must serialise to JSON");
    assert!(
        json_val["entries"].is_array(),
        "serialised capabilities_manifest must have entries array; got: {json_val}"
    );
    assert_eq!(
        json_val["entries"].as_array().unwrap().len(),
        2,
        "entries array must have 2 elements for 2-binding AnfIr"
    );
}

// ── Closure-capture PR1: AnfExpr::Lambda captures field ───────────────────
//
// Verify that the `captures` field is correctly populated during ANF lowering.
// Scenarios: no capture, simple capture, shadowed bound var, EffectCall arg.

// Scenario: lambda whose body only references its own params — no captures.
