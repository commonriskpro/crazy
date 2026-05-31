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

    /// `blake3(capabilities_manifest_cbor_bytes)` — capability manifest seal.
    ///
    /// `None` only for legacy artifacts or backends that do not emit capability
    /// sidecars yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities_manifest_hash: Option<[u8; 32]>,
}

// ── ArtifactManifest validation ─────────────────────────────────────────

/// Current package-facing schema identifier for artifact manifest validation.
///
/// Kept outside `ArtifactManifest` so older manifest sidecars can still
/// deserialize while package tooling gets a stable gate for schema envelopes.
pub const ARTIFACT_MANIFEST_SCHEMA_VERSION: &str = "artifact-manifest/1.0";

/// Stable issue code for package entries that reference the same artifact id.
pub const E_ARTIFACT_MANIFEST_DUPLICATE_ARTIFACT: &str = "E_ARTIFACT_MANIFEST_DUPLICATE_ARTIFACT";

/// Stable issue code for package manifests missing production hash seals.
pub const E_ARTIFACT_MANIFEST_MISSING_HASH: &str = "E_ARTIFACT_MANIFEST_MISSING_HASH";

/// Stable issue code for package manifest schema envelope mismatches.
pub const E_ARTIFACT_MANIFEST_SCHEMA_MISMATCH: &str = "E_ARTIFACT_MANIFEST_SCHEMA_MISMATCH";

/// A manifest plus package-envelope metadata used by package integration gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactManifestValidationEntry<'a> {
    /// Stable package artifact id/path for duplicate detection and diagnostics.
    pub artifact_id: &'a str,
    /// Schema id from the package manifest envelope.
    pub schema_version: &'a str,
    /// Compiler-emitted manifest sidecar to validate.
    pub manifest: &'a ArtifactManifest,
}

impl<'a> ArtifactManifestValidationEntry<'a> {
    /// Build a validation entry for one package artifact manifest sidecar.
    pub fn new(
        artifact_id: &'a str,
        schema_version: &'a str,
        manifest: &'a ArtifactManifest,
    ) -> Self {
        Self {
            artifact_id,
            schema_version,
            manifest,
        }
    }
}

/// Machine-readable artifact manifest validation issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifestValidationIssue {
    /// Stable issue code for downstream policy gates.
    pub code: String,
    /// Package artifact id/path that owns the issue.
    pub artifact_id: String,
    /// Manifest or envelope field that failed validation.
    pub field: String,
    /// Human-readable explanation for logs and reports.
    pub message: String,
}

impl ArtifactManifestValidationIssue {
    fn new(
        code: &'static str,
        artifact_id: impl Into<String>,
        field: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            artifact_id: artifact_id.into(),
            field: field.to_string(),
            message: message.into(),
        }
    }

    fn sort_key(&self) -> (&str, &str, &str, &str) {
        (&self.code, &self.artifact_id, &self.field, &self.message)
    }
}

/// Validate package-facing artifact manifests for reproducibility gates.
///
/// The gate is intentionally stricter than deserialization compatibility:
/// legacy manifests may deserialize with missing optional hashes, but package
/// integration must surface stable issue codes before publish/promotion.
pub fn validate_artifact_manifest_entries(
    entries: &[ArtifactManifestValidationEntry<'_>],
) -> Vec<ArtifactManifestValidationIssue> {
    use std::collections::BTreeMap;

    let mut issues = Vec::new();
    let mut artifact_counts: BTreeMap<&str, usize> = BTreeMap::new();

    for entry in entries {
        *artifact_counts.entry(entry.artifact_id).or_default() += 1;

        if entry.schema_version != ARTIFACT_MANIFEST_SCHEMA_VERSION {
            issues.push(ArtifactManifestValidationIssue::new(
                E_ARTIFACT_MANIFEST_SCHEMA_MISMATCH,
                entry.artifact_id,
                "schema_version",
                format!(
                    "expected schema {ARTIFACT_MANIFEST_SCHEMA_VERSION}, found {}",
                    entry.schema_version
                ),
            ));
        }

        if entry.manifest.capabilities_manifest_hash.is_none() {
            issues.push(ArtifactManifestValidationIssue::new(
                E_ARTIFACT_MANIFEST_MISSING_HASH,
                entry.artifact_id,
                "capabilities_manifest_hash",
                "capabilities_manifest_hash is required for package promotion",
            ));
        }

        if entry.manifest.source_map_hash.is_none() {
            issues.push(ArtifactManifestValidationIssue::new(
                E_ARTIFACT_MANIFEST_MISSING_HASH,
                entry.artifact_id,
                "source_map_hash",
                "source_map_hash is required for package promotion",
            ));
        }

        if entry.manifest.wasm_hash.is_none() && entry.manifest.native_hash.is_none() {
            issues.push(ArtifactManifestValidationIssue::new(
                E_ARTIFACT_MANIFEST_MISSING_HASH,
                entry.artifact_id,
                "wasm_hash|native_hash",
                "at least one backend artifact hash is required for package promotion",
            ));
        }
    }

    for (artifact_id, count) in artifact_counts {
        if count > 1 {
            issues.push(ArtifactManifestValidationIssue::new(
                E_ARTIFACT_MANIFEST_DUPLICATE_ARTIFACT,
                artifact_id,
                "artifact_id",
                format!("artifact id appears {count} times in package manifest"),
            ));
        }
    }

    issues.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    issues
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::stable_cbor_bytes;

    fn manifest_with_backend_hash() -> ArtifactManifest {
        ArtifactManifest {
            profile: "prod".to_string(),
            compiler_version: "1.0.0".to_string(),
            graph_snapshot_hash: [1u8; 32],
            verification_report_hash: [2u8; 32],
            core_ir_hash: [3u8; 32],
            anf_ir_hash: [4u8; 32],
            wasm_hash: Some([5u8; 32]),
            native_hash: None,
            source_map_hash: Some([6u8; 32]),
            capabilities_manifest_hash: Some([7u8; 32]),
        }
    }

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
            capabilities_manifest_hash: None,
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
            capabilities_manifest_hash: Some([23u8; 32]),
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
            capabilities_manifest_hash: None,
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
            capabilities_manifest_hash: None,
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
            capabilities_manifest_hash: None,
        };
        let bytes = stable_cbor_bytes(&m).expect("encode");
        let decoded: ArtifactManifest = ciborium::from_reader(bytes.as_slice()).expect("decode");
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
            capabilities_manifest_hash: None,
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

    #[test]
    fn validation_reports_missing_hashes_with_stable_codes() {
        let manifest = ArtifactManifest {
            profile: "prod".to_string(),
            compiler_version: "1.0.0".to_string(),
            graph_snapshot_hash: [1u8; 32],
            verification_report_hash: [2u8; 32],
            core_ir_hash: [3u8; 32],
            anf_ir_hash: [4u8; 32],
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            capabilities_manifest_hash: None,
        };

        let issues = validate_artifact_manifest_entries(&[ArtifactManifestValidationEntry::new(
            "program.wasm",
            ARTIFACT_MANIFEST_SCHEMA_VERSION,
            &manifest,
        )]);

        let issue_keys: Vec<(&str, &str)> = issues
            .iter()
            .map(|issue| (issue.code.as_str(), issue.field.as_str()))
            .collect();

        assert_eq!(
            issue_keys,
            vec![
                (
                    E_ARTIFACT_MANIFEST_MISSING_HASH,
                    "capabilities_manifest_hash",
                ),
                (E_ARTIFACT_MANIFEST_MISSING_HASH, "source_map_hash"),
                (E_ARTIFACT_MANIFEST_MISSING_HASH, "wasm_hash|native_hash"),
            ]
        );
    }

    #[test]
    fn validation_reports_schema_mismatch_and_duplicate_artifacts() {
        let manifest = manifest_with_backend_hash();
        let issues = validate_artifact_manifest_entries(&[
            ArtifactManifestValidationEntry::new(
                "program.wasm",
                "artifact-manifest/0.9",
                &manifest,
            ),
            ArtifactManifestValidationEntry::new(
                "program.wasm",
                ARTIFACT_MANIFEST_SCHEMA_VERSION,
                &manifest,
            ),
        ]);

        let issue_keys: Vec<(&str, &str, &str)> = issues
            .iter()
            .map(|issue| {
                (
                    issue.code.as_str(),
                    issue.artifact_id.as_str(),
                    issue.field.as_str(),
                )
            })
            .collect();

        assert_eq!(
            issue_keys,
            vec![
                (
                    E_ARTIFACT_MANIFEST_DUPLICATE_ARTIFACT,
                    "program.wasm",
                    "artifact_id",
                ),
                (
                    E_ARTIFACT_MANIFEST_SCHEMA_MISMATCH,
                    "program.wasm",
                    "schema_version",
                ),
            ]
        );
    }

    #[test]
    fn validation_orders_issues_deterministically() {
        let mut missing = manifest_with_backend_hash();
        missing.wasm_hash = None;
        missing.source_map_hash = None;
        missing.capabilities_manifest_hash = None;

        let complete = manifest_with_backend_hash();
        let issues = validate_artifact_manifest_entries(&[
            ArtifactManifestValidationEntry::new("zeta.wasm", "artifact-manifest/0.8", &missing),
            ArtifactManifestValidationEntry::new("alpha.wasm", "artifact-manifest/0.7", &complete),
            ArtifactManifestValidationEntry::new(
                "alpha.wasm",
                ARTIFACT_MANIFEST_SCHEMA_VERSION,
                &complete,
            ),
        ]);
        let reversed_issues = validate_artifact_manifest_entries(&[
            ArtifactManifestValidationEntry::new(
                "alpha.wasm",
                ARTIFACT_MANIFEST_SCHEMA_VERSION,
                &complete,
            ),
            ArtifactManifestValidationEntry::new("alpha.wasm", "artifact-manifest/0.7", &complete),
            ArtifactManifestValidationEntry::new("zeta.wasm", "artifact-manifest/0.8", &missing),
        ]);

        let sorted_issue_keys = issues
            .windows(2)
            .all(|pair| pair[0].sort_key() <= pair[1].sort_key());

        assert!(
            sorted_issue_keys,
            "artifact manifest validation issues must have deterministic ordering: {issues:?}"
        );
        assert_eq!(
            issues, reversed_issues,
            "validation issues must not depend on package entry traversal order"
        );
    }
}
