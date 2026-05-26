// ── ail-compiler::native_backend_tests ───────────────────────────────────
//
// Integration tests for the native backend (Phase 17).
//
// Spec scenarios covered:
//  - Task 3.5: determinism — same AnfIr → byte-identical native_bytes + native_hash.
//  - Task 3.6: WASM pipeline unaffected — WasmArtifact.hash_chain.native_hash is None.
//  - Task 3.7: capability manifest has one entry per binding with correct name + source_ref.
//  - Task 3.8: cargo tree -p ail-compiler does not contain wasmtime or wasmer.

use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, CapabilitiesManifest, CapabilityEntry, SourceMap,
    anf::AnfMatchArm,
    core_ir::{LiteralValue, StageHashes},
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

fn anf_for_graph(graph: &SemanticGraph) -> ail_compiler::AnfIr {
    let core = lower_to_core_ir(graph, &proven_report()).expect("lower_to_core_ir failed");
    lower_to_anf(&core).expect("lower_to_anf failed")
}

fn anf_for_n(n: usize) -> ail_compiler::AnfIr {
    anf_for_graph(&graph_with_n_nodes(n))
}

fn sealed_anf(binding: AnfBinding) -> AnfIr {
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

fn match_zero_with_pattern(pattern: &str) -> AnfIr {
    sealed_anf(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_match".to_string(),
        expr: AnfExpr::Let {
            name: "tag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "tag".to_string(),
                arms: vec![
                    AnfMatchArm {
                        pattern: pattern.to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(99)),
                    },
                    AnfMatchArm {
                        pattern: "_".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(0)),
                    },
                ],
            }),
        },
    })
}

// ── Task 3.5: determinism ─────────────────────────────────────────────────

// Spec: same AnfIr → byte-identical native_bytes and native_hash across two calls.
// Scenario: "Same AnfIr produces identical native output"
#[test]
fn emit_native_is_deterministic() {
    let anf = anf_for_n(3);
    let a1 = emit_native(&anf).expect("emit_native first call");
    let a2 = emit_native(&anf).expect("emit_native second call");
    assert_eq!(
        a1.native_bytes, a2.native_bytes,
        "emit_native must produce byte-identical native_bytes for the same AnfIr"
    );
    assert_eq!(
        a1.hash_chain.native_hash, a2.hash_chain.native_hash,
        "native_hash must be identical across two calls with the same AnfIr"
    );
}

// TRIANGULATE: determinism holds for a 1-binding AnfIr.
#[test]
fn emit_native_deterministic_single_binding() {
    let anf = anf_for_n(1);
    let a1 = emit_native(&anf).expect("first");
    let a2 = emit_native(&anf).expect("second");
    assert_eq!(a1.native_bytes, a2.native_bytes);
    assert_eq!(a1.hash_chain.native_hash, a2.hash_chain.native_hash);
}

// ── Task 3.6: WASM pipeline unaffected ───────────────────────────────────

// Spec: WasmArtifact.hash_chain.native_hash is None after emit_wasm.
// Scenario: "WASM pipeline unaffected"
#[test]
fn wasm_pipeline_native_hash_is_none() {
    let anf = anf_for_n(2);
    let wasm_artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert!(
        wasm_artifact.hash_chain.native_hash.is_none(),
        "native_hash must be None after emit_wasm (WASM pipeline must not touch native_hash)"
    );
    assert!(
        wasm_artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be Some after emit_wasm"
    );
}

// TRIANGULATE: emit_wasm on zero-binding graph also leaves native_hash None.
#[test]
fn wasm_pipeline_empty_graph_native_hash_is_none() {
    let anf = anf_for_n(0);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    assert!(artifact.hash_chain.native_hash.is_none());
}

// ── Task 3.7: capability manifest ────────────────────────────────────────

// Spec: capabilities_manifest.entries contains one entry per binding with
// correct name and source_ref.
// Scenario: "Manifest lists all binding names"
#[test]
fn capability_manifest_has_one_entry_per_binding() {
    let n = 4;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    assert_eq!(
        artifact.capabilities_manifest.entries.len(),
        n,
        "manifest must have {n} entries for {n}-binding AnfIr"
    );
}

// Spec: each entry has the correct name and source_ref.
#[test]
fn capability_manifest_entries_have_correct_name_and_source_ref() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    for (i, entry) in artifact.capabilities_manifest.entries.iter().enumerate() {
        assert_eq!(
            entry.name,
            format!("fn_{i}"),
            "entry {i} must have name 'fn_{i}', got '{}'",
            entry.name
        );
        assert_eq!(
            entry.source_ref,
            NodeRef(i as u32),
            "entry {i} must have source_ref NodeRef({i}), got {:?}",
            entry.source_ref
        );
    }
}

// TRIANGULATE: empty AnfIr produces empty capability manifest.
#[test]
fn empty_anf_produces_empty_capability_manifest() {
    let anf = anf_for_n(0);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.capabilities_manifest.entries.is_empty(),
        "empty AnfIr must produce empty capability manifest"
    );
}

// ── Task 3.8: no runtime dependency ──────────────────────────────────────

// Spec: `cargo tree -p ail-compiler` must not contain public `wasmtime` or
// `wasmer` runtime crates.
// This test shells out to cargo tree and asserts the output.
//
// Cranelift 0.132 pulls `wasmtime-internal-*` crates from the shared
// Wasmtime/Cranelift codebase. Those are not the public Wasmtime runtime crate
// and are allowed here; this guard is about keeping runtime ownership in
// ail-runtime.
//
// Note: this test requires the Cargo workspace to be in scope and `cargo` on
// PATH.  It is intentionally a doc-comment style assertion running via
// `cargo test` so that CI can catch it without a separate shell step.
#[test]
fn ail_compiler_cargo_tree_does_not_contain_wasmtime_or_wasmer() {
    let output = std::process::Command::new("cargo")
        .args(["tree", "-p", "ail-compiler", "--no-dedupe"])
        .output()
        .expect("cargo tree must be runnable");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Match exact public runtime crate names in cargo tree lines. Do not treat
    // `wasmtime-internal-*` as `wasmtime`; Cranelift 0.132 depends on those
    // shared internal support crates without introducing the public runtime.
    let has_wasmtime = stdout.lines().any(|l| l.contains("wasmtime v"));
    let has_wasmer = stdout.lines().any(|l| l.contains("wasmer v"));

    assert!(
        !has_wasmtime,
        "ail-compiler cargo tree must not contain public 'wasmtime'; found:\n{stdout}"
    );
    assert!(
        !has_wasmer,
        "ail-compiler cargo tree must not contain public 'wasmer'; found:\n{stdout}"
    );
}

// ── Additional spec coverage ──────────────────────────────────────────────

// Spec: native_hash = blake3(anf_ir_hash || native_bytes) — verify by recomputing.
#[test]
fn native_hash_chain_matches_explicit_recomputation() {
    use ail_compiler::hash::hash_with_parent;

    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");

    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .expect("anf_ir_hash must be sealed before emit_native");

    let expected_hash = hash_with_parent(&anf_ir_hash, &artifact.native_bytes);

    assert_eq!(
        artifact.hash_chain.native_hash,
        Some(expected_hash),
        "native_hash must equal blake3(anf_ir_hash || native_bytes)"
    );
}

// Spec: native_bytes is non-empty for a module with functions.
#[test]
fn native_bytes_non_empty_for_nonempty_anf() {
    let anf = anf_for_n(1);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        !artifact.native_bytes.is_empty(),
        "native_bytes must be non-empty for a 1-binding AnfIr"
    );
}

// Spec: empty AnfIr still emits valid object bytes (object file header).
#[test]
fn empty_anf_still_emits_object_bytes() {
    let anf = anf_for_n(0);
    let artifact = emit_native(&anf).expect("emit_native");
    // Object file always has at minimum an ELF/Mach-O/COFF header.
    assert!(
        !artifact.native_bytes.is_empty(),
        "empty AnfIr must still emit a non-empty object file (header only)"
    );
}

// Spec: provenance NodeRefs match the source bindings.
#[test]
fn provenance_contains_correct_node_refs() {
    let n = 3;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");
    for i in 0..n as u32 {
        assert!(
            artifact.provenance.contains_key(&NodeRef(i)),
            "provenance must contain NodeRef({i})"
        );
    }
}

// Spec: provenance offsets are monotonically non-decreasing (binding order preserved).
#[test]
fn provenance_offsets_are_non_decreasing() {
    let anf = anf_for_n(4);
    let artifact = emit_native(&anf).expect("emit_native");

    let offsets: Vec<u64> = anf
        .bindings
        .iter()
        .map(|b| artifact.provenance[&b.source_ref])
        .collect();

    for w in offsets.windows(2) {
        assert!(
            w[1] >= w[0],
            "provenance offsets must be non-decreasing: {w:?}"
        );
    }
}

// Spec: CapabilitiesManifest and CapabilityEntry are serializable/deserializable (CBOR roundtrip).
#[test]
fn capability_manifest_serialization_roundtrip() {
    let manifest = CapabilitiesManifest {
        entries: vec![
            CapabilityEntry {
                name: "fn_a".to_string(),
                source_ref: NodeRef(0),
            },
            CapabilityEntry {
                name: "fn_b".to_string(),
                source_ref: NodeRef(1),
            },
        ],
    };

    // Serialize to CBOR bytes (ciborium is already a workspace dep).
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&manifest, &mut buf).expect("serialize to CBOR");

    // Deserialize back.
    let roundtrip: CapabilitiesManifest =
        ciborium::de::from_reader(buf.as_slice()).expect("deserialize from CBOR");
    assert_eq!(manifest, roundtrip, "manifest must roundtrip through CBOR");
}

#[test]
fn native_constructor_payload_match_does_not_lower_as_zero_pattern() {
    let unsupported = emit_native(&match_zero_with_pattern("Ok(value)"))
        .expect("unsupported constructor payload match must compile to trap");
    let zero = emit_native(&match_zero_with_pattern("0")).expect("numeric zero match must compile");

    assert_ne!(
        unsupported.native_bytes, zero.native_bytes,
        "unsupported constructor payload patterns must not silently lower as numeric 0"
    );
}
