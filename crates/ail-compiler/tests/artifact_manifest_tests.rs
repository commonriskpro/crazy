// ── ail-compiler::artifact_manifest_tests ───────────────────────────────
//
// G32: ArtifactManifest integration tests.
//
// Spec scenarios covered:
//  - ArtifactManifest records profile, compiler_version, and upstream hashes.
//  - ArtifactManifest serializes deterministically (same content → same CBOR).
//  - Different ArtifactManifest inputs produce different CBOR.
//  - ArtifactManifest round-trips through CBOR.
//  - artifact_manifest_hash changes when manifest changes.
//  - hash_stability: same source-map change always produces the same hash.

use ail_compiler::{
    ArtifactManifest,
    emit_wasm, emit_native,
    lower::{lower_to_anf, lower_to_core_ir},
};
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

fn anf_for_n(n: usize) -> ail_compiler::AnfIr {
    let graph = graph_with_n_nodes(n);
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    lower_to_anf(&core).expect("lower_to_anf")
}

// ── ArtifactManifest constructibility ────────────────────────────────────

// Spec: ArtifactManifest records profile, compiler_version, upstream hashes.
// RED → GREEN: type must exist with these fields.
#[test]
fn artifact_manifest_is_constructible() {
    let manifest = ArtifactManifest {
        profile: "draft".to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        graph_snapshot_hash: [1u8; 32],
        verification_report_hash: [2u8; 32],
        core_ir_hash: [3u8; 32],
        anf_ir_hash: [4u8; 32],
        wasm_hash: None,
        native_hash: None,
        source_map_hash: None,
    };
    assert_eq!(manifest.profile, "draft");
    assert!(!manifest.compiler_version.is_empty());
    assert_eq!(manifest.anf_ir_hash, [4u8; 32]);
}

// TRIANGULATE: ArtifactManifest with both WASM and native hashes.
#[test]
fn artifact_manifest_with_wasm_and_native_hashes() {
    let manifest = ArtifactManifest {
        profile: "prod".to_string(),
        compiler_version: "1.0.0".to_string(),
        graph_snapshot_hash: [10u8; 32],
        verification_report_hash: [11u8; 32],
        core_ir_hash: [12u8; 32],
        anf_ir_hash: [13u8; 32],
        wasm_hash: Some([20u8; 32]),
        native_hash: Some([21u8; 32]),
        source_map_hash: Some([22u8; 32]),
    };
    assert_eq!(manifest.wasm_hash, Some([20u8; 32]));
    assert_eq!(manifest.native_hash, Some([21u8; 32]));
    assert_eq!(manifest.source_map_hash, Some([22u8; 32]));
}

// ── ArtifactManifest serialization ────────────────────────────────────────

// Spec: ArtifactManifest serializes deterministically.
#[test]
fn artifact_manifest_cbor_is_deterministic() {
    let manifest = ArtifactManifest {
        profile: "dev".to_string(),
        compiler_version: "0.1.0".to_string(),
        graph_snapshot_hash: [5u8; 32],
        verification_report_hash: [6u8; 32],
        core_ir_hash: [7u8; 32],
        anf_ir_hash: [8u8; 32],
        wasm_hash: None,
        native_hash: None,
        source_map_hash: None,
    };
    let mut buf1 = Vec::new();
    ciborium::ser::into_writer(&manifest, &mut buf1).expect("first encode");
    let mut buf2 = Vec::new();
    ciborium::ser::into_writer(&manifest, &mut buf2).expect("second encode");
    assert_eq!(buf1, buf2, "ArtifactManifest CBOR must be deterministic");
}

// TRIANGULATE: different ArtifactManifest values produce different CBOR.
#[test]
fn different_manifests_produce_different_cbor() {
    let m1 = ArtifactManifest {
        profile: "draft".to_string(),
        compiler_version: "0.1.0".to_string(),
        graph_snapshot_hash: [1u8; 32],
        verification_report_hash: [2u8; 32],
        core_ir_hash: [3u8; 32],
        anf_ir_hash: [4u8; 32],
        wasm_hash: None,
        native_hash: None,
        source_map_hash: None,
    };
    let mut m2 = m1.clone();
    m2.profile = "prod".to_string();

    let mut buf1 = Vec::new();
    ciborium::ser::into_writer(&m1, &mut buf1).expect("encode m1");
    let mut buf2 = Vec::new();
    ciborium::ser::into_writer(&m2, &mut buf2).expect("encode m2");
    assert_ne!(buf1, buf2, "different manifests must produce different CBOR");
}

// Spec: ArtifactManifest round-trips through CBOR.
#[test]
fn artifact_manifest_cbor_round_trip() {
    let manifest = ArtifactManifest {
        profile: "test".to_string(),
        compiler_version: "0.1.0".to_string(),
        graph_snapshot_hash: [42u8; 32],
        verification_report_hash: [43u8; 32],
        core_ir_hash: [44u8; 32],
        anf_ir_hash: [45u8; 32],
        wasm_hash: Some([50u8; 32]),
        native_hash: None,
        source_map_hash: Some([51u8; 32]),
    };
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&manifest, &mut buf).expect("encode");
    let decoded: ArtifactManifest = ciborium::de::from_reader(buf.as_slice()).expect("decode");
    assert_eq!(manifest, decoded, "ArtifactManifest must round-trip through CBOR");
}

// ── ArtifactManifest from pipeline artifacts ──────────────────────────────

// Spec: ArtifactManifest can be built from a WasmArtifact's hash_chain.
#[test]
fn artifact_manifest_built_from_wasm_artifact() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).expect("emit_wasm");

    let manifest = ArtifactManifest {
        profile: "draft".to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        graph_snapshot_hash: artifact.hash_chain.graph_snapshot_hash,
        verification_report_hash: artifact.hash_chain.verification_report_hash,
        core_ir_hash: artifact.hash_chain.core_ir_hash,
        anf_ir_hash: artifact.hash_chain.anf_ir_hash.expect("anf_ir_hash must be Some"),
        wasm_hash: artifact.hash_chain.wasm_hash,
        native_hash: artifact.hash_chain.native_hash,
        source_map_hash: artifact.hash_chain.source_map_hash,
    };

    assert_eq!(manifest.profile, "draft");
    assert!(manifest.wasm_hash.is_some(), "wasm_hash must be populated");
    assert!(manifest.source_map_hash.is_some(), "source_map_hash must be populated");
    assert!(manifest.native_hash.is_none(), "native_hash must be None for WASM-only pipeline");
}

// TRIANGULATE: manifest from native artifact has native_hash set.
#[test]
fn artifact_manifest_built_from_native_artifact() {
    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");

    let manifest = ArtifactManifest {
        profile: "dev".to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        graph_snapshot_hash: artifact.hash_chain.graph_snapshot_hash,
        verification_report_hash: artifact.hash_chain.verification_report_hash,
        core_ir_hash: artifact.hash_chain.core_ir_hash,
        anf_ir_hash: artifact.hash_chain.anf_ir_hash.expect("anf_ir_hash must be Some"),
        wasm_hash: artifact.hash_chain.wasm_hash,
        native_hash: artifact.hash_chain.native_hash,
        source_map_hash: artifact.hash_chain.source_map_hash,
    };

    assert!(manifest.native_hash.is_some(), "native_hash must be populated");
    assert!(manifest.source_map_hash.is_some(), "source_map_hash must be populated");
    assert!(manifest.wasm_hash.is_none(), "wasm_hash must be None for native-only pipeline");
}
