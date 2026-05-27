use super::helpers::*;

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
