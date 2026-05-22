// ── ail-package::yank ─────────────────────────────────────────────────────
//
// Package yanking — recording that a specific package version should no
// longer be used for new dependency resolutions.
//
// # Design decisions
//
// - Yanked packages are NOT removed from the registry (old builds must remain
//   reproducible).  They are flagged via `YankRecord` entries.
// - `PackageRegistry` is extended with a `yanked: Vec<YankRecord>` field.
// - `is_yanked()` is a linear scan — acceptable for the expected registry size.

use serde::{Deserialize, Serialize};

// ── YankRecord ────────────────────────────────────────────────────────────

/// Records that a specific package version has been yanked.
///
/// Yanked packages remain in the registry for reproducibility of old builds,
/// but the dependency resolver will refuse to resolve them for new lockfiles
/// unless explicitly requested.
///
/// See `docs/packages.md` §Revocation and advisories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct YankRecord {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Pinned version that was yanked (e.g., `"1.2.0"`).
    pub version: String,
    /// Human-readable reason for yanking (e.g., `"Critical security regression"`).
    pub reason: String,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yank() -> YankRecord {
        YankRecord {
            name: "payments.stripe".to_string(),
            version: "1.0.0".to_string(),
            reason: "Critical idempotency regression".to_string(),
        }
    }

    // ── RED: yank_record_cbor_round_trip ──────────────────────────────────
    // Spec: REQ-YANK-1 — YankRecord is CBOR-serializable
    //   GIVEN a YankRecord with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn yank_record_cbor_round_trip() {
        let original = sample_yank();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialize must succeed");
        let decoded: YankRecord =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialize must succeed");

        assert_eq!(decoded, original, "decoded YankRecord must equal original");
    }

    // ── RED: yank_record_cbor_is_deterministic ────────────────────────────
    // TRIANGULATE: same value encoded twice yields identical bytes
    #[test]
    fn yank_record_cbor_is_deterministic() {
        let record = sample_yank();
        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&record, &mut buf1).expect("first encode");
        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&record, &mut buf2).expect("second encode");
        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }
}
