// ── ail-package::lockfile ─────────────────────────────────────────────────
//
// `LockfileEntry` — one resolved package in the dependency lock.
//
// # Determinism contract
//
// All fields use deterministic types (String, Vec<String>, Option<String>).
// CBOR serialization via `ciborium` is byte-deterministic for this layout.

use serde::{Deserialize, Serialize};

use crate::trust::TrustLevel;

// ── LockfileEntry ─────────────────────────────────────────────────────────

/// One resolved and pinned package in the workspace lock file.
///
/// A `LockfileEntry` records the exact version and content hash of a
/// resolved package, the trust level at lock time, an optional link to
/// the verification report that produced this lock entry, and the set of
/// assumption IDs that were accepted by the approver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockfileEntry {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Pinned semantic version string (e.g., `"2.3.1"`).
    pub version: String,
    /// BLAKE3 hex digest of the package artifact at lock time.
    pub package_hash: String,
    /// Trust level recorded at lock time.
    pub trust_level: TrustLevel,
    /// Optional BLAKE3 hex digest of the verification report used to
    /// produce this lock entry.
    pub verification_report_hash: Option<String>,
    /// Assumption IDs accepted by the approver at lock time, in declaration order.
    ///
    /// Uses `Vec` (not `HashSet`) to maintain CBOR determinism.
    pub accepted_assumptions: Vec<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> LockfileEntry {
        LockfileEntry {
            name: "payments.stripe".to_string(),
            version: "2.3.1".to_string(),
            package_hash: "a".repeat(64),
            trust_level: TrustLevel::Assumed,
            verification_report_hash: Some("b".repeat(64)),
            accepted_assumptions: vec!["assume-pci".to_string(), "assume-gdpr".to_string()],
        }
    }

    // ── lockfile_entry_cbor_round_trip ────────────────────────────────────
    // Spec scenario: "CBOR round-trip preserves all fields"
    //   GIVEN a LockfileEntry with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn lockfile_entry_cbor_round_trip() {
        let original = sample_entry();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf)
            .expect("CBOR serialization must succeed");

        let decoded: LockfileEntry =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original, "decoded entry must equal the original");
    }

    // ── lockfile_entry_cbor_is_deterministic ──────────────────────────────
    // TRIANGULATE: encoding the same value twice produces identical bytes.
    #[test]
    fn lockfile_entry_cbor_is_deterministic() {
        let entry = sample_entry();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf2).expect("second encode");

        assert_eq!(buf1, buf2, "identical inputs must produce identical CBOR bytes");
    }

    // ── lockfile_entry_without_report_hash ────────────────────────────────
    // TRIANGULATE: None verification_report_hash survives round-trip.
    #[test]
    fn lockfile_entry_without_report_hash() {
        let entry = LockfileEntry {
            verification_report_hash: None,
            accepted_assumptions: vec![],
            ..sample_entry()
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf).expect("encode");
        let decoded: LockfileEntry =
            ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert_eq!(decoded.verification_report_hash, None);
        assert!(decoded.accepted_assumptions.is_empty());
    }
}
