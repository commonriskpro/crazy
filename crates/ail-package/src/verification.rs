// ── ail-package::verification ─────────────────────────────────────────────
//
// `PackageVerificationReport` — full verification evidence for a package
// release, hash-bound and content-addressed.
//
// # Design (docs/packages.md §Package verification report)
//
// Package release includes:
//   package_verification_report
//     package payments.stripe
//     version 1.2.0
//     exports_verified [...]
//     effects_declared [...]
//     assumptions [...]
//     unsafe_surface [...]
//     artifact_hashes [...]
//   end
//
// The report is hash-bound: its BLAKE3 digest is stored in
// `LockfileEntry.verification_report_hash`.

use blake3::Hasher;
use ciborium::ser::into_writer;
use serde::{Deserialize, Serialize};

use crate::manifest::{PackageManifest, ReproducibleBuildEvidence};
use crate::trust::TrustLevel;

// ── PackageVerificationReport ─────────────────────────────────────────────

/// Full verification evidence produced during a package release.
///
/// The report is content-addressed: [`PackageVerificationReport::blake3_hex`]
/// returns a deterministic BLAKE3 digest of the canonical CBOR encoding.
///
/// See `docs/packages.md` §Package verification report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageVerificationReport {
    /// Name of the package this report covers (e.g., `"payments.stripe"`).
    pub package: String,
    /// Version of the package this report covers (e.g., `"1.2.0"`).
    pub version: String,
    /// Names of exports for which verification evidence was accepted.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub exports_verified: Vec<String>,
    /// Effect tokens that are declared and present in verification evidence.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub effects_declared: Vec<String>,
    /// Assumption IDs included in the release.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub assumptions: Vec<String>,
    /// Unsafe surface entries as name strings (e.g., `"fn.native_hash"`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unsafe_surface: Vec<String>,
    /// BLAKE3 hex digests of release artifacts (role → hash strings).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artifact_hashes: Vec<String>,
}

impl PackageVerificationReport {
    /// Compute the BLAKE3 content hash of this report as a hex-encoded string.
    ///
    /// The hash covers the canonical CBOR serialization of the full report.
    ///
    /// # Errors
    ///
    /// Returns `Err` if CBOR serialization fails.
    pub fn blake3_hex(&self) -> Result<String, String> {
        let mut buf = Vec::new();
        into_writer(self, &mut buf).map_err(|e| format!("CBOR serialization failed: {e}"))?;
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        Ok(hasher.finalize().to_hex().to_string())
    }
}

// ── Verified package evidence preflight ───────────────────────────────────

/// Local evidence validation failures for `TrustLevel::Verified` packages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageVerificationEvidenceError {
    /// A verified package did not embed a verification report.
    MissingReport,
    /// Report package name does not match the manifest package name.
    PackageMismatch { expected: String, actual: String },
    /// Report version does not match the manifest version.
    VersionMismatch { expected: String, actual: String },
    /// Verified manifests must bind at least one release artifact hash.
    ManifestArtifactHashesMissing,
    /// Report artifact evidence does not match manifest artifact hashes.
    ArtifactHashesMismatch {
        manifest_hashes: Vec<String>,
        report_hashes: Vec<String>,
    },
    // ── 4G errors ─────────────────────────────────────────────────────────
    /// A verified package is missing required reproducible-build evidence.
    ///
    /// `TrustLevel::Verified` packages must carry a
    /// [`crate::manifest::ReproducibleBuildEvidence`] record so reviewers
    /// can reason about build determinism locally.
    MissingReproducibleEvidence,
    /// A reproducible-build evidence field has an invalid format.
    ///
    /// All hash fields must be exactly 64 lower-case ASCII hex characters;
    /// `toolchain_id` must be non-empty.
    ReproducibleEvidenceInvalidFormat {
        /// Name of the field that failed format validation.
        field: &'static str,
        /// Short reason for the failure (e.g., `"expected 64-char hex string"`).
        reason: &'static str,
    },
    /// `build_inputs_hash` does not match the value derived from
    /// `source_digest` and `toolchain_id`.
    ///
    /// The canonical formula is
    /// `BLAKE3(source_digest_utf8_bytes || toolchain_id_utf8_bytes)`.
    ReproducibleEvidenceBuildInputsMismatch {
        /// Expected `build_inputs_hash` (derived from evidence fields).
        expected: String,
        /// Actual `build_inputs_hash` stored in the evidence record.
        actual: String,
    },
}

impl std::fmt::Display for PackageVerificationEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageVerificationEvidenceError::MissingReport => {
                write!(f, "verified package is missing verification_report")
            }
            PackageVerificationEvidenceError::PackageMismatch { expected, actual } => write!(
                f,
                "verification_report package mismatch: expected `{expected}`, got `{actual}`"
            ),
            PackageVerificationEvidenceError::VersionMismatch { expected, actual } => write!(
                f,
                "verification_report version mismatch: expected `{expected}`, got `{actual}`"
            ),
            PackageVerificationEvidenceError::ManifestArtifactHashesMissing => {
                write!(f, "verified package manifest must declare artifact_hashes")
            }
            PackageVerificationEvidenceError::ArtifactHashesMismatch {
                manifest_hashes,
                report_hashes,
            } => write!(
                f,
                "verification_report artifact hashes do not match manifest artifact_hashes: manifest={manifest_hashes:?}, report={report_hashes:?}"
            ),
            // 4G errors
            PackageVerificationEvidenceError::MissingReproducibleEvidence => write!(
                f,
                "verified package is missing required reproducible_evidence \
                 (local metadata only; no rebuild or remote attestation is performed)"
            ),
            PackageVerificationEvidenceError::ReproducibleEvidenceInvalidFormat {
                field,
                reason,
            } => write!(
                f,
                "reproducible_evidence field `{field}` has invalid format: {reason}"
            ),
            PackageVerificationEvidenceError::ReproducibleEvidenceBuildInputsMismatch {
                expected,
                actual,
            } => write!(
                f,
                "reproducible_evidence build_inputs_hash mismatch: \
                 expected `{expected}` (derived from source_digest + toolchain_id), \
                 got `{actual}`"
            ),
        }
    }
}

impl std::error::Error for PackageVerificationEvidenceError {}

/// Validate local verification evidence for a package manifest.
///
/// Lower trust tiers intentionally pass through unchanged. For
/// `TrustLevel::Verified`, the local manifest must carry a verification report
/// for the same package/version and the report's hash-only artifact evidence
/// must equal the manifest's role-bound artifact hashes, ignoring order.
pub fn validate_verified_package_evidence(
    manifest: &PackageManifest,
) -> Result<(), PackageVerificationEvidenceError> {
    if manifest.trust_level != TrustLevel::Verified {
        return Ok(());
    }

    let report = manifest
        .verification_report
        .as_ref()
        .ok_or(PackageVerificationEvidenceError::MissingReport)?;

    if report.package != manifest.name {
        return Err(PackageVerificationEvidenceError::PackageMismatch {
            expected: manifest.name.clone(),
            actual: report.package.clone(),
        });
    }

    if report.version != manifest.version {
        return Err(PackageVerificationEvidenceError::VersionMismatch {
            expected: manifest.version.clone(),
            actual: report.version.clone(),
        });
    }

    if manifest.artifact_hashes.is_empty() {
        return Err(PackageVerificationEvidenceError::ManifestArtifactHashesMissing);
    }

    let mut manifest_hashes = manifest
        .artifact_hashes
        .iter()
        .map(|entry| entry.hash.clone())
        .collect::<Vec<_>>();
    manifest_hashes.sort();

    let mut report_hashes = report.artifact_hashes.clone();
    report_hashes.sort();

    if manifest_hashes != report_hashes {
        return Err(PackageVerificationEvidenceError::ArtifactHashesMismatch {
            manifest_hashes,
            report_hashes,
        });
    }

    // ── 4G: reproducible-build evidence ───────────────────────────────────
    //
    // Conservative policy: TrustLevel::Verified packages MUST carry a
    // ReproducibleBuildEvidence record.  All hash fields must be 64-char
    // lower-case hex; toolchain_id must be non-empty.  build_inputs_hash must
    // equal the value derived from source_digest and toolchain_id.
    //
    // This is LOCAL metadata only — no rebuild is executed, no remote
    // attestation is consulted.
    let evidence = manifest
        .reproducible_evidence
        .as_ref()
        .ok_or(PackageVerificationEvidenceError::MissingReproducibleEvidence)?;

    validate_reproducible_evidence_fields(evidence)?;

    let derived_inputs_hash = ReproducibleBuildEvidence::compute_build_inputs_hash(
        &evidence.source_digest,
        &evidence.toolchain_id,
    );
    if evidence.build_inputs_hash != derived_inputs_hash {
        return Err(
            PackageVerificationEvidenceError::ReproducibleEvidenceBuildInputsMismatch {
                expected: derived_inputs_hash,
                actual: evidence.build_inputs_hash.clone(),
            },
        );
    }

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Return `true` if `s` is a valid BLAKE3 hex digest (64 lower-case hex chars).
fn is_blake3_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Validate the field formats of a [`ReproducibleBuildEvidence`] record.
fn validate_reproducible_evidence_fields(
    evidence: &ReproducibleBuildEvidence,
) -> Result<(), PackageVerificationEvidenceError> {
    if !is_blake3_hex(&evidence.build_inputs_hash) {
        return Err(
            PackageVerificationEvidenceError::ReproducibleEvidenceInvalidFormat {
                field: "build_inputs_hash",
                reason: "expected 64-char lower-case hex string",
            },
        );
    }
    if evidence.toolchain_id.is_empty() {
        return Err(
            PackageVerificationEvidenceError::ReproducibleEvidenceInvalidFormat {
                field: "toolchain_id",
                reason: "must be non-empty",
            },
        );
    }
    if !is_blake3_hex(&evidence.source_digest) {
        return Err(
            PackageVerificationEvidenceError::ReproducibleEvidenceInvalidFormat {
                field: "source_digest",
                reason: "expected 64-char lower-case hex string",
            },
        );
    }
    if !is_blake3_hex(&evidence.recipe_hash) {
        return Err(
            PackageVerificationEvidenceError::ReproducibleEvidenceInvalidFormat {
                field: "recipe_hash",
                reason: "expected 64-char lower-case hex string",
            },
        );
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::manifest::{ArtifactHashEntry, PackageDef, PackageManifest};
    use crate::trust::TrustLevel;

    fn sample_report() -> PackageVerificationReport {
        PackageVerificationReport {
            package: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            exports_verified: vec!["charge".to_string(), "refund".to_string()],
            effects_declared: vec!["payment.charge:PaymentProvider".to_string()],
            assumptions: vec!["stripe_idempotency".to_string()],
            unsafe_surface: vec![],
            artifact_hashes: vec!["a".repeat(64)],
        }
    }

    // ── verification_report_cbor_round_trip ───────────────────────────────
    // Spec scenario: "PackageVerificationReport round-trips through CBOR"
    //   GIVEN a PackageVerificationReport with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn verification_report_cbor_round_trip() {
        let original = sample_report();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialization must succeed");

        let decoded: PackageVerificationReport =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original);
    }

    // ── verification_report_cbor_is_deterministic ─────────────────────────
    // TRIANGULATE: encoding the same report twice yields identical bytes.
    #[test]
    fn verification_report_cbor_is_deterministic() {
        let report = sample_report();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&report, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&report, &mut buf2).expect("second encode");

        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }

    // ── empty_report_is_valid ─────────────────────────────────────────────
    // TRIANGULATE: a report with empty optional lists is valid and round-trips.
    #[test]
    fn empty_report_is_valid() {
        let report = PackageVerificationReport {
            package: "utils.core".to_string(),
            version: "0.1.0".to_string(),
            exports_verified: vec![],
            effects_declared: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&report, &mut buf).expect("encode");
        let decoded: PackageVerificationReport =
            ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert!(decoded.exports_verified.is_empty());
        assert!(decoded.effects_declared.is_empty());
    }

    // ── verification_report_is_hash_bound ────────────────────────────────
    // Spec scenario: "Verification report is hash-bound"
    //   GIVEN a PackageVerificationReport
    //   WHEN blake3_hex() is called
    //   THEN it returns a 64-char hex string deterministically
    #[test]
    fn verification_report_is_hash_bound() {
        let r1 = sample_report();
        let r2 = sample_report();
        let h1 = r1.blake3_hex().expect("hash must succeed");
        let h2 = r2.blake3_hex().expect("hash must succeed");
        assert_eq!(h1.len(), 64);
        assert_eq!(h1, h2, "same report must hash to same value");
    }

    // ── different_reports_produce_different_hashes ────────────────────────
    // TRIANGULATE: mutating a field changes the hash
    #[test]
    fn different_reports_produce_different_hashes() {
        let r1 = sample_report();
        let mut r2 = sample_report();
        r2.version = "2.0.0".to_string();
        assert_ne!(
            r1.blake3_hex().unwrap(),
            r2.blake3_hex().unwrap(),
            "different version must produce different hash"
        );
    }

    // ── report_includes_package_and_version ───────────────────────────────
    // Spec scenario: report fields match the doc example
    #[test]
    fn report_includes_package_and_version() {
        let r = sample_report();
        assert_eq!(r.package, "payments.stripe");
        assert_eq!(r.version, "1.2.0");
        assert!(r.exports_verified.contains(&"charge".to_string()));
    }

    /// Standard artifact hash used in test fixtures.
    fn test_hash() -> String {
        "a".repeat(64)
    }

    /// Standard source digest and toolchain for evidence fixtures.
    fn sample_evidence() -> ReproducibleBuildEvidence {
        ReproducibleBuildEvidence::new("b".repeat(64), "rustc-1.77.0-stable", "c".repeat(64))
    }

    fn package_with_evidence(
        trust_level: TrustLevel,
        artifact_hashes: Vec<ArtifactHashEntry>,
        verification_report: Option<PackageVerificationReport>,
        reproducible_evidence: Option<ReproducibleBuildEvidence>,
    ) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            trust_level,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes,
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence,
        })
    }

    // ── verified_package_evidence_accepts_complete_evidence ───────────────
    // Spec scenario: "Verified package with complete reproducible evidence passes"
    //   GIVEN a Verified manifest with matching report, artifacts, and evidence
    //   WHEN validate_verified_package_evidence is called
    //   THEN it returns Ok(())
    #[test]
    fn verified_package_evidence_accepts_complete_evidence() {
        let hash = test_hash();
        let evidence = sample_evidence();
        let manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![ArtifactHashEntry {
                role: "wasm-binary".to_string(),
                hash: hash.clone(),
            }],
            Some(PackageVerificationReport {
                package: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec![],
                effects_declared: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![hash],
            }),
            Some(evidence),
        );

        assert_eq!(validate_verified_package_evidence(&manifest), Ok(()));
    }

    // ── verified_package_evidence_rejects_missing_report ─────────────────
    #[test]
    fn verified_package_evidence_rejects_missing_report() {
        let manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![ArtifactHashEntry {
                role: "wasm-binary".to_string(),
                hash: test_hash(),
            }],
            None,
            Some(sample_evidence()),
        );

        assert_eq!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::MissingReport)
        );
    }

    // ── verified_package_evidence_rejects_name_version_and_hash_mismatch ─
    #[test]
    fn verified_package_evidence_rejects_name_version_and_hash_mismatch() {
        let hash = test_hash();
        let evidence = sample_evidence();
        let mut manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![ArtifactHashEntry {
                role: "wasm-binary".to_string(),
                hash: hash.clone(),
            }],
            Some(PackageVerificationReport {
                package: "other.package".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec![],
                effects_declared: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![hash.clone()],
            }),
            Some(evidence),
        );

        assert!(matches!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::PackageMismatch { .. })
        ));

        manifest.verification_report.as_mut().unwrap().package = "payments.stripe".to_string();
        manifest.verification_report.as_mut().unwrap().version = "2.0.0".to_string();
        assert!(matches!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::VersionMismatch { .. })
        ));

        manifest.verification_report.as_mut().unwrap().version = "1.2.0".to_string();
        manifest
            .verification_report
            .as_mut()
            .unwrap()
            .artifact_hashes = vec!["b".repeat(64)];
        assert!(matches!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::ArtifactHashesMismatch { .. })
        ));
    }

    // ── verified_package_evidence_rejects_empty_manifest_artifacts ────────
    #[test]
    fn verified_package_evidence_rejects_empty_manifest_artifacts() {
        let manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![],
            Some(PackageVerificationReport {
                package: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec![],
                effects_declared: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![],
            }),
            Some(sample_evidence()),
        );

        assert_eq!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::ManifestArtifactHashesMissing)
        );
    }

    // ── verified_package_evidence_ignores_lower_trust_tiers ───────────────
    // Spec scenario: "Legacy non-Verified behavior preserved"
    //   GIVEN a manifest with TrustLevel::Assumed and no evidence
    //   WHEN validate_verified_package_evidence is called
    //   THEN it returns Ok(()) (lower tiers are not checked)
    #[test]
    fn verified_package_evidence_ignores_lower_trust_tiers() {
        let manifest = package_with_evidence(TrustLevel::Assumed, vec![], None, None);
        assert_eq!(validate_verified_package_evidence(&manifest), Ok(()));
    }

    // ── 4G: verified_package_rejects_missing_reproducible_evidence ────────
    // Spec scenario: "Missing reproducible evidence fails for Verified packages"
    //   GIVEN a Verified manifest with matching report and artifacts but no evidence
    //   WHEN validate_verified_package_evidence is called
    //   THEN it returns Err(MissingReproducibleEvidence)
    #[test]
    fn verified_package_rejects_missing_reproducible_evidence() {
        let hash = test_hash();
        let manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![ArtifactHashEntry {
                role: "wasm-binary".to_string(),
                hash: hash.clone(),
            }],
            Some(PackageVerificationReport {
                package: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec![],
                effects_declared: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![hash],
            }),
            None, // no reproducible evidence
        );

        assert_eq!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::MissingReproducibleEvidence)
        );
    }

    // ── 4G: verified_package_rejects_invalid_evidence_hash_format ─────────
    // Spec scenario: "Mismatched/invalid evidence fails"
    //   GIVEN a Verified manifest with evidence containing an invalid hash field
    //   WHEN validate_verified_package_evidence is called
    //   THEN it returns Err(ReproducibleEvidenceInvalidFormat)
    #[test]
    fn verified_package_rejects_invalid_evidence_hash_format() {
        let hash = test_hash();
        // Too-short source_digest
        let bad_evidence = ReproducibleBuildEvidence {
            build_inputs_hash: "x".repeat(64),
            toolchain_id: "rustc-1.77.0".to_string(),
            source_digest: "tooshort".to_string(), // invalid: not 64 chars
            recipe_hash: "c".repeat(64),
        };
        let manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![ArtifactHashEntry {
                role: "wasm-binary".to_string(),
                hash: hash.clone(),
            }],
            Some(PackageVerificationReport {
                package: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec![],
                effects_declared: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![hash],
            }),
            Some(bad_evidence),
        );

        assert!(matches!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::ReproducibleEvidenceInvalidFormat { .. })
        ));
    }

    // ── 4G: verified_package_rejects_build_inputs_hash_mismatch ──────────
    // Spec scenario: "Mismatched build_inputs_hash fails"
    //   GIVEN a Verified manifest with evidence where build_inputs_hash is wrong
    //   WHEN validate_verified_package_evidence is called
    //   THEN it returns Err(ReproducibleEvidenceBuildInputsMismatch)
    #[test]
    fn verified_package_rejects_build_inputs_hash_mismatch() {
        let hash = test_hash();
        // Manually set a wrong build_inputs_hash
        let bad_evidence = ReproducibleBuildEvidence {
            build_inputs_hash: "e".repeat(64), // wrong: not derived from source+toolchain
            toolchain_id: "rustc-1.77.0".to_string(),
            source_digest: "b".repeat(64),
            recipe_hash: "c".repeat(64),
        };
        let manifest = package_with_evidence(
            TrustLevel::Verified,
            vec![ArtifactHashEntry {
                role: "wasm-binary".to_string(),
                hash: hash.clone(),
            }],
            Some(PackageVerificationReport {
                package: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec![],
                effects_declared: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![hash],
            }),
            Some(bad_evidence),
        );

        assert!(matches!(
            validate_verified_package_evidence(&manifest),
            Err(PackageVerificationEvidenceError::ReproducibleEvidenceBuildInputsMismatch { .. })
        ));
    }

    // ── 4G: evidence_blake3_hex_is_deterministic ──────────────────────────
    // Spec scenario: "Evidence hash deterministic"
    //   GIVEN two identical ReproducibleBuildEvidence values
    //   WHEN blake3_hex() is called on each
    //   THEN both return the same 64-char hex string
    #[test]
    fn evidence_blake3_hex_is_deterministic() {
        let e1 = sample_evidence();
        let e2 = sample_evidence();
        let h1 = e1.blake3_hex().expect("hash must succeed");
        let h2 = e2.blake3_hex().expect("hash must succeed");
        assert_eq!(h1.len(), 64, "evidence hash must be 64 chars");
        assert_eq!(h1, h2, "identical evidence must hash to same value");
    }

    // ── 4G: lower_trust_tiers_pass_without_evidence ───────────────────────
    // Spec scenario: "Legacy non-Verified behavior preserved"
    //   All non-Verified trust tiers pass even without evidence.
    #[test]
    fn lower_trust_tiers_pass_without_evidence() {
        for level in [
            TrustLevel::Unverified,
            TrustLevel::Unsafe,
            TrustLevel::Assumed,
        ] {
            let manifest = package_with_evidence(level, vec![], None, None);
            assert_eq!(
                validate_verified_package_evidence(&manifest),
                Ok(()),
                "trust level {level} must pass without evidence"
            );
        }
    }
}
