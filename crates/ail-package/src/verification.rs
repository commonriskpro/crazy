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

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
