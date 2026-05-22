// ── ail-compiler::artifact_manifest ──────────────────────────────────────
//
// ArtifactManifest — the profile-bound metadata record emitted alongside
// every compiled artifact.
//
// # Purpose
//
// Records the verification profile, compiler version, and the full hash chain
// (all upstream artifact hashes) so that:
//   - Tooling can verify artifact provenance without re-running the pipeline.
//   - Artifact promotion from draft → prod can be rejected if hashes mismatch.
//   - Source map integrity can be verified by checking `source_map_hash`.
//
// # Design constraints
//
// - Deterministic CBOR serialization: `Vec` only (no `HashMap`).
// - All fields use `#[serde(default, skip_serializing_if = "Option::is_none")]`
//   for optional hashes so forward-compat is preserved.
// - Does NOT include the `wasm` / `native_bytes` themselves — only hashes.
//
// # docs/compiler.md §Profile-bound artifacts
//
// Every executable artifact records:
//   target_profile, compiler_version, graph_snapshot_hash, core_ir_hash,
//   anf_ir_hash, capabilities_manifest_hash (future).

use serde::{Deserialize, Serialize};

// ── ArtifactManifest ─────────────────────────────────────────────────────

/// Profile-bound metadata record for a compiled artifact.
///
/// Emitted alongside WASM / native artifacts as `program.artifact.json`
/// (JSON sidecar serialized from this struct via `serde_json`).
///
/// Keeps the full hash chain so downstream tooling can verify provenance
/// without access to the intermediate IR stages.
///
/// # Backward Compatibility
///
/// Optional hash fields (`wasm_hash`, `native_hash`, `source_map_hash`)
/// are serialized only when `Some`.  Older readers that do not know these
/// fields will silently skip them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    /// Verification profile this artifact was compiled for.
    ///
    /// Examples: `"draft"`, `"dev"`, `"test"`, `"prod"`, `"critical"`.
    /// Runtime rejects artifacts whose profile doesn't match the host profile.
    pub profile: String,

    /// Semver string of the `ail-compiler` crate that produced this artifact.
    ///
    /// Set to `env!("CARGO_PKG_VERSION")` at compile time by callers.
    pub compiler_version: String,

    /// BLAKE3 hash of the serialized `SemanticGraph` (pipeline input).
    pub graph_snapshot_hash: [u8; 32],

    /// BLAKE3 hash of the serialized `VerificationReport` (pipeline input).
    pub verification_report_hash: [u8; 32],

    /// `blake3(graph_snapshot_hash || core_ir_bytes)` — Core IR stage seal.
    pub core_ir_hash: [u8; 32],

    /// `blake3(core_ir_hash || anf_ir_bytes)` — ANF IR stage seal.
    pub anf_ir_hash: [u8; 32],

    /// `blake3(anf_ir_hash || wasm_binary)` — WASM backend seal.
    ///
    /// `None` when no WASM artifact was produced (native-only pipeline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_hash: Option<[u8; 32]>,

    /// `blake3(anf_ir_hash || native_bytes)` — native backend seal.
    ///
    /// `None` when no native artifact was produced (WASM-only pipeline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_hash: Option<[u8; 32]>,

    /// `blake3(source_map_cbor_bytes)` — semantic source map content seal.
    ///
    /// Changes whenever any `wasm_offset`, `native_offset`, `block_ref`,
    /// `contract_ref`, or other provenance field in the source map changes.
    /// `None` when source map hashing is not yet performed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map_hash: Option<[u8; 32]>,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::stable_cbor_bytes;

    // Spec: ArtifactManifest is constructible with all required fields.
    // RED → GREEN: type must exist with these exact field names and types.
    #[test]
    fn artifact_manifest_is_constructible_with_required_fields() {
        let m = ArtifactManifest {
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
        assert_eq!(m.profile, "draft");
        assert_eq!(m.compiler_version, "0.1.0");
    }

    // TRIANGULATE: ArtifactManifest with all optional fields populated.
    #[test]
    fn artifact_manifest_with_all_optional_fields() {
        let m = ArtifactManifest {
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
        assert_eq!(m.wasm_hash, Some([20u8; 32]));
        assert_eq!(m.native_hash, Some([21u8; 32]));
        assert_eq!(m.source_map_hash, Some([22u8; 32]));
    }

    // Spec: CBOR encoding is deterministic.
    #[test]
    fn artifact_manifest_cbor_is_deterministic() {
        let m = ArtifactManifest {
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
        let b1 = stable_cbor_bytes(&m).expect("first encode");
        let b2 = stable_cbor_bytes(&m).expect("second encode");
        assert_eq!(b1, b2, "ArtifactManifest CBOR must be deterministic");
    }

    // TRIANGULATE: different manifests produce different CBOR.
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
        let b1 = stable_cbor_bytes(&m1).expect("encode m1");
        let b2 = stable_cbor_bytes(&m2).expect("encode m2");
        assert_ne!(b1, b2, "different manifests must produce different CBOR");
    }

    // Spec: ArtifactManifest round-trips through CBOR.
    #[test]
    fn artifact_manifest_cbor_round_trip() {
        let m = ArtifactManifest {
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
        let bytes = stable_cbor_bytes(&m).expect("encode");
        let decoded: ArtifactManifest =
            ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(m, decoded, "ArtifactManifest must round-trip through CBOR");
    }

    // Spec: Optional fields are omitted from CBOR when None.
    // A manifest without optional fields must encode to fewer bytes than one with them.
    #[test]
    fn optional_fields_absent_from_cbor_when_none() {
        let minimal = ArtifactManifest {
            profile: "draft".to_string(),
            compiler_version: "0.1.0".to_string(),
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [0u8; 32],
            anf_ir_hash: [0u8; 32],
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
        };
        let mut full = minimal.clone();
        full.wasm_hash = Some([99u8; 32]);
        full.native_hash = Some([98u8; 32]);
        full.source_map_hash = Some([97u8; 32]);

        let b_min = stable_cbor_bytes(&minimal).expect("encode minimal");
        let b_full = stable_cbor_bytes(&full).expect("encode full");
        assert!(
            b_min.len() < b_full.len(),
            "manifest with optional fields must encode to more bytes: {} vs {}",
            b_min.len(),
            b_full.len()
        );
    }
}
